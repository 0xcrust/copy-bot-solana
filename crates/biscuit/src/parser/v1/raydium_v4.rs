use super::KeysWindow;

use anyhow::Result;
use raydium_amm::instruction::{
    AmmInstruction as RaydiumV4Instruction, DepositInstruction, InitializeInstruction2,
    SwapInstructionBaseIn, SwapInstructionBaseOut, WithdrawInstruction,
};
use solana_sdk::instruction::CompiledInstruction;
use solana_sdk::message::AccountKeys;
use solana_sdk::pubkey::Pubkey;

pub enum ParsedRaydiumV4Instruction {
    Initialize2 {
        args: InitializeInstruction2,
        accounts: InitializeAccounts2,
    },
    SwapBaseIn {
        args: SwapInstructionBaseIn,
        accounts: SwapBaseInAccounts,
    },
    SwapBaseOut {
        args: SwapInstructionBaseOut,
        accounts: SwapBaseOutAccounts,
    },
    Deposit {
        args: DepositInstruction,
        accounts: DepositAccounts,
    },
    Withdraw {
        args: WithdrawInstruction,
        accounts: WithdrawAccounts,
    },
    Unknown {
        args: RaydiumV4Instruction,
    },
}

impl ParsedRaydiumV4Instruction {
    pub fn get_pool_state(&self) -> Option<Pubkey> {
        let state = match self {
            ParsedRaydiumV4Instruction::Initialize2 { args, accounts } => accounts.amm_account,
            ParsedRaydiumV4Instruction::Deposit { args, accounts } => accounts.amm_account,
            ParsedRaydiumV4Instruction::SwapBaseIn { args, accounts } => accounts.amm_account,
            ParsedRaydiumV4Instruction::SwapBaseOut { args, accounts } => accounts.amm_account,
            ParsedRaydiumV4Instruction::Withdraw { args, accounts } => accounts.amm_account,
            ParsedRaydiumV4Instruction::Unknown { args } => return None,
        };
        Some(state)
    }
}

const SWAP_INSTRUCTIONS_ACCOUNT_LENGTH: usize = 17; // 18 if target orders account is included

pub(super) fn decode_raydium_v4_instruction(
    instruction: &CompiledInstruction,
    keys: &AccountKeys,
) -> Result<ParsedRaydiumV4Instruction> {
    log::debug!("Decoding RaydiumV4 instruction");
    let unpacked = RaydiumV4Instruction::unpack(&instruction.data)?;
    let mut keys = KeysWindow::new_from_instruction(keys, instruction);
    Ok(match unpacked {
        RaydiumV4Instruction::Initialize2(args) => {
            let accounts = InitializeAccounts2 {
                token_program: keys.next_account_key()?,
                associated_token_program: keys.next_account_key()?,
                system_program: keys.next_account_key()?,
                rent: keys.next_account_key()?,
                amm_account: keys.next_account_key()?,
                amm_authority: keys.next_account_key()?,
                amm_open_orders: keys.next_account_key()?,
                amm_lp_mint: keys.next_account_key()?,
                amm_coin_mint: keys.next_account_key()?,
                amm_pc_mint: keys.next_account_key()?,
                amm_coin_vault: keys.next_account_key()?,
                amm_pc_vault: keys.next_account_key()?,
                amm_target_orders: keys.next_account_key()?,
                amm_config: keys.next_account_key()?,
                amm_fee_destination_account: keys.next_account_key()?,
                market_program: keys.next_account_key()?,
                market_account: keys.next_account_key()?,
                user_account: keys.next_account_key()?,
                user_token_coin_account: keys.next_account_key()?,
                user_token_pc_account: keys.next_account_key()?,
                user_destination_lp_token_account: keys.next_account_key()?,
            };
            ParsedRaydiumV4Instruction::Initialize2 { args, accounts }
        }
        RaydiumV4Instruction::SwapBaseIn(args) => {
            let include_target_orders =
                instruction.accounts.len() > SWAP_INSTRUCTIONS_ACCOUNT_LENGTH;
            let accounts = SwapBaseInAccounts {
                token_program: keys.next_account_key()?,
                amm_account: keys.next_account_key()?,
                amm_authority: keys.next_account_key()?,
                amm_open_orders: keys.next_account_key()?,
                amm_target_orders: if include_target_orders {
                    Some(keys.next_account_key()?)
                } else {
                    None
                },
                amm_coin_vault: keys.next_account_key()?,
                amm_pc_vault: keys.next_account_key()?,
                market_program: keys.next_account_key()?,
                market_account: keys.next_account_key()?,
                market_bids: keys.next_account_key()?,
                market_asks: keys.next_account_key()?,
                market_event_queue: keys.next_account_key()?,
                market_coin_vault: keys.next_account_key()?,
                market_pc_vault: keys.next_account_key()?,
                market_vault_signer: keys.next_account_key()?,
                user_source_token_account: keys.next_account_key()?,
                user_destination_token_account: keys.next_account_key()?,
                user_wallet_account: keys.next_account_key()?,
            };
            ParsedRaydiumV4Instruction::SwapBaseIn { args, accounts }
        }
        RaydiumV4Instruction::SwapBaseOut(args) => {
            let include_target_orders =
                instruction.accounts.len() > SWAP_INSTRUCTIONS_ACCOUNT_LENGTH;
            let accounts = SwapBaseOutAccounts {
                token_program: keys.next_account_key()?,
                amm_account: keys.next_account_key()?,
                amm_authority: keys.next_account_key()?,
                amm_open_orders: keys.next_account_key()?,
                amm_target_orders: if include_target_orders {
                    Some(keys.next_account_key()?)
                } else {
                    None
                },
                amm_coin_vault: keys.next_account_key()?,
                amm_pc_vault: keys.next_account_key()?,
                market_program: keys.next_account_key()?,
                market_account: keys.next_account_key()?,
                market_bids: keys.next_account_key()?,
                market_asks: keys.next_account_key()?,
                market_event_queue: keys.next_account_key()?,
                market_coin_vault: keys.next_account_key()?,
                market_pc_vault: keys.next_account_key()?,
                market_vault_signer: keys.next_account_key()?,
                user_source_token_account: keys.next_account_key()?,
                user_destination_token_account: keys.next_account_key()?,
                user_wallet_account: keys.next_account_key()?,
            };
            ParsedRaydiumV4Instruction::SwapBaseOut { args, accounts }
        }
        RaydiumV4Instruction::Deposit(args) => {
            let accounts = DepositAccounts {
                token_program: keys.next_account_key()?,
                amm_account: keys.next_account_key()?,
                amm_authority: keys.next_account_key()?,
                amm_open_orders: keys.next_account_key()?,
                amm_target_orders: keys.next_account_key()?,
                amm_lp_mint: keys.next_account_key()?,
                amm_coin_vault: keys.next_account_key()?,
                amm_pc_vault: keys.next_account_key()?,
                market_account: keys.next_account_key()?,
                user_coin_token_account: keys.next_account_key()?,
                user_pc_token_account: keys.next_account_key()?,
                user_lp_token: keys.next_account_key()?,
                user_wallet: keys.next_account_key()?,
                market_event_queue: keys.next_account_key()?,
            };
            ParsedRaydiumV4Instruction::Deposit { args, accounts }
        }
        RaydiumV4Instruction::Withdraw(args) => {
            let accounts = WithdrawAccounts {
                token_program: keys.next_account_key()?,
                amm_account: keys.next_account_key()?,
                amm_authority: keys.next_account_key()?,
                amm_open_orders: keys.next_account_key()?,
                amm_target_orders: keys.next_account_key()?,
                amm_lp_mint: keys.next_account_key()?,
                amm_coin_vault: keys.next_account_key()?,
                amm_pc_vault: keys.next_account_key()?,
                market_program: keys.next_account_key()?,
                market_account: keys.next_account_key()?,
                market_coin_vault: keys.next_account_key()?,
                market_pc_vault: keys.next_account_key()?,
                market_vault_signer: keys.next_account_key()?,
                user_lp_token_account: keys.next_account_key()?,
                user_coin_token_account: keys.next_account_key()?,
                user_pc_token_account: keys.next_account_key()?,
                user_wallet: keys.next_account_key()?,
                market_event_queue: keys.next_account_key()?,
                market_bids: keys.next_account_key()?,
                market_asks: keys.next_account_key()?,
            };
            ParsedRaydiumV4Instruction::Withdraw { args, accounts }
        }
        ix => ParsedRaydiumV4Instruction::Unknown { args: ix },
    })
}

pub struct InitializeAccounts2 {
    pub token_program: Pubkey,
    pub associated_token_program: Pubkey,
    pub system_program: Pubkey,
    pub rent: Pubkey,
    pub amm_account: Pubkey,
    pub amm_authority: Pubkey,
    pub amm_open_orders: Pubkey,
    pub amm_lp_mint: Pubkey,
    pub amm_coin_mint: Pubkey,
    pub amm_pc_mint: Pubkey,
    pub amm_coin_vault: Pubkey,
    pub amm_pc_vault: Pubkey,
    pub amm_target_orders: Pubkey,
    pub amm_config: Pubkey,
    pub amm_fee_destination_account: Pubkey,
    pub market_program: Pubkey,
    pub market_account: Pubkey,
    pub user_account: Pubkey,
    pub user_token_coin_account: Pubkey,
    pub user_token_pc_account: Pubkey,
    pub user_destination_lp_token_account: Pubkey,
}

pub struct DepositAccounts {
    pub token_program: Pubkey,
    pub amm_account: Pubkey,
    pub amm_authority: Pubkey,
    pub amm_open_orders: Pubkey,
    pub amm_target_orders: Pubkey,
    pub amm_lp_mint: Pubkey,
    pub amm_coin_vault: Pubkey,
    pub amm_pc_vault: Pubkey,
    pub market_account: Pubkey,
    pub user_coin_token_account: Pubkey,
    pub user_pc_token_account: Pubkey,
    pub user_lp_token: Pubkey,
    pub user_wallet: Pubkey,
    pub market_event_queue: Pubkey,
}

pub struct WithdrawAccounts {
    pub token_program: Pubkey,
    pub amm_account: Pubkey,
    pub amm_authority: Pubkey,
    pub amm_open_orders: Pubkey,
    pub amm_target_orders: Pubkey,
    pub amm_lp_mint: Pubkey,
    pub amm_coin_vault: Pubkey,
    pub amm_pc_vault: Pubkey,
    pub market_program: Pubkey,
    pub market_account: Pubkey,
    pub market_coin_vault: Pubkey,
    pub market_pc_vault: Pubkey,
    pub market_vault_signer: Pubkey,
    pub user_lp_token_account: Pubkey,
    pub user_coin_token_account: Pubkey,
    pub user_pc_token_account: Pubkey,
    pub user_wallet: Pubkey,
    pub market_event_queue: Pubkey,
    pub market_bids: Pubkey,
    pub market_asks: Pubkey,
}

pub struct SwapBaseInAccounts {
    /// 0. `[]` Spl Token program id
    pub token_program: Pubkey,
    /// 1. `[writable]` AMM Account
    pub amm_account: Pubkey,
    /// 2. `[]` $authority derived from `create_program_address(&[AUTHORITY_AMM, &[nonce]])`.
    pub amm_authority: Pubkey,
    /// 3. `[writable]` AMM open orders Account
    pub amm_open_orders: Pubkey,
    /// 4. `[writable]` (optional)AMM target orders Account, no longer used in the contract, recommended no need to add this Account.
    pub amm_target_orders: Option<Pubkey>,
    /// 5. `[writable]` AMM coin vault Account to swap FROM or To.
    pub amm_coin_vault: Pubkey,
    /// 6. `[writable]` AMM pc vault Account to swap FROM or To.
    pub amm_pc_vault: Pubkey,
    /// 7. `[]` Market program id
    pub market_program: Pubkey,
    /// 8. `[writable]` Market Account. Market program is the owner.
    pub market_account: Pubkey,
    /// 9. `[writable]` Market bids Account
    pub market_bids: Pubkey,
    /// 10. `[writable]` Market asks Account
    pub market_asks: Pubkey,
    /// 11. `[writable]` Market event queue Account
    pub market_event_queue: Pubkey,
    /// 12. `[writable]` Market coin vault Account
    pub market_coin_vault: Pubkey,
    /// 13. `[writable]` Market pc vault Account
    pub market_pc_vault: Pubkey,
    /// 14. '[]` Market vault signer Account
    pub market_vault_signer: Pubkey,
    /// 15. `[writable]` User source token Account.
    pub user_source_token_account: Pubkey,
    /// 16. `[writable]` User destination token Account.
    pub user_destination_token_account: Pubkey,
    /// 17. `[signer]` User wallet Account
    pub user_wallet_account: Pubkey,
}

pub struct SwapBaseOutAccounts {
    /// 0. `[]` Spl Token program id
    pub token_program: Pubkey,
    /// 1. `[writable]` AMM Account
    pub amm_account: Pubkey,
    /// 2. `[]` $authority derived from `create_program_address(&[AUTHORITY_AMM, &[nonce]])`.
    pub amm_authority: Pubkey,
    /// 3. `[writable]` AMM open orders Account
    pub amm_open_orders: Pubkey,
    /// 4. `[writable]` (optional)AMM target orders Account, no longer used in the contract, recommended no need to add this Account.
    pub amm_target_orders: Option<Pubkey>,
    /// 5. `[writable]` AMM coin vault Account to swap FROM or To.
    pub amm_coin_vault: Pubkey,
    /// 6. `[writable]` AMM pc vault Account to swap FROM or To.
    pub amm_pc_vault: Pubkey,
    /// 7. `[]` Market program id
    pub market_program: Pubkey,
    /// 8. `[writable]` Market Account. Market program is the owner.
    pub market_account: Pubkey,
    /// 9. `[writable]` Market bids Account
    pub market_bids: Pubkey,
    /// 10. `[writable]` Market asks Account
    pub market_asks: Pubkey,
    /// 11. `[writable]` Market event queue Account
    pub market_event_queue: Pubkey,
    /// 12. `[writable]` Market coin vault Account
    pub market_coin_vault: Pubkey,
    /// 13. `[writable]` Market pc vault Account
    pub market_pc_vault: Pubkey,
    /// 14. '[]` Market vault signer Account
    pub market_vault_signer: Pubkey,
    /// 15. `[writable]` User source token Account.
    pub user_source_token_account: Pubkey,
    /// 16. `[writable]` User destination token Account.
    pub user_destination_token_account: Pubkey,
    /// 17. `[signer]` User wallet Account
    pub user_wallet_account: Pubkey,
}
