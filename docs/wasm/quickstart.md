---
title: WebAssembly quickstart
description: Installing the package, running layout in a worker, and painting what comes back.
---

The engine compiles to WebAssembly and ships as `@fleuron/wasm`: the module, a worker, a client, and a reader for the display list.

```sh
npm install @fleuron/wasm
```

## In a worker

Layout belongs off the main thread. A book-scale manuscript is hundreds of milliseconds of work, and that much time on the main thread drops interactions.

```js
// fleuron.worker.js
import { createEngine } from '@fleuron/wasm';

const engine = await createEngine();

self.onmessage = ({ data }) => {
  engine.submit(data, (response, transfer) => self.postMessage(response, transfer));
};
```

That is the whole worker, and the package ships it: a host with nothing to add points `new Worker` at `@fleuron/wasm/worker` instead.

```js
// the host
import { Client } from '@fleuron/wasm';

const worker = new Worker(new URL('./fleuron.worker.js', import.meta.url), {
  type: 'module',
});
const client = new Client({
  post: (request, transfer) => worker.postMessage(request, transfer),
});
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

`null` is a render the reader typed past: a later one overtook it, and there is nothing to paint. [The wire](wire.md) has the protocol that decides that.

## Sending what changed

The module holds a [session](../library/sessions.md) between calls, so the second render of a book pays for the edit rather than for the book.

```js
await client.preview([{ op: 'style', css: '@page { margin-bottom: 84pt }' }]);
await client.preview([{ op: 'edit', name: 'ch03.md', text }]);
await client.apply([{ op: 'font', bytes: face }]);
```

A sheet that only moves the page box re-fragments over lines already broken. A keystroke in one chapter reparses that file alone. Font bytes cross once and stay registered, so nothing re-sends a face per keystroke.

`client.stages` reports how many times each stage has run since the session opened. A host reading it can see a cache serve, where a clock would only show a fast machine.

## The ops

| op | what crosses |
|---|---|
| `markdown` | one source as the whole book, its frontmatter the book's metadata |
| `edit` | one source replaced, or appended when the book has not seen the name |
| `content` | a content tree as JSON, for a host with a structured source of its own |
| `style` | the author stylesheet, as CSS text |
| `font` | font bytes, registered for the session's life |
| `dialect` | `commonmark`, `gfm` or `obsidian` |
| `split` | the heading level a section begins at, or `0` for one section per file |

## What comes back

`client.preview` hands back a display list: pages of text runs, rules and images, in points, origin top left. [The display-list reference](../reference/display-list.md) is the structure. The postcard encoding underneath it is the client's business rather than a caller's, and [the wire](wire.md) is where it is written down.

Turning those pages into pixels is a painter's job, and the package ships no preview painter yet. The one painter that exists is the PDF writer, on `exportPdf` below. A screen preview means walking the pages and drawing them for now.

## Exporting

```js
const pdf = await client.exportPdf();
```

The stages above the painter are the ones the preview used, so the PDF cannot contradict what is on screen.

## Batch

Nothing about the module needs a worker. For a build step, or a test:

```js
import { decodeDisplayList, initWasm, render, renderPdf } from '@fleuron/wasm';

await initWasm();
const output = decodeDisplayList(render(markdown, css));
const pdf = renderPdf(markdown, css);
```

`initWasm` fetches the module beside the package's JavaScript. Anywhere that cannot fetch its own files, such as Node or an extension or a bundle that inlines the module, passes the bytes instead: `initWasm({ module_or_path: bytes })`, and `createEngine({ wasm: bytes })` for the worker.

## Building it yourself

```sh
cargo install wasm-pack
wasm-pack build crates/fleuron-wasm --target web --release --no-pack \
  --out-dir npm/wasm --out-name fleuron
cd crates/fleuron-wasm/npm && npm ci && npm run build
```

`npm test` then runs the headless harness: the fixture book through a real worker thread, held against the PDF the CLI writes from the same manuscript.

## What the host owns

**Fonts.** The engine reads no paths. Fetch them, cache them, send the bytes once.

**Images.** Layout never decodes an image. The host probes the header for intrinsic size and draws the pixels on its own side of the wall.

**The thread.** Layout runs in a worker.
