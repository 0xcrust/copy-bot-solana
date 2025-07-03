use crate::analytics::birdeye::HistoricalPrice;
use crate::analytics::dump::MintInfo;
use crate::analytics::price_history::get_price_for_timestamp;
use crate::constants::mints::{SOL, USDC, USDT};
use crate::core::types::transaction::ITransaction;
use crate::parser::swap::ResolvedSwap;
use crate::parser::v1::V1Parser;
use crate::services::copy::config::{CopyConfig, CopyMode};
use crate::services::copy::history::{calculate_token_prices, HistoricalTrades, TradeDirection};
use crate::services::copy::v2::ping::{Ping, PingTracker};
use crate::services::copy::v2::service::CopyMetrics;
use crate::swap::jupiter::executor::JupiterExecutor;
use crate::swap::jupiter::price::PriceFeed;
use crate::wire::helius_rpc::PriorityLevel;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::native_token::LAMPORTS_PER_SOL;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signature};
use thiserror::Error;

use super::execution::{CopyType, ExecutionHandler};
use super::service::{Balance, CopyDetails, RetryStrategy, WalletConfig};

pub const SOL_DECIMALS: u8 = 9;
pub const TOKEN_ACCOUNT_CREATION_LAMPORTS: u64 = 2000000;

#[derive(Clone)]
pub struct Simulator {
    pub lamports_bal: u64,
    pub log_swaps: bool,
    pub execution_handler: ExecutionHandler<JupiterExecutor>,
    pub wallets: HashMap<Pubkey, WalletConfig>,
    pub token_balances: HashMap<Pubkey, u64>,
    pub sol_price_history: Vec<HistoricalPrice>,
    pub decimals: HashMap<Pubkey, u8>,
}

#[derive(Debug, Error)]
pub enum SimulationError {
    #[error("Simulator ran out of lamports balance")]
    InsufficientLamportBalance,
    #[error("Simulator ran out of token balance. Token={0}")]
    InsufficientTokenBalance(Pubkey),
    #[error(transparent)]
    Any(#[from] anyhow::Error),
}

impl Simulator {
    pub fn insert_wallet(&mut self, wallet: Pubkey, config: WalletConfig) {
        _ = self.wallets.insert(wallet, config);
    }

    pub async fn start(
        initial_lamports: u64,
        log_swaps: bool,
        sol_price_history: Vec<HistoricalPrice>,
        rpc_client: Arc<RpcClient>,
        config: Arc<CopyConfig>,
        wallets: HashMap<Pubkey, WalletConfig>,
    ) -> anyhow::Result<Self> {
        log::info!(
            "Starting simulator. Initial SOL balance: {}",
            initial_lamports / LAMPORTS_PER_SOL
        );
        let (_price_feed_task, price_feed) = PriceFeed::start(
            config.price_feed_url.clone(),
            config.price_feed_refresh_frequency_ms,
            None,
        );
        price_feed.subscribe_token(&SOL, Some(Duration::MAX));
        price_feed.subscribe_token(&USDC, Some(Duration::MAX));
        price_feed.subscribe_token(&USDT, Some(Duration::MAX));
        price_feed.refresh().await?;
        let (execution_output_sender, _) = tokio::sync::mpsc::unbounded_channel();
        let ping_tracker = match &config.ping_config_path {
            Some(path) => Some(PingTracker::start(&path, false)?),
            None => None,
        };
        let handler = ExecutionHandler {
            keypair: Arc::new(Keypair::new()),
            jito_tips: Default::default(),
            config: Arc::clone(&config),
            rpc_client: Arc::clone(&rpc_client),
            trade_history: HistoricalTrades::default(),
            ping_tracker,
            priofees_handle: None,
            swap_executor: JupiterExecutor::new(None, rpc_client),
            execution_output_sender,
            price_feed,
            buy_disabled: Default::default(),
            metrics: Arc::new(CopyMetrics::default()),
            buy_disabled_wallets: Default::default(),
        };

        let mut simulator = Simulator {
            lamports_bal: initial_lamports,
            log_swaps,
            execution_handler: handler,
            wallets,
            token_balances: HashMap::default(),
            sol_price_history,
            decimals: HashMap::default(),
        };
        simulator.decimals.insert(SOL, SOL_DECIMALS);

        Ok(simulator)
    }

    pub async fn process_txn(&mut self, txn: ITransaction) -> Result<(), SimulationError> {
        let swap = V1Parser::parse_swap_transaction(&txn)?;
        let Some(swap) = swap else { return Ok(()) };
        let Some(signer_balance) = Balance::create_sol_balance(&txn, 0) else {
            return Err(anyhow!(
                "Failed to get signer balance for txn {}",
                txn.signature
            ))?;
        };
        let origin_signer = *txn
            .account_keys()
            .get(0)
            .context("no keys present for txn")?;
        self.process_swap(
            swap.clone(),
            Balance::create_sol_balance(&txn, 0),
            Balance::create_token_balance(&txn, &origin_signer, &swap.input_token_mint),
            Balance::create_token_balance(&txn, &origin_signer, &swap.output_token_mint),
            origin_signer,
            txn.signature,
            txn.block_time
                .context(anyhow!("blocktime is null for tx {}", txn.signature))?,
        )
        .await
    }

    pub async fn process_swap(
        &mut self,
        swap: ResolvedSwap,
        origin_sol_balance: Option<Balance>,
        origin_input_mint_balance: Option<Balance>,
        origin_output_mint_balance: Option<Balance>,
        origin_signer: Pubkey,
        origin_signature: Signature,
        blocktime: i64,
    ) -> Result<(), SimulationError> {
        let Some(wallet_config) = self.wallets.get(&origin_signer) else {
            return Ok(());
        };

        let Some(origin_decimals) = swap.token_decimals else {
            return Err(anyhow!("Token decimals not resolved for swap"))?;
        };
        self.decimals
            .insert(swap.input_token_mint, origin_decimals.input);
        self.decimals
            .insert(swap.output_token_mint, origin_decimals.output);

        let mut details = CopyDetails {
            origin_signature,
            origin_signer,
            origin_sol_balance,
            origin_input_mint_balance,
            origin_output_mint_balance,
            origin_swap: swap.clone(),
            send_txn_retries: self.execution_handler.config.send_txn_retries,
            confirmed_error_retries: self.execution_handler.config.confirmed_error_retries,
            retry_strategy: RetryStrategy::BuildTransaction,
            wallet_config: wallet_config.clone(),
            next_priority_level: PriorityLevel::High,
            send_to_jito: true,
            last_fee_lamports: 0,
            quote_output: None,
            copy_type: None,
        };
        let execution_input = self
            .execution_handler
            .get_execution_input(&mut details)
            .await?;

        let Some(exec) = execution_input else {
            return Ok(());
        };
        let current_sol_price = self
            .execution_handler
            .price_feed
            .get_price_usd(&SOL)
            .and_then(|p| p.price());
        let sol_price = get_price_for_timestamp(&self.sol_price_history, blocktime);
        let sol_price = match sol_price {
            Some(price) => price,
            None => {
                let current_timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64;
                let most_recent_time = self
                    .sol_price_history
                    .last()
                    .map(|x| x.unix_time)
                    .unwrap_or(current_timestamp);

                if current_timestamp - most_recent_time < 24 * 60 * 60 {
                    // max staleness is a day. use current price
                    current_sol_price
                        .context("Failed to get current sol price while getting historical price")?
                } else {
                    return Err(anyhow!("Failed to get historical sol price"))?;
                }
            }
        };
        let origin_price = calculate_token_prices(
            &details.origin_swap.input_token_mint,
            &details.origin_swap.output_token_mint,
            details.origin_swap.input_amount,
            details.origin_swap.output_amount,
            origin_decimals.input,
            origin_decimals.output,
            sol_price,
        );

        let origin_price = match origin_price {
            Ok(price) => price,
            Err(e) => return Err(anyhow!("{}", e))?,
        };
        let Some(copy_type) = details.copy_type else {
            return Err(anyhow!("No copy-type set for swap"))?;
        };

        let (
            exec_input_decimals,
            exec_output_decimals,
            exec_input_price_usd,
            exec_output_price_usd,
        ) = match copy_type {
            CopyType::Buy => (
                SOL_DECIMALS,
                origin_decimals.output,
                origin_price.sol_price_usd,
                origin_price.output_price_usd,
            ),
            CopyType::Sell | CopyType::Both => (
                origin_decimals.input,
                SOL_DECIMALS,
                origin_price.input_price_usd,
                origin_price.sol_price_usd,
            ),
        };

        log::debug!(
            "exec-input-price={}, exec-output-price={}, origin-input-price={}, origin-output-price={}", 
            exec_input_price_usd, exec_output_price_usd, origin_price.input_price_usd, origin_price.output_price_usd
        );

        let exec_input_usd = (exec.amount as f64 / 10_u32.pow(exec_input_decimals as u32) as f64)
            * exec_input_price_usd as f64;

        let normalized_exec_input =
            exec.amount as f64 / 10_u32.pow(exec_input_decimals as u32) as f64;
        let mut exec_out_amount = (((normalized_exec_input * exec_input_price_usd)
            / exec_output_price_usd)
            * 10_u32.pow(exec_output_decimals as u32) as f64)
            .trunc() as u64;
        let dust_bps = self.execution_handler.config.moonbag_bps;
        if dust_bps > 10_000 {
            return Err(anyhow!("Invalid dust bps: {}", dust_bps))?;
        }
        let dust_amount = (dust_bps as u64 * exec_out_amount) / 10_000;
        exec_out_amount -= dust_amount;

        // let exec_output_usd = exec_input_usd; // can't use current price
        let exec_output_usd = (exec_out_amount as f64
            / 10_u32.pow(exec_output_decimals as u32) as f64)
            * exec_output_price_usd;
        let origin_output_usd = (details.origin_swap.output_amount as f64
            / 10_u32.pow(origin_decimals.output as u32) as f64)
            * origin_price.output_price_usd;

        let config = &self.execution_handler.config;
        let max_fee_cost = config.max_fee_lamports
            + (config.send_txn_retries as u64 * config.fee_increase_multiplier);
        if self.lamports_bal < max_fee_cost {
            return Err(SimulationError::InsufficientLamportBalance);
        }
        self.lamports_bal -= max_fee_cost;
        if !self.token_balances.contains_key(&exec.output_token_mint) {
            if self.lamports_bal < TOKEN_ACCOUNT_CREATION_LAMPORTS {
                return Err(SimulationError::InsufficientLamportBalance);
            } else {
                self.lamports_bal -= TOKEN_ACCOUNT_CREATION_LAMPORTS
            }
        }

        match copy_type {
            CopyType::Buy => {
                if self.lamports_bal < exec.amount {
                    return Err(SimulationError::InsufficientLamportBalance);
                }
                self.lamports_bal -= exec.amount;
                self.token_balances
                    .entry(exec.output_token_mint)
                    .and_modify(|bal| *bal += exec_out_amount)
                    .or_insert(exec_out_amount);
            }
            CopyType::Sell => {
                let Some(token_balance) = self.token_balances.get_mut(&exec.input_token_mint)
                else {
                    return Err(anyhow!("Attempted to sell token not present in balances"))?;
                };
                if *token_balance < exec.amount {
                    return Err(SimulationError::InsufficientTokenBalance(
                        exec.input_token_mint,
                    ));
                }
                *token_balance -= exec.amount;
                self.lamports_bal += exec_out_amount;
            }
            CopyType::Both => {
                let Some(input_token_balance) = self.token_balances.get_mut(&exec.input_token_mint)
                else {
                    return Err(anyhow!("Attempted to sell token not present in balances"))?;
                };
                if *input_token_balance < exec.amount {
                    return Err(SimulationError::InsufficientTokenBalance(
                        exec.input_token_mint,
                    ));
                }
                *input_token_balance -= exec.amount;
                self.token_balances
                    .entry(exec.output_token_mint)
                    .and_modify(|bal| *bal += exec_out_amount)
                    .or_insert(exec_out_amount);
            }
        }

        let origin = details.origin_swap;
        let origin_input_usd = (origin.input_amount as f64 * origin_price.input_price_usd)
            / 10_u32.pow(origin_decimals.input as u32) as f64;
        let origin_input_sol = origin_input_usd / origin_price.sol_price_usd;
        let exec_input_sol = exec_input_usd / origin_price.sol_price_usd;
        let origin_output_sol = origin_output_usd / origin_price.sol_price_usd;
        let exec_output_sol = exec_output_usd / origin_price.sol_price_usd;

        log::debug!(
            "origin: input={},output={},input-usd={},output-usd={},input-sol={},output-sol={},sol-price={}",
            origin.input_amount,
            origin.output_amount,
            origin_input_usd,
            origin_output_usd,
            origin_input_sol,
            origin_output_sol,
            origin_price.sol_price_usd,
        );
        log::debug!(
            "exec: input={},output={},input-usd={},output-usd={},input-sol={},output-sol={},sol-price={}",
            exec.amount,
            exec_out_amount,
            exec_input_usd,
            exec_output_usd,
            exec_input_sol,
            exec_output_sol,
            origin_price.sol_price_usd,
        );

        // TODO: Insert trades
        if matches!(copy_type, CopyType::Buy | CopyType::Both) {
            match details.wallet_config.mode {
                CopyMode::Direct => {
                    self.execution_handler.trade_history.insert_trade_inner(
                        &details.origin_signer,
                        &exec.output_token_mint,
                        exec_out_amount,
                        swap.output_amount,
                        exec_input_usd,
                        origin_input_usd,
                        exec_input_sol,
                        origin_input_sol,
                        dust_amount,
                        &TradeDirection::Buy,
                    )?;
                }
                CopyMode::Ping => {
                    let Some(ping_tracker) = self.execution_handler.ping_tracker.as_ref() else {
                        return Err(anyhow!("Ping tracker not set for simulator"))?;
                    };
                    let v = ping_tracker.insert_execution(
                        &exec.output_token_mint,
                        exec_output_decimals,
                        exec_out_amount,
                        exec_output_price_usd,
                        exec_output_price_usd / origin_price.sol_price_usd,
                        &TradeDirection::Buy,
                        Some(config.moonbag_bps),
                    )?;

                    match v.ping {
                        Some(ping) => info!("Ping={:#?}", ping),
                        None => error!(
                            "Trade executed but no pings found for token {}",
                            origin.output_token_mint
                        ),
                    }
                }
            }
        }

        if matches!(copy_type, CopyType::Sell | CopyType::Both) {
            let Some(ping_tracker) = self.execution_handler.ping_tracker.as_ref() else {
                return Err(anyhow!("Ping tracker not set for simulator"))?;
            };
            match details.wallet_config.mode {
                CopyMode::Direct => {
                    self.execution_handler.trade_history.insert_trade_inner(
                        &details.origin_signer,
                        &exec.input_token_mint,
                        exec.amount,
                        swap.input_amount,
                        exec_output_usd,
                        origin_output_usd,
                        exec_output_sol,
                        origin_output_sol,
                        0,
                        &TradeDirection::Sell,
                    )?;
                }
                CopyMode::Ping => {
                    let Some(ping_tracker) = self.execution_handler.ping_tracker.as_ref() else {
                        return Err(anyhow!("Ping tracker not set for simulator"))?;
                    };
                    let v = ping_tracker.insert_execution(
                        &exec.input_token_mint,
                        exec_input_decimals,
                        exec.amount,
                        exec_input_price_usd,
                        exec_input_price_usd / origin_price.sol_price_usd,
                        &TradeDirection::Sell,
                        Some(config.moonbag_bps),
                    )?;

                    match v.ping {
                        Some(ping) => info!("Ping={:#?}", ping),
                        None => error!(
                            "Trade executed but no pings found for token {}",
                            origin.output_token_mint
                        ),
                    }
                }
            }
        }

        let ping_tag = match details.wallet_config.mode {
            CopyMode::Direct => "",
            CopyMode::Ping => "[ping]: ",
        };
        let sol_balance = self.lamports_bal as f64 / LAMPORTS_PER_SOL as f64;
        let message = match copy_type {
            CopyType::Buy => {
                format!(
                    "{}Swapped {} of SOL ({:.3} usd) for {} of {}, balance={:.3} SOL",
                    ping_tag,
                    exec_input_sol,
                    exec_input_usd,
                    exec_out_amount,
                    origin.output_token_mint,
                    sol_balance
                )
            }
            CopyType::Both => {
                format!(
                    "{}Swapped {} of {} for {} of {}, balance={:.3} SOL",
                    ping_tag,
                    exec.amount,
                    exec.input_token_mint,
                    exec_out_amount,
                    exec.output_token_mint,
                    sol_balance
                )
            }
            CopyType::Sell => {
                format!(
                    "{}Swapped {} of {} for {} of SOL ({:.3} usd), balance={:.3} SOL",
                    ping_tag,
                    exec.amount,
                    origin.input_token_mint,
                    exec_output_sol,
                    exec_output_usd,
                    sol_balance
                )
            }
        };

        if self.log_swaps {
            log::info!("{}", message);
        } else {
            println!("{}", message);
        }

        Ok(())
    }

    pub async fn print_simulation_state(
        &self,
        json_out: Option<impl AsRef<std::path::Path>>,
        token_info: Option<HashMap<Pubkey, MintInfo>>,
    ) -> anyhow::Result<()> {
        let price_feed = &self.execution_handler.price_feed;
        let mut tokens = std::collections::HashSet::new();
        tokens.extend(self.token_balances.keys().copied());
        let ping_snapshot = self
            .execution_handler
            .ping_tracker
            .as_ref()
            .map(|t| t.to_snapshot());
        if let Some(snapshot) = ping_snapshot {
            for (token, _) in &snapshot.execution {
                let Some(key) = Pubkey::from_str(token).ok() else {
                    log::error!("Failed to convert snapshot token {} to pubkey", token);
                    continue;
                };
                tokens.insert(key);
            }
        }
        tokens.extend([SOL, USDC, USDT]);
        for token in tokens {
            price_feed.subscribe_token(&token, Some(Duration::from_secs(u64::MAX)));
        }
        price_feed.refresh().await?;

        let sol_balance_f64 = self.lamports_bal as f64 / LAMPORTS_PER_SOL as f64;
        let sol_price_usd = price_feed
            .get_price_usd(&SOL)
            .and_then(|res| res.price())
            .context("Failed to get current sol price")?;
        let sol_value_usd = sol_balance_f64 * sol_price_usd;
        log::info!("\n******* [Simulation State] *******");
        log::info!(
            "* SOL balance={:.3}, value={}",
            sol_balance_f64,
            sol_value_usd
        );
        let mut total_token_balance_value_usd = 0.0;

        // log::info!(
        //     "******* Printing {} token balances *******",
        //     self.token_balances.len()
        // );
        // let mut balances = Vec::with_capacity(self.token_balances.len());
        for (token, amount) in &self.token_balances {
            let Some(decimals) = self.decimals.get(token) else {
                warn!(
                    "failed to get decimals for token {}, skipping for simulation stats",
                    token
                );
                continue;
            };
            let price_usd = price_feed
                .get_price_usd(token)
                .and_then(|p| p.price())
                .unwrap_or_default();
            let value_usd = (*amount as f64 / 10_u32.pow(*decimals as u32) as f64) * price_usd;
            total_token_balance_value_usd += value_usd;
            // balances.push(RawBalance {
            //     token: *token,
            //     amount: *amount,
            //     value_usd,
            // });
        }
        // balances.sort_by(|a, b| {
        //     b.value_usd
        //         .partial_cmp(&a.value_usd)
        //         .unwrap_or(Ordering::Equal)
        // });
        // for balance in balances {
        //     log::info!(
        //         "* {}, value={}, amount={}",
        //         balance.token, balance.value_usd, balance.amount
        //     );
        // }

        let total_balance_usd = sol_value_usd + total_token_balance_value_usd;
        let total_balance_sol = total_balance_usd / sol_price_usd;
        log::info!(
            "\ntotal-simulated-balance = {} USD ({} SOL)",
            total_balance_usd,
            total_balance_sol
        );

        if let Some(tracker) = self.execution_handler.ping_tracker.as_ref() {
            log::info!(
                "******* Printing {} Ping tracker details *******",
                self.token_balances.len()
            );
            let mut snapshot = tracker.to_snapshot();
            snapshot.write_to_file(&tracker.config.snapshot_path)?;

            let token_info = token_info.unwrap_or_default();
            let mut balances = Vec::new();
            for (token, history) in snapshot.execution {
                let Ok(token_key) = Pubkey::from_str(&token) else {
                    error!("unreachable!: couldn't convert token {} to pubkey", token);
                    continue;
                };
                let Some(pings) = snapshot.pings.remove(&token) else {
                    error!(
                        "unreachable!: no ping found for execution for token {}",
                        token
                    );
                    continue;
                };
                let Some(decimals) = self.decimals.get(&token_key) else {
                    error!(
                        "failed to get decimals for token {}, skipping for tracker stats",
                        token
                    );
                    continue;
                };
                let amount = history
                    .buy_volume_token
                    .saturating_sub(history.sell_volume_token);
                let price_usd = price_feed
                    .get_price_usd(&token_key)
                    .and_then(|p| p.price())
                    .unwrap_or_default();
                let value_usd = (amount as f64 / 10_u32.pow(*decimals as u32) as f64) * price_usd;
                let pnl_usd = (history.sell_volume_usd + value_usd) - history.buy_volume_usd;
                let token = token_info
                    .get(&token_key)
                    .map(|token| Token {
                        pubkey: token.pubkey.to_string(),
                        symbol: token.symbol.clone(),
                        name: token.name.clone(),
                    })
                    .unwrap_or(Token {
                        pubkey: token_key.to_string(),
                        symbol: None,
                        name: None,
                    });
                balances.push(PingBalance {
                    token,
                    total_buy_value_usd: history.buy_volume_usd,
                    total_sell_value_usd: history.sell_volume_usd,
                    total_holding_value_usd: value_usd,
                    pnl_usd,
                    pings,
                })
            }
            balances.sort_by(|a, b| b.pnl_usd.partial_cmp(&a.pnl_usd).unwrap_or(Ordering::Equal));
            let total_pnl = balances
                .iter()
                .map(|x| x.pnl_usd)
                .fold(0.0, |acc, x| acc + x);
            let total_holding_value = balances
                .iter()
                .map(|x| x.total_holding_value_usd)
                .fold(0.0, |acc, x| acc + x);
            let total_execution_count = balances
                .iter()
                .map(|x| x.pings.execution_count)
                .fold(0, |acc, x| acc as u64 + x as u64);
            log::info!(
                "total-ping-tracker-balance: {} USD",
                total_pnl + total_holding_value
            );
            log::info!("total-tokens-traded: {}", balances.len());
            log::info!("total execution count: {}", total_execution_count);
            if let Some(path) = json_out {
                std::fs::write(&path, serde_json::to_string_pretty(&balances)?)?;
                log::info!("Output wallet results to {}", path.as_ref().display());
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PingBalance {
    pub token: Token,
    pub total_buy_value_usd: f64,
    pub total_sell_value_usd: f64,
    pub total_holding_value_usd: f64,
    pub pnl_usd: f64,
    pub pings: Ping,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Token {
    pub pubkey: String,
    pub symbol: Option<String>,
    pub name: Option<String>,
}

struct RawBalance {
    token: Pubkey,
    amount: u64,
    value_usd: f64,
}
