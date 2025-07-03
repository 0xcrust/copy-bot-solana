use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;

use crate::core::traits::TransactionCache;
use crate::core::types::transaction::ITransaction;
use crate::parser::swap::{ResolvedSwap, TokenDecimals};
use crate::parser::v1::V1Parser;
use crate::services::copy::v2::service::Balance;
use crate::swap::jupiter::token_list_new::JupApiToken;
use crate::utils::serde_helpers::{field_as_string, option_field_as_string};
use crate::utils::token::RawMintInfo;
use crate::wire::rpc::utils::{
    get_multiple_account_data, get_signatures_for_address, get_transactions_for_signatures,
    TerminationReason, TransactionHistoryLimit, TransactionHistoryOrder,
};
use anyhow::{anyhow, Context};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::program_pack::Pack;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use spl_token::state::Mint;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WalletSwaps {
    pub swaps: Vec<Swap>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Swap {
    pub swap: ResolvedSwap,
    #[serde(with = "field_as_string")]
    pub origin_signer: Pubkey,
    #[serde(with = "field_as_string")]
    pub origin_signature: Signature,
    pub block_time: i64,
    pub origin_sol_balance: Option<Balance>,
    pub origin_input_mint_balance: Option<Balance>,
    pub origin_output_mint_balance: Option<Balance>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MintInfo {
    #[serde(with = "field_as_string")]
    pub pubkey: Pubkey,
    pub decimals: u8,
    pub supply: Option<u64>,
    #[serde(with = "option_field_as_string")]
    pub program_id: Option<Pubkey>,
    #[serde(with = "option_field_as_string")]
    pub mint_authority: Option<Pubkey>,
    #[serde(with = "option_field_as_string")]
    pub freeze_authority: Option<Pubkey>,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub uri: Option<String>,
}

impl From<RawMintInfo> for MintInfo {
    fn from(value: RawMintInfo) -> Self {
        MintInfo {
            pubkey: value.pubkey,
            decimals: value.decimals,
            supply: Some(value.supply),
            program_id: Some(value.program_id),
            mint_authority: value.mint_authority,
            freeze_authority: value.mint_authority,
            name: value.name,
            symbol: value.symbol,
            uri: value.uri,
        }
    }
}

impl From<JupApiToken> for MintInfo {
    fn from(value: JupApiToken) -> Self {
        MintInfo {
            pubkey: value.address,
            decimals: value.decimals,
            supply: None,
            program_id: value.program_id,
            mint_authority: value.mint_authority,
            freeze_authority: value.freeze_authority,
            name: Some(value.name),
            symbol: Some(value.symbol),
            uri: value.logo_uri,
        }
    }
}

pub fn wallet_filepath(dir: &std::path::PathBuf, wallet: &Pubkey) -> std::path::PathBuf {
    let mut dir = dir.clone();
    dir.push(format!("{}.json", wallet.to_string()));
    dir
}

pub async fn get_swaps_for_wallet(
    wallet: Pubkey,
    rpc_client: Arc<RpcClient>,
    duration_days: u64,
    until_signature: Option<Signature>,
    transaction_limit: Option<usize>,
    rpc_concurrency: Option<usize>,
    txn_cache: Option<Arc<dyn TransactionCache>>,
) -> anyhow::Result<Vec<Swap>> {
    let current_timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let duration_in_secs = duration_days * 24 * 60 * 60;

    let mut limits = vec![TransactionHistoryLimit::Blocktime(
        current_timestamp
            .checked_sub(duration_in_secs)
            .context("underflow")? as i64,
    )];
    if let Some(limit) = transaction_limit {
        limits.push(TransactionHistoryLimit::N(limit));
    }
    let (mut signatures, termination) = get_signatures_for_address(
        &rpc_client,
        Some(CommitmentConfig::confirmed()),
        wallet,
        None,
        until_signature,
        limits,
        Some(TransactionHistoryOrder::Forward),
    )
    .await?;
    if let Some(limit) = transaction_limit {
        if matches!(termination, TerminationReason::N) {
            return Err(anyhow!("Transactions exceeded limit of {}", limit));
        }
    }
    signatures.retain(|sig| sig.err.is_none());
    if let Some(limit) = transaction_limit {
        if signatures.len() > limit {
            return Err(anyhow!(
                "Signatures are of length {}, exceeding {}",
                signatures.len(),
                limit
            ));
        }
    }

    let start = std::time::Instant::now();
    let signatures_count = signatures.len();
    log::info!(
        "Getting transactions for {} signatures for wallet {}",
        signatures.len(),
        wallet
    );
    let concurrency = rpc_concurrency.unwrap_or(100);
    let transactions = get_transactions_for_signatures(
        Arc::clone(&rpc_client),
        signatures
            .into_iter()
            .map(|sig| Signature::from_str(&sig.signature).unwrap())
            .collect(),
        Some(concurrency),
        Some(|tx: &ITransaction| {
            tx.err.is_none() && matches!(tx.account_keys().get(0).copied(), Some(wallet))
        }),
        txn_cache,
    )
    .await?;
    log::info!(
        "Got {} transactions. filtered={}, time-taken={}ms",
        transactions.len(),
        signatures_count - transactions.len(),
        start.elapsed().as_millis(),
    );

    let transactions_and_swaps = transactions
        .into_iter()
        .filter_map(|txn| {
            let swap = match V1Parser::parse_swap_transaction(&txn) {
                Ok(swap) => swap?,
                Err(e) => {
                    log::error!("Error parsing txn {}: {}", txn.signature, e);
                    return None;
                }
            };
            Some((txn, swap))
        })
        .collect::<Vec<_>>();

    let mut token_mints = HashSet::new();
    for (_, swap) in &transactions_and_swaps {
        token_mints.insert(swap.input_token_mint);
        token_mints.insert(swap.output_token_mint);
    }

    let token_mints = Vec::from_iter(token_mints.into_iter());
    let accounts = get_multiple_account_data(&rpc_client, &token_mints).await?;
    let mut token_decimals = HashMap::new();

    for (i, mint) in token_mints.into_iter().enumerate() {
        let Some(account) = &accounts[i] else {
            log::error!("Failed to get account data for mint {}", mint);
            continue;
        };
        let decimals = match Mint::unpack(&account.data[..Mint::LEN]) {
            Ok(mint) => mint.decimals,
            Err(e) => {
                log::error!("Failed to unpack account data for mint {}: {}", mint, e);
                continue;
            }
        };
        token_decimals.insert(mint, decimals);
    }

    let mut swaps = Vec::with_capacity(transactions_and_swaps.len());
    let mut failed_mints = vec![];
    for (transaction, mut swap) in transactions_and_swaps {
        let input_mint_decimals = token_decimals.get(&swap.input_token_mint);
        let output_mint_decimals = token_decimals.get(&swap.output_token_mint);

        match (input_mint_decimals, output_mint_decimals) {
            (Some(input), Some(output)) => {
                swap.token_decimals = Some(TokenDecimals {
                    input: *input,
                    output: *output,
                })
            }
            _ => {}
        }
        if input_mint_decimals.is_none() {
            failed_mints.push(swap.input_token_mint);
        }
        if output_mint_decimals.is_none() {
            failed_mints.push(swap.output_token_mint);
        }

        let origin_signer = *transaction
            .account_keys()
            .get(0)
            .context("no keys present in txn")?;
        let origin_sol_balance = Balance::create_sol_balance(&transaction, 0);
        let origin_input_mint_balance =
            Balance::create_token_balance(&transaction, &origin_signer, &swap.input_token_mint);
        let origin_output_mint_balance =
            Balance::create_token_balance(&transaction, &origin_signer, &swap.output_token_mint);

        swaps.push(Swap {
            swap,
            origin_signer,
            origin_signature: transaction.signature,
            block_time: transaction.block_time.context("blocktime not defined")?,
            origin_sol_balance,
            origin_input_mint_balance,
            origin_output_mint_balance,
        });
    }
    if !failed_mints.is_empty() {
        log::error!(
            "Wallet {}: Failed to get decimals for mints: {:#?}",
            wallet,
            failed_mints
        );
    }

    Ok(swaps)
}

pub async fn get_swaps_for_wallet2(
    wallet: Pubkey,
    rpc_client: Arc<RpcClient>,
    target_start: i64,
    target_end: i64,
    rpc_concurrency: Option<usize>,
    append_file_path: Option<String>,
    txn_cache: Option<Arc<dyn TransactionCache>>,
) -> anyhow::Result<Vec<Swap>> {
    if target_start >= target_end {
        return Err(anyhow!(
            "start timestamp must be greater than end timestamp"
        ));
    }
    let mut additional = vec![];
    if let Some(path) = append_file_path.as_ref() {
        let fixtures = serde_json::from_str::<WalletSwaps>(&path)?;
        log::debug!(
            "Found {} additional swaps for {}",
            fixtures.swaps.len(),
            wallet
        );
        additional.extend(fixtures.swaps);
    }

    let mut windows = vec![];
    let mut within_bounds = true;

    if additional.is_empty() {
        windows.push(SearchWindow {
            before: None,
            until: None,
            until_ts: target_start,
        })
    }

    if let Some(swap) = additional.first() {
        if target_start < swap.block_time {
            // note: this will fill gaps between target-start and earliest, even if target_end is < earliest
            windows.push(SearchWindow {
                before: Some(swap.origin_signature),
                until: None,
                until_ts: target_start,
            });
        }
        if target_end < swap.block_time {
            within_bounds = false;
        }
    }

    if let Some(swap) = additional.last() {
        if target_end > swap.block_time {
            // note: This will fetch more signatures than requested with `target_end`
            windows.push(SearchWindow {
                before: None,
                until: Some(swap.origin_signature),
                until_ts: swap.block_time,
            })
        }
        if target_start > swap.block_time {
            within_bounds = false;
        }
    }
    log::debug!("within-bounds: {}", within_bounds);

    let mut signatures = vec![];
    for window in windows {
        let (sigs, _) = get_signatures_for_address(
            &rpc_client,
            Some(CommitmentConfig::confirmed()),
            wallet,
            window.before,
            window.until,
            vec![TransactionHistoryLimit::Blocktime(window.until_ts)],
            Some(TransactionHistoryOrder::Forward),
        )
        .await?;
        signatures.extend(sigs);
    }

    let idx = match signatures.binary_search_by(|sig| sig.block_time.unwrap_or(0).cmp(&target_end))
    {
        Ok(idx) => idx,
        Err(idx) => idx,
    };
    _ = signatures.split_off(idx);

    let start = std::time::Instant::now();
    let signatures_count = signatures.len();
    log::info!("Getting transactions for {} signatures", signatures.len());
    let concurrency = rpc_concurrency.unwrap_or(100);
    let transactions = get_transactions_for_signatures(
        Arc::clone(&rpc_client),
        signatures
            .into_iter()
            .map(|sig| Signature::from_str(&sig.signature).unwrap())
            .collect(),
        Some(concurrency),
        Some(|tx: &ITransaction| tx.err.is_none()),
        txn_cache,
    )
    .await?;
    log::info!(
        "Got {} transactions. filtered={}, time-taken={}ms",
        transactions.len(),
        signatures_count - transactions.len(),
        start.elapsed().as_millis(),
    );

    let transactions_and_swaps = transactions
        .into_iter()
        .filter_map(|txn| {
            let swap = match V1Parser::parse_swap_transaction(&txn) {
                Ok(swap) => swap?,
                Err(e) => {
                    log::error!("Error parsing txn {}: {}", txn.signature, e);
                    return None;
                }
            };
            Some((txn, swap))
        })
        .collect::<Vec<_>>();

    let mut token_mints = HashSet::new();
    for (_, swap) in &transactions_and_swaps {
        token_mints.insert(swap.input_token_mint);
        token_mints.insert(swap.output_token_mint);
    }

    let token_mints = Vec::from_iter(token_mints.into_iter());
    let accounts = get_multiple_account_data(&rpc_client, &token_mints).await?;
    let mut token_decimals = HashMap::new();

    for (i, mint) in token_mints.into_iter().enumerate() {
        let Some(account) = &accounts[i] else {
            log::error!("Failed to get account data for mint {}", mint);
            continue;
        };
        let decimals = match Mint::unpack(&account.data[..Mint::LEN]) {
            Ok(mint) => mint.decimals,
            Err(e) => {
                log::error!("Failed to unpack account data for mint {}: {}", mint, e);
                continue;
            }
        };
        token_decimals.insert(mint, decimals);
    }

    let mut swaps = Vec::with_capacity(transactions_and_swaps.len());
    let mut failed_mints = vec![];
    for (transaction, mut swap) in transactions_and_swaps {
        let input_mint_decimals = token_decimals.get(&swap.input_token_mint);
        let output_mint_decimals = token_decimals.get(&swap.output_token_mint);

        match (input_mint_decimals, output_mint_decimals) {
            (Some(input), Some(output)) => {
                swap.token_decimals = Some(TokenDecimals {
                    input: *input,
                    output: *output,
                })
            }
            _ => {}
        }
        if input_mint_decimals.is_none() {
            failed_mints.push(swap.input_token_mint);
        }
        if output_mint_decimals.is_none() {
            failed_mints.push(swap.output_token_mint);
        }

        let origin_signer = *transaction
            .account_keys()
            .get(0)
            .context("no keys present in txn")?;
        let origin_sol_balance = Balance::create_sol_balance(&transaction, 0);
        let origin_input_mint_balance =
            Balance::create_token_balance(&transaction, &origin_signer, &swap.input_token_mint);
        let origin_output_mint_balance =
            Balance::create_token_balance(&transaction, &origin_signer, &swap.output_token_mint);

        swaps.push(Swap {
            swap,
            origin_signer,
            origin_signature: transaction.signature,
            block_time: transaction.block_time.context("blocktime not defined")?,
            origin_sol_balance,
            origin_input_mint_balance,
            origin_output_mint_balance,
        });
    }
    log::error!("Failed mints: {:#?}", failed_mints);

    // Before we create the final fixtures, sort if necessary and remove duplicates
    // We only need a sort if we additional is empty
    if !additional.is_empty() {
        swaps.extend(additional);
        let start = std::time::Instant::now();
        swaps.par_sort_by(|a, b| a.block_time.cmp(&b.block_time));
        swaps.dedup_by(|a, b| a.origin_signature.eq(&b.origin_signature));
        log::debug!(
            "sorting (with deduplication) took {} ms",
            start.elapsed().as_millis()
        );
    }

    let swaps = WalletSwaps { swaps };
    if let Some(path) = append_file_path {
        if within_bounds {
            std::fs::write(path, serde_json::to_string_pretty(&swaps)?)?;
        }
    }

    // Now return our actual answer
    let start_idx = match swaps
        .swaps
        .binary_search_by(|o| o.block_time.cmp(&target_start))
    {
        Ok(idx) => idx,
        Err(idx) => idx,
    };
    let end_idx = match swaps
        .swaps
        .binary_search_by(|o| o.block_time.cmp(&target_end))
    {
        Ok(idx) => idx,
        Err(idx) => idx,
    };

    Ok(swaps.swaps[start_idx..=end_idx].to_vec())
}

struct SearchWindow {
    before: Option<Signature>,
    until: Option<Signature>,
    until_ts: i64,
}
