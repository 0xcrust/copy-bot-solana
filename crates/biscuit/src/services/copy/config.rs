use crate::utils::serde_helpers::{field_as_string, option_field_as_string};
use crate::wire::helius_rpc::PriorityLevel;
use anyhow::anyhow;
use envconfig::Envconfig;
use serde::{Deserialize, Serialize};
use solana_sdk::commitment_config::CommitmentLevel;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;

const DEFAULT_MOONBAG_BPS: u16 = 1000; // 10%
const DEFAULT_SWAP_EXEC_RETRIES: u8 = 3;
const DEFAULT_SEND_TXN_RETRIES: u8 = 3;
const DEFAULT_CONFIRMED_ERR_RETRIES: u8 = 3;
const DEFAULT_PRICE_REFRESH_MS: u64 = 1000;
const DEFAULT_LOG_INTERVAL_SECONDS: u64 = 60;
const DEFAULT_MAX_PENDING_QUEUE_LEN: usize = 500;
const DEFAULT_MAX_EXECUTION_QUEUE_LEN: usize = 200;

// Trade token is always SOL
#[derive(Clone, Debug, Envconfig, Serialize, Deserialize)]
pub struct CopyConfig {
    #[envconfig(from = "CB_MODE", default = "direct")]
    pub copy_mode_default: CopyMode,

    /// The default minimum amount spent on each trade
    #[envconfig(from = "CB_MIN_INPUT_AMOUNT", default = "100000000")]
    pub min_input_amount_default: u64,

    /// The default maximum amount spent on each trade
    #[envconfig(from = "CB_MAX_INPUT_AMOUNT", default = "500000000")]
    pub max_input_amount_default: u64,

    #[envconfig(from = "CB_PING_CONFIG_PATH")]
    pub ping_config_path: Option<String>,

    /// Tolerable slippage basis points
    #[envconfig(from = "CB_SLIPPAGE_BPS", default = "1000")]
    pub slippage_bps: u16,

    #[envconfig(from = "CB_REFRESH_BALANCE_SECS", default = "30")]
    pub refresh_balance_interval_secs: u64,

    #[envconfig(from = "CB_DISABLE_BUY_LAMPORTS", default = "500000000")]
    // disable buys when our balance is less than 0.5 SOL
    pub disable_buy_threshold_lamports: u64,

    /// Percentage of buy amount always left as dust in wallet
    #[envconfig(from = "CB_MOONBAG_BPS", default = "1000")]
    pub moonbag_bps: u16,

    /// How many times to retry getting a swap quote + transaction
    #[envconfig(from = "CB_EXECUTION_RETRIES", default = "3")]
    pub execution_retries: u8,

    /// How many times to retry sending an unconfirmed transaction
    #[envconfig(from = "CB_SEND_TXN_RETRIES", default = "3")]
    pub send_txn_retries: u8,

    /// How many times to retry sending an error transaction
    #[envconfig(from = "CB_CONFIRMED_ERROR_RETRIES", default = "3")]
    pub confirmed_error_retries: u8,

    /// Override the Jupiter quote api url
    #[envconfig(from = "QUOTE_API_URL")]
    pub quote_api_url: Option<String>,

    /// Override the Jupiter price api url
    #[envconfig(from = "PRICE_API_URL")]
    pub price_feed_url: Option<String>,

    /// How often to refresh stale prices in the price feed
    #[envconfig(from = "CB_PRICE_FEED_REFRESH_FREQUENCY_MS", default = "15000")]
    pub price_feed_refresh_frequency_ms: u64, //15s. The price of SOL, USDC, USDT is hardly fluctuating

    /// How often to log transaction stats
    #[envconfig(from = "CB_LOG_INTERVAL_SECONDS", default = "60")]
    pub log_interval_seconds: u64,

    /// Size bound on the pending-transactions
    #[envconfig(from = "CB_PENDING_QUEUE_LENGTH", default = "500")]
    pub max_pending_queue_length: usize,

    /// Size bound on the execution-queue channel
    #[envconfig(from = "CB_EXECUTION_QUEUE_LENGTH", default = "200")]
    pub max_execution_queue_length: usize,

    /// Whether the default behaviour for txns is to send to jito
    #[envconfig(from = "CB_SEND_JITO_DEFAULT", default = "true")]
    pub send_jito_default: bool,

    /// The minimum tip to pay when sending a jito txn
    #[envconfig(from = "CB_MIN_FEE_LAMPORTS", default = "150000")]
    pub min_fee_lamports: u64,

    /// The maximum tip to pay when sending a jito txn
    #[envconfig(from = "CB_MAX_FEE_LAMPORTS", default = "500000")]
    pub max_fee_lamports: u64,

    #[envconfig(from = "CB_FEE_INCREASE_MULTIPLIER", default = "50000")]
    pub fee_increase_multiplier: u64,

    /// Default priority level for txns
    #[envconfig(from = "CB_PRIORITY_LEVEL_DEFAULT", default = "high")]
    pub priority_level_default: PriorityLevel,

    #[envconfig(from = "CB_DISABLE_BUY_LAMPORTS", default = "100000000")]
    pub disable_buy_lamports: u64,

    #[envconfig(from = "CB_DUST_WALLET")]
    pub dust_wallet: Option<Pubkey>,

    #[envconfig(from = "CB_DUST_CLEANUP_SECS", default = "900")] // default is 15 minutes
    pub dust_cleanup_secs: u64,
}

impl Default for CopyConfig {
    fn default() -> Self {
        CopyConfig {
            slippage_bps: 1000,
            copy_mode_default: CopyMode::Direct,
            min_input_amount_default: 100_000_000,
            max_input_amount_default: 500_000_000,
            ping_config_path: None,
            refresh_balance_interval_secs: 30,
            disable_buy_threshold_lamports: 100_000,
            moonbag_bps: DEFAULT_MOONBAG_BPS,
            execution_retries: DEFAULT_SWAP_EXEC_RETRIES,
            send_txn_retries: DEFAULT_SEND_TXN_RETRIES,
            confirmed_error_retries: DEFAULT_CONFIRMED_ERR_RETRIES,
            quote_api_url: None,
            price_feed_url: None,
            price_feed_refresh_frequency_ms: DEFAULT_PRICE_REFRESH_MS,
            log_interval_seconds: DEFAULT_LOG_INTERVAL_SECONDS,
            max_pending_queue_length: DEFAULT_MAX_PENDING_QUEUE_LEN,
            max_execution_queue_length: DEFAULT_MAX_EXECUTION_QUEUE_LEN,
            send_jito_default: false,
            min_fee_lamports: 50_000,
            max_fee_lamports: 200_000, // 0.0002 SOL. 1000 txns = 0.2 SOL
            fee_increase_multiplier: 15000,
            priority_level_default: PriorityLevel::High,
            disable_buy_lamports: 100_000_000,
            dust_wallet: None,
            dust_cleanup_secs: 15 * 60,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyWallet {
    #[serde(with = "field_as_string")]
    /// The wallet we're tracking
    pub wallet: Pubkey,
    /// The display name of the wallet(for better interpretation of events)
    pub display_name: Option<String>,
    /// Override the default minimum input amount for this wallet
    pub min_input_amount: Option<u64>,
    /// Override the default maximum input amount for this wallet
    pub max_input_amount: Option<u64>,
    /// Whether or not buys are disabled for this wallet
    pub buy_disabled: Option<bool>,
    /// 'direct' or 'ping'
    pub mode: Option<CopyMode>,
    /// Commitment level to poll wallet's transactions with
    pub commitment_level: Option<CommitmentLevel>,
    /// How frequently to poll this wallet's transactions
    pub poll_transactions_frequency_seconds: Option<u64>,
    /// Whether to ignore error transactions when polling this wallet
    pub ignore_error_transactions: Option<bool>,
    /// Signature to start looking forward from
    #[serde(with = "option_field_as_string", default)]
    pub recent_signature: Option<Signature>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CopyMode {
    Direct,
    Ping,
}

impl std::str::FromStr for CopyMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "direct" => Ok(CopyMode::Direct),
            "ping" => Ok(CopyMode::Ping),
            _ => Err(anyhow!(
                "Invalid copy-mode string. expected (direct | ping)"
            )),
        }
    }
}

// Pumpfun config. Input amount is in SOL.
//
// This is a separate config because it deals with only SOL, but our trade-token
// for swaps might be different(USDC, USDT)
// pub struct PFConfig {
//     pub input_amount: u64,
//     pub slippage_bps: u16,
//     pub moonbag_bps: u16
// }
// impl Default for PFConfig {
//     fn default() -> Self {
//         PFConfig {
//             input_amount: 50_000_000,
//             slippage_bps: 1000,
//             moonbag_bps: 0
//         }
//     }
// }
