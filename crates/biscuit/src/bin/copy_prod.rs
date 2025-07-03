use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use std::time::Duration;

use biscuit::core::traits::{StreamInterface, TxnStreamConfig};
use biscuit::core::types::transaction::ITransaction;
use biscuit::services::copy::storage::file::FileStorage;
use biscuit::services::copy::{config::CopyConfig, v2::service::start_copy_service_v2};
use biscuit::wire::geyser::subscribe_transactions::grpc_txns_task;
use biscuit::wire::helius_rpc::{
    start_helius_priority_fee_task, Cluster, HeliusClient, HeliusConfig,
};
use biscuit::wire::jito::{subscribe_jito_tips, JitoClient, JitoRegion, JitoTips};
use biscuit::wire::rpc::poll_blockhash::{
    get_blockhash_data_with_retry, start_blockhash_polling_task,
};
use biscuit::wire::rpc::poll_slot_leaders::bootstrap_leader_tracker_tasks;
use biscuit::wire::rpc::poll_slots::poll_slots;
use biscuit::wire::rpc::poll_transactions::RpcTransactionsConfigBuilder;
use biscuit::wire::send_txn::service::{start_send_transaction_service, Leaders};
use clap::Parser;
use educe::Educe;
use envconfig::Envconfig;
use flexi_logger::{Cleanup, Criterion, Logger, Naming};
use solana_client::connection_cache::ConnectionCache;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::{EncodableKey, Signer};
use tokio::sync::RwLock;

#[derive(Parser, Educe)]
#[educe(Debug)]
#[clap(version, about, long_about = None)]
pub struct Config {
    /// Either a keypair string OR a path to a keypair file
    #[clap(env = "KEYPAIR")]
    #[educe(Debug(ignore))]
    keypair: String,
    /// The path to the json file containing the wallets to copy
    #[clap(env = "WALLETS")]
    wallets: String,
    /// The path to the directory where logs will be output
    #[clap(env = "LOG_DIR")]
    log_dir: String,
    /// The prefix for log filename
    #[clap(env = "LOG_PREFIX")]
    log_prefix: String,
    /// Solana RPC-URL
    #[clap(env = "RPC_URL")]
    rpc_url: String,
    /// Helius API KEY
    #[clap(env = "HELIUS_API_KEY")]
    helius_api_key: String,
    // Postgres URL
    // #[clap(env = "DATABASE_URL")]
    // database_url: String,
    /// Timeout for each send-txn call
    #[clap(env = "SEND_TXN_TIMEOUT_SECS")]
    send_txn_timeout_secs: u64,
    /// Whether or not to bootstrap history from a database
    #[clap(env = "BOOTSTRAP_HISTORY", default_value_t = true)]
    bootstrap_history: bool,
    #[clap(env = "INIT_PRIOFEE", default_value_t = false)]
    with_priofees: bool,
    /// How frequently to update the priofee cache
    #[clap(env = "PRIOFEE_POLL_SECS", short, default_value_t = 60)]
    priority_fee_poll_frequency_secs: u64,
    /// How frequently to poll for wallet transactions
    #[clap(env = "TXN_POLL_SECS", long, default_value_t = 5)]
    txn_poll_frequency_secs: u64,
    #[clap(env = "BLOCKHASH_POLL_SECS", long)]
    blockhash_poll_seconds: Option<u64>,
    #[clap(env = "LOG_MAX_SIZE_MB", long, default_value_t = 200)] // default is 200 mb
    log_max_size_mb: u64,
    #[clap(env = "LOG_KEEP_TXT_COUNT", long, default_value_t = 5)]
    log_keep_txt_count: usize,
    #[clap(env = "LOG_KEEP_COMPRESSED_COUNT", long, default_value_t = 95)]
    log_keep_compressed_count: usize,
    #[clap(env = "DISABLE_LEADER_TRACKER", long, default_value_t = false)]
    disable_leader_tracker: bool,
    #[clap(env = "STAKED_RPC_CONNECTION_URL")]
    staked_rpc_connection: Option<String>,
    #[clap(env = "GRPC_ENDPOINT", long)]
    grpc_endpoint: Option<String>,
    #[clap(env = "GRPC_X_TOKEN", long)]
    grpc_x_token: Option<String>,
}

fn log_formatter(
    w: &mut dyn std::io::Write,
    now: &mut flexi_logger::DeferredNow,
    record: &log::Record,
) -> Result<(), std::io::Error> {
    write!(
        w,
        "[{}] {} [{}] ",
        now.format("%Y-%m-%d %H:%M:%S"),
        record.level(),
        record.module_path().unwrap_or("<unnamed>"),
    )?;

    write!(w, "{}", record.args())
}

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    dotenv::from_path(".env.copy")?;
    let config = Config::parse();

    // Logs shouldn't take more than 5gb
    Logger::try_with_env_or_str("info")?
        .log_to_file(
            flexi_logger::FileSpec::default()
                .directory(&config.log_dir)
                .basename(&config.log_prefix),
        )
        .cleanup_in_background_thread(true)
        .rotate(
            Criterion::Size(config.log_max_size_mb * 1024 * 1024), // convert to bytes
            Naming::Numbers,
            Cleanup::KeepLogAndCompressedFiles(
                config.log_keep_txt_count,
                config.log_keep_compressed_count,
            ),
        )
        .duplicate_to_stdout(flexi_logger::Duplicate::All)
        .format(log_formatter)
        .start()?;

    log::info!("Config: {:#?}", config);
    let keypair = Keypair::read_from_file(&config.keypair)
        .or::<anyhow::Error>(Ok(Keypair::from_base58_string(&config.keypair)))?;
    log::info!("Starting copy with wallet {}", keypair.pubkey());
    let commitment = CommitmentConfig::confirmed();
    let rpc_client = Arc::new(RpcClient::new_with_commitment(
        config.rpc_url.clone(),
        commitment,
    ));

    let mut tasks = vec![];
    let jito_client = Arc::new(JitoClient::new(Some(JitoRegion::NewYork)));
    let jito_tips = Arc::new(RwLock::new(JitoTips::default()));
    _ = subscribe_jito_tips(Arc::clone(&jito_tips));

    let file_storage = FileStorage::create(".copy_storage")?;
    // #[cfg(debug_assertions)]
    // {
    //     println!("fetching token list");
    //     biscuit::swap::jupiter::token_list_old::fetch_and_save_token_list(true, None, true).await?;
    //     println!("fetched token list");
    // }

    let (blockhash_notif, current_block_height) =
        get_blockhash_data_with_retry(&rpc_client, commitment, 3).await?;
    let blockhash_notif = Arc::new(RwLock::new(blockhash_notif));
    let current_block_height = Arc::new(AtomicU64::new(current_block_height));
    let blockhash_polling_task = start_blockhash_polling_task(
        Arc::clone(&rpc_client),
        Arc::clone(&blockhash_notif),
        Arc::clone(&current_block_height),
        commitment,
        config.blockhash_poll_seconds.map(Duration::from_secs),
    );
    tasks.push(blockhash_polling_task);

    let leader_tracker = if config.disable_leader_tracker {
        None
    } else {
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
            Some((&keypair, IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)))),
            None,
        ));

        Some(Leaders {
            leader_tracker,
            tpu_connection_cache: connection_cache,
        })
    };

    let (send_transactions_service, send_txn_tasks) = start_send_transaction_service(
        Arc::clone(&rpc_client),
        config
            .staked_rpc_connection
            .clone()
            .map(|url| Arc::new(RpcClient::new(url))),
        Some(Arc::clone(&jito_client)), // Should be `Some` in prod!
        Arc::clone(&current_block_height),
        Arc::clone(&blockhash_notif),
        leader_tracker,
        commitment,
        None,
        Some(std::time::Duration::from_secs(config.send_txn_timeout_secs)), // send-txn-timeout
    );
    tasks.extend(send_txn_tasks);

    let helius_priofees = if dbg!(config.with_priofees) {
        let helius_client = HeliusClient::new(
            Arc::new(reqwest::Client::new()),
            Arc::new(HeliusConfig::new(
                &config.helius_api_key,
                Cluster::MainnetBeta,
            )?),
        )?;

        Some(start_helius_priority_fee_task(
            helius_client,
            Duration::from_secs(config.priority_fee_poll_frequency_secs),
            None,
        ))
    } else {
        None
    };

    let txn_stream = if let Some(endpoint) = config.grpc_endpoint {
        let (txn_stream, task) = grpc_txns_task(
            endpoint,
            config.grpc_x_token,
            //vec![],
            "copy_txns_watcher".to_string(),
        );
        tasks.push(task);
        Arc::new(txn_stream) as Arc<dyn StreamInterface<Item = anyhow::Result<ITransaction>>>
    } else {
        let txn_stream = RpcTransactionsConfigBuilder::new()
            .with_shared_rpc_client(Arc::clone(&rpc_client))
            .with_poll_frequency(Duration::from_secs(config.txn_poll_frequency_secs))
            .with_commitment(CommitmentConfig::confirmed())
            .build()?
            .poll_transactions();
        Arc::new(txn_stream) as Arc<dyn StreamInterface<Item = anyhow::Result<ITransaction>>>
    };

    let mut copy_config = CopyConfig::init_from_env()?;
    if config.staked_rpc_connection.is_some() {
        copy_config.send_jito_default = false;
    }
    log::info!("Copy config: {:#?}", copy_config);
    let handle = start_copy_service_v2(
        Arc::clone(&rpc_client),
        Arc::new(keypair),
        send_transactions_service,
        CommitmentConfig::confirmed(),
        copy_config,
        helius_priofees.as_ref(),
        config.wallets.clone(),
        txn_stream,
        jito_tips,
        config.bootstrap_history,
        Box::new(file_storage),
    )
    .await?;
    tasks.extend(handle.tasks);

    futures::future::join_all(tasks).await;
    log::warn!("Exiting copy-prod...");
    Ok(())
}
