#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(clippy::new_without_default)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::identity_op)]
// #![warn(clippy::future_not_send)]

pub mod analytics;
pub mod constants;
pub mod core;
pub mod database;
pub mod parser;
pub mod services;
pub mod swap;
pub mod utils;
pub mod wire;

// Re-exports so the generated code section compiles
use parser::generated::{
    jupiter_aggregator_v6::JUPITER_AGGREGATOR_V6_ID, orca_whirlpools::ORCA_WHIRLPOOLS_ID,
    pump::PUMP_ID,
};

use std::time::Duration;

use solana_sdk::commitment_config::CommitmentLevel;
use solana_sdk::pubkey::Pubkey;

#[derive(Clone, Debug)]
pub struct Config {
    rpc_url: String,
    ws_url: String,
    addresses: Vec<Pubkey>,
    block_subscription_commitment: Option<CommitmentLevel>,
    block_ws_reconnect_after: Duration,
}

// TODO: See `https://cybernetist.com/2024/04/19/rust-tokio-task-cancellation-patterns/` for cancellation patterns
