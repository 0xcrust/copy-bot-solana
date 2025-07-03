use anyhow::Context;
use futures::StreamExt;
use log::error;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Client, ClientBuilder};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone)]
pub struct Birdeye {
    client: Client,
    api_key: String,
    base_url: String,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: T,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct ApiError {
    pub success: bool,
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for ApiError {}

const DEFAULT_BASE_URL: &str = "https://public-api.birdeye.so";

impl Birdeye {
    pub fn new(
        api_key: String,
        base_url: Option<String>,
        chain: Option<Chain>,
    ) -> anyhow::Result<Self> {
        let mut headers = HeaderMap::with_capacity(2);
        headers.insert("X-API-KEY", HeaderValue::from_str(&api_key)?);
        headers.insert(
            "X-CHAIN",
            HeaderValue::from_str(&chain.unwrap_or(Chain::Solana).to_string())?,
        );
        let client = ClientBuilder::new().default_headers(headers).build()?;
        Ok(Birdeye {
            client,
            api_key,
            base_url: base_url.unwrap_or(DEFAULT_BASE_URL.to_string()),
        })
    }
}

// note: queries are in snake-case
impl Birdeye {
    // GET
    // https://public-api.birdeye.so/defi/networks
    pub async fn get_supported_networks_defi(&self) -> anyhow::Result<Vec<Chain>> {
        let response = self
            .client
            .get(format!("{}/defi/networks", self.base_url))
            .send()
            .await?;

        let response = handle_response_or_error(response).await?;
        Ok(response.data)
    }

    // GET
    // https://public-api.birdeye.so/defi/price?address=So11111111111111111111111111111111111111112
    // headers: x-chain (defaults to solana): a chain-name listed in supported networks
    // Get price update of a token
    pub async fn get_price(
        &self,
        token: String,
        check_liquidity: Option<u64>, // a number like 100
        include_liquidity: Option<bool>,
    ) -> anyhow::Result<Price> {
        #[derive(Serialize)]
        struct Query {
            address: String,
            check_liquidity: Option<u64>,
            include_liquidity: Option<bool>,
        }
        let query_str = serde_qs::to_string(&Query {
            address: token,
            check_liquidity,
            include_liquidity,
        })?;

        let response = self
            .client
            .get(format!("{}/defi/price?{}", self.base_url, query_str))
            .send()
            .await?;
        let response = handle_response_or_error(response).await?;
        Ok(response.data)
    }

    // GET
    // https://public-api.birdeye.so/defi/multi_price?list_address=So11111111111111111111111111111111111111112%2CmSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So
    // headers: x-chain (defaults to solana): a chain-name listed in supported networks
    /// Get price updates of multiple tokens in a single API call. Maximum 100 tokens
    ///
    /// The `check-liquidity` makes the query only consider markets that meet a certain liquidity threshold
    /// If the specified threshold is too high, some return objects will return `null` for the `value` field
    pub async fn get_price_multiple(
        &self,
        tokens: &[String],
        check_liquidity: Option<u64>,
        include_liquidity: Option<bool>,
    ) -> anyhow::Result<HashMap<String, Option<Price>>> {
        #[derive(Serialize)]
        struct Query {
            list_address: String,
            check_liquidity: Option<u64>,
            include_liquidity: Option<bool>,
        }
        let query_str = serde_qs::to_string(&Query {
            list_address: tokens.join(","),
            check_liquidity,
            include_liquidity,
        })?;

        let response = self
            .client
            .get(format!("{}/defi/multi_price?{}", self.base_url, query_str))
            .send()
            .await?;

        Ok(handle_response_or_error(response).await?.data)
    }

    // POST : { list_address: String } (comma-separated string)
    // https://public-api.birdeye.so/defi/multi_price
    // headers: x-chain (defaults to solana): a chain-name listed in supported networks
    /// Get price updates of multiple tokens in a single API call. Maximum 100 tokens
    ///
    /// The `check-liquidity` makes the query only consider markets that meet a certain liquidity threshold
    /// If the specified threshold is too high, some return objects will return `null` for the `value` field
    pub async fn get_price_multiple2(
        &self,
        tokens: Vec<String>,
        check_liquidity: Option<u64>,
        include_liquidity: Option<bool>,
    ) -> anyhow::Result<HashMap<String, Price>> {
        #[derive(Serialize)]
        struct Query {
            check_liquidity: Option<u64>,
            include_liquidity: Option<bool>,
        }
        #[derive(Serialize)]
        struct Param {
            list_address: String,
        }

        let query_str = serde_qs::to_string(&Query {
            check_liquidity,
            include_liquidity,
        })?;

        let param = Param {
            list_address: tokens.join(","),
        };

        let response = self
            .client
            .post(format!("{}/defi/multi_price?{}", self.base_url, query_str))
            .json(&param)
            .send()
            .await?;

        let response = handle_response_or_error(response).await?;
        Ok(response.data)
    }

    // GET
    // headers: x-chain (defaults to solana): a chain-name listed in supported networks
    // https://public-api.birdeye.so/defi/history_price?address=So11111111111111111111111111111111111111112&address_type=token&type=15m
    /// Get historical price line chart of a token
    pub async fn get_price_historical(
        &self,
        token: String,                            // address
        address_type: HistoricalPriceAddressType, // "token" or "pair",
        timeframe: TimeFrame,                     // type
        time_from: i64, // Specify the start time using Unix timestamps in seconds
        time_to: i64,   // Specify the end time using Unix timestamps in seconds
    ) -> anyhow::Result<HistoricalPriceList> {
        #[derive(Serialize)]
        struct Query {
            address: String,
            address_type: HistoricalPriceAddressType,
            #[serde(rename = "type")]
            timeframe: TimeFrame,
            time_from: i64,
            time_to: i64,
        }

        let query_str = serde_qs::to_string(&Query {
            address: token,
            address_type,
            timeframe,
            time_from,
            time_to,
        })?;

        let response = self
            .client
            .get(format!(
                "{}/defi/history_price?{}",
                self.base_url, query_str
            ))
            .send()
            .await?;

        let response = handle_response_or_error(response).await?;
        Ok(response.data)
    }

    // GET
    // headers: x-chain (defaults to solana): a chain-name listed in supported networks
    // https://public-api.birdeye.so/defi/historical_price_unix?address=So11111111111111111111111111111111111111112
    /// Get historical price by unix timestamp
    pub async fn get_price_historical_by_unix_time(
        &self,
        token: String,          // address
        unix_time: Option<i64>, // unixtime: query param, should this really be optional?
    ) -> anyhow::Result<Option<HistoricalPriceByUnixTime>> {
        #[derive(Serialize)]
        struct Query {
            address: String,
            unixtime: Option<i64>,
        }
        let query_str = serde_qs::to_string(&Query {
            address: token,
            unixtime: unix_time,
        })?;

        let response = self
            .client
            .get(format!(
                "{}/defi/historical_price_unix?{}",
                self.base_url, query_str
            ))
            .send()
            .await?;

        let response = handle_response_or_error(response).await?;
        Ok(response.data)
    }

    // GET
    // headers: x-chain (defaults to solana): a chain-name listed in supported networks
    // https://public-api.birdeye.so/defi/txs/token?address=So11111111111111111111111111111111111111112&offset=0&limit=50&tx_type=swap&sort_type=desc
    /// Get list of trades of a certain token
    pub async fn get_trades_for_token(
        &self,
        token: String,            // address(required)
        offset: Option<u16>,      // offset(Optional) integer 0 to 50_000. Defaults to 0
        limit: Option<u8>,        // limit(optional) integer 1 to 50 defaults to 50
        tx_type: TransactionType, // docs say default is swap but it is required
        sort_type: SortType,      // docs say default is desc but it is required
    ) -> anyhow::Result<PaginatedResponse<TokenTransaction>> {
        #[derive(Serialize)]
        struct Query {
            address: String,
            tx_type: TransactionType,
            offset: Option<u16>,
            limit: Option<u8>,
            sort_type: SortType,
        }
        let query_str = serde_qs::to_string(&Query {
            address: token,
            offset,
            limit,
            tx_type,
            sort_type,
        })?;

        let response = self
            .client
            .get(format!("{}/defi/txs/token?{}", self.base_url, query_str))
            .send()
            .await?;

        let response = handle_response_or_error(response).await?;
        Ok(response.data)
    }

    /// Get OHLCV price of a token
    pub async fn get_ohlcv(&self) {
        unimplemented!()
    }

    /// Get OHLCV price of a pair
    pub async fn get_ohlcv_pair(&self) {
        unimplemented!()
    }

    /// Get OHLCV price of a base-quote pair
    pub async fn get_ohlcv_base_quote(&self) {
        unimplemented!()
    }

    // GET
    // headers: x-chain (defaults to solana): a chain-name listed in supported networks
    // https://public-api.birdeye.so/defi/price_volume/single?address=So11111111111111111111111111111111111111112&type=24h
    /// Get price and volume updates of a token
    pub async fn get_price_volume_single(
        &self,
        token: String,                   // address: required
        timeframe: PriceVolumeTimeFrame, // type: Only 1h, 2h, 4h, 8h, 24h allowed
    ) -> anyhow::Result<PriceVolume> {
        #[derive(Serialize)]
        struct Query {
            address: String,
            timeframe: PriceVolumeTimeFrame,
        }
        let query_str = serde_qs::to_string(&Query {
            address: token,
            timeframe,
        })?;

        let response = self
            .client
            .get(format!(
                "{}/defi/price_volume/single?{}",
                self.base_url, query_str
            ))
            .send()
            .await?;

        let response = handle_response_or_error(response).await?;
        Ok(response.data)
    }

    // POST; { list_address: string(comma-separated), type: TimeFrame }
    // headers: x-chain (defaults to solana): a chain-name listed in supported networks
    // https://public-api.birdeye.so/defi/price_volume/multi
    /// Get price and volume updates of maximum 50 tokens
    pub async fn get_price_volume_multiple(
        &self,
        tokens: Vec<String>,
        timeframe: PriceVolumeTimeFrame,
    ) -> anyhow::Result<HashMap<String, PriceVolume>> {
        #[derive(Serialize)]
        struct Param {
            list_address: String,
            timeframe: PriceVolumeTimeFrame,
        }
        let param = Param {
            list_address: tokens.join(","),
            timeframe,
        };

        let response = self
            .client
            .post(format!("{}/defi/price_volume/multi", self.base_url))
            .json(&param)
            .send()
            .await?;

        let response = handle_response_or_error(response).await?;
        Ok(response.data)
    }

    // GET
    // https://public-api.birdeye.so/defi/tokenlist?sort_by=v24hChangePercent&sort_type=desc&offset=0&limit=50&min_liquidity=100
    // headers: x-chain (defaults to solana): a chain-name listed in supported networks
    /// Get token list of any supported chains
    pub async fn get_token_list(
        &self,
        sort_by: TokenListSort,     // v24hUSD or v24hChangePercent, required
        sort_type: SortType,        // required,
        offset: Option<u64>,        // Defaults to 0
        limit: Option<u8>,          // 1 to 50. Defaults to 50
        min_liquidity: Option<f64>, // number >=0, defaults to 100
    ) -> anyhow::Result<TokenListResponse<ListToken<TokenListTokenExt>>> {
        #[derive(Serialize)]
        struct Query {
            sort_by: TokenListSort,
            offset: Option<u64>,
            limit: Option<u8>,
            sort_type: SortType,
            min_liquidity: Option<f64>,
        }
        let query_str = serde_qs::to_string(&Query {
            sort_by,
            sort_type,
            offset,
            limit,
            min_liquidity,
        })?;

        let response = self
            .client
            .get(format!("{}/defi/tokenlist?{}", self.base_url, query_str))
            .send()
            .await?;

        let response = handle_response_or_error(response).await?;
        Ok(response.data)
    }

    // GET
    // https://public-api.birdeye.so/defi/token_security?address=So11111111111111111111111111111111111111112
    // headers: x-chain (defaults to solana): a chain-name listed in supported networks
    /// Get token security of any supported chains
    pub async fn get_token_security(
        &self,
        token: String, // address(required)
    ) -> anyhow::Result<TokenSecurity> {
        let response = self
            .client
            .get(format!(
                "{}/defi/token_security?address={}",
                self.base_url, token
            ))
            .send()
            .await?;

        let response = handle_response_or_error(response).await?;
        Ok(response.data)
    }

    // GET
    // https://public-api.birdeye.so/defi/token_creation_info?address=D7rcV8SPxbv94s3kJETkrfMrWqHFs6qrmtbiu6saaany
    // headers: x-chain (defaults to solana): a chain-name listed in supported networks
    /// Get creation info of token
    pub async fn get_creation_token_info(
        &self,
        token: String,
    ) -> anyhow::Result<CreationTokenInfo> {
        let response = self
            .client
            .get(format!(
                "{}/defi/token_creation_info?address={}",
                self.base_url, token
            ))
            .send()
            .await?;

        let response = handle_response_or_error(response).await?;
        Ok(response.data)
    }

    // GET
    // https://public-api.birdeye.so/defi/token_trending?sort_by=rank&sort_type=asc
    // headers: x-chain (defaults to solana): a chain-name listed in supported networks
    /// Retrieve a dynamic and up-to-date list of trending tokens based on specified sorting criteria.
    pub async fn get_trending_token_list(
        &self,
        sort_by: TrendingTokenSort, // rank || volume24hUSD || liquidity, required, defaults to rank
        sort_type: SortType,        // required, defaults to asc
        offset: Option<u64>,        // not-requird, defaults to 0
        limit: Option<u8>,          // not-required, 1 to 20. defaults to 20
    ) -> anyhow::Result<TokenListResponse<ListToken<TrendingTokenExt>>> {
        #[derive(Serialize)]
        struct Query {
            sort_by: TrendingTokenSort,
            sort_type: SortType,
            offset: Option<u64>,
            limit: Option<u8>,
        }
        let query_str = serde_qs::to_string(&Query {
            sort_by,
            sort_type,
            offset,
            limit,
        })?;
        let response = self
            .client
            .get(format!(
                "{}/defi/token_trending?{}",
                self.base_url, query_str
            ))
            .send()
            .await?;

        let response = handle_response_or_error(response).await?;
        Ok(response.data)
    }

    // POST: No data
    // https://public-api.birdeye.so/defi/v2/tokens/all
    // headers: x-chain (defaults to solana): a chain-name listed in supported networks
    /// This endpoint facilitates the retrieval of a list of tokens on a specified blockchain network. This upgraded version is exclusive to business and enterprise packages. By simply including the header for the requested blockchain without any query parameters, business and enterprise users can get the full list of tokens on the specified blockchain in the URL returned in the response. This removes the need for the limit response of the previous version and reduces the workload of making multiple calls.
    ///
    /// NOTE: This is unauthorized, even for the Premium plan. It's only available for Business
    pub async fn get_token_list_all(
        &self,
    ) -> anyhow::Result<TokenListResponse<ListToken<TrendingTokenExt>>> {
        // unsure if this is the actual response structure
        let response = self
            .client
            .post(format!("{}/defi/v2/tokens/all", self.base_url))
            .send()
            .await?;

        let response = handle_response_or_error(response).await?;
        Ok(response.data)
    }

    // GET:
    // https://public-api.birdeye.so/defi/v2/tokens/new_listing?limit=10
    // headers: x-chain (defaults to solana): a chain-name listed in supported networks
    /// Get newly listed tokens of any supported chains
    pub async fn get_new_token_listings(
        &self,
        end_time: i64,     // Specify the end time using Unix timestamps in seconds
        limit: Option<u8>, // 1 to 10. Defaults to 10
    ) -> anyhow::Result<NewTokenListing> {
        #[derive(Serialize)]
        struct Query {
            end_time: i64,
            limit: Option<u8>,
        }
        let query_str = serde_qs::to_string(&Query { end_time, limit })?;
        let response = self
            .client
            .get(format!(
                "{}/defi/v2/tokens/new_listing?{}",
                self.base_url, query_str
            ))
            .send()
            .await?;

        let response = handle_response_or_error(response).await?;
        Ok(response.data)
    }

    // GET
    // https://public-api.birdeye.so/defi/v2/tokens/top_traders?address=So11111111111111111111111111111111111111112&time_frame=24h&sort_type=desc&sort_by=volume&offset=0&limit=10
    // headers: x-chain (defaults to solana): a chain-name listed in supported networks
    /// Get top traders of given token
    pub async fn get_token_top_traders(
        &self,
        token: String,           // address
        time_frame: V2TimeFrame, // time_frame, required, defaults to 24h, 30m | 1h | 2h | 4h | 6h | 8h | 12h | 24h
        sort_type: SortType,
        sort_by: TokenTopTradersSort, // volume | trade
        offset: Option<u64>,          // optional, defaults to zero
        limit: Option<u8>,            // 1 to 10, defaults to 10
    ) -> anyhow::Result<TokenTopTraders> {
        #[derive(Serialize)]
        struct Query {
            address: String,
            time_frame: V2TimeFrame,
            sort_type: SortType,
            sort_by: TokenTopTradersSort,
            offset: Option<u64>,
            limit: Option<u8>,
        }

        let query_str = serde_qs::to_string(&Query {
            address: token,
            time_frame,
            sort_type,
            sort_by,
            offset,
            limit,
        })?;

        let response = self
            .client
            .get(format!(
                "{}/defi/v2/tokens/top_traders?{}",
                self.base_url, query_str
            ))
            .send()
            .await?;

        let response = handle_response_or_error(response).await?;
        Ok(response.data)
    }

    // GET
    // https://public-api.birdeye.so/defi/v2/markets?address=So11111111111111111111111111111111111111112&time_frame=24h&sort_type=desc&sort_by=liquidity&offset=0&limit=10
    // headers: x-chain (defaults to solana): a chain-name listed in supported networks
    /// The API provides detailed information about the markets for a specific cryptocurrency token on a specified blockchain. Users can retrieve data for one or multiple markets related to a single token. This endpoint requires the specification of a token address and the blockchain to filter results. Additionally, it supports optional query parameters such as offset, limit, and required sorting by liquidity or sort type (ascending or descending) to refine the output.
    pub async fn get_token_markets_list(
        &self,
        token: String,           // address
        time_frame: V2TimeFrame, // time_frame, required, defaults to 24h, 30m | 1h | 2h | 4h | 6h | 8h | 12h | 24h
        sort_type: SortType,
        sort_by: TokenMarketSort, // liquidity | volume24h
        offset: Option<u64>,      // optional, defaults to zero
        limit: Option<u8>,        // 1 to 10, defaults to 10
    ) -> anyhow::Result<TokenMarketList> {
        #[derive(Serialize)]
        struct Query {
            address: String,
            time_frame: V2TimeFrame,
            sort_type: SortType,
            sort_by: TokenMarketSort,
            offset: Option<u64>,
            limit: Option<u8>,
        }

        let query_str = serde_qs::to_string(&Query {
            address: token,
            time_frame,
            sort_type,
            sort_by,
            offset,
            limit,
        })?;

        let response = self
            .client
            .get(format!("{}/defi/v2/markets?{}", self.base_url, query_str))
            .send()
            .await?;

        let response = handle_response_or_error(response).await?;
        Ok(response.data)
    }

    // GET
    // https://public-api.birdeye.so/v1/wallet/list_supported_chain
    pub async fn get_supported_networks_wallet(&self) -> anyhow::Result<Vec<Chain>> {
        let response = self
            .client
            .get(format!("{}/v1/wallet/list_supported_chain", self.base_url))
            .send()
            .await?;

        let response = handle_response_or_error(response).await?;
        Ok(response.data)
    }

    // GET
    // headers: x-chain (defaults to solana): a chain-name listed in supported networks
    // https://public-api.birdeye.so/v1/wallet/token_list?wallet=0xf584f8728b874a6a5c7a8d4d387c9aae9172d621
    pub async fn get_wallet_portfolio(&self, wallet: String) -> anyhow::Result<WalletPortfolio> {
        let response = self
            .client
            .get(format!(
                "{}/v1/wallet/token_list?wallet={}",
                self.base_url, wallet
            ))
            .send()
            .await?;

        let response = handle_response_or_error(response).await?;
        Ok(response.data)
    }

    // GET
    // headers: x-chain (defaults to solana): a chain-name listed in supported networks
    // https://public-api.birdeye.so/v1/wallet/token_balance?wallet=0xf584f8728b874a6a5c7a8d4d387c9aae9172d621&token_address=0xf85fEea2FdD81d51177F6b8F35F0e6734Ce45F5F
    pub async fn get_wallet_token_balance(
        &self,
        wallet: String,
        token_address: String,
    ) -> anyhow::Result<WalletTokenBalance> {
        let response = self
            .client
            .get(format!(
                "{}/v1/wallet/token_balance?wallet={}&token_address={}",
                self.base_url, wallet, token_address
            ))
            .send()
            .await?;

        let response = handle_response_or_error(response).await?;
        Ok(response.data)
    }

    // GET
    // headers: x-chain (defaults to solana): a chain-name listed in supported networks
    // https://public-api.birdeye.so/v1/wallet/tx_list?wallet=0xf584f8728b874a6a5c7a8d4d387c9aae9172d621&limit=100
    pub async fn get_wallet_transaction_history(
        &self,
        wallet: String,
        limit: Option<u32>,     // 1 to 1000. Defaults to 100
        before: Option<String>, // A transaction hash to traverse starting from. Only works with Solana
    ) -> anyhow::Result<HashMap<Chain, Vec<WalletTransaction>>> {
        let query_str = serde_qs::to_string(&WalletHistoryQuery {
            wallet,
            limit,
            before,
        })?;

        let response = self
            .client
            .get(format!("{}/v1/wallet/tx_list?{}", self.base_url, query_str))
            .send()
            .await?;

        let response = handle_response_or_error(response).await?;
        Ok(response.data)
    }

    // GET
    // headers: x-chain (defaults to solana): a chain-name listed in supported networks
    // https://public-api.birdeye.so/v1/wallet/multichain_tx_list?wallet=0xf584f8728b874a6a5c7a8d4d387c9aae9172d621
    pub async fn get_wallet_transaction_history_multichain(
        &self,
        chains: Vec<Chain>,
        wallet: String,
        limit: Option<u32>,     // 1 to 1000. Defaults to 100
        before: Option<String>, // A transaction hash to traverse starting from. Only works with Solana
    ) -> anyhow::Result<HashMap<Chain, Vec<WalletTransaction>>> {
        let query_str = serde_qs::to_string(&WalletHistoryQuery {
            wallet,
            limit,
            before,
        })?;

        let chains = chains
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let mut headers = HeaderMap::with_capacity(2);
        headers.insert("X-API-KEY", HeaderValue::from_str(&self.api_key)?);
        headers.insert("X-CHAINS", HeaderValue::from_str(&chains)?);

        let response = Client::builder()
            .default_headers(headers)
            .build()?
            .get(format!(
                "{}/v1/wallet/multichain_tx_list?{}",
                self.base_url, query_str
            ))
            .send()
            .await?;

        let response = handle_response_or_error(response).await?;
        Ok(response.data)
    }
}

async fn handle_response_or_error<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<ApiResponse<T>, anyhow::Error> {
    let json = response.json::<serde_json::Value>().await?;
    // log::info!("Json: {:#?}", json);
    let success = json
        .get("success")
        .and_then(|v| v.as_bool())
        .context("Invalid birdeye api response")?;
    if success {
        Ok(serde_json::from_value::<ApiResponse<T>>(json)?)
    } else {
        Err(serde_json::from_value::<ApiError>(json)?.into())
    }
}

#[derive(Serialize)]
struct WalletHistoryQuery {
    pub wallet: String,
    pub limit: Option<u32>,
    pub before: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenMarketSort {
    Liquidity,
    Volume24h,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenTopTradersSort {
    Volume,
    Trade,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum V2TimeFrame {
    #[serde(rename = "30m")]
    _30M,
    #[serde(rename = "1h")]
    _1H,
    #[serde(rename = "2h")]
    _2H,
    #[serde(rename = "4h")]
    _4H,
    #[serde(rename = "6h")]
    _6H,
    #[serde(rename = "8h")]
    _8H,
    #[serde(rename = "12h")]
    _12H,
    #[serde(rename = "24h")]
    _24H,
}

#[derive(Serialize)]
pub enum TrendingTokenSort {
    #[serde(rename = "rank")]
    Rank,
    #[serde(rename = "volume24hUSD")]
    Volume24hUSD,
    #[serde(rename = "liquidity")]
    Liquidity,
}

#[derive(Serialize)]
pub enum TokenListSort {
    #[serde(rename = "v24hUSD")]
    V24hUSD,
    #[serde(rename = "v24hChangePercent")]
    V24hChangePercent,
}

#[derive(Clone, Debug, Serialize)]
pub enum PriceVolumeTimeFrame {
    #[serde(rename = "1h")]
    _1H,
    #[serde(rename = "2h")]
    _2H,
    #[serde(rename = "4h")]
    _4H,
    #[serde(rename = "8h")]
    _8H,
    #[serde(rename = "24h")]
    _24H,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoricalPriceAddressType {
    Token,
    Pair,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletTransaction {
    pub tx_hash: String,
    pub block_number: u64,
    pub block_time: String, // Date
    pub status: bool,
    pub from: String,
    pub to: String,
    pub gas_used: Option<u64>,  // is this a thing?
    pub gas_price: Option<u64>, // is this a thing?
    pub fee: u64,               // u64?
    pub fee_usd: Option<f64>,
    pub value: Option<String>, // u64? is this a thing?
    pub contract_label: ContractLabel,
    pub main_action: String, // e.g "call"
    pub balance_change: Vec<BalanceChange>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractLabel {
    pub address: String,
    pub name: Option<String>,
    pub metadata: Metadata,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Metadata {
    pub icon: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceChange {
    pub name: Option<String>,
    pub symbol: Option<String>,
    #[serde(rename = "logo_uri")]
    pub logo_uri: Option<String>,
    pub address: String,
    pub amount: i64,
    pub decimals: u8,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletPortfolio {
    pub wallet: String,
    pub total_usd: f64,
    pub items: Vec<WalletPortfolioBalance>,
}

pub type WalletPortfolioBalance = WalletBalance<PortfolioBalanceExt>;
pub type WalletTokenBalance = Option<WalletBalance<()>>;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletBalance<T> {
    pub address: String,
    pub decimals: u8,
    pub balance: u64, // u64 string?
    pub ui_amount: f64,
    pub chain_id: Chain, // like ethereum
    #[serde(rename = "logo_uri")]
    pub logo_uri: Option<String>,
    pub price_usd: f64,
    pub value_usd: f64,
    #[serde(flatten)]
    pub ext: T,
}
#[derive(Clone, Debug, Deserialize)]
pub struct PortfolioBalanceExt {
    pub name: String,
    pub symbol: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TokenMarketList {
    pub items: Vec<TokenMarket>,
    pub total: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenMarket {
    pub address: String,
    pub base: MarketToken,
    pub created_at: String, // Date!
    pub liquidity: f64,
    pub liquidity_change_percentage_24h: Option<f64>,
    pub name: String,
    pub price: Option<f64>,
    pub quote: MarketToken,
    pub source: String,
    pub trade_24h: f64,                        // u64?
    pub trade_24h_change_percent: Option<f64>, // can be negative!
    pub unique_wallet_24h: u64,
    pub unique_wallet_24h_change_percent: Option<f64>, // can be negative!
    pub volume_24h: f64,
    pub volume_24h_change_percentage_24h: Option<f64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MarketToken {
    pub address: String,
    pub decimals: u8,
    pub symbol: String,
    pub icon: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TokenTopTraders {
    pub items: Vec<TokenTrader>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenTrader {
    pub token_address: String,
    pub owner: String,
    pub tags: Vec<String>,
    #[serde(rename = "type")]
    pub timeframe: String, // 24h, what else??
    pub volume: f64,
    pub trade: f64,
    pub trade_buy: u64,
    pub trade_sell: u64,
    pub volume_buy: f64,
    pub volume_sell: f64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct NewTokenListing {
    pub items: Vec<TokenListing>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenListing {
    pub address: String,
    pub symbol: Option<String>,
    pub name: Option<String>,
    pub decimals: u8,
    pub liquidity_added_at: String, // Date!
    #[serde(rename = "logo_uri")]
    pub logo_uri: Option<String>,
    pub liquidity: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreationTokenInfo {
    pub tx_hash: String,
    pub slot: u64,
    pub token_address: String,
    pub decimals: u8,
    pub owner: String,
    pub block_unix_time: i64,
    pub block_human_time: String, // Date
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenSecurity {
    pub creator_address: Option<String>,
    pub creator_owner_address: Option<String>,
    pub owner_address: Option<String>,
    pub owner_of_owner_address: Option<String>,
    pub creation_tx: Option<String>,
    pub creation_time: Option<i64>, // timestamp?,
    pub creation_slot: Option<u64>,
    pub mint_tx: Option<String>,
    pub mint_time: Option<i64>, // timestamp?
    pub mint_slot: Option<u64>,
    pub creator_balance: Option<f64>,    // ??
    pub owner_balance: Option<u64>,      // ??
    pub owner_percentage: Option<f64>,   // ??
    pub creator_percentage: Option<f64>, // ??
    pub metaplex_update_authority: String,
    pub metaplex_owner_update_authority: Option<String>,
    pub metaplex_update_authority_balance: f64,
    pub metaplex_update_authority_percent: f64,
    pub mutable_metadata: bool,
    pub top_10_holder_balance: f64,
    pub top_10_holder_percent: f64,
    pub top_10_user_balance: f64,
    pub top_10_user_percent: f64,
    pub is_true_token: Option<bool>,
    pub total_supply: f64,
    pub pre_market_holder: Vec<String>, // default
    // lock_info: Option<T>, // what???
    pub freezeable: Option<bool>,
    pub freeze_authority: Option<String>,
    pub transfer_fee_enable: Option<bool>,
    // transfer_fee_data: Option<T>, // what??
    pub is_token_2022: bool,
    pub non_transferable: Option<bool>, // is this bool
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenListResponse<T> {
    pub update_unix_time: i64,
    pub update_time: String, // Date!
    pub tokens: Vec<T>,
    pub total: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListToken<T> {
    pub address: String,
    pub decimals: u8,
    #[serde(rename = "logoURI")]
    pub logo_uri: String,
    pub name: Option<String>,
    pub symbol: Option<String>,

    #[serde(flatten)]
    pub extra: T,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenListTokenExt {
    pub last_trade_unix_time: i64,
    #[serde(rename = "mc")]
    pub market_cap: Option<f64>,
    #[serde(rename = "v24hUSD")]
    pub volume_24h_usd: f64,
    #[serde(rename = "v24hChangePercent")]
    pub volume_24h_change_percentage: Option<f64>, // can be negative!
}
#[derive(Clone, Debug, Deserialize)]
pub struct TrendingTokenExt {
    #[serde(rename = "volume24hUSD")]
    pub volume_24h_usd: f64,
    pub rank: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceVolume {
    pub price: f64,
    pub update_unix_time: i64,
    pub update_human_time: String, // Date
    #[serde(rename = "volumeUSD")]
    pub volume_usd: f64,
    pub volume_change_percent: f64, // could be negative!!
    pub price_change_percent: f64,  // could be negative!!
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub has_next: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum TokenTransaction {
    Swap(TokenTransactionBase<SwapTransaction>),
    Liquidity(TokenTransactionBase<LiquidityTransaction>),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenTransactionBase<T> {
    pub tx_hash: String,
    pub source: String, // "raydium", what else?
    pub block_unix_time: i64,
    pub alias: Option<String>, // ??
    pub owner: Option<String>,
    pub pool_id: String,
    #[serde(flatten)]
    pub data: T,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapTransaction {
    pub quote: Token,
    pub base: Token,
    pub base_price: Option<f64>,  // ??
    pub quote_price: Option<f64>, // ??
    pub tx_type: TransactionType,
    pub side: SwapSide,
    pub price_pair: f64,
    pub from: Token,
    pub to: Token,
    pub token_price: Option<f64>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    pub symbol: String,
    pub decimals: u8,
    pub address: String,
    #[serde(
        deserialize_with = "crate::utils::serde_helpers::field_as_string_or_self::deserialize"
    )]
    pub amount: f64,
    pub fee_info: Option<String>, // ??
    pub ui_amount: f64,
    pub price: Option<f64>, // ??
    pub nearest_price: Option<f64>,
    #[serde(
        deserialize_with = "crate::utils::serde_helpers::field_as_string_or_self::deserialize"
    )]
    pub change_amount: f64,
    pub ui_change_amount: f64, // can be negative!!
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SwapSide {
    Sell,
    Buy,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiquidityTransaction {
    tokens: Vec<AddLiqTransactionToken>, // just make this a vec?
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddLiqTransactionToken {
    pub symbol: String,
    pub decimals: u8,
    pub address: String,
    #[serde(
        deserialize_with = "crate::utils::serde_helpers::field_as_string_or_self::deserialize"
    )]
    pub amount: u64,
    pub ui_amount: f64,
}

#[derive(Clone, Debug, Deserialize, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TransactionType {
    #[default]
    Swap,
    Add,
    Remove,
    All,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SortType {
    #[default]
    Desc,
    Asc,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HistoricalPriceList {
    pub items: Vec<HistoricalPrice>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalPriceByUnixTime {
    pub value: f64,
    pub update_unix_time: i64,
    pub price_change_24h: Option<f64>, // can be negative! should it be less
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalPrice {
    pub unix_time: i64,
    pub value: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Price {
    pub value: Option<f64>,        // 142.50778419462551
    pub update_unix_time: i64,     // 1728402103
    pub update_human_time: String, // "2024-10-08T15:41:43"
    pub liquidity: Option<f64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Chain {
    #[default]
    Solana,
    Ethereum,
    Arbitrum,
    Avalanche,
    Bsc,
    Optimism,
    Polygon,
    Base,
    Zksync,
    Sui,
    #[serde(untagged)] // #[serde(other)]
    Unknown(String),
}
impl std::fmt::Display for Chain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let chain_str = match self {
            Chain::Solana => "solana",
            Chain::Ethereum => "ethereum",
            Chain::Arbitrum => "arbitrum",
            Chain::Avalanche => "avalanche",
            Chain::Bsc => "bsc",
            Chain::Optimism => "optimism",
            Chain::Polygon => "polygon",
            Chain::Base => "base",
            Chain::Zksync => "zksync",
            Chain::Sui => "sui",
            Chain::Unknown(catch_all) => &catch_all,
        };
        f.write_str(&chain_str)
    }
}
// impl std::fmt::Display for Chain {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         f.write_str(&serde_json::to_string(self).map_err(|_| std::fmt::Error)?.trim_matches('"'))
//     }
// }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TimeFrame {
    #[serde(rename = "1m")]
    _1Min,
    #[serde(rename = "3m")]
    _3Min,
    #[serde(rename = "5m")]
    _5Min,
    #[serde(rename = "15m")]
    _15Min,
    #[serde(rename = "30m")]
    _30Min,
    #[serde(rename = "1H")]
    _1Hour,
    #[serde(rename = "2H")]
    _2Hour,
    #[serde(rename = "4H")]
    _4Hour,
    #[serde(rename = "6H")]
    _6Hour,
    #[serde(rename = "8H")]
    _8Hour,
    #[serde(rename = "12H")]
    _12Hour,
    #[serde(rename = "1D")]
    _1Day,
    #[serde(rename = "3D")]
    _3Day,
    #[serde(rename = "1W")]
    _1Week,
    #[serde(rename = "1M")]
    _1Month,
}

// impl std::fmt::Display for TimeFrame {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         f.write_str(&serde_json::to_string(self).map_err(|_| std::fmt::Error)?)
//     }
// }

// https://public-api.birdeye.so/defi/networks

impl Birdeye {
    pub async fn make_chunked_price_request_usd(&self, ids: &[String]) -> HashMap<String, Price> {
        if ids.is_empty() {
            return HashMap::default();
        }
        let chunks = ids.chunks(100).into_iter();
        let mut tasks = futures::stream::FuturesUnordered::new();
        for chunk in chunks {
            tasks.push({
                let client = self.clone();
                async move {
                    let prices = client.get_price_multiple(chunk, None, None).await?;
                    Ok::<_, anyhow::Error>(prices)
                }
            });
        }

        let mut ret = HashMap::<String, Option<Price>>::default();
        while let Some(prices) = tasks.next().await {
            match prices {
                Ok(prices) => ret.extend(prices.into_iter()),
                Err(e) => error!("Error making price request: {}", e),
            }
        }

        ret.into_iter().filter_map(|p| Some((p.0, p.1?))).collect()
    }
}
