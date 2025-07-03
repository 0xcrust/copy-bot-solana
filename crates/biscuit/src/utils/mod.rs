pub mod fees;
pub mod lookup_tables;
pub mod retry;
pub mod serde_helpers;
pub mod token;
pub mod token_instr;
pub mod transaction;

use anchor_lang::AccountDeserialize;
use anyhow::{format_err, Result};

use solana_sdk::account::Account;
use solana_sdk::account_info::AccountInfo;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;

/// (Pubkey, is_signer, is_writable, Account)
pub fn create_account_infos<'a>(
    // inspired by `create_is_signer_account_infos` in solana-sdk/account.rs
    accounts: &'a mut [(&'a Pubkey, bool, bool, &'a mut Account)],
) -> Vec<AccountInfo<'a>> {
    accounts
        .iter_mut()
        .map(|(key, is_signer, is_writable, account)| {
            AccountInfo::new(
                key,
                *is_signer,
                *is_writable,
                &mut account.lamports,
                &mut account.data,
                &account.owner,
                account.executable,
                account.rent_epoch,
            )
        })
        .collect()
}

pub fn read_keypair_file(s: &str) -> Result<Keypair> {
    solana_sdk::signature::read_keypair_file(s)
        .map_err(|_| format_err!("failed to read keypair from {}", s))
}

pub fn deserialize_anchor_account<T: AccountDeserialize>(account: &Account) -> Result<T> {
    let mut data: &[u8] = &account.data;
    T::try_deserialize(&mut data).map_err(Into::into)
}

pub fn create_dir_if_not_exists(path: impl AsRef<std::path::Path>) -> anyhow::Result<()> {
    match std::fs::create_dir(path) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => {
            Err(e)?;
        }
    }

    Ok(())
}
