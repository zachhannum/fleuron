/**
 * What the landing page may cost before it is legible.
 *
 * The page now carries a demo, so it is no longer scriptless. What
 * it must still be is a typeset page the moment the HTML lands: the
 * poster is in the markup, nothing blocks the parser, and neither
 * the module nor the corpus is on the way to first paint. A stray
 * integration that starts inlining an engine should fail the build
 * rather than the Lighthouse run.
 */

import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const page = fileURLToPath(new URL('../dist/index.html', import.meta.url));
const demos = fileURLToPath(new URL('../src/styles/demos.css', import.meta.url));
const html = await readFile(page, 'utf8');
// Comments say why there is none; the check is about declarations.
const css = (await readFile(demos, 'utf8')).replace(/\/\*[\s\S]*?\*\//g, '');

let failed = 0;

function check(what, passed, detail = '') {
  console.log(`${passed ? 'ok  ' : 'FAIL'} ${what}${detail === '' ? '' : `\n       ${detail}`}`);
  if (!passed) failed += 1;
}

// Nothing may stop the parser. An inline script and a module script
// both run after the markup they follow; a plain `src` script does
// not.
const blocking = (html.match(/<script\b[^>]*>/g) ?? []).filter(
  (tag) => / src=/.test(tag) && !/type="module"/.test(tag) && !/\bdefer\b|\basync\b/.test(tag),
);
check('nothing on the landing page blocks the parser', blocking.length === 0, blocking.join('\n       '));

// The engine is the worker's business. Naming it in the markup is
// how it ends up on the critical path.
check(
  'the module is not named in the markup',
  !html.includes('.wasm'),
  (html.match(/[^"'\s]*\.wasm/g) ?? []).join(', '),
);
check(
  'the corpus is not named in the markup',
  !html.includes('/fixtures/'),
  (html.match(/[^"'\s]*\/fixtures\/[^"'\s]*/g) ?? []).join(', '),
);
check(
  'nothing but the display face is preloaded',
  (html.match(/rel="preload"/g) ?? []).length === 1 && html.includes('as="font"'),
);

// A reader whose scripts never run still gets a book.
const runs = (html.match(/<text\b/g) ?? []).length;
check('the page the server sends is a typeset page', runs > 0, `${runs} runs of shaped text`);

// Glass goes around a demo, never on it.
check('no demo container is a blurred surface', !css.includes('backdrop-filter'));

if (failed > 0) {
  console.error(`\n${failed} landing check(s) failed`);
  process.exit(1);
}
console.log('\nthe landing page is a typeset page before anything runs');
