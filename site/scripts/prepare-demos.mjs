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

/**
 * The face a poster is drawn in.
 *
 * A poster is painted before any engine face exists on the page, so
 * the painter's stack has to fall through to something. The site's
 * display face is subset from the file the engine shapes with, so it
 * is the one thing on the page whose metrics are the ones the
 * display list was measured against.
 */
const POSTER_FACE = 'EB Garamond Subset';

// The module, and one session per demo: a session holds the whole
// pipeline, and two demos with different stylesheets have nothing to
// share.
await initWasm({ module_or_path: readFileSync(module_) });

const posters = new URL('src/generated/posters/', site);
rmSync(posters, { recursive: true, force: true });
mkdirSync(posters, { recursive: true });

let wrong = 0;
for (const id of Object.keys(DEMOS)) {
  const { name, markdown, css, page } = demo(id);
  const session = new Session();
  session.setMarkdown(name, markdown);
  session.setStyle(css);
  const output = decodeDisplayList(session.preview());
  session.free();

  const sheet = output.pages[Math.min(page, output.pages.length) - 1];
  if (sheet === undefined) {
    throw new Error(`demo ${id} set no pages`);
  }
  const fonts = output.fonts.map((font) => ({ ...font, family: POSTER_FACE }));
  const svg = paintPage(sheet, { fonts, paper: null, ink: 'currentColor' });
  writeFileSync(new URL(`${id}.svg`, posters), `${svg}\n`);

  const misplaced = displaced(sheet, svg);
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

/** Every glyph of a page, held against the x the painter wrote. */
function displaced(page, svg) {
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
      if (written === undefined || Math.fround(Number(written)) !== glyph.x) {
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
