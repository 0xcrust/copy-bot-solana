pub mod streaming;
pub mod transaction_cache;
pub mod txn_extensions;
pub mod txn_filters;

pub use streaming::*;
pub use transaction_cache::*;
pub use txn_extensions::*;
pub use txn_filters::*;

use solana_sdk::signers::Signers;

pub trait IndexedSigners {
    type T: Signers + ?Sized;

    fn get_signers_for_index(&self, idx: usize) -> &Self::T;
}
