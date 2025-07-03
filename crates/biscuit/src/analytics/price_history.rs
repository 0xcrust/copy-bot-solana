use super::birdeye::*;
use dashmap::DashMap;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

#[derive(Clone)]
pub struct PriceHistory(Arc<Inner>);

struct Inner {
    prices: DashMap<Pubkey, Vec<HistoricalPrice>>,
    birdeye: Birdeye,
}

impl PriceHistory {
    pub fn new(birdeye_api_key: String) -> anyhow::Result<PriceHistory> {
        Ok(PriceHistory(Arc::new(Inner::new(birdeye_api_key)?)))
    }

    pub async fn load_prices_for_token(
        &self,
        token: &Pubkey,
        until_ts: i64,
        from: Option<i64>,
    ) -> anyhow::Result<()> {
        self.0.load_prices_for_token(token, until_ts, from).await
    }

    pub fn get_historical_price_for_token(&self, token: &Pubkey, timestamp: i64) -> Option<f64> {
        self.0.get_historical_price_for_token(token, timestamp)
    }

    pub async fn get_historical_price_for_token_with_api_fallback(
        &self,
        token: &Pubkey,
        timestamp: i64,
    ) -> anyhow::Result<Option<f64>> {
        self.0
            .get_historical_price_for_token_with_api_fallback(token, timestamp)
            .await
    }

    async fn get_historical_price_from_api(
        &self,
        token: &Pubkey,
        timestamp: i64,
    ) -> anyhow::Result<Option<f64>> {
        self.0.get_historical_price_from_api(token, timestamp).await
    }
}

impl Inner {
    fn new(birdeye_api_key: String) -> anyhow::Result<Inner> {
        Ok(Inner {
            prices: Default::default(),
            birdeye: Birdeye::new(birdeye_api_key, None, None)?,
        })
    }

    async fn load_prices_for_token(
        &self,
        token: &Pubkey,
        until_ts: i64,
        from: Option<i64>,
    ) -> anyhow::Result<()> {
        let from = from.unwrap_or(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs() as i64,
        );
        self.prices.insert(
            *token,
            self.birdeye
                .get_price_historical(
                    token.to_string(),
                    HistoricalPriceAddressType::Token,
                    TimeFrame::_15Min,
                    from,
                    until_ts,
                )
                .await?
                .items,
        );
        Ok(())
    }

    fn get_historical_price_for_token(&self, token: &Pubkey, timestamp: i64) -> Option<f64> {
        let price = self.prices.get(token)?;
        get_price_for_timestamp(&price.value(), timestamp)
    }

    async fn get_historical_price_for_token_with_api_fallback(
        &self,
        token: &Pubkey,
        timestamp: i64,
    ) -> anyhow::Result<Option<f64>> {
        if let Some(price) = self.get_historical_price_for_token(token, timestamp) {
            return Ok(Some(price));
        }

        self.get_historical_price_from_api(token, timestamp).await
    }

    async fn get_historical_price_from_api(
        &self,
        token: &Pubkey,
        timestamp: i64,
    ) -> anyhow::Result<Option<f64>> {
        let price = self
            .birdeye
            .get_price_historical_by_unix_time(token.to_string(), Some(timestamp))
            .await?;
        Ok(price.map(|p| p.value))
    }
}

pub fn get_price_for_timestamp(prices: &Vec<HistoricalPrice>, timestamp: i64) -> Option<f64> {
    if prices.is_empty() {
        return None;
    }
    if timestamp < prices.first()?.unix_time || timestamp > prices.last()?.unix_time {
        return None;
    }

    let idx = prices.partition_point(|x| x.unix_time < timestamp); // can be improved?
    Some(prices[idx].value)
}
