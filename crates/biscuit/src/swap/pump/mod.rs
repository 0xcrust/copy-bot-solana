// https://medium.com/@buildwithbhavya/the-math-behind-pump-fun-b58fdb30ed77
pub mod bonding_curve;
pub mod executor;

use crate::constants::programs::PUMPFUN_PROGRAM_ID;
use crate::parser::generated::pump::accounts::{BondingCurve, Global};
use crate::parser::generated::pump::instructions::{BuyBuilder, SellBuilder};

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::instruction::Instruction;
use solana_sdk::{pubkey, pubkey::Pubkey};
use spl_associated_token_account::get_associated_token_address;
use std::sync::Arc;

pub const EVENT_AUTHORITY: Pubkey = pubkey!("Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1");

pub struct PFClient {
    pub client: Arc<RpcClient>,
    pub address: Pubkey,
    pub protocol: Global,
    pub config: PFConfig,
}

pub struct PFConfig {
    pub refresh_interval_seconds: u64,
}

pub struct PFQuote {
    /// The amount of tokens to buy or sell
    amount: u64,
    /// The amount of SOL that would be spent or gotten from the swap, without adjusting for slippage
    sol_amount: u64,
    /// The SOL amount with slippage
    sol_amount_with_slippage: u64,
    /// The input mint, SOL for buys
    input_mint: Pubkey,
    /// The output mint, SOL for sells
    output_mint: Pubkey,
}

impl PFClient {
    pub fn new_with_config(client: Arc<RpcClient>, config: PFConfig) -> Self {
        let address = Self::get_global_account_pda();
        PFClient {
            client,
            address,
            protocol: Global::default(),
            config,
        }
    }

    pub fn initial_buy_price(&self, amount: u64) -> u64 {
        self.protocol.get_initial_buy_price(amount)
    }

    pub async fn get_buy_quote(
        &self,
        mint: &Pubkey,
        sol_amount: u64,
        slippage_bps: u16,
    ) -> anyhow::Result<PFQuote> {
        let (bonding_curve, _) = self.get_bonding_curve_account_for_mint(mint).await?;
        let amount = bonding_curve.get_buy_price(sol_amount)?;
        Ok(PFQuote {
            amount,
            sol_amount,
            sol_amount_with_slippage: Self::buy_with_slippage(sol_amount, slippage_bps),
            input_mint: spl_token::native_mint::ID,
            output_mint: *mint,
        })
    }

    pub async fn get_sell_quote(
        &self,
        mint: &Pubkey,
        token_amount: u64,
        slippage_bps: u16,
    ) -> anyhow::Result<PFQuote> {
        let (bonding_curve, _) = self.get_bonding_curve_account_for_mint(mint).await?;
        let sol_amount =
            bonding_curve.get_sell_price(token_amount, self.protocol.fee_basis_points)?;
        Ok(PFQuote {
            amount: token_amount,
            sol_amount,
            sol_amount_with_slippage: Self::sell_with_slippage(sol_amount, slippage_bps),
            input_mint: *mint,
            output_mint: spl_token::native_mint::ID,
        })
    }

    fn buy_with_slippage(amount: u64, slippage_bps: u16) -> u64 {
        amount + (amount * slippage_bps as u64) / 10_000
    }

    fn sell_with_slippage(amount: u64, slippage_bps: u16) -> u64 {
        amount - (amount * slippage_bps as u64) / 10_000
    }

    pub async fn get_bonding_curve_account_for_mint(
        &self,
        mint: &Pubkey,
    ) -> anyhow::Result<(BondingCurve, Pubkey)> {
        let pda = Self::get_bonding_curve_pda(mint);
        Ok((self.get_bonding_curve_account(&pda).await?, pda))
    }

    pub async fn get_bonding_curve_account(
        &self,
        address: &Pubkey,
    ) -> anyhow::Result<BondingCurve> {
        let account = self.client.get_account(address).await?;
        Ok(BondingCurve::from_bytes(&account.data)?)
    }

    pub fn buy_instruction(
        &self,
        user: &Pubkey,
        mint: &Pubkey,
        amount: u64,
        sol_amount: u64,
    ) -> Instruction {
        let bonding_curve = Self::get_bonding_curve_pda(mint);
        let bonding_curve_ata = get_associated_token_address(&bonding_curve, mint);
        let user_ata = get_associated_token_address(user, mint);

        BuyBuilder::new()
            .amount(amount)
            .max_sol_cost(sol_amount)
            .fee_recipient(self.protocol.fee_recipient)
            .mint(*mint)
            .associated_bonding_curve(bonding_curve_ata)
            .associated_user(user_ata)
            .user(*user)
            .global(self.address)
            .bonding_curve(bonding_curve)
            .event_authority(EVENT_AUTHORITY)
            .instruction()
    }

    pub fn sell_instruction(
        &self,
        user: &Pubkey,
        mint: &Pubkey,
        amount: u64,
        minimum_sol_out: u64,
    ) -> Instruction {
        let bonding_curve = Self::get_bonding_curve_pda(mint);
        let bonding_curve_ata = get_associated_token_address(&bonding_curve, mint);
        let user_ata = get_associated_token_address(user, mint);

        SellBuilder::new()
            .amount(amount)
            .min_sol_output(minimum_sol_out)
            .fee_recipient(self.protocol.fee_recipient)
            .mint(*mint)
            .associated_bonding_curve(bonding_curve_ata)
            .associated_user(user_ata)
            .user(*user)
            .global(self.address)
            .bonding_curve(bonding_curve)
            .event_authority(EVENT_AUTHORITY)
            .instruction()
    }

    async fn refresh(&mut self) -> anyhow::Result<()> {
        self.protocol = Self::get_global_account(&self.client, &self.address).await?;
        Ok(())
    }

    async fn get_global_account(client: &RpcClient, address: &Pubkey) -> anyhow::Result<Global> {
        let account = client.get_account(address).await?;
        Ok(Global::from_bytes(&account.data)?)
    }

    fn get_global_account_pda() -> Pubkey {
        Pubkey::find_program_address(&[b"global"], &PUMPFUN_PROGRAM_ID).0
    }

    fn get_bonding_curve_pda(mint: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(&[b"bonding-curve", mint.as_ref()], &PUMPFUN_PROGRAM_ID).0
    }
}

impl Global {
    pub fn get_initial_buy_price(&self, amount: u64) -> u64 {
        if amount == 0 {
            return 0;
        }

        let n =
            self.initial_virtual_sol_reserves as u128 * self.initial_virtual_sol_reserves as u128;
        let i = (self.initial_virtual_sol_reserves + amount) as u128;
        let r = (n / i) as u64 + 1;
        let s = self.initial_virtual_token_reserves - r;
        std::cmp::min(s, self.initial_real_token_reserves)
    }
}
