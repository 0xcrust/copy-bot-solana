use crate::core::traits::streaming::{PollRequest, StreamInterface, TxnStreamConfig};
use crate::core::types::transaction::ITransaction;
use crate::core::JoinHandleResult;
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Error};
use futures::{StreamExt, TryStreamExt};
use futures_util::stream::BoxStream;
use log::{error, info};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_transaction_status::UiTransactionEncoding;
use tokio::sync::broadcast::{channel, Receiver, Sender};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::RwLock;
use tokio_stream::wrappers::BroadcastStream;

const DEFAULT_POLL_TRANSACTIONS_FREQUENCY: Duration = Duration::from_secs(5);
const DEFAULT_RETRY_SIG_FETCH_FREQUENCY: Duration = Duration::from_secs(1);
const CONCURRENT_GET_TRANSACTION_REQUESTS: usize = 20;
const MAX_RETRY_SIGNATURE_ATTEMPTS: u8 = 3;
const DEFAULT_MAX_INITIAL_SIGNATURE_RETRIES: u8 = 3;
const TRANSACTIONS_BROADCAST_SIZE: usize = 2000;

#[derive(Debug)]
enum HistoricalTxnCommand {
    Add(HashSet<PollRequest>),
    Remove(HashSet<Pubkey>),
    Quit,
}

#[derive(Clone)] // impl std::fmt::Debug manually
pub struct RpcHistoricalTxnsConfig {
    rpc: Arc<RpcClient>,
    commitment_config: Option<CommitmentConfig>,
    n_transactions: usize,
    max_initial_signature_retries: Option<u8>,
    initial_accounts: Vec<PollRequest>,
    ignore_error_transactions: bool,
} // todo: Add transaction filters?

#[derive(Clone)]
pub struct RpcHistoricalTxnsHandle(Arc<RpcHistoricalTxnsHandleInner>);

pub(crate) struct RpcHistoricalTxnsHandleInner {
    commands_tx: UnboundedSender<HistoricalTxnCommand>,
    transactions_rx: Receiver<ITransaction>,
    task: RwLock<Option<JoinHandleResult<anyhow::Error>>>,
}

// TODO: Add direct, non-broadcast streams for each account we're watching
impl StreamInterface for RpcHistoricalTxnsHandle {
    type Item = Result<ITransaction, Error>;

    fn subscribe(&self) -> BoxStream<'_, Result<ITransaction, Error>> {
        BroadcastStream::new(self.0.transactions_rx.resubscribe())
            .map_err(|e| e.into())
            .boxed()
    }

    fn watch_accounts(&self, requests: Vec<PollRequest>) -> anyhow::Result<()> {
        self.0.send_watch_accounts(requests)
    }

    fn drop_accounts(&self, requests: Vec<Pubkey>) -> anyhow::Result<()> {
        self.0.send_drop_accounts(requests)
    }

    fn notify_shutdown(&self) -> anyhow::Result<()> {
        self.0.send_terminate()
    }
}

impl RpcHistoricalTxnsHandleInner {
    fn send_watch_accounts(&self, requests: Vec<PollRequest>) -> anyhow::Result<()> {
        let requests = HashSet::from_iter(requests);
        if let Err(e) = self.commands_tx.send(HistoricalTxnCommand::Add(requests)) {
            return Err(anyhow!("failed to send add command to background: {}", e));
        }
        Ok(())
    }

    fn send_drop_accounts(&self, requests: Vec<Pubkey>) -> anyhow::Result<()> {
        return Err(anyhow!(
            "Cannot dynamically drop accounts from RpcHistoricalTxns once started"
        ));
    }

    fn send_terminate(&self) -> anyhow::Result<()> {
        if let Err(e) = self.commands_tx.send(HistoricalTxnCommand::Quit) {
            return Err(anyhow!("failed to send quit command to background: {}", e));
        }
        Ok(())
    }
}

impl TxnStreamConfig for RpcHistoricalTxnsConfig {
    type Item = Result<ITransaction, anyhow::Error>;

    type Handle = RpcHistoricalTxnsHandle;

    fn poll_transactions(self) -> Self::Handle {
        self._start_poll_transactions()
    }
}

impl RpcHistoricalTxnsConfig {
    pub fn new(url: String, n: usize, ignore_error_transactions: bool) -> Self {
        RpcHistoricalTxnsConfig {
            rpc: Arc::new(RpcClient::new(url)),
            commitment_config: None,
            n_transactions: n,
            max_initial_signature_retries: None,
            initial_accounts: Vec::new(),
            ignore_error_transactions,
        }
    }

    fn _start_poll_transactions(self) -> RpcHistoricalTxnsHandle {
        let client = Arc::clone(&self.rpc);
        let addresses = HashSet::from_iter(self.initial_accounts);
        let (commands_tx, commands_rx) = tokio::sync::mpsc::unbounded_channel();
        let (transactions_tx, transactions_rx) = channel(TRANSACTIONS_BROADCAST_SIZE);

        let main_task = poll_transactions(
            client,
            addresses,
            self.n_transactions,
            commands_rx,
            transactions_tx,
            self.commitment_config,
            self.max_initial_signature_retries,
            self.ignore_error_transactions,
        );
        let inner = RpcHistoricalTxnsHandleInner {
            commands_tx,
            transactions_rx: transactions_rx.resubscribe(),
            task: RwLock::new(Some(main_task)),
        };

        RpcHistoricalTxnsHandle(Arc::new(inner))
    }
}

#[derive(Default)]
pub struct RpcHistoricalTxnsConfigBuilder {
    rpc_url: Option<String>,
    rpc: Option<Arc<RpcClient>>,
    commitment_config: Option<CommitmentConfig>,
    n_transactions: Option<usize>,
    ignore_error_transactions: Option<bool>,
    max_initial_signature_retries: Option<u8>,
    initial_accounts: Vec<PollRequest>,
}
impl RpcHistoricalTxnsConfigBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn with_endpoint(self, url: String) -> Self {
        Self {
            rpc_url: Some(url),
            ..self
        }
    }

    pub fn with_commitment(self, commitment: CommitmentConfig) -> Self {
        Self {
            commitment_config: Some(commitment),
            ..self
        }
    }

    pub fn with_shared_rpc_client(self, rpc: Arc<RpcClient>) -> Self {
        Self {
            rpc: Some(rpc),
            ..self
        }
    }

    pub fn with_n_transactions(self, n_transactions: usize) -> Self {
        Self {
            n_transactions: Some(n_transactions),
            ..self
        }
    }

    pub fn ignore_error_transactions(self, ignore_error_transactions: bool) -> Self {
        Self {
            ignore_error_transactions: Some(ignore_error_transactions),
            ..self
        }
    }

    pub fn with_max_initial_signature_retries(self, max_initial_signature_retries: u8) -> Self {
        Self {
            max_initial_signature_retries: Some(max_initial_signature_retries),
            ..self
        }
    }

    pub fn with_accounts(mut self, accounts: Vec<impl Into<PollRequest>>) -> Self {
        let extend = accounts.into_iter().map(Into::into);
        self.initial_accounts.extend(extend);
        self
    }

    pub fn with_account(mut self, account: impl Into<PollRequest>) -> Self {
        self.initial_accounts.push(account.into());
        self
    }

    /// Returns an error is the endpoint is not set. See [RpcHistoricalTxnsBuilder::with_endpoint]
    pub fn build(self) -> anyhow::Result<RpcHistoricalTxnsConfig> {
        let rpc = if self.rpc.is_some() {
            self.rpc.unwrap()
        } else {
            let rpc_url = self
                .rpc_url
                .ok_or(anyhow!("None of rpc client/url specified for builder"))?;
            Arc::new(RpcClient::new(rpc_url))
        };

        Ok(RpcHistoricalTxnsConfig {
            rpc,
            commitment_config: self.commitment_config,
            n_transactions: self.n_transactions.unwrap_or(1000),
            max_initial_signature_retries: self.max_initial_signature_retries,
            ignore_error_transactions: self.ignore_error_transactions.unwrap_or(false),
            initial_accounts: self.initial_accounts,
        })
    }
}

fn poll_transactions(
    client: Arc<RpcClient>,
    addresses: HashSet<PollRequest>,
    n_transactions: usize,
    commands_receiver: UnboundedReceiver<HistoricalTxnCommand>,
    transactions_sender: Sender<ITransaction>,
    commitment_config: Option<CommitmentConfig>,
    max_initial_signature_retries: Option<u8>,
    ignore_error_transactions: bool,
) -> JoinHandleResult<anyhow::Error> {
    let exit_signal = Arc::new(AtomicBool::new(false));

    for request in addresses {
        let client = Arc::clone(&client);
        let transactions_sender = transactions_sender.clone();

        let task = tokio::task::spawn({
            let exit_signal = Arc::clone(&exit_signal);
            async move {
                let transactions = get_last_n_historical_transactions(
                    client,
                    request.commitment.or(commitment_config),
                    request.address,
                    request.recent_signature,
                    n_transactions,
                    request.order_by_earliest,
                    max_initial_signature_retries.unwrap_or(DEFAULT_MAX_INITIAL_SIGNATURE_RETRIES),
                    ignore_error_transactions,
                    exit_signal,
                )
                .await?;
                for transaction in transactions {
                    if let Err(e) = transactions_sender.send(transaction) {
                        error!(
                            "Failed sending txn from historical transactions task: {}",
                            e
                        );
                    }
                }
                Ok::<_, anyhow::Error>(())
            }
        });
    }

    let process_commands_task = tokio::spawn({
        let mut commands_receiver = commands_receiver;
        let exit_signal = Arc::clone(&exit_signal);
        let transactions_sender = transactions_sender.clone();
        async move {
            loop {
                let command = match commands_receiver.recv().await {
                    Some(command) => command,
                    None => {
                        break;
                    }
                };
                match command {
                    HistoricalTxnCommand::Add(requests) => {
                        for request in requests {
                            let client = Arc::clone(&client);
                            let transactions_sender = transactions_sender.clone();

                            let task = tokio::task::spawn({
                                let exit_signal = Arc::clone(&exit_signal);
                                async move {
                                    let transactions = get_last_n_historical_transactions(
                                        client,
                                        request.commitment.or(commitment_config),
                                        request.address,
                                        request.recent_signature,
                                        n_transactions,
                                        request.order_by_earliest,
                                        max_initial_signature_retries
                                            .unwrap_or(DEFAULT_MAX_INITIAL_SIGNATURE_RETRIES),
                                        ignore_error_transactions,
                                        exit_signal,
                                    )
                                    .await?;
                                    for transaction in transactions {
                                        if let Err(e) = transactions_sender.send(transaction) {
                                            error!(
                                                "Failed sending txn from historical transactions task: {}",
                                                e
                                            );
                                        }
                                    }
                                    Ok::<_, anyhow::Error>(())
                                }
                            });
                        }
                    }
                    HistoricalTxnCommand::Remove(keys) => {}
                    HistoricalTxnCommand::Quit => {
                        exit_signal.store(true, Ordering::Relaxed);
                        info!("Sent exit signal to historical txn background tasks");
                        break;
                    }
                }
            }
            Ok(())
        }
    });

    process_commands_task
}

/// Get the last n historical transactions for a wallet. If `recent-signature` is not provided, it defaults to the
/// latest transaction sent by the wallet and starts its search backwards from that signature.
///
/// If `order_by_earliest` is not provided, it defaults to `true` and returns results with the earliest transaction
/// coming first
async fn get_last_n_historical_transactions(
    client: Arc<RpcClient>,
    commitment_config: Option<CommitmentConfig>,
    wallet: Pubkey,
    recent_signature: Option<Signature>,
    n: usize,
    order_by_earliest: Option<bool>,
    max_initial_signature_retries: u8,
    ignore_error_transactions: bool,
    exit_signal: Arc<AtomicBool>,
) -> anyhow::Result<Vec<ITransaction>> {
    info!("Getting historical transactions for wallet={}", wallet);
    let mut recent_signature = recent_signature;
    if recent_signature.is_none() {
        for i in 0..max_initial_signature_retries {
            let config = GetConfirmedSignaturesForAddress2Config {
                before: None,
                until: None,
                limit: Some(1),
                commitment: Some(CommitmentConfig::confirmed()),
            };
            let signatures = match client
                .get_signatures_for_address_with_config(&wallet, config)
                .await
            {
                Ok(sigs) => sigs,
                Err(e) => {
                    error!(
                        "failed to get initial signature for address {}. attempt={}, error={}",
                        wallet, i, e
                    );
                    continue;
                }
            };
            if let Some(sig) = signatures.first() {
                recent_signature = Some(Signature::from_str(&sig.signature).unwrap());
            }
        }
    }
    if recent_signature.is_none() {
        return Err(anyhow!(
            "Failed to get initial signature for wallet={},retries={}",
            wallet,
            max_initial_signature_retries
        ));
    }

    let mut signatures_left = n;
    let mut last_signature = recent_signature.unwrap();
    let mut signatures = Vec::with_capacity(n); // this is from newest to oldest
    while signatures_left > 0 && !exit_signal.load(Ordering::Relaxed) {
        let signatures_to_query = std::cmp::min(signatures_left, 1000);
        signatures_left -= signatures_to_query;

        let config = GetConfirmedSignaturesForAddress2Config {
            before: Some(last_signature),
            until: None,
            limit: Some(signatures_to_query),
            commitment: commitment_config,
        };

        // Signatures are ordered from newest to oldest
        let results = match client
            .get_signatures_for_address_with_config(&wallet, config)
            .await
        {
            Ok(signatures) => signatures
                .into_iter()
                .filter_map(|sig| {
                    if ignore_error_transactions && sig.err.is_some() {
                        None
                    } else {
                        Some((sig.signature, sig.slot))
                    }
                })
                .collect::<Vec<_>>(),
            Err(e) => {
                error!(
                    "Error getting historical signatures for address. error={}",
                    e
                );
                vec![]
            }
        };
        info!("got {} signatures", results.len());
        if let Some(sig) = results
            .last()
            .and_then(|(sig, _)| Signature::from_str(&sig).ok())
        {
            last_signature = sig;
        }
        signatures.extend(results);
    }

    // Now we have all the signatures, ordered from earliest to latest(right to left in the timeline)
    let config = RpcTransactionConfig {
        encoding: Some(UiTransactionEncoding::Base58),
        commitment: Some(CommitmentConfig::confirmed()),
        max_supported_transaction_version: Some(0),
    };
    let order_by_earliest = order_by_earliest.unwrap_or(true);
    let sig_iterator = if order_by_earliest {
        itertools::Either::Left(signatures.into_iter())
    } else {
        itertools::Either::Right(signatures.into_iter().rev())
    };

    let transactions = futures::stream::iter(sig_iterator)
        .map(|(sig, slot)| {
            let client = Arc::clone(&client);
            async move {
                let signature = Signature::from_str(&sig)?;
                let tx = client
                    .get_transaction_with_config(&signature, config)
                    .await?;
                let mut tx = ITransaction::try_from(tx)?;
                tx.load_keys(&client).await?;
                Ok::<_, anyhow::Error>(tx)
            }
        })
        .buffered(CONCURRENT_GET_TRANSACTION_REQUESTS)
        .filter_map(|tx| async move { tx.ok() })
        .collect::<Vec<_>>()
        .await;

    Ok(transactions)
}
