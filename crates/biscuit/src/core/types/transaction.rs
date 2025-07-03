use crate::core::traits::{TransactionExtensions as _, TransactionMetaExtensions as _};
use crate::utils::fees::PriorityDetails;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use anyhow::anyhow;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::CompiledInstruction;
use solana_sdk::address_lookup_table::state::AddressLookupTable;
use solana_sdk::hash::Hash;
use solana_sdk::message::v0::{LoadedAddresses, MessageAddressTableLookup};
use solana_sdk::message::{AccountKeys, VersionedMessage};
use solana_sdk::program_utils::limited_deserialize;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_sdk::slot_history::Slot;
use solana_sdk::transaction::TransactionError;
use solana_sdk::vote::instruction::VoteInstruction;
use solana_transaction_status::{
    EncodedConfirmedTransactionWithStatusMeta, EncodedTransactionWithStatusMeta,
    UiTransactionStatusMeta, UiTransactionTokenBalance,
};
use sqlx::FromRow;
use yellowstone_grpc_proto::convert_from::{create_tx_meta, create_tx_versioned};
use yellowstone_grpc_proto::geyser::SubscribeUpdateTransaction;

#[derive(Debug, Clone, FromRow)]
pub struct ITransaction {
    // TODO: Use unwraps on the meta? Should always be Some yh?
    /// The transaction signature
    pub signature: Signature,
    /// The transaction message
    pub message: VersionedMessage,
    /// The slot corresponding to this transaction's entry
    pub slot: Slot,
    /// Loaded addresses for this transaction
    pub loaded_addresses: Option<LoadedAddresses>, // todo: borrow?
    /// The transaction meta
    pub meta: Option<UiTransactionStatusMeta>,
    /// Error that might have occured during the transaction
    pub err: Option<TransactionError>,
    /// Priority fee details for this transaction
    pub priority_details: PriorityDetails,
    /// Compute units consumed by this transaction
    pub compute_units_consumed: Option<u64>,
    /// The blockhash used in sending the transaction
    pub recent_blockhash: Hash,
    /// Map of an instruction index to its inner instructions
    pub inner_instructions: Option<HashMap<u8, Vec<CompiledInstruction>>>,
    /// Whether or not this transaction is a vote transaction
    pub is_vote_transaction: bool,
    /// Address-lookup-tables needed for this transaction's keys
    pub address_table_lookups: Vec<MessageAddressTableLookup>,
    pub readable_accounts: Vec<Pubkey>,
    pub writable_accounts: Vec<Pubkey>,
    pub block_time: Option<i64>,
}

pub struct TokenBalances {
    pub token_accounts: HashMap<Pubkey, Pubkey>,
    //pub mints: HashMap<Pubkey, MintAccountInfo>,
}

// pub struct MintAccountInfo {
//     decimals: u8,
// }

pub struct TokenAccountInfo {
    pub index: u8,
    pub owner: Pubkey,
    pub mint: Pubkey,
    pub decimals: u8,
    pub pre_balance: f64,
    pub post_balance: f64,
}

impl ITransaction {
    pub fn is_legacy(&self) -> bool {
        matches!(self.message, VersionedMessage::Legacy(_))
    }

    pub fn is_versioned(&self) -> bool {
        matches!(self.message, VersionedMessage::V0(_))
    }

    pub async fn extract_or_load_additional_keys(
        &mut self,
        client: &RpcClient,
    ) -> anyhow::Result<()> {
        if let Some(loaded) = self.extract_loaded_addresses() {
            self.loaded_addresses = Some(loaded);
        } else {
            self.load_keys(client).await?;
        }
        Ok(())
    }

    pub async fn load_keys(&mut self, client: &RpcClient) -> anyhow::Result<()> {
        match self.message.address_table_lookups() {
            Some(lookups) if !lookups.is_empty() => match lookup_addresses(client, lookups).await {
                Ok(addresses) => {
                    self.loaded_addresses = Some(addresses);
                    Ok(())
                }
                Err(e) => Err(anyhow!("Failed lookup for tx {}: {}", self.signature, e)),
            },
            _ => Ok(()),
        }
    }

    pub fn account_keys(&self) -> AccountKeys {
        AccountKeys::new(
            self.message.static_account_keys(),
            self.loaded_addresses.as_ref(),
        )
    }

    pub fn extract_mints_from_meta(&self) -> Option<Vec<Pubkey>> {
        let post_token_balances = Option::<Vec<UiTransactionTokenBalance>>::from(
            self.meta.as_ref()?.post_token_balances.clone(),
        )
        .unwrap_or_default();

        let pre_token_balances = Option::<Vec<UiTransactionTokenBalance>>::from(
            self.meta.as_ref()?.pre_token_balances.clone(),
        )
        .unwrap_or_default();

        let mints = post_token_balances
            .into_iter()
            .chain(pre_token_balances.into_iter())
            .map(|b| Pubkey::from_str(&b.mint).unwrap())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        Some(mints)
    }

    pub fn extract_token_balance_details(
        &self,
    ) -> Option<HashMap<(Pubkey, Pubkey), TokenAccountInfo>> {
        let post_token_balances = Option::<Vec<UiTransactionTokenBalance>>::from(
            self.meta.as_ref()?.post_token_balances.clone(),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|balance| (balance.account_index, balance))
        .collect::<HashMap<_, _>>();
        let pre_token_balances = Option::<Vec<UiTransactionTokenBalance>>::from(
            self.meta.as_ref()?.pre_token_balances.clone(),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|balance| (balance.account_index, balance))
        .collect::<HashMap<_, _>>();

        let mut token_account_map = HashMap::new();
        for (index, balance) in post_token_balances {
            let owner = Option::<String>::from(balance.owner)?;
            let owner = Pubkey::from_str(&owner).ok()?;
            let mint = Pubkey::from_str(&balance.mint).ok()?;
            let decimals = balance.ui_token_amount.decimals;
            let pre_balance = balance.ui_token_amount.ui_amount?;

            let post_balance = pre_token_balances.get(&index)?;
            let info = TokenAccountInfo {
                index,
                owner,
                mint,
                decimals,
                pre_balance,
                post_balance: post_balance.ui_token_amount.ui_amount?,
            };
            token_account_map.insert((owner, mint), info);
        }

        Some(token_account_map)
    }

    fn extract_loaded_addresses(&self) -> Option<LoadedAddresses> {
        self.meta
            .as_ref()
            .and_then(|m| m.extract_loaded_addresses())
    }
}

pub async fn lookup_addresses(
    client: &RpcClient,
    lookups: &[MessageAddressTableLookup],
) -> Result<LoadedAddresses, anyhow::Error> {
    let mut loaded_addresses = vec![];
    let keys = lookups.iter().map(|t| t.account_key).collect::<Vec<_>>();
    let lookup_accounts = client.get_multiple_accounts(&keys).await?;
    debug_assert!(lookup_accounts.len() == lookups.len());
    for (i, account) in lookup_accounts.into_iter().enumerate() {
        if account.is_none() {
            return Err(anyhow!("Failed to get account for address table lookup"));
        }
        let account = account.unwrap();
        let lookup_table = AddressLookupTable::deserialize(&account.data)
            .map_err(|_| anyhow!("failed to deserialize account lookup table"))?;
        loaded_addresses.push(LoadedAddresses {
            writable: lookups[i]
                .writable_indexes
                .iter()
                .map(|idx| {
                    lookup_table
                        .addresses
                        .get(*idx as usize)
                        .copied()
                        .ok_or(anyhow!(
                            "account lookup went out of bounds of address lookup table"
                        ))
                })
                .collect::<Result<_, _>>()?,
            readonly: lookups[i]
                .readonly_indexes
                .iter()
                .map(|idx| {
                    lookup_table
                        .addresses
                        .get(*idx as usize)
                        .copied()
                        .ok_or(anyhow!(
                            "account lookup went out of bounds of address lookup table"
                        ))
                })
                .collect::<Result<_, _>>()?,
        });
    }
    Ok(LoadedAddresses::from_iter(loaded_addresses))
}

impl TryFrom<SubscribeUpdateTransaction> for ITransaction {
    type Error = anyhow::Error;
    fn try_from(value: SubscribeUpdateTransaction) -> Result<Self, Self::Error> {
        let slot = value.slot;
        if value.transaction.as_ref().is_none() {
            return Err(anyhow!("No transaction data in Geyser update"));
        }
        let transaction_info = value.transaction.unwrap();
        if transaction_info.transaction.is_none() {
            return Err(anyhow!("No transaction data in Geyser update"));
        }
        let transaction = transaction_info.transaction.unwrap();
        let versioned_tx = create_tx_versioned(transaction).map_err(|e| anyhow!(e))?;
        let is_vote_transaction = versioned_tx.message.instructions().iter().any(|i| {
            i.program_id(versioned_tx.message.static_account_keys())
                .eq(&solana_sdk::vote::program::id())
                && limited_deserialize::<VoteInstruction>(&i.data)
                    .map(|vi| vi.is_simple_vote())
                    .unwrap_or(false)
        });

        let priority_details = PriorityDetails::from_versioned_transaction(&versioned_tx);
        let signature = Signature::try_from(transaction_info.signature)
            .map_err(|_| anyhow!("Failed converting signature from u8 array"))?;
        let address_table_lookups = versioned_tx
            .message
            .address_table_lookups()
            .map(|x| x.to_vec())
            .unwrap_or_default();

        let meta: Option<UiTransactionStatusMeta> = transaction_info
            .meta
            .and_then(|meta| create_tx_meta(meta).ok().map(|m| m.into()));
        let err = meta.as_ref().and_then(|m| m.err.clone());
        let inner_instructions = meta
            .as_ref()
            .and_then(|meta| meta.extract_compiled_inner_instructions());
        let recent_blockhash = *versioned_tx.message.recent_blockhash();
        let compute_units_consumed = meta
            .as_ref()
            .and_then(|meta| meta.compute_units_consumed.clone().into());

        let mut readable_accounts = vec![];
        let mut writable_accounts = vec![];
        let keys = versioned_tx.message.static_account_keys();
        for (index, key) in keys.iter().enumerate() {
            if versioned_tx.message.is_maybe_writable(index) {
                writable_accounts.push(*key);
            } else {
                readable_accounts.push(*key);
            }
        }

        Ok(ITransaction {
            signature,
            slot,
            message: versioned_tx.message,
            loaded_addresses: None,
            meta,
            err,
            priority_details,
            compute_units_consumed,
            recent_blockhash,
            inner_instructions,
            is_vote_transaction,
            address_table_lookups,
            readable_accounts,
            writable_accounts,
            block_time: Some(0),
        })
    }
}

impl TryFrom<(EncodedTransactionWithStatusMeta, u64, Option<i64>)> for ITransaction {
    type Error = anyhow::Error;

    fn try_from(
        value: (EncodedTransactionWithStatusMeta, u64, Option<i64>),
    ) -> Result<Self, Self::Error> {
        value
            .0
            .to_unified_transaction(value.1, value.2)
            .ok_or(anyhow::anyhow!("Failed decoding transaction"))
    }
}

impl TryFrom<EncodedConfirmedTransactionWithStatusMeta> for ITransaction {
    type Error = anyhow::Error;

    fn try_from(value: EncodedConfirmedTransactionWithStatusMeta) -> Result<Self, Self::Error> {
        (value.transaction, value.slot, value.block_time).try_into()
    }
}
