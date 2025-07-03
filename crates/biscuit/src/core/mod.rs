pub mod traits;
pub mod types;

use solana_sdk::hash::Hash;
use solana_sdk::slot_history::Slot;

pub type JoinHandleResult<E> = tokio::task::JoinHandle<std::result::Result<(), E>>;

#[derive(Debug, Clone, Default)]
pub struct SlotNotification {
    pub processed_slot: Slot,
    pub estimated_processed_slot: Slot,
}

pub struct BlockHashNotification {
    pub blockhash: Hash,
    pub last_valid_block_height: u64,
}
