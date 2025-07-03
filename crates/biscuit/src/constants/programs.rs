use solana_program::pubkey;
use solana_sdk::pubkey::Pubkey;

pub const ALL_KNOWN_PROGRAMS: [(&str, Pubkey); 32] = [
    ("System", SYSTEM_PROGRAM_ID),
    ("ComputeBudget", COMPUTE_BUDGET_PROGRAM_ID),
    ("AssociatedToken", SPL_ASSOCIATED_TOKEN_PROGRAM_ID),
    ("Token", SPL_TOKEN_PROGRAM_ID),
    ("Jupiter Aggregator V6", JUPITER_AGGREGATOR_V6_PROGRAM_ID),
    (
        "Raydium Liquidity Pool V4",
        RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID,
    ),
    (
        "Serum Assert Owner(Phantom Deployment)",
        PHANTOM_ASSERT_OWNER_PROGRAM_ID,
    ),
    ("Meteora Vault", METEORA_VAULT_PROGRAM_ID),
    ("Noop", SPL_NOOP_PROGRAM_ID),
    ("Bubblegum", METAPLEX_BUBBLEGUM_PROGRAM_ID),
    ("Meteora DLMM", METEORA_DLMM_PROGRAM_ID),
    ("Account Compression", SPL_ACCOUNT_COMPRESSION_PROGRAM_ID),
    ("Jupiter DCA", JUPITER_DCA_PROGRAM_ID),
    ("Memo V2", SPL_MEMO_V2_PROGRAM_ID),
    ("DLN Destination", DLN_DESTINATION_PROGRAM_ID),
    ("Parcl V3", PARCL_V3_PROGRAM_ID),
    (
        "Raydium Concentrated Liquidity",
        RAYDIUM_CONCENTRATED_LIQUIDITY_PROGRAM_ID,
    ),
    (
        "Raydium Cp Swap",
        RAYDIUM_CP_SWAP
    ),
    ("Lifinity Swap V2", LIFINITY_SWAP_V2_PROGRAM_ID),
    ("OpenbookV2", OPENBOOK_V2_PROGRAM_ID),
    ("Jupiter Governance", JUPITER_GOVERNANCE_PROGRAM_ID),
    ("Invariant Swap", INVARIANT_SWAP_PROGRAM_ID),
    ("Sol Incinerator", SOL_INCINERATOR_PROGRAM_ID),
    ("Pheonix", PHEONIX_PROGRAM_ID),
    ("Flux Beam", FLUX_BEAM_PROGRAM_ID),
    ("Metaplex Candy Guard", METAPLEX_CANDY_GUARD_PROGRAM_ID),
    (
        "Metaplex Token Metadata",
        METAPLEX_TOKEN_METADATA_PROGRAM_ID,
    ),
    ("Meteora Pools", METEORA_POOLS_PROGRAM_ID),
    ("Saber Stable Swap", SABER_STABLE_SWAP_PROGRAM_ID),
    ("Mercurial Stable Swap", MERCURIAL_STABLE_SWAP_PROGRAM_ID),
    ("Metaplex Candy Core", METAPLEX_CANDY_CORE_PROGRAM_ID),
    ("Orca Whirlpools", ORCA_WHIRLPOOLS_PROGRAM_ID),
];

// NATIVE PROGRAMS
pub const COMPUTE_BUDGET_PROGRAM_ID: Pubkey =
    pubkey!("ComputeBudget111111111111111111111111111111");
pub const SYSTEM_PROGRAM_ID: Pubkey = pubkey!("11111111111111111111111111111111");

// SPL PROGRAMS
pub const SPL_ASSOCIATED_TOKEN_PROGRAM_ID: Pubkey =
    pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
pub const SPL_TOKEN_PROGRAM_ID: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
pub const SPL_ACCOUNT_COMPRESSION_PROGRAM_ID: Pubkey =
    pubkey!("cmtDvXumGCrqC1Age74AVPhSRVXJMd8PJS91L8KbNCK");
/// https://github.com/solana-labs/solana-program-library/tree/master/account-compression/programs/noop
pub const SPL_NOOP_PROGRAM_ID: Pubkey = pubkey!("noopb9bkMVfRPU8AsbpTUg8AQkHtKwMYZiFUjNRtMmV");
pub const SPL_MEMO_V2_PROGRAM_ID: Pubkey = pubkey!("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");

// JUPITER PROGRAMS
pub const JUPITER_AGGREGATOR_V6_PROGRAM_ID: Pubkey =
    pubkey!("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4");
pub const JUPITER_DCA_PROGRAM_ID: Pubkey = pubkey!("DCA265Vj8a9CEuX1eb1LWRnDT7uK6q1xMipnNyatn23M");
pub const JUPITER_GOVERNANCE_PROGRAM_ID: Pubkey =
    pubkey!("GovaE4iu227srtG2s3tZzB4RmWBzw8sTwrCLZz7kN7rY");

// RAYDIUM PROGRAMS
pub const RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID: Pubkey =
    pubkey!("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8");
pub const RAYDIUM_CONCENTRATED_LIQUIDITY_PROGRAM_ID: Pubkey =
    pubkey!("CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK");
pub const RAYDIUM_CP_SWAP: Pubkey = pubkey!("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C");

// METAPLEX PROGRAMS
/// https://github.com/metaplex-foundation/mpl-candy-machine/blob/main/programs/candy-guard/README.md
pub const METAPLEX_CANDY_GUARD_PROGRAM_ID: Pubkey =
    pubkey!("Guard1JwRhJkVH6XZhzoYxeBVQe872VH6QggF4BWmS9g");
pub const METAPLEX_TOKEN_METADATA_PROGRAM_ID: Pubkey =
    pubkey!("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s");
pub const METAPLEX_CANDY_CORE_PROGRAM_ID: Pubkey =
    pubkey!("CndyV3LdqHUfDLmE5naZjVN8rBZz4tqhdefbAnjHG3JR");
/// https://developers.metaplex.com/bubblegum
pub const METAPLEX_BUBBLEGUM_PROGRAM_ID: Pubkey =
    pubkey!("BGUMAp9Gq7iTEuizy4pqaxsTyUCBK68MDfK752saRPUY");

/// https://docs.debridge.finance/contracts/mainnet-addresses
pub const DLN_DESTINATION_PROGRAM_ID: Pubkey =
    pubkey!("dst5MGcFPoBeREFAA5E3tU5ij8m5uVYwkzkSAbsLbNo"); // DLN -> DeSwap Liquidity Network
pub const FLUX_BEAM_PROGRAM_ID: Pubkey = pubkey!("FLUXubRmkEi2q6K3Y9kBPg9248ggaZVsoSFhtJHSrm1X");
pub const INVARIANT_SWAP_PROGRAM_ID: Pubkey =
    pubkey!("HyaB3W9q6XdA5xwpU4XnSZV94htfmbmqJXZcEbRaJutt");
pub const LIFINITY_SWAP_V2_PROGRAM_ID: Pubkey =
    pubkey!("2wT8Yq49kHgDzXuPxZSaeLaH1qbmGXtEyPy64bL7aD3c");
pub const MERCURIAL_STABLE_SWAP_PROGRAM_ID: Pubkey =
    pubkey!("MERLuDFBMmsHnsBPZw2sDQZHvXFMwp8EdjudcU2HKky");
pub const METEORA_DLMM_PROGRAM_ID: Pubkey = pubkey!("LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo");
pub const METEORA_POOLS_PROGRAM_ID: Pubkey =
    pubkey!("Eo7WjKq67rjJQSZxS6z3YkapzY3eMj6Xy8X5EQVn5UaB");
pub const METEORA_VAULT_PROGRAM_ID: Pubkey =
    pubkey!("24Uqj9JCLxUeoC3hGfh5W3s9FM9uCHDS2SG3LYwBpyTi");
pub const OPENBOOK_V2_PROGRAM_ID: Pubkey = pubkey!("opnb2LAfJYbRMAHHvqjCwQxanZn7ReEHp1k81EohpZb");
pub const ORCA_WHIRLPOOLS_PROGRAM_ID: Pubkey =
    pubkey!("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc");
pub const PARCL_V3_PROGRAM_ID: Pubkey = pubkey!("3parcLrT7WnXAcyPfkCz49oofuuf2guUKkjuFkAhZW8Y");
/// https://docs.phantom.app/resources/faq#why-does-phantom-prepend-an-additional-instruction-on-standard-spl-token-transfers
pub const PHANTOM_ASSERT_OWNER_PROGRAM_ID: Pubkey =
    pubkey!("DeJBGdMFa1uynnnKiwrVioatTuHmNLpyFKnmB5kaFdzQ");
pub const PHEONIX_PROGRAM_ID: Pubkey = pubkey!("PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY");
pub const SABER_STABLE_SWAP_PROGRAM_ID: Pubkey =
    pubkey!("SSwpkEEcbUqx4vtoEByFjSkhKdCT862DNVb52nZg1UZ");
pub const SOL_INCINERATOR_PROGRAM_ID: Pubkey =
    pubkey!("F6fmDVCQfvnEq2KR8hhfZSEczfM9JK9fWbCsYJNbTGn7");
pub const PUMPFUN_PROGRAM_ID: Pubkey = pubkey!("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"); // Has an IDL
pub const MOONSHOT_PROGRAM_ID: Pubkey = pubkey!("MoonCVVNZFSYkqNXP6bxHLPL6QQJiMagDL3qcqUQTrG"); // Has an IDL
pub const DEGEN_FUND_PROGRAM_ID: Pubkey = pubkey!("degenhbmsyzLpJUwwrzjsPyhPNvLurBFB4k1pBWSoxs"); // No IDL

// stt2KgKH6CJsi4C95dCaeHgvCX2S3gz24pxh3JR42jU
// BSfD6SHZigAfDWSjzD5Q41jw8LmKwtmjskPH9XW1mrRW // photon?
// voTpe3tHQ7AjQHMapgSue2HJFAh2cGsdokqN3XqmVSj // Jupiter voting?
// BSKTvA6XG9QyqhW5Hgq8pG8pm5NnvuYyc4pYefSzM62X // 1 occurence. Not important?
// HQ2UUt18uJqKaQFJhgV9zaTdQxUZjNrsKFgoEDquBkcx
// DiSLRwcSFvtwvMWSs7ubBMvYRaYNYupa76ZSuYLe6D7j // Tensor?
// Ref47T9HMdxRsTDc6PPJWt8msE9UftrXr7Gfpqijkdg // 2 times occured. Not important?
// tro46jTMkb56A3wPepo5HT7JcvX9wFWvR8VaJzgdjEf // Trojan(Unibot)?
// degenhbmsyzLpJUwwrzjsPyhPNvLurBFB4k1pBWSoxs
// 8x8Veat3UDUDERsRxPW3yywN7j1q1y7rMf9LxahwjaEy
// 5tu3xkmLfud5BAwSuQke4WSjoHcQ52SbrPwX9es8j6Ve
// PERPHjGBqRHArX4DySjwM6UJHiR3sWAatqfdBS2qQJu // Jupiter perps?
// 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P(seen in more than 1 wallet)
// 7oyG4wSf2kz2CxTqKTf1uhpPqrw9a8Av1w5t8Uj5PfXb
