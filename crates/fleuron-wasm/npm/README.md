# fleuron

Paged-media layout in a worker: markdown and CSS in, a display
structure or PDF bytes out.

[fleuron](https://fleuron.typeworks.dev/) is a layout engine for
book-shaped documents, compiled to WebAssembly. It shapes text, breaks
and hyphenates lines, fragments the result into pages, and paints the
preview and the PDF from the same numbers. It touches no DOM and opens
no files.

```sh
npm install fleuron
```

## On screen

```js
import { Preview } from 'fleuron';

const preview = await Preview.mount(document.querySelector('#book'));
await preview.setStyle(css);
await preview.setMarkdown(markdown);

preview.page = 12;
preview.zoom = 1.5;
```

`Preview` starts the worker, loads the module into it, keeps the
session, fetches the fonts the book was set in, and paints a page as
SVG. The encoded buffer, the worker messages and the display structure
are handled internally, and all three stay exported.

`fleuron-react` is the same thing as a component, and holds no engine
logic of its own.

## In a worker

```js
// fleuron.worker.js
import { createEngine } from 'fleuron';

const engine = createEngine();
self.onmessage = ({ data }) => {
  void engine.then((ready) =>
    ready.submit(data, (response, transfer) => self.postMessage(response, transfer)),
  );
};
```

```js
// the host
import { Client, paintPage } from 'fleuron';

const worker = new Worker(new URL('./fleuron.worker.js', import.meta.url), { type: 'module' });
const client = new Client({ post: (request, transfer) => worker.postMessage(request, transfer) });
worker.onmessage = ({ data }) => client.receive(data);

const output = await client.preview([
  { op: 'markdown', name: 'manuscript.md', text: markdown },
  { op: 'style', css },
]);
if (output !== null) {
  element.innerHTML = paintPage(output.pages[0], { fonts: output.fonts });
}
```

`null` means a later render overtook this one, so there is nothing to
paint. Every render raises a generation, the worker echoes it back, and
a reply that arrives behind the current one is dropped.

The package ships the worker in the shape above, so a host that wants
no worker file of its own can point at `fleuron/worker`.

## Sending what changed

The module keeps a session between calls: the content tree, the
styling, and every stage between them and the page. A second render
pays for the edit rather than for the book.

```js
await client.preview([{ op: 'style', css: '@page { margin-bottom: 84pt }' }]);
await client.preview([{ op: 'edit', name: 'ch03.md', text }]);
await client.apply([{ op: 'font', bytes }]);
```

A stylesheet that only moves the page box re-fragments over lines that
are already broken. A keystroke in one chapter reparses that file and
leaves every other section's lines alone. Font bytes cross once and
stay registered. `client.stages` reports how many times each stage has
run, which shows when a cache served.

## Batch

```js
import { decodeDisplayList, initWasm, render, renderPdf } from 'fleuron';

await initWasm();
const output = decodeDisplayList(render(markdown, css));
const pdf = renderPdf(markdown, css);
```

## The display structure

`client.preview` hands back pages of text runs, rules and images, in
points, origin top left. Each text run carries the string it was shaped
from and each glyph a byte range into it, which is what a painter needs
for selection and copy-and-paste.

`paintPage` draws one of them as SVG. Each run becomes one `<text>`
carrying an x for every character in it, so the browser places the
glyphs where the engine put them instead of working out positions of
its own. `exportPdf` writes the same pages as PDF.

The bytes underneath are postcard with a version in front of them.
`decodeDisplayList` reads them, exported for a host that moves them
around itself. Nothing about using the package requires touching them.

## What the host owns

The engine reads no paths, so the host fetches the font bytes and sends
them once. `client.fontBytes(id)` hands back the file a face was
registered from, which is how a painter draws with the bundled one.

Layout never decodes an image. It places one from the size the host
gives it, and the host draws the pixels.

The host starts the worker. A book-scale manuscript is hundreds of
milliseconds of work, and that much time on the main thread drops
interactions.

MIT or Apache-2.0.
