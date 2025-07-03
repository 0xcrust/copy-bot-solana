use crate::swap::jupiter::token_list_new::JupApiToken;
use anyhow::Context;
use async_trait::async_trait;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_transaction_status::EncodedConfirmedTransactionWithStatusMeta;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct FileTxnCache {
    dir_path: String,
}

impl FileTxnCache {
    pub fn create(path: &str) -> anyhow::Result<Self> {
        Self::make_dir_if_not_exists(&path)?;

        Ok(FileTxnCache {
            dir_path: path.to_string(),
        })
    }

    fn insert(
        &self,
        signature: &Signature,
        transaction: &EncodedConfirmedTransactionWithStatusMeta,
    ) -> anyhow::Result<()> {
        let mut path = PathBuf::from(self.dir_path.clone());
        path.push(signature.to_string());

        std::fs::write(&path, serde_json::to_vec(&transaction)?)?;

        Ok(())
    }

    fn get(
        &self,
        signature: &Signature,
    ) -> anyhow::Result<Option<EncodedConfirmedTransactionWithStatusMeta>> {
        let mut path = PathBuf::from(self.dir_path.clone());
        path.push(signature.to_string());

        if std::fs::metadata(&path).is_err() {
            return Ok(None);
        }

        let tx = serde_json::from_slice(&std::fs::read(&path)?)?;
        Ok(tx)
    }

    fn make_dir_if_not_exists(path: impl AsRef<Path>) -> anyhow::Result<()> {
        match std::fs::create_dir(path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => {
                Err(e)?;
            }
        }

        Ok(())
    }
}

#[async_trait]
impl TransactionCache for FileTxnCache {
    async fn insert_transaction(
        &self,
        transaction: &EncodedConfirmedTransactionWithStatusMeta,
    ) -> anyhow::Result<()> {
        let decoded = transaction
            .transaction
            .transaction
            .decode()
            .context("Failed to decode transaction")?;
        let signature = decoded
            .signatures
            .first()
            .context("Transaction has at least one signature")?;
        self.insert(signature, transaction)?;
        Ok(())
    }

    async fn get_transaction(
        &self,
        signature: &Signature,
    ) -> anyhow::Result<Option<EncodedConfirmedTransactionWithStatusMeta>> {
        Ok(self.get(signature)?)
    }

    async fn get_transactions_for_signer(
        &self,
        signer: &Pubkey,
    ) -> anyhow::Result<Vec<EncodedConfirmedTransactionWithStatusMeta>> {
        Ok(vec![])
    }
}

#[async_trait]
pub trait TokenCache {
    async fn insert_token_info(&self, token: JupApiToken) -> anyhow::Result<()>;

    async fn get_token_info(&self, address: String) -> anyhow::Result<Option<JupApiToken>>;

    async fn get_token_decimals(&self, address: String) -> anyhow::Result<Option<u8>>;
}

#[async_trait]
pub trait TransactionCache {
    async fn insert_transaction(
        &self,
        transaction: &EncodedConfirmedTransactionWithStatusMeta,
    ) -> anyhow::Result<()>;

    async fn get_transaction(
        &self,
        signature: &Signature,
    ) -> anyhow::Result<Option<EncodedConfirmedTransactionWithStatusMeta>>;

    async fn get_transactions_for_signer(
        &self,
        signer: &Pubkey,
    ) -> anyhow::Result<Vec<EncodedConfirmedTransactionWithStatusMeta>>;
}

#[derive(Clone)]
pub struct NoopTransactionCache;

#[async_trait]
impl TransactionCache for NoopTransactionCache {
    async fn insert_transaction(
        &self,
        _transaction: &EncodedConfirmedTransactionWithStatusMeta,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn get_transaction(
        &self,
        _signature: &Signature,
    ) -> anyhow::Result<Option<EncodedConfirmedTransactionWithStatusMeta>> {
        Ok(None)
    }

    async fn get_transactions_for_signer(
        &self,
        _signer: &Pubkey,
    ) -> anyhow::Result<Vec<EncodedConfirmedTransactionWithStatusMeta>> {
        Ok(vec![])
    }
}
