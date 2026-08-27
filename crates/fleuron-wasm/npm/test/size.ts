/**
 * The size budget: what a host downloads to lay out a book.
 *
 * The module carries its own text face, its own hyphenation
 * patterns and its own segmentation data, because the engine reads
 * no files and asks the host for nothing. That is a fair trade at a
 * few megabytes and a bad one at ten, so the number is checked
 * rather than watched.
 */

import { gzipSync } from 'node:zlib';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

/** What the module may weigh over the wire, compressed as a server sends it. */
const BUDGET = 5 * 1024 * 1024;

const pkg = fileURLToPath(new URL('../', import.meta.url));

function gzipped(path: string): number {
  return gzipSync(readFileSync(path), { level: 9 }).byteLength;
}

/** Every shipped file under a directory, gzipped, in bytes. */
function shipped(dir: string): number {
  return readdirSync(dir, { withFileTypes: true })
    .filter((entry) => !entry.name.endsWith('.map') && !entry.name.endsWith('.d.ts'))
    .reduce((total, entry) => {
      const path = join(dir, entry.name);
      return total + (entry.isDirectory() ? shipped(path) : gzipped(path));
    }, 0);
}

const wasm = join(pkg, 'wasm', 'fleuron_bg.wasm');
const module_ = gzipped(wasm);
const glue = gzipped(join(pkg, 'wasm', 'fleuron.js'));
const client = shipped(join(pkg, 'dist'));
const total = module_ + glue + client;

const mib = (bytes: number): string => `${(bytes / 1024 / 1024).toFixed(2)} MiB`;
const kib = (bytes: number): string => `${(bytes / 1024).toFixed(1)} KiB`;

console.log('### @fleuron/wasm size\n');
console.log('| part | raw | gzipped |');
console.log('|---|---|---|');
console.log(`| \`fleuron_bg.wasm\` | ${mib(statSync(wasm).size)} | ${mib(module_)} |`);
console.log(`| bindgen glue | | ${kib(glue)} |`);
console.log(`| client and worker | | ${kib(client)} |`);
console.log(`| **total** | | **${mib(total)}** |`);
console.log(
  `\nBudget ${mib(BUDGET)} gzipped, ${(((BUDGET - total) / BUDGET) * 100).toFixed(0)}% headroom.`,
);

if (total > BUDGET) {
  console.log(`\nover budget by ${mib(total - BUDGET)}`);
  process.exit(1);
}
