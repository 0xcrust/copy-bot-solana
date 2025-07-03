use super::api::IWhirlPool;
use super::utils::{
    calculate_swap_amounts_from_quote, get_default_sqrt_price_limit, get_oracle_address,
    get_tick_arrays,
};
use crate::swap::execution::SwapInput;
use crate::utils::deserialize_anchor_account;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ops::Div;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::anyhow;
use log::info;
use raydium_client::utils;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::instruction::Instruction;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::{pubkey, pubkey::Pubkey};

use whirlpool::manager::swap_manager::swap;
use whirlpool::state::Whirlpool;
use whirlpool::util::SwapTickSequence;

const WHIRLPOOL_PROGRAM_ID: Pubkey = pubkey!("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc");

/// Returns `(quote, slippage_adjusted_quote, swap_ix)`
pub async fn quote(
    keypair: Arc<Keypair>,
    client: Arc<RpcClient>,
    pool_id: &Pubkey,
    pool_info: Option<&Whirlpool>,
    swap_input: &SwapInput,
) -> anyhow::Result<(u64, u64, OrcaInstructionBuilder)> {
    let SwapInput {
        input_token_mint,
        output_token_mint,
        slippage_bps,
        amount,
        mode,
        market: _,
    } = swap_input;
    let amount_specified_is_input = mode.amount_specified_is_input();
    let whirlpool = match pool_info {
        Some(p) => p,
        None => {
            let pool_account = client.get_account(pool_id).await?;
            &utils::deserialize_anchor_account::<Whirlpool>(&pool_account)?
        }
    };

    let a_to_b = if whirlpool.token_mint_a == *input_token_mint
        && whirlpool.token_mint_b == *output_token_mint
    {
        true
    } else if whirlpool.token_mint_b == *input_token_mint
        && whirlpool.token_mint_a == *output_token_mint
    {
        false
    } else {
        return Err(anyhow!(
            "Whirlpool state from RPC doesn't match specified mints"
        ));
    };

    info!(
        "whirlpool quote. in={}. out={}",
        input_token_mint, output_token_mint
    );
    info!(
        "a_to_b={}. amount_specified_is_input={}. tick-spacing={}",
        a_to_b, amount_specified_is_input, whirlpool.tick_spacing
    );

    // todo: Piggyback fetching mints off the get-accounts rpc-call
    let (tick_arrays, tick_array_keys) = get_tick_arrays(
        &client,
        whirlpool.tick_current_index,
        whirlpool.tick_spacing as i32,
        a_to_b,
        &WHIRLPOOL_PROGRAM_ID,
        pool_id,
    )
    .await?;
    let mut tick_arrays = tick_arrays
        .into_iter()
        .map(|a| Rc::new(RefCell::new(a)))
        .collect::<VecDeque<_>>();

    let tick_array_0 = tick_arrays.pop_front().unwrap();
    let tick_array_1 = tick_arrays.pop_front().unwrap();
    let tick_array_2 = tick_arrays.pop_front().unwrap();

    let mut swap_tick_sequence = SwapTickSequence::new(
        tick_array_0.try_borrow_mut()?,
        tick_array_1.try_borrow_mut().ok(),
        tick_array_2.try_borrow_mut().ok(),
    );

    let sqrt_price_limit = get_default_sqrt_price_limit(a_to_b);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let swap_result = swap(
        whirlpool,
        &mut swap_tick_sequence,
        *amount,
        sqrt_price_limit,
        amount_specified_is_input,
        a_to_b,
        timestamp,
    )?;

    let quote = if a_to_b {
        swap_result.amount_b
    } else {
        swap_result.amount_a
    };
    let (amount_in, amount_out) = if a_to_b == amount_specified_is_input {
        (swap_result.amount_a, swap_result.amount_b)
    } else {
        (swap_result.amount_b, swap_result.amount_a)
    };

    let slippage_adjusted_quote = calculate_swap_amounts_from_quote(
        amount_in,
        amount_out,
        *slippage_bps as f64 / 1000.0,
        amount_specified_is_input,
    );

    #[allow(clippy::get_first)]
    let tick_array_0 = *tick_array_keys.get(0).expect("At least one tick array");
    let tick_array_1 = *tick_array_keys.get(1).unwrap_or(&tick_array_0);
    let tick_array_2 = *tick_array_keys.get(2).unwrap_or(&tick_array_1);
    let accounts = whirlpool::accounts::Swap {
        token_program: spl_token::ID,
        token_authority: keypair.pubkey(),
        whirlpool: *pool_id,
        token_owner_account_a: Pubkey::default(),
        token_owner_account_b: Pubkey::default(),
        token_vault_a: whirlpool.token_vault_a,
        token_vault_b: whirlpool.token_vault_b,
        tick_array_0,
        tick_array_1,
        tick_array_2,
        oracle: get_oracle_address(&WHIRLPOOL_PROGRAM_ID, pool_id),
    };
    let data = whirlpool::instruction::Swap {
        amount: *amount,
        other_amount_threshold: slippage_adjusted_quote,
        sqrt_price_limit,
        amount_specified_is_input,
        a_to_b,
    };

    let ix = OrcaInstructionBuilder {
        program_id: whirlpool::id(),
        accounts,
        data,
    };

    /*let (in_token_decimals, out_token_decimals) = if a_to_b {
        (pool_info.token_a.decimals, pool_info.token_b.decimals)
    } else {
        (pool_info.token_b.decimals, pool_info.token_a.decimals)
    };
    let normalized_input = (amount as f64).div(10_u64.pow(in_token_decimals as u32) as f64);
    let normalized_output = (quote as f64).div(10_u64.pow(out_token_decimals as u32) as f64);
    let normalized_output_with_slippage =
        (slippage_adjusted_quote as f64).div(10_u64.pow(out_token_decimals as u32) as f64);
    info!(
        "amount={}. quote={}. quote-with-slippage={}",
        normalized_input, normalized_output, normalized_output_with_slippage
    );*/
    info!(
        "amount={}. quote={}, quote-with-slippage={}",
        amount, quote, slippage_adjusted_quote
    );
    Ok((quote, slippage_adjusted_quote, ix))
}

/// Returns `(quote, slippage_adjusted_quote, swap_ix)`
pub async fn quote_with_api_info(
    keypair: Arc<Keypair>,
    client: Arc<RpcClient>,
    pool_info: &IWhirlPool,
    swap_input: &SwapInput,
) -> anyhow::Result<(u64, u64, OrcaInstructionBuilder)> {
    let SwapInput {
        input_token_mint,
        output_token_mint,
        slippage_bps,
        amount,
        mode,
        market: _,
    } = swap_input;

    let amount_specified_is_input = mode.amount_specified_is_input();
    let a_to_b = if pool_info.token_a.mint == *input_token_mint
        && pool_info.token_b.mint == *output_token_mint
    {
        true
    } else if pool_info.token_b.mint == *input_token_mint
        && pool_info.token_a.mint == *output_token_mint
    {
        false
    } else {
        return Err(anyhow!(
            "Whirlpool state from API doesn't match specified mints"
        ));
    };

    let (in_token, out_token) = if a_to_b {
        (pool_info.token_a.mint, pool_info.token_b.mint)
    } else {
        (pool_info.token_b.mint, pool_info.token_a.mint)
    };

    info!("whirlpool quote. in={}. out={}", in_token, out_token);
    info!(
        "a_to_b={}. amount_specified_is_input={}. tick-spacing={}",
        a_to_b, amount_specified_is_input, pool_info.tick_spacing
    );

    let whirlpool_account = client.get_account(&pool_info.address).await?;
    let whirlpool = deserialize_anchor_account::<Whirlpool>(&whirlpool_account)?;
    let (tick_arrays, tick_array_keys) = get_tick_arrays(
        &client,
        whirlpool.tick_current_index,
        whirlpool.tick_spacing as i32,
        a_to_b,
        &WHIRLPOOL_PROGRAM_ID,
        &pool_info.address,
    )
    .await?;
    let mut tick_arrays = tick_arrays
        .into_iter()
        .map(|a| Rc::new(RefCell::new(a)))
        .collect::<VecDeque<_>>();

    let tick_array_0 = tick_arrays.pop_front().unwrap();
    let tick_array_1 = tick_arrays.pop_front().unwrap();
    let tick_array_2 = tick_arrays.pop_front().unwrap();

    let mut swap_tick_sequence = SwapTickSequence::new(
        tick_array_0.try_borrow_mut()?,
        tick_array_1.try_borrow_mut().ok(),
        tick_array_2.try_borrow_mut().ok(),
    );

    let sqrt_price_limit = get_default_sqrt_price_limit(a_to_b);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let swap_result = swap(
        &whirlpool,
        &mut swap_tick_sequence,
        *amount,
        sqrt_price_limit,
        amount_specified_is_input,
        a_to_b,
        timestamp,
    )?;

    let quote = if a_to_b {
        swap_result.amount_b
    } else {
        swap_result.amount_a
    };
    let (amount_in, amount_out) = if a_to_b == amount_specified_is_input {
        (swap_result.amount_a, swap_result.amount_b)
    } else {
        (swap_result.amount_b, swap_result.amount_a)
    };

    let slippage_adjusted_quote = calculate_swap_amounts_from_quote(
        amount_in,
        amount_out,
        *slippage_bps as f64 / 1000.0,
        amount_specified_is_input,
    );

    #[allow(clippy::get_first)]
    let tick_array_0 = *tick_array_keys.get(0).expect("At least one tick array");
    let tick_array_1 = *tick_array_keys.get(1).unwrap_or(&tick_array_0);
    let tick_array_2 = *tick_array_keys.get(2).unwrap_or(&tick_array_1);
    let accounts = whirlpool::accounts::Swap {
        token_program: spl_token::ID,
        token_authority: keypair.pubkey(),
        whirlpool: pool_info.address,
        token_owner_account_a: Pubkey::default(),
        token_owner_account_b: Pubkey::default(),
        token_vault_a: whirlpool.token_vault_a,
        token_vault_b: whirlpool.token_vault_b,
        tick_array_0,
        tick_array_1,
        tick_array_2,
        oracle: get_oracle_address(&WHIRLPOOL_PROGRAM_ID, &pool_info.address),
    };
    let data = whirlpool::instruction::Swap {
        amount: *amount,
        other_amount_threshold: slippage_adjusted_quote,
        sqrt_price_limit,
        amount_specified_is_input,
        a_to_b,
    };

    let ix = OrcaInstructionBuilder {
        program_id: whirlpool::id(),
        accounts,
        data,
    };

    let (in_token_decimals, out_token_decimals) = if a_to_b {
        (pool_info.token_a.decimals, pool_info.token_b.decimals)
    } else {
        (pool_info.token_b.decimals, pool_info.token_a.decimals)
    };
    let normalized_input = (*amount as f64).div(10_u64.pow(in_token_decimals as u32) as f64);
    let normalized_output = (quote as f64).div(10_u64.pow(out_token_decimals as u32) as f64);
    let normalized_output_with_slippage =
        (slippage_adjusted_quote as f64).div(10_u64.pow(out_token_decimals as u32) as f64);
    info!(
        "amount={}. quote={}. quote-with-slippage={}",
        normalized_input, normalized_output, normalized_output_with_slippage
    );
    Ok((quote, slippage_adjusted_quote, ix))
}

pub struct OrcaInstructionBuilder {
    accounts: whirlpool::accounts::Swap,
    data: whirlpool::instruction::Swap,
    program_id: Pubkey,
}

impl OrcaInstructionBuilder {
    pub fn update_user_account(&mut self, key: Pubkey) {
        self.accounts.token_authority = key;
    }

    pub fn update_user_token_account_a(&mut self, key: Pubkey) {
        self.accounts.token_owner_account_a = key;
    }

    pub fn update_user_token_account_b(&mut self, key: Pubkey) {
        self.accounts.token_owner_account_b = key
    }

    pub fn update_program_id(&mut self, key: Pubkey) {
        self.program_id = key
    }

    pub fn build(self) -> solana_sdk::instruction::Instruction {
        if self.accounts.token_owner_account_a == Pubkey::default()
            || self.accounts.token_owner_account_b == Pubkey::default()
        {
            panic!("Failed to update one or both of input-token-account/output-token-account");
        }

        Instruction {
            program_id: self.program_id,
            accounts: anchor_lang::ToAccountMetas::to_account_metas(&self.accounts, None),
            data: anchor_lang::InstructionData::data(&self.data),
        }
    }
}
