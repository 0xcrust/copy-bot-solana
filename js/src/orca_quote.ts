require('dotenv').config()
import { AnchorProvider, BN, Wallet } from "@coral-xyz/anchor";
import {
  PDAUtil,
  buildWhirlpoolClient,
  swapQuoteByInputToken,
  WhirlpoolContext,
  SwapUtils,
  SwapDirection
} from "@orca-so/whirlpools-sdk";
import { AddressUtil, Percentage } from "@orca-so/common-sdk";
import { Connection, Keypair, PublicKey } from "@solana/web3.js";

const WHIRLPOOL_PROGRAM_ID = new PublicKey(process.env.WHIRLPOOL_PROGRAM_ID!);
const WHIRLPOOL_CONFIG = new PublicKey(process.env.WHIRLPOOL_CONFIG!);
const INPUT_TOKEN = new PublicKey(process.env.INPUT_TOKEN!);
const OUTPUT_TOKEN = new PublicKey(process.env.OUTPUT_TOKEN!);
const HTTP_URL = process.env.HTTP_URL!;
const AMOUNT = new BN(process.env.AMOUNT!);
const SLIPPAGE = new BN(process.env.SLIPPAGE!);

(async () => {
  const keypair = Keypair.generate();
  const connection = new Connection(HTTP_URL);
  const provider = new AnchorProvider(connection, new Wallet(keypair), {});
  
  const ctx = WhirlpoolContext.withProvider(
    provider,
    new PublicKey(WHIRLPOOL_PROGRAM_ID!)
  );
  
  const whirlpoolPda = PDAUtil.getWhirlpool(
    new PublicKey(WHIRLPOOL_PROGRAM_ID),
    WHIRLPOOL_CONFIG,
    INPUT_TOKEN, // SOL
    OUTPUT_TOKEN, // WIF
    4 // Hardcoded from API results: SOL/WIF Tick-spacing
  );
  const whirlpoolClient = buildWhirlpoolClient(ctx);
  const whirlpool = await whirlpoolClient.getPool(whirlpoolPda.publicKey);
  const whirlpoolData = whirlpool.getData();
  const swapMintKey = AddressUtil.toPubKey(INPUT_TOKEN);
  const amountSpecifiedIsInput = true;
  const aToB = 
    SwapUtils.getSwapDirection(whirlpoolData, swapMintKey, amountSpecifiedIsInput) ===
    SwapDirection.AtoB;
  console.log("A to B is ", aToB);

  const tickArrays = SwapUtils.getTickArrayPublicKeys(
    whirlpoolData.tickCurrentIndex,
    whirlpoolData.tickSpacing,
    aToB,
    WHIRLPOOL_PROGRAM_ID,
    whirlpool.getAddress()
  );
  console.log("tickArrays: {}", tickArrays);

  const inputTokenQuote = await swapQuoteByInputToken(
    whirlpool,
    whirlpoolData.tokenMintA,
    new BN(AMOUNT),
    Percentage.fromFraction(SLIPPAGE, 100),
    ctx.program.programId,
    ctx.fetcher
  );
  console.log("Quote: {}", inputTokenQuote);
})().catch(console.error);