use dashmap::DashMap;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

#[derive(Clone)]
pub struct WalletBalances {
    wallets: Arc<DashMap<Pubkey, Option<u64>>>,
}

impl WalletBalances {
    pub fn register_wallet(&self, address: &Pubkey) -> Option<u64> {
        self.wallets.insert(*address, None).flatten()
    }

    pub fn deregister_wallet(&self, address: &Pubkey) -> Option<u64> {
        self.wallets.remove(address).map(|m| m.1).flatten()
    }
}

pub fn start_balance_polling_task(
    rpc_client: Arc<RpcClient>,
    refresh_frequency_ms: u64,
) -> (WalletBalances, tokio::task::JoinHandle<anyhow::Result<()>>) {
    let wallets = Arc::new(DashMap::<Pubkey, Option<u64>>::new());
    let jh = tokio::task::spawn({
        let wallets = Arc::clone(&wallets);
        let rpc_client = rpc_client;
        async move {
            loop {
                let keys = wallets.iter().map(|kv| *kv.key()).collect::<Vec<_>>();
                let result = super::utils::get_multiple_account_data(&rpc_client, &keys).await;
                let accounts = match result {
                    Ok(accounts) => accounts,
                    Err(e) => {
                        log::error!("Failed making `getMultipleAccounts` rpc request in balance polling task");
                        continue;
                    }
                };

                for (account, wallet) in accounts.into_iter().zip(keys) {
                    let Some(account) = account else {
                        continue;
                    };
                    wallets.insert(wallet, Some(account.lamports));
                }
            }
        }
    });

    let balances = WalletBalances { wallets };
    (balances, jh)
}
