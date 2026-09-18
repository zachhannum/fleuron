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
  isRendered,
  WIRE_VERSION,
  decodeDisplayList,
  faceFamily,
  initWasm,
  linkAt,
  PAGE_BACKGROUND,
  PAGE_FURNITURE,
  paintPage,
  styleOp,
  wireVersion,
  type BackgroundItem,
  type Folios,
  type ImageItem,
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
  stderr: string;
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
  return { pages: Number(counted[1]), pdf: readFileSync(out), stderr: run.stderr };
}

/** The faces a PDF embeds, subset tags removed, or null where `pdffonts` is not installed. */
function embedded(pdf: Uint8Array): string[] | null {
  const path = join(mkdtempSync(join(tmpdir(), 'fleuron-fonts-')), 'book.pdf');
  writeFileSync(path, pdf);
  const run = spawnSync('pdffonts', [path], { encoding: 'utf8' });
  if (run.status !== 0) {
    return null;
  }
  return run.stdout
    .split('\n')
    .slice(2)
    .map((line) => (line.split(/\s+/)[0] ?? '').replace(/^[A-Z]{6}\+/, ''))
    .filter((name) => name !== '')
    .sort();
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
const images: [string, Uint8Array][] = ['images/plate.jpg', 'images/fleuron.png'].map((url) => [
  url,
  new Uint8Array(readFileSync(join(root, 'fixtures', url))),
]);
const book: Op[] = [
  ...images.map(([url, bytes]): Op => ({ op: 'image', url, bytes })),
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
  preview.assets.map((asset) => asset.url).join(', ') === images.map(([url]) => url).join(', '),
  preview.assets.map((asset) => `${asset.url} ${asset.intrinsic.width}px`).join(', '),
);
check(
  'and the pages place them',
  preview.pages.flatMap((page) => page.items).filter((item) => item.kind === 'image').length ===
    images.length,
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
 * Where a node is named directly, walked the way a host would have to
 * walk it without a question to ask: the pages a run of that node is
 * set on, and the pages that name it as a section, by their place in
 * the book.
 */
function pagesByWalking(pages: Page[], node: number): number[] {
  return pages.flatMap((page, at) =>
    page.sections.includes(node) ||
    page.items.some((item) => item.kind === 'text' && item.origin?.node === node)
      ? [at]
      : [],
  );
}

/** The same, as the answer the question gives for it. */
function foliosByWalking(pages: Page[], node: number): Folios | null {
  const on = pagesByWalking(pages, node);
  const at = on[0];
  const last = on[on.length - 1];
  if (at === undefined || last === undefined) {
    return null;
  }
  return {
    first: pages[at]?.number ?? 0,
    last: pages[last]?.number ?? 0,
    at,
    count: last - at + 1,
  };
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

// A run under the pointer, taken to the manuscript and back: the
// two questions the wall carries, over the book the worker holds.
const writtenRuns = preview.pages.flatMap((page, index) =>
  page.items.flatMap((item) =>
    item.kind === 'text' && item.origin !== null ? [{ node: item.origin.node, page: index }] : [],
  ),
);
const run = writtenRuns[Math.floor(writtenRuns.length / 2)];
if (run === undefined) {
  throw new Error('no run of the fixture book says where it was written');
}
const written = await client.sourceOf(run.node);
check(
  'the node a run names was read from the manuscript',
  written?.source === 'gulliver-excerpt.md' && written.start < written.end,
  JSON.stringify(written),
);
const start = written?.start ?? 0;
check(
  'the bytes it names are the ones the run was written at',
  markdown.slice(start, written?.end ?? 0).trim().length > 0,
  JSON.stringify(markdown.slice(start, Math.min(written?.end ?? 0, start + 40))),
);
const answered = await client.nodeAt('gulliver-excerpt.md', start);
check('that stretch of the manuscript names the node again', answered === run.node);
check(
  'and the node it names is painted on the page the run was on',
  writtenRuns.some((other) => other.node === answered && other.page === run.page),
);
check(
  'a byte of a file the book has not read is read from nothing',
  (await client.nodeAt('nothing.md', start)) === null,
);
check(
  'a node the book does not have was read from nothing',
  (await client.sourceOf(0)) === null,
);

// Where a node went. The fourth direction: the folios a node's
// content is set on, for several nodes at once, answered without a
// page crossing the wall.
const chapters = [...new Set(preview.pages.flatMap((page) => page.sections))];
const missing = [0, 4294967295];
const asked = [...chapters, run.node, ...missing];
const folioReplies: Response[] = [];
const tapFolios = (response: Response): void => {
  folioReplies.push(response);
};
worker.on('message', tapFolios);
const folios = await client.foliosOf(asked);
worker.off('message', tapFolios);
check('every node asked about is answered, in the order asked', folios.length === asked.length);
check(
  'a node answers with the first and last folio its content is set on',
  asked.every(
    (node, at) =>
      JSON.stringify(folios[at] ?? null) ===
      JSON.stringify(foliosByWalking(preview.pages, node)),
  ),
  JSON.stringify(folios),
);
check(
  'a chapter that runs across pages answers with both of its ends',
  folios.some((answer) => answer !== null && answer !== undefined && answer.first < answer.last),
);
check(
  'a node the book does not hold answers with nothing rather than failing the call',
  missing.every((_, at) => folios[chapters.length + 1 + at] === null),
);

// A page carries about 17 KB of glyphs. The answer is two numbers a
// node, and the point of asking is not to pay for a page to learn
// them.
const onePage = await client.render([], 'preview', { first: 0, count: 1 });
const folioBytes = folioReplies
  .filter(isRendered)
  .reduce((bytes, response) => bytes + response.bytes.byteLength, 0);
check(
  'answering costs no page over the wire',
  onePage !== null && folioBytes > 0 && folioBytes < onePage.byteLength,
  `${folioBytes} bytes for ${asked.length} nodes, against ${onePage?.byteLength ?? 0} for one page`,
);

// A folio is printed on a page and a page is fetched by its place in
// the book, and the two part company wherever the page counter
// restarts. The answer carries both, so the range it names is the
// range that brings back the pages it named the folios of.
const chapter = folios[0];
if (chapter === null || chapter === undefined) {
  throw new Error('the fixture book opens with a chapter that reaches no page');
}
const turned = await client.preview([], { first: chapter.at, count: chapter.count });
check(
  'the range the answer names fetches the pages whose folios it named',
  turned !== null &&
    turned.pages.length === chapter.count &&
    turned.pages[0]?.number === chapter.first &&
    turned.pages[turned.pages.length - 1]?.number === chapter.last,
  `${JSON.stringify(chapter)} fetched ${turned?.pages.length ?? 0} pages, ${turned?.pages[0]?.number ?? 0} to ${turned?.pages[turned.pages.length - 1]?.number ?? 0}`,
);

// A heading's runs are shaped from the text inside it, so no run
// names the heading itself. The node above the run is answered from
// what is under it.
const above = run.node - 1;
const held = await client.sourceOf(above);
const [reached] = await client.foliosOf([above]);
check(
  'a node no run names answers with the page its content is on',
  held !== null &&
    foliosByWalking(preview.pages, above) === null &&
    reached !== null &&
    reached !== undefined &&
    reached.at <= run.page &&
    run.page < reached.at + reached.count,
);

// Two questions asked together are both answered: neither overtakes
// the other, and a render in the same batch overtakes neither.
const [alone, alongside, rendered] = await Promise.all([
  client.foliosOf(chapters),
  client.foliosOf([run.node]),
  client.preview([{ op: 'markdown', name: 'gulliver-excerpt.md', text: markdown }]),
]);
check(
  'a question is neither overtaken by a render nor overtakes one',
  JSON.stringify(alone) === JSON.stringify(folios.slice(0, chapters.length)) &&
    JSON.stringify(alongside) === JSON.stringify([folios[chapters.length]]) &&
    rendered !== null,
);

// What styled the run under the pointer, and where it is: the engine's
// own cascade answers, so the host matches no selector of its own.
const inspected = await client.inspect(run.node);
check(
  'a run answers for the element that holds its text',
  inspected !== null && inspected.node !== null && inspected.node <= run.node,
  JSON.stringify(inspected === null ? null : { node: inspected.node, element: inspected.element }),
);
check(
  'the answer names the rules that matched and where they were written',
  inspected !== null &&
    inspected.rules.length > 0 &&
    inspected.rules.every((rule) => rule.sheet.length > 0 && rule.line > 0 && rule.column > 0),
);
check(
  'the answer gives computed values in points',
  inspected !== null && /pt$/.test(inspected.computed['font-size'] ?? ''),
  inspected?.computed['font-size'] ?? '',
);
const where = inspected?.boxes.find((box) => box.page === run.page);
check('the element has a border box on the page its run is on', where !== undefined);
if (where !== undefined && inspected !== null) {
  const under = await client.hit(where.page, where.x + where.width / 2, where.y + where.height / 2);
  const beneath = under === null ? null : await client.inspect(under);
  check(
    'a point inside that box answers with the element or one inside it',
    under !== null &&
      (under === inspected.node ||
        (beneath?.ancestors.some((ancestor) => ancestor.node === inspected.node) ?? false)),
    `${under}`,
  );
}
check(
  'a point outside every box answers null',
  (await client.hit(run.page, 0.5, 0.5)) === null,
);
check('a node the book does not hold is inspected as null', (await client.inspect(0)) === null);

const numbered = preview.pages.findIndex((page) =>
  page.items.some((item) => item.kind === 'text' && item.layer === PAGE_FURNITURE),
);
const folioBox = numbered === -1 ? null : await client.inspectMarginBox(numbered, 'bottom-center');
check(
  'a page number answers with its page selector and the rules that set it',
  folioBox !== null &&
    folioBox.element === '@bottom-center' &&
    (folioBox.page ?? '').startsWith('@page') &&
    folioBox.rules.some((rule) => rule.declarations.some((d) => d.property === 'content' && d.applied)) &&
    folioBox.boxes.length === 1,
  JSON.stringify(folioBox === null ? null : { page: folioBox.page, boxes: folioBox.boxes }),
);

const [askedInspect, askedHit, renderedBeside] = await Promise.all([
  client.inspect(run.node),
  client.hit(run.page, 0.5, 0.5),
  client.preview([{ op: 'markdown', name: 'gulliver-excerpt.md', text: markdown }]),
]);
check(
  'inspecting and hit testing are questions a render does not overtake',
  JSON.stringify(askedInspect) === JSON.stringify(inspected) &&
    askedHit === null &&
    renderedBeside !== null,
);

// A host that lists a pseudo-element beside its element moves from one
// to the other by id, without knowing how the id of a pseudo-element
// is packed.
const capped = await client.preview([styleOp('p::first-letter { initial-letter: 3 }')]);
const capRun = capped?.pages
  .flatMap((page) => page.items)
  .find((item) => item.kind === 'text' && item.pseudoElement !== null);
const cap =
  capRun?.kind === 'text' && capRun.pseudoElement !== null
    ? await client.inspect(capRun.pseudoElement)
    : null;
const owns = cap === null || cap.elementNode === null ? null : await client.inspect(cap.elementNode);
check(
  'a pseudo-element answers with the element it belongs to, which answers for itself',
  cap !== null &&
    cap.pseudoElement === '::first-letter' &&
    cap.elementNode !== null &&
    cap.elementNode !== cap.node &&
    owns !== null &&
    owns.node === cap.elementNode &&
    owns.element === cap.element &&
    owns.elementNode === owns.node,
  JSON.stringify(cap === null ? null : { node: cap.node, elementNode: cap.elementNode }),
);
check('a margin box answers with no element', (folioBox?.elementNode ?? null) === null);
await client.preview([styleOp('')]);

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

// Links. Each page carries the links set on it, resolved, and a host
// follows one from the link alone: the place it goes names the page by
// its place in the book, which is the range that fetches it.
const crossReference = preview.pages
  .flatMap((page, at) => page.links.map((link, index) => ({ page, at, link, index })))
  .find(({ link }) => link.to.kind === 'place');
check('the cross-reference in the fixture book is a link on its page', crossReference !== undefined);
const crossTo = crossReference?.link.to;
if (crossReference !== undefined && crossTo?.kind === 'place') {
  const { page, at, link, index } = crossReference;
  const { node, place } = crossTo;
  const area = link.areas[0];
  check(
    'linkAt finds the link at a point inside its area, and nothing just outside it',
    area !== undefined &&
      linkAt(page, area.x + area.width / 2, area.y + area.height / 2) === link &&
      linkAt(page, area.x + area.width / 2, area.y - 1) === null,
    JSON.stringify(link.areas),
  );
  const marked = paintPage(page, { fonts: preview.fonts });
  const marks = [...marked.matchAll(/<rect [^>]*data-link="(\d+)"/g)].map((mark) => Number(mark[1]));
  check(
    'the painter marks each line of each link over the selection layer, by its index',
    marks.length === page.links.flatMap((each) => each.areas).length &&
      marks.includes(index) &&
      marked.indexOf('data-link-layer') > marked.indexOf('data-selection-layer') &&
      !marked.includes('<a '),
    `${marks.length} marks`,
  );
  const unmarked = paintPage(page, { fonts: preview.fonts, links: false });
  check(
    'a host that marks links itself turns the marks off, and the rest of the page is the same',
    !unmarked.includes('data-link') &&
      marked.replace(/<g data-link-layer="true">.*?<\/g>/, '') === unmarked,
  );
  const span = await client.preview([], { first: at, count: 1 });
  const held = span?.pages[0]?.links[index];
  const outside = place.page !== at;
  const landing = held?.to.kind === 'place' ? await client.preview([], { first: held.to.place.page, count: 1 }) : null;
  const [folio] = await client.foliosOf([node]);
  check(
    'a host that holds a span without the target follows the link to it from the link alone',
    outside &&
      JSON.stringify(held) === JSON.stringify(link) &&
      landing?.first === place.page &&
      folio?.at === place.page,
    `link on page ${at} to ${JSON.stringify(place)}, the target opens at ${folio?.at}`,
  );
}
check(
  'a page with no link paints no mark',
  preview.pages
    .filter((page) => page.links.length === 0)
    .every((page) => !paintPage(page, { fonts: preview.fonts }).includes('data-link')),
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

// The layer crosses the wire as well as the geometry does: a sheet
// that raises the prose comes back with the runs in the layer it
// named, and the runs of a sheet that names none come back in layer
// 0.
/** The layers of a page that came from the blocks of the book. */
const blockLayers = (page: Page): number[] =>
  page.items
    .map((item) => item.layer)
    .filter((layer) => layer !== PAGE_BACKGROUND && layer !== PAGE_FURNITURE);
const raised = await client.preview([styleOp('p { z-index: 10 }')]);
check(
  'a layer the sheet named crosses the wire',
  raised !== null &&
    raised.pages.some((page) => blockLayers(page).includes(10)) &&
    raised.pages.every((page) => blockLayers(page).every((layer) => layer === 0 || layer === 10)),
);
const grounded = await client.preview([styleOp('')]);
check(
  'and a book that names no layer comes back in layer 0',
  grounded !== null && grounded.pages.every((page) => blockLayers(page).every((layer) => layer === 0)),
);

// The preview painter and the PDF export walk the same sorted list,
// so a tint a sheet raises covers the prose in both.
const tinted = 'section { background-color: #eeeeee }';
const order = async (css: string): Promise<[number, number] | null> => {
  const output = await client.preview([styleOp(css)]);
  const page = output?.pages[0];
  if (output === null || page === undefined) {
    return null;
  }
  const svg = paintPage(page, { fonts: output.fonts });
  return [svg.indexOf('#eeeeee'), svg.indexOf('<text')];
};
const flowed = await order(tinted);
const over = await order(`${tinted} section { z-index: 10 }`);
check(
  'the preview paints a raised tint after the prose it covers',
  flowed !== null && over !== null && flowed[0] < flowed[1] && over[0] > over[1],
  `flowed ${flowed?.join(' ')}, raised ${over?.join(' ')}`,
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
    .filter((rect) => !('asset' in rect) && !('link' in rect));
}

/** Two lengths in points, the same to within a rounding of a float. */
function near(painted: number | undefined, wanted: number): boolean {
  return painted !== undefined && Math.abs(painted - wanted) < 1e-3;
}

// Columns. The page box divides, the flow fills one column before it
// fills the next, and the painter draws the rule the display structure
// carries in the gutter, which is the rect the PDF writer fills.
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

// Spanning. The chapters run together, so a chapter heading falls
// partway down a page, and it is set across both columns with a tier
// of columns above it and a tier below. The painter draws every glyph
// and every rule of that page where the display structure put them,
// which is where the PDF writer puts them too.
const spanned = await client.preview([
  styleOp(
    '@page { column-count: 2; column-gap: 18pt; column-rule-style: solid; column-rule-width: 0.5pt } ' +
      'section { break-before: auto } h2, h3, table { column-span: all }',
  ),
]);
const across =
  spanned?.pages.find((page) => {
    const heading = page.items.find(
      (item): item is TextItem => item.kind === 'text' && item.text.startsWith('CHAPTER II'),
    );
    return (
      heading !== undefined && page.items.some((item) => item.kind === 'text' && item.y < heading.y)
    );
  }) ?? null;
check(
  'a chapter heading spans the columns partway down a page',
  across !== null,
  across === null ? 'no page sets the second chapter under a tier of columns' : '',
);
const acrossRects = (across?.items ?? []).filter((item): item is RectItem => item.kind === 'rect');
const acrossDrawn =
  across === null ? '' : paintPage(across, { fonts: spanned?.fonts ?? [], paper: null });
const acrossPainted = rects(acrossDrawn);
check(
  'and the painter draws its rules where the display structure put them',
  acrossRects.length > 0 &&
    acrossPainted.length === acrossRects.length &&
    acrossRects.every(
      (rule, at) =>
        near(acrossPainted[at]?.['x'], rule.x) &&
        near(acrossPainted[at]?.['y'], rule.y) &&
        near(acrossPainted[at]?.['width'], rule.w) &&
        near(acrossPainted[at]?.['height'], rule.h),
    ),
  `${JSON.stringify(acrossPainted)} against ${JSON.stringify(acrossRects)}`,
);
const acrossMisplaced =
  across === null || spanned === null ? 'no page to paint' : misplaced(across, spanned);
check(
  'and every glyph on it where the display structure put it',
  acrossMisplaced === null,
  acrossMisplaced ?? '',
);

// An anchored image. The sheet takes the map out of the text and
// against the page. The prose of that page sets beside it, and the
// painter draws both where the display structure put them. The PDF
// writer places the same items, so this is the preview half of that
// agreement.
const illustrated = await client.preview([
  styleOp('.map { position: absolute; top: 0; left: 0; margin-right: 12pt; wrap-flow: end }'),
]);
const wrapped =
  illustrated?.pages.find((page) =>
    page.items.some(
      (item) =>
        item.kind === 'image' &&
        page.items.some((run) => run.kind === 'text' && run.x >= item.x + item.w),
    ),
  ) ?? null;
const image = wrapped?.items.find((item): item is ImageItem => item.kind === 'image') ?? null;
const beside = (wrapped?.items ?? []).filter(
  (item): item is TextItem =>
    item.kind === 'text' && image !== null && item.x >= image.x + image.w,
);
check(
  'a page the sheet anchored an image to sets its prose beside it',
  image !== null && beside.length > 0,
  image === null ? 'no image on any page' : `${beside.length} runs beside it`,
);
const illustratedSvg =
  wrapped === null
    ? ''
    : paintPage(wrapped, {
        fonts: illustrated?.fonts ?? [],
        assets: illustrated?.assets ?? [],
        asset: () => 'data:image/gif;base64,R0lGODlhAQABAAAAACw=',
      });
const drawnImages: Record<string, number>[] = [...illustratedSvg.matchAll(/<image\b([^>]*)\/>/g)].map(
  (match) =>
    Object.fromEntries(
      [...(match[1] ?? '').matchAll(/(\w+)="([-\d.]+)"/g)].map((attribute) => [
        attribute[1] ?? '',
        Number(attribute[2]),
      ]),
    ),
);
check(
  'and the painter draws the image where the display structure put it',
  image !== null &&
    drawnImages.length === 1 &&
    near(drawnImages[0]?.['x'], image.x) &&
    near(drawnImages[0]?.['y'], image.y) &&
    near(drawnImages[0]?.['width'], image.w) &&
    near(drawnImages[0]?.['height'], image.h),
  image === null
    ? ''
    : `${JSON.stringify(drawnImages)} against ${image.x}, ${image.y}, ${image.w}, ${image.h}`,
);
const clear = texts(illustratedSvg).filter(
  (run) => image !== null && Number(run.x[0]) >= image.x + image.w,
);
check(
  'and it starts the lines beside it where their runs start',
  clear.length === beside.length &&
    clear.every((run, index) => near(Number(run.x[0]), beside[index]?.x ?? -1)),
  `${clear.length} lines painted beside the image, ${beside.length} on the page`,
);

// Positioned blocks. The sheet raises every chapter title and lifts
// the part title against the page. On the page that carries both, the
// painter must draw every glyph where the display structure puts it.
// A test in `pdf.rs` makes sure that the export does the same.
const positioned = await client.preview([
  styleOp(
    'h3 { position: relative; top: -12pt } ' +
      'h2 { position: absolute; bottom: 1in; left: 0.5in; right: 0.5in }',
  ),
]);
const positionedPage =
  positioned?.pages.find(
    (page) =>
      page.items.some((item) => item.kind === 'text' && item.text.includes('PART')) &&
      page.items.some((item) => item.kind === 'text' && item.text.includes('CHAPTER')),
  ) ?? null;
const positionedMisplaced =
  positioned === null || positionedPage === null
    ? 'no page carries the part title and a chapter title'
    : misplaced(positionedPage, positioned);
check(
  'a page with a relative block and an absolute block paints every glyph where the display structure put it',
  positionedMisplaced === null,
  positionedMisplaced ?? '',
);

// A contour. The same sheet wraps the prose to the shape the
// ornament's own alpha channel traces rather than to its box. The
// lines beside it start inside the box, which nothing but a traced
// contour allows, and the painter starts them where the display
// structure did. The PDF the same run exports is the CLI's byte for
// byte, so the preview and the export agree about this page as they
// agree about every other.
const contoured = await client.preview([
  styleOp(
    'img:not(.map) { position: absolute; top: 0; left: 0; wrap-flow: end; ' +
      'shape-outside: auto; shape-margin: 3pt }',
  ),
]);
// The ornament is the narrow image; the map is the wide one.
const ornamented =
  contoured?.pages.find((page) =>
    page.items.some((item) => item.kind === 'image' && item.w < 60),
  ) ?? null;
const ornament =
  ornamented?.items.find((item): item is ImageItem => item.kind === 'image' && item.w < 60) ?? null;
const contourLines = (ornamented?.items ?? []).filter(
  (item): item is TextItem =>
    item.kind === 'text' &&
    ornament !== null &&
    item.y > ornament.y &&
    item.y <= ornament.y + ornament.h,
);
check(
  'a page wrapped to a traced contour sets its prose inside the image box',
  ornament !== null &&
    contourLines.length > 0 &&
    contourLines.every((run) => run.x > ornament.x && run.x < ornament.x + ornament.w),
  ornament === null
    ? 'no ornament on any page'
    : `${contourLines.map((run) => run.x).join(', ')} inside ${ornament.x}..${
        ornament.x + ornament.w
      }`,
);
const contouredSvg =
  ornamented === null
    ? ''
    : paintPage(ornamented, {
        fonts: contoured?.fonts ?? [],
        assets: contoured?.assets ?? [],
        asset: () => 'data:image/gif;base64,R0lGODlhAQABAAAAACw=',
      });
const contourStarts = texts(contouredSvg).map((run) => Number(run.x[0]));
check(
  'and the painter starts them where the display structure did',
  contourLines.length > 0 &&
    contourLines.every((item) => contourStarts.some((x) => near(x, item.x))),
  `${contourStarts.length} lines painted, ${contourLines.length} beside the ornament`,
);

// Art behind the page. The sheet names a url the host has already
// handed over, so it reaches the asset table off the cascade rather
// than off the manuscript, and the page paints it over the whole page
// box before anything else on the page.
const scanned = await client.preview([
  styleOp('@page { background-image: url("images/plate.jpg"); background-size: cover; ' +
    'background-repeat: no-repeat }'),
]);
const scannedPage = scanned?.pages[0] ?? null;
const scan =
  scannedPage?.items.find((item): item is BackgroundItem => item.kind === 'background') ?? null;
check(
  'a page paints the scan the sheet named over the whole page box',
  scan !== null &&
    scan.x === 0 &&
    scan.y === 0 &&
    near(scan.w, scannedPage?.width ?? -1) &&
    near(scan.h, scannedPage?.height ?? -1),
  scan === null ? 'no background on page one' : JSON.stringify(scan),
);
check(
  'and nothing on the page is painted under it',
  scannedPage?.items[0]?.kind === 'background',
  scannedPage?.items[0]?.kind ?? 'nothing',
);
check(
  'and `cover` fills it, leaving the crop to the box',
  scan !== null && scan.tileW >= scan.w - 1e-3 && scan.tileH >= scan.h - 1e-3,
  scan === null ? '' : `${scan.tileW}x${scan.tileH} over ${scan.w}x${scan.h}`,
);
const scannedSvg =
  scannedPage === null
    ? ''
    : paintPage(scannedPage, {
        fonts: scanned?.fonts ?? [],
        assets: scanned?.assets ?? [],
        asset: () => 'data:image/gif;base64,R0lGODlhAQABAAAAACw=',
      });
const clipped = [...scannedSvg.matchAll(/<clipPath\b[^>]*><rect\b([^>]*)\/>/g)].map((match) =>
  Object.fromEntries(
    [...(match[1] ?? '').matchAll(/(\w+)="([-\d.]+)"/g)].map((attribute) => [
      attribute[1] ?? '',
      Number(attribute[2]),
    ]),
  ),
);
check(
  'and the painter clips it to that box',
  scan !== null &&
    clipped.length === 1 &&
    near(clipped[0]?.['x'], scan.x) &&
    near(clipped[0]?.['y'], scan.y) &&
    near(clipped[0]?.['width'], scan.w) &&
    near(clipped[0]?.['height'], scan.h),
  JSON.stringify(clipped),
);

// Alpha, opacity and rounded corners. A tint with an alpha over the
// scan behind a page leaves the scan showing, so the painter writes the
// alpha as an opacity rather than as an opaque fill. The tint has
// rounded corners, so it is a path, and a heading at `opacity: 0.05`
// fills its runs at that opacity. The first tint on a page opens its
// quotation, so its top corners are rounded whether or not a page turn
// squares the bottom ones.
const translucent = await client.preview([
  styleOp(
    '@page { background-image: url("images/plate.jpg"); background-size: cover } ' +
      'blockquote { background-color: rgba(0, 0, 0, 0.25); border-radius: 3pt } ' +
      'h3 { opacity: 0.05 }',
  ),
]);
const tintedPage =
  translucent?.pages.find((page) =>
    page.items.some((item) => item.kind === 'rounded' && item.color === '#00000040'),
  ) ?? null;
const tintItem = tintedPage?.items.find(
  (item) => item.kind === 'rounded' && item.color === '#00000040',
);
const tint = tintItem?.kind === 'rounded' ? tintItem : null;
check(
  'a tint with an alpha behind a quotation carries its alpha and its rounded corners',
  tint !== null && tint.radii.topLeft.x === 3 && tint.radii.topRight.y === 3 && tint.ring.top === 0,
  tint === null ? 'no rounded tint on any page' : JSON.stringify(tint),
);
check(
  'and the display structure puts it over the scan behind the page',
  tintedPage !== null &&
    tintItem !== undefined &&
    tintedPage.items.findIndex((item) => item.kind === 'background') !== -1 &&
    tintedPage.items.findIndex((item) => item.kind === 'background') < tintedPage.items.indexOf(tintItem),
  tintedPage?.items.map((item) => item.kind).join(' ') ?? '',
);
const tintedSvg =
  tintedPage === null
    ? ''
    : paintPage(tintedPage, {
        fonts: translucent?.fonts ?? [],
        assets: translucent?.assets ?? [],
        asset: () => 'data:image/gif;base64,R0lGODlhAQABAAAAACw=',
      });
const scanAt = tintedSvg.indexOf('<image');
const tintAt = tintedSvg.search(
  /<path d="M[^"]*A3 3 0 0 1 [^"]*Z" fill-rule="evenodd" fill="#000000" fill-opacity="0\.25\d*"\/>/,
);
check(
  'and the painter draws the scan, then the tint over it as a rounded path at a quarter opacity',
  scanAt !== -1 && tintAt > scanAt,
  `scan at ${scanAt}, tint at ${tintAt}`,
);
// An inline box. A chip on an emphasis is a rounded box per line the
// emphasis reaches, painted before the run it tints, and the painter
// draws its corners as arcs.
const chipped = await client.preview([
  styleOp(
    'em { background-color: #858585; color: #ffffff; padding: 2pt 4pt; border-radius: 3pt }',
  ),
]);
const chipPage =
  chipped?.pages.find((page) =>
    page.items.some((item) => item.kind === 'rounded' && item.color === '#858585'),
  ) ?? null;
const chips = (chipPage?.items ?? []).filter(
  (item) => item.kind === 'rounded' && item.color === '#858585',
);
const firstChip = chips[0];
const lastChip = chips[chips.length - 1];
check(
  'an emphasis with a chip paints one rounded box per line it reaches',
  chips.length > 1 &&
    firstChip?.kind === 'rounded' &&
    lastChip?.kind === 'rounded' &&
    firstChip.radii.topLeft.x === 3 &&
    firstChip.radii.topRight.x === 0 &&
    lastChip.radii.topRight.x === 3 &&
    lastChip.radii.topLeft.x === 0,
  `${chips.length} chips: ${JSON.stringify(firstChip)} ${JSON.stringify(lastChip)}`,
);
check(
  'and the display structure puts each chip before the run it tints',
  chipPage !== null &&
    firstChip !== undefined &&
    chipPage.items.indexOf(firstChip) <
      chipPage.items.findIndex((item) => item.kind === 'text' && item.color === '#ffffff'),
  chipPage?.items.map((item) => item.kind).join(' ') ?? '',
);
const chipSvg =
  chipPage === null
    ? ''
    : paintPage(chipPage, {
        fonts: chipped?.fonts ?? [],
        assets: chipped?.assets ?? [],
        asset: () => 'data:image/gif;base64,R0lGODlhAQABAAAAACw=',
      });
check(
  'and the painter draws its corners as arcs',
  /<path d="M[^"]*A3 3 0 0 1 [^"]*Z" fill-rule="evenodd" fill="#858585"\/>/.test(chipSvg),
  chipSvg.slice(0, 400),
);

const fadedRuns =
  translucent?.pages
    .flatMap((page) => page.items)
    .filter((item): item is TextItem => item.kind === 'text' && item.color === '#0000000d') ?? [];
check(
  'a heading at opacity 0.05 fills its runs at that opacity',
  fadedRuns.length > 0,
  `${fadedRuns.length} faded runs`,
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

// A face through `@font-face`. The rule names a url and the family,
// weight and style to register under. The sheet crosses first and the
// bytes cross after it, under that url. The CLI reads the same file
// from beside the manuscript, and the two runs agree about the book.
const fellUrl = 'fonts/IMFellEnglishSC-Regular.ttf';
const fellCss =
  `@font-face { font-family: "Fell Caps"; src: url("${fellUrl}"); font-weight: 400; font-style: normal }\n` +
  'h3 { font-family: "Fell Caps", serif }\n';
const fellFolder = mkdtempSync(join(tmpdir(), 'fleuron-face-'));
const fellSheet = join(fellFolder, 'faces.css');
writeFileSync(fellSheet, fellCss);
// A copy of the manuscript away from the fixtures has no font file beside it.
const strandedPath = join(fellFolder, 'gulliver-excerpt.md');
writeFileSync(strandedPath, markdown);
const stranded = reference([strandedPath], ['-c', fellSheet]);
const withFace = reference([fixture], ['-c', fellSheet]);

const unloaded = await client.preview([styleOp(fellCss)]);
const unloadedWarning = unloaded?.warnings.find((warning) => warning.message.includes('Fell Caps'));
check(
  'a `@font-face` whose url has no bytes warns the way the CLI warns',
  unloadedWarning !== undefined &&
    stranded.stderr.includes(`fleuron: warning: ${unloadedWarning.message}`),
  unloadedWarning?.message ?? 'no warning names the family',
);

const fellBytes = new Uint8Array(readFileSync(join(root, 'fixtures', fellUrl)));
const loaded = await client.preview([{ op: 'font', url: fellUrl, bytes: fellBytes }]);
const fellIds = (loaded?.fonts ?? []).flatMap((font, id) => (font.family === 'fell caps' ? [id] : []));
const fellId = fellIds[0] ?? -1;
check(
  'bytes sent under that url after the sheet register the face the rule declares',
  fellIds.length === 1 &&
    loaded?.fonts[fellId]?.attributes.weight === 400 &&
    loaded?.fonts[fellId]?.attributes.italic === false,
  JSON.stringify(loaded?.fonts.map((font) => font.family)),
);
const heads = (loaded?.pages ?? [])
  .flatMap((page) => page.items)
  .filter((item): item is TextItem => item.kind === 'text' && item.text.includes('CHAPTER'));
check(
  'and the chapter heads set in that face',
  heads.length > 0 && heads.every((item) => item.fontId === fellId),
  heads.map((item) => item.fontId).join(' '),
);
check(
  'and no warning names the family',
  loaded !== null && !loaded.warnings.some((warning) => warning.message.includes('Fell Caps')),
);
check(
  'the book with the face sets in the same pages as the CLI sets it',
  loaded?.pages.length === withFace.pages,
  `worker ${loaded?.pages.length}, CLI ${withFace.pages}`,
);
const fellPdf = await client.exportPdf();
const fellFaces = fellPdf === null ? null : embedded(fellPdf);
const cliFaces = embedded(withFace.pdf);
if (fellFaces === null || cliFaces === null) {
  console.log('  skip  the PDF embeds the face the CLI embeds (pdffonts not installed)');
  if (process.env['FLEURON_WASM_REQUIRE_TOOLS'] === '1') {
    failures += 1;
  }
} else {
  check(
    'the PDF embeds the face the CLI embeds',
    fellFaces.some((name) => /fell/i.test(name)) &&
      JSON.stringify(fellFaces) === JSON.stringify(cliFaces),
    `worker ${fellFaces.join(' ')}, CLI ${cliFaces.join(' ')}`,
  );
}

const fellLines = client.stages.lines;
const again = await client.preview([styleOp(fellCss)]);
check(
  'the same sheet again registers the face no second time and breaks no line again',
  again !== null &&
    again.fonts.length === (loaded?.fonts.length ?? -1) &&
    client.stages.lines === fellLines,
  `${again?.fonts.length} faces, lines broken ${fellLines} before, ${client.stages.lines} after`,
);
const unruled = await client.preview([styleOp('h3 { font-family: "Fell Caps", serif }\n')]);
check(
  'a sheet without the rule leaves no face under the family',
  unruled !== null &&
    !unruled.fonts.some((font) => font.family === 'fell caps') &&
    unruled.pages
      .flatMap((page) => page.items)
      .every((item) => item.kind !== 'text' || item.fontId < unruled.fonts.length),
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

// A host names a source's sections beside the text, and a sheet
// reaches them by that name. A small page on the front source is what
// shows the rule matched.
const small = 4 * 72;
const widths = (output: LayoutOutput | null): string =>
  output === null ? 'nothing' : `${output.pages[0]?.width} then ${output.pages.at(-1)?.width}`;
const fronted = await client.preview([
  styleOp('@page front { size: 4in 6in } section.front { page: front }'),
  {
    op: 'book',
    sources: sources.map((source, at) =>
      at === 0 ? { ...source, attributes: { classes: ['front'] } } : source,
    ),
  },
]);
check(
  'a class a host sets on a source reaches its sections in the cascade',
  fronted !== null &&
    fronted.pages[0]?.width === small &&
    fronted.pages.at(-1)?.width !== small,
  widths(fronted),
);
const edited = await client.preview([
  { op: 'edit', name: 'part-1.md', text: `${parts[0]}\n` },
]);
check(
  'and the source keeps the class when its text is edited',
  edited !== null && edited.pages[0]?.width === small,
  widths(edited),
);
const stripped = await client.preview([{ op: 'attributes', name: 'part-1.md', attributes: {} }]);
check(
  'and loses it when the host takes the class away',
  stripped !== null && stripped.pages[0]?.width !== small,
  widths(stripped),
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
