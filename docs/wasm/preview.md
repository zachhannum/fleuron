---
title: The preview
description: Painting the display list as SVG, mounting it in an element, and the harness that pages a book through it.
---

The display list is a book's worth of glyph positions. Turning it into something on screen is a painter's job, and the package ships one: SVG, one `<text>` per run, an x for every character, in the face the engine shaped with.

## Mounting one

```js
import { Preview } from '@fleuron/wasm';

const preview = await Preview.mount(document.querySelector('#book'));
await preview.setStyle(css);
await preview.setMarkdown(markdown);

preview.page = 12;
preview.zoom = 1.5;
```

That is the whole surface for a host that wants to see a book. `Preview` opens the worker, loads the module, keeps the session behind it, loads the faces the run drew with, and paints one page into the element it was given. The postcard buffer, the worker protocol and the display list are underneath it rather than in front of it.

Every input method renders and repaints, and a render the reader has already typed past paints nothing, so calling them on a keystroke is the intended use:

```js
await preview.edit('ch03.md', text);
await preview.setStyle(css);
```

`preview.pages` is the count, `preview.next()` and `preview.previous()` turn a page, `preview.warnings` is [what the run had to complain about](../library/diagnostics.md), and `preview.exportPdf()` is the same run as PDF bytes. `preview.destroy()` closes the worker and gives the element back.

## In React

`@fleuron/react` is the same thing as a component and holds no engine logic of its own:

```jsx
import { Preview } from '@fleuron/react';

<Preview markdown={markdown} css={css} page={page} zoom={1.5} />;
```

React is not a dependency of `@fleuron/wasm`, and deleting the wrapper leaves a preview a plain page can still mount.

## Painting a page yourself

A host with a painter of its own keeps the pages and the reader. The one here is exported too:

```js
import { decodeDisplayList, faceFamily, paintPage } from '@fleuron/wasm';

const output = decodeDisplayList(bytes);
element.innerHTML = paintPage(output.pages[0], { fonts: output.fonts, zoom: 2 });
```

`paintPage` returns an `<svg>` element's markup. The `viewBox` is the trim in points with the origin top left, which is the display list's own coordinate system, and `zoom` sets only the width and height the element is given, which moves nothing inside it.

`paper`, `ink` and `asset` are the rest of it: what the page is printed on, what it is printed in, and where an image's pixels come from. An asset the host cannot supply is drawn as the box layout reserved for it.

## Why the browser is given no room

A run is one `<text>` carrying an x for every character it holds, taken from the glyph the shaper put there. The browser therefore positions rather than shapes, and cannot disagree with the export about where a glyph goes.

The face is the file the engine shaped with. `session.fontBytes(id)`, or `client.fontBytes(id)` through the worker, hands back the file a `font_id` was registered from. That is the only way to reach the bundled face: it is inside the module, and there is no URL to fetch it from. A variable file names several cuts, and the display list's font table says which instance each one sits at, so a painter pins those axes rather than drawing the file's default for all of them.

A face that never arrives falls through the painter's stack to whatever the reader has. The page is then set in the wrong font, which is the point: it is set.

## The harness

`examples/preview/` is a page that opens the fixture book, pages through it, and puts the browser's own PDF viewer beside the preview, over the same run.

```sh
node examples/preview/serve.mjs
```

It is written the way a consumer writes one, which is what makes it worth having: it names no buffer, no worker and no display list.
