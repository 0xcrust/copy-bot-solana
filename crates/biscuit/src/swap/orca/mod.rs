pub mod api;
pub mod executor;
mod filter;
pub mod quote;
pub mod utils;

use crate::swap::get_program_accounts_config;
use anchor_lang::AccountDeserialize;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcProgramAccountsConfig;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use whirlpool::state::Whirlpool;

pub async fn get_pool_for_pair(
    client: &RpcClient,
    token_a: &Pubkey,
    token_b: &Pubkey,
    commitment: Option<CommitmentConfig>,
) -> anyhow::Result<Vec<(Pubkey, Whirlpool)>> {
    let filter = filter::get_whirlpool_by_pair(token_a, token_b);
    let config = get_program_accounts_config(Some(filter), commitment);
    get_whirlpools(client, config, false).await
}

pub async fn get_whirlpools(
    client: &RpcClient,
    mut config: RpcProgramAccountsConfig,
    with_size_filter: bool,
) -> anyhow::Result<Vec<(Pubkey, Whirlpool)>> {
    let size_before = config.filters.as_ref().map(|f| f.len()).unwrap_or(0);
    if with_size_filter {
        let mut f = config.filters.unwrap_or_default();
        f.push(filter::ORCA_POOL_SIZE_FILTER);
        config.filters = Some(f);
        debug_assert!(config.filters.as_ref().unwrap().len() == size_before + 1);
    }
    let accounts = client
        .get_program_accounts_with_config(&whirlpool::ID, config)
        .await?;
    Ok(accounts
        .iter()
        .map(|(key, acc)| {
            (
                *key,
                Whirlpool::try_deserialize(&mut &acc.data[..]).unwrap(),
            )
        })
        .collect())
}
