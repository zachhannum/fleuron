---
title: fleuron
description: A paged-media layout engine for book-shaped documents, in Rust.
slug: overview
---

fleuron takes markdown and CSS and generates a fully typeset and publish-ready novel. It shapes the text, breaks and hyphenates the lines, fragments the result into pages, and emits a formatted output struct that can be used to preview the book, and exported to PDF. The same source compiles to native and to WebAssembly.

## Getting started

There are three ways to use fleuron: the native Rust library, a CLI, and `npm` packages for use on the web.

Write Rust, and `fleuron` is the engine and `fleuron-markdown` the frontend in front of it. [Sessions](library/sessions.md) can be used to keep the pipeline open so a preview re-runs only what an edit changed. Start at the [library quickstart](library/quickstart.md).

The `fleuron` CLI can read markdown and write a PDF, taking author stylesheets as flags. It is the quickest way to see output. Start at the [CLI quickstart](cli/quickstart.mdx).

The `@fleuron/wasm` package can be used to run layout in a worker and render as `<svg>` or output PDF bytes. Start at the [wasm quickstart](wasm/quickstart.md), or open [the demos](https://fleuron.typeworks.dev/demos/) to test it out.

## The pipeline

```mermaid
markdown ─► content tree ──┐
                           ├─► style tree ─► box tree ─► line layout ─► fragmentation ─► pages
CSS ───────────────────────┘                                                               │
                                                                                           ├─► display list (preview)
                                                                                           └─► PDF (export)
```

Content enters as markdown and becomes a semantic tree. Styling enters as CSS and is parsed into a supported ruleset. From there, the engine lays out the lines, fragments into pages, and exports the display list (a structural representation of the typeset book). From there, the output can be rendered in one of two ways: An `<svg>` preview, or a PDF. 

See the [markdown page](reference/markdown.mdx) for what markdown syntax is supported by `fleuron-markdown`. See the [content tree](reference/content-tree.md) for a reference of the AST the engine uses directly.

## Scope

`fleuron` handles book-shaped documents: flowing prose with headings, block quotes, scene breaks, drop caps, images, running heads, footnotes, and page furniture like recto and verso, page counters and named pages.

While it uses CSS to describe the intended formatting, `fleuron` is not a browser engine.

## Status

Pre-alpha. fleuron is the pagination backend for [Orca](https://github.com/zachhannum/obsidian-orca), the Obsidian novel-writing suite, extracted into its own project.

These pages describe what has landed. A page that documents a contract before its implementation says so at the top.
