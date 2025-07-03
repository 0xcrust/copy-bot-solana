pub mod executor;
mod filter;
pub mod quote;

use crate::swap::get_program_accounts_config;
use anchor_lang::AccountDeserialize;
use raydium_amm_v3::states::PoolState;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcProgramAccountsConfig;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;

pub async fn get_pool_for_pair(
    client: &RpcClient,
    token_a: &Pubkey,
    token_b: &Pubkey,
    commitment: Option<CommitmentConfig>,
) -> anyhow::Result<(Pubkey, PoolState)> {
    let filter = filter::get_raydium_v6_pool_by_pair(token_a, token_b);
    let config = get_program_accounts_config(Some(filter), commitment);
    get_pools(client, config, false)
        .await?
        .first()
        .copied()
        .ok_or(anyhow::anyhow!("No pool found for pair"))
}

async fn get_pools(
    client: &RpcClient,
    mut config: RpcProgramAccountsConfig,
    with_size_filter: bool,
) -> anyhow::Result<Vec<(Pubkey, PoolState)>> {
    let size_before = config.filters.as_ref().map(|f| f.len()).unwrap_or(0);
    if with_size_filter {
        let mut f = config.filters.unwrap_or_default();
        f.push(filter::RAYDIUM_V6_SIZE_FILTER);
        config.filters = Some(f);
        debug_assert!(config.filters.as_ref().unwrap().len() == size_before + 1);
    }
    let accounts = client
        .get_program_accounts_with_config(&raydium_amm_v3::ID, config)
        .await?;
    Ok(accounts
        .iter()
        .map(|(key, acc)| {
            (
                *key,
                PoolState::try_deserialize(&mut &acc.data[..]).unwrap(),
            )
        })
        .collect())
}
