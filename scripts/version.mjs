/**
 * One version, in every place that carries one.
 *
 * The crates, the two packages, the peer range between them, the
 * constant the package reports itself by and the lockfiles that
 * mirror all of it name the same release. A tag argument holds them
 * to it as well, which is what a release runs before it publishes
 * anything.
 *
 *   node scripts/version.mjs            compare
 *   node scripts/version.mjs v0.1.0     compare, and against the tag
 *   node scripts/version.mjs --set 0.1.0
 *   node scripts/version.mjs --bump patch
 *
 * The two writing modes print the version they wrote and nothing
 * else, so a release can read the next tag out of one of them.
 */

import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const SEMVER = /^\d+\.\d+\.\d+(-[\w.]+)?$/;

/** A place a version is written, and how it is read and rewritten. */
const places = [
  text('Cargo.toml', 'workspace.package', /(\[workspace\.package\][\s\S]*?\nversion = ")([^"]+)(")/),
  ...['fleuron', 'fleuron-cli', 'fleuron-fixtures', 'fleuron-markdown', 'fleuron-wasm'].map((crate) =>
    text('Cargo.lock', crate, new RegExp(`(name = "${crate}"\\nversion = ")([^"]+)(")`)),
  ),
  json('crates/fleuron-wasm/npm/package.json', ['version']),
  json('crates/fleuron-wasm/npm/package-lock.json', ['version']),
  json('crates/fleuron-wasm/npm/package-lock.json', ['packages', '', 'version']),
  text('crates/fleuron-wasm/npm/src/version.ts', 'VERSION', /(VERSION = ')([^']+)(')/),
  json('packages/react/package.json', ['version']),
  json('packages/react/package.json', ['peerDependencies', '@fleuron/wasm']),
  json('packages/react/package-lock.json', ['version']),
  json('packages/react/package-lock.json', ['packages', '', 'version']),
  json('packages/react/package-lock.json', ['packages', '', 'peerDependencies', '@fleuron/wasm']),
  json('packages/react/package-lock.json', ['packages', '../../crates/fleuron-wasm/npm', 'version']),
  json('site/package-lock.json', ['packages', '../crates/fleuron-wasm/npm', 'version']),
];

function text(path, label, pattern) {
  return {
    name: `${path} ${label}`,
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

/** What every place says, and the one version they agree on. */
function read() {
  const found = places.map((place) => ({ name: place.name, version: place.read() }));
  const versions = new Set(found.map((place) => place.version));
  const agreed = versions.size === 1 && !versions.has(undefined) ? [...versions][0] : undefined;
  return { found, versions, agreed };
}

function write(version) {
  for (const place of places) {
    place.write(version);
  }
  console.log(version);
}

function fail(why) {
  console.error(why);
  process.exit(1);
}

const [mode, argument] = process.argv.slice(2);

if (mode === '--set') {
  if (!SEMVER.test(argument ?? '')) {
    fail('--set takes a version, as 0.1.0');
  }
  write(argument);
} else if (mode === '--bump') {
  const { agreed, versions } = read();
  if (agreed === undefined) {
    fail(`these do not name one version: ${[...versions].join(', ')}`);
  }
  const [major, minor, patch] = agreed.split(/[.-]/).map(Number);
  const raised = {
    major: `${major + 1}.0.0`,
    minor: `${major}.${minor + 1}.0`,
    patch: `${major}.${minor}.${patch + 1}`,
  }[argument ?? ''];
  if (raised === undefined) {
    fail('--bump takes major, minor or patch');
  }
  write(raised);
} else {
  const tag = mode?.replace(/^v/, '');
  const { found, versions } = read();
  for (const { name, version } of found) {
    console.log(`  ${version ?? '(not found)'}  ${name}`);
  }
  if (tag !== undefined) {
    console.log(`  ${tag}  the tag`);
    versions.add(tag);
  }
  if (versions.size !== 1 || versions.has(undefined)) {
    fail(`\nthese do not name one version: ${[...versions].join(', ')}`);
  }
  console.log(`\none version everywhere: ${[...versions][0]}`);
}
