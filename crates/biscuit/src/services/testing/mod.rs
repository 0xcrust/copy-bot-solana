use crate::core::traits::txn_extensions::TransactionMods;
use crate::core::types::wire_transaction::ConfirmTransactionResponse;
use crate::core::JoinHandleResult;
use crate::swap::execution::{
    PriorityFeeConfig, SwapConfigOverrides, SwapExecutionMode, SwapExecutor, SwapInput,
};
use crate::swap::jupiter::executor::JupiterExecutor;
use crate::utils::fees::PriorityDetails;
use crate::wire::helius_rpc::{HeliusPrioFeesHandle, PriorityLevel};
use crate::wire::jito::{JitoClient, JitoTips};
use crate::wire::send_txn::service::SendTransactionService;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use log::{error, info};
use rand::Rng;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::hash::Hash;
use solana_sdk::message::{v0::Message, VersionedMessage};
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::system_instruction;
use solana_sdk::transaction::VersionedTransaction;
use solana_sdk::{pubkey, pubkey::Pubkey};
use tokio::sync::RwLock;

const DEFAULT_DELAY_BETWEEN_TRANSACTIONS_MS: u64 = 1; // 1ms
const SWAP_ACCOUNTS: [Pubkey; 1] = [pubkey!("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4")];

#[derive(Clone)]
pub struct JitoTestConfig {
    pub tips: Arc<RwLock<JitoTips>>,
    pub max_tip_lamports: u64,
}

pub fn send_multiple_swap_transactions_concurrent(
    rpc_client: Arc<RpcClient>,
    keypair: Arc<Keypair>,
    quote_api_url: String,
    pairs: Vec<(Pubkey, Pubkey, u64)>,
    max_iterations: usize,
    delay_between_transactions_ms: Option<u64>,
    slippage_bps: Option<u16>,
    txn_service: SendTransactionService,
    exit_signal: Arc<AtomicBool>,
    helius_priofees_handle: &HeliusPrioFeesHandle,
    jito_config: Option<JitoTestConfig>,
) -> Vec<JoinHandleResult<anyhow::Error>> {
    info!("Sending swap transactions. Pubkey={}", keypair.pubkey());
    let (transactions_handle, confirmation_receiver) = txn_service.subscribe();
    let mut tasks = vec![];

    let sent_txns = Arc::new(AtomicUsize::new(0));
    let confirmed_txns = Arc::new(AtomicUsize::new(0));
    let unconfirmed_txns = Arc::new(AtomicUsize::new(0));
    let helius_priofees = helius_priofees_handle.subscribe();
    helius_priofees.update_accounts(SWAP_ACCOUNTS.into_iter().map(|a| a.to_string()).collect());
    let executor = Arc::new(JupiterExecutor::new(
        Some(quote_api_url),
        Arc::clone(&rpc_client),
    ));

    for (input_token, output_token, input_amount) in pairs {
        let range_start = (input_amount * 9) / 10;
        let input_amount = rand::thread_rng().gen_range(range_start..input_amount);
        let helius_priofees = helius_priofees.clone();
        let executor = Arc::clone(&executor);

        let task = tokio::task::spawn({
            let send_transactions_handle = transactions_handle.clone();
            let keypair = Arc::clone(&keypair);
            let rpc_client = Arc::clone(&rpc_client);
            let exit_signal = Arc::clone(&exit_signal);
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(
                delay_between_transactions_ms.unwrap_or(DEFAULT_DELAY_BETWEEN_TRANSACTIONS_MS),
            ));
            let sent_txns = Arc::clone(&sent_txns);
            let helius_priofees = helius_priofees.clone();
            let executor = Arc::clone(&executor);
            let jito_config = jito_config.clone();

            async move {
                let mut unique_signatures_sent = std::collections::HashMap::new();
                for i in 0..max_iterations {
                    if exit_signal.load(Ordering::Relaxed) || i == max_iterations {
                        exit_signal.store(true, Ordering::Relaxed);
                        break;
                    }

                    let swap_input = SwapInput {
                        input_token_mint: input_token,
                        output_token_mint: output_token,
                        amount: input_amount,
                        slippage_bps: slippage_bps.unwrap_or(1000),
                        mode: SwapExecutionMode::ExactIn,
                        market: None,
                    };
                    let quote = executor.quote(&swap_input, None).await?;
                    let swap_overrides = if jito_config.is_some() {
                        let config = jito_config.as_ref().unwrap();
                        let tips = config.tips.read().await;
                        let tip = config.max_tip_lamports.min(30000.max(tips.p50() + 1));
                        Some(SwapConfigOverrides {
                            priority_fee: Some(PriorityFeeConfig::JitoTip(tip)),
                            ..Default::default()
                        })
                    } else {
                        None
                    };
                    let swap_transaction = executor
                        .swap_transaction(keypair.pubkey(), quote, swap_overrides.as_ref())
                        .await?;
                    let mut transaction = swap_transaction.transaction.into_versioned_tx()?;
                    let helius_cu_micro_lamports =
                        helius_priofees.get_priority_fee(PriorityLevel::Medium);
                    if let Some(updated_cu_price) = helius_cu_micro_lamports {
                        //info!("helius cu-price: {}", updated_cu_price);
                        transaction
                            .message
                            .modify_compute_unit_price(updated_cu_price as u64);
                    }
                    transaction.message.modify_compute_unit_price(200_000);
                    let _priority_details =
                        PriorityDetails::from_versioned_transaction(&transaction);
                    match send_transactions_handle
                        .send_transaction(transaction, &[&keypair], jito_config.is_some())
                        .await
                    {
                        Err(e) => error!("Failed sending transaction to transaction task: {}", e),
                        Ok(details) => {
                            if unique_signatures_sent
                                .insert(details.signature, ())
                                .is_none()
                            {
                                sent_txns.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                    interval.tick().await;
                }

                info!("Exiting testing task");
                Ok::<_, anyhow::Error>(())
            }
        });
        tasks.push(task)
    }

    let confirmed_transactions_task = {
        let mut confirmed_txns_receiver = confirmation_receiver;
        let mut log_tick = tokio::time::interval(std::time::Duration::from_secs(10));
        tokio::task::spawn({
            async move {
                while !exit_signal.load(Ordering::Relaxed) {
                    tokio::select! {
                        Some(item) = confirmed_txns_receiver.recv() => {
                            match item.response {
                                ConfirmTransactionResponse::Confirmed {
                                    error,
                                    confirmation_slot,
                                } => {
                                    confirmed_txns.fetch_add(1, Ordering::Relaxed);
                                }
                                ConfirmTransactionResponse::Unconfirmed { reason } => {
                                    unconfirmed_txns.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                        _ = log_tick.tick() => {
                            info!(
                                "[swap-test txns]: sent={},confirmed={},unconfirmed={}",
                                sent_txns.load(Ordering::Relaxed),
                                confirmed_txns.load(Ordering::Relaxed),
                                unconfirmed_txns.load(Ordering::Relaxed)
                            );
                        }
                    }
                }
                Ok::<_, anyhow::Error>(())
            }
        })
    };
    tasks.push(confirmed_transactions_task);

    tasks
}

pub fn send_multiple_swap_transactions(
    rpc_client: Arc<RpcClient>,
    keypair: Arc<Keypair>,
    quote_api_url: String,
    pairs: Vec<(Pubkey, Pubkey, u64)>,
    delay_between_transactions_ms: Option<u64>,
    slippage_bps: Option<u16>,
    txn_service: SendTransactionService,
    exit_signal: Arc<AtomicBool>,
    helius_priofees_handle: &HeliusPrioFeesHandle,
    jito_config: Option<JitoTestConfig>,
) -> Vec<JoinHandleResult<anyhow::Error>> {
    info!("Sending swap transactions. Pubkey={}", keypair.pubkey());
    let (transactions_handle, confirmation_receiver) = txn_service.subscribe();
    let mut tasks = vec![];

    let sent_txns = Arc::new(AtomicUsize::new(0));
    let confirmed_txns = Arc::new(AtomicUsize::new(0));
    let unconfirmed_txns = Arc::new(AtomicUsize::new(0));
    let helius_priofees = helius_priofees_handle.subscribe();
    helius_priofees.update_accounts(SWAP_ACCOUNTS.into_iter().map(|a| a.to_string()).collect());
    let executor = JupiterExecutor::new(Some(quote_api_url), Arc::clone(&rpc_client));
    let sleep_duration = std::time::Duration::from_millis(
        delay_between_transactions_ms.unwrap_or(DEFAULT_DELAY_BETWEEN_TRANSACTIONS_MS),
    );

    let main_task = {
        let sent_txns = Arc::clone(&sent_txns);
        tokio::task::spawn(async move {
            for (input_token, output_token, input_amount) in pairs {
                let range_start = (input_amount * 9) / 10;
                let input_amount = rand::thread_rng().gen_range(range_start..input_amount);
                let helius_priofees = helius_priofees.clone();

                let mut unique_signatures_sent = std::collections::HashMap::new();

                let swap_input = SwapInput {
                    input_token_mint: input_token,
                    output_token_mint: output_token,
                    amount: input_amount,
                    slippage_bps: slippage_bps.unwrap_or(1000),
                    mode: SwapExecutionMode::ExactIn,
                    market: None,
                };
                let quote = executor.quote(&swap_input, None).await?;
                let swap_overrides = if jito_config.is_some() {
                    let config = jito_config.as_ref().unwrap();
                    let tips = config.tips.read().await;
                    let tip = config.max_tip_lamports.min(200000.max(tips.p50() + 1));
                    Some(SwapConfigOverrides {
                        priority_fee: Some(PriorityFeeConfig::JitoTip(tip)),
                        ..Default::default()
                    })
                } else {
                    None
                };
                let swap_transaction = executor
                    .swap_transaction(keypair.pubkey(), quote, swap_overrides.as_ref())
                    .await?;
                let mut transaction = swap_transaction.transaction.into_versioned_tx()?;
                let helius_cu_micro_lamports =
                    helius_priofees.get_priority_fee(PriorityLevel::Medium);
                if let Some(updated_cu_price) = helius_cu_micro_lamports {
                    //info!("helius cu-price: {}", updated_cu_price);
                    transaction
                        .message
                        .modify_compute_unit_price(updated_cu_price as u64);
                }
                transaction.message.modify_compute_unit_price(200_000);
                let _priority_details = PriorityDetails::from_versioned_transaction(&transaction);
                match transactions_handle
                    .send_transaction(transaction, &[&keypair], jito_config.is_some())
                    .await
                {
                    Err(e) => error!("Failed sending transaction to transaction task: {}", e),
                    Ok(details) => {
                        if unique_signatures_sent
                            .insert(details.signature, ())
                            .is_none()
                        {
                            sent_txns.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                tokio::time::sleep(sleep_duration).await;
            }
            Ok::<_, anyhow::Error>(())
        })
    };

    let confirmed_transactions_task = {
        let mut confirmed_txns_receiver = confirmation_receiver;
        let mut log_tick = tokio::time::interval(std::time::Duration::from_secs(10));
        tokio::task::spawn({
            async move {
                while !exit_signal.load(Ordering::Relaxed) {
                    tokio::select! {
                        Some(item) = confirmed_txns_receiver.recv() => {
                            match item.response {
                                ConfirmTransactionResponse::Confirmed {
                                    error,
                                    confirmation_slot,
                                } => {
                                    confirmed_txns.fetch_add(1, Ordering::Relaxed);
                                }
                                ConfirmTransactionResponse::Unconfirmed { reason } => {
                                    unconfirmed_txns.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                        _ = log_tick.tick() => {
                            info!(
                                "[swap-test txns]: sent={},confirmed={},unconfirmed={}",
                                sent_txns.load(Ordering::Relaxed),
                                confirmed_txns.load(Ordering::Relaxed),
                                unconfirmed_txns.load(Ordering::Relaxed)
                            );
                        }
                    }
                }
                Ok::<_, anyhow::Error>(())
            }
        })
    };

    tasks.push(confirmed_transactions_task);

    tasks
}

const DEFAULT_INTERVAL_BETWEEN_TXNS: Duration = Duration::from_secs(2);
/// For sanity, interval between transactions should be set to a value
/// such that the blockhash is different for each generated transaction.
///
/// Otherwise, all generated transactions will have the same signature, meaning
/// they're essentially known to the network as a single transaction.
///
/// The default if `interval_between_txns` is not set is 2 seconds.
pub fn ping_pong_sol_transfer(
    rpc_client: Arc<RpcClient>,
    keypair_a: Arc<Keypair>,
    keypair_b: Arc<Keypair>,
    amount: u64,
    txn_service: SendTransactionService,
    max_iterations: usize,
    exit_signal: Arc<AtomicBool>,
    interval_between_txns: Option<Duration>,
    jito_config: Option<JitoTestConfig>,
) -> Vec<JoinHandleResult<anyhow::Error>> {
    let (send_transactions_handle, confirmation_receiver) = txn_service.subscribe();
    let sent_txns = Arc::new(AtomicUsize::new(0));
    let confirmed_txns = Arc::new(AtomicUsize::new(0));
    let unconfirmed_txns = Arc::new(AtomicUsize::new(0));
    let interval_between_txns = interval_between_txns.unwrap_or(DEFAULT_INTERVAL_BETWEEN_TXNS);

    info!(
        "ping-pong sol: a={},b={}",
        keypair_a.pubkey(),
        keypair_b.pubkey()
    );
    let mut tasks = vec![];

    let a_task = tokio::task::spawn({
        let keypair_a = Arc::clone(&keypair_a);
        let pubkey_a = keypair_a.pubkey();
        let pubkey_b = keypair_b.pubkey();
        let send_transactions_handle = send_transactions_handle.clone();
        let sent_txns = Arc::clone(&sent_txns);
        let exit_signal = Arc::clone(&exit_signal);
        let jito_config = jito_config.clone();
        async move {
            let mut unique_signatures_sent = std::collections::HashMap::new();
            for i in 0..max_iterations {
                if exit_signal.load(Ordering::Relaxed) || i == max_iterations {
                    exit_signal.store(true, Ordering::Relaxed);
                    break;
                }

                let transfer_ix = system_instruction::transfer(&pubkey_a, &pubkey_b, amount);
                let instructions = if jito_config.is_some() {
                    let config = jito_config.as_ref().unwrap();
                    let tips = config.tips.read().await;
                    let tip = config.max_tip_lamports.min(30000.max(tips.p50() + 1));
                    vec![JitoClient::build_bribe_ix(&pubkey_a, tip), transfer_ix]
                } else {
                    vec![transfer_ix]
                };

                let message =
                    Message::try_compile(&pubkey_a, &instructions, &[], Hash::default()).unwrap();
                let versioned_message = VersionedMessage::V0(message);
                let tx = VersionedTransaction::try_new(versioned_message, &[&keypair_a]).unwrap();
                let _priority_fee = PriorityDetails::from_versioned_transaction(&tx);
                match send_transactions_handle
                    .send_transaction(tx, &[&keypair_a], jito_config.is_some())
                    .await
                {
                    Err(e) => error!("Failed sending transaction to transaction task: {}", e),
                    Ok(details) => {
                        if unique_signatures_sent
                            .insert(details.signature, ())
                            .is_none()
                        {
                            sent_txns.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                tokio::time::sleep(interval_between_txns).await;
            }
            Ok(())
        }
    });
    tasks.push(a_task);

    let b_task = tokio::task::spawn({
        let pubkey_a = keypair_a.pubkey();
        let pubkey_b = keypair_b.pubkey();
        let send_transactions_handle = send_transactions_handle.clone();
        let sent_txns = Arc::clone(&sent_txns);
        let exit_signal = Arc::clone(&exit_signal);
        let jito_config = jito_config.clone();
        async move {
            let mut unique_signatures_sent = std::collections::HashMap::new();
            for i in 0..max_iterations {
                if exit_signal.load(Ordering::Relaxed) || i == max_iterations {
                    exit_signal.store(true, Ordering::Relaxed);
                    break;
                }

                let transfer_ix = system_instruction::transfer(&pubkey_b, &pubkey_a, amount);
                let instructions = if jito_config.is_some() {
                    let config = jito_config.as_ref().unwrap();
                    let tips = config.tips.read().await;
                    let tip = config.max_tip_lamports.min(30000.max(tips.p50() + 1));
                    vec![JitoClient::build_bribe_ix(&pubkey_b, tip), transfer_ix]
                } else {
                    vec![transfer_ix]
                };

                let message =
                    Message::try_compile(&pubkey_b, &instructions, &[], Hash::default()).unwrap();
                let versioned_message = VersionedMessage::V0(message);
                let tx = VersionedTransaction::try_new(versioned_message, &[&keypair_b]).unwrap();
                let priority_fee = PriorityDetails::from_versioned_transaction(&tx);
                // info!(
                //     "cu-limit={},cu-price={},priority-fee={}",
                //     priority_fee.compute_unit_limit,
                //     priority_fee.compute_unit_price,
                //     priority_fee.priority_fee
                // );

                match send_transactions_handle
                    .send_transaction(tx, &[&keypair_b], jito_config.is_some())
                    .await
                {
                    Err(e) => error!("Failed sending transaction to transaction task: {}", e),
                    Ok(details) => {
                        if unique_signatures_sent
                            .insert(details.signature, ())
                            .is_none()
                        {
                            sent_txns.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                tokio::time::sleep(interval_between_txns).await;
            }
            Ok(())
        }
    });
    tasks.push(b_task);

    let confirmed_transactions_task = {
        let mut confirmed_txns_receiver = confirmation_receiver;
        let mut log_tick = tokio::time::interval(std::time::Duration::from_secs(10));
        tokio::task::spawn({
            async move {
                while !exit_signal.load(Ordering::Relaxed) {
                    tokio::select! {
                        Some(item) = confirmed_txns_receiver.recv() => {
                            match item.response {
                                ConfirmTransactionResponse::Confirmed {
                                    error,
                                    confirmation_slot,
                                } => {
                                    // log::info!("Received confirmation response: txn confirmed!");
                                    confirmed_txns.fetch_add(1, Ordering::Relaxed);
                                }
                                ConfirmTransactionResponse::Unconfirmed { reason } => {
                                    // log::info!("Received confirmation response: txn not confirmed!");
                                    unconfirmed_txns.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                        _ = log_tick.tick() => {
                            info!(
                                "[sol ping-pong txn stats]: sent={},confirmed={},unconfirmed={}",
                                sent_txns.load(Ordering::Relaxed),
                                confirmed_txns.load(Ordering::Relaxed),
                                unconfirmed_txns.load(Ordering::Relaxed)
                            );
                        }
                    }
                }
                Ok::<_, anyhow::Error>(())
            }
        })
    };
    tasks.push(confirmed_transactions_task);

    tasks
}
