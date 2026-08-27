# @fleuron/wasm

Paged-media layout in a worker: markdown and CSS in, a display list or
PDF bytes out.

[fleuron](https://zachhannum.github.io/fleuron/) is a layout engine for
book-shaped documents (shaping, line breaking, hyphenation,
fragmentation, page assembly) compiled to WebAssembly. It touches no
DOM, opens no files and reads no clock. The preview and the export are
painted from the same numbers, so they cannot disagree.

```sh
npm install @fleuron/wasm
```

## In a worker

```js
// fleuron.worker.js
import { createEngine } from '@fleuron/wasm';

const engine = await createEngine();
self.onmessage = ({ data }) => {
  engine.submit(data, (response, transfer) => self.postMessage(response, transfer));
};
```

```js
// the host
import { Client } from '@fleuron/wasm';

const worker = new Worker(new URL('./fleuron.worker.js', import.meta.url), { type: 'module' });
const client = new Client({ post: (request, transfer) => worker.postMessage(request, transfer) });
worker.onmessage = ({ data }) => client.receive(data);

const output = await client.preview([
  { op: 'markdown', name: 'manuscript.md', text: markdown },
  { op: 'style', css },
]);
if (output !== null) {
  // your painter: the package has none yet
  draw(output.pages);
}
```

A `null` means the render was overtaken by a later one and there is
nothing to paint. Every render raises a generation, the worker echoes
it back, and a reply that arrives behind the current one is dropped
rather than painted.

The package ships the worker in the shape above, so a host that wants
no worker file of its own can point at `@fleuron/wasm/worker`.

## Inputs cross when they change

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
run, which is how a host sees a cache serve where a clock would only
see a fast machine.

## Batch

```js
import { decodeDisplayList, initWasm, render, renderPdf } from '@fleuron/wasm';

await initWasm();
const output = decodeDisplayList(render(markdown, css));
const pdf = renderPdf(markdown, css);
```

## The display list

`client.preview` hands back pages of text runs, rules and images, in
points, origin top left. Each text run carries the string it was shaped
from and each glyph a byte range into it, which is what a painter needs
for selection and copy-and-paste.

Drawing those pages is a painter's job, and this package ships no
preview painter yet. `exportPdf` returns PDF bytes from the same stages
the preview used, which is the one painter there is.

The bytes underneath are postcard with a version in front of them.
`decodeDisplayList` is what the client reads them with, exported for a
host that moves them around itself; nothing about using the package
requires touching them.

## What the host owns

**Fonts.** The engine reads no paths. Fetch the bytes, send them once.

**Images.** Layout never decodes an image; it places one from the size
the host gives it, and the host draws the pixels.

**The thread.** A book-scale manuscript is hundreds of milliseconds of
work, which is not time the main thread has.

MIT or Apache-2.0.
