pub mod file;
pub mod postgres;

use super::history::{CopyTrade, HistoricalTrades};
use async_trait::async_trait;

#[async_trait]
pub trait CopyStorage {
    async fn bootstrap_trade_history(&self) -> anyhow::Result<HistoricalTrades>;

    async fn process_copy_trade(&self, trade: &CopyTrade) -> anyhow::Result<()>;
}
