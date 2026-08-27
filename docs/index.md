---
title: fleuron
description: A paged-media layout engine for book-shaped documents, in Rust.
slug: overview
---

fleuron takes markdown and CSS and gives back a typeset book. It shapes the text, breaks and hyphenates the lines, fragments the result into pages, and emits a display list for preview and a PDF for export. The same source compiles to native and to WebAssembly.

A fleuron is the printer's flower ❦, the ornament set into a page to mark a pause.

## Getting started

There are three ways to use fleuron, and [Install](install.md) covers all three.

Write Rust, and `fleuron` is the engine and `fleuron-markdown` the frontend in front of it. One call lays a book out; a [session](library/sessions.md) keeps the pipeline open so a preview re-runs only what an edit changed. Start at the [library quickstart](library/quickstart.md).

Work from a shell, and the `fleuron` binary reads markdown and writes a PDF, taking author stylesheets as flags. It is the quickest way to see output. Start at the [CLI quickstart](cli/quickstart.mdx).

Build for the browser, and `fleuron-wasm` runs layout in a worker and hands back one transferable buffer holding either the display list or PDF bytes. It never touches the DOM. Start at the [wasm quickstart](wasm/quickstart.md), or open [the demos](https://zachhannum.github.io/fleuron/demos/) and edit a book in your browser.

## The pipeline

```text
markdown ─► content tree ──┐
                           ├─► style tree ─► box tree ─► line layout ─► fragmentation ─► pages
CSS ───────────────────────┘                                                               │
                                                                                           ├─► display list (preview)
                                                                                           └─► PDF (export)
```

Content enters as markdown and becomes a semantic tree. Styling enters as CSS. Everything downstream reads one resolved representation, and nothing reaches back upstream.

Break decisions come out of the layout pass, which is most of why a 333-page book reaches PDF bytes in 287 ms.

[The markdown mapping](reference/markdown.md) says what each construct becomes. Most callers write markdown; the [content tree](reference/content-tree.md) stays public for a host that already has structured content.

## Invariants

1. **Styling enters as CSS.** A built-in user-agent stylesheet supplies the defaults and author CSS cascades over it. [The CSS subset](css-subset.mdx) lists what the engine understands. Anything else is reported with the line and column it was written at, and the run continues.
2. **The engine never touches the DOM.** Bytes in, bytes out. SVG, canvas and PDF are interchangeable painters over one display list.
3. **Layout never decodes images.** A header probe gives intrinsic size, orientation and DPI. Painters decode the pixels themselves.

## Scope

fleuron handles book-shaped documents: flowing prose with headings, block quotes, scene breaks, drop caps, images, running heads, footnotes, and page furniture like recto and verso, page counters and named pages.

It is not a browser engine. There is no float layout, no tables, no grid or flexbox, no transforms. CSS the engine does not support is reported through the diagnostics channel rather than ignored.

## Status

Pre-alpha. fleuron is the pagination backend for [Orca](https://github.com/zachhannum/obsidian-orca), the Obsidian novel-writing suite, extracted into its own project.

These pages describe what has landed. A page that documents a contract before its implementation says so at the top.
