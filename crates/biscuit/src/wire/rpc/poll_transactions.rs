use crate::core::traits::streaming::{PollRequest, StreamInterface, TxnStreamConfig};
use crate::core::types::transaction::ITransaction;
use crate::core::JoinHandleResult;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Error};
use dashmap::DashMap;
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
use tokio::task::JoinHandle;
use tokio_stream::wrappers::BroadcastStream;

const DEFAULT_POLL_TRANSACTIONS_FREQUENCY: Duration = Duration::from_secs(5);
const DEFAULT_RETRY_SIG_FETCH_FREQUENCY: Duration = Duration::from_secs(1);
const CONCURRENT_GET_TRANSACTION_REQUESTS: usize = 20;
const MAX_RETRY_SIGNATURE_ATTEMPTS: u8 = 3;
const DEFAULT_MAX_INITIAL_SIGNATURE_RETRIES: u8 = 3;
const TRANSACTIONS_BROADCAST_SIZE: usize = 2000;

#[derive(Debug)]
enum RpcTransactionsCommand {
    Add(HashSet<PollRequest>),
    Remove(HashSet<Pubkey>),
    Quit,
}

pub struct BackgroundTask {
    pub join_handle: JoinHandle<anyhow::Result<()>>,
    pub exit_signal: Arc<AtomicBool>,
    pub counter: Arc<BackgroundCounter>,
}

#[derive(Clone)] // impl std::fmt::Debug manually
pub struct RpcTransactionsConfig {
    rpc: Arc<RpcClient>,
    commitment_config: Option<CommitmentConfig>,
    poll_transactions_frequency: Option<Duration>,
    max_initial_signature_retries: Option<u8>,
    initial_accounts: Vec<PollRequest>,
} // todo: Add transaction filters?

#[derive(Clone)]
pub struct RpcTransactionsHandle(Arc<RpcTransactionsHandleInner>);

pub(crate) struct RpcTransactionsHandleInner {
    commands_tx: UnboundedSender<RpcTransactionsCommand>,
    transactions_rx: Receiver<ITransaction>,
    task: RwLock<Option<JoinHandleResult<anyhow::Error>>>,
}

// TODO: Add direct, non-broadcast streams for each account we're watching
impl StreamInterface for RpcTransactionsHandle {
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

impl RpcTransactionsHandleInner {
    fn send_watch_accounts(&self, requests: Vec<PollRequest>) -> anyhow::Result<()> {
        let requests = HashSet::from_iter(requests);
        if let Err(e) = self.commands_tx.send(RpcTransactionsCommand::Add(requests)) {
            return Err(anyhow!("failed to send add command to background: {}", e));
        }
        Ok(())
    }

    fn send_drop_accounts(&self, requests: Vec<Pubkey>) -> anyhow::Result<()> {
        let requests = HashSet::from_iter(requests);
        if let Err(e) = self
            .commands_tx
            .send(RpcTransactionsCommand::Remove(requests))
        {
            return Err(anyhow!("failed to send add command to background: {}", e));
        }
        Ok(())
    }

    fn send_terminate(&self) -> anyhow::Result<()> {
        if let Err(e) = self.commands_tx.send(RpcTransactionsCommand::Quit) {
            return Err(anyhow!("failed to send quit command to background: {}", e));
        }
        Ok(())
    }
}

impl TxnStreamConfig for RpcTransactionsConfig {
    type Item = Result<ITransaction, anyhow::Error>;

    type Handle = RpcTransactionsHandle;

    fn poll_transactions(self) -> Self::Handle {
        self._start_poll_transactions()
    }
}

impl RpcTransactionsConfig {
    pub fn new_from_endpoint(url: String) -> Self {
        RpcTransactionsConfig {
            rpc: Arc::new(RpcClient::new(url)),
            commitment_config: None,
            poll_transactions_frequency: None,
            max_initial_signature_retries: None,
            initial_accounts: Vec::new(),
        }
    }

    fn _start_poll_transactions(self) -> RpcTransactionsHandle {
        let client = Arc::clone(&self.rpc);
        let addresses = HashSet::from_iter(self.initial_accounts);
        let (commands_tx, commands_rx) = tokio::sync::mpsc::unbounded_channel();
        let (transactions_tx, transactions_rx) = channel(TRANSACTIONS_BROADCAST_SIZE);

        let main_task = poll_transactions(
            client,
            addresses,
            commands_rx,
            transactions_tx,
            self.commitment_config,
            self.poll_transactions_frequency,
            self.max_initial_signature_retries,
        );
        let inner = RpcTransactionsHandleInner {
            commands_tx,
            transactions_rx: transactions_rx.resubscribe(),
            task: RwLock::new(Some(main_task)),
        };

        RpcTransactionsHandle(Arc::new(inner))
    }
}

#[derive(Default)]
pub struct RpcTransactionsConfigBuilder {
    rpc_url: Option<String>,
    rpc: Option<Arc<RpcClient>>,
    commitment_config: Option<CommitmentConfig>,
    poll_transactions_frequency: Option<Duration>,
    max_initial_signature_retries: Option<u8>,
    initial_accounts: Vec<PollRequest>,
}
impl RpcTransactionsConfigBuilder {
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

    pub fn with_poll_frequency(self, poll_transactions_frequency: Duration) -> Self {
        Self {
            poll_transactions_frequency: Some(poll_transactions_frequency),
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

    /// Returns an error is the endpoint is not set. See [RpcTransactionsBuilder::with_endpoint]
    pub fn build(self) -> anyhow::Result<RpcTransactionsConfig> {
        let rpc = if self.rpc.is_some() {
            self.rpc.unwrap()
        } else {
            let rpc_url = self
                .rpc_url
                .ok_or(anyhow!("None of rpc client/url specified for builder"))?;
            Arc::new(RpcClient::new(rpc_url))
        };

        Ok(RpcTransactionsConfig {
            rpc,
            commitment_config: self.commitment_config,
            poll_transactions_frequency: self.poll_transactions_frequency,
            max_initial_signature_retries: self.max_initial_signature_retries,
            initial_accounts: self.initial_accounts,
        })
    }
}

fn poll_transactions(
    client: Arc<RpcClient>,
    addresses: HashSet<PollRequest>,
    commands_receiver: UnboundedReceiver<RpcTransactionsCommand>,
    transactions_sender: Sender<ITransaction>,
    commitment_config: Option<CommitmentConfig>,
    poll_transactions_frequency: Option<Duration>,
    max_initial_signature_retries: Option<u8>,
) -> JoinHandleResult<anyhow::Error> {
    let accounts_map: Arc<DashMap<Pubkey, BackgroundTask>> =
        Arc::new(DashMap::with_capacity(addresses.len()));

    for request in addresses {
        let client = Arc::clone(&client);
        let exit_signal = Arc::new(AtomicBool::new(false));
        let counter = Arc::new(BackgroundCounter {});
        let key = request.address;
        let commitment_config = request.commitment.or(commitment_config);
        let poll_transactions_frequency = request
            .poll_transactions_frequency
            .or(poll_transactions_frequency);
        let transactions_sender = transactions_sender.clone();

        let task = tokio::task::spawn({
            let exit_signal = Arc::clone(&exit_signal);
            let counter = Arc::clone(&counter);
            async move {
                watch_account_task(
                    client,
                    key,
                    exit_signal,
                    poll_transactions_frequency,
                    max_initial_signature_retries.unwrap_or(DEFAULT_MAX_INITIAL_SIGNATURE_RETRIES),
                    counter,
                    commitment_config,
                    transactions_sender,
                    request.ignore_error_transactions,
                )
                .await
            }
        });

        let background = BackgroundTask {
            join_handle: task,
            exit_signal,
            counter,
        };
        accounts_map.insert(request.address, background);
    }

    let process_commands_task = tokio::spawn({
        let accounts_map = Arc::clone(&accounts_map);
        let mut commands_receiver = commands_receiver;
        async move {
            loop {
                let command = match commands_receiver.recv().await {
                    Some(command) => command,
                    None => {
                        break;
                    }
                };
                match command {
                    RpcTransactionsCommand::Add(requests) => {
                        for request in requests {
                            let client = Arc::clone(&client);
                            let exit_signal = Arc::new(AtomicBool::new(false));
                            let counter = Arc::new(BackgroundCounter {});
                            let key = request.address;
                            let commitment_config = request.commitment.or(commitment_config);
                            let poll_transactions_frequency = request
                                .poll_transactions_frequency
                                .or(poll_transactions_frequency);
                            let transactions_sender = transactions_sender.clone();

                            let task = tokio::task::spawn({
                                let exit_signal = Arc::clone(&exit_signal);
                                let counter = Arc::clone(&counter);
                                async move {
                                    watch_account_task(
                                        client,
                                        key,
                                        exit_signal,
                                        poll_transactions_frequency,
                                        max_initial_signature_retries
                                            .unwrap_or(DEFAULT_MAX_INITIAL_SIGNATURE_RETRIES),
                                        counter,
                                        commitment_config,
                                        transactions_sender,
                                        request.ignore_error_transactions,
                                    )
                                    .await
                                }
                            });

                            // todo! Is this valid? We should only make a switch if there's an update to the config for watching this account
                            let background = BackgroundTask {
                                join_handle: task,
                                exit_signal,
                                counter,
                            };
                            if let Some(prev_task) =
                                accounts_map.insert(request.address, background)
                            {
                                prev_task.exit_signal.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                    RpcTransactionsCommand::Remove(keys) => {
                        for key in keys {
                            if let Some((_, prev_task)) = accounts_map.remove(&key) {
                                prev_task.exit_signal.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                    RpcTransactionsCommand::Quit => {
                        let mut join_handles = Vec::new();
                        let keys = accounts_map.iter().map(|i| *i.key()).collect::<Vec<_>>();
                        for (i, key) in keys.iter().enumerate() {
                            let (key, task) = accounts_map.remove(key).expect("Is valid");
                            task.exit_signal.store(true, Ordering::Relaxed);
                            join_handles.push(task.join_handle);
                        }
                        info!("Waiting for completion of all watch tasks");
                        futures::future::join_all(join_handles).await;
                        info!("Tasks completed!");
                        break;
                    }
                }
            }
            Ok(())
        }
    });

    process_commands_task
}

/// Track metrics for an account task
pub struct BackgroundCounter {}

async fn watch_account_task(
    client: Arc<RpcClient>,
    key: Pubkey,
    exit_signal: Arc<AtomicBool>,
    poll_transactions_frequency: Option<Duration>,
    max_initial_signature_retries: u8,
    _counter: Arc<BackgroundCounter>,
    commitment_config: Option<CommitmentConfig>,
    transactions_sender: Sender<ITransaction>,
    ignore_error_transactions: bool,
) -> anyhow::Result<()> {
    info!("starting account task for key={}", key);
    let poll_transactions_frequency =
        poll_transactions_frequency.unwrap_or(DEFAULT_POLL_TRANSACTIONS_FREQUENCY);
    let mut last_signature = None;
    for i in 0..max_initial_signature_retries {
        let config = GetConfirmedSignaturesForAddress2Config {
            before: None,
            until: None,
            limit: Some(1),
            commitment: Some(CommitmentConfig::confirmed()),
        };
        let signatures = match client
            .get_signatures_for_address_with_config(&key, config)
            .await
        {
            Ok(sigs) => sigs,
            Err(e) => {
                error!(
                    "failed to get initial signature for address {}. attempt={}, error={}",
                    key, i, e
                );
                continue;
            }
        };
        if let Some(sig) = signatures.first() {
            last_signature = Some(Signature::from_str(&sig.signature).unwrap());
        }
    }

    if last_signature.is_none() {
        return Err(anyhow!(
            "initial signature fetch: max retries exceeded for key={},retries={}",
            key,
            max_initial_signature_retries
        ));
    }
    let mut poll_transactions_interval = tokio::time::interval(poll_transactions_frequency);
    let sig_fetch_retry = std::cmp::min(
        poll_transactions_frequency,
        DEFAULT_RETRY_SIG_FETCH_FREQUENCY,
    );
    let mut poll_retry_sig_fetch = tokio::time::interval(sig_fetch_retry);
    let mut retry_signatures: HashMap<(Signature, u64), u8> = HashMap::new();
    let mut last_seen = last_signature.unwrap();

    while !exit_signal.load(Ordering::Relaxed) {
        poll_transactions_interval.tick().await;
        let config = GetConfirmedSignaturesForAddress2Config {
            before: None,
            until: Some(last_seen),
            limit: None,
            commitment: commitment_config,
        };

        // Signatures are ordered from newest to oldest
        let mut signatures = match client
            .get_signatures_for_address_with_config(&key, config)
            .await
        {
            Ok(signatures) => {
                if ignore_error_transactions {
                    signatures
                        .into_iter()
                        .filter_map(|sig| {
                            if sig.err.is_none() {
                                return Some((
                                    Signature::from_str(&sig.signature).unwrap(),
                                    sig.slot,
                                ));
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                } else {
                    signatures
                        .into_iter()
                        .map(|sig| (Signature::from_str(&sig.signature).unwrap(), sig.slot))
                        .collect::<Vec<_>>()
                }
            }
            Err(e) => {
                error!("error getting signatures for address. error={}", e);
                poll_retry_sig_fetch.tick().await;
                continue;
            }
        };
        if signatures.is_empty() {
            continue;
        }
        last_seen = signatures.first().expect("not empty").0;

        retry_signatures.retain(|_, i| *i <= MAX_RETRY_SIGNATURE_ATTEMPTS);
        signatures.extend(retry_signatures.keys().copied());
        // Deduplicate signatures. Naive attempt at a fix
        // let signatures = Vec::from_iter(signatures.into_iter().collect::<HashSet<_>>());

        let config = RpcTransactionConfig {
            encoding: Some(UiTransactionEncoding::Base58),
            commitment: Some(CommitmentConfig::confirmed()),
            max_supported_transaction_version: Some(0),
        };
        // Processing the signatures in reverse order. RPC orders transactions from newest to
        // oldest. We provide to subscribers in chronological order.
        let transactions = futures::stream::iter(signatures.into_iter().rev())
            .map(|(sig, slot)| {
                let client = Arc::clone(&client);
                let raw_signature = sig.to_string();
                async move {
                    match client.get_transaction_with_config(&sig, config).await {
                        Ok(tx) => {
                            let mut tx = ITransaction::try_from(tx)
                                .map_err(|e| ((sig, slot), anyhow!(e)))?;
                            tx.load_keys(&client)
                                .await
                                .map_err(|e| ((sig, slot), anyhow!(e)))?;
                            Ok(tx)
                        }
                        Err(e) => Err((
                            (sig, slot),
                            anyhow!("Error getting tx for signature {}: {}", raw_signature, e),
                        )),
                    }
                }
            })
            .buffered(CONCURRENT_GET_TRANSACTION_REQUESTS)
            .collect::<Vec<_>>()
            .await;

        for result in transactions {
            match result {
                Ok(transaction) => {
                    if let Err(err) = transactions_sender.send(transaction) {
                        error!(
                            "Failed to send transaction from watch-account task: {}",
                            err
                        );
                    }
                }
                Err(((sig, slot), error)) => {
                    retry_signatures
                        .entry((sig, slot))
                        .and_modify(|attempts| *attempts += 1)
                        .or_insert(1);
                }
            }
        }
    }
    info!("Exiting task for key={}", key);

    Ok(())
}
