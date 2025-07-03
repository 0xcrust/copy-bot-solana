use crate::swap::execution::{
    ComputeUnitLimits, PriorityFeeConfig, QuoteConfigOverrides, SwapConfig, SwapConfigOverrides,
    SwapExecutionMode, SwapExecutor, SwapInput, SwapQuote, SwapTransaction, TransactionVariant,
};
use crate::swap::jupiter::swap_api::{
    ComputeUnitPriceMicroLamports, JupiterSwapApiClient, PrioritizationFeeLamports, QuoteRequest,
    QuoteResponse, SwapMode, SwapRequest, TransactionConfig,
};
use crate::swap::utils::get_mint_accounts;
use std::sync::Arc;

use anyhow::anyhow;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;

/// Note: Jupiter API limits are about 60-80 requests per minute
#[derive(Clone)]
pub struct JupiterExecutor {
    client: JupiterSwapApiClient,
    rpc_client: Arc<RpcClient>,
    options: JupiterExecutorOpts,
}

impl SwapExecutor for JupiterExecutor {
    type Quote = ExecutorQuoteResponse;

    async fn quote(
        &self,
        swap_input: &SwapInput,
        overrides: Option<&QuoteConfigOverrides>,
    ) -> anyhow::Result<Self::Quote> {
        let QuoteConfigOverrides {
            as_legacy_transaction,
            include_dexes,
            exclude_dexes,
            max_accounts,
            only_direct_routes,
        } = self.resolve_quote_config(overrides);

        let quote_request = QuoteRequest {
            input_mint: swap_input.input_token_mint,
            output_mint: swap_input.output_token_mint,
            amount: swap_input.amount,
            swap_mode: Some(match swap_input.mode {
                SwapExecutionMode::ExactIn => SwapMode::ExactIn,
                SwapExecutionMode::ExactOut => SwapMode::ExactOut,
            }),
            slippage_bps: swap_input.slippage_bps,
            platform_fee_bps: None,
            dexes: include_dexes,
            exclude_dexes,
            only_direct_routes,
            as_legacy_transaction,
            max_accounts,
            quote_type: None,
        };
        // log::info!("Quote request: {:#?}", quote_request);
        let quote = self
            .client
            .quote(&quote_request)
            .await
            .map_err(|e| anyhow!("Error getting Jupiter quote: {}", e))?;

        let mut mint_accounts = get_mint_accounts(
            &self.rpc_client,
            &[swap_input.input_token_mint, swap_input.output_token_mint],
        )
        .await?;
        let input_token_decimals = mint_accounts.remove(0).map(|acc| acc.decimals);
        let output_token_decimals = mint_accounts.remove(0).map(|acc| acc.decimals);

        return Ok(ExecutorQuoteResponse {
            input_token_decimals,
            output_token_decimals,
            q: quote,
            amount_specified_is_input: swap_input.mode.amount_specified_is_input(),
        });
    }

    async fn swap_instructions(
        &self,
        input_pubkey: Pubkey,
        output: Self::Quote,
        overrides: Option<&SwapConfigOverrides>,
    ) -> anyhow::Result<Vec<Instruction>> {
        let swap_request = self.build_swap_request(input_pubkey, output, overrides);
        let swap_response = self
            .client
            .swap_instructions(&swap_request)
            .await
            .map_err(|e| anyhow!("Error getting Jupiter swap transaction: {}", e))?;
        let mut instructions = Vec::new();
        if let Some(instruction) = swap_response.token_ledger_instruction {
            instructions.push(instruction)
        }
        instructions.extend(swap_response.compute_budget_instructions);
        instructions.extend(swap_response.setup_instructions);
        instructions.push(swap_response.swap_instruction);
        if let Some(instruction) = swap_response.cleanup_instruction {
            instructions.push(instruction)
        }

        Ok(instructions)
    }

    async fn swap_transaction(
        &self,
        input_pubkey: Pubkey,
        output: Self::Quote,
        overrides: Option<&SwapConfigOverrides>,
    ) -> anyhow::Result<SwapTransaction> {
        let swap_request = self.build_swap_request(input_pubkey, output, overrides);
        let swap_response = self
            .client
            .swap(&swap_request)
            .await
            .map_err(|e| anyhow!("Error getting Jupiter swap transaction: {}", e))?;
        return Ok(SwapTransaction {
            transaction: TransactionVariant::Bytes(swap_response.swap_transaction),
            last_valid_blockheight: swap_response.last_valid_block_height,
        });
    }

    fn get_config(&self) -> SwapConfig {
        SwapConfig {
            priority_fee: self.options.priority_fee,
            cu_limits: self.options.cu_limits,
            wrap_and_unwrap_sol: Some(self.options.wrap_and_unwrap_sol),
            as_legacy_transaction: self.options.as_legacy_transaction,
        }
    }

    fn set_config(&mut self, config: &SwapConfig) {
        self.options.priority_fee = config.priority_fee;
        self.options.cu_limits = config.cu_limits;
        if let Some(param) = config.wrap_and_unwrap_sol {
            self.options.wrap_and_unwrap_sol = param;
        }
        self.options.as_legacy_transaction = config.as_legacy_transaction;
    }
}

impl JupiterExecutor {
    pub fn new(base_url: Option<String>, rpc_client: Arc<RpcClient>) -> Self {
        JupiterExecutor {
            client: JupiterSwapApiClient::new(base_url),
            rpc_client,
            options: JupiterExecutorOpts::default(),
        }
    }
    pub fn new_with_options(
        base_url: Option<String>,
        rpc_client: Arc<RpcClient>,
        options: JupiterExecutorOpts,
    ) -> Self {
        JupiterExecutor {
            client: JupiterSwapApiClient::new(base_url),
            rpc_client,
            options,
        }
    }

    fn resolve_quote_config(
        &self,
        overrides: Option<&QuoteConfigOverrides>,
    ) -> QuoteConfigOverrides {
        if let Some(o) = overrides {
            QuoteConfigOverrides {
                as_legacy_transaction: o
                    .as_legacy_transaction
                    .or(self.options.as_legacy_transaction),
                max_accounts: o.max_accounts.clone().or(self.options.max_accounts.clone()),
                include_dexes: o
                    .include_dexes
                    .clone()
                    .or(self.options.include_dexes.clone()),
                exclude_dexes: o
                    .exclude_dexes
                    .clone()
                    .or(self.options.exclude_dexes.clone()),
                only_direct_routes: o.only_direct_routes.or(self.options.only_direct_routes),
            }
        } else {
            QuoteConfigOverrides {
                as_legacy_transaction: self.options.as_legacy_transaction,
                max_accounts: self.options.max_accounts,
                include_dexes: self.options.include_dexes.clone(),
                exclude_dexes: self.options.exclude_dexes.clone(),
                only_direct_routes: self.options.only_direct_routes,
            }
        }
    }

    fn resolve_swap_config(&self, overrides: Option<&SwapConfigOverrides>) -> SwapConfigOverrides {
        if let Some(o) = overrides {
            SwapConfigOverrides {
                priority_fee: o.priority_fee.or(self.options.priority_fee),
                cu_limits: o.cu_limits.or(self.options.cu_limits),
                wrap_and_unwrap_sol: o
                    .wrap_and_unwrap_sol
                    .or(Some(self.options.wrap_and_unwrap_sol)),
                use_token_ledger: o.use_token_ledger.or(Some(self.options.use_token_ledger)),
                use_shared_accounts: o
                    .use_shared_accounts
                    .or(Some(self.options.use_shared_accounts)),
                destination_token_account: o
                    .destination_token_account
                    .or(self.options.destination_token_account),
                as_legacy_transaction: o
                    .as_legacy_transaction
                    .or(self.options.as_legacy_transaction),
                max_accounts: o.max_accounts.clone().or(self.options.max_accounts.clone()),
                include_dexes: o
                    .include_dexes
                    .clone()
                    .or(self.options.include_dexes.clone()),
                exclude_dexes: o
                    .exclude_dexes
                    .clone()
                    .or(self.options.exclude_dexes.clone()),
                only_direct_routes: o.only_direct_routes.or(self.options.only_direct_routes),
            }
        } else {
            SwapConfigOverrides {
                priority_fee: self.options.priority_fee,
                cu_limits: self.options.cu_limits,
                wrap_and_unwrap_sol: Some(self.options.wrap_and_unwrap_sol),
                use_token_ledger: Some(self.options.use_token_ledger),
                use_shared_accounts: Some(self.options.use_shared_accounts),
                destination_token_account: self.options.destination_token_account,
                as_legacy_transaction: self.options.as_legacy_transaction,
                max_accounts: self.options.max_accounts,
                include_dexes: self.options.include_dexes.clone(),
                exclude_dexes: self.options.exclude_dexes.clone(),
                only_direct_routes: self.options.only_direct_routes,
            }
        }
    }

    fn build_swap_request(
        &self,
        input_pubkey: Pubkey,
        quote: ExecutorQuoteResponse,
        overrides: Option<&SwapConfigOverrides>,
    ) -> SwapRequest {
        let resolved = self.resolve_swap_config(overrides);
        SwapRequest {
            user_public_key: input_pubkey,
            quote_response: quote.q,
            config: TransactionConfig {
                wrap_and_unwrap_sol: resolved.wrap_and_unwrap_sol.unwrap_or(true),
                fee_account: None,
                destination_token_account: resolved.destination_token_account,
                compute_unit_price_micro_lamports: match resolved.priority_fee {
                    Some(PriorityFeeConfig::FixedCuPrice(price)) => {
                        Some(ComputeUnitPriceMicroLamports::MicroLamports(price))
                    }
                    _ => None,
                },
                prioritization_fee_lamports: match resolved.priority_fee {
                    Some(PriorityFeeConfig::Dynamic) => Some(PrioritizationFeeLamports::Auto),
                    Some(PriorityFeeConfig::JitoTip(tip)) => {
                        Some(PrioritizationFeeLamports::JitoTipLamports(tip))
                    }
                    Some(PriorityFeeConfig::DynamicMultiplier(multiplier)) => {
                        Some(PrioritizationFeeLamports::AutoMultiplier(multiplier))
                    }
                    _ => None,
                },
                dynamic_compute_unit_limit: resolved
                    .cu_limits
                    .map(|cu| match cu {
                        ComputeUnitLimits::Dynamic => true,
                        ComputeUnitLimits::Fixed(_) => false,
                    })
                    .unwrap_or(false),
                as_legacy_transaction: resolved.as_legacy_transaction.unwrap_or(false),
                use_shared_accounts: resolved.use_shared_accounts.unwrap_or(false),
                use_token_ledger: resolved.use_token_ledger.unwrap_or(false),
            },
        }
    }
}

#[derive(Clone)]
pub struct JupiterExecutorOpts {
    pub include_dexes: Option<Vec<String>>,
    pub exclude_dexes: Option<Vec<String>>,
    pub as_legacy_transaction: Option<bool>,
    pub max_accounts: Option<usize>,
    pub quote_type: Option<String>,
    pub wrap_and_unwrap_sol: bool,
    pub fee_account: Option<Pubkey>,
    pub destination_token_account: Option<Pubkey>,
    pub priority_fee: Option<PriorityFeeConfig>,
    pub cu_limits: Option<ComputeUnitLimits>,
    pub use_shared_accounts: bool,
    pub use_token_ledger: bool,
    pub only_direct_routes: Option<bool>,
}

impl JupiterExecutorOpts {
    pub fn new() -> Self {
        JupiterExecutorOpts::default()
    }
}

impl Default for JupiterExecutorOpts {
    fn default() -> Self {
        JupiterExecutorOpts {
            include_dexes: None,
            exclude_dexes: None,
            as_legacy_transaction: Some(false),
            max_accounts: None,
            quote_type: None,
            wrap_and_unwrap_sol: true,
            fee_account: None,
            destination_token_account: None, // This defaults to the ATA
            priority_fee: Some(PriorityFeeConfig::Dynamic),
            cu_limits: None, // If set to dynamic, it increases latency due to simulation
            use_shared_accounts: false,
            use_token_ledger: false,
            only_direct_routes: Some(false),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ExecutorQuoteResponse {
    input_token_decimals: Option<u8>,
    output_token_decimals: Option<u8>,
    q: QuoteResponse,
    amount_specified_is_input: bool,
}

impl SwapQuote for ExecutorQuoteResponse {
    fn amount(&self) -> u64 {
        self.q.in_amount
    }

    fn amount_specified_is_input(&self) -> bool {
        self.amount_specified_is_input
    }

    fn input_token(&self) -> Pubkey {
        self.q.input_mint
    }

    fn input_token_decimals(&self) -> Option<u8> {
        self.input_token_decimals
    }

    fn other_amount(&self) -> u64 {
        self.q.out_amount
    }

    fn other_amount_threshold(&self) -> u64 {
        self.q.other_amount_threshold
    }

    fn output_token(&self) -> Pubkey {
        self.q.output_mint
    }

    fn output_token_decimals(&self) -> Option<u8> {
        self.output_token_decimals
    }
}
