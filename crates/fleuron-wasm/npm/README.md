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

`fleuron-react` is the same thing as a component, with no engine logic
of its own.

## Links

A click on a link in the preview follows it. A link to a place in the
book turns to the page of that place. A link to a url opens the url in
a new window.

`onLink` lets the host decide what a click does. The preview calls it
with the link and the click event, before it follows the link. If it
returns `false`, the page does not turn. If the host gives an
`onLink`, the preview opens no url.

```js
const preview = await Preview.mount(document.querySelector('#book'), {
  onLink: (link) => {
    if (link.to.kind === 'uri') {
      openTab(link.to.url);
    }
  },
});
```

A host that draws pages with `paintPage` reads `page.links`. Each link
has one box for each line, in points, and a target. A target in the
book carries the element, its box, and the index of its page in the
book, which is the `first` of the range that fetches that page.
`linkAt(page, x, y)` gives the link at a point in page points. The
painter marks each box with a transparent `rect[data-link]` that takes
no pointer events. `paintPage(page, { links: false })` leaves the marks
out.

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
import { Client, paintPage, styleOp } from 'fleuron';

const worker = new Worker(new URL('./fleuron.worker.js', import.meta.url), { type: 'module' });
const client = new Client({ post: (request, transfer) => worker.postMessage(request, transfer) });
worker.onmessage = ({ data }) => client.receive(data);

const output = await client.preview([
  { op: 'markdown', name: 'manuscript.md', text: markdown },
  styleOp(css),
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
await client.preview([styleOp('@page { margin-bottom: 84pt }')]);
await client.preview([{ op: 'edit', name: 'ch03.md', text }]);
await client.apply([{ op: 'font', bytes }]);
```

A stylesheet that only moves the page box re-fragments over lines that
are already broken. A keystroke in one chapter reparses that file and
leaves every other section's lines alone. Font bytes cross once and
stay registered. `client.stages` reports how many times each stage has
run, which shows when a cache served.

## Faces from a stylesheet

A `@font-face` rule gives a face a family name, a weight and a style.
The following example loads a face through a rule:

```js
const face = await fetch('/fonts/Junicode-Cond.otf');

const preview = await Preview.mount(document.querySelector('#book'), {
  fonts: { 'Junicode-Cond.otf': new Uint8Array(await face.arrayBuffer()) },
});
await preview.setStyle(`
  @font-face {
    font-family: "Junicode Cond";
    src: url("Junicode-Cond.otf");
    font-weight: 400;
    font-style: normal;
  }

  book { font-family: "Junicode Cond", serif; }
`);
```

The key in `fonts` is the string that `url()` holds. It does not have
to be a real URL. The engine registers the face under the family, the
weight and the style that the rule declares, not under the name in the
file.

The file and the sheet can arrive in either order.
`preview.addFont(bytes, url)` sends a file after the preview is
mounted. In a worker of your own, the op is `{ op: 'font', url, bytes }`.
`fleuron-react` takes the same record as its `fonts` prop.

A rule whose url has no bytes gives a warning, and the text uses the
next family in its `font-family` list.

## The CSS the engine accepts

`SUBSET` is the vocabulary of the engine in the module: every property
with the values it takes, the selectors, the units, and the at-rules
that parse. A host with a style editor completes from it. You read it
with no book laid out and no file of your own.

The following example lists every property a style rule takes, with
the values it accepts:

```js
import { SUBSET } from 'fleuron';

for (const property of SUBSET.properties) {
  console.log(`${property.name}: ${property.syntax}`);
}
```

`SUBSET.version` is the engine version the description came from,
which is the version the package ships under. The
[CSS subset](https://fleuron.typeworks.dev/css-subset/) page describes
the same vocabulary in prose.

## Batch

```js
import { decodeDisplayList, initWasm, render, renderPdf } from 'fleuron';

await initWasm();
const output = decodeDisplayList(render(markdown, css));
const pdf = renderPdf(markdown, css);
```

## The display structure

`client.preview` hands back pages of text runs, rules and images, in
points, origin top left. Each text run has the string it was shaped
from and each glyph a byte range into it, which is what a painter needs
for selection and copy-and-paste.

`paintPage` draws one of them as SVG. Each run becomes one `<text>`
with an x for every character in it, so the browser places the
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
