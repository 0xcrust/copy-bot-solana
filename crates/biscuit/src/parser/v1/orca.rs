use super::super::generated::orca_whirlpools::instructions::{
    DecreaseLiquidity, DecreaseLiquidityInstructionArgs, DecreaseLiquidityV2,
    DecreaseLiquidityV2InstructionArgs, IncreaseLiquidity, IncreaseLiquidityInstructionArgs,
    IncreaseLiquidityV2, IncreaseLiquidityV2InstructionArgs, InitializePool,
    InitializePoolInstructionArgs, InitializePoolV2, InitializePoolV2InstructionArgs, Swap,
    SwapInstructionArgs, SwapV2, SwapV2InstructionArgs,
};
use super::{decode_instruction, split_discriminator, KeysWindow};

use anyhow::Result;
use solana_sdk::instruction::CompiledInstruction;
use solana_sdk::message::AccountKeys;
use solana_sdk::pubkey::Pubkey;

const ORCA_INITIALIZE_POOL_DISC: [u8; 8] = [95, 180, 10, 172, 84, 174, 232, 40];
const ORCA_INITIALIZE_POOL_V2_DISC: [u8; 8] = [207, 45, 87, 242, 27, 63, 204, 67];
const ORCA_INCREASE_LIQUIDITY_DISC: [u8; 8] = [46, 156, 243, 118, 13, 205, 251, 178];
const ORCA_INCREASE_LIQUIDITY_V2_DISC: [u8; 8] = [133, 29, 89, 223, 69, 238, 176, 10];
const ORCA_DECREASE_LIQUIDITY_DISC: [u8; 8] = [160, 38, 208, 111, 104, 91, 44, 1];
const ORCA_DECREASE_LIQUIDITY_V2_DISC: [u8; 8] = [58, 127, 188, 62, 79, 82, 196, 96];
const ORCA_SWAP_DISC: [u8; 8] = [248, 198, 158, 145, 225, 117, 135, 200];
const ORCA_SWAP_V2_DISC: [u8; 8] = [43, 4, 237, 11, 26, 201, 30, 98];

pub enum ParsedOrcaInstruction {
    InitializePool {
        args: InitializePoolInstructionArgs,
        accounts: InitializePool,
    },
    InitializePoolV2 {
        args: InitializePoolV2InstructionArgs,
        accounts: InitializePoolV2,
    },
    IncreaseLiquidity {
        args: IncreaseLiquidityInstructionArgs,
        accounts: IncreaseLiquidity,
    },
    IncreaseLiquidityV2 {
        args: IncreaseLiquidityV2InstructionArgs,
        accounts: IncreaseLiquidityV2,
    },
    DecreaseLiquidity {
        args: DecreaseLiquidityInstructionArgs,
        accounts: DecreaseLiquidity,
    },
    DecreaseLiquidityV2 {
        args: DecreaseLiquidityV2InstructionArgs,
        accounts: DecreaseLiquidityV2,
    },
    Swap {
        args: SwapInstructionArgs,
        accounts: Swap,
    },
    SwapV2 {
        args: SwapV2InstructionArgs,
        accounts: SwapV2,
    },
    Unknown {
        discriminator: [u8; 8],
    },
}

impl ParsedOrcaInstruction {
    pub fn get_pool_state(&self) -> Option<Pubkey> {
        let state = match self {
            ParsedOrcaInstruction::DecreaseLiquidity { args, accounts } => accounts.whirlpool,
            ParsedOrcaInstruction::DecreaseLiquidityV2 { args, accounts } => accounts.whirlpool,
            ParsedOrcaInstruction::IncreaseLiquidity { args, accounts } => accounts.whirlpool,
            ParsedOrcaInstruction::IncreaseLiquidityV2 { args, accounts } => accounts.whirlpool,
            ParsedOrcaInstruction::InitializePool { args, accounts } => accounts.whirlpool,
            ParsedOrcaInstruction::InitializePoolV2 { args, accounts } => accounts.whirlpool,
            ParsedOrcaInstruction::Swap { args, accounts } => accounts.whirlpool,
            ParsedOrcaInstruction::SwapV2 { args, accounts } => accounts.whirlpool,
            ParsedOrcaInstruction::Unknown { discriminator } => return None,
        };
        Some(state)
    }
}

pub(super) fn decode_orca_instruction(
    instruction: &CompiledInstruction,
    keys: &AccountKeys,
) -> Result<ParsedOrcaInstruction> {
    log::debug!("Decoding orca instruction");
    let (discriminator, mut rest) = split_discriminator(&instruction.data);
    let mut keys = KeysWindow::new_from_instruction(keys, instruction);
    Ok(match discriminator {
        ORCA_SWAP_DISC => {
            let args = decode_instruction::<SwapInstructionArgs>(&mut rest)?;
            let accounts = Swap {
                token_program: keys.next_account_key()?,
                token_authority: keys.next_account_key()?,
                whirlpool: keys.next_account_key()?,
                token_owner_account_a: keys.next_account_key()?,
                token_vault_a: keys.next_account_key()?,
                token_owner_account_b: keys.next_account_key()?,
                token_vault_b: keys.next_account_key()?,
                tick_array0: keys.next_account_key()?,
                tick_array1: keys.next_account_key()?,
                tick_array2: keys.next_account_key()?,
                oracle: keys.next_account_key()?,
            };
            ParsedOrcaInstruction::Swap { args, accounts }
        }
        ORCA_SWAP_V2_DISC => {
            let args = decode_instruction::<SwapV2InstructionArgs>(&mut rest)?;
            let accounts = SwapV2 {
                token_program_a: keys.next_account_key()?,
                token_program_b: keys.next_account_key()?,
                memo_program: keys.next_account_key()?,
                token_authority: keys.next_account_key()?,
                whirlpool: keys.next_account_key()?,
                token_mint_a: keys.next_account_key()?,
                token_mint_b: keys.next_account_key()?,
                token_owner_account_a: keys.next_account_key()?,
                token_vault_a: keys.next_account_key()?,
                token_owner_account_b: keys.next_account_key()?,
                token_vault_b: keys.next_account_key()?,
                tick_array0: keys.next_account_key()?,
                tick_array1: keys.next_account_key()?,
                tick_array2: keys.next_account_key()?,
                oracle: keys.next_account_key()?,
            };
            ParsedOrcaInstruction::SwapV2 { args, accounts }
        }
        ORCA_INITIALIZE_POOL_DISC => {
            let args = decode_instruction::<InitializePoolInstructionArgs>(&mut rest)?;
            let accounts = InitializePool {
                whirlpools_config: keys.next_account_key()?,
                token_mint_a: keys.next_account_key()?,
                token_mint_b: keys.next_account_key()?,
                funder: keys.next_account_key()?,
                whirlpool: keys.next_account_key()?,
                token_vault_a: keys.next_account_key()?,
                token_vault_b: keys.next_account_key()?,
                fee_tier: keys.next_account_key()?,
                token_program: keys.next_account_key()?,
                system_program: keys.next_account_key()?,
                rent: keys.next_account_key()?,
            };
            ParsedOrcaInstruction::InitializePool { args, accounts }
        }
        ORCA_INITIALIZE_POOL_V2_DISC => {
            let args = decode_instruction::<InitializePoolV2InstructionArgs>(&mut rest)?;
            let accounts = InitializePoolV2 {
                whirlpools_config: keys.next_account_key()?,
                token_mint_a: keys.next_account_key()?,
                token_mint_b: keys.next_account_key()?,
                token_badge_a: keys.next_account_key()?,
                token_badge_b: keys.next_account_key()?,
                funder: keys.next_account_key()?,
                whirlpool: keys.next_account_key()?,
                token_vault_a: keys.next_account_key()?,
                token_vault_b: keys.next_account_key()?,
                fee_tier: keys.next_account_key()?,
                token_program_a: keys.next_account_key()?,
                token_program_b: keys.next_account_key()?,
                system_program: keys.next_account_key()?,
                rent: keys.next_account_key()?,
            };
            ParsedOrcaInstruction::InitializePoolV2 { args, accounts }
        }
        ORCA_INCREASE_LIQUIDITY_DISC => {
            let args = decode_instruction::<IncreaseLiquidityInstructionArgs>(&mut rest)?;
            let accounts = IncreaseLiquidity {
                whirlpool: keys.next_account_key()?,
                token_program: keys.next_account_key()?,
                position_authority: keys.next_account_key()?,
                position: keys.next_account_key()?,
                position_token_account: keys.next_account_key()?,
                token_owner_account_a: keys.next_account_key()?,
                token_owner_account_b: keys.next_account_key()?,
                token_vault_a: keys.next_account_key()?,
                token_vault_b: keys.next_account_key()?,
                tick_array_lower: keys.next_account_key()?,
                tick_array_upper: keys.next_account_key()?,
            };
            ParsedOrcaInstruction::IncreaseLiquidity { args, accounts }
        }
        ORCA_INCREASE_LIQUIDITY_V2_DISC => {
            let args = decode_instruction::<IncreaseLiquidityV2InstructionArgs>(&mut rest)?;
            let accounts = IncreaseLiquidityV2 {
                whirlpool: keys.next_account_key()?,
                token_program_a: keys.next_account_key()?,
                token_program_b: keys.next_account_key()?,
                memo_program: keys.next_account_key()?,
                position_authority: keys.next_account_key()?,
                position: keys.next_account_key()?,
                position_token_account: keys.next_account_key()?,
                token_mint_a: keys.next_account_key()?,
                token_mint_b: keys.next_account_key()?,
                token_owner_account_a: keys.next_account_key()?,
                token_owner_account_b: keys.next_account_key()?,
                token_vault_a: keys.next_account_key()?,
                token_vault_b: keys.next_account_key()?,
                tick_array_lower: keys.next_account_key()?,
                tick_array_upper: keys.next_account_key()?,
            };
            ParsedOrcaInstruction::IncreaseLiquidityV2 { args, accounts }
        }
        ORCA_DECREASE_LIQUIDITY_DISC => {
            let args = decode_instruction::<DecreaseLiquidityInstructionArgs>(&mut rest)?;
            let accounts = DecreaseLiquidity {
                whirlpool: keys.next_account_key()?,
                token_program: keys.next_account_key()?,
                position_authority: keys.next_account_key()?,
                position: keys.next_account_key()?,
                position_token_account: keys.next_account_key()?,
                token_owner_account_a: keys.next_account_key()?,
                token_owner_account_b: keys.next_account_key()?,
                token_vault_a: keys.next_account_key()?,
                token_vault_b: keys.next_account_key()?,
                tick_array_lower: keys.next_account_key()?,
                tick_array_upper: keys.next_account_key()?,
            };
            ParsedOrcaInstruction::DecreaseLiquidity { args, accounts }
        }
        ORCA_DECREASE_LIQUIDITY_V2_DISC => {
            let args = decode_instruction::<DecreaseLiquidityV2InstructionArgs>(&mut rest)?;
            let accounts = DecreaseLiquidityV2 {
                whirlpool: keys.next_account_key()?,
                token_program_a: keys.next_account_key()?,
                token_program_b: keys.next_account_key()?,
                memo_program: keys.next_account_key()?,
                position_authority: keys.next_account_key()?,
                position: keys.next_account_key()?,
                position_token_account: keys.next_account_key()?,
                token_mint_a: keys.next_account_key()?,
                token_mint_b: keys.next_account_key()?,
                token_owner_account_a: keys.next_account_key()?,
                token_owner_account_b: keys.next_account_key()?,
                token_vault_a: keys.next_account_key()?,
                token_vault_b: keys.next_account_key()?,
                tick_array_lower: keys.next_account_key()?,
                tick_array_upper: keys.next_account_key()?,
            };
            ParsedOrcaInstruction::DecreaseLiquidityV2 { args, accounts }
        }
        _ => ParsedOrcaInstruction::Unknown { discriminator },
    })
}

#[cfg(test)]
mod test {
    use super::super::anchor_instruction_sighash;
    use super::{
        ORCA_DECREASE_LIQUIDITY_DISC, ORCA_DECREASE_LIQUIDITY_V2_DISC,
        ORCA_INCREASE_LIQUIDITY_DISC, ORCA_INCREASE_LIQUIDITY_V2_DISC, ORCA_INITIALIZE_POOL_DISC,
        ORCA_INITIALIZE_POOL_V2_DISC, ORCA_SWAP_DISC, ORCA_SWAP_V2_DISC,
    };

    #[test]
    fn orca_test_instruction_sighash() {
        let init_pool = anchor_instruction_sighash("initialize_pool");
        assert_eq!(init_pool, ORCA_INITIALIZE_POOL_DISC);

        let init_pool_v2 = anchor_instruction_sighash("initialize_pool_v2");
        assert_eq!(init_pool_v2, ORCA_INITIALIZE_POOL_V2_DISC);

        let increase_liquidity = anchor_instruction_sighash("increase_liquidity");
        assert_eq!(increase_liquidity, ORCA_INCREASE_LIQUIDITY_DISC);

        let increase_liquidity_v2 = anchor_instruction_sighash("increase_liquidity_v2");
        assert_eq!(increase_liquidity_v2, ORCA_INCREASE_LIQUIDITY_V2_DISC);

        let decrease_liquidity = anchor_instruction_sighash("decrease_liquidity");
        assert_eq!(decrease_liquidity, ORCA_DECREASE_LIQUIDITY_DISC);

        let decrease_liquidity_v2 = anchor_instruction_sighash("decrease_liquidity_v2");
        assert_eq!(decrease_liquidity_v2, ORCA_DECREASE_LIQUIDITY_V2_DISC);

        let swap = anchor_instruction_sighash("swap");
        assert_eq!(swap, ORCA_SWAP_DISC);

        let swap_v2 = anchor_instruction_sighash("swap_v2");
        assert_eq!(swap_v2, ORCA_SWAP_V2_DISC);
    }
}
