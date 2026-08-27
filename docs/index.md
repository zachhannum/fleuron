---
title: fleuron
description: A paged-media layout engine for book-shaped documents, in Rust.
slug: overview
---

fleuron takes markdown plus CSS, performs inline layout — shaping, line breaking, hyphenation — fragments the result into pages, and emits a display list for preview and a PDF for export. It compiles to native and WebAssembly from the same core.

A *fleuron* is the printer's flower ❦, the ornament set into a page to mark a pause. This is one, in Rust.

## The pipeline

One way through. Content enters as markdown and becomes a semantic tree, styling enters as CSS, and everything downstream consumes a single resolved representation. Nothing reaches back upstream.

```text
markdown ─► content tree ──┐
                           ├─► style tree ─► box tree ─► line layout ─► fragmentation ─► pages
CSS ───────────────────────┘                                                               │
                                                                                           ├─► display list (preview)
                                                                                           └─► PDF (export)
```

Break decisions fall out of the layout pass itself, which is most of why a 333-page book reaches PDF bytes in 287 ms.

[The markdown mapping](reference/markdown.md) is what each construct becomes. The [content tree](reference/content-tree.md) stays public for a host with a structured source of its own, but markdown is the way in.

## Three invariants

**Styling enters as CSS.** A built-in user-agent stylesheet supplies the defaults; author CSS cascades over it. The supported subset is written down in [the CSS subset](css-subset.md); anything outside it is reported with the line and column it was written at, and the run continues.

**The engine never touches the DOM.** Bytes in, bytes out. SVG, canvas and PDF are interchangeable painters over one display list.

**Layout never decodes images.** Header probes yield intrinsic size, orientation and DPI; painters decode pixels on their own side of the wall.

## Three ways in

**A Rust library.** `fleuron` is the engine: style compilation, box construction, inline layout, fragmentation, page assembly. Pure library, no I/O. `fleuron-markdown` is the frontend in front of it. One call lays a book out, and a [session](library/sessions.md) makes a preview re-run only the stages an edit changed. Start at the [library quickstart](library/quickstart.md).

**A command-line binary.** `fleuron` reads markdown and writes a PDF, taking author stylesheets on the command line. Batch-friendly, and the fastest way to see output. Start at the [CLI quickstart](cli/quickstart.md).

**A WebAssembly module.** `fleuron-wasm` runs layout in a worker and returns one transferable buffer: the display list, or PDF bytes. Zero DOM access, and an SVG painter over what comes back. Start at the [wasm quickstart](wasm/quickstart.md).

## Scope

fleuron is scoped to book-shaped documents: flowing prose with headings, block quotes, scene breaks, drop caps, images, running heads, footnotes, and page machinery — recto and verso, page counters, named pages.

It is not a browser engine. There is no float layout, no tables, no grid or flexbox, no transforms. CSS outside the supported subset is reported through the diagnostics channel rather than silently ignored.

## Status

Pre-alpha. Development happens in service of [Orca](https://github.com/zachhannum/obsidian-orca), the Obsidian novel-writing suite — fleuron is its pagination backend, extracted.

Pages here describe what has landed. A page that writes down a contract ahead of its implementation says so at the top.
