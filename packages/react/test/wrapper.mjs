/**
 * The wrapper stays a wrapper.
 *
 * The claim this package makes is a negative one: there is no engine
 * logic in here. So what is checked is what the built module reaches
 * for, React and one name from `@fleuron/wasm`, and that the
 * vocabulary of the engine appears nowhere in it.
 */

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const built = readFileSync(fileURLToPath(new URL('../dist/index.js', import.meta.url)), 'utf8');

let failures = 0;

function check(what, passed, detail = '') {
  console.log(`  ${passed ? 'ok  ' : 'FAIL'}  ${what}${detail === '' ? '' : `\n          ${detail}`}`);
  if (!passed) {
    failures += 1;
  }
}

console.log('@fleuron/react: the wrapper stays a wrapper\n');

const imports = [...built.matchAll(/^import\s+(?:(.+?)\s+from\s+)?["']([^"']+)["'];$/gm)].map(
  ([, names, from]) => ({ names: names ?? '', from }),
);

check(
  'it reaches for React and the bindings, and nothing else',
  imports.every(({ from }) => ['react', 'react/jsx-runtime', '@fleuron/wasm'].includes(from)),
  imports.map(({ from }) => from).join(', '),
);

const fromBindings = imports.filter(({ from }) => from === '@fleuron/wasm');
check(
  'and takes one name from the bindings: the preview it wraps',
  fromBindings.length === 1 && /^\{\s*Preview(\s+as\s+\w+)?\s*\}$/.test(fromBindings[0].names),
  fromBindings.map(({ names }) => names).join(', '),
);

const engine = ['postcard', 'decodeDisplayList', 'Worker', 'Client', 'paintPage', 'wire'];
const found = engine.filter((word) => built.includes(word));
check('and knows none of the words the engine is built out of', found.length === 0, found.join(', '));

console.log(failures === 0 ? '\nall checks passed' : `\n${failures} check(s) failed`);
process.exit(failures === 0 ? 0 : 1);
