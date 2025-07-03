use crate::core::traits::TxnFilter;
use crate::core::types::block::IBlock;
use crate::core::types::transaction::ITransaction;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use futures::stream::StreamExt;
use log::{debug, error, info};
use solana_client::nonblocking::pubsub_client::PubsubClient;
use solana_client::rpc_config::{RpcBlockSubscribeConfig, RpcBlockSubscribeFilter};
use solana_sdk::commitment_config::{CommitmentConfig, CommitmentLevel};
use solana_sdk::pubkey::Pubkey;
use solana_transaction_status::{TransactionDetails, UiTransactionEncoding};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

pub type AccountId = usize;
pub type WsUnsubscribeFunction =
    Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send>;

#[derive(Default)]
pub struct RpcWatcherConfig<TFilter> {
    // Use https://docs.triton.one/project-yellowstone/whirligig-websockets for faster ws updates
    pub ws_url: String,
    pub block_commitment: Option<CommitmentLevel>, // use `processed` for intra-slot updates with whirlgig-websockets
    pub block_response_filter: TFilter,
    pub reconnect_after: Duration,
}
pub struct RpcWatcherHandle {
    main_task: Option<JoinHandle<anyhow::Result<()>>>,
    command_tx: Arc<UnboundedSender<RpcWatcherCommand>>,
    pub transactions_rx: UnboundedReceiver<ITransaction>,
    background_tasks: Arc<DashMap<Pubkey, BackgroundHandle>>,
}

impl RpcWatcherHandle {
    pub async fn wait_for_shutdown(&mut self) -> anyhow::Result<()> {
        match self.command_tx.send(RpcWatcherCommand::Shutdown) {
            Ok(_) => info!("Sent shutdown command to main task"),
            Err(_) => info!("Failed sending shutdown command. Main task already terminated?"),
        }
        for mut entry in self.background_tasks.iter_mut() {
            let (key, background) = entry.pair_mut();
            debug!("Waiting for {key} watching task to shutdown");
            await_task_completion(
                key,
                background
                    .bg_task_handle
                    .take()
                    .expect("Background task has already been terminated"),
            )
            .await?;
        }
        debug!("Waiting for main task shutdown");
        self.main_task
            .take()
            .expect("Main task has already been terminated")
            .await??;
        Ok(())
    }
}

pub enum RpcWatcherCommand {
    StopWatching(Vec<Pubkey>),
    StartWatching(Vec<Pubkey>),
    Shutdown,
}
pub enum RpcWatcherBackgroundCommand {
    Shutdown,
}

pub struct BackgroundHandle {
    bg_task_handle: Option<JoinHandle<anyhow::Result<()>>>,
    bg_command_tx: UnboundedSender<RpcWatcherBackgroundCommand>,
}

impl BackgroundHandle {
    async fn shutdown_background(&self) -> anyhow::Result<()> {
        self.bg_command_tx
            .send(RpcWatcherBackgroundCommand::Shutdown)?;
        Ok(())
    }
}

impl<TFilter> RpcWatcherConfig<TFilter>
where
    TFilter: TxnFilter + Send + 'static,
{
    pub fn new(
        ws_url: String,
        block_commitment: Option<CommitmentLevel>,
        reconnect_after: Duration,
        block_response_filter: TFilter,
    ) -> Self {
        RpcWatcherConfig {
            ws_url,
            block_commitment,
            reconnect_after,
            block_response_filter,
        }
    }

    fn spawn_tasks_for_accounts(
        accounts: Vec<Pubkey>,
        client: Arc<PubsubClient>,
        filter: TFilter,
        transactions_tx: Arc<UnboundedSender<ITransaction>>,
        shutdown_tx: &tokio::sync::broadcast::Sender<()>,
        manager_background_tasks: Arc<DashMap<Pubkey, BackgroundHandle>>,
        block_commitment: Option<CommitmentLevel>,
        reconnect_after: Duration,
    ) {
        for account in accounts {
            let client = Arc::clone(&client);
            let transactions_tx = Arc::clone(&transactions_tx);
            let (background_cmd_tx, background_cmd_rx) = unbounded_channel();

            let handle = tokio::spawn(watch_account_task(
                client,
                account,
                block_commitment,
                filter.clone(),
                transactions_tx,
                background_cmd_rx,
                shutdown_tx.subscribe(),
                reconnect_after,
            ));

            manager_background_tasks.insert(
                account,
                BackgroundHandle {
                    bg_task_handle: Some(handle),
                    bg_command_tx: background_cmd_tx,
                },
            );
        }
    }

    pub async fn watch_for_transactions(
        &self,
        mut init_accounts: Vec<Pubkey>,
    ) -> anyhow::Result<RpcWatcherHandle> {
        info!("Starting block-watching task");
        init_accounts.dedup();
        let (watcher_command_tx, mut watcher_command_rx) = unbounded_channel::<RpcWatcherCommand>();
        let (transactions_tx, transactions_rx) = unbounded_channel();
        let watcher_command_tx = Arc::new(watcher_command_tx);
        let background_tasks = Arc::new(DashMap::new());
        let manager_background_tasks = Arc::clone(&background_tasks);
        let ws_url = self.ws_url.clone();
        let block_commitment = self.block_commitment;
        let reconnect_after = self.reconnect_after;
        let filter = self.block_response_filter.clone();

        let manager_task = tokio::spawn(async move {
            let pubsub_client = Arc::new(PubsubClient::new(&ws_url).await?);
            info!("Manager task: connected to pubsub client successfully");
            let transactions_tx = Arc::new(transactions_tx);
            let (shutdown_tx, _shutdown_rx) = tokio::sync::broadcast::channel::<()>(1);
            let background_tasks = manager_background_tasks;

            Self::spawn_tasks_for_accounts(
                init_accounts,
                Arc::clone(&pubsub_client),
                filter.clone(),
                Arc::clone(&transactions_tx),
                &shutdown_tx,
                Arc::clone(&background_tasks),
                block_commitment,
                reconnect_after,
            );

            loop {
                match watcher_command_rx.recv().await {
                    Some(cmd) => {
                        match cmd {
                            RpcWatcherCommand::Shutdown => {
                                match shutdown_tx.send(()) {
                                    Ok(_) => info!("Broadcast shutdown signal to background tasks successfully"),
                                    Err(_) => info!("Failed sending broadcast shutdown signal. No active receivers")
                                }
                                break;
                            }
                            RpcWatcherCommand::StopWatching(accounts) => {
                                for account in accounts {
                                    match background_tasks.remove(&account) {
                                        None => info!("Received command to shutdown watcher task for account {}, but no task exists for it", account),
                                        Some((_, mut handle)) => {
                                            info!("Shutting down watcher task for account {}", account);
                                            handle.shutdown_background().await?;
                                            await_task_completion(&account, handle.bg_task_handle.take().expect("Background task has already been terminated")).await?;
                                        }
                                    }
                                }
                            }
                            RpcWatcherCommand::StartWatching(accounts) => {
                                Self::spawn_tasks_for_accounts(
                                    accounts,
                                    Arc::clone(&pubsub_client),
                                    filter.clone(),
                                    Arc::clone(&transactions_tx),
                                    &shutdown_tx,
                                    Arc::clone(&background_tasks),
                                    block_commitment,
                                    reconnect_after,
                                );
                            }
                        }
                    }
                    None => {
                        error!("Manager cmd channel closed. Exiting manager task");
                        break;
                    }
                }
            }

            debug!("Exiting RpcWatcher main task");
            Ok(())
        });

        Ok(RpcWatcherHandle {
            main_task: Some(manager_task),
            command_tx: watcher_command_tx,
            transactions_rx,
            background_tasks,
        })
    }
}

/// TODO: Cleaner interface for block-watching. Make a type that provides a unified stream of ws-notifications that fails to
/// process duplicate blocks? All it'd need to do is cache processed block-ids for a particular time-period and not process them.
///
/// Use Rayon parallel iterator for block-filtering
async fn watch_account_task<TFilter>(
    client: Arc<PubsubClient>,
    account: Pubkey,
    commitment_level: Option<CommitmentLevel>,
    block_response_filter: TFilter,
    transactions_tx: Arc<UnboundedSender<ITransaction>>, // Better to batch-send?
    mut bg_command_rx: UnboundedReceiver<RpcWatcherBackgroundCommand>,
    mut shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    reconnect_after: Duration,
) -> anyhow::Result<()>
where
    TFilter: TxnFilter,
{
    let mut connection_retries = 0;
    loop {
        info!(
            "Subscribing to block updates from ws connection. Connection retries: {}",
            connection_retries
        );
        let commitment_config = commitment_level
            .map(|commitment| CommitmentConfig { commitment })
            .unwrap_or(CommitmentConfig::confirmed());
        let (mut block_notifications, block_unsubscribe) = client
            .block_subscribe(
                //RpcBlockSubscribeFilter::All,
                RpcBlockSubscribeFilter::MentionsAccountOrProgram(account.to_string()),
                Some(RpcBlockSubscribeConfig {
                    commitment: Some(commitment_config),
                    encoding: Some(UiTransactionEncoding::JsonParsed),
                    transaction_details: Some(TransactionDetails::Full),
                    show_rewards: Some(false),
                    max_supported_transaction_version: None,
                }),
            )
            .await?;
        info!("Subcription successful. Waiting for notifications");
        let shutdown_notified;
        loop {
            tokio::select! {
                _shutdown_result = shutdown_rx.recv() => {
                    block_unsubscribe().await;
                    shutdown_notified = true;
                    break;
                }
                Some(cmd) = bg_command_rx.recv() => {
                    match cmd {
                        RpcWatcherBackgroundCommand::Shutdown => {
                            block_unsubscribe().await;
                            shutdown_notified = true;
                            break;
                        }
                    }
                }
                Some(block_update) = block_notifications.next() => {
                    info!("Received block notification for slot={}", block_update.context.slot);
                    let block = IBlock::try_from((block_update.value, commitment_config))?;
                    block.transactions.into_iter().for_each(|tx| {
                        info!("Sending transaction = {:?}", tx);
                        match transactions_tx.send(tx) {
                            Ok(_) => {},
                            Err(_) => {
                                error!("Failed sending results. Receiving channel closed?")
                            }
                        }
                    });
                }
            }
        }

        if shutdown_notified {
            break;
        }
        tokio::time::sleep(reconnect_after).await;
        connection_retries += 1;
    }

    info!("Background task reached end");
    Ok(())
}

async fn await_task_completion(
    key: &Pubkey,
    task: JoinHandle<anyhow::Result<()>>,
) -> anyhow::Result<()> {
    debug!("Waiting for task {key} completion");
    match task.await {
        Ok(Err(e)) => info!("Background task for account {} returned error {}", key, e),
        Ok(Ok(())) => info!("Background task for account {} completed successfully", key),
        Err(e) => {
            let message = format!(
                "Background task for account {} failed to complete. Message={}",
                key, e
            );
            error!("{}", message);
            return Err(anyhow::anyhow!(message));
        }
    }
    debug!("{key} task completed!");

    Ok(())
}
