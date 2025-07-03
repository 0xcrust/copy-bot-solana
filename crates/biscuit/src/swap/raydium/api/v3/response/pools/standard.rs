use super::super::ApiV3Token;
use crate::swap::raydium::api::v3::PoolType;
use crate::utils::serde_helpers::option_pubkey as serde_option_pubkey;
use crate::utils::serde_helpers::pubkey as serde_pubkey;

use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct _ApiV3StandardPool {
    /// Standard
    #[serde(rename = "type")]
    pub pool_type: PoolType,
    #[serde(default, with = "serde_option_pubkey")]
    pub market_id: Option<Pubkey>,
    #[serde(default, with = "serde_option_pubkey")]
    pub config_id: Option<Pubkey>,
    pub lp_price: f64,
    pub lp_amount: f64,
    pub lp_mint: ApiV3Token,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct _ApiV3StandardPoolKeys {
    #[serde(with = "serde_pubkey")]
    pub authority: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub open_orders: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub target_orders: Pubkey,
    pub mint_lp: ApiV3Token,
    #[serde(flatten)]
    pub market: MarketKeys,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketKeys {
    #[serde(with = "serde_pubkey")]
    pub market_program_id: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub market_id: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub market_authority: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub market_base_vault: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub market_quote_vault: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub market_bids: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub market_asks: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub market_event_queue: Pubkey,
}
