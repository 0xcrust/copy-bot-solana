// node ./kinobi.mjs
import { rootNodeFromAnchor } from '@kinobi-so/nodes-from-anchor';
import { createFromRoot } from 'kinobi';
import { readFileSync } from 'node:fs';
import path from 'path'

// Read the content of your IDL file.
const anchorIdlPath = path.join(__dirname, 'target', 'idl', 'anchor_program.json');
const anchorIdl = JSON.parse(readFileSync(anchorIdlPath, 'utf-8'));

// Parse it into a Kinobi IDL.
const kinobi = createFromRoot(rootNodeFromAnchor(anchorIdl));

(() => {

});