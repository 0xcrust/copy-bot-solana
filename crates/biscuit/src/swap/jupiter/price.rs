use crate::core::JoinHandleResult;
use crate::utils::serde_helpers::field_as_string;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use futures::StreamExt;
use log::error;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

pub const DEFAULT_PRICE_URL: &str = "https://api.jup.ag/price/v2";
pub const DEFAULT_DELAY_SECS: u64 = 60;

pub const DEFAULT_CLEANUP_FREQUENCY: Duration = Duration::from_secs(60);
pub const DEFAULT_TTL: Duration = Duration::from_secs(150);

#[derive(Copy, Clone, Debug, Default)]
pub enum TokenPrice {
    // Not the `Option` type for clarity
    #[default]
    NeverPolled,
    RecentPrice(f64),
}
impl TokenPrice {
    pub fn price(&self) -> Option<f64> {
        match self {
            TokenPrice::NeverPolled => None,
            TokenPrice::RecentPrice(f64) => Some(*f64),
        }
    }
}

struct Entry {
    price: TokenPrice,
    last_updated: Instant,
    ttl: Duration,
}

#[derive(Clone)]
pub struct PriceFeed {
    // bool: Whether or not we should cleanup this token
    cache: Arc<DashMap<Pubkey, Entry>>,
    price_url: String,
}

impl PriceFeed {
    pub fn start(
        price_url: Option<String>,
        refresh_frequency_ms: u64,
        cleanup_frequency: Option<Duration>,
    ) -> (JoinHandleResult<anyhow::Error>, PriceFeed) {
        let cleanup_frequency = cleanup_frequency.unwrap_or(DEFAULT_CLEANUP_FREQUENCY);
        start_jupiter_price_feed(price_url, refresh_frequency_ms, cleanup_frequency)
    }

    pub fn get_price_usd(&self, token: &Pubkey) -> Option<TokenPrice> {
        self.cache.get(&token).map(|t| t.price)
    }

    pub async fn refresh(&self) -> anyhow::Result<()> {
        update_token_prices(Arc::clone(&self.cache), self.price_url.clone()).await?;
        Ok(())
    }

    /// Register a token to be polled for its price
    pub fn subscribe_token(&self, token: &Pubkey, ttl: Option<Duration>) -> TokenPrice {
        let ttl = ttl.unwrap_or(DEFAULT_TTL);
        self.cache
            .entry(*token)
            .and_modify(|entry| {
                entry.last_updated = Instant::now();
                entry.ttl = ttl
            })
            .or_insert(Entry {
                price: TokenPrice::NeverPolled,
                last_updated: Instant::now(),
                ttl,
            })
            .price
    }

    pub async fn get_price(&self, input_token: &Pubkey, output_token: &Pubkey) -> Option<f64> {
        let input_token_price = self.get_price_usd(input_token)?.price()?;
        let output_token_price = self.get_price_usd(output_token)?.price()?;

        Some(input_token_price / output_token_price)
    }
}

pub fn start_jupiter_price_feed(
    price_url: Option<String>,
    refresh_frequency_ms: u64,
    cleanup_frequency: Duration,
) -> (JoinHandleResult<anyhow::Error>, PriceFeed) {
    let token_price_cache = Arc::new(DashMap::<Pubkey, Entry>::new());
    let price_url = price_url.unwrap_or(DEFAULT_PRICE_URL.to_string());
    let jh = tokio::task::spawn({
        let token_price_cache = Arc::clone(&token_price_cache);
        let mut refresh_interval =
            tokio::time::interval(std::time::Duration::from_millis(refresh_frequency_ms));
        let mut cleanup_interval =
            tokio::time::interval(std::time::Duration::from_millis(refresh_frequency_ms));
        let price_url = price_url.clone();

        async move {
            tokio::select! {
                _ = refresh_interval.tick() => update_token_prices(token_price_cache, price_url.clone()).await?,
                _ = cleanup_interval.tick() => {
                    let entries = token_price_cache
                        .iter()
                        .map(|kv| (*kv.key(), kv.value().last_updated, kv.value().ttl))
                        .collect::<Vec<_>>();
                    for (key, instant, ttl) in entries {
                        if instant.elapsed() > ttl {
                            log::debug!("Removing {} from price cache", key);
                            _ = token_price_cache.remove(&key);
                        }
                    }
                }
            }

            #[allow(unreachable_code)]
            Ok::<_, anyhow::Error>(())
        }
    });

    let feed = PriceFeed {
        cache: token_price_cache,
        price_url,
    };
    (jh, feed)
}

async fn update_token_prices(
    token_price_cache: Arc<DashMap<Pubkey, Entry>>,
    price_url: String,
) -> anyhow::Result<()> {
    let tokens = token_price_cache
        .iter()
        .map(|kv| kv.key().to_string())
        .collect::<Vec<_>>();
    log::debug!("Updating price for {} tokens", tokens.len());
    let updated_prices = if tokens.is_empty() {
        HashMap::default()
    } else {
        // log::info!("Requesting price for {} tokens", tokens.len());
        make_chunked_price_request_usd(Some(price_url), &tokens).await
    };
    log::trace!("Updated prices: {:#?}", updated_prices);

    // Update prices and/or cleanup
    for (token, price) in updated_prices {
        let Some(price) = price else {
            log::trace!("Failed to get price for token {}", token);
            continue;
        };
        let token = match Pubkey::from_str(&token) {
            Ok(key) => key,
            Err(e) => {
                error!("Should be unreachable. Failed conversion from string to pubkey");
                Pubkey::default()
            }
        };

        token_price_cache
            .entry(token)
            .and_modify(|v| (*v).price = TokenPrice::RecentPrice(price.price));
    }
    Ok(())
}

// Not using more familiar `futures::stream::iter` because of https://users.rust-lang.org/t/implementation-of-fnonce-is-not-general-enough/78006/3
pub async fn make_chunked_price_request_usd(
    url: Option<String>,
    ids: &[String],
) -> HashMap<String, Option<PriceData>> {
    let url = url.unwrap_or(DEFAULT_PRICE_URL.to_string());
    if ids.is_empty() {
        return HashMap::default();
    }
    let chunks = ids.chunks(100).into_iter();
    let tasks = futures::stream::FuturesUnordered::new();
    for chunk in chunks {
        tasks.push(async {
            let ids = chunk.join(",");

            let url = format!("{url}?ids={}", ids);
            handle_price_response(url)
        });
    }

    tasks
        .filter_map(|res| async move {
            let res = res.await;
            if let Err(ref e) = res {
                error!("Error making price request: {}", e);
            }
            Some(futures::stream::iter(res.ok()?.data))
        })
        .flatten_unordered(20)
        .collect()
        .await
}

async fn handle_price_response(url: String) -> anyhow::Result<PriceResponse> {
    let response = reqwest::get(url).await?.json::<serde_json::Value>().await?;
    // log::debug!("Response: {:#?}", response);
    match serde_json::from_value(response)? {
        ApiResponse::Error(e) => Err(e)?,
        ApiResponse::Success(price) => Ok(price),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ApiResponse<T> {
    Error(ApiError),
    Success(T),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
    pub message: String,
    #[serde(rename = "statusCode")]
    pub status_code: u16,
}
impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{:?}", self))
    }
}
impl std::error::Error for ApiError {}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceResponse {
    pub data: HashMap<String, Option<PriceData>>,
    pub time_taken: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceData {
    #[serde(with = "field_as_string")]
    pub id: Pubkey,
    #[serde(with = "field_as_string")]
    pub price: f64,
    #[serde(rename = "type")]
    pub price_type: String, // 'derivedPrice'
}
