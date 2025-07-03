use crate::core::types::transaction::ITransaction;
use crate::utils::fees::PriorityDetails;

use solana_program::message::v0::{LoadedAddresses, LoadedMessage};
use solana_sdk::bs58;
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::instruction::{AccountMeta, CompiledInstruction, Instruction};
use solana_sdk::message::{Message, SanitizedMessage, VersionedMessage};
use solana_sdk::program_utils::limited_deserialize;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::vote::instruction::VoteInstruction;
use solana_transaction_status::{
    EncodedTransactionWithStatusMeta, UiInnerInstructions, UiInstruction, UiLoadedAddresses,
    UiTransactionStatusMeta,
};

use std::collections::HashMap;
use std::str::FromStr;

pub trait TransactionMetaExtensions {
    fn extract_compiled_inner_instructions(&self) -> Option<HashMap<u8, Vec<CompiledInstruction>>>;
    fn extract_loaded_addresses(&self) -> Option<LoadedAddresses>;
}

pub trait MessageExtensions {
    fn extract_keys(&self) -> Vec<Pubkey>;
    fn extract_instructions(&self) -> Vec<Instruction>;
}

pub trait TransactionExtensions {
    fn to_unified_transaction(&self, slot: u64, block_time: Option<i64>) -> Option<ITransaction>;
}

impl TransactionExtensions for EncodedTransactionWithStatusMeta {
    fn to_unified_transaction(&self, slot: u64, block_time: Option<i64>) -> Option<ITransaction> {
        let versioned_tx = self.transaction.decode()?;
        let is_vote_transaction = versioned_tx.message.instructions().iter().any(|i| {
            i.program_id(versioned_tx.message.static_account_keys())
                .eq(&solana_sdk::vote::program::id())
                && limited_deserialize::<VoteInstruction>(&i.data)
                    .map(|vi| vi.is_simple_vote())
                    .unwrap_or(false)
        });
        let priority_details = PriorityDetails::from_versioned_transaction(&versioned_tx);
        let inner_instructions = self
            .meta
            .as_ref()
            .and_then(|meta| meta.extract_compiled_inner_instructions());
        let err = self.meta.as_ref().and_then(|m| m.err.clone());
        let recent_blockhash = *versioned_tx.message.recent_blockhash();
        let address_table_lookups = versioned_tx
            .message
            .address_table_lookups()
            .map(|x| x.to_vec())
            .unwrap_or_default();
        let compute_units_consumed = self
            .meta
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

        Some(ITransaction {
            signature: versioned_tx.signatures[0],
            slot,
            message: versioned_tx.message,
            meta: self.meta.clone(),
            err,
            loaded_addresses: None,
            priority_details,
            recent_blockhash,
            inner_instructions,
            is_vote_transaction,
            address_table_lookups,
            compute_units_consumed,
            readable_accounts,
            writable_accounts,
            block_time,
        })
    }
}

impl TransactionMetaExtensions for UiTransactionStatusMeta {
    fn extract_compiled_inner_instructions(&self) -> Option<HashMap<u8, Vec<CompiledInstruction>>> {
        let inner_instructions: Option<&Vec<UiInnerInstructions>> =
            self.inner_instructions.as_ref().into();
        Some(HashMap::from_iter(inner_instructions?.iter().map(
            |inner_ix| {
                (
                    inner_ix.index,
                    inner_ix
                        .instructions
                        .iter()
                        .filter_map(|ix| match ix {
                            UiInstruction::Compiled(ix) => Some(CompiledInstruction {
                                program_id_index: ix.program_id_index,
                                accounts: ix.accounts.clone(),
                                data: bs58::decode(&ix.data).into_vec().unwrap(),
                            }),
                            _ => None,
                        })
                        .collect::<Vec<_>>(),
                )
            },
        )))
    }

    fn extract_loaded_addresses(&self) -> Option<LoadedAddresses> {
        let addresses: Option<&UiLoadedAddresses> = self.loaded_addresses.as_ref().into();
        let ui_loaded_addresses = addresses?;
        Some(LoadedAddresses {
            readonly: ui_loaded_addresses
                .readonly
                .iter()
                .map(|s| Pubkey::from_str(s.as_str()).unwrap())
                .collect(),
            writable: ui_loaded_addresses
                .writable
                .iter()
                .map(|s| Pubkey::from_str(s.as_str()).unwrap())
                .collect(),
        })
    }
}

/// Decompile a [VersionedMessage] back into its instructions.
pub fn extract_instructions_from_versioned_message(
    message: &VersionedMessage,
    loaded_addresses: &LoadedAddresses,
) -> Vec<Instruction> {
    match &message {
        VersionedMessage::Legacy(message) => extract_instructions_from_legacy_message(message),
        VersionedMessage::V0(message) => {
            let loaded_message = LoadedMessage::new_borrowed(message, loaded_addresses);
            let addrs: Vec<Pubkey> = loaded_message.account_keys().iter().copied().collect();
            message
                .instructions
                .iter()
                .map(|ix| {
                    let mut account_metas = vec![];
                    for idx in &ix.accounts {
                        let idx = *idx as usize;
                        let is_signer = loaded_message.is_signer(idx);
                        if loaded_message.is_writable(idx) {
                            account_metas
                                .push(AccountMeta::new(*addrs.get(idx).unwrap(), is_signer));
                        } else {
                            account_metas.push(AccountMeta::new_readonly(
                                *addrs.get(idx).unwrap(),
                                is_signer,
                            ));
                        }
                    }
                    let program = addrs.get(ix.program_id_index as usize).unwrap();
                    Instruction::new_with_bytes(*program, &ix.data, account_metas)
                })
                .collect()
        }
    }
}

/// Decompile a [Message] back into its instructions.
pub fn extract_instructions_from_legacy_message(message: &Message) -> Vec<Instruction> {
    let message = match SanitizedMessage::try_from(message.clone()) {
        Ok(m) => m,
        Err(e) => {
            panic!(
                "Failed to sanitize message due to {e}: {:#?} ({} keys) {:#?}, {:#?}",
                message.header,
                message.account_keys.len(),
                message.instructions,
                message.account_keys,
            );
        }
    };
    message
        .decompile_instructions()
        .iter()
        .map(|ix| {
            Instruction::new_with_bytes(
                *ix.program_id,
                ix.data,
                ix.accounts
                    .iter()
                    .map(|act| AccountMeta {
                        pubkey: *act.pubkey,
                        is_signer: act.is_signer,
                        is_writable: act.is_writable,
                    })
                    .collect(),
            )
        })
        .collect()
}

pub trait TransactionMods {
    fn modify_compute_unit_price(&mut self, updated_cu_price: u64);
    fn instructions_mut<'a>(&'a mut self) -> &'a mut [CompiledInstruction];
    fn modify_or_set_compute_unit_price(&mut self, cu_price: u64) {
        unimplemented!()
    }
}

impl TransactionMods for VersionedMessage {
    fn modify_compute_unit_price(&mut self, updated_cu_price: u64) {
        match self {
            VersionedMessage::Legacy(message) => {
                message.modify_compute_unit_price(updated_cu_price)
            }
            VersionedMessage::V0(message) => message.modify_compute_unit_price(updated_cu_price),
        }
    }
    fn instructions_mut<'a>(&'a mut self) -> &'a mut [CompiledInstruction] {
        match self {
            VersionedMessage::Legacy(message) => message.instructions_mut(),
            VersionedMessage::V0(message) => message.instructions_mut(),
        }
    }
}

impl TransactionMods for solana_sdk::message::legacy::Message {
    fn modify_compute_unit_price(&mut self, updated_cu_price: u64) {
        for instruction in self.instructions.iter_mut() {
            let program_id = self.account_keys[instruction.program_id_index as usize];
            if solana_sdk::compute_budget::check_id(&program_id) {
                match solana_sdk::borsh1::try_from_slice_unchecked(&instruction.data) {
                    Ok(ComputeBudgetInstruction::SetComputeUnitPrice(micro_lamports)) => {
                        let updated_data = borshv1::to_vec(
                            &ComputeBudgetInstruction::SetComputeUnitPrice(updated_cu_price),
                        )
                        .unwrap();
                        instruction.data = updated_data;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    fn instructions_mut<'a>(&'a mut self) -> &'a mut [CompiledInstruction] {
        &mut self.instructions
    }
}

impl TransactionMods for solana_sdk::message::v0::Message {
    fn modify_compute_unit_price(&mut self, updated_cu_price: u64) {
        for instruction in self.instructions.iter_mut() {
            let program_id = self.account_keys[instruction.program_id_index as usize];
            if solana_sdk::compute_budget::check_id(&program_id) {
                match solana_sdk::borsh1::try_from_slice_unchecked(&instruction.data) {
                    Ok(ComputeBudgetInstruction::SetComputeUnitPrice(micro_lamports)) => {
                        let updated_data = borshv1::to_vec(
                            &ComputeBudgetInstruction::SetComputeUnitPrice(updated_cu_price),
                        )
                        .unwrap();
                        instruction.data = updated_data;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    fn instructions_mut<'a>(&'a mut self) -> &'a mut [CompiledInstruction] {
        &mut self.instructions
    }
}
