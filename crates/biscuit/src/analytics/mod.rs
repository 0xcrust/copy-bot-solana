use birdeye::HistoricalPrice;
use coingecko::CoinGeckoPrice;

pub mod birdeye;
pub mod coingecko;
pub mod database;
pub mod dump;
pub mod price_history;
pub mod wallet;

pub fn coingecko_price_response_to_historical_price(
    response: &CoinGeckoPrice,
) -> Vec<HistoricalPrice> {
    response
        .prices
        .iter()
        .copied()
        .map(|(unix_time, value)| HistoricalPrice { unix_time, value })
        .collect()
}
