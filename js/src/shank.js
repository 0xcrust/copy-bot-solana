import path from "path";
import { generateIdl } from "@metaplex-foundation/shank-js";
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const anchorIdlDir = path.join(__dirname, "../../artifacts/program_idls/anchor");
const shankIdlDir = path.join(__dirname, "../../artifacts/program_idls/shank");

const binaryInstallDir = path.join(__dirname, "../..", ".crates");

//const programsDir = process.env.PROGRAMS_SHARED_DIR;
//const programsDir = '~/dedicated/study/blockchains/solana/programs'; // TODO: Set to env
const programsDir = "../dedicated/study/blockchains/solana/programs";
//const programsDir = '';
const raydiumAmmProgramDir = programsDir + '/raydium/amm/program';
console.log('amm directory: ', raydiumAmmProgramDir);
const raydiumClmmProgramDir = programsDir + '/raydium/clmm/programs/amm';
console.log('clmm directory: ', raydiumClmmProgramDir);
//const raydiumAmmProgramDir = path.join(programsDir, "programs/raydium/amm/program");
//const raydiumClmmProgramDir = path.join(programsDir, "programs/raydium/clmm/programs/amm");

// const { idlDir, programName, idlName } = config;

// From a vanilla Solana program using Shank.
/*generateIdl({
  generator: "shank",
  programName: "raydium_amm",
  programId: "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8",
  idlName: "Raydium Liquidity Pool V4",
  idlDir: shankIdlDir,
  binaryInstallDir,
  programDir: raydiumAmmProgramDir,
  removeExistingIdl: true
  //programDir: path.join(programDir, "token-metadata"),
});*/

// From an Anchor program.
generateIdl({  
  generator: "anchor",
  programName: "raydium-amm-v3",
  programId: "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK",
  idlName: "Raydium Concentrated Liquidity",
  idlDir: anchorIdlDir,
  binaryInstallDir,
  programDir: raydiumClmmProgramDir
  //programDir: path.join(programDir, "candy-machine-core"),
});