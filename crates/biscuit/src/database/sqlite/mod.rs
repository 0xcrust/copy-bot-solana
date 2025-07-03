use anyhow::anyhow;
use log::info;
use sqlx::{
    migrate::MigrateDatabase, query::{Query, QueryAs}, sqlite::{Sqlite, SqliteArguments}, FromRow, Pool, Postgres, SqlitePool
};
use std::{path::Path, str::FromStr};

type TypedQuery<T> = QueryAs<'static, Sqlite, T, SqliteArguments<'static>>;
type UntypedQuery = Query<'static, Sqlite, SqliteArguments<'static>>;

#[derive(Debug, Clone)]
pub struct SqliteDb {
    pool: Pool<Sqlite>,
}
impl From<Pool<Sqlite>> for SqliteDb {
    fn from(pool: Pool<Sqlite>) -> Self {
        SqliteDb { pool }
    }
}

impl SqliteDb {
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        Sqlite::create_database(url).await?;
        let pool = SqlitePool::connect(url).await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> Pool<Sqlite> {
        self.pool.clone()
    }

    pub async fn fetch_one<T, Raw: Into<T> + Send + Unpin + for<'r> FromRow<'r, PgRow>>(
        &self,
        query: TypedQuery<Raw>,
    ) -> anyhow::Result<T> {
        query
            .fetch_one(&self.pool())
            .await
            .map(|row_raw| row_raw.into())
            .map_err(|e| e.into())
    }

    #[allow(unused)]
    pub async fn fetch_optional<T, Raw: Into<T> + Send + Unpin + for<'r> FromRow<'r, PgRow>>(
        &self,
        query: TypedQuery<Raw>,
    ) -> anyhow::Result<Option<T>> {
        query
            .fetch_optional(&self.pool())
            .await
            .map(|row_raw| row_raw.map(|r| r.into()))
            .map_err(|e| e.into())
    }

    #[allow(unused)]
    pub async fn fetch_all<T, Raw: Into<T> + Send + Unpin + for<'r> FromRow<'r, PgRow>>(
        &self,
        query: TypedQuery<Raw>,
    ) -> anyhow::Result<Vec<T>> {
        query
            .fetch_all(&self.pool())
            .await
            .map(|rows_raw| rows_raw.into_iter().map(|r| r.into()).collect())
            .map_err(|e| e.into())
    }

    #[allow(unused)]
    pub async fn execute(&self, query: UntypedQuery) -> anyhow::Result<PgQueryResult> {
        query.execute(&self.pool()).await.map_err(|e| e.into())
    }
}
