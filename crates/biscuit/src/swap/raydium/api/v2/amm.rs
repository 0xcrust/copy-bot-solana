use crate::utils::serde_helpers::pubkey as serde_pubkey;

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

// See: https://api-v3.raydium.io/docs/#/

pub const RAYDIUM_POOL_INFO_ENDPOINT: &str = "https://api.raydium.io/v2/sdk/liquidity/mainnet.json";
pub const RAYDIUM_PRICE_INFO_ENDPOINT: &str = "https://api.raydium.io/v2/main/price";
pub const DEFAULT_CACHE_PATH: &str = "artifacts/raydium_v4_pools.json";

pub async fn get_liquidity_pools(
    override_cache: bool,
    path: Option<String>,
) -> anyhow::Result<RaydiumV4PoolInfo> {
    let path = path.unwrap_or(DEFAULT_CACHE_PATH.to_string());

    let cache_exists = Path::new(&path).try_exists()?;
    if cache_exists && !override_cache {
        serde_json::from_str(&std::fs::read_to_string(path)?).map_err(Into::into)
    } else {
        fetch_pools_from_api(Some(path)).await
    }
}

async fn fetch_pools_from_api(out: Option<String>) -> anyhow::Result<RaydiumV4PoolInfo> {
    let pools = reqwest::get(RAYDIUM_POOL_INFO_ENDPOINT)
        .await?
        .json::<RaydiumV4PoolInfo>()
        .await?;

    if let Some(out) = out {
        std::fs::write(out, serde_json::to_string_pretty(&pools)?)?;
    }

    Ok(pools)
}

pub async fn fetch_prices_from_api() -> anyhow::Result<Price> {
    let price = reqwest::get(RAYDIUM_PRICE_INFO_ENDPOINT)
        .await?
        .json::<Price>()
        .await?;
    Ok(price)
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RaydiumV4PoolInfo {
    pub official: Vec<RaydiumV4Pool>,
    #[serde(rename = "unOfficial")]
    pub unofficial: Vec<RaydiumV4Pool>,
}

impl RaydiumV4PoolInfo {
    pub fn find_pool(
        &self,
        mint_a: &Pubkey,
        mint_b: &Pubkey,
        both_sides: bool,
        allow_unofficial: bool,
    ) -> Option<&RaydiumV4Pool> {
        let mut pools: Box<dyn Iterator<Item = _>> = if allow_unofficial {
            Box::new(self.official.iter().chain(self.unofficial.iter()))
        } else {
            Box::new(self.official.iter())
        };

        if both_sides {
            pools.find(|pool| {
                pool.base_mint == *mint_a && pool.quote_mint == *mint_b
                    || pool.base_mint == *mint_b && pool.quote_mint == *mint_a
            })
        } else {
            pools.find(|pool| pool.base_mint == *mint_a && pool.quote_mint == *mint_b)
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RaydiumV4Pool {
    #[serde(with = "serde_pubkey")]
    pub id: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub base_mint: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub quote_mint: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub lp_mint: Pubkey,
    pub base_decimals: u8,
    pub quote_decimals: u8,
    pub lp_decimals: u8,
    pub version: u8,
    #[serde(with = "serde_pubkey")]
    pub program_id: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub authority: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub open_orders: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub target_orders: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub base_vault: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub quote_vault: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub withdraw_queue: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub lp_vault: Pubkey,
    pub market_version: u8,
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

pub struct Price(pub HashMap<Pubkey, f64>);

impl Price {
    pub fn get_token_price(&self, token: &Pubkey) -> anyhow::Result<Option<f64>> {
        Ok(self
            .0
            .iter()
            .find_map(|(tok, price)| {
                if *token == *tok {
                    return Some(price);
                }
                None
            })
            .copied())
    }
}

impl<'de> serde::de::Deserialize<'de> for Price {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        price::deserialize(deserializer)
    }
}

mod price {
    use super::Price;
    use std::str::FromStr;

    use serde::de::{self, Deserializer, Visitor};
    use solana_sdk::pubkey::Pubkey;

    struct PriceVisitor;
    impl<'de> Visitor<'de> for PriceVisitor {
        type Value = Price;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str(r#"an hashmap of pubkey strings to f64 amounts"#)
        }
        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::MapAccess<'de>,
        {
            let mut hmap = std::collections::HashMap::with_capacity(map.size_hint().unwrap_or(0));
            while let Some((x, y)) = map.next_entry::<String, f64>()? {
                let key = Pubkey::from_str(&x)
                    .map_err(|_| de::Error::custom("failed string to pubkey conversion"))?;
                hmap.insert(key, y);
            }
            Ok(Price(hmap))
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Price, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(PriceVisitor)
    }
}

/*fn deserialize_price_info(value: serde_json::Value) -> anyhow::Result<HashMap<String, f64>> {
    debug!("fn: deserialize_price_info(value={})", value);
    Ok(value
        .as_object()
        .ok_or(anyhow::format_err!("malformed content. expected object."))?
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.as_f64().expect("value is f64")))
        .collect())
}*/
