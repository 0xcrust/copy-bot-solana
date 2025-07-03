use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use anyhow::anyhow;
use biscuit::core::types::wire_transaction::ConfirmTransactionResponse;
use biscuit::services::copy::v2::dust::DustTracker;
use biscuit::swap::execution::{
    ComputeUnitLimits, PriorityFeeConfig, SwapConfigOverrides, SwapExecutionMode, SwapExecutor,
    SwapInput, SwapQuote,
};
use biscuit::swap::jupiter::executor::JupiterExecutor;
use biscuit::wire::jito::{subscribe_jito_tips, JitoClient, JitoRegion, JitoTips};
use biscuit::wire::rpc::poll_blockhash::{
    get_blockhash_data_with_retry, start_blockhash_polling_task,
};
use biscuit::wire::send_txn::service::start_send_transaction_service;
use clap::Parser;
use futures::StreamExt;
use log::{error, info};
use serde::Deserialize;
use solana_account_decoder::UiAccountData;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_rpc_client_api::request::TokenAccountsFilter;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use std::str::FromStr;
use tokio::sync::RwLock;

// cargo run --bin convert_wallet_balance -- --excluded-mints artifacts/excluded.json --dust-bps 1000 --dust-wallet 22SzUa36V7m9AvKvWmsHV1dyjbETZgjUrYTVjFUQQ4YX

#[derive(Parser)]
#[clap(version, about, long_about = None)]
pub struct Opts {
    #[clap(long, env = "CONVERT_BALANCES_KEYPAIR")]
    keypair: String,
    #[clap(long, default_value_t = 1000)]
    slippage_bps: u16,
    #[clap(long, default_value_t = 500)]
    dust_bps: u16,
    #[clap(long)]
    dust_wallet: Option<Pubkey>, // 22SzUa36V7m9AvKvWmsHV1dyjbETZgjUrYTVjFUQQ4YX
    #[clap(long)]
    excluded_mints: Option<String>,
}

#[derive(Deserialize)]
struct Token {
    #[serde(with = "biscuit::utils::serde_helpers::field_as_string")]
    address: Pubkey,
    name: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv()?;
    env_logger::init();

    let opts = Opts::parse();
    let keypair = Arc::new(Keypair::from_base58_string(&opts.keypair));
    let mut excluded_mints = vec![];
    let mut excluded_mints_display = vec![];
    if let Some(path) = opts.excluded_mints {
        let overrides = serde_json::from_str::<Vec<Token>>(&std::fs::read_to_string(&path)?)?;
        excluded_mints.extend(overrides.iter().map(|m| m.address));
        excluded_mints_display.extend(overrides.iter().map(|m| m.name.clone()));
    }

    if opts.dust_bps > 10_000 {
        return Err(anyhow!("dust-bps must be less than 10_000"));
    }

    println!(
        "Proceed to clear wallet balances for wallet {}?\n- dust%={:.2}\n- dust-wallet={}. \n- excluded-mints: {:#?}",
        keypair.pubkey(), (opts.dust_bps as f64)/10_000.0, opts.dust_wallet.map(|w| w.to_string()).unwrap_or("null".to_string()), excluded_mints_display
    );
    println!("Enter to confirm: `y or n`");
    let mut buf = String::new();
    std::io::stdin().read_line(&mut buf)?;
    let response = buf.trim().to_lowercase();

    if response == "n".to_string() || response == "no".to_string() {
        println!("Failed to get confirmation to proceed. Exiting..");
        return Ok(());
    } else if response == "y".to_string() || response == "yes".to_string() {
    } else {
        panic!("Invalid response!");
    }

    let commitment = CommitmentConfig::confirmed();
    let rpc_url = std::env::var("QUICKNODE_MAINNET_RPC_URL")?;
    let output_token = spl_token::native_mint::ID;
    let rpc_client = Arc::new(RpcClient::new_with_commitment(rpc_url, commitment));
    let swap_executor = JupiterExecutor::new(
        Some(std::env::var("QUOTE_API_URL")?),
        Arc::clone(&rpc_client),
    );

    let mut tasks = Vec::new();

    let (blockhash_notif, current_block_height) =
        get_blockhash_data_with_retry(&rpc_client, commitment, 3).await?;
    let blockhash_notif = Arc::new(RwLock::new(blockhash_notif));
    let current_block_height = Arc::new(AtomicU64::new(current_block_height));
    let blockhash_polling_task = start_blockhash_polling_task(
        Arc::clone(&rpc_client),
        Arc::clone(&blockhash_notif),
        Arc::clone(&current_block_height),
        commitment,
        None,
    );
    tasks.push(blockhash_polling_task);

    let jito_client = Arc::new(JitoClient::new(Some(JitoRegion::NewYork)));
    let jito_tips = Arc::new(RwLock::new(JitoTips::default()));
    _ = subscribe_jito_tips(Arc::clone(&jito_tips));

    let (send_transactions_service, send_txn_tasks) = start_send_transaction_service(
        Arc::clone(&rpc_client),
        None,
        Some(jito_client),
        Arc::clone(&current_block_height),
        Arc::clone(&blockhash_notif),
        None,
        commitment,
        None,
        Some(std::time::Duration::from_secs(5)), // send-txn-timeout(don't use this value for prod)
    );
    tasks.extend(send_txn_tasks);

    let (send_txns, mut confirmations) = send_transactions_service.subscribe();
    let dust_tracker = opts.dust_wallet.map(|destination| {
        let tracker = DustTracker::new(
            Arc::clone(&keypair),
            Arc::clone(&rpc_client),
            destination,
            send_transactions_service.subscribe().0,
        );
        tracker.set_jito_tip_amount(75_000);
        tracker
    });

    let user_pubkey = keypair.pubkey();
    let balances = rpc_client
        .get_token_accounts_by_owner_with_commitment(
            &user_pubkey,
            TokenAccountsFilter::ProgramId(spl_token::ID),
            CommitmentConfig::confirmed(),
        )
        .await?
        .value;
    let mut final_balances = vec![];

    for account in balances {
        let process_optional = || async {
            let (mint, mut amount) = match account.account.data {
                UiAccountData::Json(json) => {
                    let parsed = json.parsed.get("info")?;
                    let mint = parsed
                        .get("mint")
                        .and_then(|x| Pubkey::from_str(x.as_str()?).ok())?;
                    let amount = parsed
                        .get("tokenAmount")
                        .and_then(|x| x.get("amount")?.as_str()?.parse::<u64>().ok())?;
                    Some((mint, amount))
                }
                _ => None,
            }?;

            if mint == output_token || excluded_mints.contains(&mint) {
                return None;
            }

            let dust = ((opts.dust_bps as f64 / 10_000.0) * amount as f64).trunc() as u64;
            amount -= dust;

            if let Some(tracker) = &dust_tracker {
                tracker.add_transfer_amount(mint, dust).await;
            }

            if amount == 0 {
                return None;
            }

            Some((mint, amount))
        };

        let Some((mint, amount)) = process_optional().await else {
            continue;
        };

        final_balances.push((mint, amount));
    }

    log::info!("Found {} token balances", final_balances.len());

    let mut stream = futures::stream::iter(final_balances)
        .map(move |(mint, amount)| {
            let executor = swap_executor.clone();
            let send_txns_handle = send_txns.clone();
            let keypair = Arc::clone(&keypair);
            let tips = Arc::clone(&jito_tips);

            async move {
                let quote = executor
                    .quote(
                        &SwapInput {
                            input_token_mint: mint,
                            output_token_mint: output_token,
                            slippage_bps: opts.slippage_bps,
                            amount,
                            mode: SwapExecutionMode::ExactIn,
                            market: None,
                        },
                        None,
                    )
                    .await?;
                let tip = 2_000_000.min(500_000.max(tips.read().await.p95() + 1));
                if quote.other_amount_threshold() < tip {
                    // Don't send a transaction if it's not worth it
                    return Ok(());
                }
                let tx = executor
                    .swap_transaction(
                        keypair.pubkey(),
                        quote,
                        Some(&SwapConfigOverrides {
                            priority_fee: Some(PriorityFeeConfig::JitoTip(tip)),
                            cu_limits: Some(ComputeUnitLimits::Dynamic), // for some reason, this is very important
                            ..Default::default()
                        }),
                    )
                    .await?;
                let tx = tx.transaction.into_versioned_tx()?;
                // if let Some(updated_cu_price) =
                //     helius_priofees.get_priority_fee(PriorityLevel::High)
                // {
                //     tx.message
                //         .modify_compute_unit_price(updated_cu_price as u64);
                // }
                if let Err(e) = send_txns_handle
                    .send_transaction(tx, &[&keypair], true)
                    .await
                {
                    error!("failed to send txn to transaction task: {}", e);
                }
                Ok::<(), anyhow::Error>(())
            }
        })
        .buffered(5);

    let processing_task = tokio::task::spawn(async move {
        while let Some(next) = stream.next().await {
            if let Err(e) = next {
                error!("Error processing futures task: {}", e);
            }
        }
        Ok(())
    });

    let confirmation_task = tokio::task::spawn(async move {
        info!("Looping for confirmations...");
        while let Some(x) = confirmations.recv().await {
            match &x.response {
                ConfirmTransactionResponse::Confirmed {
                    error,
                    confirmation_slot: _,
                } => {
                    info!("Confirmed txn {}. error={:?}", x.data.signature, error);
                }
                ConfirmTransactionResponse::Unconfirmed { reason } => {
                    info!(
                        "Txn {} not confirmed. reason={:?}",
                        x.data.signature, reason
                    );
                }
            }
        }
        Ok(())
    });

    if let Some(dust) = dust_tracker {
        tasks.push(tokio::task::spawn(async move {
            info!("Processing dust...");
            dust.process_dust().await?;
            Ok(())
        }));
    };

    tasks.push(processing_task);
    tasks.push(confirmation_task);

    futures::future::join_all(tasks).await;
    Ok(())
}
