use crate::analytics::dump::MintInfo;
use crate::analytics::price_history::get_price_for_timestamp;
use crate::analytics::HistoricalPrice;
use crate::constants::mints::{SOL, USDT};
use crate::parser::swap::ResolvedSwap;
use crate::services::copy::history::calculate_token_prices;
use crate::swap::jupiter::price::make_chunked_price_request_usd;
use crate::utils::serde_helpers::field_as_string;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use anchor_spl::mint::USDC;
use anyhow::{anyhow, Context};
use chrono::{DateTime, Utc};
use rayon::slice::ParallelSliceMut;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;

pub struct WalletAnalyzer {
    wallets: HashSet<Pubkey>,
    sol_price_history: Vec<HistoricalPrice>,
    wallet_stats: HashMap<Pubkey, WalletStats>,
    current_sol_price: f64,
}

impl WalletAnalyzer {
    pub async fn new(
        wallets: HashSet<Pubkey>,
        sol_price_history: Vec<HistoricalPrice>,
        price_url: Option<String>,
    ) -> anyhow::Result<Self> {
        let price_map = make_chunked_price_request_usd(price_url, &[SOL.to_string()]).await;
        let current_sol_price = price_map
            .get(&SOL.to_string())
            .and_then(|d| d.as_ref().map(|d| d.price))
            .context("Failed to get SOL price")?;

        Ok(WalletAnalyzer {
            wallets,
            sol_price_history,
            wallet_stats: HashMap::default(),
            current_sol_price,
        })
    }

    pub fn insert_wallet(&mut self, wallet: Pubkey) {
        _ = self.wallets.insert(wallet);
    }

    pub fn process_swap(
        &mut self,
        swap: ResolvedSwap,
        signature: Signature,
        signer: Pubkey,
        block_time: i64,
    ) -> anyhow::Result<()> {
        if !self.wallets.contains(&signer) {
            return Ok(());
        }

        self.wallet_stats.entry(signer).or_insert(WalletStats {
            address: signer,
            ..Default::default()
        });

        let wallet_stats = self.wallet_stats.get_mut(&signer).unwrap();

        let Some(decimals) = swap.token_decimals else {
            wallet_stats.failed_signatures.push(signature);
            return Err(anyhow!(
                "Token decimals not resolved for swap, signature={}",
                signature
            ))?;
        };

        match wallet_stats.earliest_txn {
            Some(timestamp) => {
                if block_time < timestamp.timestamp() {
                    wallet_stats.earliest_txn = Some(
                        DateTime::from_timestamp(block_time, 0)
                            .context("failed converting blocktime to date")?,
                    );
                }
            }
            None => {
                wallet_stats.earliest_txn = Some(
                    DateTime::from_timestamp(block_time, 0)
                        .context("failed converting blocktime to date")?,
                );
            }
        }

        match wallet_stats.latest_txn {
            Some(timestamp) => {
                if block_time > timestamp.timestamp() {
                    wallet_stats.latest_txn = Some(
                        DateTime::from_timestamp(block_time, 0)
                            .context("failed converting blocktime to date")?,
                    );
                }
            }
            None => {
                wallet_stats.latest_txn = Some(
                    DateTime::from_timestamp(block_time, 0)
                        .context("failed converting blocktime to date")?,
                );
            }
        }

        wallet_stats
            .token_stats
            .entry(swap.input_token_mint)
            .and_modify(|stats| stats.total_sells += 1)
            .or_insert(TokenStats {
                decimals: decimals.input,
                total_sells: 1,
                ..Default::default()
            });
        wallet_stats
            .token_stats
            .entry(swap.output_token_mint)
            .and_modify(|stats| stats.total_buys += 1)
            .or_insert(TokenStats {
                decimals: decimals.output,
                total_buys: 1,
                ..Default::default()
            });

        let sol_price = get_price_for_timestamp(&self.sol_price_history, block_time);
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
                    self.current_sol_price
                } else {
                    wallet_stats.failed_signatures.push(signature);
                    return Err(anyhow!("Failed to get historical sol price"))?;
                }
            }
        };

        let origin_price = calculate_token_prices(
            &swap.input_token_mint,
            &swap.output_token_mint,
            swap.input_amount,
            swap.output_amount,
            decimals.input,
            decimals.output,
            sol_price,
        );

        let origin_price = match origin_price {
            Ok(price) => price,
            Err(e) => {
                wallet_stats.failed_signatures.push(signature);
                return Err(anyhow!("{}", e))?;
            }
        };

        let input_volume_usd = ((swap.input_amount as f64)
            / 10_u32.pow(decimals.input as u32) as f64)
            * origin_price.input_price_usd;
        let output_volume_usd = ((swap.output_amount as f64)
            / 10_u32.pow(decimals.output as u32) as f64)
            * origin_price.input_price_usd;

        let buy = !matches!(swap.output_token_mint, SOL | USDC | USDT);
        let sell = !matches!(swap.input_token_mint, SOL | USDC | USDT);

        let (input_mint_swap, output_mint_swap) = match (buy, sell) {
            (false, false) => (None, None),
            (true, false) => {
                // Buy only. Register a swap for only the output mint
                wallet_stats.num_swaps += 1;
                wallet_stats.total_buys += 1;
                wallet_stats.total_buy_volume_usd += input_volume_usd;

                let output_token_stats = wallet_stats
                    .token_stats
                    .get_mut(&swap.output_token_mint)
                    .expect("unreachable");
                output_token_stats.total_buys += 1;
                output_token_stats.buy_volume_token += swap.output_amount;
                output_token_stats.buy_volume_usd += input_volume_usd;
                (
                    None,
                    Some(Swap {
                        signature,
                        block_time,
                        token: swap.output_token_mint,
                        other_token: swap.input_token_mint,
                        amount: swap.output_amount,
                        other_amount: swap.input_amount,
                        volume_token_usd: output_volume_usd,
                        volume_other_token_usd: input_volume_usd,
                        direction: Direction::Buy,
                    }),
                )
            }
            (false, true) => {
                // Sell only. Register a swap for only the input mint
                wallet_stats.num_swaps += 1;
                wallet_stats.total_sells += 1;
                wallet_stats.total_sell_volume_usd += output_volume_usd;

                let input_token_stats = wallet_stats
                    .token_stats
                    .get_mut(&swap.input_token_mint)
                    .expect("unreachable");
                input_token_stats.total_sells += 1;
                input_token_stats.sell_volume_token += swap.input_amount;
                input_token_stats.sell_volume_usd += output_volume_usd;
                (
                    Some(Swap {
                        signature,
                        block_time,
                        token: swap.input_token_mint,
                        other_token: swap.output_token_mint,
                        amount: swap.input_amount,
                        other_amount: swap.output_amount,
                        volume_token_usd: input_volume_usd,
                        volume_other_token_usd: output_volume_usd,
                        direction: Direction::Sell,
                    }),
                    None,
                )
            }
            (true, true) => {
                // Both a buy and a sell. Register swaps for both mints
                wallet_stats.num_swaps += 2;
                wallet_stats.total_buys += 1;
                wallet_stats.total_buy_volume_usd += input_volume_usd;
                let output_token_stats = wallet_stats
                    .token_stats
                    .get_mut(&swap.output_token_mint)
                    .expect("unreachable");
                output_token_stats.total_buys += 1;
                output_token_stats.buy_volume_token += swap.output_amount;
                output_token_stats.buy_volume_usd += input_volume_usd;

                wallet_stats.total_sells += 1;
                wallet_stats.total_sell_volume_usd += output_volume_usd;
                let input_token_stats = wallet_stats
                    .token_stats
                    .get_mut(&swap.input_token_mint)
                    .expect("unreachable");
                input_token_stats.total_sells += 1;
                input_token_stats.sell_volume_token += swap.input_amount;
                input_token_stats.sell_volume_usd += output_volume_usd;

                let sell = Swap {
                    signature,
                    block_time,
                    token: swap.input_token_mint,
                    other_token: swap.output_token_mint,
                    amount: swap.input_amount,
                    other_amount: swap.output_amount,
                    volume_token_usd: input_volume_usd,
                    volume_other_token_usd: output_volume_usd,
                    direction: Direction::Sell,
                };
                let buy = Swap {
                    signature,
                    block_time,
                    token: swap.output_token_mint,
                    other_token: swap.input_token_mint,
                    amount: swap.output_amount,
                    other_amount: swap.input_amount,
                    volume_token_usd: output_volume_usd,
                    volume_other_token_usd: input_volume_usd,
                    direction: Direction::Buy,
                };
                (Some(sell), Some(buy))
            }
        };

        if let Some(input_mint_swap) = input_mint_swap {
            wallet_stats
                .token_swaps
                .entry(swap.input_token_mint)
                .and_modify(|swaps| swaps.push(input_mint_swap))
                .or_insert(vec![]);
        }

        if let Some(output_mint_swap) = output_mint_swap {
            wallet_stats
                .token_swaps
                .entry(swap.output_token_mint)
                .and_modify(|swaps| swaps.push(output_mint_swap))
                .or_insert(vec![]);
        }

        Ok(())
    }

    pub fn print_state(
        &self,
        out_path: Option<&impl AsRef<std::path::Path>>,
        token_info: Option<HashMap<Pubkey, MintInfo>>,
        token_prices: Option<HashMap<Pubkey, f64>>,
        n_swaps_display: Option<usize>,
    ) -> anyhow::Result<()> {
        let token_info = token_info.unwrap_or_default();
        let token_prices = token_prices.unwrap_or_default();
        let mut final_stats = HashMap::new();
        for (wallet, stats) in &self.wallet_stats {
            log::info!("* {}", wallet);
            let mut token_stats_map = HashMap::<Pubkey, TokenExt<TokenStats>>::new();
            for (token, token_stats) in &stats.token_stats {
                let mint_info = token_info.get(&token);
                let name = mint_info.as_ref().and_then(|info| info.name.clone());
                let symbol = mint_info.as_ref().and_then(|info| info.symbol.clone());
                let value_held_token = token_stats
                    .buy_volume_token
                    .saturating_sub(token_stats.sell_volume_token);
                let price = match token_prices.get(&token) {
                    Some(price) => *price,
                    None => {
                        log::error!(
                            "Failed to get price for token '{}'",
                            name.as_ref().unwrap_or(&token.to_string())
                        );
                        0.0
                    }
                };
                let unrealized_usd = (value_held_token as f64
                    / 10_u32.pow(token_stats.decimals as u32) as f64)
                    * price;
                let pnl_usd =
                    (unrealized_usd + token_stats.sell_volume_usd) - token_stats.buy_volume_usd;
                token_stats_map.insert(
                    *token,
                    TokenExt {
                        address: *token,
                        name,
                        symbol,
                        pnl_usd,
                        unrealized_usd,
                        value_held_token,
                        value: token_stats.clone(),
                    },
                );
            }

            let mut token_swaps_vec = vec![];
            for (i, (token, token_swaps)) in stats.token_swaps.iter().enumerate() {
                let token_stats = token_stats_map
                    .get(token)
                    .context(format!("should find stats for token {}", token))?;
                token_swaps_vec.push(TokenExt {
                    address: *token,
                    name: token_stats.name.clone(),
                    symbol: token_stats.symbol.clone(),
                    pnl_usd: token_stats.pnl_usd,
                    unrealized_usd: token_stats.unrealized_usd,
                    value_held_token: token_stats.value_held_token,
                    value: TokenSwaps {
                        swaps: token_swaps.clone(),
                    },
                })
            }

            let mut token_stats_vec = token_stats_map
                .into_iter()
                .map(|(_, v)| v)
                .collect::<Vec<_>>();
            token_stats_vec.par_sort_by(|a, b| {
                b.pnl_usd
                    .partial_cmp(&a.pnl_usd)
                    .unwrap_or(Ordering::Greater)
            });
            token_swaps_vec.par_sort_by(|a, b| {
                b.pnl_usd
                    .partial_cmp(&a.pnl_usd)
                    .unwrap_or(Ordering::Greater)
            });

            final_stats.insert(
                *wallet,
                WalletStatsJson {
                    address: *wallet,
                    earliest_txn: stats.earliest_txn,
                    latest_txn: stats.latest_txn,
                    num_swaps: stats.num_swaps,
                    total_buys: stats.total_buys,
                    total_sells: stats.total_sells,
                    total_buy_volume_usd: stats.total_buy_volume_usd,
                    total_sell_volume_usd: stats.total_sell_volume_usd,
                    token_stats: token_stats_vec,
                    token_swaps: token_swaps_vec,
                    failed_signatures: stats
                        .failed_signatures
                        .clone()
                        .into_iter()
                        .map(|sig| sig.to_string())
                        .collect(),
                },
            );
        }

        log::info!(
            "******* Printing stats for {} wallets. *******",
            self.wallet_stats.len()
        );

        if !final_stats.is_empty() {
            log::info!(
                "*****************************************************************************"
            );
        }

        let n_swaps_display = n_swaps_display.unwrap_or(30);
        for (wallet, stats) in &final_stats {
            let earliest_txn = stats
                .earliest_txn
                .map(|x| x.to_string())
                .unwrap_or("null".to_string());
            let latest_txn = stats
                .latest_txn
                .map(|x| x.to_string())
                .unwrap_or("null".to_string());
            let total_pnl_usd = stats.token_stats.iter().fold(0.0, |acc, x| acc + x.pnl_usd);
            let total_unrealized = stats
                .token_stats
                .iter()
                .fold(0.0, |acc, x| acc + x.unrealized_usd);

            log::info!("* {}", wallet);
            log::info!("- [earliest-txn]: {}", earliest_txn);
            log::info!("- [latest-txn]: {}", latest_txn);
            log::info!("- [num-swaps]: {}", stats.num_swaps);
            log::info!("- [total-buys]: {}", stats.total_buys);
            log::info!("- [total-sells]: {}", stats.total_sells);
            log::info!(
                "- [total-buy-volume-usd]: {:.0} USD",
                stats.total_buy_volume_usd
            );
            log::info!(
                "- [total-sell-volume-usd]: {:.0} USD",
                stats.total_sell_volume_usd
            );
            log::info!("- [total-pnl-usd]: {:.0} USD", total_pnl_usd);
            log::info!("- [total-unrealized]: {:.0} USD", total_unrealized);

            if !stats.token_stats.is_empty() {
                log::info!("[x]................WALLET TOKENS................[x]");
            }

            for stats in stats.token_stats.iter().take(n_swaps_display) {
                let name = stats
                    .name
                    .clone()
                    .unwrap_or(format!("Unknown({})", stats.address.to_string()));
                log::info!(
                    "- token='{}',pnl={:.0} USD,unrealized={:.0} USD,buys={},sells={}",
                    name,
                    stats.pnl_usd,
                    stats.unrealized_usd,
                    stats.value.total_buys,
                    stats.value.total_sells
                );
            }

            log::info!(
                "******************************************************************************"
            );
        }

        if let Some(out) = out_path {
            std::fs::write(
                out,
                serde_json::to_string_pretty(&AnalyzeOutput {
                    wallets: final_stats.keys().map(|key| key.to_string()).collect(),
                    results: final_stats
                        .into_iter()
                        .map(|(k, v)| (k.to_string(), v))
                        .collect(),
                })?,
            )?;
            log::info!("Output wallet results to {}", out.as_ref().display());
        }

        Ok(())
    }

    pub fn get_token_addresses(&self) -> HashSet<Pubkey> {
        let mut tokens = HashSet::new();
        for wallet_stats in self.wallet_stats.values() {
            for token in wallet_stats.token_stats.keys() {
                tokens.insert(*token);
            }
        }
        tokens
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AnalyzeOutput {
    pub wallets: Vec<String>,
    pub results: HashMap<String, WalletStatsJson>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TokenSwaps {
    swaps: Vec<Swap>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct WalletStatsJson {
    #[serde(with = "field_as_string")]
    pub address: Pubkey,
    pub earliest_txn: Option<DateTime<Utc>>,
    pub latest_txn: Option<DateTime<Utc>>,
    pub num_swaps: usize,
    pub total_buys: usize,
    pub total_sells: usize,
    pub total_buy_volume_usd: f64,
    pub total_sell_volume_usd: f64,
    pub token_stats: Vec<TokenExt<TokenStats>>,
    pub token_swaps: Vec<TokenExt<TokenSwaps>>,
    pub failed_signatures: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TokenExt<T> {
    #[serde(with = "field_as_string")]
    address: Pubkey,
    name: Option<String>,
    symbol: Option<String>,
    #[serde(flatten)]
    value: T,
    pnl_usd: f64,
    unrealized_usd: f64,
    value_held_token: u64,
}

#[derive(Debug, Default)]
struct WalletStats {
    pub address: Pubkey,
    pub earliest_txn: Option<DateTime<Utc>>,
    pub latest_txn: Option<DateTime<Utc>>,
    pub num_swaps: usize,
    pub total_buys: usize,
    pub total_sells: usize,
    pub total_buy_volume_usd: f64,
    pub total_sell_volume_usd: f64,
    pub token_stats: HashMap<Pubkey, TokenStats>,
    pub token_swaps: HashMap<Pubkey, Vec<Swap>>,
    pub failed_signatures: Vec<Signature>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TokenStats {
    pub decimals: u8,
    pub total_buys: usize,
    pub total_sells: usize,
    pub buy_volume_token: u64,
    pub sell_volume_token: u64,
    pub buy_volume_usd: f64,
    pub sell_volume_usd: f64,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct Swap {
    #[serde(with = "field_as_string")]
    pub signature: Signature,
    pub block_time: i64,
    #[serde(with = "field_as_string")]
    pub token: Pubkey,
    #[serde(with = "field_as_string")]
    pub other_token: Pubkey,
    pub amount: u64,
    pub other_amount: u64,
    pub volume_token_usd: f64,
    pub volume_other_token_usd: f64,
    pub direction: Direction,
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    Buy,
    Sell,
}
