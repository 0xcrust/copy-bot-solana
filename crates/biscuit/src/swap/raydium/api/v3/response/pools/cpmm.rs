use crate::swap::raydium::api::v3::response::ApiV3Token;
use crate::swap::raydium::api::v3::PoolType;
use crate::utils::serde_helpers::pubkey as serde_pubkey;

use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct _ApiV3CpmmPool {
    /// Standard
    #[serde(rename = "type")]
    pub pool_type: PoolType,
    pub lp_mint: ApiV3Token,
    pub lp_price: f64,
    pub lp_amount: u64,
    pub config: ApiV3CpmmConfig,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiV3CpmmConfig {
    #[serde(with = "serde_pubkey")]
    pub id: Pubkey,
    pub index: u16,
    pub protocol_fee_rate: u32,
    pub trade_fee_rate: u32,
    pub fund_fee_rate: u32,
    pub create_pool_fee: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct _ApiV3CpmmPoolKeys {
    #[serde(with = "serde_pubkey")]
    pub authority: Pubkey,
    pub mint_lp: ApiV3Token,
    pub config: ApiV3CpmmConfig,
}
