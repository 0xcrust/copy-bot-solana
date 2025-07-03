use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use biscuit::constants::mints::{BONK, SOL, WIF};
use biscuit::core::traits::{StreamInterface, TxnStreamConfig};
use biscuit::core::types::transaction::ITransaction;
use biscuit::database::PgInstance;
use biscuit::services::copy::storage::file::FileStorage;
use biscuit::services::copy::{config::CopyConfig, v2::service::start_copy_service_v2};
use biscuit::services::testing::JitoTestConfig;
use biscuit::wire::geyser::subscribe_transactions::grpc_txns_task;
use biscuit::wire::helius_rpc::{
    start_helius_priority_fee_task, Cluster, HeliusClient, HeliusConfig,
};
use biscuit::wire::jito::{jito_tip_stream, subscribe_jito_tips, JitoClient, JitoRegion, JitoTips};
use biscuit::wire::rpc::poll_blockhash::{
    get_blockhash_data_with_retry, start_blockhash_polling_task,
};
use biscuit::wire::rpc::poll_slots::poll_slots;
use biscuit::wire::send_txn::service::start_send_transaction_service;
use envconfig::Envconfig;
use flexi_logger::Logger;
use solana_client::connection_cache::ConnectionCache;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::EncodableKey;
use solana_sdk::{pubkey, pubkey::Pubkey};
use tokio::sync::RwLock;

const CHILLGUY: Pubkey = pubkey!("Df6yfrKC8kZE3KNkrHERKzAetSxbrWeniQfyJY4Jpump");

// Nirvana:
// We'd like to advise that you please check the following:
// - The vpc security rules - make sure port 22 is still open to your IP
// - That your IP is static and hasn’t changed
// - Or if your router has flagged the ssh connection and blocked it via firewall

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    dotenv::from_path(".env.copy")?;
    //dotenv::dotenv()?;
    // env_logger::init();

    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    Logger::try_with_env_or_str("info")?
        .log_to_file(
            flexi_logger::FileSpec::default()
                .directory("artifacts/logs")
                .basename("copy2_")
                .discriminant(timestamp.to_string()),
        )
        .duplicate_to_stdout(flexi_logger::Duplicate::All)
        .start()?;

    let commitment = CommitmentConfig::confirmed();
    let rpc_url = std::env::var("RPC_URL")?;
    let api_key = std::env::var("HELIUS_API_KEY")?;
    // let rpc_url = std::env::var("HELIUS_MAINNET_RPC_URL")?;
    // let _ws_url = std::env::var("HELIUS_MAINNET_WS_URL")?;
    let rpc_client = Arc::new(RpcClient::new_with_commitment(rpc_url, commitment));
    // let api_key = std::env::var("HELIUS_API_KEY")?;
    let config = Arc::new(HeliusConfig::new(&api_key, Cluster::MainnetBeta)?);
    let client = Arc::new(reqwest::Client::new());
    let helius_client = HeliusClient::new(client, config)?;

    let test_keypair = Arc::new(Keypair::read_from_file("keys/NEFs.json").unwrap());
    let copy_keypair = Arc::new(Keypair::read_from_file("keys/A41E.json").unwrap());
    let mut tasks = vec![];

    let jito_client = Arc::new(JitoClient::new(Some(JitoRegion::NewYork)));
    let jito_tips = Arc::new(RwLock::new(JitoTips::default()));

    // let db_url = std::env::var("DATABASE_URL")?;
    // let postgres = PgInstance::connect(&db_url, None).await?;
    let bootstrap_history = std::env::var("BOOTSTRAP_HISTORY")
        .unwrap_or("false".to_string())
        .parse::<bool>()?;

    if cfg!(debug_assertions) {
        _ = tokio::task::spawn(jito_tip_stream(Arc::clone(&jito_tips)));
    } else {
        _ = subscribe_jito_tips(Arc::clone(&jito_tips));
    }

    #[cfg(debug_assertions)]
    {
        biscuit::swap::jupiter::token_list_old::fetch_and_save_token_list(true, None, true).await?;
    }

    let (blockhash_notif, current_block_height) =
        get_blockhash_data_with_retry(&rpc_client, commitment, 3).await?;
    let blockhash_notif = Arc::new(RwLock::new(blockhash_notif));
    let current_block_height = Arc::new(AtomicU64::new(current_block_height));
    let blockhash_polling_task = start_blockhash_polling_task(
        // make sure it's the `confirmed` commitment
        Arc::clone(&rpc_client),
        Arc::clone(&blockhash_notif),
        Arc::clone(&current_block_height),
        commitment,
        None,
    );
    tasks.push(blockhash_polling_task);

    let (slots_sender, slots_receiver) = tokio::sync::broadcast::channel(100); // todo:
    let _exit_signal = Arc::new(AtomicBool::new(false));
    let slot_tasks = poll_slots(
        Arc::clone(&rpc_client),
        CommitmentConfig::confirmed(),
        slots_sender,
        _exit_signal,
    );
    tasks.extend(slot_tasks);

    let (send_transactions_service, send_txn_tasks) = start_send_transaction_service(
        Arc::clone(&rpc_client),
        Some(Arc::new(RpcClient::new(std::env::var(
            "STAKED_RPC_CONNECTION_URL",
        )?))),
        Some(Arc::clone(&jito_client)), // Should be `Some` in prod!
        Arc::clone(&current_block_height),
        Arc::clone(&blockhash_notif),
        None,
        commitment,
        None,
        Some(std::time::Duration::from_secs(5)), // send-txn-timeout(don't use this value for prod)
    );
    tasks.extend(send_txn_tasks);

    let helius_priofees =
        start_helius_priority_fee_task(helius_client, Duration::from_secs(60), None);

    let test_tasks = biscuit::services::testing::send_multiple_swap_transactions(
        Arc::clone(&rpc_client),
        Arc::clone(&test_keypair),
        std::env::var("QUOTE_API_URL")?,
        vec![
            (SOL, WIF, 100_000),
            (SOL, BONK, 100_000),
            // (BONK, SOL, 700_777_125),
            // (BONK, WIF, 700_777_125),
            (SOL, WIF, 100_000),
            (SOL, BONK, 100_000),
            // (WIF, SOL, 84_837),
            // (WIF, BONK, 84_837),
        ],
        Some(5000), // 5s delay between transactions so order is preserved
        Some(1000),
        send_transactions_service.clone(),
        Arc::new(AtomicBool::new(false)),
        &helius_priofees,
        Some(JitoTestConfig {
            tips: Arc::clone(&jito_tips),
            max_tip_lamports: 500_000,
        }),
        // None
    );
    tasks.extend(test_tasks);

    let txn_stream = {
        let (txn_stream, task) = grpc_txns_task(
            std::env::var("GRPC_ENDPOINT")?,
            std::env::var("GRPC_X_TOKEN").ok(),
            //vec![],
            "copy_txns_watcher".to_string(),
        );
        tasks.push(task);
        Arc::new(txn_stream) as Arc<dyn StreamInterface<Item = anyhow::Result<ITransaction>>>
    };
    let mut copy_config = CopyConfig::init_from_env()?;
    copy_config.send_jito_default = false;

    let handle = start_copy_service_v2(
        Arc::clone(&rpc_client),
        copy_keypair,
        send_transactions_service,
        CommitmentConfig::confirmed(),
        copy_config,
        Some(&helius_priofees),
        "artifacts/wallets/test.json".to_string(),
        txn_stream,
        jito_tips,
        bootstrap_history,
        Box::new(FileStorage::create(".copy_storage")?),
    )
    .await?;
    tasks.extend(handle.tasks);

    futures::future::join_all(tasks).await;
    Ok(())
}
