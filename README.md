# fleuron ❦

A paged-media layout engine for book-shaped documents, in Rust.

[Documentation](https://zachhannum.github.io/fleuron/) · [Demos](https://zachhannum.github.io/fleuron/demos/) · [API](https://zachhannum.github.io/fleuron/api/fleuron/)

fleuron takes markdown and CSS and gives back a typeset book. It shapes
the text, breaks and hyphenates the lines, fragments the result into
pages, and emits a display list for preview and a PDF for export. The
same source compiles to native and to WebAssembly.

A fleuron is the printer's flower ❦, the ornament set into a page to
mark a pause.

## What it does

fleuron repaginates a full-length manuscript while the writer waits, and
gives them a preview that matches the export.

It owns the whole pipeline: [harfrust] shaping, Unicode line breaking
and segmentation, Knuth-Plass justification, and CSS Fragmentation over
a box tree it builds itself. Break decisions come out of the layout
pass, so a 333-page novel reaches PDF bytes in 287 ms. The preview and
the PDF are painted from the same display list, so they cannot come out
different. The engine does no I/O and links no platform library, so it
builds for any target Rust does.

## Scope

fleuron handles book-shaped documents: flowing prose with headings,
block quotes, scene breaks, drop caps, images, running heads, footnotes,
and page furniture like recto and verso, page counters and named pages.

It is not a browser engine. There is no float layout, no tables, no grid
or flexbox, no transforms. CSS the engine does not support is reported
with the line and column it was written at, and the book lays out
anyway.

## Architecture

```
markdown ─► content tree ──┐
                           ├─► style tree ─► box tree ─► line layout ─► fragmentation ─► pages
CSS ───────────────────────┘                                                               │
                                                                                           ├─► display list (preview)
                                                                                           └─► PDF (export)
```

The pipeline runs one way. Nothing downstream reaches back upstream.

| crate | |
|---|---|
| `fleuron` | The engine: style compilation, box construction, inline layout, fragmentation, page assembly. No I/O. |
| `fleuron-markdown` | Markdown in, sections out. The mapping is in [`docs/reference/markdown.md`](docs/reference/markdown.md). |
| `fleuron-cli` | The `fleuron` binary. Markdown in, PDF out. |
| `fleuron-wasm` | WASM bindings, plus a worker, a display-list reader and an SVG painter in TypeScript. Ships as `@fleuron/wasm`. |

`@fleuron/react` wraps the preview as a component and holds no engine
logic.

Most callers write markdown. The content tree stays public for a host
that already has structured content, such as a CMS or a docx converter.

### Invariants

1. **Styling enters as CSS.** A built-in user-agent stylesheet supplies
   the defaults and author CSS cascades over it. Everything downstream
   reads the resolved style tree. [`docs/css-subset.mdx`](docs/css-subset.mdx)
   lists what the engine understands.
2. **The engine never touches the DOM.** Bytes in, bytes out. SVG,
   canvas and PDF are interchangeable painters over the display list.
3. **Layout never decodes images.** A header probe gives intrinsic size,
   orientation and DPI. Painters decode the pixels themselves.

## Performance

Two public-domain novels, laid out from markdown and held against fixed
budgets.

| | pages | parse | style | line layout | fragment | PDF | end to end | layout peak |
|---|---|---|---|---|---|---|---|---|
| *Pride and Prejudice* | 333 | 1 ms | 1 ms | 128 ms | 5 ms | 150 ms | 287 ms | 12 MiB |
| *The Count of Monte Cristo* | 1254 | 5 ms | 6 ms | 505 ms | 20 ms | 563 ms | 1.10 s | 47 MiB |

Four times the book costs about four times the time and four times the
memory.

A [session](docs/library/sessions.md) keeps the same pipeline open and
re-runs only the stages an edit invalidates. Restyling *Pride and
Prejudice* with a sheet that moves the page box costs 6 ms, against the
128 ms of line breaking it skips.

Apple M-series, release build, best of three. The
[demos](https://zachhannum.github.io/fleuron/demos/) run the same two
books in your browser and name the machine they ran on.

The budgets: a book-scale manuscript reaches PDF bytes in under a second
natively, lays out in under half a second in a WebAssembly worker, stays
under 32 MiB doing it, and re-renders a style change in under 20 ms from
a session capped at 64 MiB. CI checks them natively and under wasm on
every pull request.

```
cargo run --release -p fleuron-fixtures --bin perf-gate
cargo bench -p fleuron
```

## Status

Pre-alpha. fleuron is the pagination backend for [Orca], the Obsidian
novel-writing suite, extracted into its own project. The build order is
in the issue tracker.

## License

Dual-licensed under either of [Apache-2.0](LICENSE-APACHE) or
[MIT](LICENSE-MIT) at your option.

[Orca]: https://github.com/zachhannum/obsidian-orca
[harfrust]: https://github.com/harfbuzz/harfrust
