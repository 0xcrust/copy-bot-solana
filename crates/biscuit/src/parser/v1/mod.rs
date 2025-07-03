//! V1 of the tx-parser will just try to parse an instruction and get token swaps that might have happened.
//!
//! It will do this by only focusing on major programs: JupiterV6, Raydium, Orca, Pumpfun, DegenFund,
//! Moonshot as of now. It will also only focus on parsing the swap instructions for these programs.
//!
//! Given an instruction, we want to at least be able to get the input and output token-mints, so we can copy
//! the trade.
//!
//! Swap params gotten from Jupiter, Raydium or Orca can be executed in any of Jupiter, Raydium, or Orca.
//!
//! Swap params gotten from Pumpfun, DegenFund, and Moonshot are only executed by the protocol the original
//! swap instruction was made.
//!
//! LFG!
//!
//! TODO:
//! - Parse the program-instructions we know how to.
//! - Parse the transaction and check for token-balance updates for the transaction payer.
//! - Construct transactions from scratch.
//! - Construct transactions by directly replacing the copied transaction with the right accounts(and amount).

pub mod jupiter_v6;
pub mod logs;
pub mod orca;
pub mod raydium_v4;
pub mod raydium_v6;

use super::swap::{Provider, ResolvedSwap};
use crate::constants::programs::{
    JUPITER_AGGREGATOR_V6_PROGRAM_ID, ORCA_WHIRLPOOLS_PROGRAM_ID, PUMPFUN_PROGRAM_ID,
    RAYDIUM_CONCENTRATED_LIQUIDITY_PROGRAM_ID, RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID,
};
use crate::core::types::transaction::ITransaction;
use crate::parser::v1::logs::jupiter_v6::{merge_jupiter_events, parse_jupiter_swap_events};
use jupiter_v6::{decode_jupiter_v6_instruction, ParsedJupiterInstruction};
use logs::pump::parse_pumpfun_swap_events;
use logs::{get_cpi_logs_for_instruction, get_logs_for_program};
use orca::{decode_orca_instruction, ParsedOrcaInstruction};
use raydium_v4::{decode_raydium_v4_instruction, ParsedRaydiumV4Instruction};
use raydium_v6::{decode_raydium_v6_instruction, ParsedRaydiumV6Instruction};
use solana_transaction_status::{UiTransactionStatusMeta, UiTransactionTokenBalance};
use std::ops::Mul;
use std::str::FromStr;

use anyhow::{anyhow, Context, Result};
use solana_sdk::instruction::CompiledInstruction;
use solana_sdk::message::AccountKeys;
use solana_sdk::pubkey::Pubkey;

// TODO: Add Meteora support
const PROGRAMS: [Pubkey; 5] = [
    JUPITER_AGGREGATOR_V6_PROGRAM_ID,
    RAYDIUM_CONCENTRATED_LIQUIDITY_PROGRAM_ID,
    RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID,
    ORCA_WHIRLPOOLS_PROGRAM_ID,
    PUMPFUN_PROGRAM_ID,
];

pub struct V1Parser;

impl V1Parser {
    // todo: Too specific and currently relies on exact knowledge of Jupiter and PF log methods.
    // We need to make this robust enough to be generalized.
    //
    // Note: This isn't fool-proof. There could be multiple hops of a swap in the same transaction, and there's hardly
    // a clean way to catch all edge-cases, but we can try.
    pub fn parse_swap_transaction(transaction: &ITransaction) -> Result<Option<ResolvedSwap>> {
        for (instruction_idx, instruction) in transaction.message.instructions().iter().enumerate()
        {
            let Some(swap) =
                Self::parse_swap_instruction(transaction, instruction, Some(instruction_idx))?
            else {
                continue;
            };
            return Ok(Some(swap));
        }

        if let Some(instructions) = transaction.inner_instructions.as_ref() {
            for instruction in instructions.values().flatten() {
                let Some(swap) = Self::parse_swap_instruction(transaction, instruction, None)?
                else {
                    continue;
                };
                return Ok(Some(swap));
            }
        }

        Ok(None)
    }

    pub fn parse_swap_instruction(
        transaction: &ITransaction,
        instruction: &CompiledInstruction,
        idx: Option<usize>,
    ) -> Result<Option<ResolvedSwap>> {
        let program_id = *transaction
            .account_keys()
            .get(instruction.program_id_index as usize)
            .ok_or(anyhow!("Failed getting program-id. Invalid index?"))?;

        Ok(match program_id {
            JUPITER_AGGREGATOR_V6_PROGRAM_ID => match idx {
                Some(idx) => Self::decode_jupiter(&transaction, idx)?,
                None => return Ok(None),
            },
            PUMPFUN_PROGRAM_ID => Self::decode_pumpfun(&transaction, &instruction)?,
            RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID => {
                Self::decode_raydium_v4(&transaction, &instruction)?
            }
            _ => return Ok(None),
        })
    }

    pub fn decode_jupiter(transaction: &ITransaction, idx: usize) -> Result<Option<ResolvedSwap>> {
        let account_keys = transaction.account_keys();
        let cpi_logs = get_cpi_logs_for_instruction(transaction, idx).unwrap_or_default();
        let event = merge_jupiter_events(parse_jupiter_swap_events(&cpi_logs));
        let Some(event) = event else { return Ok(None) };

        Ok(Some(ResolvedSwap {
            provider: Provider::Jupiter,
            owner: None,
            input_amount: event.input_amount,
            output_amount: event.output_amount,
            input_token_mint: event.input_mint,
            output_token_mint: event.output_mint,
            token_decimals: None,
            input_mint_info: None,
            output_mint_info: None,
        }))
    }

    pub fn decode_pumpfun(
        transaction: &ITransaction,
        instruction: &CompiledInstruction,
    ) -> Result<Option<ResolvedSwap>> {
        let account_keys = transaction.account_keys();
        let logs = get_logs_for_program(&transaction, &PUMPFUN_PROGRAM_ID);
        let events = parse_pumpfun_swap_events(&logs);

        let Some(event) = events.first() else {
            return Ok(None);
        };

        let (input_amount, output_amount, input_token_mint, output_token_mint) = if event.is_buy {
            (
                event.sol_amount,
                event.token_amount,
                spl_token::native_mint::ID,
                event.mint,
            )
        } else {
            (
                event.token_amount,
                event.sol_amount,
                event.mint,
                spl_token::native_mint::ID,
            )
        };

        Ok(Some(ResolvedSwap {
            provider: Provider::Pump,
            owner: None,
            input_amount,
            output_amount,
            input_token_mint,
            output_token_mint,
            token_decimals: None,
            input_mint_info: None,
            output_mint_info: None,
        }))
    }

    pub fn decode_raydium_v4(
        transaction: &ITransaction,
        instruction: &CompiledInstruction,
    ) -> Result<Option<ResolvedSwap>> {
        let instruction = decode_raydium_v4_instruction(instruction, &transaction.account_keys())?;
        let meta = transaction
            .meta
            .as_ref()
            .context("transaction meta is null")?;
        let account_keys = transaction.account_keys();

        Ok(Some(match instruction {
            ParsedRaydiumV4Instruction::SwapBaseIn { args, accounts } => {
                let RaydiumAccounts {
                    amm_coin_pre,
                    amm_coin_post,
                    amm_coin_mint,
                    amm_pc_pre,
                    amm_pc_post,
                    amm_pc_mint,
                    coin_to_pc,
                } = Self::resolve_raydium_accounts(
                    &meta,
                    &account_keys,
                    &accounts.user_source_token_account,
                    &accounts.user_destination_token_account,
                    &accounts.amm_coin_vault,
                    &accounts.amm_pc_vault,
                )?;

                let (input_token_mint, output_token_mint, output_amount) = if coin_to_pc {
                    (
                        amm_coin_mint,
                        amm_pc_mint,
                        amm_pc_pre.checked_sub(amm_pc_post),
                    )
                } else {
                    (
                        amm_pc_mint,
                        amm_coin_mint,
                        amm_coin_pre.checked_sub(amm_coin_post),
                    )
                };
                let output_amount =
                    output_amount.context("amm output vault should decrease in balance")?;

                ResolvedSwap {
                    provider: Provider::RaydiumV4 {
                        state: Some(accounts.amm_account),
                    },
                    owner: Some(accounts.user_wallet_account),
                    input_amount: args.amount_in,
                    output_amount,
                    input_token_mint,
                    output_token_mint,
                    token_decimals: None,
                    input_mint_info: None,
                    output_mint_info: None,
                }
            }
            ParsedRaydiumV4Instruction::SwapBaseOut { args, accounts } => {
                let RaydiumAccounts {
                    amm_coin_pre,
                    amm_coin_post,
                    amm_coin_mint,
                    amm_pc_pre,
                    amm_pc_post,
                    amm_pc_mint,
                    coin_to_pc,
                } = Self::resolve_raydium_accounts(
                    &meta,
                    &account_keys,
                    &accounts.user_source_token_account,
                    &accounts.user_destination_token_account,
                    &accounts.amm_coin_vault,
                    &accounts.amm_pc_vault,
                )?;

                let (input_token_mint, output_token_mint, input_amount) = if coin_to_pc {
                    (
                        amm_coin_mint,
                        amm_pc_mint,
                        amm_coin_post.checked_sub(amm_coin_pre),
                    )
                } else {
                    (
                        amm_pc_mint,
                        amm_coin_mint,
                        amm_pc_post.checked_sub(amm_pc_pre),
                    )
                };
                let input_amount =
                    input_amount.context("amm input vault should increase in balance")?;

                ResolvedSwap {
                    provider: Provider::RaydiumV4 {
                        state: Some(accounts.amm_account),
                    },
                    owner: Some(accounts.user_wallet_account),
                    input_amount,
                    output_amount: args.amount_out,
                    input_token_mint,
                    output_token_mint,
                    token_decimals: None,
                    input_mint_info: None,
                    output_mint_info: None,
                }
            }
            _ => return Ok(None),
        }))
    }

    fn resolve_raydium_accounts(
        meta: &UiTransactionStatusMeta,
        account_keys: &AccountKeys,
        user_source: &Pubkey,
        user_destination: &Pubkey,
        amm_coin_vault: &Pubkey,
        amm_pc_vault: &Pubkey,
    ) -> Result<RaydiumAccounts> {
        let mut user_accounts = Vec::with_capacity(4);
        log::debug!("Source={}, destination={}", user_source, user_destination);

        let (user_input_pre, user_input_post) =
            Self::get_balances_for_token_account(meta, account_keys, &user_source);
        log::debug!(
            "user-input-pre={:#?}, user-input-pre={:#?}",
            user_input_pre,
            user_input_post
        );

        user_accounts.extend([(user_input_pre, true), (user_input_post, true)]); // is input
        let (user_output_pre, user_output_post) =
            Self::get_balances_for_token_account(meta, account_keys, &user_destination);
        log::debug!(
            "user-output-pre={:#?}, user-output-post={:#?}",
            user_output_pre,
            user_output_post
        );

        user_accounts.extend([(user_output_pre, false), (user_output_post, false)]); // is not input
        let item = user_accounts.iter().find_map(|(b, is_input)| {
            if let Some(balance) = b {
                Some((balance, is_input))
            } else {
                None
            }
        });

        // At least one of the user's accounts should be present
        let Some((user_account, is_input)) = item else {
            return Err(anyhow!("Failed to get info for any user account"));
        };
        let user_account_mint = Pubkey::from_str(&user_account.mint)?;

        let (amm_coin_pre, amm_coin_post) =
            Self::get_balances_for_token_account(meta, account_keys, &amm_coin_vault);
        let (amm_pc_pre, amm_pc_post) =
            Self::get_balances_for_token_account(meta, account_keys, &amm_pc_vault);

        let amm_coin_pre = amm_coin_pre.context("failed getting pre info for amm coin vault")?;
        let amm_coin_post = amm_coin_post.context("failed getting post info for amm coin vault")?;
        let amm_pc_pre = amm_pc_pre.context("failed getting pre info for amm pc vault")?;
        let amm_pc_post = amm_pc_post.context("failed getting post info for amm pc vault")?;

        let amm_coin_mint = Pubkey::from_str(&amm_coin_post.mint)?;
        let amm_pc_mint = Pubkey::from_str(&amm_pc_post.mint)?;

        let amm_coin_pre = Self::calc_amount(&amm_coin_pre);
        let amm_coin_post = Self::calc_amount(&amm_coin_post);
        let amm_pc_pre = Self::calc_amount(&amm_pc_pre);
        let amm_pc_post = Self::calc_amount(&amm_pc_post);

        Ok(RaydiumAccounts {
            amm_coin_pre,
            amm_coin_post,
            amm_pc_pre,
            amm_pc_post,
            amm_coin_mint,
            amm_pc_mint,
            coin_to_pc: (amm_coin_mint == user_account_mint) == *is_input,
        })
    }

    fn get_balances_for_token_account(
        meta: &UiTransactionStatusMeta,
        account_keys: &AccountKeys,
        account: &Pubkey,
    ) -> (
        Option<UiTransactionTokenBalance>,
        Option<UiTransactionTokenBalance>,
    ) {
        let pre_token_balances: Vec<UiTransactionTokenBalance> =
            Option::from(meta.pre_token_balances.clone()).unwrap_or_default();
        let post_token_balances: Vec<UiTransactionTokenBalance> =
            Option::from(meta.post_token_balances.clone()).unwrap_or_default();

        let find_balance = |balance: &Vec<UiTransactionTokenBalance>| {
            balance.iter().find_map(|bal| {
                if *account_keys.get(bal.account_index as usize)? == *account {
                    Some(bal.clone())
                } else {
                    None
                }
            })
        };

        let pre = find_balance(&pre_token_balances);
        let post = find_balance(&post_token_balances);

        (pre, post)
    }

    fn calc_amount(balance: &UiTransactionTokenBalance) -> u64 {
        let multiplier = 10u32.pow(balance.ui_token_amount.decimals as u32) as f64;
        balance
            .ui_token_amount
            .ui_amount
            .unwrap_or(0.0)
            .mul(multiplier)
            .trunc() as u64
    }

    pub fn parse_instruction(
        instruction: &CompiledInstruction,
        loaded_keys: &AccountKeys,
    ) -> anyhow::Result<Option<ParsedInstruction>> {
        let program_id = *loaded_keys
            .get(instruction.program_id_index as usize)
            .ok_or(anyhow!("Failed getting program-id. Invalid index?"))?;

        let parsed = match program_id {
            JUPITER_AGGREGATOR_V6_PROGRAM_ID => {
                let decoded = decode_jupiter_v6_instruction(instruction, &loaded_keys)?;
                ParsedInstruction::Jupiter(decoded)
            }
            RAYDIUM_CONCENTRATED_LIQUIDITY_PROGRAM_ID => {
                let decoded = decode_raydium_v6_instruction(instruction, &loaded_keys)?;
                ParsedInstruction::RaydiumV6(decoded)
            }
            RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID => {
                let decoded = decode_raydium_v4_instruction(instruction, &loaded_keys)?;
                ParsedInstruction::RaydiumV4(decoded)
            }
            ORCA_WHIRLPOOLS_PROGRAM_ID => {
                let decoded = decode_orca_instruction(instruction, &loaded_keys)?;
                ParsedInstruction::Orca(decoded)
            }
            PUMPFUN_PROGRAM_ID => {
                return Ok(None); // Pumpfun instruction parsing is not supported
            }
            _ => return Ok(None),
        };

        Ok(Some(parsed))
    }
}

struct RaydiumAccounts {
    amm_coin_pre: u64,
    amm_coin_post: u64,
    amm_coin_mint: Pubkey,
    amm_pc_pre: u64,
    amm_pc_post: u64,
    amm_pc_mint: Pubkey,
    coin_to_pc: bool,
}

pub fn split_discriminator(data: &Vec<u8>) -> ([u8; 8], &[u8]) {
    let mut ix_data: &[u8] = data;
    let sighash: [u8; 8] = {
        let mut sighash: [u8; 8] = [0; 8];
        sighash.copy_from_slice(&ix_data[..8]);
        ix_data = &ix_data[8..];
        sighash
    };

    (sighash, ix_data)
}

pub struct KeysWindow<'a> {
    keys: &'a AccountKeys<'a>,
    accounts: &'a Vec<u8>,
    index: usize,
    program_id: Pubkey,
}

impl<'a> KeysWindow<'a> {
    pub fn new(keys: &'a AccountKeys<'a>, accounts: &'a Vec<u8>, program_id_idx: u8) -> Self {
        log::trace!(
            "Keys length: {}. Instruction accounts length: {}",
            keys.len(),
            accounts.len()
        );
        let program_id = keys[program_id_idx as usize];
        KeysWindow {
            keys,
            accounts,
            index: 0,
            program_id,
        }
    }
    pub fn new_from_instruction(
        keys: &'a AccountKeys<'a>,
        instruction: &'a CompiledInstruction,
    ) -> Self {
        log::debug!(
            "Keys length: {}. Instruction accounts length: {}",
            keys.len(),
            instruction.accounts.len()
        );
        let program_id = keys[instruction.program_id_index as usize];
        KeysWindow {
            keys,
            accounts: &instruction.accounts,
            index: 0,
            program_id,
        }
    }
    pub fn next_account_key(&mut self) -> anyhow::Result<Pubkey> {
        let ret = self
            .keys
            .get(self.accounts[self.index] as usize)
            .ok_or(anyhow!("KeysWindow tried to access out-of-bounds index"))?;
        self.index += 1;
        Ok(*ret)
    }
    pub fn next_anchor_opt_account_key(&mut self) -> anyhow::Result<Option<Pubkey>> {
        let ret = self.next_account_key()?;
        if ret == self.program_id {
            Ok(None)
        } else {
            Ok(Some(ret))
        }
    }
}

pub enum ParsedInstruction {
    Jupiter(ParsedJupiterInstruction),
    RaydiumV6(ParsedRaydiumV6Instruction),
    RaydiumV4(ParsedRaydiumV4Instruction),
    Orca(ParsedOrcaInstruction),
}

impl std::fmt::Debug for ParsedInstruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use ParsedInstruction::*;
        match self {
            Jupiter(ix) => f.write_str("Jupiter"),
            RaydiumV6(ix) => f.write_str("RaydiumV6"),
            RaydiumV4(ix) => f.write_str("RaydiumV4"),
            Orca(ix) => f.write_str("Orca: {:?}"),
        }
    }
}

fn decode_instruction<T: anchor_lang::AnchorDeserialize>(
    slice: &mut &[u8],
) -> Result<T, anchor_lang::error::Error> {
    let instruction: T = anchor_lang::AnchorDeserialize::deserialize(slice).map_err(|_| {
        anchor_lang::error::Error::from(anchor_lang::error::ErrorCode::InstructionDidNotDeserialize)
    })?;
    Ok(instruction)
}

const SIGHASH_GLOBAL_NAMESPACE: &str = "global";
pub fn anchor_instruction_sighash(ix_name: &str) -> [u8; 8] {
    anchor_sighash(SIGHASH_GLOBAL_NAMESPACE, ix_name)
}

const SIGHASH_EVENT_NAMESPACE: &str = "event";
pub fn anchor_event_sighash(event_name: &str) -> [u8; 8] {
    anchor_sighash(SIGHASH_EVENT_NAMESPACE, event_name)
}

pub fn anchor_sighash(namespace: &str, name: &str) -> [u8; 8] {
    let preimage = format!("{namespace}:{name}");

    let mut sighash = [0u8; 8];
    sighash.copy_from_slice(&solana_sdk::hash::hash(preimage.as_bytes()).to_bytes()[..8]);
    sighash
}

#[cfg(test)]
mod test {
    use super::{split_discriminator, KeysWindow};
    use solana_sdk::message::AccountKeys;
    use solana_sdk::signature::{Keypair, Signer};

    fn split_discriminator_test() {
        let data = [vec![0u8; 10], vec![1, 1, 1, 1]].concat();
        let (split, rest) = split_discriminator(&data);
        assert!(rest.len() == 6);
        assert!(split == [0, 0, 0, 0, 0, 0, 0, 0]);
        assert!(*rest == [0, 0, 1, 1, 1, 1]);
    }

    fn keys_window_test() {
        let keys = (0..20)
            .into_iter()
            .map(|_| Keypair::new().pubkey())
            .collect::<Vec<_>>();
        let accounts = vec![10, 9, 6, 8, 13, 14, 2, 2, 4, 4, 5, 16, 16];
        let account_keys = AccountKeys::new(&keys, None);
        let mut window = KeysWindow::new(&account_keys, &accounts, 16);
        assert!(window.next_account_key().unwrap() == keys[10]);
        assert!(window.next_account_key().unwrap() == keys[9]);
        assert!(window.next_account_key().unwrap() == keys[6]);
        assert!(window.next_account_key().unwrap() == keys[8]);
        assert!(window.next_account_key().unwrap() == keys[13]);
        assert!(window.next_account_key().unwrap() == keys[14]);
        assert!(window.next_account_key().unwrap() == keys[2]);
        assert!(window.next_account_key().unwrap() == keys[2]);
        assert!(window.next_account_key().unwrap() == keys[4]);
        assert!(window.next_account_key().unwrap() == keys[4]);
        assert!(window.next_account_key().unwrap() == keys[5]);
        assert!(window.next_anchor_opt_account_key().unwrap().is_none());
        assert!(window.next_anchor_opt_account_key().unwrap().is_none());
    }
}
