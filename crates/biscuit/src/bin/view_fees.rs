use std::sync::Arc;
use std::time::Duration;

use biscuit::parser::generated::jupiter_aggregator_v6::programs::JUPITER_AGGREGATOR_V6_ID;
use biscuit::utils::fees::PriorityDetails;
use biscuit::wire::helius_rpc::{
    start_helius_priority_fee_task, Cluster, HeliusClient, HeliusConfig, PriorityLevel,
};
use biscuit::wire::jito::{subscribe_jito_tips, JitoTips};
use clap::Parser;
use educe::Educe;
use solana_sdk::native_token::LAMPORTS_PER_SOL;
use tokio::sync::RwLock;

#[derive(Parser, Educe)]
#[educe(Debug)]
#[clap(version, about, long_about = None)]
pub struct Config {
    /// Helius API KEY
    #[clap(env = "HELIUS_API_KEY")]
    helius_api_key: String,
    /// How frequently to update the priofee cache
    #[clap(env = "PRIOFEE_POLL_SECS", short, default_value_t = 60)]
    priority_fee_poll_frequency_secs: u64,
}

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    dotenv::dotenv()?;
    let config = Config::parse();
    let helius_client = HeliusClient::new(
        Arc::new(reqwest::Client::new()),
        Arc::new(HeliusConfig::new(
            &config.helius_api_key,
            Cluster::MainnetBeta,
        )?),
    )?;

    let jito_tips = Arc::new(RwLock::new(JitoTips::default()));
    _ = subscribe_jito_tips(Arc::clone(&jito_tips));

    let priofees_handle = start_helius_priority_fee_task(
        helius_client,
        Duration::from_secs(config.priority_fee_poll_frequency_secs),
        None,
    );
    priofees_handle.update_accounts(vec![JUPITER_AGGREGATOR_V6_ID.to_string()]);

    let _task = tokio::task::spawn({
        async move {
            loop {
                let jito_tips = jito_tips.read().await;

                let fees = priofees_handle.get_priority_fee(PriorityLevel::VeryHigh);
                let priofee = fees.map(|cu_price| {
                    let fee =
                        PriorityDetails::from_cu_price(cu_price.trunc() as u64, None).priority_fee;
                    fee as f64 / LAMPORTS_PER_SOL as f64
                });
                println!("Helius priofees: {:#?}", priofee);
                println!("Jito tips: {:#?}", jito_tips);

                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
            #[allow(unreachable_code)]
            Ok::<_, anyhow::Error>(())
        }
    });

    tokio::time::sleep(std::time::Duration::from_secs(300)).await;
    // futures::future::join_all(tasks).await;
    log::warn!("Exiting view fees binary...");
    Ok(())
}
