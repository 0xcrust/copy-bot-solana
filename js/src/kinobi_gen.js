import { 
  createFromIdls, 
  renderRustVisitor, 
  updateProgramsVisitor, 
  updateInstructionsVisitor,
  setAnchorDiscriminatorsVisitor,
  setAccountDiscriminatorFromFieldVisitor,
  setInstructionDiscriminatorsVisitor,
  updateAccountsVisitor, 
  numberTypeNode, 
  STANDALONE_TYPE_NODES, 
  updateDefinedTypesVisitor,
  arrayTypeNode,
  createTypeNodeFromIdl,
  definedTypeNodeFromIdl,
} from "@metaplex-foundation/kinobi";
import path from "path";
import { fileURLToPath } from 'url';
import fs from 'fs';
import { snakeCase } from "snake-case";
import { sha256 } from "@noble/hashes/sha256";
import { Buffer } from "buffer";

// Get the directory name from the module's URL
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const idls = [
    {
        idlFile: 'pump.json',
        outName: 'pump',
        idlName: 'pump',
        programId: '6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P',
        origin: 'anchor'
    }
//   {
//     idlFile: 'Bubblegum.json',
//     outName: 'bubblegum',
//     idlName: 'bubblegum',
//     programId: 'BGUMAp9Gq7iTEuizy4pqaxsTyUCBK68MDfK752saRPUY',
//     origin: 'anchor'
//   },
//   {
//     idlFile: 'Jupiter Aggregator V6.json', 
//     outName: 'jupiter_aggregator_v6',
//     idlName: 'jupiter', 
//     programId: 'JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4',
//     origin: 'anchor',
//     instructions: ['route', 'routeWithTokenLedger', 'exactOutRoute', 'sharedAccountsRoute', 'sharedAccountsRouteWithTokenLedger', 'sharedAccountsExactOutRoute']
//   },
//   {
//     idlFile: 'Jupiter DCA.json', 
//     outName: 'jupiter_dca', 
//     idlName: 'dca', 
//     programId: 'DCA265Vj8a9CEuX1eb1LWRnDT7uK6q1xMipnNyatn23M',
//     origin: 'anchor'
//   },
//   {
//     idlFile: 'Jupiter Governance.json', 
//     outName: 'jupiter_governance', 
//     idlName: 'govern', 
//     programId: 'GovaE4iu227srtG2s3tZzB4RmWBzw8sTwrCLZz7kN7rY',
//     origin: 'anchor'
//   },
//   {
//     idlFile: 'Metaplex Candy Core.json', 
//     outName: 'metaplex_candy_core',
//     idlName: 'candy_machine_core', 
//     programId: 'CndyV3LdqHUfDLmE5naZjVN8rBZz4tqhdefbAnjHG3JR',
//     origin: 'anchor'
//   },
//   {
//     idlFile: 'Metaplex Candy Guard.json', 
//     outName: 'metaplex_candy_guard',
//     idlName: 'candy_guard', 
//     programId: 'Guard1JwRhJkVH6XZhzoYxeBVQe872VH6QggF4BWmS9g',
//     origin: 'anchor'
//   },
//   {
//     idlFile: 'Meteora DLMM.json', 
//     outName: 'meteora_dlmm',
//     idlName: 'lb_clmm', 
//     programId: 'LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo',
//     origin: 'anchor',
//     instructions: ['addLiquidity, addLiquidityByWeight', 'addLiquidityByStrategy', 'addLiquidityByStrategyOneSide', 'addLiquidityOneSide', 'removeLiquidity', 'swap']
//   },
//   {
//     idlFile: 'Meteora Pools.json', 
//     outName: 'meteora_pools',
//     idlName: 'amm', 
//     programId: 'Eo7WjKq67rjJQSZxS6z3YkapzY3eMj6Xy8X5EQVn5UaB',
//     origin: 'anchor',
//     instructions: ['initializePermissionedPool', 'initializePermissionlessPool', 'initializePermissionlessPoolWithFeeTier', 'swap', 'removeLiquiditySingleSide', 'addImbalanceLiquidity', 'removeBalanceLiquidity', 'addBalanceLiquidity', 'bootstrapLiquidity', 'swap']
//   },
//   {
//     idlFile: 'Meteora Vault.json', 
//     outName: 'meteora_vault',
//     idlName: 'vault', 
//     programId: '24Uqj9JCLxUeoC3hGfh5W3s9FM9uCHDS2SG3LYwBpyTi',
//     origin: 'anchor'
//   },
//   {
//     idlFile: 'OpenbookV2.json', 
//     outName: 'openbook_v2',
//     idlName: 'openbook_v2', 
//     programId: 'opnb2LAfJYbRMAHHvqjCwQxanZn7ReEHp1k81EohpZb',
//     origin: 'anchor'
//   },
//   {
//     idlFile: 'Orca Whirlpools.json', 
//     outName: 'orca_whirlpools',
//     idlName: 'whirlpool', 
//     programId: "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc",
//     origin: 'anchor',
//     instructions: ['increase_liquidity', 'increase_liquidity_v2', 'initialize_pool', 'initialize_pool_v2', 'swap', 'swap_v2']
//   },
//   { 
//     idlFile: 'Raydium Concentrated Liquidity.json', 
//     outName: 'raydium_clmm',
//     idlName: 'amm_v3', 
//     programId: 'CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK',
//     origin: 'anchor',
//     instructions: ['createPool', 'openPosition', 'openPositionV2', 'closePosition', 'increaseLiquidity', 'increaseLiquidityV2', 'decreaseLiquidity', 'decreaseLiquidityV2', 'swap', 'swapV2'],
//   },
//   { 
//     idlFile: 'Sol Incinerator.json', 
//     outName: 'sol_incinerator',
//     idlName: 'incinerator_contract', 
//     programId: 'F6fmDVCQfvnEq2KR8hhfZSEczfM9JK9fWbCsYJNbTGn7',
//     origin: 'anchor'
//   },
//   { 
//     idlFile: 'Spl Account Compression.json',
//     outName: 'spl_account_compression',
//     idlName: 'spl_account_compression', 
//     programId: 'cmtDvXumGCrqC1Age74AVPhSRVXJMd8PJS91L8KbNCK',
//     origin: 'anchor'
//   }
];

let generatedModuleImports = [];
for (const item of idls) {
  generatedModuleImports.push("pub mod " + item.outName + ";");
}
// TODO: Create generated/mod.rs and write the above to it

let programIdImports = [];
for (const item of idls) {
  console.log('Generating for program item', item.outName);
  let idlPath = path.join(__dirname, "../../artifacts/idls", item.origin, item.idlFile);
  let kinobi = createFromIdls([idlPath]);
  const outDir = path.join(__dirname, "../../crates/biscuit/src", "parser", "generated", item.outName);
  let programVisitorMap = {};
  const idl = JSON.parse(fs.readFileSync(idlPath, 'utf8'));

  let eventStructs = [];
  for (const event of idl.events) {
    let idlType = {
        kind: 'struct',
        fields: event.fields.map(field => {
            return {
                name: field.name,
                type: field.type
            }
        })
    };
    let type = definedTypeNodeFromIdl({
        name: event.name,
        type: idlType
    });
    eventStructs.push(type);
  };

  let root = JSON.parse(kinobi.getJson());
  let previousTypes = root.programs[0].definedTypes;

  programVisitorMap[item.idlName] = {
    name: item.outName,
    publicKey: item.programId,
    origin: item.origin,
    definedTypes: previousTypes.concat(eventStructs)
  };

//   let instructionDiscriminators = {};
//   for (const instruction of idl.instructions) {
//     let discriminator = new Uint8Array(getInstructionDiscriminator(instruction.name));
//     let items = Array.from(discriminator, (i) => ({
//       kind: 'numberValueNode',
//       number: i
//     }));
    
//     instructionDiscriminators[instruction.name] = {
//       value: {
//         kind: 'arrayValueNode',
//         items,
//       },
//       type: arrayTypeNode(numberTypeNode('u8'))
//     };
//   }

  kinobi.update(
    updateProgramsVisitor(programVisitorMap)
  );
  kinobi.update(
    setAnchorDiscriminatorsVisitor()
  );
//   kinobi.update(
//     updateDefinedTypesVisitor(eventStructs)
//   )

  // TODO: Currently, We have to manually edit the generated code to exlude deriving `Eq` for 
  // structs with f32 / f64 fields(causes a compilation error). As of now, this is only an issue 
  // in openbook_v2 
  let rootPath = "crate::parser::generated";
  kinobi.accept(renderRustVisitor(outDir, {
    dependencyMap: {
      generated: rootPath,
      generatedAccounts: rootPath + '::' + item.outName + '::accounts',
      generatedTypes: rootPath + '::' + item.outName + '::types',
      generatedErrors: rootPath + '::' + item.outName + '::errors',
    },
    deleteFolderBeforeRendering: true,
    crateFolder: ".", // note: will cause errors if not run from root Cargo.toml dir
    formatCode: true
  }))

  programIdImports.push(item.outName + "::" + item.outName.toUpperCase() + "_ID,")
}

console.log('- COPY AND PASTE THE FOLLOWING INTO src/parser/generated/mod.rs: ');
for (const item of generatedModuleImports) {
  console.log(item);
}

console.log('\n\n- COPY AND PASTE THE FOLLOWING INTO src/lib.rs: ');
console.log('use parser::generated::{')
for (const item of programIdImports) {
  console.log(item);
}
console.log('};')


function getInstructionDiscriminator(ixName) {
  let name = snakeCase(ixName);
  let preimage = `global:${name}`;
  return Buffer.from(sha256(preimage).slice(0, 8));
}

function getAccountDiscriminator(accountName) {
  const preimage = `account:${camelcase(accountName, {
    pascalCase: true,
    preserveConsecutiveUppercase: true,
  })}`;
  return Buffer.from(sha256(preimage).slice(0, 8));
}

// function toPascalCase (str) {
//   if (/^[a-z\d]+$/i.test(str)) {
//     return str.charAt(0).toUpperCase() + str.slice(1);
//   }
//   return str.replace(
//     /([a-z\d])([a-z\d]*)/gi,
//     (g0, g1, g2) => g1.toUpperCase() + g2.toLowerCase()
//   ).replace(/[^a-z\d]/gi, '');
// }

export function capitalize(str) {
  if (str.length === 0) return str;
  return str.charAt(0).toUpperCase() + str.slice(1).toLowerCase();
}

export function titleCase(str) {
  return str
    .replace(/([A-Z])/g, ' $1')
    .split(/[-_\s+.]/)
    .filter((word) => word.length > 0)
    .map(capitalize)
    .join(' ');
}

export function pascalCase(str) {
  return titleCase(str).split(' ').join('');
}

export function camelCase(str) {
  if (str.length === 0) return str;
  const pascalStr = pascalCase(str);
  return pascalStr.charAt(0).toLowerCase() + pascalStr.slice(1);
}