use super::super::history::{self as copy_history, CopyTrade, HistoricalTrades};
use super::CopyStorage;
use crate::database::PgInstance;
use async_trait::async_trait;
use educe::Educe;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use sqlx::types::{
    chrono::{DateTime, Utc},
    Json,
};
use sqlx::{query_as, FromRow, Type};
use std::str::FromStr;

#[async_trait]
impl CopyStorage for PgInstance {
    async fn bootstrap_trade_history(&self) -> anyhow::Result<HistoricalTrades> {
        let exec_trades = self.get_all_copy_execution_trades().await?;
        let history = HistoricalTrades::default();
        for trade in exec_trades {
            let origin_wallet = Pubkey::from_str(&trade.origin_trade.wallet)?;
            let token = Pubkey::from_str(&trade.origin_trade.token)?;
            history.insert_trade_inner(
                &origin_wallet,
                &token,
                u64::try_from(trade.execution_trade.volume_token)?,
                u64::try_from(trade.origin_trade.volume_token)?,
                trade.execution_trade.volume_usd,
                trade.origin_trade.volume_usd,
                trade.execution_trade.volume_sol,
                trade.origin_trade.volume_sol,
                0, // todo! Database doesn't track dust yet
                &trade.origin_trade.direction.into(),
            )?;
        }
        Ok(history)
    }

    async fn process_copy_trade(&self, trade: &CopyTrade) -> anyhow::Result<()> {
        let origin_trade = self
            .insert_trade(
                trade.origin_signature.to_string(),
                trade.origin_wallet.to_string(),
                trade.token.to_string(),
                trade.direction.into(),
                trade
                    .volume
                    .origin_volume_token
                    .try_into()
                    .unwrap_or(i64::MAX), // hmmmm
                trade.volume.origin_volume_usd,
                trade.volume.origin_volume_sol,
            )
            .await?;

        let exec_trade = self
            .insert_trade(
                trade.exec_signature.to_string(),
                trade.exec_wallet.to_string(),
                trade.token.to_string(),
                trade.direction.into(),
                trade
                    .volume
                    .exec_volume_token
                    .try_into()
                    .unwrap_or(i64::MAX), // hmmmm
                trade.volume.exec_volume_usd,
                trade.volume.exec_volume_sol,
            )
            .await?;

        self.insert_copy_execution(exec_trade.id, origin_trade.id)
            .await?;
        Ok(())
    }
}

// Enum representing the trade direction (buy/sell)
#[derive(Debug, Clone, Copy, Type, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "trade_direction", rename_all = "lowercase")]
pub enum TradeDirection {
    Buy,
    Sell,
}
impl From<TradeDirection> for super::super::history::TradeDirection {
    fn from(value: TradeDirection) -> Self {
        match value {
            TradeDirection::Buy => copy_history::TradeDirection::Buy,
            TradeDirection::Sell => copy_history::TradeDirection::Sell,
        }
    }
}
impl From<copy_history::TradeDirection> for TradeDirection {
    fn from(value: copy_history::TradeDirection) -> Self {
        match value {
            copy_history::TradeDirection::Buy => TradeDirection::Buy,
            copy_history::TradeDirection::Sell => TradeDirection::Sell,
        }
    }
}

// Trade row directly from Db
#[derive(Debug, Educe, Clone, FromRow, Serialize, Deserialize)]
#[educe(PartialEq)]
pub struct TradeRaw {
    pub id: i64,
    pub signature: String,
    pub wallet: String,
    pub token: String,
    pub direction: TradeDirection,
    pub volume_token: i64,
    pub volume_usd: f64,
    pub volume_sol: f64,
    #[educe(PartialEq(ignore))]
    pub inserted_at: DateTime<Utc>,
}

// Copy execution row directly from db
#[derive(Debug, Clone, FromRow, Type, PartialEq)]
pub struct CopyExecutionRaw {
    pub trade_id: i64,        // Reference to execution trade id
    pub origin_trade_id: i64, // Reference to origin trade id
}

// Copy execution row joined with trade row
#[derive(Debug, Clone, Educe, FromRow)]
#[educe(PartialEq)]
pub struct CopyExecutionJoined {
    pub execution_trade: Json<TradeRaw>, // Execution trade details
    pub origin_trade: Json<TradeRaw>,    // Origin trade details
}

// Copy execution stats, holding volume and trade count
#[derive(Debug, Clone, FromRow)]
pub struct TradeStats(Json<Stats>);

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, PartialEq)]
pub struct Stats {
    pub total_buy_volume_sol: f64,
    pub total_sell_volume_sol: f64,
    pub total_buy_volume_usd: f64,
    pub total_sell_volume_usd: f64,
    pub total_buys: i64,
    pub total_sells: i64,
}

impl PgInstance {
    // Insert a new trade
    pub async fn insert_trade(
        &self,
        signature: String,
        wallet: String,
        token: String,
        direction: TradeDirection,
        volume_token: i64,
        volume_usd: f64,
        volume_sol: f64,
    ) -> anyhow::Result<TradeRaw> {
        self.fetch_one(
            query_as::<_, TradeRaw>(
                r#"
                INSERT INTO trade (
                    signature,
                    wallet,
                    token,
                    direction,
                    volume_token,
                    volume_usd,
                    volume_sol
                ) 
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                RETURNING *
                "#,
            )
            .bind(signature)
            .bind(wallet)
            .bind(token)
            .bind(direction)
            .bind(volume_token)
            .bind(volume_usd)
            .bind(volume_sol),
        )
        .await
    }

    // Insert a new copy-execution trade (with references to trade ids)
    pub async fn insert_copy_execution(
        &self,
        trade_id: i64,
        origin_trade_id: i64,
    ) -> anyhow::Result<CopyExecutionRaw> {
        self.fetch_one(
            query_as::<_, CopyExecutionRaw>(
                r#"
                INSERT INTO copy_execution (
                    trade_id,
                    origin_trade_id
                )
                VALUES ($1, $2)
                RETURNING *
                "#,
            )
            .bind(trade_id)
            .bind(origin_trade_id),
        )
        .await
    }

    // Retrieve trades by signature
    pub async fn get_trades_for_signature(
        &self,
        signature: String,
    ) -> anyhow::Result<Vec<TradeRaw>> {
        self.fetch_all(
            query_as::<_, TradeRaw>(
                r#"
                SELECT * FROM trade WHERE signature = $1
                "#,
            )
            .bind(signature),
        )
        .await
    }

    // Retrieve trades by the wallet
    pub async fn get_trades_for_wallet(&self, wallet: String) -> anyhow::Result<Vec<TradeRaw>> {
        self.fetch_all(
            query_as::<_, TradeRaw>(
                r#"
                SELECT * FROM trade WHERE wallet = $1
                "#,
            )
            .bind(wallet),
        )
        .await
    }

    // Retrieve a copy-execution trade with joins on the trade table
    pub async fn get_all_copy_execution_trades(&self) -> anyhow::Result<Vec<CopyExecutionJoined>> {
        self.fetch_all(query_as::<_, CopyExecutionJoined>(
            r#"SELECT
            json_build_object(
                'id', et.id,
                'signature', et.signature,
                'wallet', et.wallet,
                'token', et.token,
                'direction', et.direction,
                'volume_token', et.volume_token,
                'volume_usd', et.volume_usd,
                'volume_sol', et.volume_sol,
                'inserted_at', et.inserted_at
            )::jsonb AS execution_trade,
            json_build_object(
                'id', ot.id,
                'signature', ot.signature,
                'wallet', ot.wallet,
                'token', ot.token,
                'direction', ot.direction,
                'volume_token', ot.volume_token,
                'volume_usd', ot.volume_usd,
                'volume_sol', ot.volume_sol,
                'inserted_at', ot.inserted_at
            )::jsonb AS origin_trade
        FROM copy_execution cet
        JOIN trade et ON cet.trade_id = et.id
        JOIN trade ot ON cet.origin_trade_id = ot.id
    "#,
        ))
        .await
    }

    pub async fn get_trade_stats_for_wallet(&self, wallet: String) -> anyhow::Result<TradeStats> {
        self.fetch_one(
            query_as::<_, TradeStats>(
                r#"
                SELECT json_build_object(
                    'total_buy_volume_sol', COALESCE(SUM(CASE WHEN direction = 'buy' THEN volume_sol ELSE 0 END), 0.0),
                    'total_sell_volume_sol', COALESCE(SUM(CASE WHEN direction = 'sell' THEN volume_sol ELSE 0 END), 0.0),
                    'total_buy_volume_usd', COALESCE(SUM(CASE WHEN direction = 'buy' THEN volume_usd ELSE 0 END), 0.0),
                    'total_sell_volume_usd', COALESCE(SUM(CASE WHEN direction = 'sell' THEN volume_usd ELSE 0 END), 0.0),
                    'total_buys', COUNT(CASE WHEN direction = 'buy' THEN 1 END),
                    'total_sells', COUNT(CASE WHEN direction = 'sell' THEN 1 END)
                )::jsonb AS stats
                FROM trade
                WHERE wallet = $1
                "#,
            )
            .bind(wallet),
        )
        .await
    }

    pub async fn get_copy_execution_stats(&self) -> anyhow::Result<TradeStats> {
        self.fetch_one(query_as::<_, TradeStats>(
            r#"
                SELECT json_build_object(
                    'total_buy_volume_sol', COALESCE(SUM(CASE WHEN et.direction = 'buy' THEN et.volume_sol ELSE 0 END), 0.0),
                    'total_sell_volume_sol', COALESCE(SUM(CASE WHEN et.direction = 'sell' THEN et.volume_sol ELSE 0 END), 0.0),
                    'total_buy_volume_usd', COALESCE(SUM(CASE WHEN et.direction = 'buy' THEN et.volume_usd ELSE 0 END), 0.0),
                    'total_sell_volume_usd', COALESCE(SUM(CASE WHEN et.direction = 'sell' THEN et.volume_usd ELSE 0 END), 0.0),
                    'total_buys', COUNT(CASE WHEN et.direction = 'buy' THEN 1 END),
                    'total_sells', COUNT(CASE WHEN et.direction = 'sell' THEN 1 END)
                )::jsonb AS stats
                FROM copy_execution cet
                JOIN trade et ON cet.trade_id = et.id
                "#,
        ))
        .await
    }

    pub async fn get_copy_execution_stats_for_wallet(
        &self,
        wallet: String,
    ) -> anyhow::Result<TradeStats> {
        self.fetch_one(
            query_as::<_, TradeStats>(
                r#"
                SELECT json_build_object(
                    'total_buy_volume_sol', COALESCE(SUM(CASE WHEN et.direction = 'buy' THEN et.volume_sol ELSE 0 END), 0.0),
                    'total_sell_volume_sol', COALESCE(SUM(CASE WHEN et.direction = 'sell' THEN et.volume_sol ELSE 0 END), 0.0),
                    'total_buy_volume_usd', COALESCE(SUM(CASE WHEN et.direction = 'buy' THEN et.volume_usd ELSE 0 END), 0.0),
                    'total_sell_volume_usd', COALESCE(SUM(CASE WHEN et.direction = 'sell' THEN et.volume_usd ELSE 0 END), 0.0),
                    'total_buys', COUNT(CASE WHEN direction = 'buy' THEN 1 END),
                    'total_sells', COUNT(CASE WHEN direction = 'sell' THEN 1 END)
                )::jsonb AS stats
                FROM copy_execution cet
                JOIN trade et ON cet.trade_id = et.id
                WHERE et.wallet = $1
                "#,
            )
            .bind(wallet),
        )
        .await
    }

    pub async fn get_copy_execution_stats_for_origin_wallet(
        &self,
        origin_wallet: String,
    ) -> anyhow::Result<TradeStats> {
        self.fetch_one(
            query_as::<_, TradeStats>(
                r#"
                SELECT json_build_object(
                    'total_buy_volume_sol', COALESCE(SUM(CASE WHEN et.direction = 'buy' THEN et.volume_sol ELSE 0 END), 0.0),
                    'total_sell_volume_sol', COALESCE(SUM(CASE WHEN et.direction = 'sell' THEN et.volume_sol ELSE 0 END), 0.0),
                    'total_buy_volume_usd', COALESCE(SUM(CASE WHEN et.direction = 'buy' THEN et.volume_usd ELSE 0 END), 0.0),
                    'total_sell_volume_usd', COALESCE(SUM(CASE WHEN et.direction = 'sell' THEN et.volume_usd ELSE 0 END), 0.0),
                    'total_buys', COUNT(CASE WHEN et.direction = 'buy' THEN 1 END),
                    'total_sells', COUNT(CASE WHEN et.direction = 'sell' THEN 1 END)
                )::jsonb AS stats
                FROM copy_execution cet
                JOIN trade et ON cet.trade_id = et.id
                JOIN trade ot ON cet.origin_trade_id = ot.id
                WHERE ot.wallet = $1
                "#,
            )
            .bind(origin_wallet),
        )
        .await
    }
}

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");
#[cfg(test)]
pub mod storage_tests {
    use super::*;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::Signature;
    use sqlx::PgPool;

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn db_insertion_and_retrieval_tests(pool: PgPool) -> sqlx::Result<()> {
        let db = PgInstance::from(pool);

        let expected_exec = TradeRaw {
            id: 1,
            signature: Signature::new_unique().to_string(),
            wallet: Pubkey::new_unique().to_string(),
            token: Pubkey::new_unique().to_string(),
            direction: TradeDirection::Buy,
            volume_token: 70_000_000_000,
            volume_usd: 20000.567,
            volume_sol: 87.219876,
            inserted_at: Default::default(),
        };

        let exec = db
            .insert_trade(
                expected_exec.signature.clone(),
                expected_exec.wallet.clone(),
                expected_exec.token.clone(),
                expected_exec.direction.clone(),
                expected_exec.volume_token.clone(),
                expected_exec.volume_usd.clone(),
                expected_exec.volume_sol.clone(),
            )
            .await
            .unwrap();
        assert_eq!(expected_exec, exec);

        let expected_origin = TradeRaw {
            id: 2,
            signature: Signature::new_unique().to_string(),
            wallet: Pubkey::new_unique().to_string(),
            token: exec.token.clone(), // must match exec's
            direction: exec.direction, // must match exec's
            volume_token: 10_000_000_000,
            volume_usd: 4999.9998,
            volume_sol: 20.569878959995595,
            inserted_at: Default::default(),
        };

        let origin = db
            .insert_trade(
                expected_origin.signature.clone(),
                expected_origin.wallet.clone(),
                expected_origin.token.clone(),
                expected_origin.direction.clone(),
                expected_origin.volume_token.clone(),
                expected_origin.volume_usd.clone(),
                expected_origin.volume_sol.clone(),
            )
            .await
            .unwrap();
        assert_eq!(expected_origin, origin);

        let retrieved_trade = db
            .get_trades_for_signature(expected_exec.signature.clone())
            .await
            .unwrap();
        assert_eq!(expected_exec, retrieved_trade[0]);

        let retrieved_trade = db
            .get_trades_for_wallet(expected_exec.wallet.clone())
            .await
            .unwrap();
        assert_eq!(expected_exec, retrieved_trade[0]);

        let expected_copy_execution = CopyExecutionRaw {
            trade_id: exec.id,
            origin_trade_id: origin.id,
        };

        let copy_execution = db.insert_copy_execution(exec.id, origin.id).await.unwrap();

        assert_eq!(expected_copy_execution, copy_execution);

        let joined_copy_execution = db.get_all_copy_execution_trades().await.unwrap();
        assert_eq!(1, joined_copy_execution.len());
        assert_eq!(joined_copy_execution[0].execution_trade.0, exec);
        assert_eq!(joined_copy_execution[0].origin_trade.0, origin);

        // Updated expected stats for buy and sell volumes
        let stats_for_wallet = db
            .get_copy_execution_stats_for_wallet(expected_exec.wallet.clone())
            .await
            .unwrap();
        let expected_stats = Stats {
            total_buy_volume_sol: expected_exec.volume_sol,
            total_sell_volume_sol: 0.0,
            total_buy_volume_usd: expected_exec.volume_usd,
            total_sell_volume_usd: 0.0,
            total_buys: 1,
            total_sells: 0,
        };
        assert_eq!(expected_stats, stats_for_wallet.0 .0);

        let stats_for_origin_wallet = db
            .get_copy_execution_stats_for_origin_wallet(expected_origin.wallet.clone())
            .await
            .unwrap();
        let expected_exec_stats = Stats {
            total_buy_volume_sol: expected_exec.volume_sol,
            total_sell_volume_sol: 0.0,
            total_buy_volume_usd: expected_exec.volume_usd,
            total_sell_volume_usd: 0.0,
            total_buys: 1,
            total_sells: 0,
        };
        assert_eq!(expected_exec_stats, stats_for_origin_wallet.0 .0);

        let no_trade_wallet = Pubkey::new_unique().to_string();
        let empty_stats = db
            .get_copy_execution_stats_for_wallet(no_trade_wallet.clone())
            .await
            .unwrap();
        let expected_empty_stats = Stats {
            total_buy_volume_sol: 0.0,
            total_sell_volume_sol: 0.0,
            total_buy_volume_usd: 0.0,
            total_sell_volume_usd: 0.0,
            total_buys: 0,
            total_sells: 0,
        };
        assert_eq!(expected_empty_stats, empty_stats.0 .0);

        let execution_stats = db.get_copy_execution_stats().await.unwrap();
        let expected_exec_stats = Stats {
            total_buy_volume_sol: expected_exec.volume_sol,
            total_sell_volume_sol: 0.0,
            total_buy_volume_usd: expected_exec.volume_usd,
            total_sell_volume_usd: 0.0,
            total_buys: 1,
            total_sells: 0,
        };
        assert_eq!(expected_exec_stats, execution_stats.0 .0);

        let origin_wallet_stats = db
            .get_trade_stats_for_wallet(origin.wallet.clone())
            .await
            .unwrap();
        let expected_exec_stats = Stats {
            total_buy_volume_sol: origin.volume_sol,
            total_sell_volume_sol: 0.0,
            total_buy_volume_usd: origin.volume_usd,
            total_sell_volume_usd: 0.0,
            total_buys: 1,
            total_sells: 0,
        };
        assert_eq!(expected_exec_stats, origin_wallet_stats.0 .0);
        Ok(())
    }
}

// Yes, when using sqlx::migrate!, your migration files need to follow a specific naming convention in order for sqlx to pick them up correctly.
// The convention for migration files is <VERSION>_<NAME>.sql
// * VERSION is a sequential number (starting from 1).
// * NAME is a description of the migration, usually in lowercase and separated by underscores.
// For example:
// - 1_create_trade_table.sql
// - 2_add_enum_trade_direction.sql
// - 3_add_copy_execution_table.sql

// - https://sqlpad.io/tutorial/postgres-mac-installation/
// - https://gist.github.com/phortuin/2fe698b6c741fd84357cec84219c6667
// - https://www.cherryservers.com/blog/how-to-install-and-setup-postgresql-server-on-ubuntu-20-04
// - https://www.devart.com/dbforge/postgresql/how-to-install-postgresql-on-linux/
// - https://ubuntu.com/server/docs/install-and-configure-postgresql
