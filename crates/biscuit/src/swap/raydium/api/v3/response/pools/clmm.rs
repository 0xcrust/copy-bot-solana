use crate::swap::raydium::api::v3::response::ApiV3Token;
use crate::swap::raydium::api::v3::PoolType;
use crate::utils::serde_helpers::pubkey as serde_pubkey;

use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct _ApiV3ClmmPool {
    /// Concentrated
    #[serde(rename = "type")]
    pub pool_type: PoolType,
    pub config: ApiV3ClmmConfig,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiV3ClmmConfig {
    #[serde(with = "serde_pubkey")]
    id: Pubkey,
    index: u16,
    protocol_fee_rate: u32,
    trade_fee_rate: u32,
    tick_spacing: u16,
    fund_fee_rate: u32,
    //description: Option<String>,
    default_range: f64,
    default_range_point: Vec<f64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct _ApiV3ClmmPoolKeys {
    config: ApiV3ClmmConfig,
    reward_infos: Vec<ClmmRewardType>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ClmmRewardType {
    mint: ApiV3Token,
    #[serde(with = "serde_pubkey")]
    vault: Pubkey,
}
