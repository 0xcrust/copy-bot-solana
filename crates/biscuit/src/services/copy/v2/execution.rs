use super::super::config::CopyConfig;
use super::ping::PingTracker;
use super::service::{CopyDetails, CopyMetrics, QuoteDetails};
use crate::constants::mints::{SOL, USDC, USDT};
use crate::core::traits::txn_extensions::TransactionMods;
use crate::parser::swap::TokenDecimals;
use crate::services::copy::config::CopyMode;
use crate::services::copy::history::HistoricalTrades;
use crate::swap::execution::{
    PriorityFeeConfig, SwapConfigOverrides, SwapExecutionMode, SwapExecutor, SwapInput,
};
use crate::swap::jupiter::price::PriceFeed as JupiterPriceFeed;
use crate::utils::fees::PriorityDetails;
use crate::utils::token::get_info_for_mints;
use crate::wire::helius_rpc::HeliusPrioFeesHandle;
use crate::wire::jito::JitoTips;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Error};
use dashmap::DashSet;
use log::{debug, error, info};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::system_instruction::SystemInstruction;
use solana_sdk::transaction::VersionedTransaction;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::RwLock;

const PRICE_TTL: Duration = Duration::from_secs(150);

pub enum ExecutionOutput {
    Success((CopyDetails, VersionedTransaction)),
    Failure(CopyDetails),
}

#[derive(Clone)]
pub struct ExecutionHandler<T> {
    /// The execution wallet
    pub keypair: Arc<Keypair>,

    /// Updated by a ws subscription to the Jito tip stream
    pub jito_tips: Arc<RwLock<JitoTips>>,

    /// Config for execution
    pub config: Arc<CopyConfig>,

    /// Shared rpc client
    pub rpc_client: Arc<RpcClient>,

    /// The trade history for the copy service
    pub trade_history: HistoricalTrades,

    /// The ping tracker for the copy service
    pub ping_tracker: Option<PingTracker>,

    /// Handle to get priority fees for transactions
    pub priofees_handle: Option<HeliusPrioFeesHandle>,

    /// The swap executor
    pub swap_executor: T,

    // Used to return an execution output to the main handler
    // pub execution_output_sender: UnboundedSender<anyhow::Result<Option<(CopyDetails, VersionedTransaction)>>>,
    pub execution_output_sender: UnboundedSender<ExecutionOutput>,

    /// Price feed from Jupiter's price API
    pub price_feed: JupiterPriceFeed,

    /// Whether or not buys are disabled
    pub buy_disabled: Arc<AtomicBool>,

    pub metrics: Arc<CopyMetrics>,

    pub buy_disabled_wallets: Arc<DashSet<Pubkey>>,
    // The wallet's balance
    // pub wallet_balance: Arc<AtomicU64>,
}

#[derive(Copy, Clone, Debug)]
pub enum CopyType {
    /// Origin wallet sold a token we copied it for so we sell it too
    Sell,
    /// Origin wallet bought a token we copied it for so we buy it too
    Buy,
    /// Origin wallet sold a token for another token
    Both,
}

pub enum ExecutionCommand {
    /// Command to execute an item
    Execute(CopyDetails),
}

impl<T> ExecutionHandler<T>
where
    T: SwapExecutor + Clone + Send + Sync + 'static,
    T::Quote: Clone + Send,
{
    pub fn handle_command(&self, cmd: ExecutionCommand) {
        match cmd {
            ExecutionCommand::Execute(details) => {
                let handler = self.clone();
                let metrics = Arc::clone(&self.metrics);
                let max_attempts = self.config.execution_retries;
                tokio::task::spawn({
                    let handler = handler.clone();
                    let mut details = details.clone();
                    let metrics = metrics.clone();
                    async move {
                        let execution_input = handler.get_execution_input(&mut details).await?;
                        let Some(execution_input) = execution_input else {
                            if details.is_first_execution() {
                                metrics.skipped_copy_txns.fetch_add(1, Ordering::Relaxed);
                            }
                            return Ok::<_, anyhow::Error>(());
                        };

                        // Process the swap with retries
                        let mut attempts = 0;
                        loop {
                            attempts += 1;
                            let origin_signature = details.origin_signature;
                            let first_execution = details.is_first_execution();
                            match handler
                                .process_execution_input(&execution_input, &mut details)
                                .await
                            {
                                Err(e) => {
                                    debug!(
                                        "Attempt {} of {}. Exec task failed with err: {}",
                                        attempts, max_attempts, e
                                    );
                                    if attempts < max_attempts {
                                        tokio::time::sleep(Duration::from_millis(
                                            2_u64.pow(attempts as u32) * 1000,
                                        ))
                                        .await;
                                        continue;
                                    } else {
                                        log::error!("Execution failed: Exec task failed after {} attempts: {}", attempts, e);
                                        if let Err(e) = handler
                                            .execution_output_sender
                                            .send(ExecutionOutput::Failure(details))
                                        {
                                            error!("Failed to send execution output: {}", e);
                                        }
                                        break;
                                    }
                                }
                                Ok(result) => {
                                    if let Err(e) = handler
                                        .execution_output_sender
                                        .send(ExecutionOutput::Success((details, result)))
                                    {
                                        error!("Failed to send execution output: {}", e);
                                    }
                                    break;
                                }
                            }
                        }
                        return Ok(());
                    }
                });
            }
        }
    }

    async fn process_ping(&self, details: &mut CopyDetails) -> anyhow::Result<Option<SwapInput>> {
        let Some(ping_tracker) = self.ping_tracker.as_ref() else {
            return Err(anyhow!("Tried to process ping but ping-tracker is not set"));
        };
        let pinged = ping_tracker
            .insert_swap(
                details.origin_signer,
                details.origin_signature,
                &details.origin_swap,
                &self.price_feed,
            )
            .await?;

        // Detect sells
        let Some(balance) = details.origin_input_mint_balance else {
            return Err(anyhow!(
                "Got null for origin input balance. mint:{}",
                details.origin_swap.input_token_mint
            ));
        };
        let token_input = ping_tracker.get_token_input(
            &details.origin_swap.input_token_mint,
            &details.origin_signer,
            details.origin_swap.input_amount,
            balance.post,
        );

        // Detect buys
        let global_buys_disabled = self.buy_disabled.load(Ordering::Relaxed);
        let wallet_buys_disabled = self.buy_disabled_wallets.contains(&details.origin_signer);
        let buy_disabled = global_buys_disabled || wallet_buys_disabled;
        let input_amount = if pinged && !buy_disabled {
            ping_tracker.get_lamports_input(&details.origin_swap.output_token_mint)
        } else {
            None
        };

        let (input_token_mint, output_token_mint, amount, copy_type) =
            match (token_input, input_amount) {
                (Some(token_in), Some(lamports_in)) => (
                    details.origin_swap.input_token_mint,
                    details.origin_swap.output_token_mint,
                    token_in,
                    CopyType::Both,
                ),
                (Some(token_in), None) => (
                    details.origin_swap.input_token_mint,
                    SOL,
                    token_in,
                    CopyType::Sell,
                ),
                (None, Some(lamports_in)) => (
                    SOL,
                    details.origin_swap.output_token_mint,
                    lamports_in,
                    CopyType::Buy,
                ),
                (None, None) => return Ok(None),
            };
        details.copy_type = Some(copy_type);

        Ok(input_amount.map(|amount| SwapInput {
            input_token_mint,
            output_token_mint,
            slippage_bps: self.config.slippage_bps,
            amount,
            mode: SwapExecutionMode::ExactIn,
            market: None,
        }))
    }

    pub async fn get_execution_input(
        &self,
        details: &mut CopyDetails,
    ) -> anyhow::Result<Option<SwapInput>> {
        let mut mint_info = get_info_for_mints(
            &self.rpc_client,
            &[
                details.origin_swap.input_token_mint,
                details.origin_swap.output_token_mint,
            ],
        )
        .await?;
        let output_mint_info = mint_info.pop().context("output mint info not fetched")?;
        let input_mint_info = mint_info.pop().context("input mint info not fetched")?;
        let mut token_decimals = None;
        if input_mint_info.is_some() && output_mint_info.is_some() {
            token_decimals = Some(TokenDecimals {
                input: input_mint_info.as_ref().unwrap().decimals,
                output: output_mint_info.as_ref().unwrap().decimals,
            })
        }

        details.origin_swap.input_mint_info = input_mint_info.map(Into::into);
        details.origin_swap.output_mint_info = output_mint_info.map(Into::into);
        details.origin_swap.token_decimals = token_decimals;

        if details.origin_swap.input_token_mint == details.origin_swap.output_token_mint {
            log::warn!(
                "Skipping copy-txn {}. Input and output mints are the same: {}",
                details.origin_signature,
                details.origin_swap.input_token_mint,
            );
            return Ok(None);
        }

        let execution_input = match details.wallet_config.mode {
            CopyMode::Direct => self.process_direct(details).await,
            CopyMode::Ping => self.process_ping(details).await,
        }?;

        let Some(execution_input) = execution_input else {
            return Ok(None);
        };

        self.price_feed
            .subscribe_token(&execution_input.input_token_mint, Some(PRICE_TTL));
        self.price_feed
            .subscribe_token(&details.origin_swap.input_token_mint, Some(PRICE_TTL));
        self.price_feed
            .subscribe_token(&execution_input.output_token_mint, Some(PRICE_TTL));
        self.price_feed
            .subscribe_token(&details.origin_swap.output_token_mint, Some(PRICE_TTL));

        debug!("execution input amount: {}", execution_input.amount);
        if execution_input.amount == 0 {
            error!("Calculate input amount for trade returned zero.");
            return Ok(None);
        }

        Ok(Some(execution_input))
    }

    async fn process_direct(&self, details: &mut CopyDetails) -> anyhow::Result<Option<SwapInput>> {
        let Some(token_decimals) = details.origin_swap.token_decimals else {
            return Err(anyhow!("Token decimals not resolved"));
        };

        let input_token = details.origin_swap.input_token_mint;
        let output_token = details.origin_swap.output_token_mint;

        let origin_swap_amount = details.origin_swap.input_amount;
        // First check: We only care about a `sell` if the input-token of a swap is one we have from
        // copying that wallet. We register a single-trade.
        //
        // If the output-token of a sell is not SOL | USDC | USDT then we're selling one token
        // to buy another, we register two trades.

        // We only care about a `buy` if the output-token is not SOL | USDC | USDT
        // We still register a single trade.
        let origin_wallet = details.origin_signer;
        let global_buys_disabled = self.buy_disabled.load(Ordering::Relaxed);
        let wallet_buys_disabled = self.buy_disabled_wallets.contains(&details.origin_signer);
        let buy_disabled = global_buys_disabled || wallet_buys_disabled;
        let (execution_input, copy_type) = match self
            .trade_history
            .get_history_for_wallet_and_token(origin_wallet, input_token)
        {
            // Questionable but:
            // - https://users.rust-lang.org/t/how-should-i-handle-f64-division-by-zero-panic-elegantly/32485
            // - https://stackoverflow.com/questions/72325988/does-rust-define-what-happens-when-you-cast-a-non-finite-float-to-an-integer
            Some(history_ref) => {
                let history = history_ref.clone();
                drop(history_ref);

                let origin_tokens_left = history
                    .origin
                    .buy_volume_token
                    .saturating_sub(history.origin.sell_volume_token);
                if origin_tokens_left == 0 {
                    // This should be impossible, but let's be safe and prevent division by zero
                    return Err(anyhow!(
                        "Wallet {} has zero tokens left for token {}. buy-volume={},sell-volume={}",
                        origin_wallet,
                        input_token,
                        history.origin.buy_volume_token,
                        history.origin.sell_volume_token
                    ));
                }
                if history.origin.buy_volume_token == 0 {
                    // This should also be impossible, but be safe to prevent division by zero
                    return Err(anyhow!(
                        "Wallet {}'s buy-volume for token is zero",
                        origin_wallet
                    ));
                }

                // Get the total ratio of tokens sold for the origin wallet. Guarantee that we don't get a sell-ratio > 1
                let origin_sell_total = std::cmp::min(
                    history
                        .origin
                        .sell_volume_token
                        .saturating_add(origin_swap_amount),
                    history.origin.buy_volume_token,
                );
                let origin_sell_ratio =
                    origin_sell_total as f64 / history.origin.buy_volume_token as f64;
                // Get the amount of tokens we need to have sold to achieve the same sell ratio as the origin
                let exec_target_amount =
                    (origin_sell_ratio * history.exec.buy_volume_token as f64).trunc() as u64;
                let input_amount =
                    exec_target_amount.saturating_sub(history.exec.sell_volume_token);

                // If the origin has sold most of his tokens(above 0.95), stop tracking its history
                if origin_sell_ratio > 0.95 {
                    _ = self
                        .trade_history
                        .remove_history(&origin_wallet, &input_token);
                }

                // If we no longer have any tokens to sell, stop tracking this token.
                if input_amount == 0 {
                    log::warn!("Skipping copy txn. Calculate input-amount for swap is zero");
                    return Ok(None);
                }

                if output_token == SOL || output_token == USDC || output_token == USDT {
                    // Normal sell. We follow our usual strategy and sell this token for SOL
                    // our input-token = `input-token`, our output-token = `SOL`(or whatever the strategy specifies)
                    // our sell amount is the origin's sell percentage * our balance for this token
                    (
                        SwapInput {
                            input_token_mint: input_token,
                            output_token_mint: SOL,
                            slippage_bps: self.config.slippage_bps,
                            amount: input_amount,
                            mode: SwapExecutionMode::ExactIn, // NOTE(before-change): Pumpfun quotes only work with Jupiter as `Exact-in`
                            market: None,
                        },
                        CopyType::Sell,
                    )
                } else {
                    // Wallet is selling a token to buy another. We have that token so we do the same
                    // We're making two trades. Selling a token and buying another token
                    // our input-token = `input-token`, our output-token = `output-token`

                    if buy_disabled {
                        log::warn!(
                            "Copy execution: Buys disabled for wallet {}(global={},wallet={}). Processing only sell-side of origin-tx={}",
                            details.origin_signer,
                            global_buys_disabled,
                            wallet_buys_disabled,
                            details.origin_signature
                        );
                        (
                            SwapInput {
                                input_token_mint: input_token,
                                output_token_mint: SOL,
                                amount: input_amount,
                                slippage_bps: self.config.slippage_bps,
                                mode: SwapExecutionMode::ExactIn, // NOTE(before-change): Pumpfun quotes only work with Jupiter as `Exact-in`
                                market: None,
                            },
                            CopyType::Sell,
                        )
                    } else {
                        (
                            SwapInput {
                                input_token_mint: input_token,
                                output_token_mint: output_token,
                                amount: input_amount,
                                slippage_bps: self.config.slippage_bps,
                                mode: SwapExecutionMode::ExactIn, // NOTE(before-change): Pumpfun quotes only work with Jupiter as `Exact-in`
                                market: None,
                            },
                            CopyType::Both,
                        )
                    }
                }
            }
            None => {
                if buy_disabled {
                    log::warn!(
                        "Copy execution: Buys disabled for wallet {}(global={},wallet={}). Skipping origin tx={}",
                        details.origin_signer,
                        global_buys_disabled,
                        wallet_buys_disabled,
                        details.origin_signature,
                    );
                    return Ok(None);
                }

                if output_token == SOL || output_token == USDC || output_token == USDT {
                    // Skip this. We don't buy SOL/USDC/USDT and we also don't have the input-token so nothing to do
                    log::warn!(
                        "Skipping copy txn {} to sell token for SOL | USDC | USDT",
                        details.origin_signature
                    );
                    return Ok(None);
                } else {
                    let signer_balance = details.origin_sol_balance.unwrap_or_default();

                    // We just buy the token whatever token they're buying.
                    debug!(
                        "origin-pre-balance={},origin-amount-in={},min-input={},max-input={}",
                        signer_balance.pre,
                        origin_swap_amount,
                        details.wallet_config.min_input_amount,
                        details.wallet_config.max_input_amount
                    );

                    // todo: This calculation should be generalized for other input tokens
                    let mut actual_input_amount = if input_token == SOL
                        && signer_balance.pre != 0
                        && origin_swap_amount < signer_balance.pre
                    {
                        let conviction = ((origin_swap_amount as u128)
                            * (details.wallet_config.max_input_amount as u128
                                - details.wallet_config.min_input_amount as u128))
                            / signer_balance.pre as u128;

                        let final_amount = details.wallet_config.min_input_amount
                            + u64::try_from(conviction).unwrap_or(0);
                        final_amount
                    } else {
                        details.wallet_config.min_input_amount
                    };

                    // todo: Check only applies to SOL
                    if input_token == SOL {
                        // Never buy in with more than the origin wallet's swap amount
                        actual_input_amount =
                            std::cmp::min(actual_input_amount, origin_swap_amount);
                        // This should already be guaranteed, but make sure we don't go over the max
                        actual_input_amount = std::cmp::min(
                            actual_input_amount,
                            details.wallet_config.max_input_amount,
                        );
                    }

                    (
                        SwapInput {
                            input_token_mint: SOL,
                            amount: actual_input_amount,
                            output_token_mint: output_token,
                            slippage_bps: self.config.slippage_bps,
                            mode: SwapExecutionMode::ExactIn, // NOTE(before-change): Pumpfun quotes only work with Jupiter as `Exact-in`
                            market: None,
                        },
                        CopyType::Buy,
                    )
                }
            }
        };
        // Update copy-type and token subscriptions in order to get the price later
        details.copy_type = Some(copy_type);

        Ok(Some(execution_input))
    }

    async fn process_execution_input(
        &self,
        execution_input: &SwapInput,
        details: &mut CopyDetails,
    ) -> Result<VersionedTransaction, Error> {
        let quote = self.swap_executor.quote(&execution_input, None).await?;
        // Increase the fee amount for each send-txn retry
        let additional_fee = details.send_txn_retries as u64 * self.config.fee_increase_multiplier;
        let tips = self.jito_tips.read().await;
        let base_fee = self
            .config
            .max_fee_lamports
            .min(self.config.min_fee_lamports.max(tips.p95() + 1));
        let fee = base_fee + additional_fee;
        debug!(
            "jito-p95={},base-fee={},additional-fee={},total-fee={}",
            tips.p95(),
            base_fee,
            additional_fee,
            fee
        );
        details.last_fee_lamports = fee;

        // note: If the send-txn-service does not support jito, this is just free money to the jito tip account!
        let swap_overrides = if details.send_to_jito {
            Some(SwapConfigOverrides {
                priority_fee: Some(PriorityFeeConfig::JitoTip(fee)),
                ..Default::default()
            })
        } else {
            Some(SwapConfigOverrides {
                priority_fee: Some(PriorityFeeConfig::FixedCuPrice(150_000)), // so the transaction is built with a cu-price instruction that can be modified later
                ..Default::default()
            })
        };
        let swap_transaction = self
            .swap_executor
            .swap_transaction(
                self.keypair.pubkey(),
                quote.clone(),
                swap_overrides.as_ref(),
            )
            .await?;
        let mut transaction = swap_transaction.transaction.into_versioned_tx()?;
        if !details.send_to_jito {
            let priority = PriorityDetails::from_versioned_transaction(&transaction);
            let cu_price = calculate_cu_price(fee, priority.compute_unit_limit);
            debug!(
                "cu-limit={}, cu-price={}",
                priority.compute_unit_limit, cu_price
            );
            transaction.message.modify_compute_unit_price(cu_price);
            if cfg!(debug_assertions) {
                let priority = PriorityDetails::from_versioned_transaction(&transaction);
                debug!("Final priority details: {:#?}", priority);
            }
        }
        details.quote_output = Some(QuoteDetails::from_quote_response(quote));
        Ok::<_, anyhow::Error>(transaction)
    }
}

#[allow(unused)]
pub fn find_transfer_instructions(transaction: &VersionedTransaction) {
    let x = transaction
        .message
        .instructions()
        .into_iter()
        .filter_map(|ix| {
            let caller_program = transaction
                .message
                .static_account_keys()
                .get(ix.program_id_index as usize)?;
            if *caller_program != solana_program::system_program::ID {
                return None;
            }
            let ix = bincode::deserialize::<SystemInstruction>(&ix.data).ok();
            match ix.as_ref() {
                Some(SystemInstruction::Transfer { lamports }) => ix,
                _ => None,
            }
        })
        .collect::<Vec<_>>();
    info!("Transfer instructions: {:#?}", x);
}

/// Protocol defined: There are 10^6 micro-lamports in one lamport
const MICRO_LAMPORTS_PER_LAMPORT: u64 = 1_000_000;

fn calculate_cu_price(priority_fee: u64, compute_units: u32) -> u64 {
    let cu_price = (priority_fee as u128)
        .checked_mul(MICRO_LAMPORTS_PER_LAMPORT as u128)
        .expect("u128 multiplication shouldn't overflow")
        .saturating_sub(MICRO_LAMPORTS_PER_LAMPORT as u128 - 1)
        .checked_div(compute_units as u128 + 1)
        .expect("non-zero compute units");
    log::trace!("cu-price u128: {}", cu_price);
    u64::try_from(cu_price).unwrap_or(u64::MAX)
}
