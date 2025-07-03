pub mod builder;
pub mod execution;
pub mod jupiter;
pub mod orca;
pub mod pump;
pub mod raydium;
pub mod token_extensions;
pub mod utils;

use solana_account_decoder::UiAccountEncoding;
use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
use solana_client::rpc_filter::RpcFilterType;
use solana_sdk::commitment_config::CommitmentConfig;
use std::ops::Mul;

/// `slippage` here is expressed as a fraction of the amount.
/// - if we have our slippage expressed in bps then we should divide by 10000 before passing it into this function
/// - if we have our slippage expressed in % then we should divide by 100 before passing it into this function
pub fn adjust_for_slippage(amount: u64, slippage: f64, round_up: bool) -> u64 {
    if round_up {
        (amount as f64).mul(1_f64 + slippage).ceil() as u64
    } else {
        (amount as f64).mul(1_f64 - slippage).floor() as u64
    }
}

pub fn get_program_accounts_config(
    filters: Option<Vec<RpcFilterType>>,
    commitment: Option<CommitmentConfig>,
) -> RpcProgramAccountsConfig {
    RpcProgramAccountsConfig {
        filters,
        account_config: RpcAccountInfoConfig {
            encoding: Some(UiAccountEncoding::Base64),
            commitment,
            ..RpcAccountInfoConfig::default()
        },
        with_context: Some(true),
    }
}
