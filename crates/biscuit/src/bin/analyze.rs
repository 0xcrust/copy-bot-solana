use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;

use anyhow::anyhow;
use biscuit::analytics::birdeye::HistoricalPrice;
use biscuit::analytics::dump::{get_swaps_for_wallet, wallet_filepath, MintInfo, Swap};
use biscuit::analytics::wallet::WalletAnalyzer;
use biscuit::core::traits::{FileTxnCache, TransactionCache};
use biscuit::services::copy::config::{CopyConfig, CopyWallet};
use biscuit::services::copy::v2::service::copy_accounts_to_config;
use biscuit::services::copy::v2::simulator::{SimulationError, Simulator};
use biscuit::swap::jupiter::price::make_chunked_price_request_usd;
use biscuit::swap::jupiter::token_list_new::JupApiToken;
use biscuit::utils::serde_helpers::field_as_string;
use biscuit::utils::token::get_info_for_mints;
use clap::Parser;
use envconfig::Envconfig;
use flexi_logger::Logger;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;

const SWAP_TOKEN_LIST_FILE: &str = "token-infos.json";

// cargo run --bin analyze --help
// cargo run --bin analyze dump --help
// cargo run --bin analyze simulate --help
// cargo run --bin analyze -- --wallets-path 'artifacts/wallets/simulation.json' dump --duration 90 --txn-limit 20000 --concurrency 250
// cargo run --bin analyze -- --wallets-path 'artifacts/wallets/simulation.json' simulate --duration 60 --log-swaps false
// cargo run --bin analyze -- --wallets-path 'artifacts/wallets/simulation.json' analyze --duration 60
//

#[derive(Debug, Parser)]
#[clap(version, about, long_about = None)]
pub struct Config {
    /// Solana RPC-URL
    #[clap(env = "RPC_URL")]
    rpc_url: String,

    /// The path to the json file containing the wallets to dump/simulate
    #[clap(short, long, env = "ANALYZE_WALLETS_PATH")]
    wallets_path: String,

    /// Path to the directory containing wallet swaps. The `simulate` command will attempt to
    /// load swaps from this directory, and the `dump` command will output swaps to this directory
    #[clap(long, default_value = "artifacts/analyze/swaps")]
    wallet_swaps_dir: String,

    /// The specific subcommand
    #[clap(subcommand)]
    mode: Command,
}

#[derive(Debug)]
pub enum DumpMode {
    Ignore,
    Overwrite,
    Update,
}
impl std::str::FromStr for DumpMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "ignore" => Ok(DumpMode::Ignore),
            "overwrite" => Ok(DumpMode::Overwrite),
            "update" => Ok(DumpMode::Update),
            _ => Err(anyhow!(
                "Failed parsing dump-mode. Expected ignore | overwrite | update"
            )),
        }
    }
}
impl ToString for DumpMode {
    fn to_string(&self) -> String {
        match self {
            DumpMode::Ignore => "ignore".to_string(),
            DumpMode::Overwrite => "overwrite".to_string(),
            DumpMode::Update => "update".to_string(),
        }
    }
}

#[derive(Debug, Parser)]
pub enum Command {
    /// Dump swaps for a list of wallets
    Dump {
        /// The duration to consider for this fixtures dump, in days
        #[clap(short, long, default_value_t = 14)]
        duration: u64,

        /// Txn-limit. Wallets with greater than this number of txns in the specified period will not be processed
        #[clap(long, default_value_t = 5000)]
        txn_limit: usize,

        /// Fetch and overwrite swaps for wallets, even though swap fixtures are already present for them.
        /// This can be used to guarantee that all wallets have fixtures covering the same time duration.
        #[clap(long, default_value_t = DumpMode::Ignore)]
        // https://stackoverflow.com/questions/77771008/how-do-i-create-a-rust-clap-derive-boolean-flag-that-is-defaulted-to-true-and-ca
        mode: DumpMode,

        #[clap(long, default_value = "artifacts/token_list/lst_community.json")]
        token_list_path: String,

        /// Max concurrent RPC requests. Quicknode gives us 400 reqs/secs
        #[clap(long)]
        concurrency: Option<usize>,

        /// Path to a txn cache folder
        #[clap(long)]
        txn_cache_path: Option<String>,

        /// If specified, combine swaps for all wallets and output it to a single swap fixture file
        #[clap(long)]
        combine_path: Option<String>,

        /// Path to a set of pre-existing simulation fixtures to combine with this one(sorted and deduplicated)
        #[clap(short, long)]
        append: Option<String>,
    },
    /// Run simulations for a list of wallets
    Simulate {
        /// The simulation start balance
        #[clap(env, long, default_value_t = 20000000000)]
        start_balance: u64,

        /// How many days ago to start simulation from
        #[clap(short, long, default_value_t = 14)] // 14 days
        duration: u64,

        /// How many days ago to end simulation
        #[clap(short, long, default_value_t = 0)]
        end_duration: u64,

        /// Whether or not to output each swap to the logs
        #[clap(env, long, default_value_t = true)]
        log_swaps: bool,

        /// Path to a file containing Solana's historical price history
        #[clap(env, long, default_value = "artifacts/solana_price_history.json")]
        sol_price_history: String,

        /// Directory to output simulation results as json
        #[clap(env, long, default_value = "artifacts/analyze/simulation/output")]
        json_out_dir: String,

        /// The file to load simulation fixtures from. If not specified, fixtures
        /// will be created naturally by combining swap fixtures for each copy-wallet
        #[clap(env, short, long)]
        fixtures_path: Option<String>,
    },
    /// Run analysis for a list of wallets
    Analyze {
        /// How many days ago to start analysis from
        #[clap(short, long, default_value_t = 14)] // 14 days
        duration: u64,

        /// How many days ago to end analysis
        #[clap(short, long, default_value_t = 0)]
        end_duration: u64,

        /// Path to a file containing Solana's historical price history
        #[clap(env, long, default_value = "artifacts/solana_price_history.json")]
        sol_price_history: String,

        /// Directory to output analysis results as json
        #[clap(env, long, default_value = "artifacts/analyze/wallets/output")]
        json_out_dir: String,

        #[clap(env, long)]
        price_url: Option<String>,
    },
}

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    dotenv::from_path(".env.simulation")?;
    let config = Config::parse();
    println!("Config: {:#?}", config);

    let current_timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let logger = Logger::try_with_env_or_str("info")?
        .cleanup_in_background_thread(true)
        .duplicate_to_stdout(flexi_logger::Duplicate::All)
        .format(log_formatter);
    log::info!("Config: {:#?}", config);

    let rpc_client = Arc::new(RpcClient::new(config.rpc_url.clone()));

    match config.mode {
        Command::Dump {
            append,
            duration,
            txn_limit,
            mode,
            token_list_path,
            txn_cache_path,
            combine_path,
            concurrency,
        } => {
            logger.start()?;
            process_dump(
                rpc_client,
                duration,
                current_timestamp,
                config.wallets_path,
                txn_limit,
                mode,
                config.wallet_swaps_dir,
                token_list_path,
                combine_path,
                append,
                txn_cache_path,
                concurrency,
            )
            .await?
        }
        Command::Simulate {
            start_balance,
            fixtures_path,
            sol_price_history,
            log_swaps,
            duration,
            end_duration,
            json_out_dir,
        } => {
            logger
                .log_to_file(
                    flexi_logger::FileSpec::default()
                        .directory("artifacts/analyze/simulation/logs")
                        .basename("")
                        .discriminant(current_timestamp.to_string()),
                )
                .start()?;
            process_simulate(
                rpc_client,
                start_balance,
                log_swaps,
                config.wallets_path,
                fixtures_path,
                config.wallet_swaps_dir,
                sol_price_history,
                json_out_dir,
                duration,
                end_duration,
            )
            .await?
        }
        Command::Analyze {
            duration,
            end_duration,
            sol_price_history,
            json_out_dir,
            price_url,
        } => {
            logger
                .log_to_file(
                    flexi_logger::FileSpec::default()
                        .directory("artifacts/analyze/wallets/logs")
                        .basename("")
                        .discriminant(current_timestamp.to_string()),
                )
                .start()?;
            process_analyze(
                config.wallets_path,
                config.wallet_swaps_dir,
                duration,
                end_duration,
                sol_price_history,
                json_out_dir,
                price_url,
            )
            .await?;
        }
    }

    Ok(())
}

#[derive(Deserialize)]
struct Wallet {
    #[serde(with = "field_as_string")]
    wallet: Pubkey,
}

pub async fn process_dump(
    rpc_client: Arc<RpcClient>,
    duration_days: u64,
    current_timestamp: u64,
    wallets_path: String,
    txn_limit: usize,
    mode: DumpMode,
    wallet_swaps_dir: String,
    token_list_path: String,
    combine_path: Option<String>,
    append: Option<String>,
    txn_cache_path: Option<String>,
    concurrency: Option<usize>,
) -> anyhow::Result<()> {
    make_dir_if_not_exists(&wallet_swaps_dir)?;
    if !std::fs::metadata(&wallet_swaps_dir)?.is_dir() {
        return Err(anyhow!("Failed to create wallet swaps directory"));
    }

    let blocktime_limit =
        i64::try_from(current_timestamp - (duration_days * 24 * 60 * 60)).unwrap();
    log::info!(
        "Dump: from-ts={}, current-ts={}, duration(days)={}",
        blocktime_limit,
        current_timestamp,
        duration_days
    );

    let txn_cache = if let Some(path) = txn_cache_path {
        Some(Arc::new(FileTxnCache::create(&path)?) as Arc<dyn TransactionCache>)
    } else {
        None
    };

    let wallets = serde_json::from_str::<Vec<Wallet>>(&std::fs::read_to_string(wallets_path)?)?
        .into_iter()
        .map(|w| w.wallet)
        .collect::<Vec<_>>();

    let mut all_wallets = HashSet::new();
    let mut all_swaps = Vec::<Swap>::new();
    let swaps_dir = std::path::PathBuf::from_str(&wallet_swaps_dir)?;

    let mut update_wallets = Vec::new();
    let mut fetch_wallets = Vec::new();
    let mut ignored_wallets = Vec::new();
    for wallet in &wallets {
        let out = wallet_filepath(&swaps_dir, &wallet);
        if std::fs::metadata(&out).is_ok() {
            match mode {
                DumpMode::Ignore => ignored_wallets.push(*wallet),
                DumpMode::Overwrite => fetch_wallets.push(*wallet),
                DumpMode::Update => update_wallets.push(*wallet),
            }
        }
    }

    if let Some(path) = append {
        // Appending only makes sense if we're outputting combined fixtures
        if combine_path.is_some() {
            let fixture =
                serde_json::from_str::<SimulationFixture>(&std::fs::read_to_string(path)?)?;
            log::info!(
                "Append filepath specified. Found {} swaps for {} wallets",
                fixture.swaps.len(),
                fixture.wallets.len()
            );
            all_wallets.extend(fixture.wallets);
            all_swaps.extend(fixture.swaps);
        }
    }

    if !ignored_wallets.is_empty() {
        log::info!(
            "Loading swaps for {} wallets without update",
            ignored_wallets.len()
        );
    }
    for wallet in ignored_wallets {
        let out = wallet_filepath(&swaps_dir, &wallet);
        let loaded_swaps = serde_json::from_str::<Vec<Swap>>(&std::fs::read_to_string(&out)?)?;
        all_swaps.extend(loaded_swaps);
    }

    if !update_wallets.is_empty() {
        log::info!(
            "Updating swap file for {} wallets: {:#?}",
            update_wallets.len(),
            update_wallets
        );
    }
    let mut failed_update_wallets = vec![];
    for wallet in update_wallets {
        let out = wallet_filepath(&swaps_dir, &wallet);
        let mut loaded_swaps = serde_json::from_str::<Vec<Swap>>(&std::fs::read_to_string(&out)?)?;
        let recent_signature = loaded_swaps.last().map(|swap| swap.origin_signature);

        let fetched_swaps = match get_swaps_for_wallet(
            wallet,
            Arc::clone(&rpc_client),
            duration_days,
            recent_signature,
            Some(txn_limit),
            concurrency,
            txn_cache.clone(),
        )
        .await
        {
            Ok(swaps) => {
                log::info!("Got {} updated swaps for wallet {}", swaps.len(), wallet);
                all_wallets.insert(wallet.to_string());
                swaps
            }
            Err(e) => {
                failed_update_wallets.push((wallet, e.to_string()));
                continue;
            }
        };
        loaded_swaps.extend(fetched_swaps); // should not need sorting?
        std::fs::write(&out, serde_json::to_string_pretty(&loaded_swaps)?)?;
        all_swaps.extend(loaded_swaps)
    }
    if !failed_update_wallets.is_empty() {
        log::error!(
            "Failed to update swaps for {} wallets",
            failed_update_wallets.len()
        );
        for (wallet, error) in failed_update_wallets {
            log::error!("{}. reason: {}", wallet, error);
        }
    }

    if !fetch_wallets.is_empty() {
        log::info!(
            "Loading from RPC for {} wallets: {:#?}",
            wallets.len(),
            wallets
        );
    }

    let mut failed_wallets = Vec::new();
    for wallet in &fetch_wallets {
        let wallet_swaps = match get_swaps_for_wallet(
            *wallet,
            Arc::clone(&rpc_client),
            duration_days,
            None,
            Some(txn_limit),
            concurrency,
            txn_cache.clone(),
        )
        .await
        {
            Ok(swaps) => {
                all_wallets.insert(wallet.to_string());
                log::info!("Got {} swaps for wallet {}", swaps.len(), wallet);
                swaps
            }
            Err(e) => {
                failed_wallets.push((*wallet, e.to_string()));
                continue;
            }
        };
        let out = wallet_filepath(&swaps_dir, &wallet);
        std::fs::write(&out, serde_json::to_string_pretty(&wallet_swaps)?)?;
        all_swaps.extend(wallet_swaps)
    }
    if !failed_wallets.is_empty() {
        log::error!("Failed to fetch swaps for {} wallets", failed_wallets.len());
        for (wallet, error) in failed_wallets {
            log::error!("{}. reason: {}", wallet, error);
        }
    }

    {
        let mut unique_mints = HashSet::new();
        for swap in &all_swaps {
            unique_mints.extend([swap.swap.input_token_mint, swap.swap.output_token_mint]);
        }
        log::info!("unique mints length: {}", unique_mints.len());

        let mut path = std::path::PathBuf::from_str(&wallet_swaps_dir)?;
        path.push(SWAP_TOKEN_LIST_FILE);
        let swap_tokens = if std::fs::metadata(&path).is_ok() {
            let info = serde_json::from_str::<Vec<MintInfo>>(&std::fs::read_to_string(&path)?)?;
            log::info!("Loaded {} mint-infos from {}", info.len(), path.display());
            info
        } else {
            log::error!("Couldn't find file {} to load swap tokens", path.display());
            vec![]
        };

        let token_list = if std::fs::metadata(&token_list_path).is_ok() {
            let info = serde_json::from_str::<Vec<JupApiToken>>(&std::fs::read_to_string(
                &token_list_path,
            )?)?;
            log::info!("Loaded {} mint-infos from {}", info.len(), token_list_path);
            info
        } else {
            log::error!("Couldn't find file {} to load swap tokens", token_list_path);
            vec![]
        }
        .into_iter()
        .map(|token| MintInfo::from(token))
        .collect::<Vec<_>>();

        let mut loaded = [swap_tokens, token_list]
            .concat()
            .into_iter()
            .map(|token| (token.pubkey, token))
            .collect::<HashMap<Pubkey, MintInfo>>();

        let mut mints_to_fetch = vec![];
        for mint in &unique_mints {
            if !loaded.contains_key(&mint) {
                mints_to_fetch.push(*mint)
            }
        }
        log::info!("mint to fetch length={}", mints_to_fetch.len());

        log::info!("Fetching mint infos for {} mints", mints_to_fetch.len());
        let additional_infos = get_info_for_mints(&rpc_client, &mints_to_fetch)
            .await?
            .into_iter()
            .filter_map(|opt| opt.map(|t| (t.pubkey, MintInfo::from(t))))
            .collect::<Vec<_>>();
        log::info!(
            "Fetched {} additional infos successfully",
            additional_infos.len()
        );
        log::info!("Loaded length before: {}", loaded.len());
        loaded.extend(additional_infos);
        log::info!("Loaded length after: {}", loaded.len());

        let mut missing = vec![];
        for mint in &unique_mints {
            if !loaded.contains_key(&mint) {
                missing.push(*mint);
            }
        }
        let token_vec = loaded.into_iter().map(|(_, info)| info).collect::<Vec<_>>();

        if !missing.is_empty() {
            log::error!("Failed to get infos for {} mints", missing.len(),);
        }
        std::fs::write(&path, serde_json::to_string_pretty(&token_vec)?)?;
    }

    if let Some(path) = combine_path {
        log::info!("Combining swap fixtures. Output path: {}", path);
        let start = std::time::Instant::now();
        // First deduplicate
        all_swaps.par_sort_by(|a, b| a.origin_signature.cmp(&b.origin_signature));
        all_swaps.dedup_by(|a, b| a.origin_signature.eq(&b.origin_signature));
        // Then sort by blocktime
        all_swaps.par_sort_by(|a, b| a.block_time.cmp(&b.block_time));
        log::info!(
            "Deduplication and sorting took {} ms",
            start.elapsed().as_millis()
        );

        let deduped_wallets = all_wallets.into_iter().collect::<Vec<_>>();
        let fixture = SimulationFixture {
            wallets: deduped_wallets,
            swaps: all_swaps,
        };
        std::fs::write(path, serde_json::to_string_pretty(&fixture)?)?;
    }

    Ok(())
}

pub async fn process_simulate(
    rpc_client: Arc<RpcClient>,
    start_balance: u64,
    log_swaps: bool,
    wallets_path: String,
    fixtures_path: Option<String>,
    wallet_swaps_dir: String,
    price_history_path: String,
    json_out_dir: String,
    duration_days: u64,
    end_duration_days: u64,
) -> anyhow::Result<()> {
    let copy_config = CopyConfig::init_from_env()?;
    log::info!("Copy config: {:#?}", copy_config);
    if end_duration_days >= duration_days {
        return Err(anyhow!(
            "Simulation end-duration must be more recent than start-duration"
        ));
    }

    let mut path = std::path::PathBuf::from_str(&wallet_swaps_dir)?;
    path.push(SWAP_TOKEN_LIST_FILE);
    let loaded_tokens = if std::fs::metadata(&path).is_ok() {
        let info = serde_json::from_str::<Vec<MintInfo>>(&std::fs::read_to_string(&path)?)?;
        log::info!("Loaded {} mint-infos from {}", info.len(), path.display());
        info
    } else {
        log::error!("Couldn't find file {} to load swap tokens", path.display());
        vec![]
    }
    .into_iter()
    .map(|token| (token.pubkey, token))
    .collect::<HashMap<_, _>>();

    let accounts =
        serde_json::from_str::<Vec<CopyWallet>>(&std::fs::read_to_string(&wallets_path)?)?;
    log::info!("Accounts: {:#?}", accounts);
    let accounts_config = copy_accounts_to_config(&copy_config, &accounts);

    let time_now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let time_start = (time_now - (duration_days * 24 * 60 * 60)) as i64;
    let time_end = (time_now - (end_duration_days * 24 * 60 * 60)) as i64;

    let price_history: Vec<HistoricalPrice> =
        serde_json::from_str(&std::fs::read_to_string(price_history_path)?)?;
    let mut simulator = Simulator::start(
        start_balance,
        log_swaps,
        price_history,
        Arc::clone(&rpc_client),
        Arc::new(copy_config),
        Default::default(),
    )
    .await?;
    log::info!("Started simulator");

    let (mut swaps, wallets) = match fixtures_path {
        Some(path) => {
            log::info!("Specified fixtures path. Loading fixed set of swaps for simulation");
            let fixture =
                serde_json::from_str::<SimulationFixture>(&std::fs::read_to_string(path)?)?;
            (fixture.swaps, fixture.wallets)
        }
        None => {
            log::info!("Loading swaps for each copy wallet");
            let dir = std::path::PathBuf::from_str(&wallet_swaps_dir)?;
            let mut wallets = Vec::new();
            let mut swaps = Vec::new();
            let mut wallets_not_found = Vec::new();
            for account in &accounts {
                let out = wallet_filepath(&dir, &account.wallet);
                if std::fs::metadata(&out).is_ok() {
                    let wallet_swaps =
                        serde_json::from_str::<Vec<Swap>>(&std::fs::read_to_string(&out)?)?;
                    swaps.extend(wallet_swaps);
                    wallets.push(account.wallet.to_string());
                } else {
                    wallets_not_found.push(account.wallet);
                }
            }

            // First deduplicate
            swaps.par_sort_by(|a, b| a.origin_signature.cmp(&b.origin_signature));
            swaps.dedup_by(|a, b| a.origin_signature.eq(&b.origin_signature));
            // Then sort by blocktime
            swaps.par_sort_by(|a, b| a.block_time.cmp(&b.block_time));

            (swaps, wallets)
        }
    };

    log::info!("Loaded simulation fixtures");
    swaps.retain(|swap| swap.block_time >= time_start && swap.block_time <= time_end);

    let mut not_present = Vec::new();
    for account in accounts {
        if !wallets.contains(&account.wallet.to_string()) {
            not_present.push(account.wallet);
        }
    }

    if !not_present.is_empty() {
        println!(
            "[Warning!]: Fixtures not present for wallets {:#?}",
            not_present
        );
        println!("Proceed with simulation?: y/n");
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf)?;
        let response = buf.trim().to_lowercase();

        if response == "y".to_string() || response == "yes".to_string() {
            println!("Proceeding with simulation...");
        } else {
            return Ok(());
        }
    }

    for (wallet, config, _) in accounts_config {
        _ = simulator.insert_wallet(wallet, config);
    }

    log::info!("Running simulations for {} swaps", swaps.len());

    let swaps_len = swaps.len();
    let mut last_i = 0;
    for (i, swap) in swaps.into_iter().enumerate() {
        if let Err(e) = simulator
            .process_swap(
                swap.swap,
                swap.origin_sol_balance,
                swap.origin_input_mint_balance,
                swap.origin_output_mint_balance,
                swap.origin_signer,
                swap.origin_signature,
                swap.block_time,
            )
            .await
        {
            match e {
                SimulationError::InsufficientLamportBalance => {
                    log::error!("Simulation ran out of lamports balance. Breaking");
                    break;
                }
                err => log::error!(
                    "Simulation for {} failed with error: {}",
                    swap.origin_signature,
                    err
                ),
            }
        }
        last_i = i;
    }
    log::info!("Completed {}/{} swap simulations", last_i, swaps_len);

    let mut json_out_dir = std::path::PathBuf::from(json_out_dir);
    let current_timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let filename = format!("{}.json", current_timestamp);
    json_out_dir.push(filename);
    simulator
        .print_simulation_state(Some(&json_out_dir), Some(loaded_tokens))
        .await?;
    Ok(())
}

pub async fn process_analyze(
    wallets_path: String,
    wallet_swaps_dir: String,
    duration_days: u64,
    end_duration_days: u64,
    price_history_path: String,
    json_out_dir: String,
    price_url: Option<String>,
) -> anyhow::Result<()> {
    let mut out_path = std::path::PathBuf::from_str(&json_out_dir)?;
    make_dir_if_not_exists(&out_path)?;
    let wallets =
        serde_json::from_str::<Vec<CopyWallet>>(&std::fs::read_to_string(&wallets_path)?)?;
    let mut path = std::path::PathBuf::from_str(&wallet_swaps_dir)?;
    path.push(SWAP_TOKEN_LIST_FILE);
    let loaded_tokens = if std::fs::metadata(&path).is_ok() {
        let info = serde_json::from_str::<Vec<MintInfo>>(&std::fs::read_to_string(&path)?)?;
        log::info!("Loaded {} mint-infos from {}", info.len(), path.display());
        info
    } else {
        log::error!("Couldn't find file {} to load swap tokens", path.display());
        vec![]
    }
    .into_iter()
    .map(|token| (token.pubkey, token))
    .collect::<HashMap<_, _>>();

    let price_history: Vec<HistoricalPrice> =
        serde_json::from_str(&std::fs::read_to_string(price_history_path)?)?;

    log::info!("Loading swaps for each copy wallet");
    let dir = std::path::PathBuf::from_str(&wallet_swaps_dir)?;
    let mut swaps = Vec::new();
    let mut wallets_not_found = Vec::new();
    for wallet in &wallets {
        let out = wallet_filepath(&dir, &wallet.wallet);
        if std::fs::metadata(&out).is_ok() {
            let wallet_swaps = serde_json::from_str::<Vec<Swap>>(&std::fs::read_to_string(&out)?)?;
            swaps.extend(wallet_swaps);
        } else {
            wallets_not_found.push(wallet);
        }
    }

    let time_now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let time_start = (time_now - (duration_days * 24 * 60 * 60)) as i64;
    let time_end = (time_now - (end_duration_days * 24 * 60 * 60)) as i64;
    swaps.retain(|swap| swap.block_time >= time_start && swap.block_time <= time_end);

    // First deduplicate
    swaps.par_sort_by(|a, b| a.origin_signature.cmp(&b.origin_signature));
    swaps.dedup_by(|a, b| a.origin_signature.eq(&b.origin_signature));
    // Then sort by blocktime
    swaps.par_sort_by(|a, b| a.block_time.cmp(&b.block_time));
    if !wallets_not_found.is_empty() {
        log::warn!(
            "Couldn't find swap dumps for wallets {:#?}",
            wallets_not_found
        );
    }

    let mut analyzer = WalletAnalyzer::new(
        wallets.into_iter().map(|w| w.wallet).collect(),
        price_history,
        price_url.clone(),
    )
    .await?;
    for swap in swaps {
        if let Err(e) = analyzer.process_swap(
            swap.swap,
            swap.origin_signature,
            swap.origin_signer,
            swap.block_time,
        ) {
            log::error!(
                "Analyzer error for signature {}: {}",
                swap.origin_signature,
                e
            );
        }
    }

    let tokens = analyzer
        .get_token_addresses()
        .into_iter()
        .map(|token| token.to_string())
        .collect::<Vec<_>>();

    let price_map = make_chunked_price_request_usd(price_url, &tokens)
        .await
        .into_iter()
        .filter_map(|(k, v)| Some((Pubkey::from_str(&k).ok()?, v?.price)))
        .collect();
    out_path.push(format!("{}.json", time_now));
    analyzer.print_state(Some(&out_path), Some(loaded_tokens), Some(price_map), None)?;

    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimulationFixture {
    pub wallets: Vec<String>,
    pub swaps: Vec<Swap>,
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

fn make_dir_if_not_exists(path: impl AsRef<std::path::Path>) -> anyhow::Result<()> {
    match std::fs::create_dir(path) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => {
            Err(e)?;
        }
    }

    Ok(())
}
