use super::super::api::v2::clmm::RaydiumV6Pool;
use crate::swap::adjust_for_slippage;
use crate::swap::execution::SwapInput;
use crate::utils;
use std::ops::Div;
use std::sync::Arc;

use anchor_spl::token_2022::spl_token_2022::{extension::StateWithExtensionsMut, state::Mint};
use arrayref::array_ref;
use log::info;
use raydium_amm_v3::states::{PoolState, POOL_TICK_ARRAY_BITMAP_SEED};
use raydium_client::utils as raydium_utils;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::{pubkey, pubkey::Pubkey};

pub const RAYDIUM_CONCENTRATED_LIQUIDITY: Pubkey =
    pubkey!("CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK");

pub async fn quote_v6(
    keypair: Arc<Keypair>,
    client: Arc<RpcClient>,
    pool_id: &Pubkey,
    pool_state: Option<&PoolState>,
    swap_input: &SwapInput,
    limit_price: Option<f64>, // todo: wtf is this
) -> anyhow::Result<(u64, u64, RaydiumV6InstructionBuilder)> {
    let SwapInput {
        input_token_mint,
        output_token_mint,
        slippage_bps,
        amount,
        mode,
        market: _,
    } = swap_input;
    let amount_specified_is_input = mode.amount_specified_is_input();
    let user_address = keypair.pubkey();

    info!(
        "raydium clmm quote: in={}. out={}",
        input_token_mint, output_token_mint
    );
    let base_in = amount_specified_is_input;

    let pool_state = match pool_state {
        Some(ps) => ps,
        None => {
            let pool_account = client.get_account(pool_id).await?;
            &utils::deserialize_anchor_account::<raydium_amm_v3::states::PoolState>(&pool_account)?
        }
    };
    let zero_for_one = *input_token_mint == pool_state.token_mint_0
        && *output_token_mint == pool_state.token_mint_1; // todo: Check that pool matches
    info!("a_to_b={}. base_in={}", zero_for_one, base_in);

    let tickarray_bitmap_extension_address = Pubkey::find_program_address(
        &[
            POOL_TICK_ARRAY_BITMAP_SEED.as_bytes(),
            pool_id.to_bytes().as_ref(),
        ],
        &RAYDIUM_CONCENTRATED_LIQUIDITY,
    )
    .0;

    let load_accounts = vec![
        pool_state.amm_config,
        tickarray_bitmap_extension_address,
        pool_state.token_mint_0,
        pool_state.token_mint_1,
    ];
    let rsps = client.get_multiple_accounts(&load_accounts).await?;
    let epoch = client.get_epoch_info().await.unwrap().epoch;
    let [amm_config_account, tickarray_bitmap_extension_account, mint0_account, mint1_account] =
        array_ref![rsps, 0, 4];
    let mut mint0_data = mint0_account.clone().unwrap().data;
    let mint0_state = StateWithExtensionsMut::<Mint>::unpack(&mut mint0_data)?;
    let mut mint1_data = mint1_account.clone().unwrap().data;
    let mint1_state = StateWithExtensionsMut::<Mint>::unpack(&mut mint1_data)?;
    let amm_config_state = utils::deserialize_anchor_account::<raydium_amm_v3::states::AmmConfig>(
        amm_config_account.as_ref().unwrap(),
    )?;

    let tickarray_bitmap_extension = utils::deserialize_anchor_account::<
        raydium_amm_v3::states::TickArrayBitmapExtension,
    >(tickarray_bitmap_extension_account.as_ref().unwrap())?;
    let zero_for_one = *input_token_mint == pool_state.token_mint_0
        && *output_token_mint == pool_state.token_mint_1;

    let transfer_fee = if base_in {
        if zero_for_one {
            raydium_utils::get_transfer_fee(&mint0_state, epoch, *amount)
        } else {
            raydium_utils::get_transfer_fee(&mint1_state, epoch, *amount)
        }
    } else {
        0
    };
    let amount_specified = amount.checked_sub(transfer_fee).unwrap();
    // load tick_arrays
    let mut tick_arrays = raydium_utils::load_cur_and_next_five_tick_array(
        &client,
        pool_id,
        &RAYDIUM_CONCENTRATED_LIQUIDITY,
        pool_state,
        &tickarray_bitmap_extension,
        zero_for_one,
    )
    .await;

    let mut sqrt_price_limit_x64 = None;
    if limit_price.is_some() {
        let sqrt_price_x64 = raydium_utils::price_to_sqrt_price_x64(
            limit_price.unwrap(),
            pool_state.mint_decimals_0,
            pool_state.mint_decimals_1,
        );
        sqrt_price_limit_x64 = Some(sqrt_price_x64);
    }

    let (mut other_amount_threshold, tick_array_indexs) =
        raydium_utils::get_out_put_amount_and_remaining_accounts(
            amount_specified,
            sqrt_price_limit_x64,
            zero_for_one,
            base_in,
            &amm_config_state,
            pool_state,
            &tickarray_bitmap_extension,
            &mut tick_arrays,
        )
        .unwrap();
    let quote = other_amount_threshold;
    let slippage = *slippage_bps as f64 / 10000.0;
    if base_in {
        // calc mint out amount with slippage
        other_amount_threshold = adjust_for_slippage(other_amount_threshold, slippage, false);
    } else {
        // calc max in with slippage
        other_amount_threshold = adjust_for_slippage(other_amount_threshold, slippage, true);
        // calc max in with transfer_fee
        let transfer_fee = if zero_for_one {
            raydium_utils::get_transfer_inverse_fee(&mint0_state, epoch, other_amount_threshold)
        } else {
            raydium_utils::get_transfer_inverse_fee(&mint1_state, epoch, other_amount_threshold)
        };
        other_amount_threshold += transfer_fee;
    }

    let mut remaining_accounts = Vec::new();
    remaining_accounts.push(AccountMeta::new_readonly(
        tickarray_bitmap_extension_address,
        false,
    ));
    let mut accounts = tick_array_indexs
        .into_iter()
        .map(|index| {
            AccountMeta::new(
                Pubkey::find_program_address(
                    &[
                        raydium_amm_v3::states::TICK_ARRAY_SEED.as_bytes(),
                        pool_id.to_bytes().as_ref(),
                        &index.to_be_bytes(),
                    ],
                    &RAYDIUM_CONCENTRATED_LIQUIDITY,
                )
                .0,
                false,
            )
        })
        .collect();
    remaining_accounts.append(&mut accounts);

    let (input_vault, output_vault, input_vault_mint, output_vault_mint) = if zero_for_one {
        (
            pool_state.token_vault_0,
            pool_state.token_vault_1,
            pool_state.token_mint_0,
            pool_state.token_mint_1,
        )
    } else {
        (
            pool_state.token_vault_1,
            pool_state.token_vault_0,
            pool_state.token_mint_1,
            pool_state.token_mint_0,
        )
    };
    let accounts = raydium_amm_v3::accounts::SwapSingleV2 {
        payer: user_address,
        amm_config: pool_state.amm_config,
        pool_state: *pool_id,
        input_token_account: Pubkey::default(),
        output_token_account: Pubkey::default(),
        input_vault,
        output_vault,
        observation_state: pool_state.observation_key, // Cannot get from API?
        token_program: anchor_spl::token::ID,
        token_program_2022: anchor_spl::token_2022::ID,
        memo_program: anchor_spl::memo::spl_memo::ID,
        input_vault_mint,
        output_vault_mint,
    };
    let data = raydium_amm_v3::instruction::SwapV2 {
        amount: *amount,
        other_amount_threshold,
        sqrt_price_limit_x64: sqrt_price_limit_x64.unwrap_or(0u128),
        is_base_input: base_in,
    };
    let ix = RaydiumV6InstructionBuilder {
        program_id: RAYDIUM_CONCENTRATED_LIQUIDITY,
        accounts,
        data,
        remaining_accounts,
    };

    let (in_token_decimals, out_token_decimals) = if zero_for_one {
        (mint0_state.base.decimals, mint1_state.base.decimals)
    } else {
        (mint1_state.base.decimals, mint0_state.base.decimals)
    };
    let normalized_input = (*amount as f64).div(10_u64.pow(in_token_decimals as u32) as f64);
    let normalized_output = (quote as f64).div(10_u64.pow(out_token_decimals as u32) as f64);
    let normalized_output_with_slippage =
        (other_amount_threshold as f64).div(10_u64.pow(out_token_decimals as u32) as f64);
    info!(
        "amount={}. quote={}. quote-with-slippage={}",
        normalized_input, normalized_output, normalized_output_with_slippage
    );
    Ok((quote, other_amount_threshold, ix))
}

pub async fn quote_v6_with_api_info(
    keypair: Arc<Keypair>,
    client: Arc<RpcClient>,
    pool_info: &RaydiumV6Pool,
    a_to_b: bool,
    swap_input: &SwapInput,
    limit_price: Option<f64>,
) -> anyhow::Result<(u64, u64, RaydiumV6InstructionBuilder)> {
    let SwapInput {
        input_token_mint,
        output_token_mint,
        slippage_bps,
        amount,
        mode,
        market: _,
    } = swap_input;
    let amount_specified_is_input = mode.amount_specified_is_input();
    let user_address = keypair.pubkey();

    let user_address = keypair.pubkey();
    let (in_token, out_token) = if a_to_b {
        (pool_info.mint_a, pool_info.mint_b)
    } else {
        (pool_info.mint_b, pool_info.mint_a)
    };
    info!("raydium clmm quote: in={}. out={}", in_token, out_token);
    let base_in = amount_specified_is_input;
    info!("a_to_b={}. base_in={}", a_to_b, base_in);

    let tickarray_bitmap_extension_address = Pubkey::find_program_address(
        &[
            POOL_TICK_ARRAY_BITMAP_SEED.as_bytes(),
            pool_info.id.to_bytes().as_ref(),
        ],
        &RAYDIUM_CONCENTRATED_LIQUIDITY,
    )
    .0;

    let load_accounts = vec![
        pool_info.amm_config.id,
        pool_info.id,
        tickarray_bitmap_extension_address,
        pool_info.mint_a,
        pool_info.mint_b,
    ];
    let rsps = client.get_multiple_accounts(&load_accounts).await?;
    let epoch = client.get_epoch_info().await.unwrap().epoch;
    let [amm_config_account, pool_account, tickarray_bitmap_extension_account, mint0_account, mint1_account] =
        array_ref![rsps, 0, 5];
    let mut mint0_data = mint0_account.clone().unwrap().data;
    let mint0_state = StateWithExtensionsMut::<Mint>::unpack(&mut mint0_data)?;
    let mut mint1_data = mint1_account.clone().unwrap().data;
    let mint1_state = StateWithExtensionsMut::<Mint>::unpack(&mut mint1_data)?;
    let amm_config_state = utils::deserialize_anchor_account::<raydium_amm_v3::states::AmmConfig>(
        amm_config_account.as_ref().unwrap(),
    )?;
    let pool_state = utils::deserialize_anchor_account::<raydium_amm_v3::states::PoolState>(
        pool_account.as_ref().unwrap(),
    )?;
    let tickarray_bitmap_extension = utils::deserialize_anchor_account::<
        raydium_amm_v3::states::TickArrayBitmapExtension,
    >(tickarray_bitmap_extension_account.as_ref().unwrap())?;
    let zero_for_one = in_token == pool_state.token_mint_0 && out_token == pool_state.token_mint_1;

    let transfer_fee = if base_in {
        if zero_for_one {
            raydium_utils::get_transfer_fee(&mint0_state, epoch, *amount)
        } else {
            raydium_utils::get_transfer_fee(&mint1_state, epoch, *amount)
        }
    } else {
        0
    };
    let amount_specified = amount.checked_sub(transfer_fee).unwrap();
    // load tick_arrays
    let mut tick_arrays = raydium_utils::load_cur_and_next_five_tick_array(
        &client,
        &pool_info.id,
        &RAYDIUM_CONCENTRATED_LIQUIDITY,
        &pool_state,
        &tickarray_bitmap_extension,
        zero_for_one,
    )
    .await;

    let mut sqrt_price_limit_x64 = None;
    if limit_price.is_some() {
        let sqrt_price_x64 = raydium_utils::price_to_sqrt_price_x64(
            limit_price.unwrap(),
            pool_state.mint_decimals_0,
            pool_state.mint_decimals_1,
        );
        sqrt_price_limit_x64 = Some(sqrt_price_x64);
    }

    let (mut other_amount_threshold, tick_array_indexs) =
        raydium_utils::get_out_put_amount_and_remaining_accounts(
            amount_specified,
            sqrt_price_limit_x64,
            zero_for_one,
            base_in,
            &amm_config_state,
            &pool_state,
            &tickarray_bitmap_extension,
            &mut tick_arrays,
        )
        .unwrap();
    let quote = other_amount_threshold;
    let slippage = *slippage_bps as f64 / 10000.0;
    if base_in {
        // calc mint out amount with slippage
        other_amount_threshold = adjust_for_slippage(other_amount_threshold, slippage, false);
    } else {
        // calc max in with slippage
        other_amount_threshold = adjust_for_slippage(other_amount_threshold, slippage, true);
        // calc max in with transfer_fee
        let transfer_fee = if zero_for_one {
            raydium_utils::get_transfer_inverse_fee(&mint0_state, epoch, other_amount_threshold)
        } else {
            raydium_utils::get_transfer_inverse_fee(&mint1_state, epoch, other_amount_threshold)
        };
        other_amount_threshold += transfer_fee;
    }

    let mut remaining_accounts = Vec::new();
    remaining_accounts.push(AccountMeta::new_readonly(
        tickarray_bitmap_extension_address,
        false,
    ));
    let mut accounts = tick_array_indexs
        .into_iter()
        .map(|index| {
            AccountMeta::new(
                Pubkey::find_program_address(
                    &[
                        raydium_amm_v3::states::TICK_ARRAY_SEED.as_bytes(),
                        pool_info.id.to_bytes().as_ref(),
                        &index.to_be_bytes(),
                    ],
                    &RAYDIUM_CONCENTRATED_LIQUIDITY,
                )
                .0,
                false,
            )
        })
        .collect();
    remaining_accounts.append(&mut accounts);

    let (input_vault, output_vault, input_vault_mint, output_vault_mint) = if zero_for_one {
        (
            pool_info.vault_a,
            pool_info.vault_b,
            pool_info.mint_a,
            pool_info.mint_b,
        )
    } else {
        (
            pool_info.vault_b,
            pool_info.vault_a,
            pool_info.mint_b,
            pool_info.mint_a,
        )
    };
    let accounts = raydium_amm_v3::accounts::SwapSingleV2 {
        payer: user_address,
        amm_config: pool_info.amm_config.id,
        pool_state: pool_info.id,
        input_token_account: Pubkey::default(),
        output_token_account: Pubkey::default(),
        input_vault,
        output_vault,
        observation_state: pool_state.observation_key, // Cannot get from API?
        token_program: anchor_spl::token::ID,
        token_program_2022: anchor_spl::token_2022::ID,
        memo_program: anchor_spl::memo::spl_memo::ID,
        input_vault_mint,
        output_vault_mint,
    };
    let data = raydium_amm_v3::instruction::SwapV2 {
        amount: *amount,
        other_amount_threshold,
        sqrt_price_limit_x64: sqrt_price_limit_x64.unwrap_or(0u128),
        is_base_input: base_in,
    };
    let ix = RaydiumV6InstructionBuilder {
        program_id: RAYDIUM_CONCENTRATED_LIQUIDITY,
        accounts,
        data,
        remaining_accounts,
    };

    let (in_token_decimals, out_token_decimals) = if zero_for_one {
        (mint0_state.base.decimals, mint1_state.base.decimals)
    } else {
        (mint1_state.base.decimals, mint0_state.base.decimals)
    };
    let normalized_input = (*amount as f64).div(10_u64.pow(in_token_decimals as u32) as f64);
    let normalized_output = (quote as f64).div(10_u64.pow(out_token_decimals as u32) as f64);
    let normalized_output_with_slippage =
        (other_amount_threshold as f64).div(10_u64.pow(out_token_decimals as u32) as f64);
    info!(
        "amount={}. quote={}. quote-with-slippage={}",
        normalized_input, normalized_output, normalized_output_with_slippage
    );
    Ok((quote, other_amount_threshold, ix))
}

pub struct RaydiumV6InstructionBuilder {
    accounts: raydium_amm_v3::accounts::SwapSingleV2,
    data: raydium_amm_v3::instruction::SwapV2,
    program_id: Pubkey,
    remaining_accounts: Vec<AccountMeta>,
}

impl RaydiumV6InstructionBuilder {
    pub fn update_user_account(&mut self, key: Pubkey) {
        self.accounts.payer = key;
    }

    pub fn update_user_input_token_account(&mut self, key: Pubkey) {
        self.accounts.input_token_account = key;
    }

    pub fn update_user_output_token_account(&mut self, key: Pubkey) {
        self.accounts.output_token_account = key
    }

    pub fn update_program_id(&mut self, key: Pubkey) {
        self.program_id = key
    }

    pub fn build(self) -> solana_sdk::instruction::Instruction {
        if self.accounts.input_token_account == Pubkey::default()
            || self.accounts.output_token_account == Pubkey::default()
        {
            panic!("Failed to update one or both of input-token-account/output-token-account");
        }

        Instruction {
            program_id: self.program_id,
            accounts: [
                anchor_lang::ToAccountMetas::to_account_metas(&self.accounts, None),
                self.remaining_accounts,
            ]
            .concat(),
            data: anchor_lang::InstructionData::data(&self.data),
        }
    }
}
