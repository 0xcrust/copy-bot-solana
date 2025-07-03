use super::poll_slots::poll_slots;
use crate::core::traits::{PollRequest, StreamInterface, TxnStreamConfig};
use crate::core::types::block::IBlock;
use crate::core::types::transaction::ITransaction;
use crate::core::{JoinHandleResult, SlotNotification};

use anyhow::{anyhow, bail, Context};
use futures::{StreamExt, TryStreamExt};
use futures_util::stream::BoxStream;
use log::debug;
use solana_client::nonblocking::pubsub_client::PubsubClient;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_response::SlotUpdate;
use solana_rpc_client_api::config::RpcBlockConfig;
use solana_sdk::{
    commitment_config::{CommitmentConfig, CommitmentLevel},
    pubkey::Pubkey,
    slot_history::Slot,
};
use solana_transaction_status::{TransactionDetails, UiTransactionEncoding};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::{sync::Arc, time::Duration};
use tokio::sync::broadcast::{self, Receiver, Sender};
use tokio::sync::RwLock;
use tokio_stream::wrappers::BroadcastStream;

pub const DEFAULT_NUM_PARALLEL_TASKS: usize = 16;
pub const DEFAULT_RETRY_BLOCKS_DELAY_MS: u64 = 10;
pub const DEFAULT_FINALIZED_DELAY_S: u64 = 2;
pub const BLOCK_CHANNEL_CAPACITY: usize = 200;
pub const DEFAULT_SLOTS_CHANNEL_CAPACITY: usize = 16;
pub const TRANSACTIONS_BROADCAST_SIZE: usize = 1000;

#[derive(Default)]
pub struct RpcBlocksConfigBuilder {
    rpc_client: Option<Arc<RpcClient>>, // todo: Share rpc-url rather than reconnecting
    ws_url: Option<String>,
    commitment_scheme: Option<CommitmentScheme>,
    retry_delay_ms: Option<u64>,
    num_parallel_tasks: Option<usize>,
    slot_notification_receiver: Option<Receiver<SlotNotification>>,
}

impl RpcBlocksConfigBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn with_rpc_url(self, url: String) -> Self {
        Self {
            rpc_client: Some(Arc::new(RpcClient::new(url))),
            ..self
        }
    }

    pub fn with_ws_url(self, url: String) -> Self {
        Self {
            ws_url: Some(url),
            ..self
        }
    }

    pub fn with_shared_rpc_client(self, rpc_client: Arc<RpcClient>) -> Self {
        Self {
            rpc_client: Some(rpc_client),
            ..self
        }
    }

    pub fn confirmed_commitment_only(self) -> Self {
        Self {
            commitment_scheme: Some(CommitmentScheme::ConfirmedOnly),
            ..self
        }
    }

    pub fn finalized_commitment_only(self) -> Self {
        Self {
            commitment_scheme: Some(CommitmentScheme::FinalizedOnly),
            ..self
        }
    }

    pub fn both_confirmed_and_finalized(self) -> Self {
        Self {
            commitment_scheme: Some(CommitmentScheme::Both),
            ..self
        }
    }

    pub fn with_block_retry_delay_ms(self, delay: u64) -> Self {
        Self {
            retry_delay_ms: Some(delay),
            ..self
        }
    }

    pub fn with_num_parallel_tasks(self, num: usize) -> Self {
        Self {
            num_parallel_tasks: Some(num),
            ..self
        }
    }

    pub fn with_slot_notification_receiver(self, receiver: Receiver<SlotNotification>) -> Self {
        Self {
            slot_notification_receiver: Some(receiver),
            ..self
        }
    }

    pub fn build(self) -> anyhow::Result<RpcBlocksConfig> {
        Ok(RpcBlocksConfig {
            rpc_client: self
                .rpc_client
                .ok_or(anyhow!("rpc client not set for builder"))?,
            ws_url: self.ws_url.ok_or(anyhow!("ws url not set for builder"))?,
            commitment_scheme: self.commitment_scheme,
            retry_delay_ms: self.retry_delay_ms,
            num_parallel_tasks: self
                .num_parallel_tasks
                .unwrap_or(DEFAULT_NUM_PARALLEL_TASKS),
            slot_notification_receiver: self.slot_notification_receiver,
        })
    }
}

pub struct RpcBlocksConfig {
    rpc_client: Arc<RpcClient>,
    ws_url: String,
    commitment_scheme: Option<CommitmentScheme>,
    retry_delay_ms: Option<u64>,
    num_parallel_tasks: usize,
    slot_notification_receiver: Option<Receiver<SlotNotification>>,
}

impl TxnStreamConfig for RpcBlocksConfig {
    type Item = Result<ITransaction, anyhow::Error>;

    type Handle = RpcBlocksHandle<ITransaction>;

    fn poll_transactions(self) -> Self::Handle {
        self._poll_transactions()
    }
}

#[derive(Clone)]
pub struct RpcBlocksHandle<T>(Arc<RpcBlocksHandleInner<T>>);
struct RpcBlocksHandleInner<T> {
    exit_signal: Arc<AtomicBool>,
    receiver: Receiver<T>,
    tasks: RwLock<Option<Vec<JoinHandleResult<anyhow::Error>>>>,
}

impl<T: Clone + Send + Sync + 'static> StreamInterface for RpcBlocksHandle<T> {
    type Item = Result<T, anyhow::Error>;

    fn subscribe(&self) -> BoxStream<'_, Self::Item> {
        BroadcastStream::new(self.0.receiver.resubscribe())
            .map_err(|e| e.into())
            .boxed()
    }

    fn watch_accounts(&self, _requests: Vec<PollRequest>) -> anyhow::Result<()> {
        Ok(())
    }

    fn drop_accounts(&self, _requests: Vec<Pubkey>) -> anyhow::Result<()> {
        Ok(())
    }

    fn notify_shutdown(&self) -> anyhow::Result<()> {
        self.0.exit_signal.store(true, Ordering::Relaxed);
        Ok(())
    }
}

impl RpcBlocksConfig {
    pub fn new_from_endpoints(url: String, ws_url: String) -> Self {
        RpcBlocksConfig {
            rpc_client: Arc::new(RpcClient::new(url)),
            ws_url,
            commitment_scheme: Some(CommitmentScheme::ConfirmedOnly),
            retry_delay_ms: Some(DEFAULT_RETRY_BLOCKS_DELAY_MS),
            num_parallel_tasks: DEFAULT_NUM_PARALLEL_TASKS,
            slot_notification_receiver: None,
        }
    }

    pub fn new_from_rpc_client(rpc_client: Arc<RpcClient>, ws_url: String) -> Self {
        RpcBlocksConfig {
            rpc_client,
            ws_url,
            commitment_scheme: Some(CommitmentScheme::ConfirmedOnly),
            retry_delay_ms: Some(DEFAULT_RETRY_BLOCKS_DELAY_MS),
            num_parallel_tasks: DEFAULT_NUM_PARALLEL_TASKS,
            slot_notification_receiver: None,
        }
    }

    pub fn _poll_transactions(self) -> RpcBlocksHandle<ITransaction> {
        let rpc_client = self.rpc_client;
        let exit_signal = Arc::new(AtomicBool::new(false));
        let (slot_notification_receiver, slot_tasks) = match self.slot_notification_receiver {
            Some(receiver) => (receiver, None),
            None => {
                let (slot_notification_sender, slot_notification_receiver) =
                    broadcast::channel(DEFAULT_SLOTS_CHANNEL_CAPACITY);
                let slot_tasks = poll_slots(
                    Arc::clone(&rpc_client),
                    CommitmentConfig::processed(),
                    slot_notification_sender,
                    Arc::clone(&exit_signal),
                );
                (slot_notification_receiver, Some(slot_tasks))
            }
        };

        let (transactions_tx, transactions_rx) = broadcast::channel(TRANSACTIONS_BROADCAST_SIZE);

        let mut block_tasks = start_block_polling_tasks(
            rpc_client,
            self.ws_url,
            NotificationSender::Transactions(transactions_tx),
            slot_notification_receiver,
            self.num_parallel_tasks,
            self.retry_delay_ms,
            Arc::clone(&exit_signal),
            self.commitment_scheme,
        );
        block_tasks.extend(slot_tasks.unwrap_or_default());

        let inner = RpcBlocksHandleInner {
            exit_signal,
            receiver: transactions_rx,
            tasks: RwLock::new(Some(block_tasks)),
        };
        RpcBlocksHandle(Arc::new(inner))
    }

    pub fn poll_blocks(self) -> RpcBlocksHandle<IBlock> {
        let client = self.rpc_client;
        let exit_signal = Arc::new(AtomicBool::new(false));
        let (slot_notification_receiver, slot_tasks) = match self.slot_notification_receiver {
            Some(receiver) => (receiver, None),
            None => {
                let (slot_notification_sender, slot_notification_receiver) =
                    broadcast::channel(DEFAULT_SLOTS_CHANNEL_CAPACITY);
                let slot_tasks = poll_slots(
                    Arc::clone(&client),
                    CommitmentConfig::processed(),
                    slot_notification_sender,
                    Arc::clone(&exit_signal),
                );
                (slot_notification_receiver, Some(slot_tasks))
            }
        };

        let (blocks_tx, blocks_rx) = broadcast::channel(TRANSACTIONS_BROADCAST_SIZE);

        let mut block_tasks = start_block_polling_tasks(
            client,
            self.ws_url,
            NotificationSender::Blocks(blocks_tx),
            slot_notification_receiver,
            self.num_parallel_tasks,
            self.retry_delay_ms,
            Arc::clone(&exit_signal),
            self.commitment_scheme,
        );
        block_tasks.extend(slot_tasks.unwrap_or_default());

        let inner = RpcBlocksHandleInner {
            exit_signal,
            receiver: blocks_rx,
            tasks: RwLock::new(Some(block_tasks)),
        };
        RpcBlocksHandle(Arc::new(inner))
    }
}

#[derive(Clone)]
pub enum NotificationSender {
    Blocks(Sender<IBlock>),
    Transactions(Sender<ITransaction>),
}

/// Commitment scheme to employ when polling blocks
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CommitmentScheme {
    /// Get blocks/transactions at only the confirmed commitment
    #[default]
    ConfirmedOnly,
    /// Send blocks/transactions at only the finalized commitment
    FinalizedOnly,
    /// Send blocks/transactions at both the confirmed and finalized commitment
    ///
    /// Note that this might mean receiving the same blocks/transactions twice
    Both,
}

impl CommitmentScheme {
    fn should_send_confirmed_blocks(&self) -> bool {
        matches!(
            self,
            CommitmentScheme::ConfirmedOnly | CommitmentScheme::Both
        )
    }

    fn should_send_finalized_blocks(&self) -> bool {
        matches!(
            self,
            CommitmentScheme::FinalizedOnly | CommitmentScheme::Both
        )
    }
}

// Polls blocks at both the `confirmed` and `finalized` commitments
pub fn start_block_polling_tasks(
    rpc_client: Arc<RpcClient>,
    ws_url: String,
    notification_sender: NotificationSender,
    slot_notification: Receiver<SlotNotification>,
    num_parallel_tasks: usize,
    retry_after_ms: Option<u64>,
    exit_signal: Arc<AtomicBool>,
    commitment_scheme: Option<CommitmentScheme>,
) -> Vec<JoinHandleResult<anyhow::Error>> {
    log::info!(
        "starting block-polling tasks. commitment-scheme={:?}",
        commitment_scheme
    );
    let mut tasks = vec![];

    let recent_slot = Arc::<AtomicU64>::default();
    let (slot_retry_queue_sx, mut slot_retry_queue_rx) = tokio::sync::mpsc::unbounded_channel();
    let (block_schedule_queue_sx, block_schedule_queue_rx) =
        tokio::sync::broadcast::channel::<(Slot, CommitmentConfig)>(5_000); // todo: doesn't need a bound
    let commitment_scheme = commitment_scheme.unwrap_or(CommitmentScheme::ConfirmedOnly);

    let (slot_sender, slot_receiver) = async_channel::unbounded();
    let task = tokio::task::spawn({
        async move {
            let pubsub_client = PubsubClient::new(&ws_url).await?;
            let (mut slots, _unsubscribe) = pubsub_client.slot_updates_subscribe().await?;
            while let Some(update) = slots.next().await {
                match update {
                    SlotUpdate::OptimisticConfirmation { slot, timestamp: _ } => {
                        match slot_sender.send(slot).await {
                            Ok(_) => {}
                            Err(e) => {
                                log::error!("Error sending slot: {}", e);
                                log::info!("Exiting slot sending task");
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }

            Ok::<_, anyhow::Error>(())
        }
    });

    for _i in 0..num_parallel_tasks {
        let notification_sender = notification_sender.clone();
        let rpc_client = rpc_client.clone();
        let block_schedule_queue_rx = block_schedule_queue_rx.resubscribe();
        let slot_retry_queue_sx = slot_retry_queue_sx.clone();
        let exit_signal = Arc::clone(&exit_signal);

        let slot_receiver = slot_receiver.clone();
        let task: JoinHandleResult<anyhow::Error> = tokio::spawn(async move {
            while !exit_signal.load(Ordering::Relaxed) {
                let slot = slot_receiver
                    .recv()
                    .await
                    .context("Recv error on block channel")?;
                let commitment_config = CommitmentConfig::confirmed();
                let block = fetch_block(rpc_client.as_ref(), slot, commitment_config).await;
                match block {
                    Some(processed_block) => {
                        if commitment_scheme.should_send_confirmed_blocks() {
                            match notification_sender {
                                NotificationSender::Blocks(ref sender) => {
                                    sender
                                        .send(processed_block)
                                        .map_err(|e| anyhow!("Failed to send block {}", e))?;
                                }
                                NotificationSender::Transactions(ref sender) => {
                                    for transaction in processed_block.transactions.into_iter() {
                                        sender
                                            .send(transaction)
                                            .map_err(|e| anyhow!("Failed to send block {}", e))?;
                                    }
                                }
                            }
                        }

                        if commitment_scheme.should_send_finalized_blocks() {
                            // schedule to get finalized commitment
                            if commitment_config.commitment != CommitmentLevel::Finalized {
                                let retry_at = tokio::time::Instant::now()
                                    .checked_add(Duration::from_secs(DEFAULT_FINALIZED_DELAY_S))
                                    .unwrap();
                                slot_retry_queue_sx
                                    .send(((slot, CommitmentConfig::finalized()), retry_at))
                                    .map_err(|e| {
                                        anyhow!(
                                            "Failed to reschedule fetch of finalized block: {}",
                                            e
                                        )
                                    })?;
                            }
                        }
                    }
                    None => {
                        log::info!("Retrying block for slot {}", slot);
                        let retry_after_ms =
                            retry_after_ms.unwrap_or(DEFAULT_RETRY_BLOCKS_DELAY_MS);
                        let retry_at = tokio::time::Instant::now()
                            .checked_add(Duration::from_millis(retry_after_ms))
                            .unwrap();
                        slot_retry_queue_sx
                            .send(((slot, commitment_config), retry_at))
                            .context("should be able to rescheduled for replay")?;
                    }
                }
            }
            Ok(())
        });
        tasks.push(task);
    }

    //let replay task
    {
        let recent_slot = recent_slot.clone();
        let block_schedule_queue_sx = block_schedule_queue_sx.clone();
        let exit_signal = Arc::clone(&exit_signal);
        let replay_task: JoinHandleResult<anyhow::Error> = tokio::spawn(async move {
            while let Some(((slot, commitment_config), instant)) = slot_retry_queue_rx.recv().await
            {
                if exit_signal.load(Ordering::Relaxed) {
                    break;
                }
                let recent_slot = recent_slot.load(std::sync::atomic::Ordering::Relaxed);
                // if slot is too old ignore
                if recent_slot.saturating_sub(slot) > 128 {
                    // slot too old to retry
                    // most probably its an empty slot
                    continue;
                }

                let now = tokio::time::Instant::now();
                if now < instant {
                    tokio::time::sleep_until(instant).await;
                }
                if block_schedule_queue_sx
                    .send((slot, commitment_config))
                    .is_err()
                {
                    bail!("could not schedule replay for a slot")
                }
            }
            Ok(())
        });
        tasks.push(replay_task)
    }

    //slot poller
    let slot_poller = tokio::spawn(async move {
        log::info!("block listener started");
        let current_slot = rpc_client
            .get_slot()
            .await
            .context("Should get current slot")?;
        recent_slot.store(current_slot, std::sync::atomic::Ordering::Relaxed);
        let mut slot_notification = slot_notification;
        loop {
            let SlotNotification {
                processed_slot,
                estimated_processed_slot,
                ..
            } = slot_notification
                .recv()
                .await
                .context("Should get slot notification")?;

            recent_slot.store(processed_slot, std::sync::atomic::Ordering::Relaxed);
            block_schedule_queue_sx
                .send((processed_slot, CommitmentConfig::confirmed()))
                .context("should be able to schedule message")?;

            // let last_slot = recent_slot.load(std::sync::atomic::Ordering::Relaxed);
            // if last_slot < estimated_processed_slot {
            //     recent_slot.store(
            //         estimated_processed_slot,
            //         std::sync::atomic::Ordering::Relaxed,
            //     );
            //     for slot in last_slot + 1..estimated_processed_slot + 1 {
            //         block_schedule_queue_sx
            //             .send((slot, CommitmentConfig::confirmed()))
            //             .context("Should be able to schedule message")?;
            //     }
            // }
        }
    });
    tasks.push(slot_poller);

    tasks
}

pub async fn fetch_block(
    rpc_client: &RpcClient,
    slot: Slot,
    commitment_config: CommitmentConfig,
) -> Option<IBlock> {
    let block = rpc_client
        .get_block_with_config(
            slot,
            RpcBlockConfig {
                transaction_details: Some(TransactionDetails::Full),
                commitment: Some(commitment_config),
                max_supported_transaction_version: Some(0),
                encoding: Some(UiTransactionEncoding::Base64),
                rewards: Some(true),
            },
        )
        .await;
    if let Err(ref e) = block {
        debug!("Error getting block for slot {}: {}", slot, e);
    }
    block
        .ok()
        .and_then(|block| IBlock::try_from((block, commitment_config, slot)).ok())
}
