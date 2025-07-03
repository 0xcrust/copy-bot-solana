pub mod error;
pub mod request_handler;
pub mod types;

use std::fmt::Debug;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use error::{HeliusError, Result};
use request_handler::RequestHandler;
pub use types::{Cluster, HeliusEndpoints, MicroLamportPriorityFeeLevels, RpcRequest, RpcResponse};
pub use types::{
    GetPriorityFeeEstimateOptions, GetPriorityFeeEstimateRequest, GetPriorityFeeEstimateResponse,
    GetTokenAccounts, PriorityLevel, TokenAccountsList,
};

use dashmap::DashMap;
use futures::StreamExt;
use reqwest::{Client, Method, Url};
use serde::de::DeserializeOwned;
use serde::Serialize;
use solana_client::rpc_client::RpcClient as SolanaRpcClient;

pub fn start_helius_priority_fee_task(
    helius: HeliusClient,
    poll_interval: Duration,
    lookback_slots: Option<u8>,
) -> HeliusPrioFeesHandle {
    let accounts_map = Arc::new(DashMap::<usize, PriorityData>::new());
    let options = GetPriorityFeeEstimateOptions {
        priority_level: Some(PriorityLevel::Medium),
        include_all_priority_fee_levels: Some(true),
        transaction_encoding: None,
        lookback_slots,
        recommended: None,
        include_vote: Some(false),
    };

    let jh = tokio::task::spawn({
        let accounts_map = Arc::clone(&accounts_map);
        let mut interval = tokio::time::interval(poll_interval);
        let helius = helius.clone();
        let options = options.clone();

        async move {
            loop {
                let prio_accounts = accounts_map
                    .iter()
                    .map(|x| (*x.key(), x.value().accounts.clone()))
                    .collect::<Vec<_>>();
                let mut responses = futures::stream::iter(prio_accounts)
                    .map(|(id, accounts)| {
                        let helius_client = helius.clone();
                        let options = options.clone();
                        async move {
                            let response = helius_client
                                .get_priority_fee_estimate(GetPriorityFeeEstimateRequest {
                                    transaction: None,
                                    account_keys: Some(accounts),
                                    options: Some(options),
                                })
                                .await?;

                            Ok::<_, anyhow::Error>((id, response))
                        }
                    })
                    .buffer_unordered(30);

                while let Some(result) = responses.next().await {
                    if let Ok((index, response)) = result {
                        accounts_map.entry(index).and_modify(|value| {
                            value.latest_response = response.priority_fee_levels
                        });
                    }
                }

                interval.tick().await;
            }
        }
    });

    HeliusPrioFeesHandle {
        id: 1,
        next_id: Arc::new(AtomicUsize::new(2)),
        map: accounts_map,
        client: helius,
        lookback_slots,
        options,
    }
}

#[derive(Clone)]
pub struct HeliusPrioFeesHandle {
    id: usize,
    next_id: Arc<AtomicUsize>,
    map: Arc<DashMap<usize, PriorityData>>,
    client: HeliusClient,
    lookback_slots: Option<u8>,
    options: GetPriorityFeeEstimateOptions,
}

impl HeliusPrioFeesHandle {
    pub fn subscribe(&self) -> HeliusPrioFeesHandle {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        HeliusPrioFeesHandle {
            id,
            next_id: Arc::clone(&self.next_id),
            map: Arc::clone(&self.map),
            client: self.client.clone(),
            lookback_slots: self.lookback_slots,
            options: self.options.clone(),
        }
    }

    pub fn update_accounts(&self, updated_keys: Vec<String>) {
        self.map
            .entry(self.id)
            .and_modify(|value| value.accounts = updated_keys.clone())
            .or_insert(PriorityData {
                accounts: updated_keys.clone(),
                latest_response: Default::default(),
            });

        tokio::task::spawn({
            let map = Arc::clone(&self.map);
            let helius_client = self.client.clone();
            let lookback_slots = self.lookback_slots;
            let id = self.id;
            let options = self.options.clone();
            async move {
                let response = helius_client
                    .get_priority_fee_estimate(GetPriorityFeeEstimateRequest {
                        transaction: None,
                        account_keys: Some(updated_keys.clone()),
                        options: Some(options),
                    })
                    .await?;

                map.entry(id)
                    .and_modify(|value| value.latest_response = response.priority_fee_levels);
                Ok::<_, anyhow::Error>(())
            }
        });
    }

    pub fn get_priority_fee(&self, level: PriorityLevel) -> Option<f64> {
        let result = self.map.get(&self.id).as_ref()?.latest_response?;
        let value = match level {
            PriorityLevel::Default => result.medium,
            PriorityLevel::High => result.high,
            PriorityLevel::Low => result.low,
            PriorityLevel::Medium => result.medium,
            PriorityLevel::Min => result.min,
            PriorityLevel::UnsafeMax => result.unsafe_max,
            PriorityLevel::VeryHigh => result.very_high,
        };
        Some(value)
    }
}

#[derive(Debug, Default)]
struct PriorityData {
    accounts: Vec<String>,
    latest_response: Option<MicroLamportPriorityFeeLevels>,
}

#[derive(Clone)]
pub struct HeliusClient {
    pub handler: RequestHandler,
    pub config: Arc<HeliusConfig>,
    pub solana_client: Arc<SolanaRpcClient>,
}

impl HeliusClient {
    pub fn new(client: Arc<Client>, config: Arc<HeliusConfig>) -> Result<Self> {
        let handler: RequestHandler = RequestHandler::new(client)?;
        let url: String = format!("{}/?api-key={}", config.endpoints.rpc, config.api_key);
        let solana_client: Arc<SolanaRpcClient> = Arc::new(SolanaRpcClient::new(url));

        Ok(HeliusClient {
            handler,
            config,
            solana_client,
        })
    }

    pub async fn post_rpc_request<R, T>(&self, method: &str, request: R) -> Result<T>
    where
        R: Debug + Serialize + Send + Sync,
        T: Debug + DeserializeOwned + Default,
    {
        let base_url: String = format!(
            "{}/?api-key={}",
            self.config.endpoints.rpc, self.config.api_key
        );
        let url: Url = Url::parse(&base_url).expect("Failed to parse URL");

        let rpc_request: RpcRequest<R> = RpcRequest::new(method.to_string(), request);
        let rpc_response: RpcResponse<T> = self
            .handler
            .send(Method::POST, url, Some(&rpc_request))
            .await?;

        Ok(rpc_response.result)
    }

    pub async fn get_token_accounts(&self, request: GetTokenAccounts) -> Result<TokenAccountsList> {
        self.post_rpc_request("getTokenAccounts", request).await
    }

    pub async fn get_priority_fee_estimate(
        &self,
        request: GetPriorityFeeEstimateRequest,
    ) -> Result<GetPriorityFeeEstimateResponse> {
        self.post_rpc_request("getPriorityFeeEstimate", vec![request])
            .await
    }
}

/// Configuration settings for the Helius client
///
/// `Config` contains all the necessary parameters needed to configure and authenticate the `Helius` client to interact with a specific Solana cluster
#[derive(Clone)]
pub struct HeliusConfig {
    /// The API key used for authenticating requests
    pub api_key: String,
    /// The target Solana cluster the client will interact with
    pub cluster: Cluster,
    /// The endpoints associated with the specified `cluster`. Note these endpoints are automatically determined based on the cluster to ensure requests
    /// are made to the correct cluster
    pub endpoints: HeliusEndpoints,
}

impl HeliusConfig {
    pub fn new(api_key: &str, cluster: Cluster) -> Result<Self> {
        if api_key.is_empty() {
            return Err(HeliusError::InvalidInput(
                "API key cannot be empty".to_string(),
            ));
        }

        let endpoints: HeliusEndpoints = HeliusEndpoints::for_cluster(&cluster);

        Ok(HeliusConfig {
            api_key: api_key.to_string(),
            cluster,
            endpoints,
        })
    }
}
