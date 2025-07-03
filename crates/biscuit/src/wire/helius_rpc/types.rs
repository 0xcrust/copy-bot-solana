use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solana_transaction_status::UiTransactionEncoding;
use std::fmt::Debug;

/// Defines the available clusters supported by Helius
#[derive(Debug, Clone, PartialEq)]
pub enum Cluster {
    Devnet,
    MainnetBeta,
}

/// Stores the API and RPC endpoint URLs for a specific Helius cluster
#[derive(Debug, Clone)]
pub struct HeliusEndpoints {
    pub api: String,
    pub rpc: String,
}

impl HeliusEndpoints {
    pub fn for_cluster(cluster: &Cluster) -> Self {
        match cluster {
            Cluster::Devnet => HeliusEndpoints {
                api: "https://api-devnet.helius-rpc.com/".to_string(),
                rpc: "https://devnet.helius-rpc.com/".to_string(),
            },
            Cluster::MainnetBeta => HeliusEndpoints {
                api: "https://api-mainnet.helius-rpc.com/".to_string(),
                rpc: "https://mainnet.helius-rpc.com/".to_string(),
            },
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct RpcRequest<T> {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    #[serde(rename = "params")]
    pub parameters: T,
}

impl<T> RpcRequest<T> {
    pub fn new(method: String, parameters: T) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: "1".to_string(),
            method,
            parameters,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct RpcResponse<T> {
    pub jsonrpc: String,
    pub id: String,
    pub result: T,
}

#[derive(Copy, Clone, Serialize, Deserialize, Debug)]
pub enum PriorityLevel {
    Min,
    Low,
    Medium,
    High,
    VeryHigh,
    UnsafeMax,
    Default,
}

impl std::str::FromStr for PriorityLevel {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let level = match s {
            "min" => PriorityLevel::Min,
            "low" => PriorityLevel::Low,
            "medium" => PriorityLevel::Medium,
            "high" => PriorityLevel::High,
            "veryhigh" => PriorityLevel::VeryHigh,
            "unsafemax" => PriorityLevel::UnsafeMax,
            "default" => PriorityLevel::Default,
            _ => return Err(anyhow!("Invalid priority level. Expected `min` | `low` | `medium` | high | veryhigh | `unsafemax` | `default`. Got {}", s)),
        };
        Ok(level)
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetPriorityFeeEstimateOptions {
    pub priority_level: Option<PriorityLevel>,
    pub include_all_priority_fee_levels: Option<bool>,
    pub transaction_encoding: Option<UiTransactionEncoding>,
    pub lookback_slots: Option<u8>,
    pub recommended: Option<bool>,
    pub include_vote: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct GetPriorityFeeEstimateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<String>,
    #[serde(rename = "accountKeys", skip_serializing_if = "Option::is_none")]
    pub account_keys: Option<Vec<String>>,
    pub options: Option<GetPriorityFeeEstimateOptions>,
}

#[derive(Serialize, Deserialize, Debug, Default, Copy, Clone)]
pub struct MicroLamportPriorityFeeLevels {
    pub min: f64,
    pub low: f64,
    pub medium: f64,
    pub high: f64,
    #[serde(rename = "veryHigh")]
    pub very_high: f64,
    #[serde(rename = "unsafeMax")]
    pub unsafe_max: f64,
}

#[derive(Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetPriorityFeeEstimateResponse {
    pub priority_fee_estimate: Option<f64>,
    pub priority_fee_levels: Option<MicroLamportPriorityFeeLevels>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
#[serde(default)]
pub struct TokenAccountsList {
    pub total: u32,
    pub limit: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    pub token_accounts: Vec<TokenAccount>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TokenAccount {
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_amount: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_extensions: Option<Value>,
    pub frozen: bool,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetTokenAccounts {
    pub owner: Option<String>,
    pub mint: Option<String>,
    pub limit: Option<u32>,
    pub page: Option<u32>,
    pub before: Option<String>,
    pub after: Option<String>,
    #[serde(default, alias = "displayOptions")]
    pub options: Option<DisplayOptions>,
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct DisplayOptions {
    pub show_collection_metadata: bool,
    pub show_grand_total: bool,
    pub show_unverified_collections: bool,
    pub show_raw_data: bool,
    pub show_fungible: bool,
    pub require_full_index: bool,
    pub show_system_metadata: bool,
    pub show_zero_balance: bool,
    pub show_closed_accounts: bool,
}
