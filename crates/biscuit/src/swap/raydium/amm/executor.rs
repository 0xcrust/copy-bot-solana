use crate::constants::programs::RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID;
use crate::core::BlockHashNotification;
use crate::swap::builder::SwapInstructionsBuilder;
use crate::swap::execution::{
    ComputeUnitLimits, PriorityFeeConfig, QuoteConfigOverrides, SwapConfig, SwapExecutor,
    SwapInput, SwapQuote, SwapTransaction, TransactionVariant,
};
use crate::swap::raydium::api::v3::response::{
    ApiV3PoolsPage, ApiV3StandardPool, ApiV3StandardPoolKeys,
};
use crate::swap::raydium::api::v3::{
    ApiV3Client, PoolFetchParams, PoolSort, PoolSortOrder, PoolType,
};
use crate::wire::helius_rpc::HeliusPrioFeesHandle;
use std::sync::Arc;

use anyhow::{anyhow, Context};
use arrayref::array_ref;
use raydium_library::amm::AmmKeys;
use safe_transmute::{transmute_one_pedantic, transmute_to_bytes};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::account_info::IntoAccountInfo;
use solana_sdk::hash::Hash;
use solana_sdk::instruction::Instruction;
use solana_sdk::program_pack::Pack;
use solana_sdk::pubkey::Pubkey;
use tokio::sync::RwLock;

// https://api-v3.raydium.io/pools/info/mint?mint1=So11111111111111111111111111111111111111112&mint2=EKpQGSJtjMFqKZ9KQanSqYXRcF8fBopzLHYxdM65zcjm&poolType=standard&poolSortField=liquidity&sortType=desc&pageSize=100&page=1

#[derive(Clone)]
pub struct RaydiumAmmExecutor {
    client: Arc<RpcClient>,
    api: ApiV3Client,
    priofee: Option<HeliusPrioFeesHandle>,
    blockhash: Option<Arc<RwLock<BlockHashNotification>>>,
    config: SwapConfig,
    get_market_keys_by_api: bool,
}

// todo: Builder pattern for this
#[derive(Default)]
pub struct RaydiumAmmExecutorOpts {
    pub priority_fee: Option<PriorityFeeConfig>,
    pub cu_limits: Option<ComputeUnitLimits>,
    pub wrap_and_unwrap_sol: Option<bool>,
    pub get_market_keys_by_api: Option<bool>,
}

impl RaydiumAmmExecutor {
    pub fn new(
        client: Arc<RpcClient>,
        config: RaydiumAmmExecutorOpts,
        api: ApiV3Client,
        priofee: Option<HeliusPrioFeesHandle>,
        blockhash: Option<Arc<RwLock<BlockHashNotification>>>,
    ) -> Self {
        let RaydiumAmmExecutorOpts {
            priority_fee,
            cu_limits,
            wrap_and_unwrap_sol,
            get_market_keys_by_api,
        } = config;
        Self {
            client,
            api,
            priofee,
            blockhash,
            get_market_keys_by_api: get_market_keys_by_api.unwrap_or(true),
            config: SwapConfig {
                priority_fee,
                cu_limits,
                wrap_and_unwrap_sol,
                as_legacy_transaction: Some(true),
            },
        }
    }

    async fn make_swap(
        &self,
        input_pubkey: Pubkey,
        output: RaydiumAmmQuote,
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

        let mut builder = SwapInstructionsBuilder::default();
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
            spl_token::ID,
            spl_token::ID,
            None,
        )?;
        let instruction = swap_instruction(
            &RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID,
            &output.amm_keys,
            &output.market_keys,
            &input_pubkey,
            &spl_associated_token_account::get_associated_token_address(
                &input_pubkey,
                &output.input_mint,
            ),
            &spl_associated_token_account::get_associated_token_address(
                &input_pubkey,
                &output.output_mint,
            ),
            output.amount,
            output.other_amount_threshold,
            output.amount_specified_is_input,
        )?;
        builder.swap_instruction = Some(instruction);

        let compute_units = builder
            .handle_compute_units_params(cu_limits, &self.client, input_pubkey)
            .await?;
        builder
            .handle_priority_fee_params(&self.priofee, priority_fee, compute_units, input_pubkey)
            .await?;

        Ok(builder)
    }
}

use educe::Educe;

#[derive(Educe)]
#[educe(Debug)]
pub struct RaydiumAmmQuote {
    /// The address of the amm pool
    market: Pubkey,
    /// The input mint
    input_mint: Pubkey,
    /// The output mint,
    output_mint: Pubkey,
    /// The amount specified
    amount: u64,
    /// The other amount
    other_amount: u64,
    /// The other amount with slippage
    other_amount_threshold: u64,
    /// Whether the amount specified is in terms of the input token
    amount_specified_is_input: bool,
    /// The input mint decimals
    input_mint_decimals: u8,
    /// The output mint decimals
    output_mint_decimals: u8,
    /// Amm keys
    #[educe(Debug(ignore))]
    amm_keys: AmmKeys,
    /// Market keys
    #[educe(Debug(ignore))]
    market_keys: MarketKeys,
}

impl SwapExecutor for RaydiumAmmExecutor {
    type Quote = RaydiumAmmQuote;

    async fn quote(
        &self,
        swap_input: &SwapInput,
        _overrides: Option<&QuoteConfigOverrides>,
    ) -> anyhow::Result<Self::Quote> {
        if swap_input.input_token_mint == swap_input.output_token_mint {
            return Err(anyhow!(
                "Input token cannot equal output token {}",
                swap_input.input_token_mint
            ));
        }

        let mut pool_id = swap_input.market;
        if pool_id.is_none() {
            let response: ApiV3PoolsPage<ApiV3StandardPool> = self
                .api
                .fetch_pool_by_mints(
                    &swap_input.input_token_mint,
                    Some(&swap_input.output_token_mint),
                    &PoolFetchParams {
                        pool_type: PoolType::Standard,
                        pool_sort: PoolSort::Liquidity,
                        sort_type: PoolSortOrder::Descending,
                        page_size: 10, // should be more than enough?
                        page: 1,
                    },
                )
                .await?;
            pool_id = response.data.into_iter().find_map(|data| {
                if data.mint_a.address == swap_input.input_token_mint
                    && data.mint_b.address == swap_input.output_token_mint
                    || data.mint_a.address == swap_input.output_token_mint
                        && data.mint_b.address == swap_input.input_token_mint
                {
                    Some(data.id)
                } else {
                    None
                }
            });
        }

        let Some(pool_id) = pool_id else {
            return Err(anyhow!(
                "todo: Return identifiable `marketNotFound` error so we can retry with Jupiter?"
            ));
        };

        let amm_keys = raydium_library::amm::utils::load_amm_keys(
            &self.client,
            &RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID,
            &pool_id,
        )
        .await?;

        let market_keys = if self.get_market_keys_by_api {
            let response = self
                .api
                .fetch_pool_keys_by_ids::<ApiV3StandardPoolKeys, _>([&pool_id].into_iter())
                .await?;
            let keys = response.first().context(format!(
                "Failed to get pool keys for raydium standard pool {}",
                pool_id
            ))?;
            MarketKeys::from(&keys.keys.market)
        } else {
            MarketKeys::from(
                raydium_library::amm::openbook::get_keys_for_market(
                    &self.client,
                    &amm_keys.market_program,
                    &amm_keys.market,
                )
                .await?,
            )
        };

        // reload accounts data to calculate amm pool vault amount
        // get multiple accounts at the same time to ensure data consistency
        let load_pubkeys = vec![
            pool_id,
            amm_keys.amm_target,
            amm_keys.amm_pc_vault,
            amm_keys.amm_coin_vault,
            amm_keys.amm_open_order,
            amm_keys.market,
            market_keys.event_queue,
        ];
        let rsps =
            crate::wire::rpc::utils::get_multiple_account_data(&self.client, &load_pubkeys).await?;
        let accounts = array_ref![rsps, 0, 7];
        let [amm_account, amm_target_account, amm_pc_vault_account, amm_coin_vault_account, amm_open_orders_account, market_account, market_event_q_account] =
            accounts;
        let amm: raydium_amm::state::AmmInfo = transmute_one_pedantic::<super::amm_info::AmmInfo>(
            transmute_to_bytes(&amm_account.as_ref().unwrap().clone().data),
        )
        .map_err(|e| e.without_src())?
        .into();
        let _amm_target: raydium_amm::state::TargetOrders =
            transmute_one_pedantic::<raydium_amm::state::TargetOrders>(transmute_to_bytes(
                &amm_target_account.as_ref().unwrap().clone().data,
            ))
            .map_err(|e| e.without_src())?;
        let amm_pc_vault =
            spl_token::state::Account::unpack(&amm_pc_vault_account.as_ref().unwrap().clone().data)
                .unwrap();
        let amm_coin_vault = spl_token::state::Account::unpack(
            &amm_coin_vault_account.as_ref().unwrap().clone().data,
        )
        .unwrap();
        let (amm_pool_pc_vault_amount, amm_pool_coin_vault_amount) =
            if raydium_amm::state::AmmStatus::from_u64(amm.status).orderbook_permission() {
                let amm_open_orders_account =
                    &mut amm_open_orders_account.as_ref().unwrap().clone();
                let market_account = &mut market_account.as_ref().unwrap().clone();
                let market_event_q_account = &mut market_event_q_account.as_ref().unwrap().clone();
                let amm_open_orders_info =
                    (&amm.open_orders, amm_open_orders_account).into_account_info();
                let market_account_info = (&amm.market, market_account).into_account_info();
                let market_event_queue_info =
                    (&(market_keys.event_queue), market_event_q_account).into_account_info();
                let amm_authority = Pubkey::find_program_address(
                    &[raydium_amm::processor::AUTHORITY_AMM],
                    &RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID,
                )
                .0;
                let lamports = &mut 0;
                let data = &mut [0u8];
                let owner = Pubkey::default();
                let amm_authority_info = solana_program::account_info::AccountInfo::new(
                    &amm_authority,
                    false,
                    false,
                    lamports,
                    data,
                    &owner,
                    false,
                    0,
                );
                let (market_state, open_orders) =
                    raydium_amm::processor::Processor::load_serum_market_order(
                        &market_account_info,
                        &amm_open_orders_info,
                        &amm_authority_info,
                        &amm,
                        false,
                    )?;
                let (amm_pool_pc_vault_amount, amm_pool_coin_vault_amount) =
                    raydium_amm::math::Calculator::calc_total_without_take_pnl(
                        amm_pc_vault.amount,
                        amm_coin_vault.amount,
                        &open_orders,
                        &amm,
                        &market_state,
                        &market_event_queue_info,
                        &amm_open_orders_info,
                    )?;
                (amm_pool_pc_vault_amount, amm_pool_coin_vault_amount)
            } else {
                let (amm_pool_pc_vault_amount, amm_pool_coin_vault_amount) =
                    raydium_amm::math::Calculator::calc_total_without_take_pnl_no_orderbook(
                        amm_pc_vault.amount,
                        amm_coin_vault.amount,
                        &amm,
                    )?;
                (amm_pool_pc_vault_amount, amm_pool_coin_vault_amount)
            };

        let (direction, coin_to_pc) = if swap_input.input_token_mint == amm_keys.amm_coin_mint
            && swap_input.output_token_mint == amm_keys.amm_pc_mint
        {
            (raydium_library::amm::utils::SwapDirection::Coin2PC, true)
        } else {
            (raydium_library::amm::utils::SwapDirection::PC2Coin, false)
        };

        let amount_specified_is_input = swap_input.mode.amount_specified_is_input();
        let (other_amount, other_amount_threshold) = raydium_library::amm::swap_with_slippage(
            amm_pool_pc_vault_amount,
            amm_pool_coin_vault_amount,
            amm.fees.swap_fee_numerator,
            amm.fees.swap_fee_denominator,
            direction,
            swap_input.amount,
            amount_specified_is_input,
            swap_input.slippage_bps as u64,
        )?;
        log::debug!(
            "raw quote: {}. raw other_amount_threshold: {}",
            other_amount,
            other_amount_threshold
        );

        Ok(RaydiumAmmQuote {
            market: pool_id,
            input_mint: swap_input.input_token_mint,
            output_mint: swap_input.output_token_mint,
            amount: swap_input.amount,
            other_amount,
            other_amount_threshold,
            amount_specified_is_input,
            input_mint_decimals: if coin_to_pc {
                amm.coin_decimals
            } else {
                amm.pc_decimals
            } as u8,
            output_mint_decimals: if coin_to_pc {
                amm.pc_decimals
            } else {
                amm.coin_decimals
            } as u8,
            amm_keys,
            market_keys,
        })
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

impl SwapQuote for RaydiumAmmQuote {
    fn amount(&self) -> u64 {
        self.amount
    }

    fn amount_specified_is_input(&self) -> bool {
        self.amount_specified_is_input
    }

    fn input_token(&self) -> anchor_lang::prelude::Pubkey {
        self.input_mint
    }

    fn input_token_decimals(&self) -> Option<u8> {
        Some(self.input_mint_decimals)
    }

    fn other_amount(&self) -> u64 {
        self.other_amount
    }

    fn other_amount_threshold(&self) -> u64 {
        self.other_amount_threshold
    }

    fn output_token(&self) -> anchor_lang::prelude::Pubkey {
        self.output_mint
    }

    fn output_token_decimals(&self) -> Option<u8> {
        Some(self.output_mint_decimals)
    }
}

#[derive(Debug, Clone, Copy)]
struct MarketKeys {
    event_queue: Pubkey,
    bids: Pubkey,
    asks: Pubkey,
    coin_vault: Pubkey,
    pc_vault: Pubkey,
    vault_signer_key: Pubkey,
}

fn swap_instruction(
    amm_program: &Pubkey,
    amm_keys: &AmmKeys,
    market_keys: &MarketKeys,
    user_owner: &Pubkey,
    user_source: &Pubkey,
    user_destination: &Pubkey,
    amount_specified: u64,
    other_amount_threshold: u64,
    swap_base_in: bool,
) -> anyhow::Result<Instruction> {
    let swap_instruction = if swap_base_in {
        raydium_amm::instruction::swap_base_in(
            &amm_program,
            &amm_keys.amm_pool,
            &amm_keys.amm_authority,
            &amm_keys.amm_open_order,
            &amm_keys.amm_coin_vault,
            &amm_keys.amm_pc_vault,
            &amm_keys.market_program,
            &amm_keys.market,
            &market_keys.bids,
            &market_keys.asks,
            &market_keys.event_queue,
            &market_keys.coin_vault,
            &market_keys.pc_vault,
            &market_keys.vault_signer_key,
            user_source,
            user_destination,
            user_owner,
            amount_specified,
            other_amount_threshold,
        )?
    } else {
        raydium_amm::instruction::swap_base_out(
            &amm_program,
            &amm_keys.amm_pool,
            &amm_keys.amm_authority,
            &amm_keys.amm_open_order,
            &amm_keys.amm_coin_vault,
            &amm_keys.amm_pc_vault,
            &amm_keys.market_program,
            &amm_keys.market,
            &market_keys.bids,
            &market_keys.asks,
            &market_keys.event_queue,
            &market_keys.coin_vault,
            &market_keys.pc_vault,
            &market_keys.vault_signer_key,
            user_source,
            user_destination,
            user_owner,
            other_amount_threshold,
            amount_specified,
        )?
    };

    Ok(swap_instruction)
}

impl From<&raydium_library::amm::MarketPubkeys> for MarketKeys {
    fn from(keys: &raydium_library::amm::MarketPubkeys) -> Self {
        MarketKeys {
            event_queue: *keys.event_q,
            bids: *keys.bids,
            asks: *keys.asks,
            coin_vault: *keys.coin_vault,
            pc_vault: *keys.pc_vault,
            vault_signer_key: *keys.vault_signer_key,
        }
    }
}
impl From<raydium_library::amm::MarketPubkeys> for MarketKeys {
    fn from(keys: raydium_library::amm::MarketPubkeys) -> Self {
        MarketKeys::from(&keys)
    }
}
impl From<&crate::swap::raydium::api::v3::response::pools::standard::MarketKeys> for MarketKeys {
    fn from(keys: &crate::swap::raydium::api::v3::response::pools::standard::MarketKeys) -> Self {
        MarketKeys {
            event_queue: keys.market_event_queue,
            bids: keys.market_bids,
            asks: keys.market_asks,
            coin_vault: keys.market_base_vault,
            pc_vault: keys.market_quote_vault,
            vault_signer_key: keys.market_authority,
        }
    }
}
impl From<crate::swap::raydium::api::v3::response::pools::standard::MarketKeys> for MarketKeys {
    fn from(keys: crate::swap::raydium::api::v3::response::pools::standard::MarketKeys) -> Self {
        MarketKeys::from(&keys)
    }
}
