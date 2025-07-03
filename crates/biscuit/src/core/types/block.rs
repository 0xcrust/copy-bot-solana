use super::transaction::ITransaction;
use crate::core::traits::TransactionExtensions as _;
use std::str::FromStr;

use anyhow::anyhow;
use solana_client::rpc_response::RpcBlockUpdate;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::hash::Hash;
use solana_sdk::slot_history::Slot;
use solana_transaction_status::{RewardType, UiConfirmedBlock};

pub struct IBlockDetails {
    pub parent_slot: u64,
    pub current_slot: u64,
    pub block_height: Option<u64>,
    pub block_time: Option<i64>,
    pub previous_blockhash: String,
    pub commitment: CommitmentConfig,
    pub blockhash: String,
}

#[derive(Clone)]
pub struct IBlock {
    pub transactions: Vec<ITransaction>,
    pub previous_blockhash: Hash,
    pub block_hash: Hash,
    pub block_height: u64,
    pub parent_slot: Slot,
    pub slot: Slot,
    pub block_time: i64,
    pub leader_id: Option<String>,
    pub commitment_config: CommitmentConfig,
}

impl std::fmt::Debug for IBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "IBlock {{ slot: {}, commitment_config: {}, blockhash: {}, transactions_count: {} }}",
            self.slot,
            self.commitment_config.commitment,
            self.block_hash,
            self.transactions.len()
        )
    }
}

impl TryFrom<(RpcBlockUpdate, CommitmentConfig)> for IBlock {
    type Error = anyhow::Error;

    fn try_from(value: (RpcBlockUpdate, CommitmentConfig)) -> Result<Self, Self::Error> {
        let (value, commitment) = value;
        if value.block.is_none() {
            return Err(anyhow!("No value received for block"));
        }
        let block = value.block.unwrap();
        Self::try_from((block, commitment, value.slot))
    }
}

impl TryFrom<(UiConfirmedBlock, CommitmentConfig, u64)> for IBlock {
    type Error = anyhow::Error;

    fn try_from(value: (UiConfirmedBlock, CommitmentConfig, u64)) -> Result<Self, Self::Error> {
        let (block, commitment_config, slot) = value;
        let block_time = block.block_time;

        let signatures = block.signatures;
        let transactions = block
            .transactions
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .filter_map(|(i, tx)| tx.to_unified_transaction(slot, block_time))
            .collect::<Vec<_>>();
        let leader_id = if let Some(rewards) = block.rewards {
            rewards
                .iter()
                .find(|reward| Some(RewardType::Fee) == reward.reward_type)
                .map(|leader_reward| leader_reward.pubkey.clone())
        } else {
            None
        };
        Ok(IBlock {
            transactions,
            slot,
            parent_slot: block.parent_slot,
            previous_blockhash: Hash::from_str(&block.previous_blockhash).unwrap(),
            block_hash: Hash::from_str(&block.blockhash).unwrap(),
            block_height: block.block_height.unwrap(),
            block_time: block.block_time.unwrap(),
            leader_id,
            commitment_config,
        })
    }
}
