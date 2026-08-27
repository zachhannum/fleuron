---
title: The preview
description: Painting the display list as SVG, mounting it in an element, and the harness that pages a book through it.
---

The engine's output is a display list: a book's worth of glyph positions, rules and image boxes. Turning that into pixels is a painter's job, and the package ships one. It draws a page as SVG, with one `<text>` element per run of text and an explicit x for every character in it.

## Mounting one

```js
import { Preview } from '@fleuron/wasm';

const preview = await Preview.mount(document.querySelector('#book'));
await preview.setStyle(css);
await preview.setMarkdown(markdown);

preview.page = 12;
preview.zoom = 1.5;
```

That is a working preview. `Preview` starts the worker, loads the module into it, keeps the session that makes a second render cheap, fetches the fonts the book was set in, and paints one page into the element you gave it. You never handle the encoded buffer, the worker messages or the display list yourself, though all three stay exported if you want them.

The rest of the surface. Everything in the first group changes an input and repaints; everything in the second only reads or moves what is already there.

| | |
|---|---|
| `setMarkdown(text, name?)` | one markdown source as the whole book |
| `setBook(sources)` | several sources, in reading order |
| `edit(name, text)` | one source replaced, the rest of the book left standing |
| `remove(name)` | one source dropped |
| `setMetadata({ title, author, extra })` | what names the book |
| `setStyle(css)` | the author stylesheet |
| `addFont(bytes)` | a face, registered for the session's life |
| `render(ops?)` | lay out again, or apply anything the methods above do not reach |

| | |
|---|---|
| `pages` | how many pages the book set to |
| `page` | the page on screen, counting from 1; assigning to it turns the page |
| `next()`, `previous()` | the same, one page at a time |
| `zoom` | points to CSS pixels |
| `warnings` | [what the run had to complain about](../library/diagnostics.md) |
| `svg(page?)` | the markup for a page, painted but not mounted |
| `exportPdf()` | the same run as PDF bytes |
| `destroy()` | closes the worker and empties the element |

The dialect and the section-splitting level are set when the preview is mounted and not after. Changing either means reading every source again, which needs the sources, and the preview does not keep a copy of them.

## A book of several files

`setBook` takes the sources in reading order:

```js
await preview.setBook([
  { name: 'ch01.md', text: one },
  { name: 'ch02.md', text: two },
]);
```

After that, `edit(name, text)` is the keystroke path: it reads that one file again and every other file keeps the lines it already has. A name the book has not seen is appended, so `edit` is also how a file arrives mid-session, and `remove(name)` is how one leaves.

Metadata is the part that catches people out. A book of one file takes its title and author from that file's frontmatter. A book of several has no frontmatter of its own, so it is left unnamed rather than named after whichever chapter happened to come first, and you name it yourself:

```js
await preview.setMetadata({
  title: "Gulliver's Travels",
  author: 'Jonathan Swift',
  extra: { language: 'en' },
});
```

Only the PDF writer reads metadata, so naming a book costs no layout: the pages already on screen are the pages the export writes under the new name.

## When it renders

Every method that changes an input renders. There is no timer anywhere, and that is deliberate rather than an omission.

The worker lets everything the host has already posted arrive before it renders anything. So a burst of edits fired without awaiting them collapses into one render: twenty of them cost one paint, not twenty. Every edit is still applied, in order, and the render that survives produces exactly what it would have produced had nobody typed. The ones it overtook resolve to nothing rather than painting a stale page.

Nothing is interrupted to make that happen. A render occupies the worker from start to finish, so the collapsing happens in the gap before one begins. Change something while a render is running and it finishes, its output is dropped as stale, and yours renders next. A superseded render is one that never started, not one abandoned half-way, which is what keeps the session's caches sound.

The effect is a debounce whose delay is the last render's duration: a long book waits behind its own slow render, a short one repaints at once, and neither pays a fixed delay it did not need. A host that wants a fixed one puts it in front of these calls.

## In React

`@fleuron/react` is the same thing as a component:

```jsx
import { Preview } from '@fleuron/react';

<Preview markdown={markdown} css={css} page={page} zoom={1.5} />;
```

The manuscript and the stylesheet are props, so an edit is a re-render and the engine is handed the one input that changed. `onMount` gives you the underlying preview, for the page count and the PDF export.

React is not a dependency of `@fleuron/wasm`. The wrapper holds no engine logic of its own, so a host that does not use React downloads none of it, and deleting the wrapper leaves a preview a plain page can still mount.

## Painting a page yourself

If you want the pages but not the mounting, because you are drawing to a canvas, running your own scroll container, or rendering somewhere there is no DOM to mount into, call the painter directly:

```js
import { decodeDisplayList, paintPage } from '@fleuron/wasm';

const output = decodeDisplayList(bytes);
element.innerHTML = paintPage(output.pages[0], { fonts: output.fonts, zoom: 2 });
```

`paintPage` returns the markup of one `<svg>` element. Its `viewBox` is the trim size in points with the origin at the top left, which is the display list's own coordinate system, so nothing is rescaled on the way out.

The second argument is all optional:

| option | default | |
|---|---|---|
| `fonts` | none | the font table, `output.fonts`. Leave it out and the painter has no way to name the faces, so every run falls back to a generic serif. |
| `zoom` | `1` | points to CSS pixels. This sets the `width` and `height` the `<svg>` element is given and nothing else: the coordinates inside it do not move, so the page scales without re-laying out. |
| `paper` | `'#ffffff'` | the fill of a rectangle painted behind the page. `null` paints none, which leaves the page transparent. |
| `ink` | `'#000000'` | the fill for text and rules. The display list carries no colour of its own yet, so this is the whole page's. |
| `asset` | none | `(index) => url`, for images. |

The last one takes some explaining. Layout never decodes an image: it probes the header for the intrinsic size, reserves a box of the right shape, and records an index into a table of assets the host supplied. The painter has to turn that index back into something a browser can load, and `asset` is where you do it. Return any URL an `<img>` would take, including a `blob:` or `data:` one, or `null` if you cannot supply it, in which case the painter outlines the box the image would have filled so the space it takes is still visible. There is no op for handing assets to the module yet, so a display list that came from `@fleuron/wasm` never carries an image, and this matters only to a painter given pages from somewhere else.

## Why the preview matches the export

Hand a browser a string and a font and it does its own typesetting: it chooses glyphs, applies kerning and ligatures, and works out where each one goes. fleuron has already done that work. If the browser did it again the two answers would differ a little, over a different shaper version, a rounded advance or a substituted font, and the preview would drift from the PDF.

So the painter leaves the browser nothing to work out. Each `<text>` carries an x for every character it holds, taken from the glyph the engine placed there, and the browser puts characters where it is told instead of measuring them itself.

The PDF writer reads the same display list, so the two painters are drawing the same numbers rather than agreeing by luck. The test suite checks it both ways: every glyph of every page of the fixture book against the x the painter wrote for it, and a screenshot of the preview against a raster of the PDF from the same run.

## Fonts

A page is usually set in several faces at once: body text, an italic aside, a bold heading. Each text run in the display list carries a `fontId`, and `output.fonts` is the table those ids index. The painter draws every run in the file registered under that run's id, which is the file the engine shaped that particular run with.

Those files have to reach the browser as `FontFace`s. For a face you registered yourself you already hold the bytes. For the face built into the engine you do not, and there is no URL to fetch it from, so ask the module for it:

```js
import { faceFamily } from '@fleuron/wasm';

const bytes = await client.fontBytes(fontId);
document.fonts.add(new FontFace(faceFamily(fontId), bytes.buffer));
```

`faceFamily(fontId)` is the family name `paintPage` will ask for, exported so that both ends agree on it. `Preview` does all of this itself, for every face a run actually used.

One file can answer for several faces. The bundled EB Garamond is a variable font whose Regular, Medium, Bold and the rest are one file read at different points on a `wght` axis, and each of those is its own `fontId`. The font table records the point, and the painter writes it out as `font-variation-settings`; a painter that ignored it would draw every weight at the file's default.

A face that never arrives does not blank the run. The painter's `font-family` list ends in `serif`, so the text appears in whatever the reader has: the wrong font, but on the page and readable rather than missing.

## The harness

`examples/preview/` is a page that opens the fixture book, pages through it, and puts the browser's own PDF viewer beside the preview, showing the same page of the same run.

```sh
node examples/preview/serve.mjs
```

It is written the way a consumer writes one, naming no buffer, no worker and no display list, which is why the browser test drives that page rather than one of its own.
