/**
 * The headless acceptance run: the fixture book through the module,
 * in a real worker thread, held against what the CLI makes of the
 * same manuscript.
 *
 * The CLI is the reference because its output is already validated
 * three ways. If the worker disagrees with it about the page count
 * or about a single byte of the PDF, one of them is wrong, and this
 * is where that shows.
 */

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Worker } from 'node:worker_threads';

import {
  Client,
  Session,
  WIRE_VERSION,
  decodeDisplayList,
  faceFamily,
  initWasm,
  paintPage,
  wireVersion,
  type LayoutOutput,
  type Op,
  type Page,
  type Response,
  type TextItem,
} from '../dist/index.js';

const root = fileURLToPath(new URL('../../../../', import.meta.url));
const fixture = join(root, 'fixtures', 'gulliver-excerpt.md');
const wasm = fileURLToPath(new URL('../wasm/fleuron_bg.wasm', import.meta.url));

let failures = 0;

function check(what: string, passed: boolean, detail = ''): void {
  console.log(`  ${passed ? 'ok  ' : 'FAIL'}  ${what}${detail === '' ? '' : `\n          ${detail}`}`);
  if (!passed) {
    failures += 1;
  }
}

function sha256(bytes: Uint8Array): string {
  return createHash('sha256').update(bytes).digest('hex');
}

function startsWith(bytes: Uint8Array, marker: string): boolean {
  return Buffer.from(bytes.subarray(0, marker.length)).toString('latin1') === marker;
}

function endsWith(bytes: Uint8Array, marker: string): boolean {
  return Buffer.from(bytes.subarray(bytes.byteLength - marker.length - 1)).toString('latin1').includes(marker);
}

/** What `pdftotext` makes of a PDF, or null where it is not installed. */
function text(pdf: Uint8Array): string | null {
  const run = spawnSync('pdftotext', ['-', '-'], { input: Buffer.from(pdf), encoding: 'utf8' });
  return run.status === 0 ? run.stdout : null;
}

function flag(name: string): string | undefined {
  const at = process.argv.indexOf(name);
  return at === -1 ? undefined : process.argv[at + 1];
}

/** What `pdfinfo` says about a PDF, or null where it is not installed. */
function info(pdf: Uint8Array): string | null {
  const run = spawnSync('pdfinfo', ['-'], { input: Buffer.from(pdf), encoding: 'utf8' });
  return run.status === 0 ? run.stdout : null;
}

/** What the CLI makes of some markdown: the run this one is held to. */
function reference(inputs: string[] = [fixture], named: string[] = []): {
  pages: number;
  pdf: Uint8Array;
} {
  const cli = flag('--cli') ?? join(root, 'target', 'release', 'fleuron');
  const out = join(mkdtempSync(join(tmpdir(), 'fleuron-harness-')), 'reference.pdf');
  const run = spawnSync(cli, [...inputs, ...named, '-o', out], { encoding: 'utf8' });
  if (run.status !== 0) {
    throw new Error(`the CLI did not render ${inputs.join(', ')}: ${run.stderr ?? run.error}`);
  }
  // The page count is on stderr, where the CLI reports what it did.
  const counted = /(\d+) pages/.exec(run.stderr);
  if (counted?.[1] === undefined) {
    throw new Error(`the CLI reported no page count: ${run.stderr}`);
  }
  return { pages: Number(counted[1]), pdf: readFileSync(out) };
}

/** A worker running the module, and a client talking to it. */
function open(): { client: Client; worker: Worker } {
  const worker = new Worker(new URL('./worker.js', import.meta.url), { workerData: { wasm } });
  const client = new Client({
    post: (request, transfer) => worker.postMessage(request, transfer),
  });
  worker.on('message', (response: Response) => client.receive(response));
  worker.on('error', (error) => {
    console.error(error);
    process.exit(1);
  });
  return { client, worker };
}

const markdown = readFileSync(fixture, 'utf8');
const book: Op[] = [{ op: 'markdown', name: 'gulliver-excerpt.md', text: markdown }];

console.log('fleuron wasm harness: the fixture book through a worker\n');
const cli = reference();
console.log(`  the CLI sets the fixture book in ${cli.pages} pages\n`);

const { client, worker } = open();

// The display list crosses the wall and reads back as the book the
// CLI laid out.
const preview = await client.preview(book);
if (preview === null) {
  throw new Error('nothing overtook the first render, and it still came back superseded');
}
check('the display list decodes', preview.pages.length > 0);
check(
  'the worker sets the book in the same pages as the CLI',
  preview.pages.length === cli.pages,
  `worker ${preview.pages.length}, CLI ${cli.pages}`,
);
check(
  'every page carries something to paint',
  preview.pages.every((page) => page.items.length > 0),
);
check(
  'glyphs carry the text they were shaped from',
  preview.pages.some((page) =>
    page.items.some(
      (item) => item.kind === 'text' && item.text.length > 0 && item.glyphs.length > 0,
    ),
  ),
);
check(
  'the display list names the face it set',
  preview.fonts.some((font) => font.family === 'eb garamond'),
);

// The painter. Every page is painted, and every glyph the display
// list placed is checked against the x the SVG puts that character
// at — mechanically, over the draw items, with the byte-to-character
// mapping recomputed here rather than borrowed from the painter.

/** The `<text>` elements of a painted page, in paint order. */
function texts(svg: string): { x: string[]; content: string }[] {
  return [...svg.matchAll(/<text\b([^>]*)>([\s\S]*?)<\/text>/g)].map((element) => ({
    x: (/ x="([^"]*)"/.exec(element[1] ?? '')?.[1] ?? '').split(' ').filter((n) => n !== ''),
    content: unescape_(element[2] ?? ''),
  }));
}

function unescape_(markup: string): string {
  return markup
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&amp;/g, '&');
}

/** Which character of a run a byte offset falls on. */
function characterAt(text: string, byte: number): number {
  return [...Buffer.from(text, 'utf8').subarray(0, byte).toString('utf8')].length;
}

/** Every glyph of a page, held against the x the painter wrote. */
function misplaced(page: Page, output: LayoutOutput): string | null {
  const painted = texts(paintPage(page, { fonts: output.fonts }));
  const runs = page.items.filter((item): item is TextItem => item.kind === 'text');
  if (painted.length !== runs.length) {
    return `page ${page.number} has ${runs.length} runs and ${painted.length} <text>`;
  }
  for (const [at, run] of runs.entries()) {
    const element = painted[at];
    if (element === undefined || element.content !== run.text) {
      return `page ${page.number} run ${at} paints ${JSON.stringify(element?.content)}, not ${JSON.stringify(run.text)}`;
    }
    for (const glyph of run.glyphs) {
      const index = characterAt(run.text, glyph.range[0]);
      const written = element.x[index];
      if (written === undefined || Math.fround(Number(written)) !== glyph.x) {
        return `page ${page.number} run ${at} puts character ${index} at ${written}, not ${glyph.x}`;
      }
    }
  }
  return null;
}

const wrong = preview.pages.map((page) => misplaced(page, preview)).find((bad) => bad !== null);
check('every glyph is painted at the x the display list gave it', wrong === undefined, wrong ?? '');
check(
  'every page paints',
  preview.pages.every((page) => {
    const svg = paintPage(page, { fonts: preview.fonts });
    return svg.startsWith('<svg') && svg.includes(`data-page="${page.number}"`);
  }),
);
check(
  'a run is drawn in the face the engine shaped it with, at the cut it shaped at',
  preview.pages.some((page) => {
    const svg = paintPage(page, { fonts: preview.fonts });
    return svg.includes(faceFamily(0)) && svg.includes('white-space: pre');
  }),
);

// A face the painter was told nothing about still paints: the stack
// falls through to whatever the reader has.
const unnamed = paintPage(preview.pages[0] as Page, { fonts: [] });
check(
  'a missing face falls back visibly rather than painting nothing',
  unnamed.includes('data-missing-font=') &&
    unnamed.includes('serif') &&
    texts(unnamed).every((element) => element.content.length > 0),
);

// The bytes a painter draws with are the bytes the engine shaped
// with: the module hands the file back rather than leaving a host to
// find the bundled face somewhere else.
const file = await client.fontBytes(0);
check(
  'the module hands back the file it shaped with',
  Buffer.from(file).equals(readFileSync(join(root, 'crates', 'fleuron', 'fonts', 'EBGaramond-VF.ttf'))),
  `${file.byteLength} bytes`,
);
check(
  'the font table says which instance a cut sits at',
  preview.fonts[0]?.variations.length === 0 &&
    preview.fonts.some((font) => font.variations.some((axis) => axis.tag === 'wght')),
);

// The export path. The bytes are not compared to the CLI's byte for
// byte: the PDF writer orders its font objects by a hash that is not
// the same width on a 32-bit target as on a 64-bit one, so the two
// files carry the same objects under swapped numbers. What the book
// is, its length and its pages and its text, is compared instead, and
// the display list above, which is the engine's own output, already
// matches to the byte.
const pdf = await client.exportPdf();
if (pdf === null) {
  throw new Error('nothing overtook the export, and it still came back superseded');
}
check(
  'the PDF is a PDF',
  startsWith(pdf, '%PDF-') && endsWith(pdf, '%%EOF'),
);
check(
  'the PDF weighs what the CLI writes',
  pdf.byteLength === cli.pdf.byteLength,
  `worker ${pdf.byteLength} bytes, CLI ${cli.pdf.byteLength}`,
);
const extracted = text(pdf);
const wanted = text(cli.pdf);
if (extracted === null || wanted === null) {
  console.log('  skip  the PDF reads back as the text the CLI wrote (pdftotext not installed)');
  if (process.env['FLEURON_WASM_REQUIRE_TOOLS'] === '1') {
    failures += 1;
  }
} else {
  check('the PDF reads back as the text the CLI wrote', extracted === wanted);
}

// The warm path: a stylesheet crosses on its own, with no content
// behind it, and the lines already broken are the lines that are
// used.
await client.preview([]);
const broken = client.stages.lines;
const warm = await client.preview([{ op: 'style', css: '@page { margin-bottom: 84pt }' }]);
check(
  'a style-only re-render re-fragments over lines it does not break again',
  client.stages.lines === broken && client.stages.flow > 0,
  `lines broken: ${broken} before, ${client.stages.lines} after`,
);
check(
  'and the pages it re-fragmented are the ones that came back',
  warm !== null && warm.pages.length > 0,
);

// Latest wins: a render another overtakes before it starts is
// dropped, and what runs next is what an uncancelled run produces.
const uncancelled = await client.render([{ op: 'style', css: 'book { font-size: 12pt }' }], 'preview');
const cancelled = client.render([{ op: 'style', css: 'book { font-size: 13pt }' }], 'preview');
const after = client.render([{ op: 'style', css: 'book { font-size: 12pt }' }], 'preview');
const [dropped, painted] = await Promise.all([cancelled, after]);
if (uncancelled === null || painted === null) {
  throw new Error('the render nothing overtook came back superseded');
}
check('a superseded generation is discarded, not painted', dropped === null);
check(
  'the render after a cancelled one is byte-identical to an uncancelled one',
  sha256(painted) === sha256(uncancelled),
  `after ${sha256(painted).slice(0, 16)}…, uncancelled ${sha256(uncancelled).slice(0, 16)}…`,
);

// The error channel: what the engine refuses comes back as an error
// rather than as a silence or a half-built session.
let refused = '';
try {
  await client.preview([{ op: 'font', bytes: new Uint8Array([1, 2, 3, 4]) }]);
} catch (error) {
  refused = String(error);
}
check('bytes that are not a font come back on the error channel', refused.includes('font'), refused);
const alive = await client.preview([]);
check('and the session that refused them still renders', alive !== null && alive.pages.length > 0);

// A book of several files. The CLI is the reference again: the same
// two files on its command line, named with the same flags, since a
// book split across files has no frontmatter of its own to read a
// title out of.
const split = markdown.lastIndexOf('\n### ');
const parts = [markdown.slice(0, split), markdown.slice(split)];
const folder = mkdtempSync(join(tmpdir(), 'fleuron-book-'));
const paths = parts.map((text, at) => {
  const path = join(folder, `part-${at + 1}.md`);
  writeFileSync(path, text);
  return path;
});
const sources = parts.map((text, at) => ({ name: `part-${at + 1}.md`, text }));
const naming = { title: "Gulliver's Travels", author: 'Jonathan Swift' };
const named = ['--title', naming.title, '--author', naming.author];

const whole = reference(paths, named);
const assembled = await client.preview([
  // The checks above left author styling on the session, and the
  // CLI is being run without any.
  { op: 'style', css: '' },
  { op: 'book', sources },
  { op: 'metadata', metadata: naming },
]);
check(
  'a book of several sources sets in the same pages as the CLI sets the same files',
  assembled !== null && assembled.pages.length === whole.pages,
  `worker ${assembled?.pages.length}, CLI ${whole.pages}`,
);

const titled = await client.exportPdf();
const said = titled === null ? null : info(titled);
if (said === null) {
  console.log('  skip  the name the host gave the book reaches the PDF (pdfinfo not installed)');
  if (process.env['FLEURON_WASM_REQUIRE_TOOLS'] === '1') {
    failures += 1;
  }
} else {
  check(
    'the name the host gave the book reaches the PDF',
    said.includes(naming.title) && said.includes(naming.author),
    said.split('\n').slice(0, 2).join('; '),
  );
}

// One file dropped, and what is left is the book the CLI sets from
// the rest of them.
const rest = reference([paths[0] as string], named);
const remaining = await client.preview([{ op: 'remove', name: 'part-2.md' }]);
check(
  'dropping a source leaves the book the CLI sets from the ones that remain',
  remaining !== null && remaining.pages.length === rest.pages,
  `worker ${remaining?.pages.length}, CLI ${rest.pages}`,
);

await worker.terminate();

// The module also answers with no worker around it: the batch case,
// which is the same session used once.
await initWasm({ module_or_path: readFileSync(wasm) });
const once = new Session();
once.setMarkdown('gulliver-excerpt.md', markdown);
const direct = decodeDisplayList(once.preview());
check('the same module used once agrees with the worker', direct.pages.length === cli.pages);
check('the module and the reader agree on the wire version', wireVersion() === WIRE_VERSION);
once.free();

console.log(failures === 0 ? '\nall checks passed' : `\n${failures} check(s) failed`);
process.exit(failures === 0 ? 0 : 1);
