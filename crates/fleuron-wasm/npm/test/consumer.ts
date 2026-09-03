/**
 * The install a host actually does.
 *
 * The tarball npm would publish, into an empty directory that has
 * never seen this repository, and the fixture book out of it. The
 * render runs in its own process with nothing on the resolution path
 * but `node_modules`, so anything the package forgot to carry (the
 * module above all) fails here rather than in someone's plugin.
 */

import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const pkg = fileURLToPath(new URL('../', import.meta.url));
const root = fileURLToPath(new URL('../../../../', import.meta.url));
const version: string = JSON.parse(readFileSync(join(pkg, 'package.json'), 'utf8')).version;

let failures = 0;

function check(what: string, passed: boolean, detail = ''): void {
  console.log(`  ${passed ? 'ok  ' : 'FAIL'}  ${what}${detail === '' ? '' : `\n          ${detail}`}`);
  if (!passed) {
    failures += 1;
  }
}

/** What a host writes, and all it is given to write it with. */
const consumer = `import { createRequire } from 'node:module';
import { readFileSync, writeFileSync } from 'node:fs';
import { Session, VERSION, decodeDisplayList, initWasm } from '@fleuron/wasm';

const require = createRequire(import.meta.url);
await initWasm({ module_or_path: readFileSync(require.resolve('@fleuron/wasm/fleuron_bg.wasm')) });

const session = new Session();
for (const url of ['images/plate.jpg', 'images/fleuron.png']) {
  session.addImage(url, new Uint8Array(readFileSync(url)));
}
session.setMarkdown('book.md', readFileSync('book.md', 'utf8'));

const output = decodeDisplayList(session.preview());
const pdf = session.exportPdf();
writeFileSync('book.pdf', pdf);

const items = output.pages.flatMap((page) => page.items);
console.log(JSON.stringify({
  version: VERSION,
  pages: output.pages.length,
  bare: output.pages.filter((page) => page.items.length === 0).length,
  images: items.filter((item) => item.kind === 'image').length,
  characters: items.reduce((total, item) => total + (item.kind === 'text' ? item.text.length : 0), 0),
  bytes: pdf.byteLength,
  head: Buffer.from(pdf.subarray(0, 5)).toString('latin1'),
  tail: Buffer.from(pdf.subarray(-8)).toString('latin1'),
}));
`;

console.log('@fleuron/wasm: the fixture book out of an installed package\n');

const where = mkdtempSync(join(tmpdir(), 'fleuron-consumer-'));
const packed = JSON.parse(
  execFileSync('npm', ['pack', '--json', '--pack-destination', where], {
    cwd: pkg,
    encoding: 'utf8',
  }),
)[0];
const carried: string[] = packed.files.map((file: { path: string }) => file.path);

console.log(`  ${packed.filename}, ${(packed.size / 1024 / 1024).toFixed(2)} MiB over the wire\n`);

const needed = ['wasm/fleuron_bg.wasm', 'wasm/fleuron.js', 'wasm/fleuron.d.ts', 'dist/index.js', 'dist/worker.js'];
const missing = needed.filter((file) => !carried.includes(file));
check('the tarball carries the module and the glue beside it', missing.length === 0, missing.join(', '));

// An empty directory: a package.json, the book, and the package.
const home = join(where, 'host');
mkdirSync(join(home, 'images'), { recursive: true });
copyFileSync(join(root, 'fixtures', 'gulliver-excerpt.md'), join(home, 'book.md'));
for (const image of ['plate.jpg', 'fleuron.png']) {
  copyFileSync(join(root, 'fixtures', 'images', image), join(home, 'images', image));
}
writeFileSync(
  join(home, 'package.json'),
  `${JSON.stringify({ name: 'a-host', private: true, type: 'module' }, null, 2)}\n`,
);
writeFileSync(join(home, 'render.mjs'), consumer);

execFileSync('npm', ['install', '--no-audit', '--no-fund', join(where, packed.filename)], {
  cwd: home,
  stdio: 'inherit',
});

const said = JSON.parse(execFileSync(process.execPath, ['render.mjs'], { cwd: home, encoding: 'utf8' }));

check('the installed package sets the fixture book', said.pages > 1, `${said.pages} pages`);
check('every page of it carries something to paint', said.bare === 0, `${said.bare} empty`);
check('the pictures the host handed over are placed', said.images === 2, `${said.images} images`);
check('the prose is all there', said.characters > 10000, `${said.characters} characters`);
check(
  'and the PDF it wrote is a PDF',
  said.head === '%PDF-' && said.tail.includes('%%EOF'),
  `${said.bytes} bytes`,
);
check('the package names the version it was published at', said.version === version, said.version);

console.log(failures === 0 ? '\nall checks passed' : `\n${failures} check(s) failed`);
process.exit(failures === 0 ? 0 : 1);
