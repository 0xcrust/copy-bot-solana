//! This code was AUTOGENERATED using the kinobi library.
//! Please DO NOT EDIT THIS FILE, instead use visitors
//! to add features, then rerun kinobi to update it.
//!
//! [https://github.com/metaplex-foundation/kinobi]
//!

#[cfg(feature = "anchor")]
use anchor_lang::prelude::{AnchorDeserialize, AnchorSerialize};
#[cfg(not(feature = "anchor"))]
use borsh::{BorshDeserialize, BorshSerialize};

/// Accounts.
pub struct Buy {
    pub global: solana_program::pubkey::Pubkey,

    pub fee_recipient: solana_program::pubkey::Pubkey,

    pub mint: solana_program::pubkey::Pubkey,

    pub bonding_curve: solana_program::pubkey::Pubkey,

    pub associated_bonding_curve: solana_program::pubkey::Pubkey,

    pub associated_user: solana_program::pubkey::Pubkey,

    pub user: solana_program::pubkey::Pubkey,

    pub system_program: solana_program::pubkey::Pubkey,

    pub token_program: solana_program::pubkey::Pubkey,

    pub rent: solana_program::pubkey::Pubkey,

    pub event_authority: solana_program::pubkey::Pubkey,

    pub program: solana_program::pubkey::Pubkey,
}

impl Buy {
    pub fn instruction(
        &self,
        args: BuyInstructionArgs,
    ) -> solana_program::instruction::Instruction {
        self.instruction_with_remaining_accounts(args, &[])
    }
    #[allow(clippy::vec_init_then_push)]
    pub fn instruction_with_remaining_accounts(
        &self,
        args: BuyInstructionArgs,
        remaining_accounts: &[solana_program::instruction::AccountMeta],
    ) -> solana_program::instruction::Instruction {
        let mut accounts = Vec::with_capacity(12 + remaining_accounts.len());
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            self.global,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new(
            self.fee_recipient,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            self.mint, false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new(
            self.bonding_curve,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new(
            self.associated_bonding_curve,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new(
            self.associated_user,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new(
            self.user, true,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            self.system_program,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            self.token_program,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            self.rent, false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            self.event_authority,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            self.program,
            false,
        ));
        accounts.extend_from_slice(remaining_accounts);
        let mut data = BuyInstructionData::new().try_to_vec().unwrap();
        let mut args = args.try_to_vec().unwrap();
        data.append(&mut args);

        solana_program::instruction::Instruction {
            program_id: crate::PUMP_ID,
            accounts,
            data,
        }
    }
}

#[cfg_attr(not(feature = "anchor"), derive(BorshSerialize, BorshDeserialize))]
#[cfg_attr(feature = "anchor", derive(AnchorSerialize, AnchorDeserialize))]
pub struct BuyInstructionData {
    discriminator: [u8; 8],
}

impl BuyInstructionData {
    pub fn new() -> Self {
        Self {
            discriminator: [102, 6, 61, 18, 1, 218, 235, 234],
        }
    }
}

#[cfg_attr(not(feature = "anchor"), derive(BorshSerialize, BorshDeserialize))]
#[cfg_attr(feature = "anchor", derive(AnchorSerialize, AnchorDeserialize))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuyInstructionArgs {
    pub amount: u64,
    pub max_sol_cost: u64,
}

/// Instruction builder for `Buy`.
///
/// ### Accounts:
///
///   0. `[]` global
///   1. `[writable]` fee_recipient
///   2. `[]` mint
///   3. `[writable]` bonding_curve
///   4. `[writable]` associated_bonding_curve
///   5. `[writable]` associated_user
///   6. `[writable, signer]` user
///   7. `[optional]` system_program (default to `11111111111111111111111111111111`)
///   8. `[optional]` token_program (default to `TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA`)
///   9. `[optional]` rent (default to `SysvarRent111111111111111111111111111111111`)
///   10. `[]` event_authority
///   11. `[]` program
#[derive(Default)]
pub struct BuyBuilder {
    global: Option<solana_program::pubkey::Pubkey>,
    fee_recipient: Option<solana_program::pubkey::Pubkey>,
    mint: Option<solana_program::pubkey::Pubkey>,
    bonding_curve: Option<solana_program::pubkey::Pubkey>,
    associated_bonding_curve: Option<solana_program::pubkey::Pubkey>,
    associated_user: Option<solana_program::pubkey::Pubkey>,
    user: Option<solana_program::pubkey::Pubkey>,
    system_program: Option<solana_program::pubkey::Pubkey>,
    token_program: Option<solana_program::pubkey::Pubkey>,
    rent: Option<solana_program::pubkey::Pubkey>,
    event_authority: Option<solana_program::pubkey::Pubkey>,
    program: Option<solana_program::pubkey::Pubkey>,
    amount: Option<u64>,
    max_sol_cost: Option<u64>,
    __remaining_accounts: Vec<solana_program::instruction::AccountMeta>,
}

impl BuyBuilder {
    pub fn new() -> Self {
        Self::default()
    }
    #[inline(always)]
    pub fn global(&mut self, global: solana_program::pubkey::Pubkey) -> &mut Self {
        self.global = Some(global);
        self
    }
    #[inline(always)]
    pub fn fee_recipient(&mut self, fee_recipient: solana_program::pubkey::Pubkey) -> &mut Self {
        self.fee_recipient = Some(fee_recipient);
        self
    }
    #[inline(always)]
    pub fn mint(&mut self, mint: solana_program::pubkey::Pubkey) -> &mut Self {
        self.mint = Some(mint);
        self
    }
    #[inline(always)]
    pub fn bonding_curve(&mut self, bonding_curve: solana_program::pubkey::Pubkey) -> &mut Self {
        self.bonding_curve = Some(bonding_curve);
        self
    }
    #[inline(always)]
    pub fn associated_bonding_curve(
        &mut self,
        associated_bonding_curve: solana_program::pubkey::Pubkey,
    ) -> &mut Self {
        self.associated_bonding_curve = Some(associated_bonding_curve);
        self
    }
    #[inline(always)]
    pub fn associated_user(
        &mut self,
        associated_user: solana_program::pubkey::Pubkey,
    ) -> &mut Self {
        self.associated_user = Some(associated_user);
        self
    }
    #[inline(always)]
    pub fn user(&mut self, user: solana_program::pubkey::Pubkey) -> &mut Self {
        self.user = Some(user);
        self
    }
    /// `[optional account, default to '11111111111111111111111111111111']`
    #[inline(always)]
    pub fn system_program(&mut self, system_program: solana_program::pubkey::Pubkey) -> &mut Self {
        self.system_program = Some(system_program);
        self
    }
    /// `[optional account, default to 'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA']`
    #[inline(always)]
    pub fn token_program(&mut self, token_program: solana_program::pubkey::Pubkey) -> &mut Self {
        self.token_program = Some(token_program);
        self
    }
    /// `[optional account, default to 'SysvarRent111111111111111111111111111111111']`
    #[inline(always)]
    pub fn rent(&mut self, rent: solana_program::pubkey::Pubkey) -> &mut Self {
        self.rent = Some(rent);
        self
    }
    #[inline(always)]
    pub fn event_authority(
        &mut self,
        event_authority: solana_program::pubkey::Pubkey,
    ) -> &mut Self {
        self.event_authority = Some(event_authority);
        self
    }
    #[inline(always)]
    pub fn program(&mut self, program: solana_program::pubkey::Pubkey) -> &mut Self {
        self.program = Some(program);
        self
    }
    #[inline(always)]
    pub fn amount(&mut self, amount: u64) -> &mut Self {
        self.amount = Some(amount);
        self
    }
    #[inline(always)]
    pub fn max_sol_cost(&mut self, max_sol_cost: u64) -> &mut Self {
        self.max_sol_cost = Some(max_sol_cost);
        self
    }
    /// Add an aditional account to the instruction.
    #[inline(always)]
    pub fn add_remaining_account(
        &mut self,
        account: solana_program::instruction::AccountMeta,
    ) -> &mut Self {
        self.__remaining_accounts.push(account);
        self
    }
    /// Add additional accounts to the instruction.
    #[inline(always)]
    pub fn add_remaining_accounts(
        &mut self,
        accounts: &[solana_program::instruction::AccountMeta],
    ) -> &mut Self {
        self.__remaining_accounts.extend_from_slice(accounts);
        self
    }
    #[allow(clippy::clone_on_copy)]
    pub fn instruction(&self) -> solana_program::instruction::Instruction {
        let accounts = Buy {
            global: self.global.expect("global is not set"),
            fee_recipient: self.fee_recipient.expect("fee_recipient is not set"),
            mint: self.mint.expect("mint is not set"),
            bonding_curve: self.bonding_curve.expect("bonding_curve is not set"),
            associated_bonding_curve: self
                .associated_bonding_curve
                .expect("associated_bonding_curve is not set"),
            associated_user: self.associated_user.expect("associated_user is not set"),
            user: self.user.expect("user is not set"),
            system_program: self
                .system_program
                .unwrap_or(solana_program::pubkey!("11111111111111111111111111111111")),
            token_program: self.token_program.unwrap_or(solana_program::pubkey!(
                "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
            )),
            rent: self.rent.unwrap_or(solana_program::pubkey!(
                "SysvarRent111111111111111111111111111111111"
            )),
            event_authority: self.event_authority.expect("event_authority is not set"),
            program: self.program.expect("program is not set"),
        };
        let args = BuyInstructionArgs {
            amount: self.amount.clone().expect("amount is not set"),
            max_sol_cost: self.max_sol_cost.clone().expect("max_sol_cost is not set"),
        };

        accounts.instruction_with_remaining_accounts(args, &self.__remaining_accounts)
    }
}

/// `buy` CPI accounts.
pub struct BuyCpiAccounts<'a, 'b> {
    pub global: &'b solana_program::account_info::AccountInfo<'a>,

    pub fee_recipient: &'b solana_program::account_info::AccountInfo<'a>,

    pub mint: &'b solana_program::account_info::AccountInfo<'a>,

    pub bonding_curve: &'b solana_program::account_info::AccountInfo<'a>,

    pub associated_bonding_curve: &'b solana_program::account_info::AccountInfo<'a>,

    pub associated_user: &'b solana_program::account_info::AccountInfo<'a>,

    pub user: &'b solana_program::account_info::AccountInfo<'a>,

    pub system_program: &'b solana_program::account_info::AccountInfo<'a>,

    pub token_program: &'b solana_program::account_info::AccountInfo<'a>,

    pub rent: &'b solana_program::account_info::AccountInfo<'a>,

    pub event_authority: &'b solana_program::account_info::AccountInfo<'a>,

    pub program: &'b solana_program::account_info::AccountInfo<'a>,
}

/// `buy` CPI instruction.
pub struct BuyCpi<'a, 'b> {
    /// The program to invoke.
    pub __program: &'b solana_program::account_info::AccountInfo<'a>,

    pub global: &'b solana_program::account_info::AccountInfo<'a>,

    pub fee_recipient: &'b solana_program::account_info::AccountInfo<'a>,

    pub mint: &'b solana_program::account_info::AccountInfo<'a>,

    pub bonding_curve: &'b solana_program::account_info::AccountInfo<'a>,

    pub associated_bonding_curve: &'b solana_program::account_info::AccountInfo<'a>,

    pub associated_user: &'b solana_program::account_info::AccountInfo<'a>,

    pub user: &'b solana_program::account_info::AccountInfo<'a>,

    pub system_program: &'b solana_program::account_info::AccountInfo<'a>,

    pub token_program: &'b solana_program::account_info::AccountInfo<'a>,

    pub rent: &'b solana_program::account_info::AccountInfo<'a>,

    pub event_authority: &'b solana_program::account_info::AccountInfo<'a>,

    pub program: &'b solana_program::account_info::AccountInfo<'a>,
    /// The arguments for the instruction.
    pub __args: BuyInstructionArgs,
}

impl<'a, 'b> BuyCpi<'a, 'b> {
    pub fn new(
        program: &'b solana_program::account_info::AccountInfo<'a>,
        accounts: BuyCpiAccounts<'a, 'b>,
        args: BuyInstructionArgs,
    ) -> Self {
        Self {
            __program: program,
            global: accounts.global,
            fee_recipient: accounts.fee_recipient,
            mint: accounts.mint,
            bonding_curve: accounts.bonding_curve,
            associated_bonding_curve: accounts.associated_bonding_curve,
            associated_user: accounts.associated_user,
            user: accounts.user,
            system_program: accounts.system_program,
            token_program: accounts.token_program,
            rent: accounts.rent,
            event_authority: accounts.event_authority,
            program: accounts.program,
            __args: args,
        }
    }
    #[inline(always)]
    pub fn invoke(&self) -> solana_program::entrypoint::ProgramResult {
        self.invoke_signed_with_remaining_accounts(&[], &[])
    }
    #[inline(always)]
    pub fn invoke_with_remaining_accounts(
        &self,
        remaining_accounts: &[(
            &'b solana_program::account_info::AccountInfo<'a>,
            bool,
            bool,
        )],
    ) -> solana_program::entrypoint::ProgramResult {
        self.invoke_signed_with_remaining_accounts(&[], remaining_accounts)
    }
    #[inline(always)]
    pub fn invoke_signed(
        &self,
        signers_seeds: &[&[&[u8]]],
    ) -> solana_program::entrypoint::ProgramResult {
        self.invoke_signed_with_remaining_accounts(signers_seeds, &[])
    }
    #[allow(clippy::clone_on_copy)]
    #[allow(clippy::vec_init_then_push)]
    pub fn invoke_signed_with_remaining_accounts(
        &self,
        signers_seeds: &[&[&[u8]]],
        remaining_accounts: &[(
            &'b solana_program::account_info::AccountInfo<'a>,
            bool,
            bool,
        )],
    ) -> solana_program::entrypoint::ProgramResult {
        let mut accounts = Vec::with_capacity(12 + remaining_accounts.len());
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            *self.global.key,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new(
            *self.fee_recipient.key,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            *self.mint.key,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new(
            *self.bonding_curve.key,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new(
            *self.associated_bonding_curve.key,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new(
            *self.associated_user.key,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new(
            *self.user.key,
            true,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            *self.system_program.key,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            *self.token_program.key,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            *self.rent.key,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            *self.event_authority.key,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            *self.program.key,
            false,
        ));
        remaining_accounts.iter().for_each(|remaining_account| {
            accounts.push(solana_program::instruction::AccountMeta {
                pubkey: *remaining_account.0.key,
                is_signer: remaining_account.1,
                is_writable: remaining_account.2,
            })
        });
        let mut data = BuyInstructionData::new().try_to_vec().unwrap();
        let mut args = self.__args.try_to_vec().unwrap();
        data.append(&mut args);

        let instruction = solana_program::instruction::Instruction {
            program_id: crate::PUMP_ID,
            accounts,
            data,
        };
        let mut account_infos = Vec::with_capacity(12 + 1 + remaining_accounts.len());
        account_infos.push(self.__program.clone());
        account_infos.push(self.global.clone());
        account_infos.push(self.fee_recipient.clone());
        account_infos.push(self.mint.clone());
        account_infos.push(self.bonding_curve.clone());
        account_infos.push(self.associated_bonding_curve.clone());
        account_infos.push(self.associated_user.clone());
        account_infos.push(self.user.clone());
        account_infos.push(self.system_program.clone());
        account_infos.push(self.token_program.clone());
        account_infos.push(self.rent.clone());
        account_infos.push(self.event_authority.clone());
        account_infos.push(self.program.clone());
        remaining_accounts
            .iter()
            .for_each(|remaining_account| account_infos.push(remaining_account.0.clone()));

        if signers_seeds.is_empty() {
            solana_program::program::invoke(&instruction, &account_infos)
        } else {
            solana_program::program::invoke_signed(&instruction, &account_infos, signers_seeds)
        }
    }
}

/// Instruction builder for `Buy` via CPI.
///
/// ### Accounts:
///
///   0. `[]` global
///   1. `[writable]` fee_recipient
///   2. `[]` mint
///   3. `[writable]` bonding_curve
///   4. `[writable]` associated_bonding_curve
///   5. `[writable]` associated_user
///   6. `[writable, signer]` user
///   7. `[]` system_program
///   8. `[]` token_program
///   9. `[]` rent
///   10. `[]` event_authority
///   11. `[]` program
pub struct BuyCpiBuilder<'a, 'b> {
    instruction: Box<BuyCpiBuilderInstruction<'a, 'b>>,
}

impl<'a, 'b> BuyCpiBuilder<'a, 'b> {
    pub fn new(program: &'b solana_program::account_info::AccountInfo<'a>) -> Self {
        let instruction = Box::new(BuyCpiBuilderInstruction {
            __program: program,
            global: None,
            fee_recipient: None,
            mint: None,
            bonding_curve: None,
            associated_bonding_curve: None,
            associated_user: None,
            user: None,
            system_program: None,
            token_program: None,
            rent: None,
            event_authority: None,
            program: None,
            amount: None,
            max_sol_cost: None,
            __remaining_accounts: Vec::new(),
        });
        Self { instruction }
    }
    #[inline(always)]
    pub fn global(
        &mut self,
        global: &'b solana_program::account_info::AccountInfo<'a>,
    ) -> &mut Self {
        self.instruction.global = Some(global);
        self
    }
    #[inline(always)]
    pub fn fee_recipient(
        &mut self,
        fee_recipient: &'b solana_program::account_info::AccountInfo<'a>,
    ) -> &mut Self {
        self.instruction.fee_recipient = Some(fee_recipient);
        self
    }
    #[inline(always)]
    pub fn mint(&mut self, mint: &'b solana_program::account_info::AccountInfo<'a>) -> &mut Self {
        self.instruction.mint = Some(mint);
        self
    }
    #[inline(always)]
    pub fn bonding_curve(
        &mut self,
        bonding_curve: &'b solana_program::account_info::AccountInfo<'a>,
    ) -> &mut Self {
        self.instruction.bonding_curve = Some(bonding_curve);
        self
    }
    #[inline(always)]
    pub fn associated_bonding_curve(
        &mut self,
        associated_bonding_curve: &'b solana_program::account_info::AccountInfo<'a>,
    ) -> &mut Self {
        self.instruction.associated_bonding_curve = Some(associated_bonding_curve);
        self
    }
    #[inline(always)]
    pub fn associated_user(
        &mut self,
        associated_user: &'b solana_program::account_info::AccountInfo<'a>,
    ) -> &mut Self {
        self.instruction.associated_user = Some(associated_user);
        self
    }
    #[inline(always)]
    pub fn user(&mut self, user: &'b solana_program::account_info::AccountInfo<'a>) -> &mut Self {
        self.instruction.user = Some(user);
        self
    }
    #[inline(always)]
    pub fn system_program(
        &mut self,
        system_program: &'b solana_program::account_info::AccountInfo<'a>,
    ) -> &mut Self {
        self.instruction.system_program = Some(system_program);
        self
    }
    #[inline(always)]
    pub fn token_program(
        &mut self,
        token_program: &'b solana_program::account_info::AccountInfo<'a>,
    ) -> &mut Self {
        self.instruction.token_program = Some(token_program);
        self
    }
    #[inline(always)]
    pub fn rent(&mut self, rent: &'b solana_program::account_info::AccountInfo<'a>) -> &mut Self {
        self.instruction.rent = Some(rent);
        self
    }
    #[inline(always)]
    pub fn event_authority(
        &mut self,
        event_authority: &'b solana_program::account_info::AccountInfo<'a>,
    ) -> &mut Self {
        self.instruction.event_authority = Some(event_authority);
        self
    }
    #[inline(always)]
    pub fn program(
        &mut self,
        program: &'b solana_program::account_info::AccountInfo<'a>,
    ) -> &mut Self {
        self.instruction.program = Some(program);
        self
    }
    #[inline(always)]
    pub fn amount(&mut self, amount: u64) -> &mut Self {
        self.instruction.amount = Some(amount);
        self
    }
    #[inline(always)]
    pub fn max_sol_cost(&mut self, max_sol_cost: u64) -> &mut Self {
        self.instruction.max_sol_cost = Some(max_sol_cost);
        self
    }
    /// Add an additional account to the instruction.
    #[inline(always)]
    pub fn add_remaining_account(
        &mut self,
        account: &'b solana_program::account_info::AccountInfo<'a>,
        is_writable: bool,
        is_signer: bool,
    ) -> &mut Self {
        self.instruction
            .__remaining_accounts
            .push((account, is_writable, is_signer));
        self
    }
    /// Add additional accounts to the instruction.
    ///
    /// Each account is represented by a tuple of the `AccountInfo`, a `bool` indicating whether the account is writable or not,
    /// and a `bool` indicating whether the account is a signer or not.
    #[inline(always)]
    pub fn add_remaining_accounts(
        &mut self,
        accounts: &[(
            &'b solana_program::account_info::AccountInfo<'a>,
            bool,
            bool,
        )],
    ) -> &mut Self {
        self.instruction
            .__remaining_accounts
            .extend_from_slice(accounts);
        self
    }
    #[inline(always)]
    pub fn invoke(&self) -> solana_program::entrypoint::ProgramResult {
        self.invoke_signed(&[])
    }
    #[allow(clippy::clone_on_copy)]
    #[allow(clippy::vec_init_then_push)]
    pub fn invoke_signed(
        &self,
        signers_seeds: &[&[&[u8]]],
    ) -> solana_program::entrypoint::ProgramResult {
        let args = BuyInstructionArgs {
            amount: self.instruction.amount.clone().expect("amount is not set"),
            max_sol_cost: self
                .instruction
                .max_sol_cost
                .clone()
                .expect("max_sol_cost is not set"),
        };
        let instruction = BuyCpi {
            __program: self.instruction.__program,

            global: self.instruction.global.expect("global is not set"),

            fee_recipient: self
                .instruction
                .fee_recipient
                .expect("fee_recipient is not set"),

            mint: self.instruction.mint.expect("mint is not set"),

            bonding_curve: self
                .instruction
                .bonding_curve
                .expect("bonding_curve is not set"),

            associated_bonding_curve: self
                .instruction
                .associated_bonding_curve
                .expect("associated_bonding_curve is not set"),

            associated_user: self
                .instruction
                .associated_user
                .expect("associated_user is not set"),

            user: self.instruction.user.expect("user is not set"),

            system_program: self
                .instruction
                .system_program
                .expect("system_program is not set"),

            token_program: self
                .instruction
                .token_program
                .expect("token_program is not set"),

            rent: self.instruction.rent.expect("rent is not set"),

            event_authority: self
                .instruction
                .event_authority
                .expect("event_authority is not set"),

            program: self.instruction.program.expect("program is not set"),
            __args: args,
        };
        instruction.invoke_signed_with_remaining_accounts(
            signers_seeds,
            &self.instruction.__remaining_accounts,
        )
    }
}

struct BuyCpiBuilderInstruction<'a, 'b> {
    __program: &'b solana_program::account_info::AccountInfo<'a>,
    global: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    fee_recipient: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    mint: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    bonding_curve: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    associated_bonding_curve: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    associated_user: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    user: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    system_program: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    token_program: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    rent: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    event_authority: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    program: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    amount: Option<u64>,
    max_sol_cost: Option<u64>,
    /// Additional instruction accounts `(AccountInfo, is_writable, is_signer)`.
    __remaining_accounts: Vec<(
        &'b solana_program::account_info::AccountInfo<'a>,
        bool,
        bool,
    )>,
}
