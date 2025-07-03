use crate::core::JoinHandleResult;
use crate::utils::serde_helpers::pubkey;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use dashmap::DashMap;
use futures::StreamExt;
use log::error;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio::sync::oneshot::{channel as oneshot_channel, Sender as OneshotSender};
use tokio::sync::RwLock;

pub const DEFAULT_PRICE_URL: &str = "https://price.jup.ag/v6/price";

pub const DEFAULT_DELAY_SECS: u64 = 60;

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

#[derive(Clone)]
pub struct PriceFeed {
    cache: Arc<RwLock<HashMap<Pubkey, PriceEntry>>>,
}

#[derive(Copy, Clone, Debug)]
struct PriceEntry {
    price: TokenPrice,
    subscribers: usize,
}

impl PriceFeed {
    pub fn start(
        price_url: Option<String>,
        refresh_frequency_ms: u64,
    ) -> (JoinHandleResult<anyhow::Error>, PriceFeed) {
        start_jupiter_price_feed(price_url, refresh_frequency_ms)
    }

    pub async fn get_price_usd(&self, token: &Pubkey) -> Option<TokenPrice> {
        self.cache.read().await.get(&token).map(|t| t.price)
    }

    /// Register a token to be polled for its price
    pub async fn subscribe_token(&self, token: &Pubkey) -> TokenPrice {
        self.cache
            .write()
            .await
            .entry(*token)
            .and_modify(|entry| entry.subscribers += 1)
            .or_insert(PriceEntry {
                price: TokenPrice::NeverPolled,
                subscribers: 1,
            })
            .price
    }

    pub async fn unsubscribe_token(&self, token: &Pubkey) -> Option<TokenPrice> {
        let mut cache = self.cache.write().await;
        use std::collections::hash_map::Entry;

        match cache.entry(*token) {
            Entry::Occupied(mut entry) => {
                let value = *entry.get();
                if value.subscribers > 1 {
                    entry.get_mut().subscribers -= 1;
                } else {
                    entry.remove_entry();
                };
                Some(value.price)
            }
            Entry::Vacant(_) => None,
        }
    }

    pub async fn get_price(&self, input_token: &Pubkey, output_token: &Pubkey) -> Option<f64> {
        let input_token_price = self.get_price_usd(input_token).await?.price()?;
        let output_token_price = self.get_price_usd(output_token).await?.price()?;

        Some(input_token_price / output_token_price)
    }
}

pub fn start_jupiter_price_feed(
    price_url: Option<String>,
    refresh_frequency_ms: u64,
) -> (JoinHandleResult<anyhow::Error>, PriceFeed) {
    let token_price_cache = Arc::new(RwLock::new(HashMap::<Pubkey, PriceEntry>::new()));
    let jh = tokio::task::spawn({
        let mut refresh_interval =
            tokio::time::interval(std::time::Duration::from_millis(refresh_frequency_ms));
        let token_price_cache = Arc::clone(&token_price_cache);
        async move {
            loop {
                let tokens = token_price_cache
                    .read()
                    .await
                    .keys()
                    .copied()
                    .collect::<Vec<_>>();
                let updated_prices = if tokens.is_empty() {
                    HashMap::default()
                } else {
                    // log::info!("Requesting price for {} tokens", tokens.len());
                    get_price_request_usd(price_url.clone(), &tokens).await
                };
                // log::info!("Got prices for {} tokens", updated_prices.len());
                for (token, price) in updated_prices {
                    let token = match Pubkey::from_str(&token) {
                        Ok(key) => key,
                        Err(e) => {
                            error!(
                                "Should be unreachable. Failed conversion from string to pubkey"
                            );
                            Pubkey::default()
                        }
                    };
                    // only update prices that still have subscriptions
                    token_price_cache
                        .write()
                        .await
                        .entry(token)
                        .and_modify(|v| (*v).price = TokenPrice::RecentPrice(price.price));
                }
                refresh_interval.tick().await;
                //log::info!("Polling price. Lfg!");
            }
            #[allow(unreachable_code)]
            Ok::<_, anyhow::Error>(())
        }
    });

    let feed = PriceFeed {
        cache: token_price_cache,
    };
    (jh, feed)
}

// Not using more familiar `futures::stream::iter` because of https://users.rust-lang.org/t/implementation-of-fnonce-is-not-general-enough/78006/3
pub async fn get_price_request_usd(
    base_url: Option<String>,
    ids: &Vec<Pubkey>,
) -> HashMap<String, PriceData> {
    let base_url = base_url.unwrap_or(DEFAULT_PRICE_URL.to_string());
    let chunks = ids.chunks(100).into_iter();
    let futures = futures::stream::FuturesUnordered::new();
    for chunk in chunks {
        futures.push(async {
            let ids = chunk
                .into_iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(",");
            if ids.is_empty() {
                return Ok(None);
            }
            let url = format!("{base_url}?ids={}", ids);
            let response = reqwest::get(&url).await?.json::<JupPriceResponse>().await?;
            Ok::<_, anyhow::Error>(Some(response))
        });
    }

    let results = futures
        .filter_map(|item| async move {
            match item {
                Ok(item) => {
                    // log::info!("Got jup price response successfully!: {:#?}", item);
                    item
                }
                Err(e) => {
                    error!("Error making jup price request: {}", e);
                    None
                }
            }
        })
        .collect::<Vec<_>>()
        .await;
    // log::info!("Got {} prices from API", results.len());
    // for result in &results {
    //     log::info!("Got {} tokens for chunked request", result.data.len());
    // }

    let final_results = results
        .into_iter()
        .map(|response| response.data)
        .reduce(|a, b| a.into_iter().chain(b.into_iter()).collect())
        .unwrap_or_default();
    final_results
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JupPriceResponse {
    pub data: HashMap<String, PriceData>,
    pub time_taken: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceData {
    #[serde(with = "pubkey")]
    pub id: Pubkey,
    pub mint_symbol: String,
    #[serde(with = "pubkey")]
    pub vs_token: Pubkey,
    pub vs_token_symbol: String,
    pub price: f64,
}

mod legacy {
    use super::*;

    pub struct PriceFeed {
        price_request: UnboundedSender<(Pubkey, Option<OneshotSender<Option<f64>>>)>,
        token_price_map: Arc<DashMap<Pubkey, f64>>,
    }

    impl PriceFeed {
        pub fn start(
            price_url: Option<String>,
            refresh_frequency_ms: u64,
        ) -> (JoinHandleResult<anyhow::Error>, PriceFeed) {
            start_jupiter_price_feed(price_url, refresh_frequency_ms)
        }

        pub fn get_price_usd(&self, token: &Pubkey) -> Option<f64> {
            self.token_price_map.get(&token).map(|v| *v.value())
        }

        /// Register a token to be polled for its price
        pub fn register_token(&self, token: &Pubkey) {
            _ = self.price_request.send((*token, None));
        }

        /// Attempts to register a new token in the price feed. If this token is already being
        /// tracked, its price is returned
        pub async fn get_register_token(&self, token: &Pubkey) -> Option<f64> {
            let (sender, receiver) = oneshot_channel();
            _ = self.price_request.send((*token, Some(sender)));
            match receiver.await {
                Ok(result) => result,
                Err(e) => {
                    error!("Failed to receive oneshot result from price request");
                    None
                }
            }
        }

        // 1 SOL -> 150 usdc
        // 1 WIF -> 2 USDC
        // 1 SOL -> 75 wif
        pub fn get_price(&self, input_token: &Pubkey, output_token: &Pubkey) -> Option<f64> {
            let input_token_price = self.token_price_map.get(&input_token).map(|v| *v.value())?;
            let output_token_price = self
                .token_price_map
                .get(&output_token)
                .map(|v| *v.value())?;

            Some(input_token_price / output_token_price)
        }
    }

    pub fn start_jupiter_price_feed(
        price_url: Option<String>,
        refresh_frequency_ms: u64,
    ) -> (JoinHandleResult<anyhow::Error>, PriceFeed) {
        let token_price_map = Arc::new(DashMap::new());
        let (price_request, mut price_request_receiver) =
            unbounded_channel::<(Pubkey, Option<OneshotSender<Option<f64>>>)>();
        let jh = tokio::task::spawn({
            let token_price_map = Arc::clone(&token_price_map);
            let mut refresh_interval =
                tokio::time::interval(std::time::Duration::from_millis(refresh_frequency_ms));
            async move {
                loop {
                    tokio::select! {
                        Some((token, result_sender)) = price_request_receiver.recv() => {
                            let token_price = token_price_map.get(&token);
                            if token_price.is_none() {
                                token_price_map.insert(token, 0.0);
                            }
                            if let Some(sender) = result_sender {
                                _ = sender.send(token_price.map(|v| *v.value()));
                            }
                        }
                        _ = refresh_interval.tick() => {
                            let tokens = token_price_map.iter().map(|p| *p.key()).collect();
                            let updated_prices = get_price_request_usd(price_url.clone(), &tokens).await;
                            for (token, price) in updated_prices {
                                let token = match Pubkey::from_str(&token) {
                                    Ok(key) => key,
                                    Err(e) => {
                                        error!("Should be unreachable. Failed conversion from string to pubkey");
                                        Pubkey::default()
                                    }
                                };
                                token_price_map.insert(token, price.price);
                            }
                        }
                    }
                }
                #[allow(unreachable_code)]
                Ok::<_, anyhow::Error>(())
            }
        });

        let feed = PriceFeed {
            price_request,
            token_price_map,
        };
        (jh, feed)
    }
}
