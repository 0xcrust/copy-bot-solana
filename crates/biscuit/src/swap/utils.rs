use futures::StreamExt;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::program_pack::Pack;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use spl_token::state::Mint;

pub async fn get_multiple_accounts(
    rpc_client: &RpcClient,
    accounts: &[Pubkey],
) -> anyhow::Result<Vec<Option<Account>>> {
    let chunks = accounts.chunks(100).into_iter();
    let mut futures = futures::stream::FuturesOrdered::new();
    for chunk in chunks {
        futures.push_back(async {
            let accounts = rpc_client.get_multiple_accounts(chunk).await?;
            Ok::<_, anyhow::Error>(accounts)
        })
    }
    let mut accounts_vec = Vec::with_capacity(accounts.len());
    while let Some(result) = futures.next().await {
        accounts_vec.extend(result?);
    }

    Ok(accounts_vec)
}

pub async fn get_mint_accounts(
    rpc_client: &RpcClient,
    mints: &[Pubkey],
) -> anyhow::Result<Vec<Option<Mint>>> {
    let accounts = get_multiple_accounts(rpc_client, mints).await?;
    let accounts = accounts
        .into_iter()
        .map(|opt_account| opt_account.and_then(|acc| Mint::unpack(&acc.data[0..Mint::LEN]).ok()))
        .collect::<Vec<_>>();
    Ok(accounts)
}
