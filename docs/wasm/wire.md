---
title: The wire
description: The postcard display list, the worker protocol, and what the host owes the engine.
---

## The encoding

The display list crosses as [postcard](https://postcard.jamesmunns.com/): a non-self-describing, varint-packed format. Field names do not travel, small integers cost one byte, and there is no parse step that allocates a tree of maps before you can read the first page. *Pride and Prejudice*, 333 pages and 9,667 lines, encodes to about 6 MB in 6 ms.

Inputs are a different contract. [The content tree](../reference/content-tree.md) is a Rust type a frontend constructs, and it serializes to JSON, which is also how a host hands one over with the `content` op.

A version leads the encoding, and a host reads it before anything else. The encoding is positional: a reader walks fields in the order they were written and cannot notice that the order changed. So a module and a host that disagree about the shape of the display list have to fail at the first byte. `decodeDisplayList` refuses a version it does not know, and `wireVersion()` is what the module writes.

## What crosses

In: whatever changed. Markdown source, CSS text, font bytes, a content tree. Inputs are ops on a session the module keeps, not a book re-sent per frame. The engine opens nothing, so a face that has not crossed cannot be used.

Out: one transferable `ArrayBuffer`, holding either the postcard-encoded display list or, on the export path, PDF bytes. Transferred rather than copied.

## The display list

[The display-list reference](../reference/display-list.mdx) is the structure. Three things about it matter to a host in particular.

Coordinates are points, origin top-left. Every painter, SVG, canvas or PDF, consumes the same numbers. A preview that disagrees with the export about where a glyph goes has a bug in the painter, not in the engine.

Faces carry their instance. The font table says where on its file's axes each face sits. A variable file names several cuts and they are one file, so a painter that does not pin the axes draws the default weight for every one of them.

Glyphs carry their text. Each text run holds the string it was shaped from, and each glyph a byte range into it. Only the shaper knew the correspondence, so it travels with the glyphs. A painter that wants selection, copy-and-paste or accessible text reads it through those ranges; a painter that only draws ignores it.

## One book, both targets

Layout is deterministic and the wire is positional, so the display list a worker produces is the one a native run produces, byte for byte. The perf gate encodes the gate book on both targets and CI compares the digests.

PDF bytes are not identical across builds, though the PDF is the same book, of the same length, with the same pages and the same text. The writer orders its font objects by a hash that carries the build in it: the target's pointer width, and the identity of the crates it was compiled against. So two builds can number the same two font objects the other way round. One build renders one book to one file every time, which is what the end-to-end test's two runs check, and the digest it holds the pipeline to is of the display list, where nothing about the build reaches the bytes.

## The protocol

A request is an edit, a render, a question, or an edit and one of those:

```js
{ id, generation, ops: [{ op: 'style', css }], want: 'preview' }
```

Request and response are paired by `id`, and each request carries a generation the worker echoes back untouched.

`want: 'font'` is the question: the file a `font_id` was registered from, for a painter that has to draw with the bytes the engine shaped with. Nothing overtakes it and it overtakes nothing, since a face keeps its id for the session's life and the answer cannot go stale.

The host raises the generation whenever the input goes stale, at a keystroke in a stylesheet or a new manuscript. A response whose generation is behind the current one is dropped without painting. The engine does not need to know why.

### Latest wins, and inputs are never dropped

The worker lets everything already sent arrive before it renders anything. Ops are applied in the order they arrived. Only the newest render in the batch runs, and the ones it overtook come back as `superseded`. A render the reader typed past costs nothing, and the render that follows it produces exactly what it would have produced had nobody typed.

That is also what makes it cache-safe. A superseded render is one that never started, not one abandoned half-way through a stage, so no stage is left partly rebuilt for the next call to serve.

### Errors come back on the same channel

A request the engine refuses, font bytes that are not a font, a content tree that will not parse: each replies with an error, and the session carries on rendering.

A warning is different. A book that laid out anyway reports through the display list's own `warnings`, which holds the whole run's, [the frontend's complaints included](../library/diagnostics.mdx).

## Host duties

Own the fonts. Fetch them, cache them, decide when a face has changed. The engine registers what it is handed and warns about what it is not.

Own the images. Fetch each file and hand the bytes over; the engine reads the header for the intrinsic size and decodes nothing. Decode the pixels when you paint.

Own the thread. Layout runs in a worker.

Check the version tag. Refuse a mismatch at the first byte.
