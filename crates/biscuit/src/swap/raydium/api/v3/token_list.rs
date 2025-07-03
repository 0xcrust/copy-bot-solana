use super::response::{ApiV3Token, ApiV3TokenList};
use super::ApiV3Client;
use std::collections::HashMap;
use std::sync::RwLock;

use anyhow::anyhow;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

pub static RAYDIUM_TOKEN_LIST: Lazy<RwLock<HashMap<Pubkey, (ApiV3Token, Status)>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

const DEFAULT_TOKEN_LIST_PATH: &str = "artifacts/raydium_tokens.json";

#[derive(Copy, Clone, Serialize, Deserialize)]
pub enum Status {
    BlackList,
    MintList,
    // WhiteList
}

pub async fn fetch_and_save_token_list(
    base_url: Option<String>,
    out: Option<String>,
) -> Result<(), anyhow::Error> {
    let client = ApiV3Client::new(base_url);
    let token_list = client.get_token_list().await?;
    let token_list_json = serde_json::to_string(&token_list)?;
    let out = out.unwrap_or(DEFAULT_TOKEN_LIST_PATH.to_string());
    std::fs::write(&out, token_list_json)?;
    Ok(())
}

pub async fn load_token_list(load_path: Option<String>) -> anyhow::Result<()> {
    let token_list = token_list_from_path(load_path).await?;
    let mint_list = token_list
        .mint_list
        .into_iter()
        .map(|token| (token.address, (token, Status::MintList)));
    let blacklist = token_list
        .blacklist
        .into_iter()
        .map(|token| (token.address, (token, Status::MintList)));
    let token_map = mint_list
        .chain(blacklist)
        .collect::<HashMap<Pubkey, (ApiV3Token, Status)>>();

    let mut token_list = RAYDIUM_TOKEN_LIST.write().unwrap();
    *token_list = token_map;

    Ok(())
}

pub async fn token_list_from_path(load_path: Option<String>) -> anyhow::Result<ApiV3TokenList> {
    let path = load_path.unwrap_or(DEFAULT_TOKEN_LIST_PATH.to_string());
    let token_list_json = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&token_list_json)?)
}

pub fn get_mint_by_pubkey(pubkey: &Pubkey) -> anyhow::Result<(ApiV3Token, Status)> {
    let token_list = RAYDIUM_TOKEN_LIST.read().unwrap();
    if let Some(mint_info) = token_list.get(pubkey) {
        return Ok(mint_info.clone());
    }

    Err(anyhow!("Couldn't find token-info for mint {}", pubkey))
}
