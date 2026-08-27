---
title: WebAssembly quickstart
description: Installing the package, running layout in a worker, and painting what comes back.
---

The engine compiles to WebAssembly and ships as `@fleuron/wasm`. The package holds the module, a worker, a client, a display-list reader and an SVG painter.

```sh
npm install @fleuron/wasm
```

## Preview does everything

`Preview` starts the worker, loads the module into it, keeps the session, fetches the fonts and paints pages into an element you give it.

```js
import { Preview } from '@fleuron/wasm';

const preview = await Preview.mount(document.querySelector('#book'));
await preview.setStyle(css);
await preview.setMarkdown(markdown);

preview.page = 12;
```

Layout is already running off the main thread, and page 12 is on the screen. Most hosts want this. [The preview](preview.md) has the rest of its surface.

## Assembling it yourself

The same thing is available in pieces: a worker, a client that talks to it, a reader for what comes back, and a painter. Assemble them yourself when you want to paint the pages some other way, run the engine somewhere a `Worker` is not what starts it, or take the pages without putting any of them on a screen.

### The worker

Layout belongs off the main thread. A book-scale manuscript is hundreds of milliseconds of work, and that much time on the main thread drops interactions.

You do not have to write the worker file. The package ships one at the `@fleuron/wasm/worker` export, which is what `Preview` starts, and a host that needs nothing else in there can point `new Worker` at that path and skip to the client below.

Writing your own is six lines, and worth doing when the worker has to hold something else, or when your bundler needs to hand the module its bytes rather than let it fetch them:

```js
// fleuron.worker.js
import { createEngine } from '@fleuron/wasm';

const engine = createEngine();

self.onmessage = ({ data }) => {
  void engine.then((ready) =>
    ready.submit(data, (response, transfer) => self.postMessage(response, transfer)),
  );
};
```

Install the handler before the module has finished loading, rather than after an `await`. A worker that awaits the load first drops whatever the host posted while the module was still coming down, and a host that opens a preview and immediately hands it a manuscript posts exactly then.

### The client

The host side keeps a `Client`, which pairs replies with calls and decides which renders are still worth painting:

```js
// the host
import { Client, paintPage } from '@fleuron/wasm';

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
  element.innerHTML = paintPage(output.pages[0], { fonts: output.fonts });
}
```

`null` means a later render overtook this one before it finished, so there is nothing to paint and the newer render is on its way. [The wire](wire.md) has the protocol that decides that.

Painting a run in the right font takes one more step, since the browser needs the font files as `FontFace`s. See [the preview](preview.md#fonts).

### Sending what changed

The module holds a [session](../library/sessions.md) between calls, so the second render of a book pays for the edit rather than for the book.

```js
await client.preview([{ op: 'style', css: '@page { margin-bottom: 84pt }' }]);
await client.preview([{ op: 'edit', name: 'ch03.md', text }]);
await client.apply([{ op: 'font', bytes: face }]);
```

A sheet that only moves the page box re-fragments over lines already broken. A keystroke in one chapter reparses that file alone. Font bytes cross once and stay registered, so nothing re-sends a face per keystroke.

`client.stages` reports how many times each stage has run since the session opened. It shows a host when a cache served, which a clock cannot distinguish from a fast machine.

### The ops

| op | what crosses |
|---|---|
| `markdown` | one source as the whole book, its frontmatter the book's metadata |
| `book` | every source of a book, in reading order |
| `edit` | one source replaced, or appended when the book has not seen the name |
| `remove` | one source dropped, the rest of the book left standing |
| `metadata` | title, author and a frontend's own fields |
| `content` | a content tree as JSON, for a host with a structured source of its own |
| `style` | the author stylesheet, as CSS text |
| `font` | font bytes, registered for the session's life |
| `dialect` | `commonmark`, `gfm` or `obsidian` |
| `split` | the heading level a section begins at, or `0` for one section per file |

A book of one source takes its title and author from that source's frontmatter. A book of several has no frontmatter of its own, so `metadata` is how it gets a name. Only the PDF writer reads it, so sending it costs no layout.

### What comes back

`client.preview` hands back a display list: pages of text runs, rules and images, in points, origin top left. See [the display-list reference](../reference/display-list.mdx) for the structure, and [the wire](wire.md) for the postcard encoding underneath it.

Turning those pages into pixels is a painter's job. `paintPage` is the one the package ships. See [the preview](preview.md).

### Exporting

```js
const pdf = await client.exportPdf();
```

The export draws from the same laid-out pages the preview drew, rather than laying the book out a second time, so it cannot come out different from what is on screen.

## Without a worker

Nothing about the module needs a worker. For a build step, or a test:

```js
import { decodeDisplayList, initWasm, render, renderPdf } from '@fleuron/wasm';

await initWasm();
const output = decodeDisplayList(render(markdown, css));
const pdf = renderPdf(markdown, css);
```

`initWasm` fetches the module beside the package's JavaScript. Anywhere that cannot fetch its own files, such as Node, an extension, or a bundle that inlines the module, passes the bytes instead: `initWasm({ module_or_path: bytes })`, and `createEngine({ wasm: bytes })` for the worker.

## Building it yourself

```sh
cargo install wasm-pack
wasm-pack build crates/fleuron-wasm --target web --release --no-pack \
  --out-dir npm/wasm --out-name fleuron
cd crates/fleuron-wasm/npm && npm ci && npm run build
```

`npm test` then runs the headless harness: the fixture book through a real worker thread, held against the PDF the CLI writes from the same manuscript. `npm run test:browser` opens the same book in a real browser and holds the painted page against a raster of the PDF the same run exported.

## What the host owns

Fonts, because the engine reads no paths. Fetch the files, cache them, send the bytes once. Going the other way, `fontBytes` hands a file back, which is the only way to reach the face built into the engine, since it has no URL to fetch from.

Images, because layout never decodes one. Probe the header for intrinsic size and draw the pixels on your own side of the wall.

The thread, because layout runs in a worker.
