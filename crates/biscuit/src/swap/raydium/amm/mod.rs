mod amm_info;
pub mod executor;
mod filter;

use crate::swap::get_program_accounts_config;
use futures::stream::StreamExt;
use raydium_amm::state::AmmInfo;
use raydium_library::common::deserialize_account2;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcProgramAccountsConfig;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::{pubkey, pubkey::Pubkey};

pub const RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID: Pubkey =
    pubkey!("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8");

pub async fn get_pool_for_pair(
    client: &RpcClient,
    token_a: &Pubkey,
    token_b: &Pubkey,
    commitment: Option<CommitmentConfig>,
) -> anyhow::Result<(Pubkey, AmmInfo)> {
    let result = futures::stream::iter(vec![(token_a, token_b), (token_b, token_a)])
        .map(|(a, b)| async move {
            let filter = filter::get_raydium_v4_pool_by_pair(a, b);
            let config = get_program_accounts_config(Some(filter.clone()), commitment);
            get_pools(client, config, false)
                .await?
                .first()
                .copied()
                .ok_or(anyhow::anyhow!("No pool found for pair"))
        })
        .buffer_unordered(2)
        .collect::<Vec<_>>()
        .await;
    match (&result[0], &result[1]) {
        (Ok(a), _) => Ok(*a),
        (_, Ok(b)) => Ok(*b),
        (Err(_), Err(_)) => Err(anyhow::anyhow!("No pool found for pair")),
    }
}

pub async fn get_pool_for_pair2(
    client: &RpcClient,
    token_a: &Pubkey,
    token_b: &Pubkey,
    commitment: Option<CommitmentConfig>,
) -> anyhow::Result<(Pubkey, AmmInfo)> {
    let filter = filter::get_raydium_v4_pool_by_pair(token_a, token_b);
    let config = get_program_accounts_config(Some(filter.clone()), commitment);
    let pools = get_pools(client, config, false).await?;
    for (key, _) in &pools {
        log::info!("Found key {} for pool", key);
    }
    Ok(pools[0])
}

pub async fn get_pools(
    client: &RpcClient,
    mut config: RpcProgramAccountsConfig,
    with_size_filter: bool,
) -> anyhow::Result<Vec<(Pubkey, AmmInfo)>> {
    let size_before = config.filters.as_ref().map(|f| f.len()).unwrap_or(0);
    if with_size_filter {
        let mut f = config.filters.unwrap_or_default();
        f.push(filter::RAYDIUM_V4_SIZE_FILTER);
        config.filters = Some(f);
        debug_assert!(config.filters.as_ref().unwrap().len() == size_before + 1);
    }
    let accounts = client
        .get_program_accounts_with_config(&RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID, config)
        .await?;
    Ok(accounts
        .iter()
        .map(|(key, acc)| {
            (
                *key,
                deserialize_account2::<amm_info::AmmInfo, _>(acc, false).unwrap(),
            )
        })
        .collect())
}
