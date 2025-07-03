use crate::utils::serde_helpers::option_pubkey as serde_option_pubkey;
use crate::utils::serde_helpers::pubkey as serde_pubkey;

use anyhow::anyhow;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::RwLock;

pub static TOKEN_LIST: Lazy<RwLock<HashMap<Pubkey, JupApiToken>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

pub const TOKEN_LIST_BASE_URL: &str = "https://tokens.jup.ag/tokens";
const DEFAULT_TOKEN_LIST_PATH: &str = "artifacts/jup_tokens_new.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JupApiToken {
    #[serde(with = "serde_pubkey")]
    pub address: Pubkey,
    #[serde(default, with = "serde_option_pubkey")]
    pub program_id: Option<Pubkey>,
    #[serde(rename = "logoURI")]
    pub logo_uri: Option<String>,
    pub symbol: String,
    pub name: String,
    pub decimals: u8,
    #[serde(default)]
    pub tags: Vec<TokenTag>,
    pub daily_volume: Option<f64>,
    #[serde(default, with = "serde_option_pubkey")]
    pub freeze_authority: Option<Pubkey>,
    #[serde(default, with = "serde_option_pubkey")]
    pub mint_authority: Option<Pubkey>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TokenTag {
    /// Tokens that we display as verified on jup.ag. Today, this is a superset consisting of tokens tagged “community” and “lst”. You can use this setting to automatically receive jupiter’s settings when we update our allowlist.
    Verified,
    /// Untagged tokens that we display a warning on on jup.ag.
    Unknown,
    /// Tokens that are verified by the Jupiter community. To get a community tag for your project, go to https://catdetlist.jup.ag
    Community,
    /// Tokens that were validated previously in the strict-list repo. This repo will be deprecated, please use the community site to get a community tag going forward.
    Strict,
    /// Sanctum’s list from their repo which we automatically pull: https://github.com/igneous-labs/sanctum-lst-list/blob/master/sanctum-lst-list.toml
    Lst,
    /// Top 100 trending tokens from birdeye: https://birdeye.so/find-gems?chain=solana
    BirdeyeTrending,
    /// Tokens from Clone protocol, from their repo: https://raw.githubusercontent.com/Clone-Protocol/token-list/main/token_mints.csv
    Clone,
    /// Tokens that graduated from pump, from their API
    Pump,
    #[serde(rename = "token-2022")]
    Token2022,
    #[serde(rename = "moonshot")]
    Moonshot,
    #[serde(rename = "deprecated")]
    Deprecated,
    #[serde(untagged)]
    UnrecognizedTag(String),
}

impl std::fmt::Display for TokenTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenTag::Verified => f.write_str("verified"),
            TokenTag::Unknown => f.write_str("unknown"),
            TokenTag::Community => f.write_str("community"),
            TokenTag::Strict => f.write_str("strict"),
            TokenTag::Lst => f.write_str("lst"),
            TokenTag::BirdeyeTrending => f.write_str("birdeye-trending"),
            TokenTag::Clone => f.write_str("clone"),
            TokenTag::Pump => f.write_str("pump"),
            TokenTag::Token2022 => f.write_str("token-2022"),
            TokenTag::Moonshot => f.write_str("moonshot"),
            TokenTag::Deprecated => f.write_str("deprecated"),
            TokenTag::UnrecognizedTag(tag) => f.write_fmt(format_args!("{}", tag)),
        }
    }
}

impl std::str::FromStr for TokenTag {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "verified" => Ok(TokenTag::Verified),
            "unknown" => Ok(TokenTag::Unknown),
            "community" => Ok(TokenTag::Community),
            "strict" => Ok(TokenTag::Strict),
            "lst" => Ok(TokenTag::Lst),
            "birdeye-trending" => Ok(TokenTag::BirdeyeTrending),
            "clone" => Ok(TokenTag::Clone),
            "pump" => Ok(TokenTag::Pump),
            "token-2022" => Ok(TokenTag::Token2022),
            "moonshot" => Ok(TokenTag::Moonshot),
            "deprecated" => Ok(TokenTag::Deprecated),
            x => Ok(TokenTag::UnrecognizedTag(x.to_string())),
        }
    }
}

pub async fn fetch_token_list(tags: Vec<TokenTag>) -> anyhow::Result<Vec<JupApiToken>> {
    if !tags.is_empty() {
        let tags = tags
            .into_iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let url = format!("{}?tags={}", TOKEN_LIST_BASE_URL, tags);
        println!("url: {}", url);
        Ok(reqwest::get(url).await?.json().await?)
    } else {
        Ok(reqwest::get(TOKEN_LIST_BASE_URL).await?.json().await?)
    }
}

pub async fn fetch_and_save_token_list(
    out: Option<String>,
    tags: Vec<TokenTag>,
) -> Result<Vec<JupApiToken>, anyhow::Error> {
    let tokens = fetch_token_list(tags).await?;
    let token_list_json = serde_json::to_string(&tokens)?;
    let out = out.unwrap_or(DEFAULT_TOKEN_LIST_PATH.to_string());
    std::fs::write(&out, token_list_json)?;
    Ok(tokens)
}

pub async fn load_token_list(load_path: Option<String>) -> anyhow::Result<()> {
    let token_list = token_list_from_path(load_path)?;
    let token_map: HashMap<Pubkey, JupApiToken> = token_list
        .into_iter()
        .map(|token| (token.address, token))
        .collect();
    let mut token_list = TOKEN_LIST.write().unwrap();
    *token_list = token_map;

    Ok(())
}

pub fn token_list_from_path(load_path: Option<String>) -> anyhow::Result<Vec<JupApiToken>> {
    let path = load_path.unwrap_or(DEFAULT_TOKEN_LIST_PATH.to_string());
    let token_list_json = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&token_list_json)?)
}

pub fn get_mint_by_pubkey(pubkey: &Pubkey) -> anyhow::Result<JupApiToken> {
    let token_list = TOKEN_LIST.read().unwrap();
    if let Some(mint_info) = token_list.get(pubkey) {
        return Ok(mint_info.clone());
    }

    Err(anyhow!("Couldn't find token-info for mint {}", pubkey))
}

#[cfg(test)]
pub mod jup_token_list_new {
    use super::{fetch_token_list, TokenTag};

    #[tokio::test]
    async fn get_tokens() {
        let _list = fetch_token_list(vec![TokenTag::Pump]).await.unwrap();
        let _list = fetch_token_list(vec![
            TokenTag::Community,
            TokenTag::BirdeyeTrending,
            TokenTag::Strict,
            TokenTag::Verified,
        ])
        .await
        .unwrap();
        let _list = fetch_token_list(vec![TokenTag::Lst]).await.unwrap();
    }
}
