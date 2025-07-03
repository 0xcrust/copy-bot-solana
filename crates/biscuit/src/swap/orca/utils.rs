use crate::swap::adjust_for_slippage;
use crate::utils::deserialize_anchor_account;

use anyhow::{anyhow, Result};
use log::warn;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;

use whirlpool::math::tick_math::{MAX_SQRT_PRICE_X64, MIN_SQRT_PRICE_X64};
use whirlpool::state::{
    tick::{MAX_TICK_INDEX, MIN_TICK_INDEX, TICK_ARRAY_SIZE},
    TickArray,
};

/// The maximum number of tick-arrays that can traversed across in a swap
const MAX_SWAP_TICK_ARRAYS: u16 = 3;
const PDA_TICK_ARRAY_SEED: &str = "tick_array";

pub fn get_default_sqrt_price_limit(a_to_b: bool) -> u128 {
    if a_to_b {
        MIN_SQRT_PRICE_X64
    } else {
        MAX_SQRT_PRICE_X64
    }
}

pub async fn get_tick_arrays(
    client: &RpcClient,
    tick_current_index: i32,
    tick_spacing: i32,
    a_to_b: bool,
    program_id: &Pubkey,
    whirlpool_address: &Pubkey,
) -> Result<(Vec<TickArray>, Vec<Pubkey>)> {
    let keys = get_tick_array_keys(
        tick_current_index,
        tick_spacing,
        a_to_b,
        program_id,
        whirlpool_address,
    );
    let accounts = client.get_multiple_accounts(&keys).await?;
    let mut tick_arrays = Vec::with_capacity(accounts.len());
    for account in accounts {
        let tick_array =
            deserialize_anchor_account::<TickArray>(account.as_ref().expect("No account data"))?;
        tick_arrays.push(tick_array);
    }

    Ok((tick_arrays, keys))
}

pub fn get_tick_array_keys(
    tick_current_index: i32,
    tick_spacing: i32,
    a_to_b: bool,
    program_id: &Pubkey,
    whirlpool_address: &Pubkey,
) -> Vec<Pubkey> {
    let shift = if a_to_b { 0 } else { tick_spacing };

    let mut offset = 0;
    let mut addresses = Vec::with_capacity(MAX_SWAP_TICK_ARRAYS as usize);
    for i in 0..MAX_SWAP_TICK_ARRAYS {
        if let Ok(start_index) =
            get_start_tick_index(tick_current_index + shift, tick_spacing, offset)
        {
            let address = get_tick_array_address(program_id, whirlpool_address, start_index);
            addresses.push(address);
            if a_to_b {
                offset -= 1;
            } else {
                offset += 1;
            }
        } else {
            warn!("Failed to get start-tick-index. i={}", i);
            break;
        }
    }

    addresses
}

pub fn get_start_tick_index(tick_index: i32, tick_spacing: i32, offset: i32) -> Result<i32> {
    let real_index =
        ((tick_index as f64 / tick_spacing as f64 / TICK_ARRAY_SIZE as f64).floor()) as i32;
    let start_tick_index = (real_index + offset) * tick_spacing * TICK_ARRAY_SIZE;

    let ticks_in_array = TICK_ARRAY_SIZE * tick_spacing;
    let min_tick_index = MIN_TICK_INDEX - ((MIN_TICK_INDEX % ticks_in_array) + ticks_in_array);
    if start_tick_index <= min_tick_index {
        warn!(
            "start_tick_index is less than min_tick_index={}",
            min_tick_index
        );
        return Err(anyhow!(
            "startTickIndex is too small - - ${start_tick_index}"
        ));
    }
    if start_tick_index >= MAX_TICK_INDEX {
        warn!(
            "start_tick_index is greater than max_tick_index={}",
            MAX_TICK_INDEX
        );
        return Err(anyhow!(
            "startTickIndex is too large - - ${start_tick_index}"
        ));
    }

    Ok(start_tick_index)
}

pub fn get_tick_array_address(program_id: &Pubkey, whirlpool: &Pubkey, start_tick: i32) -> Pubkey {
    Pubkey::find_program_address(
        &[
            PDA_TICK_ARRAY_SEED.as_bytes(),
            whirlpool.to_bytes().as_ref(),
            start_tick.to_string().as_bytes(),
        ],
        program_id,
    )
    .0
}

pub fn get_oracle_address(program_id: &Pubkey, whirlpool: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &["oracle".as_bytes(), whirlpool.to_bytes().as_ref()],
        program_id,
    )
    .0
}

/// Returns the other_amount_threshhold
pub fn calculate_swap_amounts_from_quote(
    est_amount_in: u64,
    est_amount_out: u64,
    slippage: f64,
    amount_specified_is_input: bool,
) -> u64 {
    if amount_specified_is_input {
        adjust_for_slippage(est_amount_out, slippage, false)
    } else {
        adjust_for_slippage(est_amount_in, slippage, true)
    }
}
