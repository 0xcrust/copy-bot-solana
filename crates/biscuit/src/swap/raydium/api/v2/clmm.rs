use crate::utils::serde_helpers::option_pubkey as serde_option_pubkey;
use crate::utils::serde_helpers::pubkey as serde_pubkey;
use std::path::Path;

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

pub const RAYDIUM_BASE_URL: &str = "https://api.raydium.io";
pub const CLMM_POOLS_ENDPOINT: &str = "/v2/ammV3/ammPools";
pub const DEFAULT_CACHE_PATH: &str = "artifacts/raydium_v6_pools.json";

pub async fn get_clmm_pools(
    override_cache: bool,
    path: Option<String>,
) -> anyhow::Result<RaydiumV6PoolInfo> {
    let path = path.unwrap_or(DEFAULT_CACHE_PATH.to_string());

    let cache_exists = Path::new(&path).try_exists()?;
    if cache_exists && !override_cache {
        serde_json::from_str(&std::fs::read_to_string(path)?).map_err(Into::into)
    } else {
        fetch_pools_from_api(Some(path)).await
    }
}

async fn fetch_pools_from_api(out: Option<String>) -> anyhow::Result<RaydiumV6PoolInfo> {
    let url = format!("{}{}", RAYDIUM_BASE_URL, CLMM_POOLS_ENDPOINT);
    let pools = reqwest::get(url).await?.json::<RaydiumV6PoolInfo>().await?;

    if let Some(out) = out {
        std::fs::write(out, serde_json::to_string_pretty(&pools)?)?;
    }

    Ok(pools)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RaydiumV6PoolInfo {
    pub data: Vec<RaydiumV6Pool>,
}

impl RaydiumV6PoolInfo {
    pub fn find_pool(
        &self,
        mint_a: &Pubkey,
        mint_b: &Pubkey,
        both_sides: bool,
    ) -> Option<&RaydiumV6Pool> {
        if both_sides {
            self.data.iter().find(|pool| {
                pool.mint_a == *mint_a && pool.mint_b == *mint_b
                    || pool.mint_a == *mint_b && pool.mint_b == *mint_a
            })
        } else {
            self.data
                .iter()
                .find(|pool| pool.mint_a == *mint_a && pool.mint_b == *mint_b)
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RaydiumV6Pool {
    #[serde(with = "serde_pubkey")]
    pub id: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub mint_program_id_a: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub mint_program_id_b: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub mint_a: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub mint_b: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub vault_a: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub vault_b: Pubkey,
    pub mint_decimals_a: u64,
    pub mint_decimals_b: u64,
    pub amm_config: ClmmConfig,
    #[serde(default, with = "serde_option_pubkey")]
    pub observation_id: Option<Pubkey>,
    pub reward_infos: Vec<RewardInfos>,
    pub tvl: f64,
    pub day: TimeStats,
    pub week: TimeStats,
    pub month: TimeStats,
    #[serde(default, with = "serde_option_pubkey")]
    pub lookup_table_account: Option<Pubkey>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeStats {
    pub volume: f64,
    pub volume_fee: f64,
    pub fee_a: f64,
    pub fee_b: f64,
    pub fee_apr: f64,
    pub reward_apr: RewardApr,
    pub apr: f64,
    pub price_min: f64,
    pub price_max: f64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct RewardApr {
    a: f64,
    b: f64,
    c: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClmmConfig {
    #[serde(with = "serde_pubkey")]
    pub id: Pubkey,
    pub index: u16,
    pub protocol_fee_rate: u32,
    pub trade_fee_rate: u32,
    pub tick_spacing: u16,
    pub fund_fee_rate: u32,
    #[serde(with = "serde_pubkey")]
    pub fund_owner: Pubkey,
    pub description: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RewardInfos {
    #[serde(with = "serde_pubkey")]
    pub mint: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub program_id: Pubkey,
}
