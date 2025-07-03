use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use biscuit::core::traits::TxnStreamConfig;
use biscuit::services::copy::storage::file::FileStorage;
use biscuit::services::copy::{config::CopyConfig, v2::service::start_copy_service_v2};
use biscuit::wire::helius_rpc::{
    start_helius_priority_fee_task, Cluster, HeliusClient, HeliusConfig,
};
use biscuit::wire::jito::{jito_tip_stream, subscribe_jito_tips, JitoClient, JitoRegion, JitoTips};
use biscuit::wire::rpc::historical_transactions::RpcHistoricalTxnsConfigBuilder;
use biscuit::wire::rpc::poll_blockhash::{
    get_blockhash_data_with_retry, start_blockhash_polling_task,
};
use biscuit::wire::rpc::poll_slot_leaders::bootstrap_leader_tracker_tasks;
use biscuit::wire::rpc::poll_slots::poll_slots;
use biscuit::wire::send_txn::service::{start_send_transaction_service, Leaders};
use envconfig::Envconfig;
use flexi_logger::Logger;
use solana_client::connection_cache::ConnectionCache;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::signature::Keypair;
use tokio::sync::RwLock;

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    // dotenv::from_path(".env.copy")?;
    dotenv::dotenv()?;
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
    let rpc_url = std::env::var("HELIUS_MAINNET_RPC_URL")?;
    let _ws_url = std::env::var("HELIUS_MAINNET_WS_URL")?;
    let rpc_client = Arc::new(RpcClient::new_with_commitment(rpc_url, commitment));
    let api_key = std::env::var("HELIUS_API_KEY")?;
    let config = Arc::new(HeliusConfig::new(&api_key, Cluster::MainnetBeta)?);
    let client = Arc::new(reqwest::Client::new());
    let helius_client = HeliusClient::new(client, config)?;

    let test_keypair = Arc::new(Keypair::from_base58_string(&std::env::var("KEYPAIR")?));

    let jito_client = Arc::new(JitoClient::new(Some(JitoRegion::NewYork)));
    let jito_tips = Arc::new(RwLock::new(JitoTips::default()));

    let file_storage = FileStorage::create(".copy_storage")?;
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

    let mut tasks = vec![];
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

    let leader_offset = 0;
    let num_leaders = 4;
    let leader_tracker = bootstrap_leader_tracker_tasks(
        Arc::clone(&rpc_client),
        commitment,
        slots_receiver,
        leader_offset,
        num_leaders,
    )
    .await?;

    let connection_pool_size = 4;
    let connection_cache = Arc::new(ConnectionCache::new_with_client_options(
        "biscuit",
        connection_pool_size,
        None,
        Some((&test_keypair, IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)))),
        None,
    ));
    let (send_transactions_service, send_txn_tasks) = start_send_transaction_service(
        Arc::clone(&rpc_client),
        None,
        Some(Arc::clone(&jito_client)),
        Arc::clone(&current_block_height),
        Arc::clone(&blockhash_notif),
        Some(Leaders {
            leader_tracker,
            tpu_connection_cache: connection_cache,
        }),
        commitment,
        None,
        Some(std::time::Duration::from_secs(5)), // send-txn-timeout(don't use this value for prod)
    );
    tasks.extend(send_txn_tasks);

    let helius_priofees =
        start_helius_priority_fee_task(helius_client, Duration::from_secs(60), None);

    let history_config = RpcHistoricalTxnsConfigBuilder::new()
        .with_shared_rpc_client(Arc::clone(&rpc_client))
        .with_commitment(CommitmentConfig::confirmed())
        .with_n_transactions(10) // Copy last 5 transactions for each
        .ignore_error_transactions(true)
        .build()?;
    let history_stream = history_config.poll_transactions();

    let config = CopyConfig::init_from_env()?;
    let handle = start_copy_service_v2(
        Arc::clone(&rpc_client),
        test_keypair,
        send_transactions_service,
        CommitmentConfig::confirmed(),
        config,
        Some(&helius_priofees),
        "artifacts/wallets/history.json".to_string(),
        Arc::new(history_stream),
        Arc::clone(&jito_tips),
        bootstrap_history,
        Box::new(file_storage),
    )
    .await?;
    tasks.extend(handle.tasks);

    futures::future::join_all(tasks).await;
    Ok(())
}
