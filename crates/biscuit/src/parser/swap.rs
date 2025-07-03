use crate::analytics::dump::MintInfo;
use crate::utils::serde_helpers::{field_as_string, option_field_as_string};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{program_pack::Pack, pubkey::Pubkey};
use spl_token::state::Mint;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ResolvedSwap {
    /// The swap initiator
    #[serde(with = "option_field_as_string")]
    pub owner: Option<Pubkey>,
    /// The market
    pub provider: Provider,
    /// The input amount
    pub input_amount: u64,
    /// The output amount
    pub output_amount: u64,
    /// The input token
    #[serde(with = "field_as_string")]
    pub input_token_mint: Pubkey,
    /// The output token
    #[serde(with = "field_as_string")]
    pub output_token_mint: Pubkey,
    /// The token decimals
    pub token_decimals: Option<TokenDecimals>,
    pub input_mint_info: Option<MintInfo>,
    pub output_mint_info: Option<MintInfo>,
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize)]
pub struct TokenDecimals {
    pub input: u8,
    pub output: u8,
}

impl ResolvedSwap {
    pub async fn resolve_and_set_token_decimals(
        &mut self,
        client: &RpcClient,
    ) -> anyhow::Result<()> {
        if self.token_decimals.is_none() {
            let (input, output) = self.resolve_token_decimals(client).await?;
            self.token_decimals = Some(TokenDecimals { input, output });
        }

        Ok(())
    }

    pub async fn resolve_token_decimals(&self, client: &RpcClient) -> anyhow::Result<(u8, u8)> {
        let mut accounts = client
            .get_multiple_accounts(&[self.input_token_mint, self.output_token_mint])
            .await?;
        let input_account = accounts.remove(0).context("input mint does not exist")?;
        let output_account = accounts.remove(0).context("output mint does not exist")?;

        let input_decimals = Mint::unpack(&input_account.data[0..Mint::LEN])?.decimals;
        let output_decimals = Mint::unpack(&output_account.data[0..Mint::LEN])?.decimals;

        Ok((input_decimals, output_decimals))
    }
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize)]
pub enum Provider {
    Jupiter,
    RaydiumV4 {
        #[serde(with = "option_field_as_string")]
        state: Option<Pubkey>,
    },
    RaydiumV6 {
        #[serde(with = "option_field_as_string")]
        state: Option<Pubkey>,
    },
    OrcaWhirlpool {
        #[serde(with = "option_field_as_string")]
        state: Option<Pubkey>,
    },
    Pump,
}
impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Provider::Jupiter => f.write_str("Jupiter"),
            Provider::OrcaWhirlpool { state: _ } => f.write_str("Whirlpool"),
            Provider::RaydiumV6 { state: _ } => f.write_str("RaydiumV6"),
            Provider::RaydiumV4 { state: _ } => f.write_str("RaydiumV4"),
            Provider::Pump => f.write_str("PumpFun"),
        }
    }
}

impl Provider {
    pub fn is_pumpfun(&self) -> bool {
        matches!(self, Provider::Pump)
    }
}
