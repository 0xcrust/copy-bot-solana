use crate::constants::mints::{SOL, USDC, USDT};
use crate::swap::jupiter::price::{make_chunked_price_request_usd, PriceFeed as JupiterPriceFeed};
use crate::swap::jupiter::token_list_old::TOKEN_LIST;
use crate::utils::serde_helpers::field_as_string;
use std::sync::Arc;

use anyhow::anyhow;
use dashmap::mapref::one::RefMut;
use dashmap::{mapref::one::Ref, DashMap};
use log::{debug, error};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;

#[derive(Copy, Clone, Deserialize, Serialize)]
pub enum TradeDirection {
    Buy,
    Sell,
}

#[derive(Debug, Default, Copy, Clone, Deserialize, Serialize)]
pub struct TradeHistory {
    pub buy_volume_token: u64,
    pub sell_volume_token: u64,
    pub buy_volume_usd: f64,
    pub sell_volume_usd: f64,
    pub buy_volume_sol: f64,
    pub sell_volume_sol: f64,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct CopyHistory {
    pub origin: TradeHistory,
    pub exec: TradeHistory,
    pub token: Pubkey, // todo: Guarantee that the tokens for origin and exec are the same
}

#[derive(Clone, Debug, Default)]
pub struct HistoricalTrades(Arc<DashMap<(Pubkey, Pubkey), CopyHistory>>);

#[derive(Deserialize, Serialize)]
pub struct TradeVolume {
    pub exec_volume_token: u64,
    pub exec_volume_usd: f64,
    pub exec_volume_sol: f64,
    pub origin_volume_token: u64,
    #[serde(deserialize_with = "deserialize_f64_null_as_nan")]
    pub origin_volume_usd: f64,
    #[serde(deserialize_with = "deserialize_f64_null_as_nan")]
    pub origin_volume_sol: f64,
    pub dust_token_amount: u64,
}

/// A helper to deserialize `f64`, treating JSON null as f64::NAN.
/// See https://github.com/serde-rs/json/issues/202
fn deserialize_f64_null_as_nan<'de, D: serde::de::Deserializer<'de>>(
    des: D,
) -> Result<f64, D::Error> {
    let optional = Option::<f64>::deserialize(des)?;
    Ok(optional.unwrap_or(f64::NAN))
}

#[derive(Deserialize, Serialize)]
pub struct CopyTrade {
    pub volume: TradeVolume,
    #[serde(with = "field_as_string")]
    pub token: Pubkey,
    #[serde(with = "field_as_string")]
    pub origin_signature: Signature,
    #[serde(with = "field_as_string")]
    pub exec_signature: Signature,
    #[serde(with = "field_as_string")]
    pub origin_wallet: Pubkey,
    #[serde(with = "field_as_string")]
    pub exec_wallet: Pubkey,
    pub direction: TradeDirection,
}

impl HistoricalTrades {
    pub fn get_history_for_wallet_and_token(
        &self,
        wallet: Pubkey,
        token: Pubkey,
    ) -> Option<Ref<(Pubkey, Pubkey), CopyHistory>> {
        self.0.get(&(wallet, token))
    }

    pub fn get_history_for_wallet_and_token_mut(
        &self,
        wallet: Pubkey,
        token: Pubkey,
    ) -> Option<RefMut<(Pubkey, Pubkey), CopyHistory>> {
        self.0.get_mut(&(wallet, token))
    }

    #[must_use]
    pub fn insert_trade(
        &self,
        origin_wallet: &Pubkey,
        origin_wallet_display: &str,
        token: &Pubkey,
        token_decimals: u8,
        exec_volume_token: u64,
        origin_volume_token: u64,
        exec_price_usd: f64,
        exec_price_sol: f64,
        origin_price_usd: f64,
        origin_price_sol: f64,
        direction: &TradeDirection,
        moonbag_bps: Option<u16>,
        signature: &Signature,
    ) -> anyhow::Result<TradeVolume> {
        if *token == SOL || *token == USDC || *token == USDT {
            return Err(anyhow!("Can't track trades for SOL | USDC | USDT"));
        }

        let dust_volume = match direction {
            TradeDirection::Buy => {
                // Dust only applies to buys
                let dust_bps = moonbag_bps.unwrap_or_default();
                if dust_bps > 10_000 {
                    return Err(anyhow!("Dust bps is greater than 100%"));
                }
                ((dust_bps as f64 / 10_000.0) * exec_volume_token as f64).trunc() as u64
            }
            TradeDirection::Sell => 0,
        };

        let origin_volume_usd = (origin_volume_token as f64 * origin_price_usd)
            / 10_u32.pow(token_decimals as u32) as f64;
        let origin_volume_sol = (origin_volume_token as f64 * origin_price_sol)
            / 10_u32.pow(token_decimals as u32) as f64;

        let exec_volume_usd =
            (exec_volume_token) as f64 * exec_price_usd / 10_u32.pow(token_decimals as u32) as f64;
        let exec_volume_sol =
            (exec_volume_token) as f64 * exec_price_sol / 10_u32.pow(token_decimals as u32) as f64;

        let token_display = if cfg!(debug_assertions) {
            TOKEN_LIST
                .read()
                .unwrap()
                .get(&token)
                .map(|t| t.name.clone())
                .unwrap_or(token.to_string())
        } else {
            token.to_string()
        };

        match direction {
            TradeDirection::Buy => {
                log::info!(
                    "Copied {}: Swapped {:.4} of SOL({:.4} USD) for {} of {} in tx {}",
                    origin_wallet_display,
                    exec_volume_sol,
                    exec_volume_usd,
                    exec_volume_token,
                    token_display,
                    signature
                );
            }
            TradeDirection::Sell => {
                log::info!(
                    "Copied {}: Swapped {} of {} for {:.4} SOL({:.4} USD in tx {})",
                    origin_wallet_display,
                    exec_volume_token,
                    token_display,
                    exec_volume_sol,
                    exec_volume_usd,
                    signature
                );
            }
        }

        self.insert_trade_inner(
            origin_wallet,
            token,
            exec_volume_token, // the amount after subtracting the dust-amount
            origin_volume_token,
            exec_volume_usd,
            origin_volume_usd,
            exec_volume_sol,
            origin_volume_sol,
            dust_volume,
            direction,
        )?;

        let trade = TradeVolume {
            exec_volume_sol,
            exec_volume_token,
            exec_volume_usd,
            origin_volume_sol,
            origin_volume_token,
            origin_volume_usd,
            dust_token_amount: match direction {
                TradeDirection::Buy => {
                    // Dust only applies to bought
                    let dust_bps = moonbag_bps.unwrap_or_default();
                    if dust_bps > 10_000 {
                        return Err(anyhow!("Dust bps is greater than 100%"));
                    }
                    ((dust_bps as f64 / 10_000.0) * exec_volume_token as f64).trunc() as u64
                }
                TradeDirection::Sell => 0,
            },
        };

        Ok(trade)
    }

    pub fn insert_trade_inner(
        &self,
        origin_wallet: &Pubkey,
        token: &Pubkey,
        exec_volume_token: u64,
        origin_volume_token: u64,
        exec_volume_usd: f64,
        origin_volume_usd: f64,
        exec_volume_sol: f64,
        origin_volume_sol: f64,
        dust_volume_token: u64,
        direction: &TradeDirection,
    ) -> anyhow::Result<()> {
        if *token == SOL || *token == USDC || *token == USDT {
            return Err(anyhow!("Can't track trades for SOL | USDC | USDT"));
        }

        let mut history = self
            .0
            .get(&(*origin_wallet, *token))
            .map(|kv| *kv.value())
            .unwrap_or_default();

        // This is guaranteed to be zero for sell trades(as it should always be)
        // We don't keep track of a percentage of our buys marked as dust.
        let exec_volume_token = exec_volume_token - dust_volume_token;

        // note: Not setting this is a bug!
        history.token = *token;

        match direction {
            TradeDirection::Buy => {
                history.origin.buy_volume_token += origin_volume_token;
                history.origin.buy_volume_usd += origin_volume_usd;
                history.origin.buy_volume_sol += origin_volume_sol;
                history.exec.buy_volume_token += exec_volume_token;
                history.exec.buy_volume_usd += exec_volume_usd;
                history.exec.buy_volume_sol += exec_volume_sol;
            }
            TradeDirection::Sell => {
                history.origin.sell_volume_token += origin_volume_token;
                history.origin.sell_volume_usd += origin_volume_usd;
                history.origin.sell_volume_sol += origin_volume_sol;
                history.exec.sell_volume_token += exec_volume_token;
                history.exec.sell_volume_usd += exec_volume_usd;
                history.exec.sell_volume_sol += exec_volume_sol;
            }
        }

        debug!("Updated history: {:#?}", history);
        _ = self.0.insert((*origin_wallet, *token), history);
        Ok(())
    }

    pub fn remove_history(
        &self,
        origin_wallet: &Pubkey,
        token: &Pubkey,
    ) -> Option<((Pubkey, Pubkey), CopyHistory)> {
        self.0.remove(&(*origin_wallet, *token))
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct Price {
    pub input_price_usd: f64,
    pub output_price_usd: f64,
    pub input_price_sol: f64,
    pub output_price_sol: f64,
    pub sol_price_usd: f64,
}

pub async fn get_token_prices(
    input_mint: &Pubkey,
    output_mint: &Pubkey,
    input_amount: u64,
    output_amount: u64,
    input_token_decimals: u8,
    output_token_decimals: u8,
    sol_price_usd: Option<f64>,
    price_feed: &JupiterPriceFeed,
) -> Option<Price> {
    let sol_price_usd = sol_price_usd.or(price_feed.get_price_usd(&SOL).and_then(|f| f.price()));
    if sol_price_usd.is_none() {
        error!("Failed to get SOL price");
        return None;
    }
    let sol_price_usd = sol_price_usd.unwrap();

    let mut input_token_price_usd = None;
    let mut output_token_price_usd = None;

    // Step1: Try to get price if it is SOL or a stablecoin
    match *input_mint {
        USDC | USDT => input_token_price_usd = Some(1.0),
        SOL => input_token_price_usd = Some(sol_price_usd),
        _ => {}
    }
    match *output_mint {
        USDC | USDT => output_token_price_usd = Some(1.0),
        SOL => output_token_price_usd = Some(sol_price_usd),
        _ => {}
    }

    // Step2: If both are `None`, send a price-request
    if input_token_price_usd.is_none() && output_token_price_usd.is_none() {
        let price_map = make_chunked_price_request_usd(
            None,
            &vec![input_mint.to_string(), output_mint.to_string()],
        )
        .await;
        if input_token_price_usd.is_none() {
            input_token_price_usd = price_map
                .get(&input_mint.to_string())
                .and_then(|p| p.as_ref().map(|price| price.price));
        }
        if output_token_price_usd.is_none() {
            output_token_price_usd = price_map
                .get(&output_mint.to_string())
                .and_then(|p| p.as_ref().map(|price| price.price))
        }
    }

    let input_factor = 10u32.pow(input_token_decimals as u32);
    let output_factor = 10u32.pow(output_token_decimals as u32);
    // a-amount * a-price = b_amount * b-price
    // Final resort. If we know the usd price of at least one of the tokens, attempt to get the value of the other using the amounts
    match (input_token_price_usd, output_token_price_usd) {
        (Some(_), Some(_)) => {}
        (Some(input_price), None) => {
            let output_price = ((input_amount as f64 * input_price) / output_amount as f64)
                * (output_factor as f64 / input_factor as f64);
            output_token_price_usd = Some(output_price);
        }
        (None, Some(output_price)) => {
            let input_price = (output_amount as f64 * output_price) / input_amount as f64
                * (input_factor as f64 / output_factor as f64);
            input_token_price_usd = Some(input_price);
        }
        (None, None) => return None, // Atp there is nothing else we can do
    }

    let input_price_usd = input_token_price_usd.unwrap();
    let output_price_usd = output_token_price_usd.unwrap();

    Some(Price {
        input_price_usd,
        input_price_sol: input_price_usd / sol_price_usd,
        output_price_usd,
        output_price_sol: output_price_usd / sol_price_usd,
        sol_price_usd,
    })
}

pub fn calculate_token_prices(
    input_mint: &Pubkey,
    output_mint: &Pubkey,
    input_amount: u64,
    output_amount: u64,
    input_token_decimals: u8,
    output_token_decimals: u8,
    sol_price_usd: f64,
) -> anyhow::Result<Price> {
    let mut input_token_price_usd = None;
    let mut output_token_price_usd = None;

    // Step1: Try to get price if it is SOL or a stablecoin
    match *input_mint {
        USDC | USDT => input_token_price_usd = Some(1.0),
        SOL => input_token_price_usd = Some(sol_price_usd),
        _ => {}
    }
    match *output_mint {
        USDC | USDT => output_token_price_usd = Some(1.0),
        SOL => output_token_price_usd = Some(sol_price_usd),
        _ => {}
    }

    let input_factor = 10u32.pow(input_token_decimals as u32);
    let output_factor = 10u32.pow(output_token_decimals as u32);
    // a-amount * a-price = b_amount * b-price
    // Final resort. If we know the usd price of at least one of the tokens, attempt to get the value of the other using the amounts
    match (input_token_price_usd, output_token_price_usd) {
        (Some(_), Some(_)) => {}
        (Some(input_price), None) => {
            let output_price = ((input_amount as f64 * input_price) / output_amount as f64)
                * (output_factor as f64 / input_factor as f64);
            output_token_price_usd = Some(output_price);
        }
        (None, Some(output_price)) => {
            let input_price = (output_amount as f64 * output_price) / input_amount as f64
                * (input_factor as f64 / output_factor as f64);
            input_token_price_usd = Some(input_price);
        }
        (None, None) => {
            return Err(anyhow!(
                "Cannot calculate token prices. Neither input nor output mint price is known"
            ))
        }
    }

    let input_price_usd = input_token_price_usd.unwrap();
    let output_price_usd = output_token_price_usd.unwrap();

    Ok(Price {
        input_price_usd,
        input_price_sol: input_price_usd / sol_price_usd,
        output_price_usd,
        output_price_sol: output_price_usd / sol_price_usd,
        sol_price_usd,
    })
}
