/**
 * The demos, driven: the same mechanical check the painter's own
 * suite makes, run against an island in a real browser.
 *
 * What is held against what: the display list the worker sent the
 * page, read off the mounted demo, and the SVG that demo put in the
 * DOM. Every glyph the engine placed has to be at that x in the
 * markup a reader is actually looking at. A painter that drifts, or
 * an island that repaints from something other than the bytes it was
 * given, fails here.
 *
 * The page is also opened with scripting off, because a poster that
 * does not survive that is not a poster.
 */

import { createReadStream } from 'node:fs';
import { stat } from 'node:fs/promises';
import { createServer } from 'node:http';
import { extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';

import { chromium } from 'playwright';

const dist = fileURLToPath(new URL('../dist/', import.meta.url));

const types = {
  '.css': 'text/css',
  '.html': 'text/html',
  '.js': 'text/javascript',
  '.json': 'application/json',
  '.md': 'text/markdown',
  '.svg': 'image/svg+xml',
  '.ttf': 'font/ttf',
  '.wasm': 'application/wasm',
  '.woff2': 'font/woff2',
};

let failures = 0;

function check(what, passed, detail = '') {
  console.log(`  ${passed ? 'ok  ' : 'FAIL'}  ${what}${detail === '' ? '' : `\n          ${detail}`}`);
  if (!passed) {
    failures += 1;
  }
}

const server = createServer(async (request, response) => {
  const asked = new URL(request.url ?? '/', 'http://x').pathname;
  const path = join(dist, normalize(decodeURIComponent(asked)));
  if (!path.startsWith(dist)) {
    response.writeHead(403).end();
    return;
  }
  try {
    const found = (await stat(path)).isDirectory() ? join(path, 'index.html') : path;
    await stat(found);
    response.writeHead(200, {
      'content-type': types[extname(found)] ?? 'application/octet-stream',
      'cache-control': 'no-store',
    });
    createReadStream(found)
      .on('error', () => response.end())
      .pipe(response);
  } catch {
    response.writeHead(404).end();
  }
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const { port } = server.address();
const at = `http://127.0.0.1:${port}/`;

console.log('fleuron island run: the landing demo, driven\n');

const browser = await chromium.launch();

// Scripting off. What is left is what the server sent, and it has to
// be a typeset page.
const quiet = await browser.newContext({ javaScriptEnabled: false });
const still = await quiet.newPage();
await still.goto(at, { waitUntil: 'load' });
const poster = await still.evaluate(() => {
  const svg = document.querySelector('.d-poster svg');
  return { runs: svg?.querySelectorAll('text').length ?? 0, page: svg?.getAttribute('data-page') };
});
check(
  'the page is a typeset page with scripting off',
  poster.runs > 0 && poster.page === '1',
  `${poster.runs} runs on page ${poster.page}`,
);
await quiet.close();

const page = await browser.newPage();
const broke = [];
page.on('pageerror', (error) => broke.push(String(error)));
await page.goto(at, { waitUntil: 'load' });

await page.waitForFunction(() => globalThis.fleuron?.get('playground')?.output != null, undefined, {
  timeout: 180_000,
});
check('the engine runs in the browser and the island paints what it was sent', true);

const state = await page.evaluate(() => {
  const svg = document.querySelector('.d-live svg');
  return {
    pages: globalThis.fleuron.get('playground').output.pages.length,
    showing: svg?.getAttribute('data-page') ?? null,
    live: getComputedStyle(document.querySelector('.d-poster')).display,
  };
});
check('the poster gives way to the page the worker sent', state.live === 'none', `page ${state.showing}`);
check('the book set in more than one page', state.pages > 1, `${state.pages} pages`);

// No engine on the main thread: the module is fetched by the worker,
// and the window has never seen a WebAssembly instance.
const wall = await page.evaluate(() => ({
  requests: performance
    .getEntriesByType('resource')
    .filter((entry) => entry.name.endsWith('.wasm'))
    .map((entry) => entry.name),
  session: 'Session' in globalThis,
}));
check(
  'no wasm module is fetched by the page, and no engine reaches the window',
  wall.requests.length === 0 && !wall.session,
  wall.requests.join(', '),
);

// Every glyph of every page, against the SVG that page is painted as.
const misplaced = await page.evaluate(async () => {
  const entry = globalThis.fleuron.get('playground');
  const characterAt = (text, byte) => {
    let at = 0;
    let seen = 0;
    for (const character of text) {
      if (seen >= byte) return at;
      const code = character.codePointAt(0) ?? 0;
      seen += code < 0x80 ? 1 : code < 0x800 ? 2 : code < 0x10000 ? 3 : 4;
      at += 1;
    }
    return at;
  };
  const turn = (to) =>
    new Promise((resolve) => {
      document.querySelector('[aria-label="Next page"]').click();
      const wait = () => {
        const svg = document.querySelector('.d-live svg');
        if (svg?.getAttribute('data-page') === String(to)) resolve(svg);
        else requestAnimationFrame(wait);
      };
      wait();
    });

  const wrong = [];
  for (const [index, sheet] of entry.output.pages.entries()) {
    const svg =
      index === 0 ? document.querySelector('.d-live svg') : await turn(sheet.number);
    const painted = [...svg.querySelectorAll('text')];
    const runs = sheet.items.filter((item) => item.kind === 'text');
    if (painted.length !== runs.length) {
      wrong.push(`page ${sheet.number}: ${runs.length} runs, ${painted.length} <text>`);
      continue;
    }
    for (const [at, run] of runs.entries()) {
      const element = painted[at];
      if (element.textContent !== run.text) {
        wrong.push(`page ${sheet.number} run ${at} paints the wrong text`);
        break;
      }
      const xs = element.getAttribute('x').split(' ');
      for (const glyph of run.glyphs) {
        const written = xs[characterAt(run.text, glyph.range[0])];
        if (written === undefined || Math.fround(Number(written)) !== glyph.x) {
          wrong.push(`page ${sheet.number} run ${at} puts a glyph at ${written}, not ${glyph.x}`);
          break;
        }
      }
    }
  }
  return wrong;
});
check(
  'every glyph on every page sits at the x the display list gave it',
  misplaced.length === 0,
  misplaced.slice(0, 3).join('; '),
);

// An edit reaches the engine and comes back as a different book.
// Typed the way a reader types it, through the controls the page
// actually offers.
const before = await page.evaluate(
  () => globalThis.fleuron.get('playground').output.pages.length,
);
await page.getByRole('tab', { name: 'Stylesheet' }).click();
const editor = page.locator('.d-area');
await editor.fill(`${await editor.inputValue()}\nbook { font-size: 18pt; }\n`);
await page
  .waitForFunction(
    (was) => globalThis.fleuron.get('playground').output.pages.length !== was,
    before,
    { timeout: 60_000 },
  )
  .catch(() => undefined);
const after = await page.evaluate(
  () => globalThis.fleuron.get('playground').output.pages.length,
);
check(
  'an edit to the stylesheet lays the book out again',
  after > before,
  `${before} pages, then ${after}`,
);

// And the edit is in the URL, so the demo can be linked to.
const shared = await page.evaluate(() => location.hash);
check('the playground round-trips its state through the URL', shared.startsWith('#try='));
const reopened = await browser.newPage();
await reopened.goto(`${at}${shared}`, { waitUntil: 'load' });
await reopened.waitForFunction(
  () => globalThis.fleuron?.get('playground')?.output != null,
  undefined,
  { timeout: 180_000 },
);
const restored = await reopened.evaluate(
  () => globalThis.fleuron.get('playground').output.pages.length,
);
check(
  'and a link to it opens the book that was shared',
  restored === after,
  `${restored} pages, shared ${after}`,
);

check('nothing threw on the page', broke.length === 0, broke.slice(0, 2).join('; '));

// The other two demos, on the page that carries them.
const demos = await browser.newPage();
const demosBroke = [];
demos.on('pageerror', (error) => demosBroke.push(String(error)));
await demos.goto(`${at}demos/`, { waitUntil: 'load' });

await demos.locator('.d-bench').scrollIntoViewIfNeeded();
await demos.getByRole('button', { name: 'Run it here' }).click();
await demos.waitForSelector('.d-table tbody tr:nth-child(4)', { timeout: 300_000 });
const bench = await demos.evaluate(() => ({
  machine: document.querySelector('.d-machine')?.textContent ?? '',
  rows: [...document.querySelectorAll('.d-table tbody tr')].map((row) => ({
    what: row.querySelector('th')?.textContent ?? '',
    stages: row.querySelector('.d-stages')?.textContent ?? '',
    clock: row.querySelector('.d-ms')?.textContent ?? '',
  })),
  pages: document.querySelector('.d-bench .d-note')?.textContent ?? '',
}));
check(
  'the bench times a whole novel in this browser, stage by stage',
  bench.rows.length === 4 &&
    bench.rows.every((row) => /\d/.test(row.clock)) &&
    bench.rows.some((row) => row.stages.includes('lines')),
  bench.rows.map((row) => `${row.what}: ${row.stages}, ${row.clock}`).join('; '),
);
check('and says which machine it ran on', bench.machine.length > 0, bench.machine);

await demos.locator('.d-diagnostics').scrollIntoViewIfNeeded();
await demos.waitForSelector('.d-warnlist li', { timeout: 300_000 });
const reported = await demos.evaluate(() =>
  [...document.querySelectorAll('.d-warnlist li')].map((item) => ({
    origin: item.querySelector('code')?.textContent ?? '',
    message: item.querySelector('span')?.textContent ?? '',
  })),
);
check(
  'CSS outside the subset is reported at the line and column it was written at',
  reported.length > 0 && reported.every((warning) => /:\d+:\d+$/.test(warning.origin)),
  reported.map((warning) => `${warning.origin} ${warning.message}`).join('; '),
);
check(
  'nothing threw on the demos page',
  demosBroke.length === 0,
  demosBroke.slice(0, 2).join('; '),
);

await browser.close();
await new Promise((resolve) => server.close(resolve));

console.log(failures === 0 ? '\nall checks passed' : `\n${failures} check(s) failed`);
process.exit(failures === 0 ? 0 : 1);
