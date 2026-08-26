---
title: Sessions
description: A retained pipeline for live preview — only the stages an edit invalidates run again.
---

`layout_book` is a pure function: every call rebuilds every stage. That is what a process rendering one book and exiting wants, and the wrong shape for a live preview, where the common event is a small change to one input while the others stand. Dragging a font-size slider through a full pipeline costs a book-scale line-breaking pass per frame.

A `Session` holds each stage's output and classifies an edit into the deepest stage it actually reaches.

```rust
use fleuron::session::Session;
use fleuron::style::{Source, Stylesheets};

let mut session = Session::new(&registry);
session.set_content(book);
session.set_style(Stylesheets::parse(&[Source::author("book.css", &css)]));

let output = session.preview();      // the display list
let bytes = session.export()?;       // the same run, as PDF
```

`preview` and `export` are painters over one set of stages, so an export can never contradict the preview it came from. Both bring the session up to date first; neither re-runs a stage the last edit left standing.

## What an edit costs

| change | deepest surviving cache | what runs |
|---|---|---|
| a property the engine models nothing of | the display list | nothing |
| margin box content, the face a folio is set in | the page boxes | the furniture |
| `@page` geometry, counters, named pages | the lines | fragmentation, then the furniture |
| face, size, measure, leading | the style tree | line breaking, and everything under it |
| content | nothing | all of it |

The middle rows are the ones that pay. On a 333-page manuscript, line breaking is around 130 ms and fragmentation around 5 ms, so a sheet that only moves the page box costs the second number rather than the first.

`Session::stages()` reports how many times each stage has run, which is how a host — or a test — sees what an edit cost without timing it.

## Editing content

`set_content` replaces the book. `replace_source(name, sections)` replaces every section that came from one file, which is the unit a host names: a markdown file may split into several sections, and all of them go together. A name the book does not carry appends, which is how a new file arrives.

Node ids are the engine's. The tree is renumbered on the way in, so sections built by hand need no ids of their own, and nothing downstream is keyed on an id that renumbering will move.

Only the sections a content edit actually changed are broken again. The rest keep their lines and are fragmented from the top, because fragmentation over a whole novel costs less than working out which pages moved — and page assembly is where counters, recto opens, running heads and blank leaves are resolved anyway.

## What the cache holds

Breaks, shaped glyph runs and advance widths. No coordinates. Where a line breaks depends on the measure, the face and the text; which page it lands on and at what baseline is fragmentation's answer, and it is asked again every time. A chapter that an edit above it pushed onto a different page paints at new coordinates with the same breaks.

Two preconditions make that sound, and the session checks both rather than trusting them:

**One measure.** Masters with different content widths — asymmetric `@page :left` and `@page :right` margins, a named master set narrower — make where a line breaks depend on which page it lands on, and that on everything before it. Mirrored margins are not this: the built-in sheet mirrors the spine margin across the spread and both sides come to the same measure.

**No pagination-dependent prose.** `counter(page)` and `string()` are legal only inside a margin box, so nothing in the text depends on where the text fell. `target-counter()` would end that — an index, or a table of contents with real page numbers, makes inline text depend on pagination and pagination on breaking, which is a fixpoint rather than a cache invalidation.

When either fails, `reuses_sections()` goes false and every edit re-breaks the whole book rather than serve a line broken against a measure it may not land on.

## What a session costs to hold

Every stage at once: the lines of every section, next to the display list they were flowed into. A one-shot `layout_book` holds one section's lines at a time and drops them as they are flowed, which is why the two carry separate memory ceilings in the perf harness. On the gate book a throwaway pass peaks around 12 MiB and a session holds around 29 MiB.

## Fonts

The registry is fixed for the session's life. A face the registry does not hold is a face no computed style can resolve to, so a sheet bringing its own `@font-face` needs the host to register it before the sheet is set. See [fonts](fonts.md).
