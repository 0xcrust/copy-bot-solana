use crate::swap::token_extensions::ExtensionsItem;
use crate::utils::serde_helpers::pubkey as serde_pubkey;

use anyhow::anyhow;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::RwLock;

pub static TOKEN_LIST: Lazy<RwLock<HashMap<Pubkey, JupApiToken>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

pub const TOKEN_LIST_BASE_URL: &str = "https://token.jup.ag";
const DEFAULT_TOKEN_LIST_PATH_STRICT: &str = "artifacts/token_lists/jupiter/strict.json";
const DEFAULT_TOKEN_LIST_PATH_ALL: &str = "artifacts/token_lists/jupiter/all.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JupApiToken {
    pub chain_id: u64,
    #[serde(with = "serde_pubkey")]
    pub address: Pubkey,
    #[serde(default, with = "serde_pubkey")]
    pub program_id: Pubkey,
    #[serde(rename = "logoURI")]
    pub logo_uri: Option<String>,
    pub symbol: String,
    pub name: String,
    pub decimals: u8,
    pub tags: Vec<TokenTag>,
    pub extensions: Option<ExtensionsItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TokenTag {
    /// Tokens that are verified by the Jupiter community. To get a community tag for your project, go to https://catdetlist.jup.ag
    Community,
    OldRegistry,
    SolanaFm,
    #[serde(rename = "token-2022")]
    Token2022,
    Wormhole,
    #[serde(untagged)]
    UnrecognizedTag(String),
}

pub async fn fetch_token_list(strict: bool) -> Result<Vec<JupApiToken>, anyhow::Error> {
    let url = if strict {
        format!("{TOKEN_LIST_BASE_URL}/strict")
    } else {
        format!("{TOKEN_LIST_BASE_URL}/all")
    };
    Ok(reqwest::get(url).await?.json().await?)
}

pub async fn fetch_and_save_token_list(
    strict: bool,
    out: Option<String>,
    map_load: bool,
) -> Result<(), anyhow::Error> {
    let tokens = fetch_token_list(strict).await?;
    let token_list_json = serde_json::to_string(&tokens)?;
    let default_out = if strict {
        DEFAULT_TOKEN_LIST_PATH_STRICT
    } else {
        DEFAULT_TOKEN_LIST_PATH_ALL
    };
    let out = out.unwrap_or(default_out.to_string());
    std::fs::write(&out, token_list_json)?;

    if map_load {
        let token_map: HashMap<Pubkey, JupApiToken> = tokens
            .into_iter()
            .map(|token| (token.address, token))
            .collect();
        let mut token_list = TOKEN_LIST.write().unwrap();
        *token_list = token_map;
    }

    Ok(())
}

pub async fn load_token_list(strict: bool, load_path: Option<String>) -> anyhow::Result<()> {
    let token_list = token_list_from_path(strict, load_path).await?;
    let token_map: HashMap<Pubkey, JupApiToken> = token_list
        .into_iter()
        .map(|token| (token.address, token))
        .collect();
    let mut token_list = TOKEN_LIST.write().unwrap();
    *token_list = token_map;

    Ok(())
}

pub async fn token_list_from_path(
    strict: bool,
    load_path: Option<String>,
) -> anyhow::Result<Vec<JupApiToken>> {
    let default_out = if strict {
        DEFAULT_TOKEN_LIST_PATH_STRICT
    } else {
        DEFAULT_TOKEN_LIST_PATH_ALL
    };
    let path = load_path.unwrap_or(default_out.to_string());
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
pub mod jup_token_list_old {
    use super::fetch_token_list;

    #[tokio::test]
    async fn get_tokens() {
        let _list = fetch_token_list(false).await.unwrap();
    }
}
