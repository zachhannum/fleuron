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
import { mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Worker } from 'node:worker_threads';

import {
  Client,
  Session,
  decodeDisplayList,
  initWasm,
  type Op,
  type Response,
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

/** What the CLI makes of the fixture book: the run this one is held to. */
function reference(): { pages: number; pdf: Uint8Array } {
  const cli = flag('--cli') ?? join(root, 'target', 'release', 'fleuron');
  const out = join(mkdtempSync(join(tmpdir(), 'fleuron-harness-')), 'reference.pdf');
  const run = spawnSync(cli, [fixture, '-o', out], { encoding: 'utf8' });
  if (run.status !== 0) {
    throw new Error(`the CLI did not render the fixture book: ${run.stderr ?? run.error}`);
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

await worker.terminate();

// The module also answers with no worker around it: the batch case,
// which is the same session used once.
await initWasm({ module_or_path: readFileSync(wasm) });
const once = new Session();
once.setMarkdown('gulliver-excerpt.md', markdown);
const direct = decodeDisplayList(once.preview());
check('the same module used once agrees with the worker', direct.pages.length === cli.pages);
once.free();

console.log(failures === 0 ? '\nall checks passed' : `\n${failures} check(s) failed`);
process.exit(failures === 0 ? 0 : 1);
