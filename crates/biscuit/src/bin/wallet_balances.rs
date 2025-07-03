use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Context;
use biscuit::constants::mints::SOL;
use biscuit::swap::jupiter::price::make_chunked_price_request_usd;
use clap::Parser;
use solana_account_decoder::UiAccountData;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_rpc_client_api::request::TokenAccountsFilter;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::native_token::LAMPORTS_PER_SOL;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::str::FromStr;

#[derive(Parser)]
#[clap(version, about, long_about = None)]
pub struct Opts {
    #[clap(short, env = "HELIUS_MAINNET_RPC_URL")]
    rpc_url: String,
    #[clap(short, long, env, default_value = "artifacts/my_wallets.json")]
    wallets: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv()?;
    env_logger::init();

    let opts = Opts::parse();
    let commitment = CommitmentConfig::confirmed();
    let rpc_client = Arc::new(RpcClient::new_with_commitment(opts.rpc_url, commitment));

    use solana_transaction_status::UiTransactionEncoding;
    let config = RpcTransactionConfig {
        encoding: Some(UiTransactionEncoding::Base58),
        commitment: Some(CommitmentConfig::confirmed()),
        max_supported_transaction_version: Some(0),
    };
    let signature = rpc_client
        .get_transaction_with_config(&Signature::new_unique(), config)
        .await;
    println!("Result: {:#?}", signature);
    return Ok(());

    let wallets = serde_json::from_str::<Vec<String>>(&std::fs::read_to_string(opts.wallets)?)?;
    let mut results = HashMap::new();

    for wallet in wallets {
        let Ok(wallet) = Pubkey::from_str(&wallet) else {
            log::error!("Failed to convert {} to pubkey", wallet);
            continue;
        };
        let mut balances = rpc_client
            .get_token_accounts_by_owner_with_commitment(
                &wallet,
                TokenAccountsFilter::ProgramId(spl_token::ID),
                CommitmentConfig::confirmed(),
            )
            .await?
            .value;
        log::debug!("Got {} Tokenkeg balances", balances.len());
        let token_22 = Pubkey::from_str("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb")?;
        let token22_balances = rpc_client
            .get_token_accounts_by_owner_with_commitment(
                &wallet,
                TokenAccountsFilter::ProgramId(token_22),
                CommitmentConfig::confirmed(),
            )
            .await?
            .value;
        log::debug!("Got {} token22 balances", balances.len());
        balances.extend(token22_balances);

        log::debug!("Total balances gotten: {}", balances.len());
        let balances = balances.into_iter().map(|account| {
            let tup = match account.account.data {
                UiAccountData::Json(json) => {
                    let parsed = json
                        .parsed
                        .get("info")
                        .context("failed to get info object from parsed json")?;
                    let mint = parsed
                        .get("mint")
                        .and_then(|x| Pubkey::from_str(x.as_str()?).ok())
                        .context("failed to get mint field from info object")?;
                    let amount = parsed
                        .get("tokenAmount")
                        .and_then(|x| x.get("uiAmountString")?.as_str()?.parse::<f64>().ok())
                        .context("failed to get uiAmountString as f64 from tokenAmount")?;
                    Some((mint, amount))
                }
                _ => None,
            };
            Ok::<_, anyhow::Error>(tup)
        });

        let mut mints = vec![];
        let mut mints_and_amounts = vec![];

        for balance in balances {
            let balance = balance?;
            let Some((mint, amount)) = balance else {
                continue;
            };
            if amount == 0.0 {
                continue;
            }

            mints.push(mint.to_string());
            mints_and_amounts.push((mint, amount));
        }

        let lamports_balance = rpc_client.get_balance(&wallet).await?;

        results.insert(
            wallet,
            WalletResult {
                lamports_balance,
                mints_and_amounts,
            },
        );
    }

    let mut unique_tokens = HashSet::new();
    for result in results.values() {
        unique_tokens.extend(result.mints_and_amounts.iter().map(|x| x.0));
    }
    unique_tokens.insert(SOL);
    let mints = Vec::from_iter(unique_tokens.into_iter().map(|t| t.to_string()));

    log::debug!("Making price request for {} tokens", mints.len());
    let prices = make_chunked_price_request_usd(None, &mints).await;
    log::debug!("Got prices for {} tokens", prices.len());

    let mut global_total = 0.0;

    for (wallet, result) in &results {
        log::info!("***** PRINTING RESULTS FOR WALLET {} *****", wallet);

        let mut total = 0.0;
        let mut mints_and_usdc_values = vec![];
        let mut failed_mints = vec![];

        for (mint, amount) in &result.mints_and_amounts {
            let Some(price) = prices
                .get(&mint.to_string())
                .and_then(|res| res.as_ref().map(|res| res.price))
            else {
                failed_mints.push((mint, amount));
                continue;
            };

            let value = amount * price;
            total += value;
            mints_and_usdc_values.push((mint, value));
        }

        let sol_value = (result.lamports_balance as f64 / LAMPORTS_PER_SOL as f64)
            * prices
                .get(&SOL.to_string())
                .and_then(|res| res.as_ref().map(|price| price.price))
                .context("SOL price should be present")?;
        total += sol_value;

        log::info!(
            "Calculated total balance for {} tokens in wallet {} = {} USD",
            result.mints_and_amounts.len(),
            wallet,
            total
        );
        mints_and_usdc_values.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        log::info!("\t- SOL. value={}", sol_value);
        for (mint, value) in mints_and_usdc_values {
            if value > 1.0 {
                log::info!("\t- mint={},value={}", mint, value);
            }
        }

        // if !failed_mints.is_empty() {
        //     log::info!(
        //         "Failed to calculate balance for {} tokens: ",
        //         failed_mints.len()
        //     );
        //     for (mint, amount) in failed_mints {
        //         log::info!("\t- mint={},amount={}", mint, amount);
        //     }
        // }

        global_total += total;
    }

    log::info!(
        "Calculated balance for {} wallets. Total = {}",
        results.len(),
        global_total
    );

    Ok(())
}

struct WalletResult {
    lamports_balance: u64,
    mints_and_amounts: Vec<(Pubkey, f64)>,
}
