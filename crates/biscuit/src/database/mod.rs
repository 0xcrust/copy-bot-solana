mod cache;
// mod sqlite;

use anyhow::anyhow;
use log::info;
use sqlx::{
    postgres::{PgArguments, PgConnectOptions, PgQueryResult, PgRow, PgSslMode},
    query::{Query, QueryAs},
    FromRow, Pool, Postgres,
};
use std::{path::Path, str::FromStr};

pub type TypedQuery<T> = QueryAs<'static, Postgres, T, PgArguments>;
pub type UntypedQuery = Query<'static, Postgres, PgArguments>;

#[derive(Debug, Clone)]
pub struct PgInstance {
    pool: Pool<Postgres>,
}
impl From<Pool<Postgres>> for PgInstance {
    fn from(pool: Pool<Postgres>) -> Self {
        PgInstance { pool }
    }
}

impl PgInstance {
    pub async fn connect(url: &str, ssl_cert: Option<&str>) -> anyhow::Result<Self> {
        info!("Attempting to connect to Postgres DB");
        let pool = if let Some(ssl_cert) = ssl_cert {
            let options = PgConnectOptions::from_str(url)
                .map_err(|e| anyhow!("Invalid database URL {}: {}", url, e))?
                .ssl_root_cert(Path::new(ssl_cert))
                .ssl_mode(PgSslMode::Require);
            Pool::<Postgres>::connect_with(options).await
        } else {
            Pool::<Postgres>::connect(url).await
        }
        .map(|pool: Pool<Postgres>| {
            info!("Successfully connected to Postgres DB");
            pool
        })
        .map_err(|e| anyhow!("DB connection error: {}", e))?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> Pool<Postgres> {
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
