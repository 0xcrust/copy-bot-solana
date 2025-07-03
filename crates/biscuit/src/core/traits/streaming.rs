use std::time::Duration;

use futures::stream::BoxStream;
use futures::StreamExt;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;

pub trait StreamFilter<T>: Clone {
    fn is_valid(&self, item: &T) -> bool;
}

#[derive(Clone)]
pub struct NoopFilter;

impl<T> StreamFilter<T> for NoopFilter {
    fn is_valid(&self, item: &T) -> bool {
        true
    }
}

pub trait StreamInterface: Send + Sync {
    type Item: Send + Sync + 'static;

    fn subscribe(&self) -> BoxStream<'_, Self::Item>;

    fn subscribe_with_filter<Fut, F>(&self, mut f: F) -> BoxStream<'_, Self::Item>
    where
        F: FnMut(&Self::Item) -> bool + Send + 'static,
        Self: Sized,
    {
        self.subscribe()
            .filter(move |item| futures::future::ready(f(item)))
            .boxed()
    }

    fn watch_accounts(&self, requests: Vec<PollRequest>) -> anyhow::Result<()>;

    fn drop_accounts(&self, requests: Vec<Pubkey>) -> anyhow::Result<()>;

    fn notify_shutdown(&self) -> anyhow::Result<()>;
}

pub trait TxnStreamConfig {
    type Item: Send + Sync + 'static;

    type Handle: StreamInterface<Item = Self::Item> + Clone;

    fn poll_transactions(self) -> Self::Handle;
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct PollRequest {
    pub address: Pubkey,
    pub commitment: Option<CommitmentConfig>,
    pub poll_transactions_frequency: Option<Duration>,
    pub ignore_error_transactions: bool,
    pub recent_signature: Option<Signature>,
    pub order_by_earliest: Option<bool>,
}

impl PollRequest {
    pub fn new(
        address: Pubkey,
        commitment: Option<CommitmentConfig>,
        poll_transactions_frequency: Option<Duration>,
        ignore_error_transactions: bool,
        recent_signature: Option<Signature>,
        order_by_earliest: Option<bool>,
    ) -> Self {
        PollRequest {
            address,
            commitment,
            poll_transactions_frequency,
            ignore_error_transactions,
            recent_signature,
            order_by_earliest,
        }
    }
}

impl Default for PollRequest {
    fn default() -> Self {
        PollRequest {
            address: Pubkey::default(),
            commitment: None,
            poll_transactions_frequency: None,
            ignore_error_transactions: true,
            recent_signature: None,
            order_by_earliest: None,
        }
    }
}

// impl<T, F> StreamFilter<T> for F where F: Fn(&T) -> bool + Clone {
//     fn is_valid(&self, item: &T) -> bool {
//         (self)(item)
//     }
// }
