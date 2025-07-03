use crate::swap::token_extensions::ExtensionsItem;
use crate::utils::serde_helpers::pubkey as serde_pubkey;

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiV3Token {
    pub chain_id: u64,
    #[serde(with = "serde_pubkey")]
    pub address: Pubkey,
    #[serde(default, with = "serde_pubkey")]
    pub program_id: Pubkey,
    #[serde(default, rename = "logoURI")]
    pub logo_uri: String,
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub name: String,
    pub decimals: u8,
    #[serde(default)]
    pub tags: Vec<ApiV3TokenTag>,
    pub extensions: ExtensionsItem,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApiV3TokenTag {
    #[serde(rename = "hasFreeze")]
    HasFreeze,
    #[serde(rename = "hasTransferFee")]
    HasTransferFee,
    #[serde(rename = "token-2022")]
    Token2022,
    #[serde(rename = "community")]
    Community,
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(untagged)]
    UnrecognizedTag(String),
}

impl std::str::FromStr for ApiV3TokenTag {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "hasFreeze" => Ok(Self::HasFreeze),
            "hasTransferFee" => Ok(Self::HasTransferFee),
            "token-2022" => Ok(Self::Token2022),
            "community" => Ok(Self::Community),
            "unknown" => Ok(Self::Unknown),
            x => Ok(Self::UnrecognizedTag(x.to_string())),
        }
    }
}

impl std::fmt::Display for ApiV3TokenTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HasFreeze => f.write_str("hasFreeze"),
            Self::HasTransferFee => f.write_str("hasTransferFee"),
            Self::Token2022 => f.write_str("token-2022"),
            Self::Community => f.write_str("community"),
            Self::Unknown => f.write_str("unknown"),
            Self::UnrecognizedTag(x) => f.write_fmt(format_args!("{}", x)),
        }
    }
}
