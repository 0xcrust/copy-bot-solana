use crate::{
    constants::mints::{SOL, USDC, USDT},
    parser::swap::ResolvedSwap,
    services::copy::history::{TradeDirection, TradeHistory},
    swap::jupiter::price::PriceFeed,
};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;

use anyhow::anyhow;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use solana_sdk::message::AccountKeys;
use solana_sdk::native_token::LAMPORTS_PER_SOL;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_transaction_status::{UiTransactionStatusMeta, UiTransactionTokenBalance};

#[derive(Clone)]
pub struct PingTracker {
    pub config: Arc<PingConfig>,
    pub pings: Arc<DashMap<Pubkey, Ping>>,
    pub wallet_history: Arc<DashMap<(Pubkey, Pubkey), WalletHistory>>,
    pub execution: Arc<DashMap<Pubkey, TradeHistory>>,
    pub tiers: Arc<DashMap<usize, u64>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PingSnapshot {
    #[serde(with = "map_serde")]
    pub pings: HashMap<String, Ping>,
    #[serde(with = "map_serde")]
    pub wallet_history: HashMap<(String, String), WalletHistory>,
    #[serde(with = "map_serde")]
    pub execution: HashMap<String, TradeHistory>,
}
impl PingSnapshot {
    pub fn write_to_file(&self, out: &str) -> anyhow::Result<()> {
        std::fs::write(out, serde_json::to_string_pretty(&self)?)?;
        Ok(())
    }
}
impl From<&PingTracker> for PingSnapshot {
    fn from(value: &PingTracker) -> Self {
        PingSnapshot {
            pings: HashMap::from_iter(
                value
                    .pings
                    .iter()
                    .map(|x| (x.key().to_string(), x.value().clone())),
            ),
            wallet_history: HashMap::from_iter(value.wallet_history.iter().map(|x| {
                (
                    (x.key().0.to_string(), x.key().1.to_string()),
                    x.value().clone(),
                )
            })),
            execution: HashMap::from_iter(
                value
                    .execution
                    .iter()
                    .map(|x| (x.key().to_string(), x.value().clone())),
            ),
        }
    }
}

#[derive(Clone, Default)]
pub struct PingTiers {
    pings: u8,
    lamports: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PingConfig {
    pub snapshot_path: String,
    pub snapshot_frequency: u64,
    pub tiers: Vec<TiersRaw>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TiersRaw {
    pings: u8,
    amount: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Ping {
    #[serde(with = "crate::utils::serde_helpers::field_as_string_collection")]
    pub wallets: HashSet<Pubkey>,
    pub execution_count: u8,
    pub total_lamports_cost: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WalletHistory {
    #[serde(with = "crate::utils::serde_helpers::field_as_string")]
    pub wallet: Pubkey,
    #[serde(with = "crate::utils::serde_helpers::field_as_string")]
    pub token: Pubkey,
    #[serde(with = "crate::utils::serde_helpers::field_as_string_collection")]
    pub signatures: HashSet<Signature>,
}

impl PingTracker {
    pub fn start(config_path: &str, load_snapshot: bool) -> anyhow::Result<PingTracker> {
        let config = serde_json::from_str::<PingConfig>(&std::fs::read_to_string(config_path)?)?;
        if config.tiers.is_empty() {
            return Err(anyhow!("ping tiers config is empty"));
        }
        let (pings, wallet_history, execution) =
            if load_snapshot && std::fs::metadata(&config.snapshot_path).is_ok() {
                let start = std::time::Instant::now();
                log::info!(
                    "Loading ping-history snapshot from {}",
                    config.snapshot_path
                );
                let snapshot = serde_json::from_str::<PingSnapshot>(&std::fs::read_to_string(
                    &config.snapshot_path,
                )?)?;
                let res = (
                    Arc::new(DashMap::from_iter(
                        snapshot
                            .pings
                            .into_iter()
                            .map(|(k, v)| (Pubkey::from_str(&k).unwrap(), v)),
                    )),
                    Arc::new(DashMap::from_iter(snapshot.wallet_history.into_iter().map(
                        |(k, v)| {
                            (
                                (
                                    Pubkey::from_str(&k.0).unwrap(),
                                    Pubkey::from_str(&k.1).unwrap(),
                                ),
                                v,
                            )
                        },
                    ))),
                    Arc::new(DashMap::from_iter(
                        snapshot
                            .execution
                            .into_iter()
                            .map(|(k, v)| (Pubkey::from_str(&k).unwrap(), v)),
                    )),
                );
                log::info!("Loading snapshot took {} ms", start.elapsed().as_millis());
                res
            } else {
                if load_snapshot {
                    log::info!(
                        "Specified snapshot file does not exist. Proceeding with no loaded history"
                    );
                }
                (Default::default(), Default::default(), Default::default())
            };

        let ping_tracker = PingTracker {
            config: Arc::new(config.clone()),
            pings,
            wallet_history,
            execution,
            tiers: Arc::new(DashMap::from_iter(config.tiers.into_iter().map(|t| {
                (
                    t.pings as usize,
                    ((t.amount * LAMPORTS_PER_SOL as f64).trunc() as u64),
                )
            }))),
        };

        let _jh = {
            let tracker = ping_tracker.clone();
            let frequency = std::time::Duration::from_secs(config.snapshot_frequency);
            let path = config.snapshot_path.clone();

            std::thread::spawn(move || {
                loop {
                    let start = std::time::Instant::now();
                    if let Err(e) = tracker.write_snapshot(&path) {
                        log::error!("Error writing ping snapshot: {}", e);
                    };
                    log::info!("Writing snapshot took {} ms", start.elapsed().as_millis());
                    std::thread::sleep(frequency);
                }
                #[allow(unreachable_code)]
                Ok::<_, anyhow::Error>(())
            })
        };

        Ok(ping_tracker)
    }

    pub fn get_token_input(
        &self,
        input_token: &Pubkey,
        origin: &Pubkey,
        token_amount: u64,
        origin_balance_after: u64,
    ) -> Option<u64> {
        let ping = self.pings.get(&input_token)?;

        if !ping.wallets.contains(origin) {
            return None;
        }

        let execution = self.execution.get(&input_token)?;

        // Calculate the sell allocation for this wallet: We give equal consideration to all pings
        let amount_under_consideration = (execution
            .buy_volume_token
            .saturating_sub(execution.sell_volume_token))
            / ping.wallets.len() as u64;
        let amount_to_sell = ((token_amount as f64 / (token_amount + origin_balance_after) as f64)
            * amount_under_consideration as f64)
            .trunc() as u64;

        return Some(amount_to_sell);
    }

    pub fn get_lamports_input(&self, output_token: &Pubkey) -> Option<u64> {
        let Some(ping) = self.pings.get(&output_token) else {
            return None;
        };
        let ping_count = ping.value().wallets.len();
        let execution_count = ping.value().execution_count;

        if (execution_count as usize) >= self.tiers.len() {
            // We must not have more executions than we have tiers
            log::debug!("No ping execution. execution_count must be < number of tiers");
            return None;
        }

        let amount = match self.tiers.get(&ping_count) {
            Some(r) => Some(*r.value()),
            None => {
                let mut pings = self.tiers.iter().map(|x| *x.key()).collect::<Vec<_>>();
                pings.sort_unstable();
                if ping_count > pings[0] {
                    let idx = match pings.binary_search(&ping_count) {
                        Ok(idx) => idx,
                        Err(idx) => std::cmp::min(idx, pings.len() - 1),
                    };
                    self.tiers.get(&pings[idx]).map(|r| *r.value())
                } else {
                    None
                }
            }
        };

        amount.map(|amt| std::cmp::min(amt, ping.value().total_lamports_cost))
    }

    pub async fn insert_swap(
        &self,
        origin: Pubkey,
        signature: Signature,
        swap: &ResolvedSwap,
        price_feed: &PriceFeed,
    ) -> anyhow::Result<bool> {
        let history = self
            .wallet_history
            .get_mut(&(origin, swap.input_token_mint));
        if let Some(mut history) = history {
            // The token being sold is one we're tracking for this wallet(at least one buy). We want to update its history
            history.value_mut().signatures.insert(signature);
        }

        // We only start tracking newly-bought tokens
        let token = swap.output_token_mint;
        if token == SOL || token == USDC || token == USDT {
            log::trace!("{} is not a coin we need to track for ping", token);
            return Ok(false);
        }

        let Some(decimals) = swap.token_decimals else {
            return Err(anyhow!("Token decimals not resolved"));
        };
        let sol_price_usd = match price_feed.get_price_usd(&SOL).and_then(|p| p.price()) {
            Some(price) => price,
            None => {
                return Err(anyhow!("Failed to get SOL price from feed"));
            }
        };
        let input_price_usd = match swap.input_token_mint {
            SOL => sol_price_usd,
            USDC | USDT => 1.0, // approximation tbf
            _ => price_feed
                .get_price_usd(&token)
                .and_then(|p| p.price())
                .unwrap_or_default(),
        };
        let input_price_sol = input_price_usd / sol_price_usd;
        let input_volume_lamports = ((swap.input_amount as f64 * input_price_sol
            / 10_u32.pow(decimals.input as u32) as f64)
            * LAMPORTS_PER_SOL as f64)
            .trunc() as u64;

        let pinged = match self.pings.entry(token) {
            dashmap::mapref::entry::Entry::Occupied(mut entry) => {
                entry.get_mut().total_lamports_cost += input_volume_lamports;
                entry.get_mut().wallets.insert(origin)
            }
            dashmap::mapref::entry::Entry::Vacant(entry) => {
                entry.insert(Ping {
                    wallets: [origin].into(),
                    execution_count: 0,
                    total_lamports_cost: input_volume_lamports,
                });
                true
            }
        };

        self.wallet_history
            .entry((origin, token))
            .and_modify(|v| _ = v.signatures.insert(signature))
            .or_insert(WalletHistory {
                wallet: origin,
                token,
                signatures: [signature].into(),
            });

        Ok(pinged)
    }

    pub fn insert_execution(
        &self,
        token: &Pubkey,
        token_decimals: u8,
        mut volume_token: u64,
        token_price_usd: f64,
        token_price_sol: f64,
        direction: &TradeDirection,
        moonbag_bps: Option<u16>,
    ) -> anyhow::Result<PingExecutionVolume> {
        let volume_usd =
            (volume_token) as f64 * token_price_usd / 10_u32.pow(token_decimals as u32) as f64;
        let volume_sol =
            (volume_token) as f64 * token_price_sol / 10_u32.pow(token_decimals as u32) as f64;

        let dust_amount = match direction {
            TradeDirection::Buy => {
                // Dust only applies to buys
                let dust_bps = moonbag_bps.unwrap_or_default();
                if dust_bps >= 5_000 {
                    return Err(anyhow!("Dust bps is greater than 50%"));
                }
                ((dust_bps as f64 / 10_000.0) * volume_token as f64).trunc() as u64
            }
            TradeDirection::Sell => 0,
        };
        volume_token = volume_token - dust_amount;

        let mut history = self
            .execution
            .get(token)
            .map(|exec| *exec.value())
            .unwrap_or_default();
        match direction {
            TradeDirection::Buy => {
                history.buy_volume_token += volume_token;
                history.buy_volume_sol += volume_sol;
                history.buy_volume_usd += volume_usd;
            }
            TradeDirection::Sell => {
                history.sell_volume_token += volume_token;
                history.sell_volume_sol += volume_sol;
                history.sell_volume_usd += volume_usd;
            }
        }

        _ = self.execution.insert(*token, history);
        self.pings
            .get_mut(token)
            .map(|mut m| m.value_mut().execution_count += 1);

        Ok(PingExecutionVolume {
            volume_token,
            volume_sol,
            volume_usd,
            dust_amount,
            ping: self.pings.get(token).map(|p| p.clone()),
        })
    }

    pub fn to_snapshot(&self) -> PingSnapshot {
        PingSnapshot::from(self)
    }

    pub fn write_snapshot(&self, out: &str) -> anyhow::Result<()> {
        let snapshot = PingSnapshot::from(self);
        snapshot.write_to_file(out)?;

        Ok(())
    }
}

pub struct PingExecutionVolume {
    pub volume_token: u64,
    pub volume_usd: f64,
    pub volume_sol: f64,
    pub dust_amount: u64,
    pub ping: Option<Ping>,
}

fn get_balances_for_owner_and_mint(
    meta: &UiTransactionStatusMeta,
    account_keys: &AccountKeys,
    origin: &Pubkey,
    token: &Pubkey,
) -> (
    Option<UiTransactionTokenBalance>,
    Option<UiTransactionTokenBalance>,
) {
    let pre_token_balances: Vec<UiTransactionTokenBalance> =
        Option::from(meta.pre_token_balances.clone()).unwrap_or_default();
    let post_token_balances: Vec<UiTransactionTokenBalance> =
        Option::from(meta.post_token_balances.clone()).unwrap_or_default();

    let find_balance = |balance: &Vec<UiTransactionTokenBalance>| {
        balance.iter().find_map(|bal| {
            let owner = Option::<String>::from(bal.owner.clone());
            let mint = Option::<String>::from(bal.mint.clone());

            if origin.to_string() == owner? && token.to_string() == mint? {
                Some(bal.clone())
            } else {
                None
            }
        })
    };

    let pre = find_balance(&pre_token_balances);
    let post = find_balance(&post_token_balances);

    (pre, post)
}

pub mod map_serde {
    use serde::{Deserialize, Serialize};
    use std::iter::FromIterator;

    /// Serializes to a `Vec<(K, V)>`.
    ///
    /// Useful for [`std::collections::HashMap`] with a non-string key,
    /// which is unsupported by [`serde_json`].
    pub fn serialize<
        'a,
        S: serde::Serializer,
        T: IntoIterator<Item = (&'a K, &'a V)>,
        K: Serialize + 'a,
        V: Serialize + 'a,
    >(
        target: T,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let container: Vec<_> = target.into_iter().collect();
        serde::Serialize::serialize(&container, s)
    }

    /// Deserializes from a `Vec<(K, V)>`.
    ///
    /// Useful for [`std::collections::HashMap`] with a non-string key,
    /// which is unsupported by [`serde_json`].
    pub fn deserialize<
        'de,
        D: serde::Deserializer<'de>,
        T: FromIterator<(K, V)>,
        K: Deserialize<'de>,
        V: Deserialize<'de>,
    >(
        d: D,
    ) -> Result<T, D::Error> {
        let hashmap_as_vec: Vec<(K, V)> = Deserialize::deserialize(d)?;
        Ok(T::from_iter(hashmap_as_vec))
    }
}
