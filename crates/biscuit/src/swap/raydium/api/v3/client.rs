use super::response::{ApiV3PoolsPage, ApiV3Token, ApiV3TokenInfo, ApiV3TokenList};
use super::{get_response_or_error, ApiV3Result, PoolFetchParams, PoolType};
use serde::de::DeserializeOwned;
use solana_sdk::pubkey::Pubkey;

#[derive(Clone, Debug)]
pub struct ApiV3Client {
    base_url: String,
}

impl Default for ApiV3Client {
    fn default() -> Self {
        ApiV3Client {
            base_url: Self::DEFAULT_BASE_URL.to_string(),
        }
    }
}

impl ApiV3Client {
    const DEFAULT_BASE_URL: &'static str = "https://api-v3.raydium.io";

    const JUP_TOKEN_LIST: &'static str = "https://tokens.jup.ag/tokens?tags=lst,community";
    const MINT_INFO_ID: &'static str = "/mint/ids";
    const TOKEN_LIST: &'static str = "/mint/list";

    const POOL_LIST: &'static str = "/pools/info/list";
    const POOL_SEARCH_BY_ID: &'static str = "/pools/info/ids";
    const POOL_SEARCH_BY_MINT: &'static str = "/pools/info/mint";
    const POOL_KEY_BY_ID: &'static str = "/pools/key/ids";

    pub fn new(base_url: Option<String>) -> Self {
        ApiV3Client {
            base_url: base_url.unwrap_or(Self::DEFAULT_BASE_URL.to_string()),
        }
    }

    pub async fn get_token_list(&self) -> ApiV3Result<ApiV3TokenList> {
        let url = format!("{}{}", &self.base_url, Self::TOKEN_LIST);
        Ok(get_response_or_error(reqwest::get(url).await?.json().await?)?.data)
    }

    pub async fn get_jup_token_list(&self) -> ApiV3Result<Vec<ApiV3Token>> {
        Ok(reqwest::get(Self::JUP_TOKEN_LIST).await?.json().await?)
    }

    pub async fn get_token_info<'a>(
        &self,
        mints: impl Iterator<Item = &'a Pubkey>,
    ) -> ApiV3Result<ApiV3TokenInfo> {
        let mints = mints
            .map(|key| key.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let url = format!("{}{}?mints={}", &self.base_url, Self::MINT_INFO_ID, mints);
        println!("url: {}", url);
        Ok(get_response_or_error(reqwest::get(url).await?.json().await?)?.data)
    }

    pub async fn get_pool_list<T: DeserializeOwned>(
        &self,
        page: usize,
        pool_type: PoolType,
    ) -> ApiV3Result<ApiV3PoolsPage<T>> {
        let url = format!(
            "{}{}?poolType={}&poolSortField={}&sortType={}&page={}&pageSize={}",
            &self.base_url,
            Self::POOL_LIST,
            pool_type,
            "liquidity",
            "desc",
            page,
            100
        );
        println!("url: {}", url);
        Ok(get_response_or_error(reqwest::get(url).await?.json().await?)?.data)
    }

    pub async fn fetch_pools_by_ids<'a, T: DeserializeOwned, S: std::fmt::Display + 'a>(
        &self,
        ids: impl Iterator<Item = &'a S>,
    ) -> ApiV3Result<Vec<T>> {
        let ids = ids.map(|id| id.to_string()).collect::<Vec<_>>().join(",");
        let url = format!("{}{}?ids={}", &self.base_url, Self::POOL_SEARCH_BY_ID, ids);
        Ok(get_response_or_error(reqwest::get(url).await?.json().await?)?.data)
    }

    pub async fn fetch_pool_keys_by_ids<'a, T: DeserializeOwned, S: std::fmt::Display + 'a>(
        &self,
        ids: impl Iterator<Item = &'a S>,
    ) -> ApiV3Result<Vec<T>> {
        let ids = ids.map(|id| id.to_string()).collect::<Vec<_>>().join(",");
        let url = format!("{}{}?ids={}", &self.base_url, Self::POOL_KEY_BY_ID, ids);
        Ok(get_response_or_error(reqwest::get(url).await?.json().await?)?.data)
    }

    pub async fn fetch_pool_by_mints<T: DeserializeOwned>(
        &self,
        mint1: &Pubkey,
        mint2: Option<&Pubkey>,
        params: &PoolFetchParams,
    ) -> ApiV3Result<ApiV3PoolsPage<T>> {
        let url = format!(
            "{}{}?mint1={}&mint2={}&poolType={}&poolSortField={}&sortType={}&pageSize={}&page={}",
            &self.base_url,
            Self::POOL_SEARCH_BY_MINT,
            mint1.to_string(),
            mint2.map(|x| x.to_string()).unwrap_or_default(),
            params.pool_type,
            params.pool_sort,
            params.sort_type,
            100,
            params.page
        );
        Ok(get_response_or_error(reqwest::get(url).await?.json().await?)?.data)
    }
}
