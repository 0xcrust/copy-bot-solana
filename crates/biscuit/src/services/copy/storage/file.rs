use super::super::history::{CopyTrade, HistoricalTrades};
use super::CopyStorage;
use anyhow::Context;
use async_trait::async_trait;
use solana_sdk::pubkey::Pubkey;
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Clone)]
pub struct FileStorage {
    dir_path: String,
}

impl FileStorage {
    pub fn create(path: &str) -> anyhow::Result<Self> {
        Self::make_dir_if_not_exists(path)?;

        Ok(FileStorage {
            dir_path: path.to_string(),
        })
    }

    fn insert_trade(&self, trade: &CopyTrade) -> anyhow::Result<()> {
        let mut path = PathBuf::from(self.dir_path.clone());

        path.push(trade.origin_wallet.to_string());
        Self::make_dir_if_not_exists(&path)?;

        path.push(trade.token.to_string());
        Self::make_dir_if_not_exists(&path)?;

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_micros();
        path.push(format!("{}.json", timestamp));

        std::fs::write(path, serde_json::to_string_pretty(&trade)?)?;
        Ok(())
    }

    fn bootstrap_history(&self) -> anyhow::Result<HistoricalTrades> {
        let historical_trades = HistoricalTrades::default();
        for dir_entry in std::fs::read_dir(&self.dir_path)? {
            let Ok(entry) = dir_entry else {
                continue;
            };
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let origin_wallet = Pubkey::from_str(
                path.file_name()
                    .context("name for wallet dir should be defined")?
                    .to_str()
                    .context("filename should be string")?,
            )?;

            for dir_entry in std::fs::read_dir(&path)? {
                let Ok(entry) = dir_entry else {
                    continue;
                };
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let token = Pubkey::from_str(
                    path.file_name()
                        .context("name for json file should be defined")?
                        .to_str()
                        .context("filename should be string")?
                        .trim_end_matches(".json"),
                )?;

                for dir_entry in std::fs::read_dir(&path)? {
                    let Ok(entry) = dir_entry else {
                        continue;
                    };
                    let trade = serde_json::from_str::<CopyTrade>(&std::fs::read_to_string(
                        &entry.path(),
                    )?)?;
                    historical_trades.insert_trade_inner(
                        &origin_wallet,
                        &token,
                        trade.volume.exec_volume_token,
                        trade.volume.origin_volume_token,
                        trade.volume.exec_volume_usd,
                        trade.volume.origin_volume_usd,
                        trade.volume.exec_volume_sol,
                        trade.volume.origin_volume_sol,
                        trade.volume.dust_token_amount,
                        &trade.direction,
                    )?;
                }
            }
        }

        Ok(historical_trades)
    }

    fn make_dir_if_not_exists(path: impl AsRef<Path>) -> anyhow::Result<()> {
        match std::fs::create_dir(path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => {
                Err(e)?;
            }
        }

        Ok(())
    }
}

#[async_trait]
impl CopyStorage for FileStorage {
    async fn bootstrap_trade_history(&self) -> anyhow::Result<HistoricalTrades> {
        self.bootstrap_history()
    }

    async fn process_copy_trade(&self, trade: &CopyTrade) -> anyhow::Result<()> {
        self.insert_trade(trade)
    }
}
