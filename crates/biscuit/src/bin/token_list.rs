use biscuit::swap::jupiter::{
    token_list_new::{self, TokenTag as TokenTagNew},
    token_list_old,
};
use biscuit::swap::raydium::api::v3::{
    response::{ApiV3ClmmPool, ApiV3StandardPool, ApiV3StandardPoolKeys},
    ApiV3Client, PoolFetchParams, PoolSort, PoolSortOrder, PoolType,
};
use log::info;

const OUT_PATH_1: &str = "artifacts/token_list/lst_community.json";
const OUT_PATH_2: &str = "artifacts/token_list/all.json";

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    dotenv::dotenv()?;
    env_logger::init();

    let task_a = tokio::task::spawn(async {
        let token_list =
            token_list_new::fetch_token_list(vec![TokenTagNew::Lst, TokenTagNew::Community])
                .await?;
        let size = std::mem::size_of::<token_list_new::JupApiToken>() * token_list.len();
        info!(
            "Token_list[Lst, Community]: length={}, size: {}kilobytes",
            token_list.len(),
            size / 1024
        );
        std::fs::write(OUT_PATH_1, serde_json::to_string_pretty(&token_list)?)?;
        Ok::<_, anyhow::Error>(())
    });

    let task_b = tokio::task::spawn(async {
        let token_list = token_list_new::fetch_token_list(vec![]).await?;
        let size = std::mem::size_of::<token_list_new::JupApiToken>() * token_list.len();
        info!(
            "Token_list[All]: length={}, size: {}kilobytes",
            token_list.len(),
            size / 1024
        );
        std::fs::write(OUT_PATH_2, serde_json::to_string_pretty(&token_list)?)?;
        Ok::<_, anyhow::Error>(())
    });

    futures::future::join_all(vec![task_a, task_b]).await;

    Ok(())
}

// [2024-08-19T12:37:36Z INFO  token_list] Token_list[Lst, Community]: length=1787, size: 432kilobytes
