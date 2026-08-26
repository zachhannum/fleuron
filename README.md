# fleuron ❦

*A paged-media layout engine for book-shaped documents, in Rust.*

[**Documentation**](https://zachhannum.github.io/fleuron/) · [**API**](https://zachhannum.github.io/fleuron/api/fleuron/)

**fleuron** takes structured content plus CSS, performs
inline layout (shaping, line breaking, hyphenation), fragments it into pages,
and emits a display list for preview and a PDF for export. It compiles to
native and WebAssembly from the same core.

> A *fleuron* is the printer's flower ❦ — the ornament set into a page to
> mark a pause. This is one, in Rust.

## Why

fleuron is built to repaginate a full-length manuscript while someone
waits, and to give that person a preview the export cannot contradict.

Both come from owning the pipeline end to end: harfrust shaping, Unicode
line breaking and segmentation, Knuth-Plass-quality justification, and
CSS Fragmentation semantics applied to a box tree it builds itself.
Break decisions fall out of the layout pass, so a 333-page novel reaches
PDF bytes in 283 ms. The preview is *exactly* the export, because both
are painted from the same display list. And because the engine does no
I/O and depends on no platform library, all of it runs anywhere Rust
does, WebAssembly included.

Shaping is [harfrust], the pure-Rust HarfBuzz port from the Google Fonts
team (successor to the archived rustybuzz).

## Scope

fleuron is scoped to **book-shaped documents**: flowing prose with
headings, block quotes, scene breaks, drop caps, images, running heads,
footnotes, and page machinery (recto/verso, page counters, named pages).

It is not a browser engine. There is no float layout, no tables, no grid
or flexbox, no transforms. CSS that falls outside the supported subset is
reported through the diagnostics channel rather than silently ignored.

## Architecture

The pipeline is one-way. Content enters as a semantic tree (markdown
frontends like [Orca] produce this from remark/rehype), styling enters
through the style compiler, and everything downstream consumes a single
resolved representation:

```
content tree ──┐
               ├─► style tree ─► box tree ─► line layout ─► fragmentation ─► pages
CSS ───────────┘                                                               │
                                                                               ├─► display list (preview)
                                                                               └─► PDF (export)
```

- **`fleuron`** — style compilation, box construction, inline layout,
  fragmentation, page assembly. Pure library, no I/O.
- **`fleuron-cli`** — `fleuron` binary: content in, PDF out. Batch-friendly.
- **`fleuron-wasm`** — WASM bindings: layout in a worker, display list and
  PDF bytes out, zero DOM access.

### Invariants

1. **Styling enters as CSS.** A built-in user-agent stylesheet supplies
   the defaults; author CSS cascades over it. Everything downstream
   consumes the resolved style tree. The supported subset is written
   down in [`docs/css-subset.md`](docs/css-subset.md); anything outside
   it is reported with the line and column it was written at.
2. **The engine never touches the DOM.** Bytes in, bytes out. SVG, canvas,
   and PDF are interchangeable painters over the display list.
3. **Layout never decodes images.** Header probes yield intrinsic size,
   orientation, and DPI; painters decode pixels on their own side of the
   wall.

## Performance

The harness lays out two complete public-domain novels — *Pride and
Prejudice* at book scale and *The Count of Monte Cristo* at four times
it — and holds the result against fixed budgets:

| | pages | style | line layout | fragment | PDF | end to end | layout peak |
|---|---|---|---|---|---|---|---|
| *Pride and Prejudice* | 333 | 1 ms | 128 ms | 5 ms | 150 ms | 283 ms | 12 MiB |
| *The Count of Monte Cristo* | 1240 | 5 ms | 502 ms | 19 ms | 561 ms | 1.08 s | 46 MiB |

Four times the book costs about four times the time and four times the
memory.

Apple M-series, release build, best of three. Budgets: a book-scale
manuscript reaches PDF bytes in under a second natively, lays out in
under half a second in a WebAssembly worker, and holds under 32 MiB
while doing it. Criterion benches time each stage on its own; a gate
binary runs the same budgets natively and under wasm, and CI reports
both on every pull request.

```
cargo run --release -p fleuron-fixtures --bin perf-gate
cargo bench -p fleuron
```

## Status

Pre-alpha. Development happens in service of [Orca], the Obsidian
novel-writing suite — fleuron is its pagination backend, extracted. The
build order lives in the issue tracker; the first milestone that matters
is *fixture book in, valid PDF out*.

## License

Dual-licensed under either of [Apache-2.0](LICENSE-APACHE) or
[MIT](LICENSE-MIT) at your option.

[Orca]: https://github.com/zachhannum/obsidian-orca
[harfrust]: https://github.com/harfbuzz/harfrust
