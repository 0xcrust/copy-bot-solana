use biscuit::analytics::{coingecko::CoinGecko, coingecko_price_response_to_historical_price};
use serde::Deserialize;
use solana_sdk::signature::Keypair;

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    dotenv::dotenv()?;
    let coingecko = CoinGecko::new(std::env::var("COINGECKO_API_KEY")?, None)?;
    let current_timestamp = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
    )?;
    let one_year_ago = current_timestamp - (365 * 24 * 60 * 60);

    let historical_prices = coingecko
        .get_historical_price_raw("solana", one_year_ago, current_timestamp, None, None)
        .await?;
    let history = coingecko_price_response_to_historical_price(&historical_prices);
    println!("history: {:#?}", history);
    std::fs::write(
        "artifacts/solana_price_history.json",
        serde_json::to_string_pretty(&history)?,
    )?;

    Ok(())
}
