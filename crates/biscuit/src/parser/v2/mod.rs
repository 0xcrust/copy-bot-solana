//! WIP. V2 will attempt to have the following feature
//! 1. Be able to parse most known programs. i.e Native programs and SPL programs, Metaplex, Jupiter, Raydium, etc
//! 2. Provide a generic way to parse an unknown program just by its IDL, using rust macros.
//! 
//! Challenges: Differences between Shank IDL, Anchor legacy IDL, Anchor new IDL

use super::*;

use solana_program::pubkey;
use solana_sdk::pubkey::Pubkey;

const SIGHASH_GLOBAL_NAMESPACE: &str = "global";
pub fn anchor_instruction_sighash(ix_name: &str) -> [u8; 8] {
    let preimage = format!("{SIGHASH_GLOBAL_NAMESPACE}:{ix_name}");

    let mut sighash = [0u8; 8];
    sighash.copy_from_slice(&solana_sdk::hash::hash(preimage.as_bytes()).to_bytes()[..8]);
    sighash
}

pub fn anchor_account_discriminator(account_name: &str, namespaced: bool) -> [u8; 8] {
    const SIGHASH_GLOBAL_NAMESPACE: &str = "global";
    // Namespace the discriminator to prevent collisions.
    let discriminator_preimage = {
        // For now, zero copy accounts can't be namespaced.
        if !namespaced {
            format!("account:{account_name}")
        } else {
            format!("{SIGHASH_GLOBAL_NAMESPACE}:{account_name}")
        }
    };

    let mut discriminator = [0u8; 8];
    discriminator.copy_from_slice(
        &solana_sdk::hash::hash(discriminator_preimage.as_bytes()).to_bytes()[..8],
    );

    discriminator
}

pub trait ProgramInstructionParser {}

pub struct ParsedTransaction {}

pub struct ParsedInstruction {}

pub enum ParsedInstructionKind {
    System(SystemInstruction),
    Token(TokenInstruction),
    AssociatedToken(AssociatedTokenInstruction),
    ComputeBudget(ComputeBudgetInstruction),
    RaydiumLiquidityPoolV4(RaydiumLiquidityPoolV4Instruction),
    Bubblegum(BubblegumInstruction),
    JupiterAggregatorV6(JupiterAggregatorV6Instruction),
    JupiterDCA(JupiterDCAInstruction),
    Pumpfun(PumpfunInstruction),
    MeteoraVault(MeteoraVaultInstruction),
    MeteroaDlmm(MeteoraDlmmInstruction),
    Orca(OrcaInstruction),
    StateCompression(StateCompressionInstruction),
    DLNDestination(DLNDestinationInstruction),
    ParclV3(ParclV3Instruction),
    RaydiumConcentratedLiquidity(RaydiumConcentratedLiquidityInstruction),
    LifinitySwap(LifinitySwapInstruction),
    OpenbookV2(OpenbookV2Instruction),
    JupiterGovernance(JupiterGovernanceInstruction),
    InvariantSwap(InvariantSwapInstruction),
    SolIncinerator(SolIncineratorInstruction),
    Pheonix(PheonixInstruction),
    FluxBeam(FluxBeamInstruction),
    MetaplexCandyGuard(MetaplexCandyGuardInstruction),
    MetaplexCandyCore(MetaplexCandyCoreInstruction),
    MetaplexTokenMetadata(MetaplexTokenMetadataInstruction),
    MeteoraPools(MeteoraPoolsInstruction),
    SaberStableSwap(SaberStableSwapInstruction),
    UnknownWithoutIDL(UnknownWithoutIDLInstruction),
    UnknownWithIDL(UnknownWithIDLInstruction),
}

pub struct SystemInstruction {}
pub struct TokenInstruction {}
pub struct AssociatedTokenInstruction {}
pub struct ComputeBudgetInstruction {}
pub struct RaydiumLiquidityPoolV4Instruction {}
pub struct JupiterAggregatorV6Instruction {}
pub struct PumpfunInstruction {}
pub struct MeteoraVaultInstruction {}
pub struct OrcaInstruction {}
pub struct UnknownWithoutIDLInstruction {}
pub struct UnknownWithIDLInstruction {}
pub struct BubblegumInstruction {}
pub struct MeteoraDlmmInstruction {}
pub struct StateCompressionInstruction {}
pub struct JupiterDCAInstruction {}
pub struct DLNDestinationInstruction {}
pub struct ParclV3Instruction {}

pub struct RaydiumConcentratedLiquidityInstruction {}
pub struct OpenbookV2Instruction {}
pub struct LifinitySwapInstruction {}
pub struct JupiterGovernanceInstruction {}
pub struct InvariantSwapInstruction {}
pub struct SolIncineratorInstruction {}
pub struct PheonixInstruction {}
pub struct FluxBeamInstruction {}
pub struct MetaplexCandyGuardInstruction {}
pub struct MetaplexTokenMetadataInstruction {}
pub struct MeteoraPoolsInstruction {}
pub struct SaberStableSwapInstruction {}
pub struct MetaplexCandyCoreInstruction {}

pub enum SystemProgramInstructionKind {}
pub enum TokenProgramInstructionKind {}
pub enum AssociatedTokenProgramInstructionKind {}
pub enum ComputeBudgetProgramInstructionKind {}
pub enum RaydiumLiquidityPoolV4InstructionKind {}
pub enum JupiterAggregatorV6InstructionKind {}
pub enum PumpfunInstructionKind {}
pub enum MeteoraVaultInstructionKind {}
pub enum OrcaInstructionKind {}
pub enum BubblegumInstructionKind {}
pub enum MeteoraDlmmInstructionKind {}

pub enum StateCompressionInstructionKind {}
pub enum JupiterDCAInstructionKind {}
pub enum DLNDestinationInstructionKind {}
pub enum ParclV3InstructionKind {}

pub enum RaydiumConcentratedLiquidityInstructionKind {}

pub enum OpenbokV2InstructionKind {}
pub enum LifinitySwapInstructionKind {}
pub enum JupiterGovernanceInstructionKind {}
pub enum InvariantSwapInstructionKind {}
pub enum SolIncineratorKind {}
pub enum PheonixInstructionKind {}
pub enum FluxBeamInstructionKind {}
pub enum MetaplexCandyGuardInstructionKind {}
pub enum MeteoraPoolsInstructionKind {}
pub enum SaberStableSwapInstructionKind {}
pub enum MetaplexCandyCoreInstructionKind {}
