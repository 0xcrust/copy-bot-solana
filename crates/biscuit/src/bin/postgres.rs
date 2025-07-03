use anyhow::anyhow;
use log::info;
use sqlx::{ Pool, Postgres };

pub const URL: &str = "";


#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv()?;
    env_logger::init();
    info!("Attempting to connect to Postgres DB");
    let _pool = Pool::<Postgres>::connect(URL).await
        .map(|pool: Pool<Postgres>| {
            info!("Successfully connected to Postgres DB");
            pool
        })
        .map_err(|e| anyhow!("DB connection error: {}", e))?;
    Ok(())
}