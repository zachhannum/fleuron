---
title: The wire
description: The postcard display structure, the worker protocol, and what the host is responsible for.
---

## The encoding

The display structure crosses as [postcard](https://postcard.jamesmunns.com/): a non-self-describing, varint-packed format. Field names do not travel, small integers cost one byte, and there is no parse step that allocates a tree of maps before the first page can be read. *Pride and Prejudice*, 333 pages and 9,667 lines, encodes to about 6 MB.

Inputs are a different contract. [The content tree](../reference/content-tree.md) is a Rust type a frontend constructs, and it serializes to JSON, which is also how a host hands one over with the `content` op.

A version leads the encoding, and a host reads it before anything else. The encoding is positional: a reader walks fields in the order they were written and cannot detect a change to that order. So a module and a host that disagree about the shape of the display structure have to fail at the first byte. `decodeDisplayList` rejects an unknown version, and `wireVersion()` is what the module writes.

`WIRE_VERSION` is the shape of the display structure. `VERSION` is the release the package was published at, and it is the one to quote in a bug report or to pin in a host's manifest; the two move independently.

## What crosses

In: whatever changed. Markdown source, CSS text, font bytes, a content tree. Inputs are ops on a session the module keeps, not a book re-sent per frame. The engine opens nothing, so a face that has not crossed cannot be used.

Out: one transferable `ArrayBuffer`: the postcard-encoded display structure, or PDF bytes on the export path. Transferred rather than copied.

## The display structure

The [display structure reference](../reference/display-structure.mdx) has the full shape. Three things about it matter to a host in particular.

Coordinates are points, origin top-left. Every painter, SVG, canvas or PDF, consumes the same numbers. A preview that disagrees with the export about where a glyph goes has a bug in the painter, not in the engine.

Faces include their instance. The font table records where on its file's axes each face sits. A variable file names several styles and they are one file, so a painter that does not pin the axes draws the default weight for every one of them.

Glyphs are tied to their text. Each text run has the string it was shaped from, and each glyph a byte range into it. Only the shaper knew the correspondence, so the display structure records it. A painter that supports selection or accessible text reads it through those ranges; a painter that only draws ignores it.

## One book, both targets

Layout is deterministic and the wire is positional, so the display structure a worker produces is the one a native run produces, byte for byte.

PDF bytes are not identical across builds, though the PDF is the same book, of the same length, with the same pages and the same text: two builds can number the same two font objects the other way round. One build renders one book to one file every time, so a digest taken to pin the output down is taken of the display structure rather than of the PDF.

## The protocol

A request is an edit, a render, a question, or an edit and one of those:

```js
{ id, generation, ops: [{ op: 'style', css }], want: 'preview' }
```

Request and response are paired by `id`, and each request has a generation the worker echoes back untouched.

`want: 'font'` is the question: the file a `font_id` was registered from, for a painter that has to draw with the bytes the engine shaped with. Nothing overtakes it and it overtakes nothing, since a face keeps its id for the session's life and the answer cannot go stale.

The host raises the generation whenever the input goes stale, at a keystroke in a stylesheet or a new manuscript. A response whose generation is behind the current one is dropped without painting.

### Latest render wins

The worker lets everything already sent arrive before it renders anything. Ops are applied in the order they arrived. Only the newest render in the batch runs, and the ones it overtook come back as `superseded`. A render the reader typed past costs nothing, and the render that follows it is the same as if nobody had typed.

That is also what makes it cache-safe. A superseded render is one that never started, not one abandoned half-way through a stage, so no stage is left partly rebuilt for the next call to serve.

### Errors and warnings

A request the engine cannot apply, font bytes that are not a font, a content tree that will not parse: each replies with an error, and the session continues rendering.

A warning is different. A book that laid out anyway reports through the display structure's own `warnings`, which is the whole run's, [the frontend's included](../library/diagnostics.mdx).

## Host duties

The host fetches the fonts, caches them, and decides when a face has changed. The engine registers what it is handed and warns about what it is not.

The host fetches each image file and hands the bytes over. The engine reads the header for the intrinsic size and decodes nothing, so the painter decodes the pixels.

The host starts the worker. Layout runs there.

The host checks the version tag and refuses a mismatch at the first byte.
