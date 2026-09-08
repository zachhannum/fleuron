---
title: The wire
description: The postcard display structure, the worker protocol, and what the host is responsible for.
---

The module and the host are separate programs. This page describes what crosses between them: the encoding the display structure travels in, the protocol a request and its reply follow, and the parts of a preview the host owns.

## The encoding

The display structure crosses as [postcard](https://postcard.jamesmunns.com/), a non-self-describing, varint-packed format. Field names do not travel, small integers cost one byte, and nothing allocates a tree of maps before the first page can be read.

Inputs are a different contract. [The content tree](../reference/content-tree.md) is a Rust type a frontend builds, and it serializes to JSON, which is how a host hands one over with the `content` op.

A version leads the encoding, and a host reads it before anything else. The encoding is positional: a reader walks the fields in the order they were written and cannot detect a change to that order. So a module and a host that disagree about the shape of the display structure have to fail at the first byte. `decodeDisplayList` rejects a version it does not know, and `wireVersion()` is the version the module writes.

`WIRE_VERSION` is the shape of what crosses, and it moves whenever the ops or the display structure change shape. What it refuses is a display structure it cannot read. An op in a shape the module no longer takes is answered on the error channel instead. `VERSION` is the release the package was published at, and it is the one to quote in a bug report or to pin in a host's manifest. The two move independently.

## What crosses

A request carries whatever changed: markdown source, stylesheets, font bytes, or a content tree. Inputs are ops on a session the module keeps, rather than a book re-sent per frame. The engine opens no files, so a face that has not crossed cannot be used.

A reply carries one transferable `ArrayBuffer`: the postcard-encoded display structure, or PDF bytes on the export path. It is transferred rather than copied.

A `preview` reply need not carry the whole book. `first` and `count` on the request ask for `count` pages starting at `first`, counting from 0. The reply's `pages` is that slice, `first` says where the slice begins, and `bookPages` says how many pages the book has, so a reply carrying one page still answers that. `fonts`, `assets` and `warnings` ride every reply whole, since none of them is per page. Leaving `first` and `count` out asks for the whole book.

## The display structure

[The display structure reference](../reference/display-structure.mdx) has the full shape. Four things about it matter to a host in particular.

Coordinates are in points, with the origin at the top left. Every painter, SVG, canvas or PDF, reads the same numbers. A preview that disagrees with the export about where a glyph goes has a bug in the painter rather than in the engine.

Faces include their instance. The font table records where on its file's axes each face sits. One variable file names several styles, so a painter that does not pin the axes draws the default weight for every one of them.

Glyphs are tied to their text. Each text run carries the string it was shaped from, and each glyph a byte range into it, because only the shaper knew which glyph came from which character. A painter that supports selection or accessible text reads the text through those ranges, and a painter that only draws ignores them.

Runs are tied to the manuscript. Each text run names the content node it was shaped from and the bytes of that node it stands for, so a host maps a cursor in the manuscript onto a page, and a click on a page back onto the manuscript. Text the engine wrote itself, such as a page number or a running head, names no node. A node id is the engine's own name for a place in the book, and the last two questions below turn it into a file and a byte of one.

## One book, both targets

Layout is deterministic and the wire is positional, so the display structure a worker produces is the one a native run produces, byte for byte.

PDF bytes are not identical across builds, though the PDF is the same book, of the same length, with the same pages and the same text. Two builds can number the same two font objects the other way round. One build writes one book to one file every time, so a digest taken to pin the output down is taken of the display structure rather than of the PDF.

## The protocol

A request is an edit, a render, a question, or an edit and one of those. The following request sets the author's stylesheets and asks for a preview:

```js
{ id, generation, ops: [{ op: 'style', sheets: [{ name, css }] }], want: 'preview' }
```

The `style` op takes the author's sheets in cascade order, each under a name. A warning names the sheet its declaration was written in, as `preset.css:12:3`, so a host that builds its styling out of layers sends the layers.

Request and response are paired by `id`, and each request carries a generation the worker echoes back untouched. The host raises the generation whenever the input goes stale, at a keystroke in a stylesheet or at a new manuscript. A response whose generation is behind the current one is dropped without painting.

Some requests are questions rather than renders, and none of them overtakes a render or is overtaken by one.

`want: 'font'` asks for the file a `font_id` was registered from, for a painter that has to draw with the bytes the engine shaped with. A face keeps its id for the session's life, so the answer cannot go stale.

`want: 'preview'` naming `first` and `count`, with no `ops` of its own, asks what a page of the book already is rather than what an edit produced. An edit that also names a range, fetching the one page it changed rather than the whole book, is still a render, since it does say what that edit produced, and it supersedes the renders it overtook.

The last two questions are about the manuscript rather than the page. `want: 'node'`, with `source` and `byte`, answers with the node that byte of that file was read into, which is the node the display structure's runs name. `want: 'source'`, with `node`, answers with `{ source, start, end }`: the file the node was read from, and the bytes of it. Both answer in JSON, the same contract the content tree crosses in, and both answer `null` where nothing was read: a blank line between chapters, a node the engine synthesized, a tree the host built rather than parsed. `Client.nodeAt` and `Client.sourceOf` are the two on the host's side.

### Latest render wins

The worker lets everything already sent arrive before it renders anything. Ops are applied in the order they arrived. Only the newest render in the batch runs, and the ones it overtook come back as `superseded`. A render the reader typed past costs nothing, and the render that follows it is the same as if nobody had typed.

That is also what keeps the session's caches sound. A superseded render is one that never started rather than one abandoned halfway through a stage, so no stage is left partly rebuilt for the next call to serve.

### Errors and warnings

A request the engine cannot apply replies with an error, and the session carries on rendering. Font bytes that are not a font, and a content tree that will not parse, are both that case.

A warning is different. A book that laid out anyway reports through the display structure's own `warnings`, which is the whole run's, [the frontend's included](../library/diagnostics.mdx).

## What the host owns

The host fetches the fonts, caches them, and decides when a face has changed. The engine registers what it is handed and warns about what it is not.

The host fetches each image file and hands the bytes over. The engine reads the header for the intrinsic size and decodes nothing, so the painter decodes the pixels.

The host starts the worker. Layout runs there.

The host checks the version tag and refuses a mismatch at the first byte.
