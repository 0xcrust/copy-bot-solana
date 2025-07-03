use anchor_lang::Discriminator;
use raydium_amm_v3::states::PoolState;
use solana_client::rpc_filter::{Memcmp, RpcFilterType};
use solana_sdk::pubkey::Pubkey;

pub const MINT_A_OFFSET: usize = 8 + 65;
pub const MINT_B_OFFSET: usize = 8 + 97;
pub const OWNER_OFFSET: usize = 8 + 33;

pub const RAYDIUM_V6_SIZE_FILTER: RpcFilterType = RpcFilterType::DataSize(PoolState::LEN as u64);

/// This function accepts tokens in any order, and maintains the whirlpools contract's
/// `token_mint_0.key() < token_mint_1.key()` constraint internally
///
/// Also see [PoolState::seeds]
pub fn get_raydium_v6_pool_by_pair(token_a: &Pubkey, token_b: &Pubkey) -> Vec<RpcFilterType> {
    let (token_a, token_b) = if token_a < token_b {
        (token_a, token_b)
    } else {
        (token_b, token_a)
    };
    vec![
        RpcFilterType::Memcmp(Memcmp::new_raw_bytes(0, PoolState::DISCRIMINATOR.to_vec())),
        // RAYDIUM_V6_SIZE_FILTER,
        RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
            MINT_A_OFFSET,
            token_a.to_bytes().to_vec(),
        )),
        RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
            MINT_B_OFFSET,
            token_b.to_bytes().to_vec(),
        )),
    ]
}

pub fn get_raydium_v6_pool_by_token_a(token_a: &Pubkey) -> Vec<RpcFilterType> {
    vec![
        RpcFilterType::Memcmp(Memcmp::new_raw_bytes(0, PoolState::DISCRIMINATOR.to_vec())),
        // RAYDIUM_V6_SIZE_FILTER,
        RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
            MINT_A_OFFSET,
            token_a.to_bytes().to_vec(),
        )),
    ]
}

pub fn get_raydium_v6_pool_by_token_b(token_b: &Pubkey) -> Vec<RpcFilterType> {
    vec![
        RpcFilterType::Memcmp(Memcmp::new_raw_bytes(0, PoolState::DISCRIMINATOR.to_vec())),
        // RAYDIUM_V6_SIZE_FILTER,
        RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
            MINT_B_OFFSET,
            token_b.to_bytes().to_vec(),
        )),
    ]
}

pub fn get_raydium_v6_pool_by_owner(owner: &Pubkey) -> Vec<RpcFilterType> {
    vec![
        RpcFilterType::Memcmp(Memcmp::new_raw_bytes(0, PoolState::DISCRIMINATOR.to_vec())),
        // RAYDIUM_V6_SIZE_FILTER,
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
    use crate::swap::raydium::clmm::get_pools;
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
    async fn test_get_raydium_v6_pools_by_pair_no_size_filter() {
        let client = setup();
        let filter = get_raydium_v6_pool_by_pair(&SOL, &USDC);
        let config = get_program_accounts_config(Some(filter), Some(COMMITMENT));

        let (token_a, token_b) = if SOL < USDC { (SOL, USDC) } else { (USDC, SOL) };
        let pools = get_pools(&client, config, false).await.unwrap();
        assert!(!pools.is_empty());
        for (_, pool) in pools {
            assert_eq!(pool.token_mint_0, token_a);
            assert_eq!(pool.token_mint_1, token_b);
        }
    }

    #[tokio::test]
    async fn test_get_raydium_v6_pools_by_pair_with_size_filter() {
        let client = setup();
        let filter = get_raydium_v6_pool_by_pair(&SOL, &USDC);
        let config = get_program_accounts_config(Some(filter), Some(COMMITMENT));

        let (token_a, token_b) = if SOL < USDC { (SOL, USDC) } else { (USDC, SOL) };
        let pools = get_pools(&client, config, true).await.unwrap();
        assert!(!pools.is_empty());
        for (_, pool) in pools {
            assert_eq!(pool.token_mint_0, token_a);
            assert_eq!(pool.token_mint_1, token_b);
        }
    }

    #[tokio::test]
    async fn test_get_raydium_v6_pools_by_token_a_no_size_filter() {
        let client = setup();
        let filter = get_raydium_v6_pool_by_token_a(&USDC);
        let config = get_program_accounts_config(Some(filter), Some(COMMITMENT));

        let pools = get_pools(&client, config, false).await.unwrap();
        assert!(!pools.is_empty());
        for (_, pool) in pools {
            assert_eq!(pool.token_mint_0, USDC);
        }
    }

    #[tokio::test]
    async fn test_get_raydium_v6_pools_by_token_a_with_size_filter() {
        let client = setup();
        let filter = get_raydium_v6_pool_by_token_a(&USDC);
        let config = get_program_accounts_config(Some(filter), Some(COMMITMENT));

        let pools = get_pools(&client, config, true).await.unwrap();
        assert!(!pools.is_empty());
        for (_, pool) in pools {
            assert_eq!(pool.token_mint_0, USDC);
        }
    }

    #[tokio::test]
    async fn test_get_raydium_v6_pools_by_token_b_no_size_filter() {
        let client = setup();
        let filter = get_raydium_v6_pool_by_token_b(&USDT);
        let config = get_program_accounts_config(Some(filter), Some(COMMITMENT));

        let pools = get_pools(&client, config, false).await.unwrap();
        assert!(!pools.is_empty());
        for (_, pool) in pools {
            assert_eq!(pool.token_mint_1, USDT);
        }
    }

    #[tokio::test]
    async fn test_get_raydium_v6_pools_by_token_b_with_size_filter() {
        let client = setup();
        let filter = get_raydium_v6_pool_by_token_b(&USDT);
        let config = get_program_accounts_config(Some(filter), Some(COMMITMENT));

        let pools = get_pools(&client, config, true).await.unwrap();
        assert!(!pools.is_empty());
        for (_, pool) in pools {
            assert_eq!(pool.token_mint_1, USDT);
        }
    }

    const SOL_USDT_POOL_OWNER: Pubkey = pubkey!("64LoMjcmVq43pTwWKQSQJQzg7Tkw27TtWh6toUxNChch");
    #[tokio::test]
    async fn test_get_raydium_v6_pools_by_owner_no_size_filter() {
        let client = setup();
        let filter = get_raydium_v6_pool_by_owner(&SOL_USDT_POOL_OWNER);
        let config = get_program_accounts_config(Some(filter), Some(COMMITMENT));

        let pools = get_pools(&client, config, false).await.unwrap();
        assert!(!pools.is_empty());
        for (_, pool) in pools {
            assert_eq!(pool.owner, SOL_USDT_POOL_OWNER);
        }
    }

    #[tokio::test]
    async fn test_get_raydium_v6_pools_by_owner_with_size_filter() {
        let client = setup();
        let filter = get_raydium_v6_pool_by_owner(&SOL_USDT_POOL_OWNER);
        let config = get_program_accounts_config(Some(filter), Some(COMMITMENT));

        let pools = get_pools(&client, config, true).await.unwrap();
        assert!(!pools.is_empty());
        for (_, pool) in pools {
            println!("pool: {:#?}", pool);
            assert_eq!(pool.owner, SOL_USDT_POOL_OWNER);
        }
    }
}
