// TODO: We can't rely on generated code for the Raydium-CLMM program because
// kinobi cannot interpret its IDL.
use anchor_lang::Discriminator;
use raydium_amm_v3::accounts::{
    CreatePool as CreatePoolAccounts, DecreaseLiquidity as DecreaseLiquidityAccounts,
    DecreaseLiquidityV2 as DecreaseLiquidityV2Accounts,
    IncreaseLiquidity as IncreaseLiquidityAccounts,
    IncreaseLiquidityV2 as IncreaseLiquidityV2Accounts, SwapSingle as SwapSingleAccounts,
    SwapSingleV2 as SwapSingleV2Accounts,
};
use raydium_amm_v3::instruction::{
    CreatePool as CreatePoolArgs, DecreaseLiquidity as DecreaseLiquidityArgs,
    DecreaseLiquidityV2 as DecreaseLiquidityV2Args, IncreaseLiquidity as IncreaseLiquidityArgs,
    IncreaseLiquidityV2 as IncreaseLiquidityV2Args, Swap as SwapSingleArgs,
    SwapV2 as SwapSingleV2Args,
};
use solana_sdk::pubkey::Pubkey;

use super::{decode_instruction, split_discriminator, KeysWindow};

use anyhow::Result;
use solana_sdk::instruction::CompiledInstruction;
use solana_sdk::message::AccountKeys;

const RAYDIUM_CLMM_CREATE_POOL_DISC: [u8; 8] = CreatePoolArgs::DISCRIMINATOR;
const RAYDIUM_CLMM_INCREASE_LIQUIDITY_DISC: [u8; 8] = IncreaseLiquidityArgs::DISCRIMINATOR;
const RAYDIUM_CLMM_INCREASE_LIQUIDITY_V2_DISC: [u8; 8] = IncreaseLiquidityV2Args::DISCRIMINATOR;
const RAYDIUM_CLMM_DECREASE_LIQUIDITY_DISC: [u8; 8] = DecreaseLiquidityArgs::DISCRIMINATOR;
const RAYDIUM_CLMM_DECREASE_LIQUIDITY_V2_DISC: [u8; 8] = DecreaseLiquidityV2Args::DISCRIMINATOR;
const RAYDIUM_CLMM_SWAP_DISC: [u8; 8] = SwapSingleArgs::DISCRIMINATOR;
const RAYDIUM_CLMM_SWAP_V2_DISC: [u8; 8] = SwapSingleV2Args::DISCRIMINATOR;

pub enum ParsedRaydiumV6Instruction {
    CreatePool {
        args: CreatePoolArgs,
        accounts: CreatePoolAccounts,
    },
    IncreaseLiquidity {
        args: IncreaseLiquidityArgs,
        accounts: IncreaseLiquidityAccounts,
    },
    DecreaseLiquidity {
        args: DecreaseLiquidityArgs,
        accounts: DecreaseLiquidityAccounts,
    },
    IncreaseLiquidityV2 {
        args: IncreaseLiquidityV2Args,
        accounts: IncreaseLiquidityV2Accounts,
    },
    DecreaseLiquidityV2 {
        args: DecreaseLiquidityV2Args,
        accounts: DecreaseLiquidityV2Accounts,
    },
    Swap {
        args: SwapSingleArgs,
        accounts: SwapSingleAccounts,
    },
    SwapV2 {
        args: SwapSingleV2Args,
        accounts: SwapSingleV2Accounts,
    },
    Unknown {
        discriminator: [u8; 8],
    },
}

impl ParsedRaydiumV6Instruction {
    pub fn get_pool_state(&self) -> Option<Pubkey> {
        let pool_state = match self {
            ParsedRaydiumV6Instruction::CreatePool { args, accounts } => accounts.pool_state,
            ParsedRaydiumV6Instruction::DecreaseLiquidity { args, accounts } => accounts.pool_state,
            ParsedRaydiumV6Instruction::DecreaseLiquidityV2 { args, accounts } => {
                accounts.pool_state
            }
            ParsedRaydiumV6Instruction::IncreaseLiquidity { args, accounts } => accounts.pool_state,
            ParsedRaydiumV6Instruction::IncreaseLiquidityV2 { args, accounts } => {
                accounts.pool_state
            }
            ParsedRaydiumV6Instruction::Swap { args, accounts } => accounts.pool_state,
            ParsedRaydiumV6Instruction::SwapV2 { args, accounts } => accounts.pool_state,
            ParsedRaydiumV6Instruction::Unknown { discriminator } => return None,
        };
        Some(pool_state)
    }
}

pub(super) fn decode_raydium_v6_instruction(
    instruction: &CompiledInstruction,
    keys: &AccountKeys,
) -> Result<ParsedRaydiumV6Instruction> {
    log::debug!("Decoding RaydiumV6 instruction");
    let (discriminator, mut rest) = split_discriminator(&instruction.data);
    let mut keys = KeysWindow::new_from_instruction(keys, instruction);
    Ok(match discriminator {
        RAYDIUM_CLMM_CREATE_POOL_DISC => {
            let args = decode_instruction::<CreatePoolArgs>(&mut rest)?;
            let accounts = CreatePoolAccounts {
                pool_creator: keys.next_account_key()?,
                amm_config: keys.next_account_key()?,
                pool_state: keys.next_account_key()?,
                token_mint_0: keys.next_account_key()?,
                token_mint_1: keys.next_account_key()?,
                token_vault_0: keys.next_account_key()?,
                token_vault_1: keys.next_account_key()?,
                observation_state: keys.next_account_key()?,
                tick_array_bitmap: keys.next_account_key()?,
                token_program_0: keys.next_account_key()?,
                token_program_1: keys.next_account_key()?,
                system_program: keys.next_account_key()?,
                rent: keys.next_account_key()?,
            };
            ParsedRaydiumV6Instruction::CreatePool { args, accounts }
        }
        RAYDIUM_CLMM_DECREASE_LIQUIDITY_DISC => {
            let args = decode_instruction::<DecreaseLiquidityArgs>(&mut rest)?;
            let accounts = DecreaseLiquidityAccounts {
                nft_owner: keys.next_account_key()?,
                nft_account: keys.next_account_key()?,
                personal_position: keys.next_account_key()?,
                pool_state: keys.next_account_key()?,
                protocol_position: keys.next_account_key()?,
                token_vault_0: keys.next_account_key()?,
                token_vault_1: keys.next_account_key()?,
                tick_array_lower: keys.next_account_key()?,
                tick_array_upper: keys.next_account_key()?,
                recipient_token_account_0: keys.next_account_key()?,
                recipient_token_account_1: keys.next_account_key()?,
                token_program: keys.next_account_key()?,
            };
            ParsedRaydiumV6Instruction::DecreaseLiquidity { args, accounts }
        }
        RAYDIUM_CLMM_DECREASE_LIQUIDITY_V2_DISC => {
            let args = decode_instruction::<DecreaseLiquidityArgs>(&mut rest)?;
            let accounts = DecreaseLiquidityAccounts {
                nft_owner: keys.next_account_key()?,
                nft_account: keys.next_account_key()?,
                personal_position: keys.next_account_key()?,
                pool_state: keys.next_account_key()?,
                protocol_position: keys.next_account_key()?,
                token_vault_0: keys.next_account_key()?,
                token_vault_1: keys.next_account_key()?,
                tick_array_lower: keys.next_account_key()?,
                tick_array_upper: keys.next_account_key()?,
                recipient_token_account_0: keys.next_account_key()?,
                recipient_token_account_1: keys.next_account_key()?,
                token_program: keys.next_account_key()?,
            };
            ParsedRaydiumV6Instruction::DecreaseLiquidity { args, accounts }
        }
        RAYDIUM_CLMM_INCREASE_LIQUIDITY_DISC => {
            let args = decode_instruction::<IncreaseLiquidityArgs>(&mut rest)?;
            let accounts = IncreaseLiquidityAccounts {
                nft_owner: keys.next_account_key()?,
                nft_account: keys.next_account_key()?,
                pool_state: keys.next_account_key()?,
                protocol_position: keys.next_account_key()?,
                personal_position: keys.next_account_key()?,
                tick_array_lower: keys.next_account_key()?,
                tick_array_upper: keys.next_account_key()?,
                token_account_0: keys.next_account_key()?,
                token_account_1: keys.next_account_key()?,
                token_vault_0: keys.next_account_key()?,
                token_vault_1: keys.next_account_key()?,
                token_program: keys.next_account_key()?,
            };
            ParsedRaydiumV6Instruction::IncreaseLiquidity { args, accounts }
        }
        RAYDIUM_CLMM_INCREASE_LIQUIDITY_V2_DISC => {
            let args = decode_instruction::<IncreaseLiquidityV2Args>(&mut rest)?;
            let accounts = IncreaseLiquidityV2Accounts {
                nft_owner: keys.next_account_key()?,
                nft_account: keys.next_account_key()?,
                pool_state: keys.next_account_key()?,
                protocol_position: keys.next_account_key()?,
                personal_position: keys.next_account_key()?,
                tick_array_lower: keys.next_account_key()?,
                tick_array_upper: keys.next_account_key()?,
                token_account_0: keys.next_account_key()?,
                token_account_1: keys.next_account_key()?,
                token_vault_0: keys.next_account_key()?,
                token_vault_1: keys.next_account_key()?,
                token_program: keys.next_account_key()?,
                token_program_2022: keys.next_account_key()?,
                vault_0_mint: keys.next_account_key()?,
                vault_1_mint: keys.next_account_key()?,
            };
            ParsedRaydiumV6Instruction::IncreaseLiquidityV2 { args, accounts }
        }
        RAYDIUM_CLMM_SWAP_DISC => {
            let args = decode_instruction::<SwapSingleArgs>(&mut rest)?;
            let accounts = SwapSingleAccounts {
                payer: keys.next_account_key()?,
                amm_config: keys.next_account_key()?,
                pool_state: keys.next_account_key()?,
                input_token_account: keys.next_account_key()?,
                output_token_account: keys.next_account_key()?,
                input_vault: keys.next_account_key()?,
                output_vault: keys.next_account_key()?,
                observation_state: keys.next_account_key()?,
                token_program: keys.next_account_key()?,
                tick_array: keys.next_account_key()?,
            };
            ParsedRaydiumV6Instruction::Swap { args, accounts }
        }
        RAYDIUM_CLMM_SWAP_V2_DISC => {
            let args = decode_instruction::<SwapSingleV2Args>(&mut rest)?;
            let accounts = SwapSingleV2Accounts {
                payer: keys.next_account_key()?,
                amm_config: keys.next_account_key()?,
                pool_state: keys.next_account_key()?,
                input_token_account: keys.next_account_key()?,
                output_token_account: keys.next_account_key()?,
                input_vault: keys.next_account_key()?,
                output_vault: keys.next_account_key()?,
                observation_state: keys.next_account_key()?,
                token_program: keys.next_account_key()?,
                token_program_2022: keys.next_account_key()?,
                memo_program: keys.next_account_key()?,
                input_vault_mint: keys.next_account_key()?,
                output_vault_mint: keys.next_account_key()?,
            };
            ParsedRaydiumV6Instruction::SwapV2 { args, accounts }
        }
        _ => ParsedRaydiumV6Instruction::Unknown { discriminator },
    })
}

#[cfg(test)]
mod test {
    use super::super::anchor_instruction_sighash;
    use super::{
        RAYDIUM_CLMM_CREATE_POOL_DISC, RAYDIUM_CLMM_DECREASE_LIQUIDITY_DISC,
        RAYDIUM_CLMM_DECREASE_LIQUIDITY_V2_DISC, RAYDIUM_CLMM_INCREASE_LIQUIDITY_DISC,
        RAYDIUM_CLMM_INCREASE_LIQUIDITY_V2_DISC, RAYDIUM_CLMM_SWAP_DISC, RAYDIUM_CLMM_SWAP_V2_DISC,
    };

    #[test]
    fn raydium_v6_test_instruction_sighash() {
        let create_pool = anchor_instruction_sighash("create_pool");
        assert_eq!(create_pool, RAYDIUM_CLMM_CREATE_POOL_DISC);

        let decrease_liquidity = anchor_instruction_sighash("decrease_liquidity");
        assert_eq!(decrease_liquidity, RAYDIUM_CLMM_DECREASE_LIQUIDITY_DISC);

        let decrease_liquidity_v2 = anchor_instruction_sighash("decrease_liquidity_v2");
        assert_eq!(
            decrease_liquidity_v2,
            RAYDIUM_CLMM_DECREASE_LIQUIDITY_V2_DISC
        );

        let increase_liquidity = anchor_instruction_sighash("increase_liquidity");
        assert_eq!(increase_liquidity, RAYDIUM_CLMM_INCREASE_LIQUIDITY_DISC);

        let increase_liquidity_v2 = anchor_instruction_sighash("increase_liquidity_v2");
        assert_eq!(
            increase_liquidity_v2,
            RAYDIUM_CLMM_INCREASE_LIQUIDITY_V2_DISC
        );

        let swap = anchor_instruction_sighash("swap");
        assert_eq!(swap, RAYDIUM_CLMM_SWAP_DISC);

        let swap_v2 = anchor_instruction_sighash("swap_v2");
        assert_eq!(swap_v2, RAYDIUM_CLMM_SWAP_V2_DISC);
    }
}
