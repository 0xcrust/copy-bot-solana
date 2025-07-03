use solana_client::rpc_filter::{Memcmp, RpcFilterType};
use solana_sdk::pubkey::Pubkey;

pub const COIN_MINT_OFFSET: usize = (16 * 8)
    + std::mem::size_of::<super::amm_info::StateData>()
    + std::mem::size_of::<raydium_amm::state::Fees>()
    + 32
    + 32;
pub const PC_MINT_OFFSET: usize = COIN_MINT_OFFSET + 32;
pub const OWNER_OFFSET: usize = PC_MINT_OFFSET + (32 * 6) + (8 * 8);

pub const RAYDIUM_V4_SIZE_FILTER: RpcFilterType =
    RpcFilterType::DataSize(std::mem::size_of::<super::amm_info::AmmInfo>() as u64);

/// For a given pair(a, b), we might have to call twice with args (a, b) and (b, a)
///
/// Seems to me like token_a should usually be SOL though?
pub fn get_raydium_v4_pool_by_pair(coin_mint: &Pubkey, pc_mint: &Pubkey) -> Vec<RpcFilterType> {
    vec![
        // RAYDIUM_V4_SIZE_FILTER,
        RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
            COIN_MINT_OFFSET,
            coin_mint.to_bytes().to_vec(),
        )),
        RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
            PC_MINT_OFFSET,
            pc_mint.to_bytes().to_vec(),
        )),
    ]
}

pub fn get_raydium_v4_pool_by_coin_mint(coin_mint: &Pubkey) -> Vec<RpcFilterType> {
    vec![
        // RAYDIUM_V4_SIZE_FILTER,
        RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
            COIN_MINT_OFFSET,
            coin_mint.to_bytes().to_vec(),
        )),
    ]
}

pub fn get_raydium_v4_pool_by_pc_mint(pc_mint: &Pubkey) -> Vec<RpcFilterType> {
    vec![
        // RAYDIUM_V4_SIZE_FILTER,
        RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
            PC_MINT_OFFSET,
            pc_mint.to_bytes().to_vec(),
        )),
    ]
}

pub fn get_raydium_v4_pool_by_owner(owner: &Pubkey) -> Vec<RpcFilterType> {
    vec![
        // RAYDIUM_V4_SIZE_FILTER,
        RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
            OWNER_OFFSET,
            owner.to_bytes().to_vec(),
        )),
    ]
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::constants::mints::*;
    use crate::swap::get_program_accounts_config;
    use crate::swap::raydium::amm::get_pools;
    use solana_client::nonblocking::rpc_client::RpcClient;
    use solana_sdk::commitment_config::CommitmentConfig;
    use solana_sdk::pubkey;

    fn setup() -> RpcClient {
        dotenv::dotenv().unwrap();
        let url = std::env::var("TEST_RPC_URL").unwrap();
        RpcClient::new(url)
    }

    const COMMITMENT: CommitmentConfig = CommitmentConfig::confirmed();
    #[tokio::test]
    async fn test_get_raydium_v4_pools_by_pair_no_size_filter() {
        let client = setup();

        let filter = get_raydium_v4_pool_by_pair(&SOL, &USDC);
        let config = get_program_accounts_config(Some(filter), Some(COMMITMENT));

        let pools = get_pools(&client, config, false).await.unwrap();
        assert!(!pools.is_empty());
        for (_, pool) in pools {
            assert_eq!(pool.coin_vault_mint, SOL);
            assert_eq!(pool.pc_vault_mint, USDC);
        }

        let filter = get_raydium_v4_pool_by_pair(&USDC, &SOL);
        let config = get_program_accounts_config(Some(filter), Some(COMMITMENT));

        let pools = get_pools(&client, config, false).await.unwrap();
        assert!(!pools.is_empty());
        for (_, pool) in pools {
            assert_eq!(pool.coin_vault_mint, USDC);
            assert_eq!(pool.pc_vault_mint, SOL);
        }
    }

    #[tokio::test]
    async fn test_get_raydium_v4_pools_by_pair_with_size_filter() {
        let client = setup();

        let filter = get_raydium_v4_pool_by_pair(&USDT, &SOL);
        let config = get_program_accounts_config(Some(filter), Some(COMMITMENT));

        let pools = get_pools(&client, config, true).await.unwrap();
        assert!(!pools.is_empty());
        for (_, pool) in pools {
            assert_eq!(pool.coin_vault_mint, USDT);
            assert_eq!(pool.pc_vault_mint, SOL);
        }
    }

    #[tokio::test]
    async fn test_get_raydium_v4_pools_by_coin_mint_no_size_filter() {
        let client = setup();
        let filter = get_raydium_v4_pool_by_coin_mint(&USDC);
        let config = get_program_accounts_config(Some(filter), Some(COMMITMENT));

        let pools = get_pools(&client, config, false).await.unwrap();
        assert!(!pools.is_empty());
        for (_, pool) in pools {
            assert_eq!(pool.coin_vault_mint, USDC);
        }
    }

    #[tokio::test]
    async fn test_get_raydium_v4_pools_by_coin_mint_with_size_filter() {
        let client = setup();
        let filter = get_raydium_v4_pool_by_coin_mint(&USDC);
        let config = get_program_accounts_config(Some(filter), Some(COMMITMENT));

        let pools = get_pools(&client, config, true).await.unwrap();
        assert!(!pools.is_empty());
        for (_, pool) in pools {
            assert_eq!(pool.coin_vault_mint, USDC);
        }
    }

    #[tokio::test]
    async fn test_get_raydium_v4_pools_by_pc_mint_no_size_filter() {
        let client = setup();
        let filter = get_raydium_v4_pool_by_pc_mint(&USDT);
        let config = get_program_accounts_config(Some(filter), Some(COMMITMENT));

        let pools = get_pools(&client, config, false).await.unwrap();
        assert!(!pools.is_empty());
        for (_, pool) in pools {
            assert_eq!(pool.pc_vault_mint, USDT);
        }
    }

    #[tokio::test]
    async fn test_get_raydium_v4_pools_by_pc_mint_with_size_filter() {
        let client = setup();
        let filter = get_raydium_v4_pool_by_pc_mint(&USDT);
        let config = get_program_accounts_config(Some(filter), Some(COMMITMENT));

        let pools = get_pools(&client, config, true).await.unwrap();
        assert!(!pools.is_empty());
        for (_, pool) in pools {
            assert_eq!(pool.pc_vault_mint, USDT);
        }
    }

    const SOL_BRO_POOL_OWNER: Pubkey = pubkey!("GThUX1Atko4tqhN2NaiTazWSeFWMuiUvfFnyJyUghFMJ");
    #[tokio::test]
    #[ignore]
    async fn test_get_raydium_v4_pools_by_owner_no_size_filter() {
        let client = setup();
        let filter = get_raydium_v4_pool_by_owner(&SOL_BRO_POOL_OWNER);
        let config = get_program_accounts_config(Some(filter), Some(COMMITMENT));

        let pools = get_pools(&client, config, false).await.unwrap();
        assert!(!pools.is_empty());
        for (_, pool) in pools {
            assert_eq!(pool.amm_owner, SOL_BRO_POOL_OWNER);
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_raydium_v4_pools_by_owner_with_size_filter() {
        let client = setup();
        let filter = get_raydium_v4_pool_by_owner(&SOL_BRO_POOL_OWNER);
        let config = get_program_accounts_config(Some(filter), Some(COMMITMENT));

        let pools = get_pools(&client, config, true).await.unwrap();
        assert!(!pools.is_empty());
        for (_, pool) in pools {
            assert_eq!(pool.amm_owner, SOL_BRO_POOL_OWNER);
        }
    }
}
