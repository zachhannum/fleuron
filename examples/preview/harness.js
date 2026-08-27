/**
 * The preview harness: a manuscript, every page of it on screen, and
 * the PDF of the same run beside it.
 *
 * Written as a consumer of `@fleuron/wasm` writes one. The module,
 * the worker, the postcard buffer and the display list are all in
 * here somewhere, and none of them is named: a `Preview` mounts into
 * an element and takes a manuscript.
 */

import { Preview } from '/crates/fleuron-wasm/npm/dist/index.js';

const status = document.getElementById('status');
const warnings = document.getElementById('warnings');
const folio = document.getElementById('folio');
const beside = document.getElementById('beside');
const exported = document.getElementById('export');

const preview = await Preview.mount(document.getElementById('preview'), {
  zoom: Number(document.getElementById('zoom').value),
  onRender: () => report(),
});

/** What the file inputs replace, and what the page opens on. */
const opening = {
  markdown: await (await fetch('/fixtures/gulliver-excerpt.md')).text(),
  css: await (await fetch('/fixtures/styled.css')).text(),
};

// Nothing in the package fetches a url, so the images are fetched
// here and the bytes handed over. The same file sizes the box and
// fills it.
for (const url of ['images/plate.jpg', 'images/fleuron.png']) {
  const file = await fetch(`/fixtures/${url}`);
  await preview.addImage(url, new Uint8Array(await file.arrayBuffer()));
}

await preview.setStyle(opening.css);
await preview.setMarkdown(opening.markdown, 'gulliver-excerpt.md');

function report() {
  folio.textContent = `${preview.page} of ${preview.pages}`;
  status.textContent = `${preview.pages} pages`;
  warnings.replaceChildren(
    ...preview.warnings.map((warning) => {
      const line = document.createElement('li');
      line.textContent =
        warning.origin === null ? warning.message : `${warning.origin}: ${warning.message}`;
      return line;
    }),
  );
  if (!exported.hidden) {
    void showPdf();
  }
}

function turn(to) {
  preview.page = to;
  report();
}

document.getElementById('previous').addEventListener('click', () => turn(preview.page - 1));
document.getElementById('next').addEventListener('click', () => turn(preview.page + 1));
document.addEventListener('keydown', (event) => {
  if (event.key === 'ArrowLeft') turn(preview.page - 1);
  if (event.key === 'ArrowRight') turn(preview.page + 1);
});

document.getElementById('zoom').addEventListener('input', (event) => {
  preview.zoom = Number(event.target.value);
  report();
});

document.getElementById('manuscript').addEventListener('change', async (event) => {
  const file = event.target.files?.[0];
  if (file !== undefined) {
    status.textContent = 'laying out…';
    await preview.setMarkdown(await file.text(), file.name);
  }
});

document.getElementById('stylesheet').addEventListener('change', async (event) => {
  const file = event.target.files?.[0];
  if (file !== undefined) {
    await preview.setStyle(await file.text());
  }
});

beside.addEventListener('change', async () => {
  exported.hidden = !beside.checked;
  if (beside.checked) {
    await showPdf();
  }
});

/**
 * The same run, exported and handed to the browser's own PDF viewer.
 * Both sides are painted from the stages one layout settled, so a
 * difference between them is the painter's.
 */
let printed = null;
async function showPdf() {
  const bytes = await preview.exportPdf();
  if (bytes === null) {
    return;
  }
  if (printed !== null) {
    URL.revokeObjectURL(printed);
  }
  printed = URL.createObjectURL(new Blob([bytes], { type: 'application/pdf' }));
  const svg = document.querySelector('#preview svg');
  const frame = document.createElement('embed');
  frame.type = 'application/pdf';
  frame.src = `${printed}#page=${preview.page}&view=Fit&toolbar=0`;
  frame.width = svg?.getAttribute('width') ?? '400';
  frame.height = svg?.getAttribute('height') ?? '600';
  exported.replaceChildren(frame);
}

// What a driver pages through. The harness is also the browser end
// of the test suite, and this is the handle it takes hold of.
globalThis.preview = preview;
document.body.dataset['ready'] = 'yes';
