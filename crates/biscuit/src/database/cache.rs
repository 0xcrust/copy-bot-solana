use super::PgInstance;
use crate::{
    core::traits::{TokenCache, TransactionCache},
    swap::jupiter::token_list_new::JupApiToken,
};
use anyhow::anyhow;
use async_trait::async_trait;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_transaction_status::EncodedConfirmedTransactionWithStatusMeta;
use sqlx::{query_as, FromRow};
use std::str::FromStr;

#[derive(Debug, FromRow)]
pub struct CacheTransaction {
    pub signature: String,
    pub signer: String,
    pub transaction: Vec<u8>,
}

#[derive(Debug, FromRow)]
pub struct CacheToken {
    pub address: String,
    pub decimals: i16,
    pub name: Option<String>,
    pub logo_uri: Option<String>,
    pub symbol: Option<String>,
    pub program_id: Option<String>,
    pub tags: Vec<String>,
    pub freeze_authority: Option<String>,
    pub mint_authority: Option<String>,
}
impl From<CacheToken> for JupApiToken {
    fn from(value: CacheToken) -> Self {
        JupApiToken {
            address: Pubkey::from_str(&value.address).unwrap(),
            decimals: u8::try_from(value.decimals).unwrap(),
            name: value.name.unwrap_or_default(),
            logo_uri: value.logo_uri,
            symbol: value.symbol.unwrap_or_default(),
            program_id: value.program_id.map(|s| Pubkey::from_str(&s).unwrap()),
            tags: value.tags.into_iter().map(|s| s.parse().unwrap()).collect(),
            freeze_authority: value
                .freeze_authority
                .map(|s| Pubkey::from_str(&s).unwrap()),
            mint_authority: value.mint_authority.map(|s| Pubkey::from_str(&s).unwrap()),
            daily_volume: None,
        }
    }
}

impl PgInstance {
    pub async fn db_insert_token_info(
        &self,
        address: String,
        decimals: u8,
        name: Option<String>,
        logo_uri: Option<String>,
        symbol: Option<String>,
        program_id: Option<String>,
        tags: Vec<String>,
        freeze_authority: Option<String>,
        mint_authority: Option<String>,
    ) -> anyhow::Result<CacheToken> {
        self.fetch_one(
            query_as::<_, CacheToken>(
                r#"
                INSERT INTO token_cache (
                    address,
                    decimals,
                    name,
                    logo_uri,
                    symbol,
                    program_id,
                    tags,
                    freeze_authority,
                    mint_authority
                ) 
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                ON CONFLICT (address) DO UPDATE
                SET 
                    decimals = COALESCE(token_cache.decimals, EXCLUDED.decimals),
                    name = COALESCE(token_cache.name, EXCLUDED.name),
                    logo_uri = COALESCE(token_cache.logo_uri, EXCLUDED.logo_uri),
                    symbol = COALESCE(token_cache.symbol, EXCLUDED.symbol),
                    program_id = COALESCE(token_cache.program_id, EXCLUDED.program_id),
                    tags = COALESCE(token_cache.tags, EXCLUDED.tags),
                    freeze_authority = COALESCE(token_cache.freeze_authority, EXCLUDED.freeze_authority),
                    mint_authority = COALESCE(token_cache.mint_authority, EXCLUDED.mint_authority)
                RETURNING *
                "#,
            )
            .bind(address)
            .bind(decimals as i16)
            .bind(name)
            .bind(logo_uri)
            .bind(symbol)
            .bind(program_id)
            .bind(tags)
            .bind(freeze_authority)
            .bind(mint_authority)
        )
        .await
    }

    pub async fn db_get_token_info(&self, address: String) -> anyhow::Result<Option<JupApiToken>> {
        self.fetch_optional(
            query_as::<_, CacheToken>(r#"SELECT * FROM token_cache WHERE address = $1"#)
                .bind(address),
        )
        .await
    }

    pub async fn db_get_token_decimals(&self, address: String) -> anyhow::Result<Option<u8>> {
        #[derive(FromRow)]
        struct TokenDecimals {
            decimals: i16,
        }
        Ok(self
            .fetch_optional(
                query_as::<_, TokenDecimals>(
                    r#"SELECT decimals FROM token_cache WHERE address = $1"#,
                )
                .bind(address),
            )
            .await?
            .map(|td: TokenDecimals| td.decimals as u8))
    }

    pub async fn db_insert_transaction(
        &self,
        signature: String,
        signer: String,
        transaction: Vec<u8>,
    ) -> anyhow::Result<CacheTransaction> {
        self.fetch_one(
            query_as::<_, CacheTransaction>(
                r#"
                INSERT INTO transaction_cache (
                    signature,
                    signer,
                    transaction
                ) 
                VALUES ($1, $2, $3)
                RETURNING *
                "#,
            )
            .bind(signature)
            .bind(signer)
            .bind(transaction),
        )
        .await
    }

    pub async fn db_get_transaction_by_signature(
        &self,
        signature: String,
    ) -> anyhow::Result<Option<CacheTransaction>> {
        self.fetch_optional(
            query_as::<_, CacheTransaction>(
                r#"SELECT * FROM transaction_cache WHERE signature = $1"#,
            )
            .bind(signature),
        )
        .await
    }

    pub async fn db_get_transaction_by_signer(
        &self,
        signer: String,
    ) -> anyhow::Result<Vec<CacheTransaction>> {
        self.fetch_all(
            query_as::<_, CacheTransaction>(r#"SELECT * FROM transaction_cache WHERE signer = $1"#)
                .bind(signer),
        )
        .await
    }
}

#[async_trait]
impl TokenCache for PgInstance {
    async fn insert_token_info(&self, token: JupApiToken) -> anyhow::Result<()> {
        self.db_insert_token_info(
            token.address.to_string(),
            token.decimals,
            Some(token.name),
            token.logo_uri,
            Some(token.symbol),
            token.program_id.map(|p| p.to_string()),
            token.tags.into_iter().map(|tag| tag.to_string()).collect(),
            token.freeze_authority.map(|p| p.to_string()),
            token.mint_authority.map(|p| p.to_string()),
        )
        .await?;
        Ok(())
    }

    async fn get_token_info(&self, address: String) -> anyhow::Result<Option<JupApiToken>> {
        self.db_get_token_info(address.to_string()).await
    }

    async fn get_token_decimals(&self, address: String) -> anyhow::Result<Option<u8>> {
        self.db_get_token_decimals(address.to_string()).await
    }
}

#[async_trait]
impl TransactionCache for PgInstance {
    async fn insert_transaction(
        &self,
        transaction: &EncodedConfirmedTransactionWithStatusMeta,
    ) -> anyhow::Result<()> {
        let decoded = transaction
            .transaction
            .transaction
            .decode()
            .ok_or(anyhow!("failed to decode transaction"))?;
        let signature = decoded.signatures.get(0).expect("at least one signature");
        let signer = decoded
            .message
            .static_account_keys()
            .get(0)
            .expect("at least one account key");
        let transaction = bincode::serialize(&transaction)?;
        self.db_insert_transaction(signature.to_string(), signer.to_string(), transaction)
            .await?;
        Ok(())
    }

    async fn get_transaction(
        &self,
        signature: &Signature,
    ) -> anyhow::Result<Option<EncodedConfirmedTransactionWithStatusMeta>> {
        let db_transaction = self
            .db_get_transaction_by_signature(signature.to_string())
            .await?;
        let Some(transaction) = db_transaction else {
            return Ok(None);
        };
        Ok(Some(bincode::deserialize(&transaction.transaction)?))
    }

    async fn get_transactions_for_signer(
        &self,
        signer: &Pubkey,
    ) -> anyhow::Result<Vec<EncodedConfirmedTransactionWithStatusMeta>> {
        let transactions = self
            .db_get_transaction_by_signer(signer.to_string())
            .await?;
        Ok(transactions
            .into_iter()
            .filter_map(|transaction| bincode::deserialize(&transaction.transaction).ok())
            .collect())
    }
}
