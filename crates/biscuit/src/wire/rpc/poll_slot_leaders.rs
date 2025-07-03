use crate::core::{JoinHandleResult, SlotNotification};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use dashmap::DashMap;
use indexmap::IndexMap;
use log::{debug, error};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_rpc_client_api::response::RpcContactInfo;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::slot_history::Slot;
use tokio::sync::broadcast::{error::RecvError, Receiver};
use tokio::sync::RwLock;

const NUM_SLOTS_PER_LEADER: usize = 4;

// See https://docs.solanalabs.com/consensus/leader-rotation

#[derive(Clone)]
pub struct LeaderScheduleHandle {
    current_leaders: Arc<DashMap<Slot, RpcContactInfo>>,
    slot_update: Arc<AtomicU64>,
    tasks: Arc<RwLock<Option<Vec<JoinHandleResult<anyhow::Error>>>>>,
    num_leaders: usize,
}

impl LeaderScheduleHandle {
    pub fn tasks(&self) -> Arc<RwLock<Option<Vec<JoinHandleResult<anyhow::Error>>>>> {
        Arc::clone(&self.tasks)
    }

    /// Returns the next slot leaders in order
    pub fn get_leaders(&self) -> Vec<RpcContactInfo> {
        let start_slot = self.slot_update.load(Ordering::Relaxed);
        let end_slot = start_slot + (self.num_leaders * NUM_SLOTS_PER_LEADER) as u64;
        let mut leaders = IndexMap::new();
        for slot in start_slot..end_slot {
            let leader = self.current_leaders.get(&slot);
            if let Some(leader) = leader {
                //log::info!("Leader for slot {} is {}", slot, leader.pubkey);
                _ = leaders.insert(leader.pubkey.to_owned(), leader.value().to_owned());
            } else {
                log::warn!("Couldn't get leader for slot {}", slot);
            }
            if leaders.len() >= self.num_leaders {
                break;
            }
        }
        // log::info!(
        //     "leaders: {:?}, start_slot: {:?}",
        //     leaders.clone().keys(),
        //     start_slot
        // );
        leaders
            .values()
            .clone()
            .into_iter()
            .map(|v| v.to_owned())
            .collect()
    }
}

pub async fn bootstrap_leader_tracker_tasks(
    rpc_client: Arc<RpcClient>,
    commitment_config: CommitmentConfig,
    slot_notification: Receiver<SlotNotification>,
    leader_offset: i64,
    num_leaders: usize,
) -> anyhow::Result<LeaderScheduleHandle> {
    let slot = rpc_client
        .get_slot_with_commitment(commitment_config)
        .await?;
    let start_slot = get_start_slot_from_offset(slot, leader_offset);
    let slot_update = Arc::new(AtomicU64::new(start_slot));
    let current_leaders_map = Arc::new(DashMap::new());

    get_leaders_for_slot(&rpc_client, &slot_update, &current_leaders_map).await?;
    let handle = start_leader_tracker_tasks(
        rpc_client,
        slot_notification,
        leader_offset,
        num_leaders,
        Some(slot_update),
        Some(current_leaders_map),
    );
    Ok(handle)
}

pub fn start_leader_tracker_tasks(
    rpc_client: Arc<RpcClient>,
    mut slot_notification: Receiver<SlotNotification>,
    leader_offset: i64,
    num_leaders: usize, // number of leaders to forward to
    bootstrap_slot_update: Option<Arc<AtomicU64>>,
    bootstrap_leaders_map: Option<Arc<DashMap<u64, RpcContactInfo>>>,
) -> LeaderScheduleHandle {
    let mut tasks = vec![];
    let slot_update = if let Some(slot_update) = bootstrap_slot_update {
        slot_update
    } else {
        Arc::new(AtomicU64::new(0))
    };
    let current_leaders_map = if let Some(leaders_map) = bootstrap_leaders_map {
        leaders_map
    } else {
        Arc::new(DashMap::new())
    };

    let poll_slot_task = tokio::task::spawn({
        let slot_update = Arc::clone(&slot_update);
        async move {
            loop {
                let next_slot = match slot_notification.recv().await {
                    Ok(slot) => slot.processed_slot,
                    Err(RecvError::Lagged(_)) => slot_update.load(Ordering::Relaxed),
                    Err(RecvError::Closed) => break,
                };
                let start_slot = get_start_slot_from_offset(next_slot, leader_offset);
                if start_slot > slot_update.load(Ordering::Relaxed) {
                    slot_update.store(start_slot, Ordering::Relaxed);
                }
            }

            Ok::<_, anyhow::Error>(())
        }
    });
    tasks.push(poll_slot_task);

    let poll_slot_leaders_task = tokio::task::spawn({
        let slot_update = Arc::clone(&slot_update);
        let rpc_client = Arc::clone(&rpc_client);
        let current_leaders_map = Arc::clone(&current_leaders_map);
        async move {
            loop {
                match get_leaders_for_slot(&rpc_client, &slot_update, &current_leaders_map).await {
                    Ok(_) => tokio::time::sleep(Duration::from_secs(60)).await,
                    Err(e) => {
                        debug!("{}", e);
                        tokio::time::sleep(Duration::from_secs(1)).await
                    }
                }
            }
        }
    });
    tasks.push(poll_slot_leaders_task);

    let handle = LeaderScheduleHandle {
        current_leaders: current_leaders_map,
        slot_update,
        tasks: Arc::new(RwLock::new(Some(tasks))),
        num_leaders,
    };

    handle
}

async fn get_leaders_for_slot(
    rpc_client: &RpcClient,
    slot_update: &AtomicU64,
    current_leaders: &DashMap<Slot, RpcContactInfo>,
) -> Result<(), anyhow::Error> {
    let next_slot = slot_update.load(Ordering::Relaxed);
    debug!("Polling slot leaders for slot {}", next_slot);
    // polling 1000 slots ahead is more than enough
    let slot_leaders = rpc_client
        .get_slot_leaders(next_slot, 1000)
        .await
        .map_err(|e| anyhow!("Error getting slot leaders: {}", e))?;
    let new_cluster_nodes = rpc_client
        .get_cluster_nodes()
        .await
        .map_err(|e| anyhow!("Error getting cluster nodes: {}", e))?;

    let mut cluster_node_map = HashMap::new();
    for node in new_cluster_nodes {
        cluster_node_map.insert(node.pubkey.clone(), node);
    }
    for (i, leader) in slot_leaders.iter().enumerate() {
        let contact_info = cluster_node_map.get(&leader.to_string());
        if let Some(contact_info) = contact_info {
            current_leaders.insert(next_slot + i as u64, contact_info.clone());
        } else {
            debug!("Leader {} not found in cluster nodes", leader);
        }
    }
    clean_up_slot_leaders(slot_update, current_leaders);
    Ok(())
}

fn clean_up_slot_leaders(slot_update: &AtomicU64, current_leaders: &DashMap<Slot, RpcContactInfo>) {
    let cur_slot = slot_update.load(Ordering::Relaxed);
    let mut slots_to_remove = vec![];
    for leaders in current_leaders.iter() {
        if leaders.key().clone() < cur_slot {
            slots_to_remove.push(leaders.key().clone());
        }
    }
    for slot in slots_to_remove {
        current_leaders.remove(&slot);
    }
}

fn get_start_slot_from_offset(next_slot: u64, leader_offset: i64) -> u64 {
    let slot_buffer = leader_offset * (NUM_SLOTS_PER_LEADER as i64);
    let start_slot = if slot_buffer > 0 {
        next_slot + slot_buffer as u64
    } else {
        next_slot - slot_buffer.abs() as u64
    };
    start_slot
}
