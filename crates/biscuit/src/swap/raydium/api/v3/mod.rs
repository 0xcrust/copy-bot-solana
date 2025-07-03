pub mod client;
pub mod response;
pub mod token_list;

pub use client::ApiV3Client;
use response::ApiV3Response;
use serde::{Deserialize, Serialize};

pub type ApiV3Result<T> = std::result::Result<T, anyhow::Error>;

#[derive(Clone, Debug, Deserialize)]
pub struct ApiV3ErrorResponse {
    pub id: String,
    pub success: bool,
    pub msg: String,
}

impl std::fmt::Display for ApiV3ErrorResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "Received error response from API: {}",
            self.msg
        ))
    }
}
impl std::error::Error for ApiV3ErrorResponse {}

fn get_response_or_error<T>(value: serde_json::Value) -> ApiV3Result<ApiV3Response<T>>
where
    T: serde::de::DeserializeOwned,
{
    if let Ok(error) = serde_json::from_value::<ApiV3ErrorResponse>(value.clone()) {
        Err(error.into())
    } else {
        serde_json::from_value(value).map_err(|err| err.into())
    }
}

#[derive(Clone, Debug)]
pub struct PoolFetchParams {
    pub pool_type: PoolType,
    pub pool_sort: PoolSort,
    pub sort_type: PoolSortOrder,
    pub page_size: u16,
    pub page: u16,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum PoolType {
    #[default]
    All,
    Standard,
    Concentrated,
    AllFarm,
    StandardFarm,
    ConcentratedFarm,
}
impl std::fmt::Display for PoolType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PoolType::All => f.write_str("all"),
            PoolType::Standard => f.write_str("standard"),
            PoolType::Concentrated => f.write_str("concentrated"),
            PoolType::AllFarm => f.write_str("allFarm"),
            PoolType::StandardFarm => f.write_str("standardFarm"),
            PoolType::ConcentratedFarm => f.write_str("concentratedFarm"),
        }
    }
}

#[derive(Clone, Debug)]
pub enum PoolSort {
    Liquidity,
    Volume24h,
    Volume7d,
    Volume30d,
    Fee24h,
    Fee7d,
    Fee30d,
    Apr24h,
    Apr7d,
    Apr30d,
}
impl std::fmt::Display for PoolSort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PoolSort::Liquidity => f.write_str("liquidity"),
            PoolSort::Volume24h => f.write_str("volume24h"),
            PoolSort::Volume7d => f.write_str("volume7d"),
            PoolSort::Volume30d => f.write_str("volume30d"),
            PoolSort::Fee24h => f.write_str("fee24h"),
            PoolSort::Fee7d => f.write_str("fee7d"),
            PoolSort::Fee30d => f.write_str("fee30d"),
            PoolSort::Apr24h => f.write_str("apr24h"),
            PoolSort::Apr7d => f.write_str("apr7d"),
            PoolSort::Apr30d => f.write_str("apr30d"),
        }
    }
}

#[derive(Clone, Debug)]
pub enum PoolSortOrder {
    Ascending,
    Descending,
}
impl std::fmt::Display for PoolSortOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PoolSortOrder::Ascending => f.write_str("asc"),
            PoolSortOrder::Descending => f.write_str("desc"),
        }
    }
}

#[cfg(test)]
pub mod raydium_api_v3 {
    use crate::swap::jupiter::token_list_new::{fetch_and_save_token_list, token_list_from_path};
    use crate::swap::raydium::api::v3::response::{
        ApiV3ClmmPool, ApiV3ClmmPoolKeys, ApiV3StandardPool, ApiV3StandardPoolKeys,
    };
    use crate::swap::raydium::api::v3::{
        ApiV3Client, PoolFetchParams, PoolSort, PoolSortOrder, PoolType,
    };

    const TOKEN_COUNT: usize = 20;
    const TOKEN_LIST_PATH: &str = "../../artifacts/token_list.json";

    #[tokio::test]
    pub async fn get_token_list() {
        let client = ApiV3Client::default();
        let _token_list = client.get_token_list().await.unwrap();
    }

    #[tokio::test]
    pub async fn get_all_pools() {
        let client = ApiV3Client::default();
        let _token_list = client
            .get_pool_list::<serde_json::Value>(1, PoolType::All)
            .await
            .unwrap();
    }

    #[tokio::test]
    pub async fn get_token_info() {
        let client = ApiV3Client::default();
        fetch_and_save_token_list(Some(TOKEN_LIST_PATH.to_string()), vec![])
            .await
            .unwrap();
        let tokens = token_list_from_path(Some(TOKEN_LIST_PATH.to_string())).unwrap();
        for mint in tokens.iter().take(TOKEN_COUNT) {
            let _token_info = client
                .get_token_info([&mint.address].into_iter())
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    pub async fn get_standard_pool_and_keys() {
        let client = ApiV3Client::default();
        let params = PoolFetchParams {
            pool_type: PoolType::Standard,
            pool_sort: PoolSort::Liquidity,
            sort_type: PoolSortOrder::Ascending,
            page_size: 100,
            page: 1,
        };

        let tokens = client
            .get_pool_list::<ApiV3StandardPool>(1, PoolType::Standard)
            .await
            .unwrap();
        for pool in tokens.data.iter().take(TOKEN_COUNT) {
            let pools = client
                .fetch_pool_by_mints::<ApiV3StandardPool>(
                    &pool.mint_a.address,
                    Some(&pool.mint_b.address),
                    &params,
                )
                .await
                .unwrap();
            let pool_id = pools.data.get(0).unwrap().id;
            assert_eq!(pool_id, pool.id);
            let _pool = client
                .fetch_pools_by_ids::<ApiV3StandardPool, _>([&pool.id].into_iter())
                .await
                .unwrap();
            let pool_keys = client
                .fetch_pool_keys_by_ids::<ApiV3StandardPoolKeys, _>([&pool.id].into_iter())
                .await
                .unwrap();
            println!("pool keys length: {}", pool_keys.len());
            for pool in pool_keys {
                let lp_token = pool.keys.mint_lp;
                println!("lp_token: {:#?}", lp_token);
            }
        }
    }

    #[tokio::test]
    pub async fn get_clmm_pool_and_keys() {
        let client = ApiV3Client::default();
        let params = PoolFetchParams {
            pool_type: PoolType::Concentrated,
            pool_sort: PoolSort::Liquidity,
            sort_type: PoolSortOrder::Ascending,
            page_size: 100,
            page: 1,
        };
        let tokens = client
            .get_pool_list::<ApiV3ClmmPool>(1, PoolType::Concentrated)
            .await
            .unwrap();
        for pool in tokens.data.iter().take(TOKEN_COUNT) {
            let pools = client
                .fetch_pool_by_mints::<ApiV3ClmmPool>(
                    &pool.mint_a.address,
                    Some(&pool.mint_b.address),
                    &params,
                )
                .await
                .unwrap();
            let pool_id = pools.data.get(0).unwrap().id;
            assert_eq!(pool_id, pool.id);
            let _pool = client
                .fetch_pools_by_ids::<ApiV3ClmmPool, _>([&pool.id].into_iter())
                .await
                .unwrap();
            let _pool_keys = client
                .fetch_pool_keys_by_ids::<ApiV3ClmmPoolKeys, _>([&pool.id].into_iter())
                .await
                .unwrap();
        }
    }
}
