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

Every method that changes an input lays the book out again and repaints, so calling them on a keystroke is the intended use. A render the reader has already typed past is dropped rather than painted, so a burst of edits costs one repaint, not one per keystroke.

```js
await preview.edit('ch03.md', text);
await preview.setStyle(css);
```

The rest of the surface:

| | |
|---|---|
| `preview.pages` | how many pages the book set to |
| `preview.page` | the page on screen, counting from 1; assigning to it turns the page |
| `preview.next()`, `preview.previous()` | the same, one page at a time |
| `preview.zoom` | points to CSS pixels |
| `preview.warnings` | [what the run had to complain about](../library/diagnostics.md) |
| `preview.svg(page)` | the markup for a page, painted but not mounted |
| `preview.exportPdf()` | the same run as PDF bytes |
| `preview.destroy()` | closes the worker and empties the element |

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
