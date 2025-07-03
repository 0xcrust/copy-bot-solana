pub mod jupiter_v6;
pub mod orca;
pub mod pump;
pub mod raydium_v4;
pub mod raydium_v6;

use crate::parser::v1::ITransaction;

// Lifted and edited from: https://github.com/ebrightfield/solana-devtools/blob/master/monitoring/src/log_parsing.rs
use anyhow::anyhow;
use base64::{engine::general_purpose::STANDARD, Engine};
use lazy_static::lazy_static;
use regex::Regex;
use solana_sdk::instruction::CompiledInstruction;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::str::FromStr;

/// Prefix of a program log inside an instruction.
const PROGRAM_LOG: &str = "Program log: ";
/// Prefix of a program log of an Event.
const PROGRAM_DATA: &str = "Program data: ";

pub const EVENT_IX_TAG_LE: [u8; 8] = [228, 69, 165, 46, 81, 203, 154, 29];

lazy_static! {
    static ref CPI_PUSH_RE: Regex = Regex::new(r"^Program (.*) invoke.*$").unwrap();
    static ref CPI_POP_RE: Regex = Regex::new(r"^Program (.*) success*$").unwrap();
    static ref PROGRAM_FAILURE_RE: Regex = Regex::new(r"^Program (.*) failed: (.*)$").unwrap();
}

pub fn get_logs(tx: &ITransaction) -> HashMap<Pubkey, Vec<Vec<u8>>> {
    let logs = tx
        .meta
        .clone()
        .and_then(|m| {
            let messages: Option<Vec<String>> = Option::from(m.log_messages);
            messages
        })
        .unwrap_or_default();
    parse_transaction_logs(logs, None)
}

pub fn get_logs_for_program(tx: &ITransaction, target_program: &Pubkey) -> Vec<Vec<u8>> {
    let logs = tx
        .meta
        .clone()
        .and_then(|m| {
            let messages: Option<Vec<String>> = Option::from(m.log_messages);
            messages
        })
        .unwrap_or_default();
    parse_transaction_logs(logs, Some(target_program))
        .into_values()
        .next()
        .unwrap_or_default()
}

pub fn get_cpi_logs_for_instruction(
    tx: &ITransaction,
    instruction_idx: usize,
) -> Option<Vec<Vec<u8>>> {
    let inner_instructions = tx
        .inner_instructions
        .as_ref()?
        .get(&(instruction_idx as u8))?;
    // neat! we don't need to check the program-id because cpi-logs are always self-cpis
    // - https://github.com/coral-xyz/anchor/issues/2408#issuecomment-1447243011
    // - https://github.com/ngundotra/anchor/blob/22b902a06b5af80606439d6fe3b79ea90ddf7073/lang/attribute/event/src/lib.rs#L103
    Some(
        inner_instructions
            .into_iter()
            .filter_map(anchor_event_cpi_filter)
            .collect(),
    ) // does this yield results in order?
}

fn anchor_event_cpi_filter(ix: &CompiledInstruction) -> Option<Vec<u8>> {
    let data = &ix.data;
    if data.len() < EVENT_IX_TAG_LE.len() {
        return None;
    }
    let (discriminator, rest) = data.split_at(EVENT_IX_TAG_LE.len());
    if discriminator == EVENT_IX_TAG_LE {
        Some(rest.to_vec())
    } else {
        None
    }
}

fn handle_program_log(l: &str) -> (Option<Vec<u8>>, CpiStackManipulation) {
    // Log emitted from the current program.
    if let Some(log) = l
        .strip_prefix(PROGRAM_LOG)
        .or_else(|| l.strip_prefix(PROGRAM_DATA))
    {
        let borsh_bytes = match STANDARD.decode(log) {
            Ok(borsh_bytes) => borsh_bytes,
            _ => {
                return (None, CpiStackManipulation::None);
            }
        };
        (Some(borsh_bytes), CpiStackManipulation::None)
    }
    // System log.
    else {
        let push_or_pop = handle_system_log(l);
        (None, push_or_pop)
    }
}

fn parse_transaction_logs(
    logs: Vec<String>,
    target_program_id: Option<&Pubkey>,
) -> HashMap<Pubkey, Vec<Vec<u8>>> {
    let mut events = HashMap::new();
    if !logs.is_empty() {
        if let Ok(mut execution) = ProgramCpiStack::new(&mut logs.as_ref()) {
            for l in logs {
                // Parse the program logs
                let (event, push_or_pop) = {
                    let execution_program = execution.program();
                    let log_filter = match target_program_id {
                        Some(program) => program.to_string() == execution_program,
                        None => true,
                    };
                    // Is the log part of the target program?
                    if !execution.is_empty() && log_filter {
                        let (logs, stack) = handle_program_log(&l);
                        (logs.map(|logs| (execution_program, logs)), stack)
                    } else {
                        // If not, then see if we pushed or popped
                        let push_or_pop = handle_system_log(&l);
                        (None, push_or_pop)
                    }
                };
                // Emit the event.
                if let Some((program, event)) = event {
                    let program = Pubkey::from_str(&program).unwrap();
                    events
                        .entry(program)
                        .and_modify(|v: &mut Vec<Vec<u8>>| v.push(event.clone()))
                        .or_insert(vec![event]);
                }
                match push_or_pop {
                    CpiStackManipulation::Push(new_program) => execution.push(new_program),
                    CpiStackManipulation::Pop => execution.pop(),
                    _ => {}
                }
            }
        }
    }
    events
}

/// Detect whether we have pushed or popped.
fn handle_system_log(log: &str) -> CpiStackManipulation {
    if CPI_PUSH_RE.is_match(log) {
        let c = CPI_PUSH_RE
            .captures(log)
            .expect("unable to parse system log");
        let program = c
            .get(1)
            .expect("unable to parse system log")
            .as_str()
            .to_string();
        CpiStackManipulation::Push(program)
    } else if CPI_POP_RE.is_match(log) {
        CpiStackManipulation::Pop
    } else {
        CpiStackManipulation::None
    }
}

/// Tracks whether logs indicate a push or pop through the CPI stack.
#[derive(Debug, Clone, PartialEq)]
enum CpiStackManipulation {
    /// Stores the Program ID of the CPI invoked
    Push(String),
    Pop,
    None,
}

/// Tracks the current-running program as a transaction pushes and pops
/// up and down the call-stack with CPIs and instructions.
/// For example, if we're at top-level execution, there is one `String`
/// in `self.stack`, which is the base58 Program ID.
/// If we're one-level down in a CPI, there are two elements in the stack,
/// and the second element is the program ID invoked as a CPI.
struct ProgramCpiStack {
    stack: Vec<String>,
}

impl ProgramCpiStack {
    fn new(logs: &mut &[String]) -> anyhow::Result<Self> {
        let l = &logs[0];
        *logs = &logs[1..];

        // These should never fail
        // as long as we are processing the first Solana transaction log.
        let c = CPI_PUSH_RE.captures(l).ok_or(anyhow!(
            "Failed to parse a program ID from Solana program log: {}",
            l
        ))?;
        let program = c
            .get(1)
            .ok_or(anyhow!(
                "Failed to parse a program ID from Solana program log: {}",
                l
            ))?
            .as_str()
            .to_string();
        Ok(Self {
            stack: vec![program],
        })
    }

    fn program(&self) -> String {
        assert!(!self.stack.is_empty(), "{:?}", self.stack);
        self.stack[self.stack.len() - 1].clone()
    }

    fn push(&mut self, new_program: String) {
        self.stack.push(new_program);
    }

    fn pop(&mut self) {
        assert!(!self.stack.is_empty());
        self.stack.pop().unwrap();
    }

    fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }
}

/// When a transaction execution fails, then its final log
/// prints the program that failed, and an error message from a
/// `solana_program::instruction::InstructionError`.
#[derive(Debug, Clone, PartialEq)]
pub struct LoggedTransactionFailure {
    pub program: Pubkey,
    /// `Display` of a `solana_program::instruction::InstructionError`.
    pub error: String,
}

/// An example might be:
/// "Program JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4 failed: custom program error: 0x1771"
/// Where the second string comes from the `Display` of a
/// `solana_program::instruction::InstructionError`.
pub fn check_for_program_error(log: &str) -> Option<LoggedTransactionFailure> {
    PROGRAM_FAILURE_RE.captures(log).map(|c| {
        let program = c.get(1).unwrap();
        let program = Pubkey::from_str(program.as_str()).unwrap();
        let error = c.get(2).unwrap().as_str().to_string();
        LoggedTransactionFailure { program, error }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::pubkey;

    #[test]
    fn program_err_log() {
        let log = "Program JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4 failed: custom program error: 0x1771";
        let LoggedTransactionFailure { program, error } = check_for_program_error(log).unwrap();
        assert_eq!(
            pubkey!("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4"),
            program
        );
        assert_eq!("custom program error: 0x1771", error);
    }
}

// Iterate through the logs of a transaction execution,
// looking for logs originating from a target program (by pubkey string),
// and return any events of type `T` logged, and error information if the transaction failed.
// pub fn parse_transaction_logs(logs: Vec<String>, target_program_id: &str) -> Vec<Vec<u8>> {
//     let mut events = Vec::new();
//     if !logs.is_empty() {
//         if let Ok(mut execution) = ProgramCpiStack::new(&mut logs.as_ref()) {
//             for l in logs {
//                 // Parse the program logs
//                 let (event, push_or_pop) = {
//                     // Is the log part of the target program?
//                     if !execution.is_empty() && target_program_id == execution.program() {
//                         handle_program_log(&l)
//                     } else {
//                         // If not, then see if we pushed or popped
//                         let push_or_pop = handle_system_log(&l);
//                         (None, push_or_pop)
//                     }
//                 };
//                 // Emit the event.
//                 if let Some(e) = event {
//                     events.push(e);
//                 }
//                 match push_or_pop {
//                     CpiStackManipulation::Push(new_program) => execution.push(new_program),
//                     CpiStackManipulation::Pop => execution.pop(),
//                     _ => {}
//                 }
//             }
//         }
//     }
//     events
// }
