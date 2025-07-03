use anchor_lang::prelude::*;

declare_id!("HaTeDoqmbTpXPP5bGXoNPXe2Ze2YAs1qM9KVUE5DR9WE");

pub const OTHER_WALLET_PREFIX: &[u8] = b"other-wallet-memo";
pub const OTHER_WALLET_FOR_MINT_PREFIX: &[u8] = b"other-wallet-for-mint-memo";

pub fn derive_other_wallet_memo(
    program_id: &Pubkey,
    initiator: &Pubkey,
    other_wallet: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            OTHER_WALLET_PREFIX,
            initiator.as_ref(),
            other_wallet.as_ref(),
        ],
        program_id,
    )
}

pub fn derive_other_wallet_for_mint_memo(
    program_id: &Pubkey,
    initiator: &Pubkey,
    other_wallet: &Pubkey,
    mint: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            OTHER_WALLET_FOR_MINT_PREFIX,
            initiator.as_ref(),
            other_wallet.as_ref(),
            mint.as_ref(),
        ],
        program_id,
    )
}

#[program]
pub mod copy {
    use super::*;

    pub fn hack_the_system(ctx: Context<HackTheSystemAccounts>, event: SwapEvent) -> Result<()> {
        require_keys_eq!(event.initiator, ctx.accounts.initiator.key());
        emit!(event);
        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(event: SwapEvent)]
pub struct HackTheSystemAccounts<'info> {
    #[account(mut)]
    pub initiator: Signer<'info>,
    /// CHECK: Just for fun
    #[account(
        seeds = [
            OTHER_WALLET_PREFIX,
            initiator.key().as_ref(),
            event.other.as_ref()
        ],
        bump
    )]
    pub other_wallet_memo: UncheckedAccount<'info>,
    /// CHECK: Just for fun
    #[account(
        seeds = [
            OTHER_WALLET_FOR_MINT_PREFIX,
            initiator.key().as_ref(),
            event.other.key().as_ref(),
            event.mint.as_ref(),
        ],
        bump
    )]
    pub other_wallet_for_mint_memo: UncheckedAccount<'info>,
}

#[event]
#[derive(Copy, Clone, Debug)]
pub struct SwapEvent {
    pub initiator: Pubkey,
    pub other: Pubkey,
    pub swap: Swap,
    pub mint: Pubkey,
    pub signature: [u8; 64],
    // TODO: Add slot and block-time of the original transaction to identify and avoid duplicates
}

#[derive(AnchorDeserialize, AnchorSerialize, Copy, Clone, Debug)]
pub enum Swap {
    Buy {
        other_input_mint: Pubkey,
        initiator_input_mint: Pubkey,
        other_amount: u64,
        initiator_amount: u64,
    },
    Sell {
        other_output_mint: Pubkey,
        initiator_output_mint: Pubkey,
        other_amount: u64,
        initiator_amount: u64,
    },
}
