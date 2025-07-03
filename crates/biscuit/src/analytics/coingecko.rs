use anyhow::anyhow;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Client, ClientBuilder};
use serde::{Deserialize, Serialize};

const DEFAULT_BASE_URL: &str = "https://api.coingecko.com/api/v3";

#[derive(Clone)]
pub struct CoinGecko {
    client: Client,
    api_key: String,
    base_url: String,
}

impl CoinGecko {
    pub fn new(api_key: String, base_url: Option<String>) -> anyhow::Result<Self> {
        let mut headers = HeaderMap::with_capacity(1);
        headers.insert("x-cg-demo-api-key", HeaderValue::from_str(&api_key)?);
        let client = ClientBuilder::new().default_headers(headers).build()?;
        Ok(CoinGecko {
            client,
            api_key,
            base_url: base_url.unwrap_or(DEFAULT_BASE_URL.to_string()),
        })
    }

    pub async fn get_historical_price_raw(
        &self,
        coin: &str,
        from: i64,
        to: i64,
        vs_currency: Option<&str>,
        precision: Option<u8>, // max is 18
    ) -> anyhow::Result<CoinGeckoPrice> {
        #[derive(Serialize)]
        struct Query {
            vs_currency: String,
            from: i64,
            to: i64,
            precision: Option<u8>,
        }
        let query_str = serde_qs::to_string(&Query {
            vs_currency: vs_currency.unwrap_or("usd").to_string(),
            from,
            to,
            precision,
        })?;

        let response = self
            .client
            .get(format!(
                "{}/coins/{}/market_chart/range?{}",
                self.base_url, coin, query_str
            ))
            .send()
            .await?;

        Ok(handle_response_or_error(response).await?)
    }
}

#[derive(Debug, Deserialize)]
pub struct CoinGeckoPrice {
    pub prices: Vec<(i64, f64)>,
    pub market_caps: Vec<(i64, f64)>,
    pub total_volumes: Vec<(i64, f64)>,
}

async fn handle_response_or_error<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, anyhow::Error> {
    let json = response.json::<serde_json::Value>().await?;
    // println!("json: {:#?}", json);
    let message = json.get("message").and_then(|v| v.as_str());

    if let Some(message) = message {
        return Err(anyhow!(message.to_string()));
    }

    Ok(serde_json::from_value(json)?)
}
