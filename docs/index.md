---
title: fleuron
description: A paged-media layout engine for book-shaped documents, in Rust.
slug: overview
---

Fleuron takes markdown and CSS and typesets a book from them. It shapes the text, breaks and hyphenates the lines, and fragments the result into pages. What comes back is a display structure: where every glyph, rule and image sits on every page. A preview draws that on screen and the PDF writer writes it to a file. The same source compiles to native and to WebAssembly.

## Getting started

There are three ways to use fleuron: as a Rust library, as a command-line binary, and as npm packages for the web.

In Rust, `fleuron` is the engine and `fleuron-markdown` is the frontend that reads markdown into it. A [session](library/sessions.md) keeps the pipeline open, so a preview re-runs only the stages an edit changed. Start at the [library quickstart](library/quickstart.md).

The `fleuron` binary reads markdown and writes a PDF, and takes author stylesheets as flags. It is the quickest way to see output. Start at the [CLI quickstart](cli/quickstart.mdx).

The `fleuron` npm package runs layout in a worker and paints the pages as `<svg>` or writes them as PDF bytes. Start at the [WebAssembly quickstart](wasm/quickstart.md), or open [the demos](https://fleuron.typeworks.dev/demos/) to try it in your browser.

## The pipeline

```text
markdown ─► content tree ──┐
                           ├─► style tree ─► box tree ─► line layout ─► fragmentation ─► pages
CSS ───────────────────────┘                                                               │
                                                                                           ├─► display structure (preview)
                                                                                           └─► PDF (export)
```

Content enters as markdown and becomes a content tree, the semantic document the engine lays out. Styling enters as CSS. The engine builds boxes, breaks the lines, fragments them into pages, and produces the display structure. Two painters read it: one draws a page as `<svg>` for the preview, and one writes the PDF.

See [the markdown mapping](reference/markdown.mdx) for the markdown `fleuron-markdown` reads. See [the content tree](reference/content-tree.md) for the document it produces, which a host with a structured source of its own can build directly.

## Scope

Fleuron typesets book-shaped documents: flowing prose with headings, block quotes, scene breaks, drop caps, images, running heads, multi-column pages, page numbers, and named pages.

It uses CSS to describe the formatting, but it is not a browser engine.

## Status

Pre-alpha. Fleuron is the pagination backend for [Orca](https://github.com/zachhannum/obsidian-orca), the Obsidian novel-writing suite, extracted into its own project.

These pages describe what the engine does today. A page that describes a contract the engine does not implement yet says so at the top.
