THINGS TO CONSIDER:
- Different strategies for amount, and maybe even mints.
- What to do if copy wallet buys with SOL vs other tokens.
- How to get the right input amount for non-fixed strategy when the buyer's inputamount is not specified.
Problem is this depends on the executor. Looks like the specific `swap-details`processor for each Executor has
to handle this, as there's no general way to do so. Get rid of the swap-details type.Potentially get rid of the
`parsed-swap` as well? We might need the swap executor to interact directly with theparsed instruction(+ strategy).
- How to identify when a wallet we're copying sells a tokens we copied them for, andsell that token as well.
- Fallback swap execution(i.e if jupiter fails, fallback to Raydium). Design a properexecutor trait.
- How to retry failed transactions.

**Storage considerations**
- First of all, I can remove the need for a DB if I just use different wallets for each wallet I'm copying. That way
if they sell a token and I have it, it means that I got it from them. That's what I'll go with in the meantime.

If I realize that I need data collection for other reasons:
- Decide on the exact structure of the data I want to store, retrieve the size it takes up in memory.
- Calculate memory-limits and how many of this data I can store.
- Store in memory up to limit. Flush the in-memory data at intervals to persistent storage(embedded-sqlite or postgres)
- Keep track of the last flush checkpoint(blocktime/slot). If program crashes then I'll need it for recovery. This might be
tricky though, as while I can recover my transactions, if I'm copying on a single wallet, I cannot say for what copied wallet
a transaction is for, which is one of the main reasons for storing this data in the first place.

What choice of database to you:
- Rocksdb vs Sqlite vs Postgres vs Mongodb?

* Calculate the cost to store this data on-chain? Can use a different account for each copied wallet. If storage costs too expensive,
can just create a pda with seeds (my-wallet, buy-token, copied-wallet) and check for its existence. Calculate the costs for this.
* Or rather use a different pda account with (my-wallet, copied-wallet) seeds, append an instruction to my program for every copy
that emits the data needed as logs. Can use `get-signatures-for-address` to retrieve the data later. Question though is does
`getSignaturesForAddress` work even if the account is uninitialized? If not then I would still need to pay rent for each of those PDA accounts.
Also check if the SPL-memo program can help me here.

TODO: Abstract over the channel sending interface in most of my services. Do it so that it works with any kind of
sender(or even shared memory like DashMap or Arc<RwLock<HashMap>> or a vec). I already did something similar
by abstracting over the receiving interface as streams.

Experiment with `getSignaturesForAddress`. If it will work even though the account is not initialized then that's pretty cool!
Update: It works, but retrieving the information we need take 2 RPC calls -> 1st one to get the signatures for that address
and then another for each of those to get the transaction data.

Just use a database. Store copy history

Storing 10 MB in Solana costs roughly 70 SOL paid by the creator of the account.
1Mb(1024 kb, 1,048,576 b) -> 7 SOL.
Signature is 64 bytes, pubkey = 32 bytes. Storing about 120 bytes * 1000 entries for each copied wallet would cost 0.8 SOL in rent

Add RPC load balancing
Use an embedded database: Sqlite/Rocksdb. Check if level db is faster. How do they handle persistence if they're on
the same process as my program? Look for async support for sqlite and rocksdb in rust. Look at how the solana codebase
uses rocksdb for inspiration.
ROCKSDb and Sqlite are kinda lower-level. Look at using sled.
Look at bigtable for Solana historical data(Can use for pricing by getting the state of a raydium account at a particular timestamp?):
https://console.cloud.google.com/bigquery?sq=208882271657:a0195371c83c4898b791382ef5cd8405&project=solana-historical-data&ws=!1m5!1m4!4m3!1sbigquery-public-data!2scrypto_solana_mainnet_us!3sAccounts!1m0
CHATGPT answer: Can get data for a particular timestamp:
https://github.com/lquerel/gcp-bigquery-client.git

https://www.chaossearch.io/blog/why-you-need-an-embedded-database
https://www.aabidsofi.com/posts/embedded-databases/
https://users.rust-lang.org/t/using-sqlite-asynchronously/39658
https://www.reddit.com/r/rust/comments/egy0zf/a_database_like_sqlite_with_async/
https://github.com/ryanfowler/async-sqlite.git
https://docs.rs/async-rusqlite/latest/async_rusqlite/
https://github.com/rusqlite/rusqlite/issues/697
https://tedspence.com/investigating-rust-with-sqlite-53d1f9a41112
https://www.powersync.com/blog/sqlite-optimizations-for-ultra-high-performance#:~:text=SQLite%20itself%20is%20fast%2C%20and,with%20typical%20client%2Fserver%20databases.

Look at this transaction!: https://solscan.io/tx/3ThmBPQyfKe1zEyqZnqxD8TziR2npg9mpG7TcrrPnVoNyWmDGWESrqVJHD8Z6VXdHVRMXdpjo9x1MrDgZ5jtKe3f

Dbs
- https://fly.io/docs/reference/redis/
- https://github.com/mozilla/rkv.git
- https://github.com/zshipko/rust-kv
- https://github.com/PoloDB/PoloDB
- https://github.com/vincent-herlemont/native_db
- https://crates.io/crates/darkbird
- https://www.reddit.com/r/rust/comments/w2xgg3/embedded_sql_database/
- https://github.com/rusqlite/rusqlite
- https://github.com/vincent-herlemont/native_db
- https://github.com/PoloDB/PoloDB.git
- https://github.com/rusqlite/rusqlite/issues/697
- https://docs.rs/rocksdb/0.22.0/rocksdb/
- http://skade.github.io/leveldb/leveldb/
- https://news.ycombinator.com/item?id=40030746(sqlite performance)
- https://github.com/shuttle-hq/bigquery-storage
- https://github.com/yoshidan/google-cloud-rust

TODO: Also stream account updates for the wallets we're tracking? Or instead just calculate SOL
balance changes from their transactions? This would be useful especially for the scaled strategy.

Periodically stream and update SOL, USDC, USDT quotes?

See if can write a smart-contract to automate wallet-creation and management(via PDAs)?

```Rust
#[derive(Clone)]
pub struct CopyConfig {
    pub trade_token: Pubkey,
    pub trade_token_decimals: u8,
    pub slippage_tiers: Vec<u16>,
}

#[derive(Clone, Debug)]
pub struct CopyWallet {
    pub buy_strategy: BuyStrategy,
    pub sell_strategy: SellStrategy
}

pub struct CopyAccountRequest {
    pub address: Pubkey,
    pub commitment: Option<CommitmentConfig>,
    pub poll_transactions_frequency: Option<Duration>,
    pub buy_strategy: Option<BuyStrategy>,
    pub sell_strategy: Option<SellStrategy>
}

#[derive(Clone, Debug)]
pub enum BuyStrategy {
    Simple {
        input_amount: u64,
        slippage_bps: u16,
    },
    /// Buy a fixed percentage of whatever the wallet buys
    ScaleFixed {
        scale_bps: u16,
    },
    /// Buy a fixed percentage of whatever the wallet buys but adjust scale based on current wallet balance
    /// Currently unsupported
    ScaleSmart {
        scale_bps: u16
    },
}

#[derive(Clone, Debug)]
pub enum SellStrategy {
    Scale {
        if_swap_keep_bps: u16,
        dust_bps: Option<u16>
    }
}```