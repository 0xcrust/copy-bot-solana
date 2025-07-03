use crate::utils::serde_helpers::field_as_string;
use crate::utils::serde_helpers::pubkey as serde_pubkey;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionsItem {
    pub coingecko_id: Option<String>,
    pub fee_config: Option<TransferFeeDatabaseType>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferFeeDatabaseType {
    #[serde(with = "serde_pubkey")]
    pub transfer_fee_config_authority: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub withdraw_withheld_authority: Pubkey,
    pub withheld_amount: String,
    pub older_transfer_fee: TransferFee,
    pub newer_transfer_fee: TransferFee,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferFee {
    #[serde(with = "field_as_string")]
    pub epoch: u64,
    #[serde(with = "field_as_string")]
    pub maximum_fee: u64,
    pub transfer_fee_basis_points: u16,
}
