/**
 * The headless acceptance run: the fixture book through the module,
 * in a real worker thread, checked against what the CLI makes of the
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
  styleOp,
  wireVersion,
  type LayoutOutput,
  type Op,
  type Page,
  type RectItem,
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

/**
 * What the CLI makes of some markdown: the run this one is checked
 * against.
 */
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
/** The images the fixture book refers to, resolved the way the CLI does. */
const pictures: [string, Uint8Array][] = ['images/plate.jpg', 'images/fleuron.png'].map((url) => [
  url,
  new Uint8Array(readFileSync(join(root, 'fixtures', url))),
]);
const book: Op[] = [
  ...pictures.map(([url, bytes]): Op => ({ op: 'image', url, bytes })),
  { op: 'markdown', name: 'gulliver-excerpt.md', text: markdown },
];

console.log('fleuron wasm harness: the fixture book through a worker\n');
const cli = reference();
console.log(`  the CLI sets the fixture book in ${cli.pages} pages\n`);

const { client, worker } = open();

// The display structure crosses the wall and reads back as the book the
// CLI laid out.
const preview = await client.preview(book);
if (preview === null) {
  throw new Error('nothing overtook the first render, and it still came back superseded');
}
check('the display structure decodes', preview.pages.length > 0);
check(
  'the worker sets the book in the same pages as the CLI',
  preview.pages.length === cli.pages,
  `worker ${preview.pages.length}, CLI ${cli.pages}`,
);
check(
  'every page has something to paint',
  preview.pages.every((page) => page.items.length > 0),
);
check(
  'a glyph names the text it was shaped from',
  preview.pages.some((page) =>
    page.items.some(
      (item) => item.kind === 'text' && item.text.length > 0 && item.glyphs.length > 0,
    ),
  ),
);
check(
  'the display structure names the face it set',
  preview.fonts.some((font) => font.family === 'eb garamond'),
);
check(
  'the asset table names the images the host handed over',
  preview.assets.map((asset) => asset.url).join(', ') === pictures.map(([url]) => url).join(', '),
  preview.assets.map((asset) => `${asset.url} ${asset.intrinsic.width}px`).join(', '),
);
check(
  'and the pages place them',
  preview.pages.flatMap((page) => page.items).filter((item) => item.kind === 'image').length ===
    pictures.length,
);

// A range: the pages nobody asked for stay off the wire, but the book's
// own length and its tables ride whole regardless of how much of it was
// asked for.
const paged = await client.preview([], { first: 4, count: 1 });
check(
  '`pages` still reports the book, from a reply that carried one page',
  paged !== null && paged.bookPages === preview.pages.length,
  `carried ${paged?.pages.length}, bookPages ${paged?.bookPages}, whole book ${preview.pages.length}`,
);
check('a ranged reply names where its slice begins', paged?.first === 4);
check(
  'a ranged reply carries exactly the page it asked for',
  paged !== null &&
    paged.pages.length === 1 &&
    JSON.stringify(paged.pages[0]) === JSON.stringify(preview.pages[4]),
);
check(
  'the font table and the warnings ride a ranged reply whole',
  paged !== null &&
    JSON.stringify(paged.fonts) === JSON.stringify(preview.fonts) &&
    JSON.stringify(paged.warnings) === JSON.stringify(preview.warnings),
);

const overrun = await client.preview([], { first: preview.pages.length + 5, count: 3 });
check(
  'a range past the end of the book clamps rather than erroring',
  overrun !== null && overrun.pages.length === 0 && overrun.bookPages === preview.pages.length,
);

// Two range fetches asked for together are two different questions
// about the same book, not two renders competing for the one answer a
// render gets: both come back, each with its own page.
const [first, third] = await Promise.all([
  client.preview([], { first: 0, count: 1 }),
  client.preview([], { first: 2, count: 1 }),
]);
check(
  'a range fetch does not supersede a sibling range fetch',
  first !== null && third !== null,
);
check(
  'and each carries the page it actually asked for',
  first !== null &&
    third !== null &&
    JSON.stringify(first.pages[0]) === JSON.stringify(preview.pages[0]) &&
    JSON.stringify(third.pages[0]) === JSON.stringify(preview.pages[2]),
);

// A range fetch is only exempt from supersession among requests that
// leave the generation where it found it. An edit fired while one is
// in flight still raises the generation, so the range fetch answers a
// book that no longer stands and comes back stale, same as any other
// reply behind the current generation. Re-setting the same markdown
// is edit enough to raise the generation without re-transferring the
// image bytes `book` already handed over once.
const racedRangeFetch = client.preview([], { first: 1, count: 1 });
const racedEdit = await client.preview([
  { op: 'markdown', name: 'gulliver-excerpt.md', text: markdown },
]);
check('an edit racing a range fetch still produces its own render', racedEdit !== null);
check(
  'and the range fetch it raced comes back stale rather than painting the old book',
  (await racedRangeFetch) === null,
);

// The painter. Every page is painted, and every glyph the display
// list placed is checked against the x the SVG puts that character
// at — mechanically, over the draw items, with the byte-to-character
// mapping recomputed here rather than borrowed from the painter.

/**
 * The glyph layer's `<text>` elements of a painted page, in paint
 * order. The selection layer's own `<text data-selection-line>` is a
 * second, later `<text>` per line rather than per run, and is not
 * this: {@link selectionLines} reads that one back.
 */
function texts(svg: string): { x: string[]; content: string }[] {
  return [...svg.matchAll(/<text\b([^>]*)>([\s\S]*?)<\/text>/g)]
    .filter((element) => !(element[1] ?? '').includes('data-selection-line'))
    .map((element) => ({
      x: (/ x="([^"]*)"/.exec(element[1] ?? '')?.[1] ?? '').split(' ').filter((n) => n !== ''),
      content: unescape_(element[2] ?? ''),
    }));
}

/** The selection layer's own `<text>` elements, one per line. */
function selectionLines(svg: string): { x: string[]; content: string }[] {
  return [...svg.matchAll(/<text\b([^>]*)>([\s\S]*?)<\/text>/g)]
    .filter((element) => (element[1] ?? '').includes('data-selection-line'))
    .map((element) => ({
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

/** Every glyph of a page, checked against the x the painter wrote. */
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

/**
 * Every selection line checked against the runs it groups: text runs
 * sharing a baseline, grouped independently of the painter here, read
 * back in the manuscript's own casing rather than what was shaped,
 * and joined in paint order.
 */
function misplacedSelection(page: Page, output: LayoutOutput): string | null {
  const lines = selectionLines(paintPage(page, { fonts: output.fonts }));
  const runs = page.items.filter((item): item is TextItem => item.kind === 'text');
  const grouped: TextItem[][] = [];
  for (const run of runs) {
    const last = grouped[grouped.length - 1];
    const first = last?.[0];
    if (first !== undefined && Math.abs(first.y - run.y) < 0.01) {
      last?.push(run);
    } else {
      grouped.push([run]);
    }
  }
  if (lines.length !== grouped.length) {
    return `page ${page.number} groups into ${grouped.length} lines and paints ${lines.length}`;
  }
  for (const [at, group] of grouped.entries()) {
    const expected = group.map((run) => (run.sourceMap.length > 0 ? run.source : run.text)).join('');
    const element = lines[at];
    if (element === undefined || element.content !== expected) {
      return `page ${page.number} line ${at} reads back ${JSON.stringify(element?.content)}, not ${JSON.stringify(expected)}`;
    }
  }
  return null;
}

/**
 * The source ranges as they arrive over the wire: the runs that name
 * one node cover it from its first byte on, without a gap and
 * without overlapping, so a cursor in the manuscript falls in exactly
 * one of them.
 */
function untiledOrigins(pages: Page[]): string | null {
  const covered = new Map<number, [number, number][]>();
  for (const page of pages) {
    for (const item of page.items) {
      if (item.kind !== 'text' || item.origin === null) {
        continue;
      }
      const ranges = covered.get(item.origin.node) ?? [];
      ranges.push(item.origin.range);
      covered.set(item.origin.node, ranges);
    }
  }
  if (covered.size === 0) {
    return 'no run says where it was written';
  }
  for (const [node, ranges] of covered) {
    ranges.sort((a, b) => a[0] - b[0]);
    let at = 0;
    for (const [start, end] of ranges) {
      if (start !== at || end < start) {
        return `node ${node} is covered as ${JSON.stringify(ranges)}`;
      }
      at = end;
    }
  }
  return null;
}

const untiled = untiledOrigins(preview.pages);
check('the runs that name one node tile it', untiled === null, untiled ?? '');
check(
  'the text the engine wrote itself crosses naming no node',
  preview.pages.some((page) =>
    page.items.some((item) => item.kind === 'text' && item.origin === null),
  ),
);

const wrong = preview.pages.map((page) => misplaced(page, preview)).find((bad) => bad !== null);
check('every glyph is painted at the x the display structure gave it', wrong === undefined, wrong ?? '');
const wrongSelection = preview.pages
  .map((page) => misplacedSelection(page, preview))
  .find((bad) => bad !== null);
check(
  "the selection layer's own lines read back every run in the manuscript's own casing",
  wrongSelection === undefined,
  wrongSelection ?? '',
);
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
// files have the same objects under swapped numbers. What the book
// is, its length and its pages and its text, is compared instead, and
// the display structure above, which is the engine's own output, already
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
// Mirrored margins swap sides. The measure is what breaking depends
// on and it does not move, so every line already broken is used
// where the new geometry puts it.
const warm = await client.preview([
  styleOp(
    '@page :left { margin-left: 54pt; margin-right: 42pt }\n' +
      '@page :right { margin-left: 42pt; margin-right: 54pt }\n',
  ),
]);
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
const uncancelled = await client.render([styleOp('book { font-size: 12pt }')], 'preview');
const cancelled = client.render([styleOp('book { font-size: 13pt }')], 'preview');
const after = client.render([styleOp('book { font-size: 12pt }')], 'preview');
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

// Latest wins over an edit that also names a range: this is the shape
// Preview.render sends on every edit, fetching the one page it
// changed rather than the whole book, and a range does not make it a
// question — it still says what that edit produced. The client
// discarding a stale reply is not proof of this on its own, since
// that happens by generation regardless of what the worker did with
// it; what is checked here is the raw protocol message, over the
// worker's own shoulder, for the `superseded` the worker sends only
// when it never ran the older one at all.
const rawReplies: Response[] = [];
const tap = (response: Response): void => {
  rawReplies.push(response);
};
worker.on('message', tap);
const rangedCancelled = client.preview(
  [styleOp('book { font-size: 13pt }')],
  { first: 0, count: 1 },
);
const rangedAfter = client.preview(
  [styleOp('book { font-size: 12pt }')],
  { first: 0, count: 1 },
);
const [rangedDropped, rangedPainted] = await Promise.all([rangedCancelled, rangedAfter]);
worker.off('message', tap);
if (rangedPainted === null) {
  throw new Error('the render nothing overtook came back superseded');
}
check(
  'a cancelled ranged render is discarded on the client',
  rangedDropped === null,
);
check(
  'and the worker itself never ran it: it is superseded on the wire, not merely stale by generation',
  rawReplies.some((response) => 'superseded' in response && response.superseded),
  rawReplies.map((response) => ('superseded' in response ? 'superseded' : 'kind' in response ? response.kind : '?')).join(', '),
);

// Colour: what the sheet names travels with the run, and the painter
// fills with it. The PDF writer fills from the same field.
const coloured = await client.preview([styleOp('h2, h3 { color: #b41e1e }')]);
const headed =
  coloured?.pages.find((page) =>
    page.items.some((item) => item.kind === 'text' && item.color !== '#000000'),
  ) ?? null;
const runs =
  headed?.items.filter((item): item is TextItem => item.kind === 'text') ?? [];
check(
  'a run the sheet coloured carries that colour',
  runs.some((run) => run.color === '#b41e1e') &&
    runs.every((run) => run.color === '#b41e1e' || run.color === '#000000'),
  runs.map((run) => run.color).join(' '),
);
const inColour = headed === null ? '' : paintPage(headed, { fonts: coloured?.fonts ?? [] });
const inRed = runs.filter((run) => run.color === '#b41e1e').length;
check(
  'and the painter fills that run with it, leaving the rest to the page ink',
  inRed > 0 && (inColour.match(/fill="#b41e1e"/g) ?? []).length === inRed,
  `${inRed} coloured runs on the page`,
);

/**
 * The filled rects a painted page draws, in paint order. An image the
 * painter was given no bytes for is drawn as a rect too, and names the
 * asset it is standing in for.
 */
function rects(svg: string): Record<string, number>[] {
  return [...svg.matchAll(/<rect\b([^>]*)\/>/g)]
    .map((match) =>
      Object.fromEntries(
        [...(match[1] ?? '').matchAll(/(\w+)="([-\d.]+)"/g)].map((attribute) => [
          attribute[1] ?? '',
          Number(attribute[2]),
        ]),
      ),
    )
    .filter((rect) => !('asset' in rect));
}

/** Two lengths in points, the same to within a rounding of a float. */
function near(painted: number | undefined, wanted: number): boolean {
  return painted !== undefined && Math.abs(painted - wanted) < 1e-3;
}

// Columns. The page box divides, the flow fills one column before it
// fills the next, and the painter draws the rule the display structure
// carries in the gutter — the same rect the PDF writer fills.
const divided = await client.preview([
  styleOp(
    '@page { column-count: 2; column-gap: 18pt; column-rule-style: solid; column-rule-width: 0.5pt }',
  ),
]);
const columned = divided?.pages.find((page) =>
  page.items.some((item) => item.kind === 'rect'),
) ?? null;
const rule = columned?.items.find((item): item is RectItem => item.kind === 'rect') ?? null;
check(
  'a two-column page carries one rule down its gutter',
  rule !== null && rule.w === 0.5 && columned?.items.filter((item) => item.kind === 'rect').length === 1,
  rule === null ? 'no rect on any page' : `${rule.w}pt wide at ${rule.x}`,
);
const drawn = columned === null ? '' : paintPage(columned, { fonts: divided?.fonts ?? [], paper: null });
const gutterRects = rects(drawn);
check(
  'and the painter draws it where the display structure put it',
  rule !== null &&
    gutterRects.length === 1 &&
    near(gutterRects[0]?.['x'], rule.x) &&
    near(gutterRects[0]?.['y'], rule.y) &&
    near(gutterRects[0]?.['width'], rule.w) &&
    near(gutterRects[0]?.['height'], rule.h),
  rule === null
    ? ''
    : `${JSON.stringify(gutterRects)} against ${rule.x}, ${rule.y}, ${rule.w}, ${rule.h}`,
);
// The columns fill in order: every run left of the gutter is painted
// before every run right of it, and the second column opens above the
// foot of the first.
const columns = (columned?.items ?? []).filter(
  (item): item is TextItem => item.kind === 'text' && item.y < (rule?.y ?? 0) + (rule?.h ?? 0),
);
const gutter = rule === null ? 0 : rule.x;
const turn = columns.findIndex((run) => run.x > gutter);
check(
  'the flow fills the first column before the second',
  turn > 0 && columns.slice(turn).every((run) => run.x > gutter),
  `${turn} runs in the first column of ${columns.length}`,
);
check(
  'and the second column opens above the foot of the first',
  turn > 0 && (columns[turn]?.y ?? 0) <= (columns[turn - 1]?.y ?? 0),
  `${columns[turn]?.y} against ${columns[turn - 1]?.y}`,
);

// Named sheets. A host that builds its styling out of layers sends
// the layers, and a warning names the layer it was written in.
const layers = [
  { name: 'preset.css', css: 'p { font-size: 12pt }\n' },
  { name: 'generated.css', css: 'p { color: #222222 }\np { text-rendering: geometricPrecision }\n' },
  { name: 'overrides.css', css: 'p { font-size: 14pt }\n' },
];
const layered = await client.preview([{ op: 'style', sheets: layers }]);
const inLayers = layered?.warnings.map((warning) => `${warning.origin} ${warning.message}`) ?? [];
check(
  'a warning in the second of three sheets names that sheet, at its own line and column',
  inLayers.length === 1 && (inLayers[0] ?? '').startsWith('generated.css:2:5 '),
  inLayers.join('; '),
);

// The same three, concatenated the way a host without this op has to
// concatenate them: the same complaints, at the positions they hold
// in the file that concatenating made.
const concatenated = await client.preview([styleOp(layers.map((sheet) => sheet.css).join(''))]);
const inOne = concatenated?.warnings.map((warning) => `${warning.origin} ${warning.message}`) ?? [];
check(
  'the same three concatenated warn about the same thing at the concatenated position',
  inOne.length === 1 &&
    inOne[0] === inLayers[0]?.replace('generated.css:2:', 'author.css:3:'),
  inOne.join('; '),
);

// Cascade order is source order, whichever way the sheets arrive.
const sizesOf = (output: LayoutOutput | null): number[] => [
  ...new Set(
    (output?.pages ?? [])
      .flatMap((page) => page.items)
      .filter((item): item is TextItem => item.kind === 'text')
      .map((item) => item.size),
  ),
];
const reversed = await client.preview([
  { op: 'style', sheets: [...layers].reverse() },
]);
check(
  'the last sheet wins, as it does when the three are one file',
  sizesOf(layered).includes(14) &&
    !sizesOf(layered).includes(12) &&
    sizesOf(concatenated).includes(14) &&
    sizesOf(reversed).includes(12) &&
    !sizesOf(reversed).includes(14),
  `layered ${sizesOf(layered).join(' ')}, concatenated ${sizesOf(concatenated).join(' ')},` +
    ` reversed ${sizesOf(reversed).join(' ')}`,
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

// The types reach a host that compiles them and nothing else, so a
// style op with no sheets on it is answered with what one takes.
let malformed = '';
try {
  await client.preview([{ op: 'style', css: 'p { color: red }' } as unknown as Op]);
} catch (error) {
  malformed = String(error);
}
check(
  'a style op written the way the types forbid says what one takes',
  malformed.includes('sheets'),
  malformed,
);
let nameless = '';
try {
  await client.preview([
    { op: 'style', sheets: ['p { color: red }'] } as unknown as Op,
  ]);
} catch (error) {
  nameless = String(error);
}
check(
  'and so does a list of sheets with nothing naming them',
  nameless.includes('sheets'),
  nameless,
);
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
  styleOp(''),
  { op: 'book', sources },
  { op: 'metadata', metadata: naming },
]);
check(
  'a book of several sources sets in the same pages as the CLI sets the same files',
  assembled !== null && assembled.pages.length === whole.pages,
  `worker ${assembled?.pages.length}, CLI ${whole.pages}`,
);

// Which section a page came out of crosses with it: a host that wants
// a contents page or a page range per chapter reads it back here.
const sectionIds = assembled?.pages.flatMap((page) => page.sections) ?? [];
check(
  'a page names the sections whose content is on it',
  new Set(sectionIds).size >= sources.length &&
    sectionIds.every((id, at) => at === 0 || id >= (sectionIds[at - 1] as number)),
  `${new Set(sectionIds).size} sections over ${assembled?.pages.length} pages`,
);
check(
  'a leaf with nobody\'s content on it names no section',
  assembled?.pages.every((page) => page.sections.length > 0 || page.items.length === 0) ?? false,
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
