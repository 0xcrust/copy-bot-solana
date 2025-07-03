use crate::database::PgInstance;
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use sqlx::{query, query_as, FromRow, Type};

// Enum representing the trade direction (buy/sell)
#[derive(Debug, Clone, Copy, Type, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "trade_direction", rename_all = "lowercase")]
pub enum TradeDirection {
    Buy,
    Sell,
}

#[derive(Debug, Clone, FromRow, PartialEq)]
pub struct AnalyticsToken {
    pub address: String,
    pub decimals: i16,
    pub name: Option<String>,
    pub logo_uri: Option<String>,
    pub symbol: Option<String>,
}

#[derive(Debug, Clone, FromRow, PartialEq)]
pub struct EarlyBirds {
    pub token: String,
    pub wallets: Vec<String>,
}

#[derive(Debug, Clone, FromRow, PartialEq)]
pub struct AnalyticsTrade {
    pub id: i64,
    pub signer: String,
    pub signature: String,
    pub slot: i64,
    pub block_time: i64,
    pub token: String,
    pub other_token: String,
    pub direction: TradeDirection,
    pub token_amount: f64,
    pub other_token_amount: f64,
    pub token_volume_usd: f64,
    pub other_token_volume_usd: f64,
}

pub struct AnalyticsDb(PgInstance);

impl AnalyticsDb {
    pub async fn new(url: &str, ssl_cert: Option<&str>) -> anyhow::Result<Self> {
        Ok(AnalyticsDb(PgInstance::connect(url, ssl_cert).await?))
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct WalletTokenPnl(Json<TokenPnl>);

impl std::ops::Deref for WalletTokenPnl {
    type Target = TokenPnl;

    fn deref(&self) -> &Self::Target {
        &self.0 .0
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct WalletPnl(Json<Pnl>);

impl std::ops::Deref for WalletPnl {
    type Target = Pnl;

    fn deref(&self) -> &Self::Target {
        &self.0 .0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, PartialEq)]
pub struct TokenPnl {
    pub total_buy_volume_token: f64,
    pub total_sell_volume_token: f64,
    pub total_buy_volume_usd: f64,
    pub total_sell_volume_usd: f64,
    pub total_buys: i64,
    pub total_sells: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, PartialEq)]
pub struct Pnl {
    pub total_buy_volume_usd: f64,
    pub total_sell_volume_usd: f64,
    pub total_buys: i64,
    pub total_sells: i64,
}

impl AnalyticsDb {
    pub async fn upsert_token(
        &self,
        address: String,
        decimals: u8,
        name: Option<String>,
        logo_uri: Option<String>,
        symbol: Option<String>,
    ) -> anyhow::Result<AnalyticsToken> {
        self.0
            .fetch_one(
                query_as::<_, AnalyticsToken>(
                    r#"
                INSERT INTO analytics.token_info (
                    address,
                    decimals,
                    name,
                    logo_uri,
                    symbol
                )
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT (address)
                DO UPDATE SET
                    decimals = EXCLUDED.decimals,
                    name = EXCLUDED.name,
                    logo_uri = EXCLUDED.logo_uri,
                    symbol = EXCLUDED.symbol
                RETURNING *
                "#,
                )
                .bind(address)
                .bind(i16::from(decimals))
                .bind(name)
                .bind(logo_uri)
                .bind(symbol),
            )
            .await
    }

    /// Inserts a single wallet or a group of wallets into the `early_birds` table.
    /// Ensures that only unique wallet addresses are stored, avoiding duplicates.
    pub async fn insert_early_birds(
        &self,
        token: String,
        wallets: Vec<String>,
    ) -> anyhow::Result<()> {
        self.0.execute(
            query::<>(
                r#"
                INSERT INTO analytics.early_birds (token, wallets)
                VALUES ($1, $2)
                ON CONFLICT (token)
                DO UPDATE SET wallets = (
                    SELECT ARRAY(
                        SELECT DISTINCT unnest(array_cat(analytics.early_birds.wallets, EXCLUDED.wallets))
                    )
                )
                "#,
            )
            .bind(token)
            .bind(wallets)
        ).await?;

        Ok(())
    }

    pub async fn insert_trade(
        &self,
        signer: String,
        signature: String,
        slot: i64,
        block_time: i64,
        token: String,
        other_token: String,
        direction: TradeDirection,
        token_amount: f64,
        other_token_amount: f64,
        token_volume_usd: f64,
        other_token_volume_usd: f64,
    ) -> anyhow::Result<AnalyticsTrade> {
        self.0
            .fetch_one(
                query_as::<_, AnalyticsTrade>(
                    r#"
                INSERT INTO analytics.trade (
                    signer,
                    signature,
                    slot,
                    block_time,
                    token,
                    other_token,
                    direction,
                    token_amount,
                    other_token_amount,
                    token_volume_usd,
                    other_token_volume_usd
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
                RETURNING *
                "#,
                )
                .bind(signer)
                .bind(signature)
                .bind(slot)
                .bind(block_time)
                .bind(token)
                .bind(other_token)
                .bind(direction)
                .bind(token_amount)
                .bind(other_token_amount)
                .bind(token_volume_usd)
                .bind(other_token_volume_usd),
            )
            .await
    }

    pub async fn get_early_bird_cache(&self) -> anyhow::Result<Vec<EarlyBirds>> {
        self.0
            .fetch_all(query_as::<_, EarlyBirds>(
                r#"SELECT * FROM analytics.early_birds"#,
            ))
            .await
    }

    pub async fn get_early_wallets_for_token(
        &self,
        token: String,
    ) -> anyhow::Result<Option<EarlyBirds>> {
        self.0
            .fetch_optional(
                query_as::<_, EarlyBirds>(
                    r#"SELECT * FROM analytics.early_birds WHERE token = $1"#,
                )
                .bind(token),
            )
            .await
    }

    pub async fn get_trades_for_wallet_and_token(
        &self,
        wallet: String,
        token: String,
    ) -> anyhow::Result<Vec<AnalyticsTrade>> {
        self.0
            .fetch_all(
                query_as::<_, AnalyticsTrade>(
                    r#"
                SELECT * FROM analytics.trade 
                WHERE signer = $1 AND token = $2
                ORDER BY block_time ASC
                "#,
                )
                .bind(wallet)
                .bind(token),
            )
            .await
    }

    pub async fn get_most_recent_trade_for_wallet(
        &self,
        wallet: String,
    ) -> anyhow::Result<Option<AnalyticsTrade>> {
        self.0
            .fetch_optional(
                query_as::<_, AnalyticsTrade>(
                    r#"
                SELECT * FROM analytics.trade 
                WHERE signer = $1
                ORDER BY block_time DESC
                LIMIT 1
                "#,
                )
                .bind(wallet),
            )
            .await
    }

    pub async fn get_all_trades_for_wallet(
        &self,
        wallet: String,
    ) -> anyhow::Result<Vec<AnalyticsTrade>> {
        self.0
            .fetch_all(
                query_as::<_, AnalyticsTrade>(r#"SELECT * FROM analytics.trade WHERE signer = $1"#)
                    .bind(wallet),
            )
            .await
    }

    pub async fn get_wallet_pnl(&self, wallet: String) -> anyhow::Result<WalletPnl> {
        self.0.fetch_one(
            query_as::<_, WalletPnl>(
                r#"
                SELECT json_build_object(
                    'total_buy_volume_usd', COALESCE(SUM(CASE WHEN direction  = 'buy' THEN token_volume_usd ELSE 0 END), 0.0),
                    'total_sell_volume_usd', COALESCE(SUM(CASE WHEN direction  = 'sell' THEN token_volume_usd ELSE 0 END), 0.0),
                    'total_buys', COUNT(CASE WHEN direction = 'buy' THEN 1 END),
                    'total_sells', COUNT(CASE WHEN direction = 'sell' THEN 1 END)
                )
                FROM analytics.trade
                WHERE signer = $1
                "#
            )
            .bind(wallet)
        ).await
    }

    pub async fn get_wallet_pnl_for_token(
        &self,
        wallet: String,
        token: String,
    ) -> anyhow::Result<WalletTokenPnl> {
        self.0.fetch_one(
            query_as::<_, WalletTokenPnl>(
                r#"
                SELECT json_build_object(
                    'total_buy_volume_token', COALESCE(SUM(CASE WHEN direction  = 'buy' THEN token_amount ELSE 0 END), 0.0),
                    'total_sell_volume_token', COALESCE(SUM(CASE WHEN direction  = 'sell' THEN token_amount ELSE 0 END), 0.0),
                    'total_buy_volume_usd', COALESCE(SUM(CASE WHEN direction  = 'buy' THEN token_volume_usd ELSE 0 END), 0.0),
                    'total_sell_volume_usd', COALESCE(SUM(CASE WHEN direction  = 'sell' THEN token_volume_usd ELSE 0 END), 0.0),
                    'total_buys', COUNT(CASE WHEN direction = 'buy' THEN 1 END),
                    'total_sells', COUNT(CASE WHEN direction = 'sell' THEN 1 END)
                )
                FROM analytics.trade
                WHERE signer = $1 AND token = $2
                "#
            )
            .bind(wallet)
            .bind(token)
        ).await
    }
}

#[cfg(test)]
pub mod analytics_db_tests {
    use super::*;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::Signature;
    use sqlx::PgPool;

    // Use the same migration logic as in the rest of the system
    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./src/analytics/migrations");

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_upsert_token(pool: PgPool) -> sqlx::Result<()> {
        let db = AnalyticsDb(PgInstance::from(pool));

        let address = Pubkey::new_unique().to_string();
        let decimals = 9;
        let name = Some("TestToken".to_string());
        let logo_uri = Some("https://logo.com/token.png".to_string());
        let symbol = Some("TT".to_string());

        // First insert (should perform an INSERT)
        let token = db
            .upsert_token(
                address.clone(),
                decimals,
                name.clone(),
                logo_uri.clone(),
                symbol.clone(),
            )
            .await
            .unwrap();

        assert_eq!(token.address, address);
        assert_eq!(token.decimals, decimals as i16);
        assert_eq!(token.name, name);

        // Upsert (should perform an UPDATE)
        let updated_name = Some("UpdatedToken".to_string());
        let updated_token = db
            .upsert_token(
                address.clone(),
                decimals,
                updated_name.clone(),
                logo_uri.clone(),
                symbol.clone(),
            )
            .await
            .unwrap();

        assert_eq!(updated_token.name, updated_name);
        assert_eq!(updated_token.address, address);

        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_insert_early_birds(pool: PgPool) -> sqlx::Result<()> {
        let db = AnalyticsDb(PgInstance::from(pool));

        let token = Pubkey::new_unique().to_string();
        let wallets = vec![
            Pubkey::new_unique().to_string(),
            Pubkey::new_unique().to_string(),
        ];

        // Insert new token with wallets
        db.insert_early_birds(token.clone(), wallets.clone())
            .await
            .unwrap();

        // Fetch and verify wallets were inserted
        let early_birds = db
            .get_early_wallets_for_token(token.clone())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(early_birds.token, token);
        assert_eq!(early_birds.wallets, wallets);

        // Insert with some duplicates (should avoid duplicates)
        let new_wallets = vec![wallets[0].clone(), Pubkey::new_unique().to_string()];
        db.insert_early_birds(token.clone(), new_wallets.clone())
            .await
            .unwrap();

        // Fetch again and verify wallets were updated without duplicates
        let updated_early_birds = db
            .get_early_wallets_for_token(token.clone())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated_early_birds.wallets.len(), 3); // Should be 3 unique wallets
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_insert_trade(pool: PgPool) -> sqlx::Result<()> {
        let db = AnalyticsDb(PgInstance::from(pool));

        let signer = Pubkey::new_unique().to_string();
        let signature = Signature::new_unique().to_string();
        let slot = 123456789;
        let block_time = 1634235260;
        let token = Pubkey::new_unique().to_string();
        let other_token = Pubkey::new_unique().to_string();
        let direction = TradeDirection::Buy;
        let token_amount = 1000.0;
        let other_token_amount = 500.0;
        let token_volume_usd = 100.0;
        let other_token_volume_usd = 50.0;

        // Insert trade
        let trade = db
            .insert_trade(
                signer.clone(),
                signature.clone(),
                slot,
                block_time,
                token.clone(),
                other_token.clone(),
                direction,
                token_amount,
                other_token_amount,
                token_volume_usd,
                other_token_volume_usd,
            )
            .await
            .unwrap();

        assert_eq!(trade.signer, signer);
        assert_eq!(trade.token, token);
        assert_eq!(trade.token_amount, token_amount);

        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_get_early_bird_cache(pool: PgPool) -> sqlx::Result<()> {
        let db = AnalyticsDb(PgInstance::from(pool));

        // Insert early birds
        let token1 = Pubkey::new_unique().to_string();
        let token2 = Pubkey::new_unique().to_string();
        db.insert_early_birds(token1.clone(), vec![Pubkey::new_unique().to_string()])
            .await
            .unwrap();
        db.insert_early_birds(token2.clone(), vec![Pubkey::new_unique().to_string()])
            .await
            .unwrap();

        // Fetch early birds cache
        let early_bird_cache = db.get_early_bird_cache().await.unwrap();

        // Verify that the cache contains both tokens
        assert_eq!(early_bird_cache.len(), 2);
        assert!(early_bird_cache.iter().any(|eb| eb.token == token1));
        assert!(early_bird_cache.iter().any(|eb| eb.token == token2));

        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_get_trades_for_wallet_and_token(pool: PgPool) -> sqlx::Result<()> {
        let db = AnalyticsDb(PgInstance::from(pool));

        let wallet = Pubkey::new_unique().to_string();
        let token = Pubkey::new_unique().to_string();

        // Insert two trades for the wallet and token
        for _ in 0..2 {
            db.insert_trade(
                wallet.clone(),
                Signature::new_unique().to_string(),
                123456789,
                1634235260,
                token.clone(),
                Pubkey::new_unique().to_string(),
                TradeDirection::Buy,
                1000.0,
                500.0,
                100.0,
                50.0,
            )
            .await
            .unwrap();
        }

        // Fetch trades for wallet and token
        let trades = db
            .get_trades_for_wallet_and_token(wallet.clone(), token.clone())
            .await
            .unwrap();

        // Verify trades
        assert_eq!(trades.len(), 2);
        assert!(trades.iter().all(|t| t.token == token));
        assert!(trades.iter().all(|t| t.signer == wallet));

        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_get_most_recent_trade_for_wallet(pool: PgPool) -> sqlx::Result<()> {
        let db = AnalyticsDb(PgInstance::from(pool));

        let wallet = Pubkey::new_unique().to_string();

        // Insert a trade for the wallet
        let trade = db
            .insert_trade(
                wallet.clone(),
                Signature::new_unique().to_string(),
                123456789,
                1634235260,
                Pubkey::new_unique().to_string(),
                Pubkey::new_unique().to_string(),
                TradeDirection::Buy,
                1000.0,
                500.0,
                100.0,
                50.0,
            )
            .await
            .unwrap();

        // Fetch the most recent trade for the wallet
        let most_recent_trade = db
            .get_most_recent_trade_for_wallet(wallet.clone())
            .await
            .unwrap();

        // Verify that the fetched trade is the most recent one
        assert!(most_recent_trade.is_some());
        assert_eq!(most_recent_trade.unwrap().id, trade.id);

        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_get_all_trades_for_wallet(pool: PgPool) -> sqlx::Result<()> {
        let db = AnalyticsDb(PgInstance::from(pool));

        let wallet = Pubkey::new_unique().to_string();

        // Insert two trades for the wallet
        for _ in 0..2 {
            db.insert_trade(
                wallet.clone(),
                Signature::new_unique().to_string(),
                123456789,
                1634235260,
                Pubkey::new_unique().to_string(),
                Pubkey::new_unique().to_string(),
                TradeDirection::Buy,
                1000.0,
                500.0,
                100.0,
                50.0,
            )
            .await
            .unwrap();
        }

        // Fetch all trades for the wallet
        let trades = db.get_all_trades_for_wallet(wallet.clone()).await.unwrap();

        // Verify that two trades exist
        assert_eq!(trades.len(), 2);
        assert!(trades.iter().all(|t| t.signer == wallet));

        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_get_wallet_pnl(pool: PgPool) -> sqlx::Result<()> {
        let db = AnalyticsDb(PgInstance::from(pool));

        let wallet = Pubkey::new_unique().to_string();

        // Insert some trades to calculate PnL
        for _ in 0..2 {
            db.insert_trade(
                wallet.clone(),
                Signature::new_unique().to_string(),
                123456789,
                1634235260,
                Pubkey::new_unique().to_string(),
                Pubkey::new_unique().to_string(),
                TradeDirection::Buy,
                1000.0,
                500.0,
                100.0,
                50.0,
            )
            .await
            .unwrap();
        }

        // Fetch the PnL for the wallet
        let pnl = db.get_wallet_pnl(wallet.clone()).await.unwrap();

        // Verify PnL data
        assert_eq!(pnl.total_buy_volume_usd, 200.0);
        assert_eq!(pnl.total_sells, 0);
        assert_eq!(pnl.total_buys, 2);

        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_get_wallet_pnl_for_token(pool: PgPool) -> sqlx::Result<()> {
        let db = AnalyticsDb(PgInstance::from(pool));

        let wallet = Pubkey::new_unique().to_string();
        let token = Pubkey::new_unique().to_string();

        // Insert two trades for the wallet and token
        for _ in 0..2 {
            db.insert_trade(
                wallet.clone(),
                Signature::new_unique().to_string(),
                123456789,
                1634235260,
                token.clone(),
                Pubkey::new_unique().to_string(),
                TradeDirection::Buy,
                1000.0,
                500.0,
                100.0,
                50.0,
            )
            .await
            .unwrap();
        }

        // Fetch the PnL for the wallet and token
        let pnl = db
            .get_wallet_pnl_for_token(wallet.clone(), token.clone())
            .await
            .unwrap();

        // Verify PnL data
        assert_eq!(pnl.total_buy_volume_token, 2000.0);
        assert_eq!(pnl.total_sells, 0);
        assert_eq!(pnl.total_buys, 2);

        Ok(())
    }
}

// #[cfg(test)]
// pub mod analytics_storage_tests {
//     use super::*;
//     use solana_sdk::pubkey::Pubkey;
//     use solana_sdk::signature::Signature;
//     use sqlx::PgPool;

//     static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./src/analytics/migrations");

//     #[sqlx::test(migrator = "MIGRATOR")]
//     async fn test_insert_and_fetch_token(pool: PgPool) -> sqlx::Result<()> {
//         let db = AnalyticsDb(PgInstance::from(pool));

//         let expected_token = AnalyticsToken {
//             address: Pubkey::new_unique().to_string(),
//             decimals: 9,
//             name: Some("TestToken".to_string()),
//             logo_uri: Some("https://logo.com/token.png".to_string()),
//             symbol: Some("TT".to_string()),
//         };

//         let inserted_token = db
//             .upsert_token(
//                 expected_token.address.clone(),
//                 expected_token.decimals as u8,
//                 expected_token.name.clone(),
//                 expected_token.logo_uri.clone(),
//                 expected_token.symbol.clone(),
//             )
//             .await
//             .unwrap();

//         // Check that the inserted token matches expectations
//         assert_eq!(expected_token, inserted_token);
//         Ok(())
//     }

//     #[sqlx::test(migrator = "MIGRATOR")]
//     async fn test_insert_and_fetch_token_trade(pool: PgPool) -> sqlx::Result<()> {
//         let db = AnalyticsDb(PgInstance::from(pool));

//         let token = db
//             .upsert_token(
//                 Pubkey::new_unique().to_string(),
//                 9,
//                 Some("TestToken".to_string()),
//                 Some("https://logo.com/token.png".to_string()),
//                 Some("TT".to_string()),
//             )
//             .await
//             .unwrap();

//         let expected_trade = AnaTrade {
//             id: 1, // Auto-generated ID
//             token: token.address.clone(),
//             birdeye_idx: 1000,
//             signer: Pubkey::new_unique().to_string(),
//             signature: Signature::new_unique().to_string(),
//             block_time: 1234567890,
//             source_token: Pubkey::new_unique().to_string(),
//             destination_token: Pubkey::new_unique().to_string(),
//             source_token_price: 50.0,
//             destination_token_price: 100.0,
//             source_amount: 1000.0,
//             destination_amount: 2000.0,
//             price_pair: 2.0,
//         };

//         let inserted_trade = db
//             .analytics_insert_token_trade(
//                 expected_trade.token.clone(),
//                 expected_trade.birdeye_idx,
//                 expected_trade.signer.clone(),
//                 expected_trade.signature.clone(),
//                 expected_trade.block_time,
//                 expected_trade.source_token.clone(),
//                 expected_trade.destination_token.clone(),
//                 expected_trade.source_token_price,
//                 expected_trade.destination_token_price,
//                 expected_trade.source_amount,
//                 expected_trade.destination_amount,
//                 expected_trade.price_pair,
//             )
//             .await
//             .unwrap();

//         assert_eq!(expected_trade.token, inserted_trade.token);
//         assert_eq!(expected_trade.birdeye_idx, inserted_trade.birdeye_idx);
//         assert_eq!(expected_trade.signer, inserted_trade.signer);

//         Ok(())
//     }

//     #[sqlx::test(migrator = "MIGRATOR")]
//     async fn test_insert_and_fetch_wallet_trade(pool: PgPool) -> sqlx::Result<()> {
//         let db = AnalyticsDb(PgInstance::from(pool));

//         let token = db
//             .analytics_insert_token(
//                 Pubkey::new_unique().to_string(),
//                 9,
//                 Some("TestToken".to_string()),
//                 Some("https://logo.com/token.png".to_string()),
//                 Some("TT".to_string()),
//             )
//             .await
//             .unwrap();

//         let expected_trade = WalletTrade {
//             id: 1, // Auto-generated ID
//             signer: Pubkey::new_unique().to_string(),
//             signature: Signature::new_unique().to_string(),
//             slot: 1234567890,
//             block_time: 1234567890,
//             token: token.address.clone(),
//             other_token: Pubkey::new_unique().to_string(),
//             direction: TradeDirection::Buy,
//             token_amount: 500.0,
//             other_token_amount: 1000.0,
//             token_volume_usd: 100.0,
//             other_token_volume_usd: 200.0,
//         };

//         let inserted_trade = db
//             .analytics_insert_wallet_trade(
//                 expected_trade.signer.clone(),
//                 expected_trade.signature.clone(),
//                 expected_trade.slot,
//                 expected_trade.block_time,
//                 expected_trade.token.clone(),
//                 expected_trade.other_token.clone(),
//                 expected_trade.direction,
//                 expected_trade.token_amount,
//                 expected_trade.other_token_amount,
//                 expected_trade.token_volume_usd,
//                 expected_trade.other_token_volume_usd,
//             )
//             .await
//             .unwrap();

//         assert_eq!(expected_trade.token, inserted_trade.token);
//         assert_eq!(expected_trade.signer, inserted_trade.signer);
//         assert_eq!(
//             expected_trade.token_volume_usd,
//             inserted_trade.token_volume_usd
//         );

//         Ok(())
//     }

//     #[sqlx::test(migrator = "MIGRATOR")]
//     async fn test_get_trades_for_token(pool: PgPool) -> sqlx::Result<()> {
//         let db = AnalyticsDb(PgInstance::from(pool));

//         let token = db
//             .analytics_insert_token(
//                 Pubkey::new_unique().to_string(),
//                 9,
//                 Some("TestToken".to_string()),
//                 Some("https://logo.com/token.png".to_string()),
//                 Some("TT".to_string()),
//             )
//             .await
//             .unwrap();

//         let trade = db
//             .analytics_insert_token_trade(
//                 token.address.clone(),
//                 1000,
//                 Pubkey::new_unique().to_string(),
//                 Signature::new_unique().to_string(),
//                 1234567890,
//                 Pubkey::new_unique().to_string(),
//                 Pubkey::new_unique().to_string(),
//                 50.0,
//                 100.0,
//                 1000.0,
//                 2000.0,
//                 2.0,
//             )
//             .await
//             .unwrap();

//         let trades = db
//             .analytics_get_trades_for_token(token.address.clone())
//             .await
//             .unwrap();

//         assert_eq!(trades.len(), 1);
//         assert_eq!(trades[0].token, trade.token);

//         Ok(())
//     }

//     #[sqlx::test(migrator = "MIGRATOR")]
//     async fn test_get_trades_for_wallet(pool: PgPool) -> sqlx::Result<()> {
//         let db = AnalyticsDb(PgInstance::from(pool));

//         let wallet = Pubkey::new_unique().to_string();
//         let token = db
//             .analytics_insert_token(
//                 Pubkey::new_unique().to_string(),
//                 9,
//                 Some("TestToken".to_string()),
//                 Some("https://logo.com/token.png".to_string()),
//                 Some("TT".to_string()),
//             )
//             .await
//             .unwrap();

//         let trade = db
//             .analytics_insert_wallet_trade(
//                 wallet.clone(),
//                 Signature::new_unique().to_string(),
//                 1234567890,
//                 1234567890,
//                 token.address.clone(),
//                 Pubkey::new_unique().to_string(),
//                 TradeDirection::Buy,
//                 1000.0,
//                 2000.0,
//                 100.0,
//                 200.0,
//             )
//             .await
//             .unwrap();

//         let trades = db
//             .analytics_get_trades_for_wallet(wallet.clone())
//             .await
//             .unwrap();

//         assert_eq!(trades.len(), 1);
//         assert_eq!(trades[0].signer, trade.signer);

//         Ok(())
//     }

//     #[sqlx::test(migrator = "MIGRATOR")]
//     async fn test_get_wallet_trades_for_token(pool: PgPool) -> sqlx::Result<()> {
//         let db = AnalyticsDb(PgInstance::from(pool));

//         let wallet = Pubkey::new_unique().to_string();
//         let token = db
//             .analytics_insert_token(
//                 Pubkey::new_unique().to_string(),
//                 9,
//                 Some("TestToken".to_string()),
//                 Some("https://logo.com/token.png".to_string()),
//                 Some("TT".to_string()),
//             )
//             .await
//             .unwrap();

//         db.analytics_insert_wallet_trade(
//             wallet.clone(),
//             Signature::new_unique().to_string(),
//             1234567890,
//             1234567890,
//             token.address.clone(),
//             Pubkey::new_unique().to_string(),
//             TradeDirection::Buy,
//             1000.0,
//             2000.0,
//             100.0,
//             200.0,
//         )
//         .await
//         .unwrap();

//         let trades = db
//             .get_wallet_trades_for_token(wallet.clone(), token.address.clone())
//             .await
//             .unwrap();

//         assert_eq!(trades.len(), 1);
//         assert_eq!(trades[0].token, token.address);

//         Ok(())
//     }

//     #[sqlx::test(migrator = "MIGRATOR")]
//     async fn test_get_wallet_pnl(pool: PgPool) -> sqlx::Result<()> {
//         let db = AnalyticsDb(PgInstance::from(pool));

//         let wallet = Pubkey::new_unique().to_string();

//         let token = db
//             .analytics_insert_token(
//                 Pubkey::new_unique().to_string(),
//                 9,
//                 Some("TestToken".to_string()),
//                 Some("https://logo.com/token.png".to_string()),
//                 Some("TT".to_string()),
//             )
//             .await
//             .unwrap();

//         db.analytics_insert_wallet_trade(
//             wallet.clone(),
//             Signature::new_unique().to_string(),
//             1234567890,
//             1234567890,
//             token.address.clone(),
//             Pubkey::new_unique().to_string(),
//             TradeDirection::Buy,
//             1000.0,
//             2000.0,
//             100.0,
//             200.0,
//         )
//         .await
//         .unwrap();

//         let pnl = db.get_wallet_pnl(wallet.clone()).await.unwrap();

//         // Ensure the PnL matches the expected result
//         assert_eq!(pnl.total_buy_volume_usd, 100.0);
//         assert_eq!(pnl.total_sells, 0);

//         Ok(())
//     }

//     #[sqlx::test(migrator = "MIGRATOR")]
//     async fn test_get_wallet_pnl_for_token(pool: PgPool) -> sqlx::Result<()> {
//         let db = AnalyticsDb(PgInstance::from(pool));

//         let wallet = Pubkey::new_unique().to_string();
//         let token = db
//             .analytics_insert_token(
//                 Pubkey::new_unique().to_string(),
//                 9,
//                 Some("TestToken".to_string()),
//                 Some("https://logo.com/token.png".to_string()),
//                 Some("TT".to_string()),
//             )
//             .await
//             .unwrap();

//         db.analytics_insert_wallet_trade(
//             wallet.clone(),
//             Signature::new_unique().to_string(),
//             1234567890,
//             1234567890,
//             token.address.clone(),
//             Pubkey::new_unique().to_string(),
//             TradeDirection::Buy,
//             1000.0,
//             2000.0,
//             100.0,
//             200.0,
//         )
//         .await
//         .unwrap();

//         let pnl = db
//             .get_wallet_pnl_for_token(wallet.clone(), token.address.clone())
//             .await
//             .unwrap();

//         assert_eq!(pnl.total_buy_volume_usd, 100.0);
//         assert_eq!(pnl.total_sells, 0);

//         Ok(())
//     }

//     #[sqlx::test(migrator = "MIGRATOR")]
//     async fn test_get_unique_token_buyers(pool: PgPool) -> sqlx::Result<()> {
//         let db = AnalyticsDb(PgInstance::from(pool));

//         let token = db
//             .analytics_insert_token(
//                 Pubkey::new_unique().to_string(),
//                 9,
//                 Some("TestToken".to_string()),
//                 Some("https://logo.com/token.png".to_string()),
//                 Some("TT".to_string()),
//             )
//             .await
//             .unwrap();

//         db.analytics_insert_token_trade(
//             token.address.clone(),
//             2,
//             Pubkey::new_unique().to_string(),
//             Signature::new_unique().to_string(),
//             1234567890,
//             Pubkey::new_unique().to_string(),
//             token.address.clone(),
//             1000.0,
//             2000.0,
//             100.0,
//             200.0,
//             2.233,
//         )
//         .await
//         .unwrap();

//         // db.analytics_insert_wallet_trade(
//         //     Pubkey::new_unique().to_string(),
//         //     Signature::new_unique().to_string(),
//         //     1234567890,
//         //     1234567890,
//         //     token.address.clone(),
//         //     Pubkey::new_unique().to_string(),
//         //     TradeDirection::Buy,
//         //     1000.0,
//         //     2000.0,
//         //     100.0,
//         //     200.0,
//         // )
//         // .await
//         // .unwrap();

//         let buyers = db
//             .get_unique_token_buyers(token.address.clone())
//             .await
//             .unwrap();

//         assert_eq!(buyers.len(), 1);

//         Ok(())
//     }

//     #[sqlx::test(migrator = "MIGRATOR")]
//     async fn test_get_most_recent_trade_for_wallet(pool: PgPool) -> sqlx::Result<()> {
//         let db = AnalyticsDb(PgInstance::from(pool));

//         let wallet = Pubkey::new_unique().to_string();
//         let token = db
//             .analytics_insert_token(
//                 Pubkey::new_unique().to_string(),
//                 9,
//                 Some("TestToken".to_string()),
//                 Some("https://logo.com/token.png".to_string()),
//                 Some("TT".to_string()),
//             )
//             .await
//             .unwrap();

//         let expected_trade = db
//             .analytics_insert_wallet_trade(
//                 wallet.clone(),
//                 Signature::new_unique().to_string(),
//                 1234567890,
//                 1234567890,
//                 token.address.clone(),
//                 Pubkey::new_unique().to_string(),
//                 TradeDirection::Buy,
//                 1000.0,
//                 2000.0,
//                 100.0,
//                 200.0,
//             )
//             .await
//             .unwrap();

//         let most_recent = db
//             .get_most_recent_trade_for_wallet(wallet.clone())
//             .await
//             .unwrap();

//         assert_eq!(most_recent.unwrap().signature, expected_trade.signature);

//         Ok(())
//     }
// }
