use crate::constants::mints::SOL;
use crate::constants::programs::PUMPFUN_PROGRAM_ID;
use crate::core::BlockHashNotification;
use crate::parser::generated::pump::accounts::{BondingCurve, Global};
use crate::parser::generated::pump::instructions::{BuyBuilder, SellBuilder};
use crate::swap::builder::SwapInstructionsBuilder;
use crate::swap::execution::{
    ComputeUnitLimits, PriorityFeeConfig, QuoteConfigOverrides, SwapConfig, SwapExecutionMode,
    SwapExecutor, SwapInput, SwapQuote, SwapTransaction, TransactionVariant,
};
use crate::wire::helius_rpc::HeliusPrioFeesHandle;
use std::sync::Arc;

use anyhow::{anyhow, Context};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::hash::Hash;
use solana_sdk::program_pack::Pack;
use solana_sdk::{pubkey, pubkey::Pubkey};
use spl_associated_token_account::get_associated_token_address;
use spl_token::state::Mint;
use tokio::sync::RwLock;

const EVENT_AUTHORITY: Pubkey = pubkey!("Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1");

#[derive(Clone)]
pub struct PFExecutor {
    client: Arc<RpcClient>,
    priofee: Option<HeliusPrioFeesHandle>,
    blockhash: Option<Arc<RwLock<BlockHashNotification>>>,
    config: SwapConfig,
}

// todo: Builder pattern for this
pub struct PumpExecutorOpts {
    pub enable_auto_priority_fee: bool, // Jito is an exception
    pub priority_fee: Option<PriorityFeeConfig>,
    pub cu_limits: Option<ComputeUnitLimits>,
    pub wrap_and_unwrap_sol: Option<bool>,
    // pub as_legacy_transaction: Option<bool>,
}

impl PFExecutor {
    pub fn new(
        client: Arc<RpcClient>,
        config: PumpExecutorOpts,
        priofee: Option<HeliusPrioFeesHandle>,
        blockhash: Option<Arc<RwLock<BlockHashNotification>>>,
    ) -> Self {
        Self {
            client,
            priofee,
            blockhash,
            config: SwapConfig {
                priority_fee: config.priority_fee,
                cu_limits: config.cu_limits,
                wrap_and_unwrap_sol: config.wrap_and_unwrap_sol,
                as_legacy_transaction: Some(true),
            },
        }
    }

    fn get_global_account_pda() -> Pubkey {
        Pubkey::find_program_address(&[b"global"], &PUMPFUN_PROGRAM_ID).0
    }

    fn get_bonding_curve_pda(mint: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(&[b"bonding-curve", mint.as_ref()], &PUMPFUN_PROGRAM_ID).0
    }

    fn buy_with_slippage(amount: u64, slippage_bps: u16) -> u64 {
        amount + (amount * slippage_bps as u64) / 10_000
    }

    fn sell_with_slippage(amount: u64, slippage_bps: u16) -> u64 {
        amount - (amount * slippage_bps as u64) / 10_000
    }

    async fn make_swap(
        &self,
        input_pubkey: Pubkey,
        output: PFQuote,
        overrides: Option<&crate::swap::execution::SwapConfigOverrides>,
    ) -> anyhow::Result<SwapInstructionsBuilder> {
        let priority_fee = overrides
            .and_then(|o| o.priority_fee)
            .or(self.config.priority_fee);
        let cu_limits = overrides
            .and_then(|o| o.cu_limits)
            .or(self.config.cu_limits);
        let wrap_and_unwrap_sol = overrides
            .and_then(|o| o.wrap_and_unwrap_sol)
            .or(self.config.wrap_and_unwrap_sol)
            .unwrap_or(true);
        let bonding_curve = Self::get_bonding_curve_pda(&output.mint);
        let bonding_curve_ata = get_associated_token_address(&bonding_curve, &output.mint);
        let user_ata = get_associated_token_address(&input_pubkey, &output.mint);

        let instruction = if output.is_buy {
            BuyBuilder::new()
                .amount(output.token_amount)
                .max_sol_cost(output.sol_amount_with_slippage)
                .fee_recipient(output.fee_recipient)
                .mint(output.mint)
                .associated_bonding_curve(bonding_curve_ata)
                .associated_user(user_ata)
                .user(input_pubkey)
                .global(output.protocol_address)
                .bonding_curve(bonding_curve)
                .event_authority(EVENT_AUTHORITY)
                .instruction()
        } else {
            SellBuilder::new()
                .amount(output.token_amount)
                .min_sol_output(output.sol_amount_with_slippage)
                .fee_recipient(output.fee_recipient)
                .mint(output.mint)
                .associated_bonding_curve(bonding_curve_ata)
                .associated_user(user_ata)
                .user(input_pubkey)
                .global(output.protocol_address)
                .bonding_curve(bonding_curve)
                .event_authority(EVENT_AUTHORITY)
                .instruction()
        };

        let mut builder = SwapInstructionsBuilder::default();
        builder.swap_instruction = Some(instruction);
        let associated_accounts = builder.handle_token_wrapping_and_accounts_creation(
            input_pubkey,
            wrap_and_unwrap_sol,
            if output.amount_specified_is_input() {
                output.amount()
            } else {
                output.other_amount()
            },
            output.input_token(),
            output.output_token(),
            output.input_token_program,
            output.output_token_program,
            None,
        )?;
        let compute_units = builder
            .handle_compute_units_params(cu_limits, &self.client, input_pubkey)
            .await?;
        builder
            .handle_priority_fee_params(&self.priofee, priority_fee, compute_units, input_pubkey)
            .await?;

        Ok(builder)
    }
}

#[derive(Debug)]
pub struct PFQuote {
    /// Whether it's a buy or a sell
    is_buy: bool,
    /// The amount of tokens to buy or sell
    token_amount: u64,
    /// The amount of SOL that would be spent or gotten from the swap, without adjusting for slippage
    sol_amount: u64,
    /// The SOL amount with slippage
    sol_amount_with_slippage: u64,
    /// The mint being traded
    mint: Pubkey,
    /// The decimals of the traded token
    mint_decimals: u8,
    /// Fee recipient for the protocol
    fee_recipient: Pubkey,
    /// Address of the protocol config account
    protocol_address: Pubkey,
    /// Token program that owns the input token
    input_token_program: Pubkey,
    /// Token program that owns the output token
    output_token_program: Pubkey,
}

impl SwapExecutor for PFExecutor {
    type Quote = PFQuote;

    async fn quote(
        &self,
        swap_input: &SwapInput,
        _overrides: Option<&QuoteConfigOverrides>,
    ) -> anyhow::Result<Self::Quote> {
        if swap_input.input_token_mint == swap_input.output_token_mint {
            return Err(anyhow!(
                "Input token cannot equal output token ; {}",
                swap_input.input_token_mint
            ));
        }
        if !matches!(swap_input.mode, SwapExecutionMode::ExactIn) {
            return Err(anyhow!("Pumpfun execution mode is always `exact-in`"));
        }

        let (mint, is_buy) = match (swap_input.input_token_mint, swap_input.output_token_mint) {
            (SOL, output) => (output, true),
            (input, SOL) => (input, false),
            _ => return Err(anyhow!("Input or output for pump swap must be SOL")),
        };

        let mut accounts = self
            .client
            .get_multiple_accounts(&[
                mint,
                Self::get_bonding_curve_pda(&mint),
                Self::get_global_account_pda(),
            ])
            .await?;

        let raw_mint_account = accounts
            .remove(0)
            .context(format!("mint {} does not exist", mint))?;
        if raw_mint_account.owner != spl_token::ID
            && raw_mint_account.owner != anchor_spl::token_2022::ID
        {
            return Err(anyhow!("Unknown owner for mint"));
        }

        let bonding_curve = accounts
            .remove(0)
            .context(format!("bonding curve does not exist for token {}", mint))?;
        let protocol = accounts
            .remove(0)
            .context("protocol config does not exist")?;

        let mint_account = Mint::unpack(&raw_mint_account.data[..Mint::LEN])?;
        let bonding_curve = BondingCurve::from_bytes(&bonding_curve.data)?;
        let protocol = Global::from_bytes(&protocol.data)?;
        let quote = if is_buy {
            // let token_amount =
            //     bonding_curve.get_buy_out_price(swap_input.amount, protocol.fee_basis_points);
            let token_amount = bonding_curve.get_buy_price(swap_input.amount)?;
            // println!("Buy-price: {}", x);
            PFQuote {
                is_buy: true,
                token_amount,
                sol_amount: swap_input.amount,
                sol_amount_with_slippage: Self::buy_with_slippage(
                    swap_input.amount,
                    swap_input.slippage_bps,
                ),
                mint,
                mint_decimals: mint_account.decimals,
                fee_recipient: protocol.fee_recipient,
                protocol_address: Self::get_global_account_pda(),
                input_token_program: spl_token::ID,
                output_token_program: raw_mint_account.owner,
            }
        } else {
            let sol_amount =
                bonding_curve.get_sell_price(swap_input.amount, protocol.fee_basis_points)?;
            PFQuote {
                is_buy: false,
                token_amount: swap_input.amount,
                sol_amount,
                sol_amount_with_slippage: Self::sell_with_slippage(
                    sol_amount,
                    swap_input.slippage_bps,
                ),
                mint,
                mint_decimals: mint_account.decimals,
                fee_recipient: protocol.fee_recipient,
                protocol_address: Self::get_global_account_pda(),
                input_token_program: raw_mint_account.owner,
                output_token_program: spl_token::ID,
            }
        };

        Ok(quote)
    }

    async fn swap_instructions(
        &self,
        input_pubkey: Pubkey,
        output: Self::Quote,
        overrides: Option<&crate::swap::execution::SwapConfigOverrides>,
    ) -> anyhow::Result<Vec<solana_sdk::instruction::Instruction>> {
        let builder = self.make_swap(input_pubkey, output, overrides).await?;
        builder.build_instructions()
    }

    async fn swap_transaction(
        &self,
        input_pubkey: Pubkey,
        output: Self::Quote,
        overrides: Option<&crate::swap::execution::SwapConfigOverrides>,
    ) -> anyhow::Result<crate::swap::execution::SwapTransaction> {
        let builder = self.make_swap(input_pubkey, output, overrides).await?;
        let (blockhash, last_valid_blockheight) = match self.blockhash.as_ref() {
            None => (Hash::default(), 0),
            Some(b) => {
                let lock = b.read().await;
                (lock.blockhash, lock.last_valid_block_height)
            }
        };

        Ok(SwapTransaction {
            transaction: TransactionVariant::Versioned(
                builder.build_transaction(Some(&input_pubkey), Some(blockhash))?,
            ),
            last_valid_blockheight,
        })
    }

    fn get_config(&self) -> crate::swap::execution::SwapConfig {
        self.config.clone()
    }

    fn set_config(&mut self, config: &crate::swap::execution::SwapConfig) {
        self.config = *config;
    }
}

impl SwapQuote for PFQuote {
    fn amount(&self) -> u64 {
        self.token_amount
    }

    fn amount_specified_is_input(&self) -> bool {
        if self.is_buy {
            false
        } else {
            true
        }
    }

    fn input_token(&self) -> anchor_lang::prelude::Pubkey {
        if self.is_buy {
            spl_token::native_mint::ID
        } else {
            self.mint
        }
    }

    fn input_token_decimals(&self) -> Option<u8> {
        Some(if self.is_buy {
            spl_token::native_mint::DECIMALS
        } else {
            self.mint_decimals
        })
    }

    fn other_amount(&self) -> u64 {
        self.sol_amount
    }

    fn other_amount_threshold(&self) -> u64 {
        self.sol_amount_with_slippage
    }

    fn output_token(&self) -> anchor_lang::prelude::Pubkey {
        if self.is_buy {
            self.mint
        } else {
            spl_token::native_mint::ID
        }
    }

    fn output_token_decimals(&self) -> Option<u8> {
        Some(if self.is_buy {
            self.mint_decimals
        } else {
            spl_token::native_mint::DECIMALS
        })
    }
}
