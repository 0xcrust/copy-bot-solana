use super::super::config::{CopyConfig, CopyWallet};
use super::super::history::{get_token_prices, CopyTrade, HistoricalTrades, TradeDirection};
use super::super::storage::CopyStorage;
use super::dust::DustTracker;
use super::execution::{CopyType, ExecutionCommand, ExecutionHandler, ExecutionOutput};
use crate::constants::mints::{SOL, USDC, USDT};
use crate::constants::programs::{
    JUPITER_AGGREGATOR_V6_PROGRAM_ID, ORCA_WHIRLPOOLS_PROGRAM_ID, PUMPFUN_PROGRAM_ID,
    RAYDIUM_CONCENTRATED_LIQUIDITY_PROGRAM_ID, RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID,
};
use crate::core::traits::{PollRequest, StreamInterface};
use crate::core::types::wire_transaction::{
    ConfirmTransactionData, ConfirmTransactionResponse, TransactionId,
};
use crate::core::JoinHandleResult;
use crate::parser::swap::ResolvedSwap;
use crate::parser::v1::V1Parser;
use crate::services::copy::config::CopyMode;
use crate::services::copy::v2::ping::PingTracker;
use crate::services::ITransaction;
use crate::swap::execution::SwapQuote;
use crate::swap::jupiter::executor::JupiterExecutor;
use crate::swap::jupiter::price::PriceFeed as JupiterPriceFeed;
use crate::wire::helius_rpc::{HeliusPrioFeesHandle, PriorityLevel};
use crate::wire::jito::JitoTips;
use crate::wire::send_txn::service::{SendTransactionService, SendTransactionsHandle};

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Error};
use dashmap::{DashMap, DashSet};
use futures::StreamExt;
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::signature::{Keypair, Signature};
use solana_sdk::signer::Signer;
use solana_sdk::transaction::VersionedTransaction;
use solana_sdk::{pubkey, pubkey::Pubkey};
use solana_transaction_status::{UiTransactionEncoding, UiTransactionTokenBalance};
use tokio::sync::mpsc::{channel, unbounded_channel, Sender, UnboundedSender};
use tokio::sync::RwLock;

const SWAP_PROGRAMS: [Pubkey; 5] = [
    JUPITER_AGGREGATOR_V6_PROGRAM_ID,
    RAYDIUM_CONCENTRATED_LIQUIDITY_PROGRAM_ID,
    RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID,
    ORCA_WHIRLPOOLS_PROGRAM_ID,
    PUMPFUN_PROGRAM_ID,
];
pub const STATIC_TOKENS: [Pubkey; 3] = [SOL, USDC, USDT];

const PRIOFEE_ACCOUNTS: [Pubkey; 1] = [pubkey!("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4")];

#[derive(Clone, Debug)]
pub struct CopyDetails {
    pub origin_signature: Signature,
    pub origin_signer: Pubkey,
    pub origin_swap: ResolvedSwap,
    pub origin_sol_balance: Option<Balance>,
    pub origin_input_mint_balance: Option<Balance>,
    pub origin_output_mint_balance: Option<Balance>,
    pub send_txn_retries: u8,
    pub confirmed_error_retries: u8,
    pub retry_strategy: RetryStrategy,
    pub wallet_config: WalletConfig,
    pub next_priority_level: PriorityLevel,
    pub last_fee_lamports: u64,
    pub send_to_jito: bool,
    pub quote_output: Option<QuoteDetails>,
    pub copy_type: Option<CopyType>,
}
impl CopyDetails {
    pub fn is_first_execution(&self) -> bool {
        self.send_txn_retries == 0 && self.confirmed_error_retries == 0
    }
}

#[derive(Clone, Debug)]
pub struct QuoteDetails {
    input_token_decimals: Option<u8>,
    output_token_decimals: Option<u8>,
    input_mint: Pubkey,
    output_mint: Pubkey,
    amount_specified_is_input: bool,
    amount: u64,
    other_amount: u64,
    other_amount_threshold: u64,
}
impl QuoteDetails {
    pub fn from_quote_response<Q: SwapQuote>(q: Q) -> Self {
        QuoteDetails {
            input_token_decimals: q.input_token_decimals(),
            output_token_decimals: q.output_token_decimals(),
            input_mint: q.input_token(),
            output_mint: q.output_token(),
            amount_specified_is_input: q.amount_specified_is_input(),
            amount: q.amount(),
            other_amount: q.other_amount(),
            other_amount_threshold: q.other_amount_threshold(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum RetryStrategy {
    BuildTransaction,
    SendRaw(VersionedTransaction),
}

#[derive(Clone, Debug)]
pub struct WalletConfig {
    pub display: String,
    pub min_input_amount: u64,
    pub max_input_amount: u64,
    pub mode: CopyMode,
}

pub struct CopyService {
    /// The configuration for the copy service
    pub config: Arc<CopyConfig>,

    /// Rpc client connector
    pub rpc_client: Arc<RpcClient>,

    /// Keypair for sending transactions
    pub keypair: Arc<Keypair>,

    /// Copy transactions pending execution
    pub pending_transactions: PendingTransactionsMap,

    /// Handle for sending copy details to the swap executor task
    pub execution_sender: Sender<ExecutionCommand>,

    /// Per-account details for a copied wallet
    pub wallet_config: Arc<DashMap<Pubkey, WalletConfig>>,

    /// The trade history for the copy service
    pub trade_history: HistoricalTrades,

    /// Tracker for ping copy txns
    pub ping_tracker: Option<PingTracker>,

    /// Handle for poll-watching transactions via RPC
    pub txns_stream: Arc<dyn StreamInterface<Item = anyhow::Result<ITransaction>>>,

    /// Handle for sending transactions to the network
    pub send_txns_handle: SendTransactionsHandle,

    /// Price feed from Jupiter's price API
    pub price_feed: JupiterPriceFeed,

    /// Send the copy-trade results to the post-processing task
    pub copy_output_sender: UnboundedSender<CopyTrade>,

    /// Copy metrics
    pub metrics: Arc<CopyMetrics>,

    /// Buys disabled
    pub buy_disabled: Arc<AtomicBool>,
    // The copy-wallet's balance
    // pub wallet_balance: Arc<AtomicU64>,
}

pub struct PendingTransactionsMap {
    m: Arc<DashMap<TransactionId, (CopyDetails, Instant)>>,
    b: Behaviour,
}

pub enum Behaviour {
    DropNWhenFull { capacity: usize, n: usize },
}
impl Behaviour {
    fn validate(&self) {
        match self {
            Behaviour::DropNWhenFull { capacity, n } => {
                if n > capacity {
                    panic!("Invalid `dropNWhenFull` behavour: n > capacity")
                }
            }
        }
    }
}

impl PendingTransactionsMap {
    pub fn new(capacity: usize, drop_size: usize) -> Self {
        let map = Self {
            m: Arc::new(DashMap::new()),
            b: Behaviour::DropNWhenFull {
                capacity,
                n: drop_size,
            },
        };
        map.b.validate();
        map
    }

    pub fn insert(&self, txn_id: TransactionId, details: CopyDetails) -> Option<CopyDetails> {
        match self.b {
            Behaviour::DropNWhenFull { capacity, n } => {
                if self.m.len() >= capacity {
                    let mut transactions = self
                        .m
                        .iter()
                        .map(|x| (*x.key(), x.value().1))
                        .collect::<Vec<_>>();
                    transactions.sort_by(|(_, a), (_, b)| a.elapsed().cmp(&b.elapsed()));
                    error!("Dropping {} items from pending-txns map", n);
                    for (id, _) in &transactions[(capacity - n)..] {
                        if let Some(v) = self.m.remove(id) {
                            error!("Shedding txn {} from pending map. Status is unknown", v.0);
                        }
                    }
                }
                self.m
                    .insert(txn_id, (details, Instant::now()))
                    .map(|v| v.0)
            }
        }
    }

    pub fn remove(&self, txn_id: &TransactionId) -> Option<(TransactionId, CopyDetails)> {
        self.m.remove(txn_id).map(|(k, (v, _))| (k, v))
    }
}

pub struct CopyHandle {
    pub commands_sender: UnboundedSender<CopyCommand>,
    pub tasks: Vec<JoinHandleResult<anyhow::Error>>,
}

pub async fn start_copy_service_v2(
    rpc_client: Arc<RpcClient>,
    keypair: Arc<Keypair>,
    txn_service: SendTransactionService,
    commitment: CommitmentConfig,
    config: CopyConfig,
    helius_priofees_handle: Option<&HeliusPrioFeesHandle>,
    load_accounts_path: String,
    txns_stream: Arc<dyn StreamInterface<Item = anyhow::Result<ITransaction>>>,
    jito_tips: Arc<RwLock<JitoTips>>,
    bootstrap_history: bool,
    database: Box<dyn CopyStorage + Send + Sync>,
) -> anyhow::Result<CopyHandle> {
    info!("Copy config: {:#?}", config);
    info!("Loading copy accounts from {}", load_accounts_path);
    let init_accounts =
        serde_json::from_str::<Vec<CopyWallet>>(&std::fs::read_to_string(&load_accounts_path)?)?;
    log::info!("Got {} copy wallets", init_accounts.len());
    let trade_history = if bootstrap_history {
        log::info!("bootstrapping copy-history..");
        database.bootstrap_trade_history().await?
    } else {
        HistoricalTrades::default()
    };
    log::info!("copy-history bootstrap successful");
    log::debug!("copy-history: {:#?}", trade_history);

    let start = std::time::Instant::now();
    let (price_feed_task, price_feed) = JupiterPriceFeed::start(
        config.price_feed_url.clone(),
        config.price_feed_refresh_frequency_ms,
        None,
    );
    let helius_priofees = helius_priofees_handle.map(|h| {
        let handle = h.subscribe();
        handle.update_accounts(
            PRIOFEE_ACCOUNTS
                .into_iter()
                .map(|p| p.to_string())
                .collect(),
        );
        handle
    });

    let (commands_sender, mut commands_receiver) = unbounded_channel::<CopyCommand>();
    let (execution_command_sender, mut execution_command_receiver) =
        channel(config.max_execution_queue_length);
    let (send_txns_handle, mut confirmation_receiver) = txn_service.subscribe();
    let (execution_output_sender, mut execution_output_receiver) = unbounded_channel();
    let (copy_output_sender, mut copy_output_receiver) = unbounded_channel::<CopyTrade>();

    let wallet_config = Arc::new(DashMap::new());
    let buy_disabled_wallets = add_accounts_to_wallet_config_and_stream(
        Arc::clone(&rpc_client),
        init_accounts,
        Arc::clone(&txns_stream),
        &config,
        &wallet_config,
    )
    .await?;
    let config = Arc::new(config);
    // let wallet_balance = Arc::new(AtomicU64::new(rpc_client.get_account(&keypair.pubkey()).await?.lamports));
    let buy_disabled = Arc::new(AtomicBool::new(false));
    let dust_tracker = config.dust_wallet.map(|wallet| {
        DustTracker::new(
            Arc::clone(&keypair),
            Arc::clone(&rpc_client),
            wallet,
            txn_service.subscribe().0,
        )
    });

    let ping_tracker = match &config.ping_config_path {
        Some(path) => Some(PingTracker::start(&path, bootstrap_history)?),
        None => None,
    };

    let service = CopyService {
        config: Arc::clone(&config),
        rpc_client: Arc::clone(&rpc_client),
        keypair: Arc::clone(&keypair),
        pending_transactions: PendingTransactionsMap::new(
            config.max_pending_queue_length,
            config.max_pending_queue_length / 5,
        ),
        wallet_config,
        execution_sender: execution_command_sender,
        trade_history: trade_history.clone(),
        ping_tracker: ping_tracker.clone(),
        send_txns_handle,
        txns_stream: txns_stream,
        copy_output_sender,
        price_feed: price_feed.clone(),
        metrics: Arc::new(CopyMetrics::default()),
        buy_disabled: Arc::clone(&buy_disabled),
    };
    let mut log_tick =
        tokio::time::interval(std::time::Duration::from_secs(config.log_interval_seconds));
    let swap_executor = JupiterExecutor::new(config.quote_api_url.clone(), Arc::clone(&rpc_client));

    let dust_interval = tokio::time::interval(Duration::from_secs(config.dust_cleanup_secs));
    let execution = ExecutionHandler {
        keypair,
        config,
        trade_history,
        rpc_client: Arc::clone(&rpc_client),
        priofees_handle: helius_priofees,
        execution_output_sender,
        price_feed: price_feed.clone(),
        ping_tracker,
        swap_executor,
        jito_tips,
        buy_disabled, // wallet_balance
        metrics: Arc::clone(&service.metrics),
        buy_disabled_wallets: Arc::new(buy_disabled_wallets),
    };

    let main_task = tokio::spawn({
        let dust = dust_tracker.clone();
        let keypair = Arc::clone(&service.keypair);
        let mut dust_interval = dust_interval;
        async move {
            price_feed.subscribe_token(&SOL, Some(Duration::MAX));
            price_feed.subscribe_token(&USDC, Some(Duration::MAX));
            price_feed.subscribe_token(&USDT, Some(Duration::MAX));
            price_feed.refresh().await?;

            let mut txn_stream = service.txns_stream.subscribe();
            loop {
                tokio::select! {
                    Some(cmd) = commands_receiver.recv() => service.handle_command(cmd).await,
                    Some(tx) = txn_stream.next() => service.handle_wire_txn(tx),
                    Some(execution_output) = execution_output_receiver.recv() => {
                        service.handle_execution_output(execution_output).await
                    }
                    Some(data) = confirmation_receiver.recv() => {
                        service.handle_confirmation(data).await
                    }
                    _ = log_tick.tick() => {
                        let total_sent = service.metrics.total_sent_txns.load(Ordering::Relaxed);
                        let total_confirmed = service.metrics.total_confirmed_txns.load(Ordering::Relaxed);
                        let confirmation_rate = (total_confirmed as f64 / total_sent as f64) * 100.0;
                        if cfg!(debug_assertions) {
                            info!(
                                "[copy txns]: tracked={},processed={},successful={},skipped={},sent={},confirmed={},confirmation-rate={:.2}%,elapsed={}",
                                service.metrics.tracked_txns.load(Ordering::Relaxed),
                                service.metrics.attempted_copy_txns.load(Ordering::Relaxed),
                                service.metrics.executed_copy_txns.load(Ordering::Relaxed),
                                service.metrics.skipped_copy_txns.load(Ordering::Relaxed),
                                total_sent,
                                total_confirmed,
                                confirmation_rate,
                                start.elapsed().as_secs()
                            );
                        } else {
                            info!(
                                "[copy txns]: tracked={},processed={},successful={},skipped={},confirmation-rate={:.2}%,elapsed={}",
                                service.metrics.tracked_txns.load(Ordering::Relaxed),
                                service.metrics.attempted_copy_txns.load(Ordering::Relaxed),
                                service.metrics.executed_copy_txns.load(Ordering::Relaxed),
                                service.metrics.skipped_copy_txns.load(Ordering::Relaxed),
                                confirmation_rate,
                                start.elapsed().as_secs()
                            );
                        }
                    }
                    _ = dust_interval.tick() => {
                        if let Some(ref tracker) = dust {
                            if let Err(e) = tracker.process_dust().await {
                                error!("Error processing dust: {}", e);
                            }
                        }
                    }
                }
            }

            #[allow(unreachable_code)]
            Ok::<_, anyhow::Error>(())
        }
    });

    let execution_task = tokio::spawn({
        async move {
            loop {
                match execution_command_receiver.recv().await {
                    None => {
                        error!("Channel closed. Exiting execution task");
                        break;
                    }
                    Some(cmd) => execution.handle_command(cmd),
                }
            }

            #[allow(unreachable_code)]
            Ok::<_, anyhow::Error>(())
        }
    });

    let database_task = tokio::spawn({
        let db = database;
        let dust = dust_tracker.clone();
        async move {
            while let Some(v) = copy_output_receiver.recv().await {
                if let Err(e) = db.process_copy_trade(&v).await {
                    error!(
                        "Failed to process copy-output for origin-tx={},copy-tx={}",
                        v.origin_signature, v.exec_signature
                    );
                }
                let dust_amount = v.volume.dust_token_amount;
                if let Some(ref tracker) = dust {
                    if dust_amount != 0 {
                        tracker
                            .dust_amounts
                            .write()
                            .await
                            .push((v.token, dust_amount));
                    }
                }
            }
            #[allow(unreachable_code)]
            Ok::<_, anyhow::Error>(())
        }
    });

    let tasks = vec![main_task, execution_task, price_feed_task];
    Ok(CopyHandle {
        commands_sender,
        tasks,
    })
}

impl CopyService {
    pub async fn handle_command(&self, cmd: CopyCommand) {
        match cmd {
            CopyCommand::CopyAccounts(accounts) => {
                if let Err(e) = add_accounts_to_wallet_config_and_stream(
                    Arc::clone(&self.rpc_client),
                    accounts,
                    Arc::clone(&self.txns_stream),
                    &self.config,
                    &self.wallet_config,
                )
                .await
                {
                    error!("Failed to add new accounts: {}", e);
                }
            }
        }
    }

    pub fn handle_wire_txn(&self, txn: Result<ITransaction, Error>) {
        tokio::task::spawn({
            let metrics = Arc::clone(&self.metrics);
            let wallet_config = Arc::clone(&self.wallet_config);
            let config = Arc::clone(&self.config);
            let execution_sender = self.execution_sender.clone();
            let rpc_client = Arc::clone(&self.rpc_client);
            async move {
                let mut transaction = match txn {
                    Err(e) => {
                        error!("Error from txn stream: {}", e);
                        return;
                    }
                    Ok(tx) => tx,
                };
                if let Err(e) = transaction
                    .extract_or_load_additional_keys(&rpc_client)
                    .await
                {
                    error!(
                        "Failed to load additional keys for origin tx {}: {}",
                        transaction.signature, e
                    );
                    return;
                }
                // Make sure we only process txns the copy service should be bothered with
                // Currently we do this by guaranteeing that this service doesn't share a txn stream
                // In the future we should do this by:
                //   * Adding filter combinators to the resulting stream
                //   * Letting the sending part of the txn stream recognize filters
                // Currently we rely on the fact that our txn stream is unique to this service

                // Skip failed transactions
                if let Some(ref err) = transaction.err {
                    debug!(
                        "Skipping failed origin tx {} with err: {:#?}",
                        transaction.signature, transaction.err
                    );
                    return;
                }

                let origin_signer = transaction
                    .message
                    .static_account_keys()
                    .get(0)
                    .expect("At least one signer present");
                let wallet_config = wallet_config.get(&origin_signer);
                if wallet_config.is_none() {
                    // The txn initiator is not a wallet we're tracking. We should skip
                    return;
                }
                log::debug!("[Copy] Detected tx {}", transaction.signature);
                let wallet_config = wallet_config.unwrap().clone();
                let origin_swap = match V1Parser::parse_swap_transaction(&transaction) {
                    Ok(swap) => swap,
                    Err(e) => {
                        error!(
                            "Error parsing swap transaction {}: {}",
                            transaction.signature, e
                        );
                        return;
                    }
                };

                let Some(origin_swap) = origin_swap else {
                    debug!(
                        "Failed to find and parse swap for txn {}, skipping",
                        transaction.signature
                    );
                    return;
                };

                info!("Found swap for txn {}", transaction.signature);
                debug!("Swap: {:#?}", origin_swap);

                let pending = CopyDetails {
                    origin_signature: transaction.signature,
                    origin_signer: *origin_signer,
                    origin_sol_balance: Balance::create_sol_balance(&transaction, 0),
                    origin_input_mint_balance: Balance::create_token_balance(
                        &transaction,
                        origin_signer,
                        &origin_swap.input_token_mint,
                    ),
                    origin_output_mint_balance: Balance::create_token_balance(
                        &transaction,
                        origin_signer,
                        &origin_swap.output_token_mint,
                    ),
                    origin_swap,
                    send_txn_retries: 0,
                    confirmed_error_retries: 0,
                    retry_strategy: RetryStrategy::BuildTransaction,
                    next_priority_level: config.priority_level_default,
                    send_to_jito: config.send_jito_default,
                    // send_to_jito: if staked_rpc_connection {
                    //     false
                    // } else {
                    //     config.send_jito_default
                    // },
                    last_fee_lamports: 0,
                    wallet_config,
                    quote_output: None,
                    copy_type: None,
                };
                metrics.attempted_copy_txns.fetch_add(1, Ordering::Relaxed);

                if let Err(e) = execution_sender.try_send(ExecutionCommand::Execute(pending))
                // [ExecQueue note]: If we use send.await() then we'd need to make sure backpressure is communicated to the txn stream
                //  so it doesn't overwhelm this task on resumption. For now, just drop the txn :(
                {
                    error!(
                        "Failed to send parsed swap details to swap processing task: {}",
                        e
                    );
                }
            }
        });
    }

    pub async fn handle_execution_output(&self, execution_output: ExecutionOutput) {
        match execution_output {
            ExecutionOutput::Success((details, transaction)) => {
                match self
                    .send_txns_handle
                    .send_transaction(transaction, &[&self.keypair], details.send_to_jito)
                    .await
                {
                    Err(e) => {
                        error!("Failed sending transaction to transaction task: {}", e)
                    }
                    Ok(res) => {
                        self.metrics.total_sent_txns.fetch_add(1, Ordering::Relaxed);
                        self.pending_transactions.insert(res.txn_id, details);
                    }
                }
            }
            ExecutionOutput::Failure(details) => self.handle_failed_execution(details).await,
        }
    }

    pub async fn handle_confirmation(&self, item: ConfirmTransactionData) {
        // TODO: Track confirmed transactions for their cu-limit to get a more appropriate value
        if let Some((txn_id, mut details)) = self.pending_transactions.remove(&item.data.txn_id) {
            let origin = details.origin_signature;
            match item.response {
                ConfirmTransactionResponse::Confirmed {
                    error,
                    confirmation_slot,
                } => {
                    // This tracks all confirmed txns, including failed ones:
                    self.metrics
                        .total_confirmed_txns
                        .fetch_add(1, Ordering::Relaxed);
                    match error {
                        Some(err) => {
                            // todo: Retry here is naive. Possible improvements are to increase slippage, etc
                            // Retries here should use a new transaction id(not `resend_transaction`)
                            debug!(
                                "Copy txn confirmed but failed, origin={}, error={}",
                                origin, err
                            );
                            if details.confirmed_error_retries < self.config.confirmed_error_retries
                            {
                                debug!("Queuing failed txn {} again", txn_id);
                                details.send_txn_retries = 0;
                                details.confirmed_error_retries += 1;
                                if let Err(e) = self
                                    .execution_sender
                                    .try_send(ExecutionCommand::Execute(details))
                                // [ExecQueue note]
                                {
                                    error!("Failed to queue failed txn {} for retry", txn_id);
                                }
                            } else {
                                error!(
                                    "Execution failed: Exceeded max error retries. origin={}",
                                    details.origin_signature
                                );
                                self.handle_failed_execution(details).await;
                            }
                        }
                        None => self.handle_success_txn(details, item.data.signature).await,
                    }
                }
                ConfirmTransactionResponse::Unconfirmed { reason } => {
                    log::info!(
                        "Failed to confirm copy txn={}, origin {}",
                        item.data.signature,
                        origin
                    );
                    // todo: Retry here is naive. Possible improvements are to increase prio-fees, send to
                    // more leaders, etc
                    if details.send_txn_retries < self.config.send_txn_retries {
                        details.send_txn_retries += 1;
                        // Always send to jito on the last pass
                        if details.send_txn_retries == self.config.send_txn_retries {
                            details.send_to_jito = true;
                        }
                        details.next_priority_level = match details.next_priority_level {
                            PriorityLevel::Medium => PriorityLevel::High,
                            PriorityLevel::High => PriorityLevel::VeryHigh,
                            PriorityLevel::VeryHigh => PriorityLevel::VeryHigh,
                            _ => PriorityLevel::High,
                        };

                        match details.retry_strategy {
                            RetryStrategy::BuildTransaction => {
                                debug!("Rebuilding swap for unconfirmed txn");
                                if let Err(e) = self
                                    .execution_sender
                                    .try_send(ExecutionCommand::Execute(details))
                                // [ExecQueue note]
                                {
                                    error!(
                                        "Failed to queue txn for retry. origin={},error={}",
                                        origin, e
                                    );
                                }
                            }
                            RetryStrategy::SendRaw(ref tx) => {
                                debug!("Resending unconfirmed txn");
                                let keypair = &[&self.keypair];
                                if let Err(e) = self
                                    .send_txns_handle
                                    .resend_transaction(
                                        tx.clone(),
                                        keypair,
                                        txn_id,
                                        details.send_to_jito,
                                    )
                                    .await
                                {
                                    error!(
                                        "Failed to send txn to send-txn task, origin={},error={}",
                                        origin, e
                                    );
                                } else {
                                    // Note: Since we already removed it above, important to insert it again
                                    self.pending_transactions.insert(txn_id, details);
                                }
                            }
                        }
                    } else {
                        self.metrics
                            .total_unconfirmed_txns
                            .fetch_add(1, Ordering::Relaxed);
                        // let quote_output =
                        //     details.quote_output.expect("execution output has been set");
                        let fee_name = if details.send_to_jito {
                            "jito-tip"
                        } else {
                            "priofee"
                        };
                        error!(
                            "Execution failed: Failed to confirm txn {} after {} retry(s). origin={},{}={}",
                            txn_id, details.send_txn_retries, details.origin_signature, fee_name, details.last_fee_lamports
                        );
                        self.handle_failed_execution(details).await;
                    }
                }
            }
        } else {
            error!(
                "failed to find details for txn {} in pending transactions map",
                item.data.txn_id
            )
        }
    }

    async fn handle_failed_execution(&self, details: CopyDetails) {
        if let Some(mut history_ref) = self.trade_history.get_history_for_wallet_and_token_mut(
            details.origin_signer,
            details.origin_swap.input_token_mint,
        ) {
            // If the wallet sold a token we copied it for, increase its sell-volume for that token.
            let (input_decimals, output_decimals) = match details
                .origin_swap
                .resolve_token_decimals(&self.rpc_client)
                .await
            {
                Ok(tup) => tup,
                Err(e) => {
                    log::error!(
                        "Failed to resolve token decimals while handling failed execution: {}",
                        e
                    );
                    return;
                }
            };

            let origin_price = get_token_prices(
                &details.origin_swap.input_token_mint,
                &details.origin_swap.output_token_mint,
                details.origin_swap.input_amount,
                details.origin_swap.output_amount,
                input_decimals,
                output_decimals,
                None,
                &self.price_feed,
            )
            .await
            .unwrap_or_default();

            let volume_sol = details.origin_swap.input_amount as f64 * origin_price.input_price_sol
                / 10_u32.pow(input_decimals as u32) as f64;
            let volume_usd = details.origin_swap.input_amount as f64 * origin_price.input_price_usd
                / 10_u32.pow(input_decimals as u32) as f64;

            history_ref.origin.sell_volume_token += details.origin_swap.input_amount;
            history_ref.origin.sell_volume_sol += volume_sol;
            history_ref.origin.sell_volume_usd += volume_usd;
        }
        // Note: Todo! We want to take into account missed sells
    }

    async fn handle_success_txn(&self, details: CopyDetails, signature: Signature) {
        // - If we have an entry for the input-token then we sold that token. Add sell volume
        // - If we have an entry for the output-token then we bought that token. Add buy volume
        self.metrics
            .executed_copy_txns
            .fetch_add(1, Ordering::Relaxed);
        if details.origin_swap.token_decimals.is_none() {
            error!(
                "Origin swap not fully resolved. Skipping history addition for copy txn: {}",
                signature
            );
            return;
        }
        let origin_mint_decimals = details.origin_swap.token_decimals.unwrap();
        let origin_amount_in = details.origin_swap.input_amount;

        let quote_output = details.quote_output.expect("execution output is not null");
        let (input_amount, output_amount) = if quote_output.amount_specified_is_input {
            (quote_output.amount, quote_output.other_amount)
        } else {
            (quote_output.other_amount, quote_output.amount)
        };

        let mut in_token_decimals = quote_output.input_token_decimals;
        let mut out_token_decimals = quote_output.output_token_decimals;

        let exec_input_mint = quote_output.input_mint;
        let exec_output_mint = quote_output.output_mint;
        let origin_input_mint = details.origin_swap.input_token_mint;
        let origin_output_mint = details.origin_swap.output_token_mint;

        if in_token_decimals.is_none() {
            // This is always a mint we know(if we're buying), or the input-mint of the wallet we're copying
            if exec_input_mint == SOL {
                in_token_decimals = Some(9)
            } else if exec_input_mint == USDC || exec_input_mint == USDT {
                in_token_decimals = Some(6)
            } else if exec_input_mint == origin_input_mint {
                in_token_decimals = Some(origin_mint_decimals.input)
            }
        }

        if out_token_decimals.is_none() {
            // This is always a mint we know(if we're selling), or the output-mint of the wallet we're copying
            if exec_output_mint == origin_output_mint {
                out_token_decimals = Some(origin_mint_decimals.output)
            } else if exec_output_mint == USDC || exec_output_mint == USDT {
                out_token_decimals = Some(6)
            } else if exec_output_mint == SOL {
                out_token_decimals = Some(9)
            }
        }

        if in_token_decimals.is_none() || out_token_decimals.is_none() {
            log::error!(
                "Couldn't get token decimals for swap. tx={}. Skipping history insertion",
                signature
            );
        }

        // note: Amount calculations here rely on the knowledge(or assumption) that quotes are `exact-in`
        let in_token_decimals = in_token_decimals.unwrap();
        let out_token_decimals = out_token_decimals.unwrap();
        let exec_price = get_token_prices(
            &quote_output.input_mint,
            &quote_output.output_mint,
            input_amount,
            output_amount,
            in_token_decimals,
            out_token_decimals,
            None,
            &self.price_feed,
        )
        .await
        .unwrap_or_default();
        debug!("exec price: {:#?}", exec_price);
        let origin_price = get_token_prices(
            &origin_input_mint,
            &origin_output_mint,
            origin_amount_in,
            details.origin_swap.output_amount,
            origin_mint_decimals.input,
            origin_mint_decimals.output,
            None,
            &self.price_feed,
        )
        .await
        .unwrap_or_default();
        debug!("origin price: {:#?}", origin_price);

        let copy_address = self.keypair.pubkey();
        let output_mint = quote_output.output_mint;
        let get_amount_out = || async {
            let txn = self
                .rpc_client
                .get_transaction_with_config(
                    &signature,
                    RpcTransactionConfig {
                        encoding: Some(UiTransactionEncoding::Base58),
                        commitment: Some(CommitmentConfig::confirmed()),
                        max_supported_transaction_version: Some(0),
                    },
                )
                .await?;
            let mut tx = ITransaction::try_from(txn)?;
            tx.load_keys(&self.rpc_client).await?;

            let post_lamports = tx
                .meta
                .as_ref()
                .and_then(|meta| Option::<Vec<u64>>::from(meta.post_balances.clone()))
                .context("failed to get post-balances from transaction meta")?
                .first()
                .copied()
                .unwrap_or(0);
            debug!("post-lamports={}", post_lamports);

            match V1Parser::parse_swap_transaction(&tx) {
                Ok(Some(swap)) => {
                    debug!("Exec swap: {:#?}", swap);
                    return Ok((swap.output_amount, post_lamports));
                }
                _ => {}
            };

            if output_mint == spl_token::native_mint::ID {
                let pre_lamports = tx
                    .meta
                    .as_ref()
                    .and_then(|meta| Option::<Vec<u64>>::from(meta.pre_balances.clone()))
                    .unwrap_or_default()
                    .first()
                    .copied()
                    .unwrap_or_default();

                // Could this be zero?
                let amount_out = post_lamports
                    .saturating_sub(pre_lamports.saturating_sub(details.last_fee_lamports));
                debug!(
                    "SOL output: pre-lamports={},last-fee-lamports={},amount-out={}",
                    pre_lamports, details.last_fee_lamports, amount_out
                );
                return Ok((amount_out, post_lamports));
            }

            let pre = tx
                .meta
                .as_ref()
                .and_then(|meta| {
                    Option::<Vec<UiTransactionTokenBalance>>::from(meta.pre_token_balances.clone())
                })
                .unwrap_or_default();
            let post = tx
                .meta
                .as_ref()
                .and_then(|meta| {
                    Option::<Vec<UiTransactionTokenBalance>>::from(meta.post_token_balances.clone())
                })
                .unwrap_or_default();

            let pre_balance = pre
                .iter()
                .find_map(
                    |balance| match Option::<String>::from(balance.owner.clone()) {
                        Some(owner)
                            if owner == copy_address.to_string()
                                && balance.mint == output_mint.to_string() =>
                        {
                            Some(balance.ui_token_amount.ui_amount?)
                        }
                        _ => None,
                    },
                )
                .unwrap_or_default(); // Our token-account might have not existed before the swap
            let (post_balance, decimals) = post
                .iter()
                .find_map(
                    |balance| match Option::<String>::from(balance.owner.clone()) {
                        Some(owner)
                            if owner == copy_address.to_string()
                                && balance.mint == output_mint.to_string() =>
                        {
                            Some((
                                balance.ui_token_amount.ui_amount?,
                                balance.ui_token_amount.decimals,
                            ))
                        }
                        _ => None,
                    },
                )
                .context(format!(
                    "Failed to get post balance for mint {} in txn {}",
                    output_mint, signature
                ))?; // At this point, our token-account must exist

            if post_balance <= pre_balance {
                return Err(anyhow!(
                    "txn {}: post-balance for output {} is <= than pre-balance {}",
                    signature,
                    post_balance,
                    pre_balance
                ));
            }

            let amount_out =
                ((post_balance - pre_balance) * 10u32.pow(decimals as u32) as f64).trunc() as u64;
            Ok::<_, anyhow::Error>((amount_out, post_lamports))
        };

        let (actual_amount_out, lamports_post_balance) = match get_amount_out().await {
            Ok((amount_out, lamports_post_balance)) => {
                // amount-out must be at least `other-amount-threshold`
                let amount_out = std::cmp::max(amount_out, quote_output.other_amount_threshold);
                (amount_out, Some(lamports_post_balance))
            }
            Err(e) => {
                error!("Error calculating output amount for swap: {}", e);
                (quote_output.other_amount_threshold, None)
            }
        };

        if let Some(post_balance) = lamports_post_balance {
            if post_balance <= self.config.disable_buy_lamports {
                info!(
                    "Disabling buys. Lamports={}. Disable-threshold={}",
                    post_balance, self.config.disable_buy_lamports
                );
                self.buy_disabled.store(true, Ordering::Relaxed);
            } else {
                if self.buy_disabled.load(Ordering::Relaxed) {
                    info!("Lamports replenished! Restoring buys..");
                    self.buy_disabled.store(false, Ordering::Relaxed);
                }
            }
        }

        let copy_type = details.copy_type.expect("Should have been set");
        // If both wallets sold:
        let exec_wallet = self.keypair.pubkey();
        if matches!(copy_type, CopyType::Sell | CopyType::Both) {
            debug_assert!(origin_input_mint == exec_input_mint);
            match details.wallet_config.mode {
                CopyMode::Direct => {
                    match self.trade_history.insert_trade(
                        &details.origin_signer,
                        &details.wallet_config.display,
                        &origin_input_mint,
                        in_token_decimals,
                        quote_output.amount, // we know that this is exact-in
                        origin_amount_in,
                        exec_price.input_price_usd,
                        exec_price.input_price_sol,
                        origin_price.input_price_usd,
                        origin_price.input_price_sol,
                        &TradeDirection::Sell,
                        Some(self.config.moonbag_bps),
                        &signature,
                    ) {
                        Err(e) => error!("Error inserting trade history: {}", e),
                        Ok(volume) => {
                            let trade = CopyTrade {
                                volume,
                                token: origin_input_mint,
                                origin_signature: details.origin_signature,
                                exec_signature: signature,
                                origin_wallet: details.origin_signer,
                                exec_wallet,
                                direction: TradeDirection::Sell,
                            };
                            if let Err(e) = self.copy_output_sender.send(trade) {
                                error!("Failed to send copy-trade item: {}", e)
                            }
                        }
                    }
                }
                CopyMode::Ping => {
                    let Some(ping_tracker) = self.ping_tracker.as_ref() else {
                        error!(
                            "Unreachable: Ping tracker should be set to handle successful ping txn"
                        );
                        return;
                    };
                    match ping_tracker.insert_execution(
                        &origin_input_mint,
                        in_token_decimals,
                        quote_output.amount,
                        exec_price.input_price_usd,
                        exec_price.input_price_sol,
                        &TradeDirection::Sell,
                        Some(self.config.moonbag_bps),
                    ) {
                        Ok(v) => {
                            let volume_token =
                                v.volume_token as f64 / 10_u32.pow(in_token_decimals as u32) as f64;
                            info!(
                                "[ping-swap]: Swapped {} of {} for {} of SOL({:.3} USD) in tx {}",
                                volume_token,
                                origin_input_mint,
                                v.volume_sol,
                                v.volume_usd,
                                signature
                            );
                            match v.ping {
                                Some(ping) => info!("Ping={:#?}", ping),
                                None => error!(
                                    "Trade executed but no pings found for token {}",
                                    origin_output_mint
                                ),
                            }
                        }
                        Err(e) => error!("Error inserting ping execution: {}", e),
                    }
                }
            }
        }

        // If both wallets bought:
        if matches!(copy_type, CopyType::Buy | CopyType::Both) {
            debug_assert!(origin_output_mint == exec_output_mint);
            match details.wallet_config.mode {
                CopyMode::Direct => {
                    match self.trade_history.insert_trade(
                        &details.origin_signer,
                        &details.wallet_config.display,
                        &origin_output_mint,
                        out_token_decimals,
                        actual_amount_out,
                        details.origin_swap.output_amount,
                        exec_price.output_price_usd,
                        exec_price.output_price_sol,
                        origin_price.output_price_usd,
                        origin_price.output_price_sol,
                        &TradeDirection::Buy,
                        Some(self.config.moonbag_bps),
                        &signature,
                    ) {
                        Ok(volume) => {
                            let trade = CopyTrade {
                                volume,
                                token: origin_output_mint,
                                origin_signature: details.origin_signature,
                                exec_signature: signature,
                                origin_wallet: details.origin_signer,
                                exec_wallet,
                                direction: TradeDirection::Buy,
                            };
                            if let Err(e) = self.copy_output_sender.send(trade) {
                                error!("Failed to send copy-trade item: {}", e)
                            }
                        }
                        Err(e) => error!("Error inserting trade history: {}", e),
                    }
                }
                CopyMode::Ping => {
                    let Some(ping_tracker) = self.ping_tracker.as_ref() else {
                        error!(
                            "Unreachable: Ping tracker should be set to handle successful ping txn"
                        );
                        return;
                    };
                    match ping_tracker.insert_execution(
                        &origin_output_mint,
                        out_token_decimals,
                        actual_amount_out,
                        exec_price.output_price_usd,
                        exec_price.output_price_sol,
                        &TradeDirection::Buy,
                        Some(self.config.moonbag_bps),
                    ) {
                        Ok(v) => {
                            let volume_token = v.volume_token as f64
                                / 10_u32.pow(out_token_decimals as u32) as f64;
                            info!(
                                "[ping-swap]: Swapped {} of SOL({:.3} USD) for {} of {} in tx {}",
                                v.volume_sol,
                                v.volume_usd,
                                volume_token,
                                origin_output_mint,
                                signature
                            );
                            match v.ping {
                                Some(ping) => info!("Ping={:#?}", ping),
                                None => error!(
                                    "Trade executed but no pings found for token {}",
                                    origin_output_mint
                                ),
                            }
                        }
                        Err(e) => error!("Error inserting ping execution: {}", e),
                    }
                }
            }
        }
    }
}

pub enum CopyCommand {
    CopyAccounts(Vec<CopyWallet>),
}

#[derive(Default)]
pub struct CopyMetrics {
    /// The total number of txns detected by the copy service
    pub tracked_txns: AtomicUsize,
    /// The total number of txns acted upon by the service
    pub attempted_copy_txns: AtomicUsize,
    /// The total number of successful txns executed by the service
    pub executed_copy_txns: AtomicUsize,
    /// The total number of txns ignored by the service
    pub skipped_copy_txns: AtomicUsize,
    /// Total number of txns sent by the service, including retries
    pub total_sent_txns: AtomicUsize,
    /// Total number of confirmations gotten by the service, including for retried txns
    pub total_confirmed_txns: AtomicUsize,
    /// Total number of non-confirmations gotten by the service
    pub total_unconfirmed_txns: AtomicUsize,
}

// - Red rising
// - James S.A Corey
// - The expanse series
// - Mercy of the Gods
// - Ty Franck
// - Daniel Abraham
// - Leviathan wakes
// - Bonehunters
// - Deadhouse gates
// - First law trilogy

fn is_static_token(token: &Pubkey) -> bool {
    *token == SOL || *token == USDT || *token == USDC
}

/// Returns (wallet, config, buy-disabled)
pub fn copy_accounts_to_config(
    global_config: &CopyConfig,
    accounts: &[CopyWallet],
) -> Vec<(Pubkey, WalletConfig, bool)> {
    let mut configs = Vec::new();
    for account in accounts {
        let min_input_amount = account
            .min_input_amount
            .unwrap_or(global_config.min_input_amount_default);
        let max_input_amount = account
            .max_input_amount
            .unwrap_or(global_config.max_input_amount_default);
        if max_input_amount <= min_input_amount {
            panic!(
                "Wallet {}. max-input {} is <= min-input {}",
                account.wallet, max_input_amount, min_input_amount
            );
        }

        let buy_disabled = account.buy_disabled.unwrap_or(false);

        configs.push((
            account.wallet,
            WalletConfig {
                display: account
                    .display_name
                    .clone()
                    .unwrap_or(account.wallet.to_string()),
                min_input_amount,
                max_input_amount,
                mode: account.mode.unwrap_or(global_config.copy_mode_default),
            },
            buy_disabled,
        ));
    }

    configs
}

// todo: We shouldn't add to the map if watching the accounts fail, but this is fine for now
async fn add_accounts_to_wallet_config_and_stream(
    rpc_client: Arc<RpcClient>,
    init_accounts: Vec<CopyWallet>,
    txns_stream: Arc<dyn StreamInterface<Item = anyhow::Result<ITransaction>>>,
    global_config: &CopyConfig,
    config_map: &DashMap<Pubkey, WalletConfig>,
) -> anyhow::Result<DashSet<Pubkey>> {
    let keys = init_accounts.iter().map(|w| w.wallet).collect::<Vec<_>>();
    let balances = rpc_client
        .get_multiple_accounts(&keys)
        .await?
        .into_iter()
        .map(|acc| acc.map(|a| a.lamports));

    let accounts = init_accounts
        .into_iter()
        .zip(balances)
        .into_iter()
        .filter_map(|(acc, balance)| {
            if balance.is_none() {
                log::error!("Wallet {} does not exist", acc.wallet);
            }
            Some(acc)
        })
        .collect::<Vec<_>>();

    let disabled_wallets = DashSet::new();
    let configs = copy_accounts_to_config(global_config, &accounts);
    for (wallet, config, is_disabled) in configs {
        config_map.insert(wallet, config);
        if is_disabled {
            disabled_wallets.insert(wallet);
        }
    }

    let stream_accounts = accounts
        .into_iter()
        .map(|wallet| PollRequest {
            address: wallet.wallet,
            commitment: wallet
                .commitment_level
                .map(|commitment| CommitmentConfig { commitment }),
            poll_transactions_frequency: wallet
                .poll_transactions_frequency_seconds
                .map(|s| Duration::from_secs(s)),
            // the following fields are currently ignored by the txn stream we use. They were added for the historical-txns
            // stream. todo: Make this trait cleaner
            ignore_error_transactions: wallet.ignore_error_transactions.unwrap_or(false),
            recent_signature: wallet.recent_signature,
            order_by_earliest: Some(false),
        })
        .collect();
    txns_stream.watch_accounts(stream_accounts)?;
    Ok(disabled_wallets)
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
pub struct Balance {
    pub pre: u64,
    pub post: u64,
}

impl Balance {
    pub fn create_sol_balance(tx: &ITransaction, origin_wallet_idx: usize) -> Option<Balance> {
        let meta = tx.meta.as_ref()?;
        Some(Balance {
            pre: *meta.pre_balances.get(origin_wallet_idx)?,
            post: *meta.post_balances.get(origin_wallet_idx)?,
        })
    }

    pub fn create_token_balance(
        tx: &ITransaction,
        wallet: &Pubkey,
        token: &Pubkey,
    ) -> Option<Balance> {
        let pre = tx.meta.as_ref().and_then(|meta| {
            Option::<Vec<UiTransactionTokenBalance>>::from(meta.pre_token_balances.clone())
        })?;
        let post = tx.meta.as_ref().and_then(|meta| {
            Option::<Vec<UiTransactionTokenBalance>>::from(meta.post_token_balances.clone())
        })?;

        let pre_balance =
            pre.iter().find_map(
                |balance| match Option::<String>::from(balance.owner.clone()) {
                    Some(owner)
                        if owner == wallet.to_string() && balance.mint == token.to_string() =>
                    {
                        Some((
                            balance.ui_token_amount.ui_amount?,
                            balance.ui_token_amount.decimals,
                        ))
                    }
                    _ => None,
                },
            ); // Our token-account might have not existed before the swap
        let post_balance =
            post.iter().find_map(
                |balance| match Option::<String>::from(balance.owner.clone()) {
                    Some(owner)
                        if owner == wallet.to_string() && balance.mint == token.to_string() =>
                    {
                        Some((
                            balance.ui_token_amount.ui_amount?,
                            balance.ui_token_amount.decimals,
                        ))
                    }
                    _ => None,
                },
            );

        let decimals = match (pre_balance.map(|p| p.1), post_balance.map(|p| p.1)) {
            (Some(decimals), _) => decimals,
            (_, Some(decimals)) => decimals,
            (None, None) => 0, // won't matter because balances will be zero
        };

        let pre_balance = pre_balance.unwrap_or_default().0;
        let post_balance = post_balance.unwrap_or_default().0;

        let pre_balance = (pre_balance * 10_u32.pow(decimals as u32) as f64).trunc() as u64;
        let post_balance = (post_balance * 10_u32.pow(decimals as u32) as f64).trunc() as u64;

        Some(Balance {
            pre: pre_balance,
            post: post_balance,
        })
    }
}

pub(super) struct TokenDecimals {
    pub input: u8,
    pub output: u8,
}
