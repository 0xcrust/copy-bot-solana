use crate::utils::serde_helpers::pubkey as serde_pubkey;
use std::path::Path;

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

pub const ORCA_API_ENDPOINT: &str = "https://api.mainnet.orca.so/v1/whirlpool/list";
pub const DEFAULT_CACHE_PATH: &str = "artifacts/orca_pools.json";

pub async fn get_whirlpools(
    override_cache: bool,
    path: Option<String>,
) -> anyhow::Result<WhirlPoolList> {
    let path = path.unwrap_or(DEFAULT_CACHE_PATH.to_string());

    let cache_exists = Path::new(&path).try_exists()?;
    if cache_exists && !override_cache {
        serde_json::from_str(&std::fs::read_to_string(path)?).map_err(Into::into)
    } else {
        fetch_pools_from_api(Some(path)).await
    }
}

pub async fn fetch_pools_from_api(out: Option<String>) -> anyhow::Result<WhirlPoolList> {
    let pools = reqwest::get(ORCA_API_ENDPOINT)
        .await?
        .json::<WhirlPoolList>()
        .await?;

    if let Some(out) = out {
        std::fs::write(out, serde_json::to_string_pretty(&pools)?)?;
    }

    Ok(pools)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WhirlPoolList {
    pub whirlpools: Vec<IWhirlPool>,
    #[serde(rename = "hasMore")]
    pub has_more: bool,
}

impl WhirlPoolList {
    pub fn find_pool(
        &self,
        mint_a: &Pubkey,
        mint_b: &Pubkey,
        both_sides: bool,
    ) -> Option<&IWhirlPool> {
        if both_sides {
            self.whirlpools.iter().find(|pool| {
                pool.token_a.mint == *mint_a && pool.token_b.mint == *mint_b
                    || pool.token_a.mint == *mint_b && pool.token_b.mint == *mint_a
            })
        } else {
            self.whirlpools
                .iter()
                .find(|pool| pool.token_a.mint == *mint_a && pool.token_b.mint == *mint_b)
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IWhirlPool {
    #[serde(with = "serde_pubkey")]
    pub address: Pubkey,
    #[serde(rename = "tokenA")]
    pub token_a: Token,
    #[serde(rename = "tokenB")]
    pub token_b: Token,
    pub whitelisted: bool,
    #[serde(rename = "tickSpacing")]
    pub tick_spacing: u64,
    pub price: f64,
    #[serde(rename = "lpFeeRate")]
    pub lp_fee_rate: f64,
    #[serde(rename = "protocolFeeRate")]
    pub protocol_fee_rate: f64,
    #[serde(rename = "whirlpoolsConfig", with = "serde_pubkey")]
    pub whirlpools_config: Pubkey,
    #[serde(rename = "modifiedTimeMs")]
    pub modified_time_ms: Option<u64>,
    pub tvl: Option<f64>,
    pub volume: Option<Volume>,
    #[serde(rename = "volumeDenominatedA")]
    pub volume_denominated_a: Option<Volume>,
    #[serde(rename = "volumeDenominatedB")]
    pub volume_denominated_b: Option<Volume>,
    #[serde(rename = "priceRange")]
    pub price_range: Option<PriceRange>,
    #[serde(rename = "feeApr")]
    pub fee_apr: Option<Volume>,
    #[serde(rename = "reward0Apr")]
    pub reward0_apr: Option<Volume>,
    #[serde(rename = "reward1Apr")]
    pub reward1_apr: Option<Volume>,
    #[serde(rename = "reward2Apr")]
    pub reward2_apr: Option<Volume>,
    #[serde(rename = "totalApr")]
    pub total_apr: Option<Volume>,
}

impl PartialEq for IWhirlPool {
    fn eq(&self, other: &Self) -> bool {
        self.token_a.symbol == other.token_a.symbol && self.token_b.symbol == other.token_b.symbol
    }
}

impl Eq for IWhirlPool {}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Token {
    #[serde(with = "serde_pubkey")]
    pub mint: Pubkey,
    pub symbol: String,
    pub name: String,
    pub decimals: u64,
    #[serde(rename = "logoURI")]
    pub logo_uri: Option<String>,
    #[serde(rename = "coingeckoId")]
    pub coingecko_id: Option<String>,
    pub whitelisted: bool,
    #[serde(rename = "poolToken")]
    pub pool_token: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Volume {
    pub day: f64,
    pub week: f64,
    pub month: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MinMax {
    pub min: f64,
    pub max: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PriceRange {
    pub day: MinMax,
    pub week: MinMax,
    pub month: MinMax,
}
