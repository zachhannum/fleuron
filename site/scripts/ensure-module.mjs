/**
 * The module the site runs, built from the working tree.
 *
 * A demo is the engine rather than a picture of one, so a site served
 * from a module older than the sources under it shows pages the
 * working tree does not produce. A property the engine now honours
 * warns in the playground, and a poster stands in for an older
 * engine.
 *
 * The module, the dependencies its package builds under, and the
 * package itself are each held against what they were last built
 * from, and only the ones behind it are built. Where none of them
 * are, this costs a walk over a few directories.
 */

import { execFileSync } from 'node:child_process';
import { readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));
const npm = join(root, 'crates/fleuron-wasm/npm');

/**
 * One thing the site consumes: what it is built into, and what it is
 * built from. A source that is a directory stands for everything
 * under it.
 */
const MODULE = {
  what: 'the module',
  built: join(npm, 'wasm/fleuron_bg.wasm'),
  sources: [join(root, 'crates'), join(root, 'Cargo.toml'), join(root, 'Cargo.lock')],
  // The TypeScript beside the module sits under `crates` too, and
  // builds into the package rather than into the module.
  skip: [npm],
};

/** The package's own dependencies, which its build needs to run. */
const DEPS = {
  what: 'the package dependencies',
  built: join(npm, 'node_modules/.package-lock.json'),
  sources: [join(npm, 'package-lock.json')],
  skip: [],
};

/** The TypeScript beside the module, which reads the module's types. */
const PACKAGE = {
  what: 'the package',
  built: join(npm, 'dist/index.js'),
  sources: [
    join(npm, 'src'),
    join(npm, 'wasm/fleuron.d.ts'),
    join(npm, 'tsconfig.json'),
    join(npm, 'package.json'),
  ],
  skip: [],
};

/**
 * The newest mtime at a path, or zero where there is nothing there.
 * For a directory, the newest under it.
 */
function newest(path, skip) {
  if (skip.includes(path)) {
    return 0;
  }
  let stats;
  try {
    stats = statSync(path);
  } catch {
    return 0;
  }
  if (!stats.isDirectory()) {
    return stats.mtimeMs;
  }
  let latest = 0;
  for (const entry of readdirSync(path, { withFileTypes: true })) {
    // Neither what a build wrote nor what a package manager fetched
    // is a source of anything.
    if (entry.name.startsWith('.') || entry.name === 'node_modules' || entry.name === 'target') {
      continue;
    }
    latest = Math.max(latest, newest(join(path, entry.name), skip));
  }
  return latest;
}

/** Whether a source has moved since the build that read it. */
function behind({ built, sources, skip }) {
  const at = newest(built, []);
  return at === 0 || sources.some((source) => newest(source, skip) > at);
}

function run(command, args, cwd) {
  execFileSync(command, args, { cwd, stdio: 'inherit' });
}

/** wasm-pack, or how it is installed. */
function wasmPack() {
  try {
    execFileSync('wasm-pack', ['--version'], { stdio: 'ignore' });
  } catch {
    console.error(
      'the module is out of date and wasm-pack is not installed:\n  cargo install wasm-pack',
    );
    process.exit(1);
  }
}

const stale = [MODULE, DEPS, PACKAGE].filter(behind);
if (stale.length === 0) {
  console.log('the module is current');
} else {
  console.log(`building: ${stale.map((one) => one.what).join(', ')}`);
}

if (stale.includes(MODULE)) {
  wasmPack();
  // The same build the docs workflow runs, so what a reader is
  // served and what a developer sees come off one command.
  run(
    'wasm-pack',
    [
      'build',
      'crates/fleuron-wasm',
      '--target',
      'web',
      '--release',
      '--no-pack',
      '--out-dir',
      'npm/wasm',
      '--out-name',
      'fleuron',
    ],
    root,
  );
}

if (stale.includes(DEPS)) {
  run('npm', ['ci'], npm);
}

// A module that was just built rewrote the types the package reads,
// so this follows it whether or not it was behind to begin with.
if (stale.includes(MODULE) || stale.includes(PACKAGE)) {
  run('npm', ['run', 'build'], npm);
}
