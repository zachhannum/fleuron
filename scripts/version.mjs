/**
 * One version, in every place that carries one.
 *
 * The crates, the two packages, the peer range between them and the
 * constant the package reports itself by all name the same release.
 * A tag argument holds them to it as well, which is what a release
 * runs before it publishes anything.
 *
 *   node scripts/version.mjs            compare
 *   node scripts/version.mjs v0.1.0     compare, and against the tag
 *   node scripts/version.mjs --set 0.1.0
 */

import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));

/** A place a version is written, and how it is read and rewritten. */
const places = [
  text('Cargo.toml', /(\[workspace\.package\][\s\S]*?\nversion = ")([^"]+)(")/),
  json('crates/fleuron-wasm/npm/package.json', ['version']),
  json('crates/fleuron-wasm/npm/package-lock.json', ['version']),
  json('crates/fleuron-wasm/npm/package-lock.json', ['packages', '', 'version']),
  text('crates/fleuron-wasm/npm/src/version.ts', /(VERSION = ')([^']+)(')/),
  json('packages/react/package.json', ['version']),
  json('packages/react/package.json', ['peerDependencies', '@fleuron/wasm']),
  json('packages/react/package-lock.json', ['version']),
  json('packages/react/package-lock.json', ['packages', '', 'version']),
  json('packages/react/package-lock.json', ['packages', '', 'peerDependencies', '@fleuron/wasm']),
  json('packages/react/package-lock.json', ['packages', '../../crates/fleuron-wasm/npm', 'version']),
  json('site/package-lock.json', ['packages', '../crates/fleuron-wasm/npm', 'version']),
];

function text(path, pattern) {
  return {
    name: path,
    read: () => pattern.exec(readFileSync(root + path, 'utf8'))?.[2],
    write: (version) =>
      writeFileSync(
        root + path,
        readFileSync(root + path, 'utf8').replace(pattern, `$1${version}$3`),
      ),
  };
}

function json(path, keys) {
  const at = (document) => keys.slice(0, -1).reduce((node, key) => node?.[key], document);
  const last = keys[keys.length - 1];
  return {
    name: `${path} ${keys.join('.')}`,
    read: () => at(JSON.parse(readFileSync(root + path, 'utf8')))?.[last],
    write: (version) => {
      const document = JSON.parse(readFileSync(root + path, 'utf8'));
      at(document)[last] = version;
      writeFileSync(root + path, `${JSON.stringify(document, null, 2)}\n`);
    },
  };
}

const args = process.argv.slice(2);
const setting = args[0] === '--set' ? args[1] : undefined;

if (args[0] === '--set') {
  if (setting === undefined || !/^\d+\.\d+\.\d+(-[\w.]+)?$/.test(setting)) {
    console.error('--set takes a version, as 0.1.0');
    process.exit(2);
  }
  for (const place of places) {
    place.write(setting);
  }
  console.log(`every version now reads ${setting}`);
  process.exit(0);
}

const tag = args[0]?.replace(/^v/, '');
const found = places.map((place) => ({ name: place.name, version: place.read() }));
for (const { name, version } of found) {
  console.log(`  ${version ?? '(not found)'}  ${name}`);
}

const versions = new Set(found.map(({ version }) => version));
if (tag !== undefined) {
  console.log(`  ${tag}  the tag`);
  versions.add(tag);
}

if (versions.size !== 1 || versions.has(undefined)) {
  console.error(`\nthese do not name one version: ${[...versions].join(', ')}`);
  process.exit(1);
}
console.log(`\none version everywhere: ${[...versions][0]}`);
