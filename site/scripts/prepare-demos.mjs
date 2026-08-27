/**
 * What the demos need before the site is built: the module, the
 * corpus book the bench lays out, and a poster for every demo.
 *
 * The posters are painted here, by the engine, from the same inputs
 * the island will hand it. A poster is therefore a real page rather
 * than a picture of one, and cannot disagree with the demo it stands
 * in for, which is what makes it safe to serve to a reader whose
 * JavaScript never runs.
 *
 * Every glyph on every poster is checked against the display list it
 * was painted from, so a painter that drifts fails the build here
 * rather than on a page nobody measured.
 */

import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';

import { DEMOS, demo } from '../src/demos/catalogue.mjs';

const root = new URL('../../', import.meta.url);
const site = new URL('../', import.meta.url);
const module_ = new URL('crates/fleuron-wasm/npm/wasm/fleuron_bg.wasm', root);

let missing = false;
try {
  readFileSync(module_);
} catch {
  missing = true;
}
if (missing) {
  console.error(
    'the module is not built. From the repository root:\n' +
      '  wasm-pack build crates/fleuron-wasm --target web --release --no-pack \\\n' +
      '    --out-dir npm/wasm --out-name fleuron\n' +
      '  npm --prefix crates/fleuron-wasm/npm ci && npm --prefix crates/fleuron-wasm/npm run build',
  );
  process.exit(1);
}

const { Session, decodeDisplayList, initWasm, paintPage } = await import('@fleuron/wasm');

// The module, and one session per demo: a session holds the whole
// pipeline, and two demos with different stylesheets have nothing to
// share.
await initWasm({ module_or_path: readFileSync(module_) });

const posters = new URL('src/generated/posters/', site);
rmSync(posters, { recursive: true, force: true });
mkdirSync(posters, { recursive: true });

let wrong = 0;
for (const id of Object.keys(DEMOS)) {
  const { name, markdown, css, page, dialect } = demo(id);
  const session = new Session();
  if (dialect !== undefined) {
    session.setDialect(dialect);
  }
  session.setMarkdown(name, markdown);
  session.setStyle(css);
  const output = decodeDisplayList(session.preview());
  session.free();

  const sheet = output.pages[Math.min(page, output.pages.length) - 1];
  if (sheet === undefined) {
    throw new Error(`demo ${id} set no pages`);
  }
  const svg = paintPage(sheet, { fonts: output.fonts, paper: null, ink: 'currentColor' });
  writeFileSync(new URL(`${id}.svg`, posters), `${lighter(svg)}\n`);

  const misplaced = displaced(sheet, svg) ?? displaced(sheet, lighter(svg), 0.051);
  if (misplaced !== null) {
    console.error(`  FAIL  ${id}: ${misplaced}`);
    wrong += 1;
  } else {
    console.log(
      `  ok    ${id}: page ${sheet.number} of ${output.pages.length}, ` +
        `${sheet.items.length} items, every glyph where the display list put it`,
    );
  }
}

// The module and the book the bench lays out are fetched by the
// worker rather than bundled, so they are served as files.
cpSync(module_, new URL('public/fleuron_bg.wasm', site));
mkdirSync(new URL('public/fixtures/', site), { recursive: true });
cpSync(
  new URL('fixtures/corpus/pride-and-prejudice.md', root),
  new URL('public/fixtures/pride-and-prejudice.md', site),
);

if (wrong > 0) {
  console.error(`${wrong} poster(s) disagree with the display list they were painted from`);
  process.exit(1);
}
console.log('demos prepared: the module, the bench corpus, and a poster each');

/**
 * The same page, in half the bytes.
 *
 * A poster is markup in every document that shows one, and the
 * painter writes every position to the last digit a 32-bit float
 * has, because a preview is checked against the display list to that
 * digit. A picture is not: a tenth of a point is a seven-thousandth
 * of an inch, and the attributes every run repeats are the same
 * attributes on all of them.
 */
function lighter(svg) {
  const shared = {};
  const runs = [...svg.matchAll(/<text\b([^>]*)>/g)].map((run) => attributes(run[1]));
  for (const name of ['font-family', 'font-weight', 'font-style', 'style', 'xml:space']) {
    const first = runs[0]?.[name];
    if (first !== undefined && runs.every((run) => run[name] === first)) {
      shared[name] = first;
    }
  }
  const hoisted = Object.entries(shared)
    .map(([name, value]) => ` ${name}="${value}"`)
    .join('');
  return svg
    .replace(/<svg\b([^>]*)>/, (_, rest) => `<svg${rest}${hoisted}>`)
    .replace(/<text\b([^>]*)>/g, (whole, rest) => {
      let out = rest;
      for (const name of Object.keys(shared)) {
        out = out.replace(new RegExp(`\\s${name.replace(':', '\\:')}="[^"]*"`), '');
      }
      return `<text${out}>`;
    })
    .replace(/\b(x|y)="([^"]*)"/g, (_, name, value) => `${name}="${value
      .split(' ')
      .map((number) => String(Math.round(Number(number) * 10) / 10))
      .join(' ')}"`);
}

/** One element's attributes, by name. */
function attributes(text) {
  return Object.fromEntries(
    [...text.matchAll(/([\w:-]+)="([^"]*)"/g)].map((found) => [found[1], found[2]]),
  );
}

/** Every glyph of a page, held against the x the painter wrote. */
function displaced(page, svg, slack = 0) {
  const painted = [...svg.matchAll(/<text\b([^>]*)>([\s\S]*?)<\/text>/g)].map((element) => ({
    x: (/ x="([^"]*)"/.exec(element[1] ?? '')?.[1] ?? '').split(' ').filter((n) => n !== ''),
    content: unescape_(element[2] ?? ''),
  }));
  const runs = page.items.filter((item) => item.kind === 'text');
  if (painted.length !== runs.length) {
    return `${runs.length} runs and ${painted.length} <text>`;
  }
  for (const [at, run] of runs.entries()) {
    const element = painted[at];
    if (element === undefined || element.content !== run.text) {
      return `run ${at} paints ${JSON.stringify(element?.content)}, not ${JSON.stringify(run.text)}`;
    }
    for (const glyph of run.glyphs) {
      const index = characterAt(run.text, glyph.range[0]);
      const written = element.x[index];
      const apart =
        written === undefined ? Infinity : Math.abs(Math.fround(Number(written)) - glyph.x);
      if (!(apart <= slack)) {
        return `run ${at} puts character ${index} at ${written}, not ${glyph.x}`;
      }
    }
  }
  return null;
}

/** Which character of a run a byte offset falls on. */
function characterAt(text, byte) {
  return [...Buffer.from(text, 'utf8').subarray(0, byte).toString('utf8')].length;
}

function unescape_(markup) {
  return markup
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&amp;/g, '&');
}
