use crate::core::traits::streaming::StreamFilter;
use crate::core::types::transaction::ITransaction;
use solana_sdk::pubkey::Pubkey;

// Move away from generic filters and use something like yellowstone-grpc's:
//
// #[derive(Debug, Clone)]
// pub struct Filter {
//     accounts: FilterAccounts,
//     slots: FilterSlots,
//     transactions: FilterTransactions,
//     transactions_status: FilterTransactions,
//     entry: FilterEntry,
//     blocks: FilterBlocks,
//     blocks_meta: FilterBlocksMeta,
//     commitment: CommitmentLevel,
//     accounts_data_slice: Vec<FilterAccountsDataSlice>,
//     ping: Option<i32>,
// }

pub trait TxnFilter: Clone {
    fn description(&self) -> String {
        "for custom filter".to_string()
    }
    fn is_valid(&self, transaction: &ITransaction) -> bool;
}

impl<T> StreamFilter<ITransaction> for T
where
    T: TxnFilter,
{
    fn is_valid(&self, item: &ITransaction) -> bool {
        self.is_valid(item)
    }
}

impl<T> StreamFilter<Result<ITransaction, anyhow::Error>> for T
where
    T: TxnFilter,
{
    fn is_valid(&self, item: &Result<ITransaction, anyhow::Error>) -> bool {
        item.as_ref()
            .map(|item| self.is_valid(item))
            .unwrap_or(false)
    }
}

#[derive(Clone)]
pub struct IsSigner(pub Pubkey);

impl TxnFilter for IsSigner {
    fn description(&self) -> String {
        format!("for signer={}", self.0)
    }

    fn is_valid(&self, transaction: &ITransaction) -> bool {
        // todo: Confirm that payer always come first
        transaction.message.static_account_keys()[0] == self.0
    }
}

#[derive(Clone)]
pub struct MentionsAccount(pub Pubkey);
impl TxnFilter for MentionsAccount {
    fn description(&self) -> String {
        format!("for account={}", self.0)
    }
    fn is_valid(&self, transaction: &ITransaction) -> bool {
        transaction.account_keys().iter().any(|x| *x == self.0)
    }
}

/*impl for <'a> FnMut(&'a ITransaction) -> bool for MentionsAccount {

}*/

#[derive(Clone)]
pub struct MentionsAccounts(pub Vec<Pubkey>); // todo: Make this faster
impl TxnFilter for MentionsAccounts {
    fn description(&self) -> String {
        format!("mentioning accounts={:?}", self.0)
    }
    fn is_valid(&self, transaction: &ITransaction) -> bool {
        let mut valid = true;
        for account in &self.0 {
            if !transaction.account_keys().iter().any(|x| *x == *account) {
                valid = false;
                break;
            }
        }
        valid
    }
}

#[derive(Clone)]
pub struct MentionsOneOf(pub Vec<Pubkey>);
impl TxnFilter for MentionsOneOf {
    fn description(&self) -> String {
        format!("mentioning one of={:?}", self.0)
    }
    fn is_valid(&self, transaction: &ITransaction) -> bool {
        let mut valid = false;
        for account in &self.0 {
            if transaction.account_keys().iter().any(|x| *x == *account) {
                valid = true;
                break;
            }
        }
        valid
    }
}

#[derive(Clone)]
pub struct Noop;
impl TxnFilter for Noop {
    fn description(&self) -> String {
        "allowing all transactions".to_string()
    }
    fn is_valid(&self, _transaction: &ITransaction) -> bool {
        true
    }
}

impl<T> TxnFilter for T
where
    T: Fn(&ITransaction) -> bool + Clone,
{
    fn is_valid(&self, tx: &ITransaction) -> bool {
        (self)(tx)
    }
}

#[cfg(test)]
mod generics_test {
    use super::{ITransaction, TxnFilter};

    fn accepts_filter<T: TxnFilter>(filter: T) {}

    fn check() {
        accepts_filter(|i: &ITransaction| i.err.is_none());
    }
}
