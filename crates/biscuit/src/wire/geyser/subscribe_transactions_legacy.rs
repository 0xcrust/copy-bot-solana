use crate::core::types::transaction::ITransaction;
use std::time::Duration;

use futures::stream::StreamExt;
use log::{debug, error, info};
use rand::distributions::Alphanumeric;
use rand::Rng;
use solana_sdk::commitment_config::CommitmentLevel;
use solana_sdk::pubkey::Pubkey;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use yellowstone_grpc_client::GeyserGrpcClient;
use yellowstone_grpc_proto::geyser::{
    subscribe_update::UpdateOneof, CommitmentLevel as YellowStoneCommitmentLevel, SubscribeRequest,
    SubscribeRequestFilterTransactions, SubscribeUpdateTransaction,
};

// TODO: Get Geyser working: https://github.com/rpcpool/yellowstone-grpc/issues/131

#[derive(Clone, Debug)]
pub struct GeyserWatcherConfig {
    pub endpoint_url: String,
    pub x_token: Option<String>,
    /// HTTP timeout
    pub connect_timeout: Duration,
    /// GRPC timeout
    pub timeout: Duration,
    pub include_failed_transactions: bool,
    pub commitment_level: CommitmentLevel,
    pub reconnect_after: Duration,
}

pub struct GeyserWatcherHandle {
    pub task: Option<JoinHandle<anyhow::Result<()>>>,
    pub results_rx: UnboundedReceiver<ITransaction>,
    pub shutdown_signal: UnboundedSender<()>,
}

impl GeyserWatcherHandle {
    pub async fn wait_for_shutdown(&mut self) -> anyhow::Result<()> {
        match self.shutdown_signal.send(()) {
            Ok(_) => info!("Sent shutdown signal to transactions task"),
            Err(_) => info!("Failed sending shutdown signal"),
        }
        self.task
            .take()
            .expect("Task has already been terminated")
            .await??;
        Ok(())
    }
}

impl GeyserWatcherConfig {
    pub fn new(
        endpoint_url: String,
        x_token: Option<String>,
        connect_timeout: Duration,
        timeout: Duration,
        include_failed_transactions: Option<bool>,
        commitment_level: CommitmentLevel,
        reconnect_after: Duration,
    ) -> Self {
        GeyserWatcherConfig {
            endpoint_url,
            x_token,
            connect_timeout,
            timeout,
            include_failed_transactions: include_failed_transactions.unwrap_or(false),
            commitment_level,
            reconnect_after,
        }
    }

    pub async fn watch_for_transactions(&self, init_accounts: Vec<Pubkey>) -> GeyserWatcherHandle {
        let config = self.clone();
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        let (results_tx, results_rx) = tokio::sync::mpsc::unbounded_channel::<ITransaction>();

        let task_handle = tokio::task::spawn(async move {
            let mut retries = 0;
            loop {
                // Retry mechanism
                let mut client =
                    match GeyserGrpcClient::build_from_shared(config.endpoint_url.clone())
                        .expect("Invalid endpoint-url from shared bytes")
                        .x_token(config.x_token.clone())
                        .expect("Invalid x-token")
                        .connect_timeout(config.connect_timeout)
                        .timeout(config.timeout)
                        .connect()
                        .await
                    {
                        Ok(client) => client,
                        Err(e) => {
                            retries += 1;
                            info!(
                                "Failed creating client with error={}. Retrying.. {}",
                                e, retries
                            );
                            continue;
                        }
                    };
                let (request_tx, mut subscription_stream) = client.subscribe().await?;
                let accounts = init_accounts
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>();

                let commitment = match config.commitment_level {
                    CommitmentLevel::Confirmed => YellowStoneCommitmentLevel::Confirmed,
                    CommitmentLevel::Finalized => YellowStoneCommitmentLevel::Finalized,
                    CommitmentLevel::Processed => YellowStoneCommitmentLevel::Processed,
                    _ => YellowStoneCommitmentLevel::Finalized,
                };

                let subscribe_request = SubscribeRequest {
                    commitment: Some(commitment.into()),
                    transactions: [(
                        generate_random_string(20),
                        SubscribeRequestFilterTransactions {
                            vote: Some(false),
                            failed: Some(config.include_failed_transactions),
                            signature: None,
                            account_include: accounts.clone(),
                            account_exclude: vec![],
                            account_required: accounts,
                        },
                    )]
                    .into(),
                    ..Default::default()
                };

                let transaction: SubscribeUpdateTransaction;

                let mut shutdown_notified = false;
                loop {
                    tokio::select! {
                        _shutdown = shutdown_rx.recv() => {
                            // TODO: Any way to gracefully shutdown geyser client connection?
                            info!("Received shutdown notification in main grpc watcher task");
                            shutdown_notified = true;
                            break;
                        }
                        Some(item) = subscription_stream.next() => {
                            match item {
                                Ok(update) => {
                                    match update.update_oneof {
                                        Some(UpdateOneof::Transaction(transaction)) => {
                                            let decoded = ITransaction::try_from(transaction)?;
                                            match results_tx.send(decoded) {
                                                Ok(_) => {},
                                                Err(_) => error!("Failed sending results. Receiving channel closed?"),
                                            }
                                        }
                                        Some(UpdateOneof::Ping(ping)) => debug!("Received ping"),
                                        Some(UpdateOneof::Pong(pong)) => debug!("Received pong. Id = {}", pong.id),
                                        _ => info!("Received unspecified message from transactions subscription stream")
                                    }
                                }
                                Err(e) => {
                                    error!("Received error from transactions subscription stream. Reconnecting to GRPC client");
                                    break;
                                }
                            }
                        }
                    }
                }

                if shutdown_notified {
                    info!("Shutting down geyser watcher task");
                    break;
                }
                tokio::time::sleep(config.reconnect_after).await;
            }

            Ok(())
        });

        GeyserWatcherHandle {
            task: Some(task_handle),
            results_rx,
            shutdown_signal: shutdown_tx,
        }
    }
}

fn generate_random_string(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}
