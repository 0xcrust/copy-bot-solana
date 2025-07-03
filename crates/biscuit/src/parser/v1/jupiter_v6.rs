use super::super::generated::jupiter_aggregator_v6::instructions::{
    ExactOutRoute, ExactOutRouteInstructionArgs, Route, RouteInstructionArgs, RouteWithTokenLedger,
    RouteWithTokenLedgerInstructionArgs, SharedAccountsExactOutRoute,
    SharedAccountsExactOutRouteInstructionArgs, SharedAccountsRoute,
    SharedAccountsRouteInstructionArgs, SharedAccountsRouteWithTokenLedger,
    SharedAccountsRouteWithTokenLedgerInstructionArgs,
};
use super::{decode_instruction, split_discriminator, KeysWindow};

use anyhow::{anyhow, Result};
use log::trace;
use solana_sdk::instruction::CompiledInstruction;
use solana_sdk::message::AccountKeys;
use solana_sdk::pubkey::Pubkey;

const JUPITER_EXACT_OUT_ROUTE_DISC: [u8; 8] = [208, 51, 239, 151, 123, 43, 237, 92];
const JUPITER_ROUTE_WITH_TOKEN_LEDGER_DISC: [u8; 8] = [150, 86, 71, 116, 167, 93, 14, 104];
const JUPITER_ROUTE_DISC: [u8; 8] = [229, 23, 203, 151, 122, 227, 173, 42];
const JUPITER_SHARED_ACCOUNTS_EXACT_OUT_ROUTE_DISC: [u8; 8] =
    [176, 209, 105, 168, 154, 125, 69, 62];
const JUPITER_SHARED_ACCOUNTS_ROUTE_DISC: [u8; 8] = [193, 32, 155, 51, 65, 214, 156, 129];
const JUPITER_SHARED_ACCOUNTS_ROUTE_WITH_TOKEN_LEDGER_DISC: [u8; 8] =
    [230, 121, 143, 80, 119, 159, 106, 170];

pub struct ParsedJupiter {
    pub instruction: ParsedJupiterInstruction,
    pub cpi_logs: Vec<Vec<u8>>,
}

pub enum ParsedJupiterInstruction {
    Route {
        args: RouteInstructionArgs,
        accounts: Route,
    },
    ExactOutRoute {
        args: ExactOutRouteInstructionArgs,
        accounts: ExactOutRoute,
    },
    RouteWithTokenLedger {
        args: RouteWithTokenLedgerInstructionArgs,
        accounts: RouteWithTokenLedger,
    },
    SharedAccountsExactOutRoute {
        args: SharedAccountsExactOutRouteInstructionArgs,
        accounts: SharedAccountsExactOutRoute,
    },
    SharedAccountsRoute {
        args: SharedAccountsRouteInstructionArgs,
        accounts: SharedAccountsRoute,
    },
    SharedAccountsRouteWithTokenLedger {
        args: SharedAccountsRouteWithTokenLedgerInstructionArgs,
        accounts: SharedAccountsRouteWithTokenLedger,
    },
    Unknown {
        discriminator: [u8; 8],
    },
}

pub(super) fn decode_jupiter_v6_instruction(
    instruction: &CompiledInstruction,
    keys: &AccountKeys,
) -> Result<ParsedJupiterInstruction> {
    log::debug!("Decoding jupiter v6 instruction");
    let (discriminator, mut rest) = split_discriminator(&instruction.data);
    let mut keys = KeysWindow::new_from_instruction(keys, instruction);
    Ok(match discriminator {
        JUPITER_ROUTE_DISC => {
            trace!(
                "JUPITER ROUTE. discriminator={}",
                hex::encode(discriminator)
            );
            // user_transfer_authority is user wallet
            // user_source_token_account is source ata
            // user_destination_token_account is destination ata
            // destination mint is the token we're getting from the swap
            //
            // ** The following is wrong. Still curious about where I got it from **
            // Route is used for WSOL -> Y swaps, so source_mint is SOL and source_token_account is WSOL ata
            let args = decode_instruction::<RouteInstructionArgs>(&mut rest)?;
            let accounts = Route {
                token_program: keys.next_account_key()?,
                user_transfer_authority: keys.next_account_key()?,
                user_source_token_account: keys.next_account_key()?,
                user_destination_token_account: keys.next_account_key()?,
                destination_token_account: keys.next_anchor_opt_account_key()?,
                destination_mint: keys.next_account_key()?,
                platform_fee_account: keys.next_anchor_opt_account_key()?,
                event_authority: keys.next_account_key()?,
                program: keys.next_account_key()?,
            };
            ParsedJupiterInstruction::Route { args, accounts }
        }
        JUPITER_EXACT_OUT_ROUTE_DISC => {
            trace!(
                "JUPITER EXACT-OUT ROUTE. discriminator={}",
                hex::encode(discriminator)
            );
            let args = decode_instruction::<ExactOutRouteInstructionArgs>(&mut rest)?;
            let accounts = ExactOutRoute {
                token_program: keys.next_account_key()?,
                user_transfer_authority: keys.next_account_key()?,
                user_source_token_account: keys.next_account_key()?,
                user_destination_token_account: keys.next_account_key()?,
                destination_token_account: keys.next_anchor_opt_account_key()?,
                source_mint: keys.next_account_key()?,
                destination_mint: keys.next_account_key()?,
                platform_fee_account: keys.next_anchor_opt_account_key()?,
                token2022_program: keys.next_anchor_opt_account_key()?,
                event_authority: keys.next_account_key()?,
                program: keys.next_account_key()?,
            };
            ParsedJupiterInstruction::ExactOutRoute { args, accounts }
        }
        JUPITER_ROUTE_WITH_TOKEN_LEDGER_DISC => {
            trace!(
                "JUPITER ROUTE WITH TOKEN LEDGER. discriminator={}",
                hex::encode(discriminator)
            );
            let args = decode_instruction::<RouteWithTokenLedgerInstructionArgs>(&mut rest)?;
            let accounts = RouteWithTokenLedger {
                token_program: keys.next_account_key()?,
                user_transfer_authority: keys.next_account_key()?,
                user_source_token_account: keys.next_account_key()?,
                user_destination_token_account: keys.next_account_key()?,
                destination_token_account: keys.next_anchor_opt_account_key()?,
                destination_mint: keys.next_account_key()?,
                platform_fee_account: keys.next_anchor_opt_account_key()?,
                token_ledger: keys.next_account_key()?,
                event_authority: keys.next_account_key()?,
                program: keys.next_account_key()?,
            };
            ParsedJupiterInstruction::RouteWithTokenLedger { args, accounts }
        }
        // Shared Accounts Route is used for A -> B swaps(not SOL)
        JUPITER_SHARED_ACCOUNTS_EXACT_OUT_ROUTE_DISC => {
            trace!(
                "JUPITER SHARED ACCOUNTS EXACT OUT ROUTE. discriminator={}",
                hex::encode(discriminator)
            );
            let args = decode_instruction::<SharedAccountsExactOutRouteInstructionArgs>(&mut rest)?;
            let accounts = SharedAccountsExactOutRoute {
                token_program: keys.next_account_key()?,
                program_authority: keys.next_account_key()?,
                user_transfer_authority: keys.next_account_key()?,
                source_token_account: keys.next_account_key()?,
                program_source_token_account: keys.next_account_key()?,
                program_destination_token_account: keys.next_account_key()?,
                destination_token_account: keys.next_account_key()?,
                source_mint: keys.next_account_key()?,
                destination_mint: keys.next_account_key()?,
                platform_fee_account: keys.next_anchor_opt_account_key()?,
                token2022_program: keys.next_anchor_opt_account_key()?,
                event_authority: keys.next_account_key()?,
                program: keys.next_account_key()?,
            };
            ParsedJupiterInstruction::SharedAccountsExactOutRoute { args, accounts }
        }
        JUPITER_SHARED_ACCOUNTS_ROUTE_DISC => {
            trace!(
                "JUPITER SHARED ACCOUNTS ROUTE. discriminator={}",
                hex::encode(discriminator)
            );
            let args = decode_instruction::<SharedAccountsRouteInstructionArgs>(&mut rest)?;
            let accounts = SharedAccountsRoute {
                token_program: keys.next_account_key()?,
                program_authority: keys.next_account_key()?,
                user_transfer_authority: keys.next_account_key()?,
                source_token_account: keys.next_account_key()?,
                program_source_token_account: keys.next_account_key()?,
                program_destination_token_account: keys.next_account_key()?,
                destination_token_account: keys.next_account_key()?,
                source_mint: keys.next_account_key()?,
                destination_mint: keys.next_account_key()?,
                platform_fee_account: keys.next_anchor_opt_account_key()?,
                token2022_program: keys.next_anchor_opt_account_key()?,
                event_authority: keys.next_account_key()?,
                program: keys.next_account_key()?,
            };
            ParsedJupiterInstruction::SharedAccountsRoute { args, accounts }
        }
        JUPITER_SHARED_ACCOUNTS_ROUTE_WITH_TOKEN_LEDGER_DISC => {
            trace!(
                "JUPITER SHARED ACCOUNTS ROUTE WITH TOKEN LEDGER. discriminator={}",
                hex::encode(discriminator)
            );
            let args =
                decode_instruction::<SharedAccountsRouteWithTokenLedgerInstructionArgs>(&mut rest)?;
            let accounts = SharedAccountsRouteWithTokenLedger {
                token_program: keys.next_account_key()?,
                program_authority: keys.next_account_key()?,
                user_transfer_authority: keys.next_account_key()?,
                source_token_account: keys.next_account_key()?,
                program_source_token_account: keys.next_account_key()?,
                program_destination_token_account: keys.next_account_key()?,
                destination_token_account: keys.next_account_key()?,
                source_mint: keys.next_account_key()?,
                destination_mint: keys.next_account_key()?,
                platform_fee_account: keys.next_anchor_opt_account_key()?,
                token2022_program: keys.next_anchor_opt_account_key()?,
                token_ledger: keys.next_account_key()?,
                event_authority: keys.next_account_key()?,
                program: keys.next_account_key()?,
            };
            ParsedJupiterInstruction::SharedAccountsRouteWithTokenLedger { args, accounts }
        }
        _ => {
            trace!(
                "JUPITER UNKNOWN DISCRIMINATOR: {:#?}, hex: {}",
                discriminator,
                hex::encode(discriminator)
            );
            ParsedJupiterInstruction::Unknown { discriminator }
        }
    })
}

impl ParsedJupiterInstruction {
    pub fn is_swap_instruction(&self) -> bool {
        !matches!(self, ParsedJupiterInstruction::Unknown { discriminator: _ })
    }

    pub fn base_input(&self) -> anyhow::Result<bool> {
        use ParsedJupiterInstruction::*;

        Ok(match self {
            Route {
                args: _,
                accounts: _,
            } => true,
            RouteWithTokenLedger {
                args: _,
                accounts: _,
            } => true,
            SharedAccountsRoute {
                args: _,
                accounts: _,
            } => true,
            SharedAccountsRouteWithTokenLedger {
                args: _,
                accounts: _,
            } => true,
            Unknown { discriminator: _ } => return Err(anyhow!("Not a swap instruction")),
            _ => false,
        })
    }

    pub fn source_mint(&self) -> anyhow::Result<Option<Pubkey>> {
        use ParsedJupiterInstruction::*;

        Ok(match self {
            Route { args, accounts } => None,
            RouteWithTokenLedger { args, accounts } => None,
            SharedAccountsRoute { args, accounts } => Some(accounts.source_mint),
            SharedAccountsRouteWithTokenLedger { args, accounts } => Some(accounts.source_mint),
            ExactOutRoute { args, accounts } => Some(accounts.source_mint),
            SharedAccountsExactOutRoute { args, accounts } => Some(accounts.source_mint),
            Unknown { discriminator: _ } => return Err(anyhow!("Not a swap instruction")),
        })
    }

    pub fn user(&self) -> anyhow::Result<Pubkey> {
        use ParsedJupiterInstruction::*;

        Ok(match self {
            Route { args, accounts } => accounts.user_transfer_authority,
            RouteWithTokenLedger { args, accounts } => accounts.user_transfer_authority,
            SharedAccountsRoute { args, accounts } => accounts.user_transfer_authority,
            SharedAccountsRouteWithTokenLedger { args, accounts } => {
                accounts.user_transfer_authority
            }
            ExactOutRoute { args, accounts } => accounts.user_transfer_authority,
            SharedAccountsExactOutRoute { args, accounts } => accounts.user_transfer_authority,
            Unknown { discriminator: _ } => return Err(anyhow!("Not a swap instruction")),
        })
    }

    pub fn destination_mint(&self) -> anyhow::Result<Pubkey> {
        use ParsedJupiterInstruction::*;

        Ok(match self {
            Route { args, accounts } => accounts.destination_mint,
            RouteWithTokenLedger { args, accounts } => accounts.destination_mint,
            SharedAccountsRoute { args, accounts } => accounts.destination_mint,
            SharedAccountsRouteWithTokenLedger { args, accounts } => accounts.destination_mint,
            ExactOutRoute { args, accounts } => accounts.destination_mint,
            SharedAccountsExactOutRoute { args, accounts } => accounts.destination_mint,
            Unknown { discriminator: _ } => return Err(anyhow!("Not a swap instruction")),
        })
    }

    pub fn other_amount_threshold(&self) -> anyhow::Result<u64> {
        use ParsedJupiterInstruction::*;

        Ok(match self {
            Route { args, accounts: _ } => args.quoted_out_amount,
            RouteWithTokenLedger { args, accounts: _ } => args.quoted_out_amount,
            SharedAccountsRoute { args, accounts } => args.quoted_out_amount,
            SharedAccountsRouteWithTokenLedger { args, accounts: _ } => args.quoted_out_amount,
            ExactOutRoute { args, accounts: _ } => args.quoted_in_amount,
            SharedAccountsExactOutRoute { args, accounts: _ } => args.quoted_in_amount,
            Unknown { discriminator: _ } => return Err(anyhow!("Not a swap instruction")),
        })
    }
}

#[cfg(test)]
mod test {
    use super::super::anchor_instruction_sighash;
    use super::{
        JUPITER_EXACT_OUT_ROUTE_DISC, JUPITER_ROUTE_DISC, JUPITER_ROUTE_WITH_TOKEN_LEDGER_DISC,
        JUPITER_SHARED_ACCOUNTS_EXACT_OUT_ROUTE_DISC, JUPITER_SHARED_ACCOUNTS_ROUTE_DISC,
        JUPITER_SHARED_ACCOUNTS_ROUTE_WITH_TOKEN_LEDGER_DISC,
    };

    #[test]
    fn jupiter_test_instruction_sighash() {
        let exact_out_discriminator = anchor_instruction_sighash("exact_out_route");
        assert_eq!(exact_out_discriminator, JUPITER_EXACT_OUT_ROUTE_DISC);

        let route_discriminator = anchor_instruction_sighash("route");
        assert_eq!(route_discriminator, JUPITER_ROUTE_DISC);

        let route_with_tl_discriminator = anchor_instruction_sighash("route_with_token_ledger");
        assert_eq!(
            route_with_tl_discriminator,
            JUPITER_ROUTE_WITH_TOKEN_LEDGER_DISC
        );

        let shared_accounts_exact_out_route_discriminator =
            anchor_instruction_sighash("shared_accounts_exact_out_route");
        assert_eq!(
            shared_accounts_exact_out_route_discriminator,
            JUPITER_SHARED_ACCOUNTS_EXACT_OUT_ROUTE_DISC
        );

        let shared_accounts_route = anchor_instruction_sighash("shared_accounts_route");
        assert_eq!(shared_accounts_route, JUPITER_SHARED_ACCOUNTS_ROUTE_DISC);

        let shared_accounts_route_with_tl =
            anchor_instruction_sighash("shared_accounts_route_with_token_ledger");
        assert_eq!(
            shared_accounts_route_with_tl,
            JUPITER_SHARED_ACCOUNTS_ROUTE_WITH_TOKEN_LEDGER_DISC
        );
    }
}
