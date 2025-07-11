use crate::core::{JoinHandleResult, SlotNotification};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{sync::Arc, time::Duration};

use anyhow::{bail, Context};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, slot_history::Slot};
use tokio::sync::broadcast::Sender;

const AVERAGE_SLOT_CHANGE_TIME: Duration = Duration::from_millis(400);

pub async fn poll_commitment_slots(
    rpc_client: Arc<RpcClient>,
    commitment_config: CommitmentConfig,
    slot_tx: tokio::sync::mpsc::UnboundedSender<Slot>,
    exit_signal: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let mut poll_frequency = tokio::time::interval(Duration::from_millis(50));
    let mut last_slot = 0;
    let mut errors = 0;

    while !exit_signal.load(Ordering::Relaxed) {
        let slot = rpc_client.get_slot_with_commitment(commitment_config).await;
        match slot {
            Ok(slot) => {
                if slot > last_slot {
                    // send
                    slot_tx.send(slot).context("Error sending slot")?;
                    last_slot = slot;
                }
                errors = 0;
            }
            Err(e) => {
                errors += 1;
                if errors > 10 {
                    bail!("Exceeded error count to get slots from rpc {e:?}");
                }
            }
        }
        // wait for next poll i.e at least 50ms
        poll_frequency.tick().await;
    }
    Ok(())
}

pub fn poll_slots(
    rpc_client: Arc<RpcClient>,
    commitment_config: CommitmentConfig,
    sender: Sender<SlotNotification>,
    exit_signal: Arc<AtomicBool>,
) -> Vec<JoinHandleResult<anyhow::Error>> {
    // processed slot update task
    let (slot_update_sx, mut slot_update_rx) = tokio::sync::mpsc::unbounded_channel();
    let exit_signal_clone = Arc::clone(&exit_signal);
    let task1 = tokio::spawn(poll_commitment_slots(
        rpc_client.clone(),
        commitment_config,
        slot_update_sx,
        exit_signal_clone,
    ));
    let task2 = tokio::spawn(async move {
        let slot = rpc_client
            .get_slot_with_commitment(CommitmentConfig::confirmed())
            .await
            .context("Error getting slot")?;

        let mut current_slot = slot;
        let mut estimated_slot = slot;

        while !exit_signal.load(Ordering::Relaxed) {
            match tokio::time::timeout(AVERAGE_SLOT_CHANGE_TIME, slot_update_rx.recv()).await {
                Ok(Some(slot)) => {
                    // slot is latest
                    if slot > current_slot {
                        current_slot = slot;
                        if current_slot > estimated_slot {
                            estimated_slot = slot;
                        }
                        sender
                            .send(SlotNotification {
                                processed_slot: current_slot,
                                estimated_processed_slot: estimated_slot,
                            })
                            .context("Cannot send slot notification")?;
                    }
                }
                Ok(None) => {
                    log::error!("got nothing from slot update notifier. exiting poll_slots task");
                    // exit_signal.store(true, Ordering::Relaxed);
                    break;
                }
                Err(err) => {
                    log::trace!("timeout on receive slot update: {err}");
                    // force update the slot
                    // estimated slot should not go ahead more than 32 slots
                    // this is because it may be a slot block
                    if estimated_slot < current_slot + 32 {
                        estimated_slot += 1;

                        sender
                            .send(SlotNotification {
                                processed_slot: current_slot,
                                estimated_processed_slot: estimated_slot,
                            })
                            .context("Cannot send slot notification")?;
                    }
                }
            }
        }
        Ok(())
    });
    vec![task1, task2]
}
