use std::future::Future;

use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::VersionedTransaction;

pub trait SwapExecutor {
    type Quote: SwapQuote;

    fn quote(
        &self,
        input: &SwapInput,
        overrides: Option<&QuoteConfigOverrides>,
    ) -> impl Future<Output = anyhow::Result<Self::Quote>> + Send;

    fn swap_transaction(
        &self,
        input_pubkey: Pubkey,
        output: Self::Quote,
        overrides: Option<&SwapConfigOverrides>,
    ) -> impl Future<Output = anyhow::Result<SwapTransaction>> + Send;

    fn swap_instructions(
        &self,
        input_pubkey: Pubkey,
        output: Self::Quote,
        overrides: Option<&SwapConfigOverrides>,
    ) -> impl Future<Output = anyhow::Result<Vec<Instruction>>> + Send;

    fn set_config(&mut self, config: &SwapConfig);

    fn get_config(&self) -> SwapConfig;
}

pub struct SwapTransaction {
    pub transaction: TransactionVariant,
    pub last_valid_blockheight: u64,
}

#[derive(Debug)]
pub enum TransactionVariant {
    Bytes(Vec<u8>),
    Versioned(VersionedTransaction),
}

impl TransactionVariant {
    pub fn into_raw(self) -> anyhow::Result<Vec<u8>> {
        Ok(match self {
            TransactionVariant::Bytes(vec) => vec,
            TransactionVariant::Versioned(v) => bincode::serialize(&v)?,
        })
    }

    pub fn into_versioned_tx(self) -> anyhow::Result<VersionedTransaction> {
        Ok(match self {
            TransactionVariant::Bytes(vec) => bincode::deserialize(&vec)?,
            TransactionVariant::Versioned(v) => v,
        })
    }
}

pub trait SwapQuote {
    fn input_token(&self) -> Pubkey;
    fn output_token(&self) -> Pubkey;
    fn input_token_decimals(&self) -> Option<u8>;
    fn output_token_decimals(&self) -> Option<u8>;
    fn amount(&self) -> u64;
    fn other_amount(&self) -> u64;
    fn other_amount_threshold(&self) -> u64;
    fn amount_specified_is_input(&self) -> bool;
}

#[derive(Copy, Clone, Debug)]
pub struct SwapConfig {
    pub priority_fee: Option<PriorityFeeConfig>,
    pub cu_limits: Option<ComputeUnitLimits>,
    pub wrap_and_unwrap_sol: Option<bool>,
    pub as_legacy_transaction: Option<bool>,
}

#[derive(Clone, Debug, Default)]
pub struct QuoteConfigOverrides {
    pub as_legacy_transaction: Option<bool>,
    pub include_dexes: Option<Vec<String>>,
    pub exclude_dexes: Option<Vec<String>>,
    pub max_accounts: Option<usize>,
    pub only_direct_routes: Option<bool>,
}

#[derive(Clone, Debug, Default)]
pub struct SwapConfigOverrides {
    pub priority_fee: Option<PriorityFeeConfig>,
    pub cu_limits: Option<ComputeUnitLimits>,
    pub wrap_and_unwrap_sol: Option<bool>,
    pub use_shared_accounts: Option<bool>,
    pub use_token_ledger: Option<bool>,
    pub destination_token_account: Option<Pubkey>,
    pub as_legacy_transaction: Option<bool>,
    pub include_dexes: Option<Vec<String>>,
    pub exclude_dexes: Option<Vec<String>>,
    pub max_accounts: Option<usize>,
    pub only_direct_routes: Option<bool>,
}

#[derive(Copy, Clone, Debug, Default)]
pub enum ComputeUnitLimits {
    #[default]
    Dynamic,
    Fixed(u64),
}

#[derive(Copy, Clone, Debug)]
pub enum PriorityFeeConfig {
    //#[default]
    Dynamic,
    DynamicMultiplier(u64),
    FixedCuPrice(u64),
    JitoTip(u64),
}

#[derive(Copy, Clone, Debug)]
pub struct SwapInput {
    pub input_token_mint: Pubkey,
    pub output_token_mint: Pubkey,
    pub slippage_bps: u16,
    pub amount: u64,
    pub mode: SwapExecutionMode,
    pub market: Option<Pubkey>,
}

#[derive(Copy, Clone, Debug)]
pub enum SwapExecutionMode {
    ExactIn,
    ExactOut,
}
impl SwapExecutionMode {
    pub fn amount_specified_is_input(&self) -> bool {
        matches!(self, SwapExecutionMode::ExactIn)
    }
}
