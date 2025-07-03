use biscuit::services::copy::v2::simulator::PingBalance;
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, HashSet};
use std::fs;

fn main() -> anyhow::Result<()> {
    // Load the JSON data from a file
    let file_path = "artifacts/simulation/output/output.json"; // Specify your file path here
    let ping_balances =
        serde_json::from_str::<Vec<PingBalance>>(&std::fs::read_to_string(file_path)?)?;

    // Create a HashMap to store the scores for each wallet
    let mut wallet_scores: HashMap<Pubkey, f64> = HashMap::new();

    // Loop through each token and assign scores based on pnl_usd
    for ping_balance in ping_balances {
        // Loop over the first 4 wallets for each token
        for wallet in ping_balance.pings.wallets.iter().take(4) {
            let score = if ping_balance.pnl_usd > 0.0 {
                ping_balance.pnl_usd
            } else {
                -ping_balance.pnl_usd
            };

            // Increment the score for this wallet
            *wallet_scores.entry(wallet.clone()).or_insert(0.0) += score;
        }
    }

    // Sort the wallets by their score in descending order
    let mut sorted_wallets: Vec<(Pubkey, f64)> = wallet_scores.into_iter().collect();
    sorted_wallets.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Less));
    //sorted_wallets.sort_by_key(|&(_, score)| std::cmp::Reverse(score));

    // Output the sorted wallets by score
    println!("Ranked wallets by score:");
    for (wallet, score) in sorted_wallets {
        println!("Wallet: {}, Score: {}", wallet, score);
    }

    Ok(())
}
