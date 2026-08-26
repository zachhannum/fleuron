---
title: The wire
description: The postcard display list, the worker protocol, and what the host owes the engine.
---

:::caution[Not shipped yet]
The display-list types derive `Serialize` and not `Deserialize`, and `postcard` is in no manifest. What follows is the contract, written down so a host can be built against it; the round trip described here does not exist yet.
:::

## Why postcard

The wire is not the input contract. [The content tree](../reference/content-tree.md) crosses as JSON because it maps one-to-one onto mdast and a frontend should be able to serialize its own tree with a field rename. The display list crosses as [postcard](https://postcard.jamesmunns.com/) because it is machine output that no one authors and everyone measures: a book's worth of per-glyph positions, produced once per keystroke, decoded on the main thread.

Postcard is a non-self-describing, varint-packed format. Field names do not travel, small integers cost one byte, and there is no parse step that allocates a tree of maps before you can read the first page.

## What crosses

**In**, one buffer: the content tree, the author stylesheets, and the font bytes each `@font-face` needs. The engine opens nothing, so a font that is not in the buffer is a font that does not exist.

**Out**, one transferable `ArrayBuffer`: either the postcard-encoded `LayoutOutput` or, on the export path, PDF bytes. Transferred rather than copied.

A version tag leads the encoding, and a host reads it before anything else. A module and a host that disagree about the layout of the wire must fail loudly at the first byte rather than subtly at the hundredth page.

## The display list

[The display-list reference](../reference/display-list.md) is the structure. Two things about it matter to a host in particular.

**Coordinates are points, origin top-left.** Every painter — SVG, canvas, PDF — consumes the same numbers. A preview that disagrees with the export about where a glyph goes has a bug in the painter, not in the engine, and that is the point of shipping one list rather than two renderers.

**Glyphs carry their text.** Each text run holds the string it was shaped from, and each glyph a byte range into it. Only the shaper knew the correspondence, so it travels with the glyphs. A painter that wants selection, copy-and-paste or accessible text reads it through those ranges; a painter that only draws ignores it.

## The protocol

Request and response are paired by an id, and each request carries a generation token.

The host raises the generation whenever the input becomes stale — a keystroke in a stylesheet, a new manuscript. A response whose generation is behind the current one is dropped without painting. The engine does not need to know why; it echoes the token back and the host decides.

This is cheaper than cancellation and strictly more useful: a layout already in flight finishes, its result is thrown away, and no work is left half-done in the module.

## Host duties

**Own the fonts.** Fetch them, cache them, decide when a face has changed. The engine registers what it is handed and warns about what it is not.

**Own the images.** Probe headers for intrinsic size, orientation and DPI, pass those in, decode pixels when you paint. Layout never decodes.

**Own the thread.** Layout runs in a worker or it runs in the way of the visitor.

**Check the version tag.** Refuse a mismatch at the first byte.
