---
title: Sessions
description: A retained pipeline for live preview, re-running only the stages an edit invalidates.
---

`layout_book` is a pure function: every call rebuilds every stage. A program that renders one book and exits wants exactly that. A live preview does not, because the common event there is a small change to one input while the others stand still, and rebuilding everything makes every drag of a font-size slider cost a book-scale line-breaking pass.

A `Session` holds each stage's output and works out which stages an edit invalidates.

```rust
use fleuron::session::Session;
use fleuron::style::{Source, Stylesheets};

let mut session = Session::new(&registry);
session.set_content(book);
session.set_style(Stylesheets::parse(&[Source::author("book.css", &css)]));

let output = session.preview();      // the display list
let bytes = session.export()?;       // the same run, as PDF
```

`preview` and `export` are painters over one set of stages, so an export cannot contradict the preview it came from. Both bring the session up to date first, and neither re-runs a stage the last edit left standing.

## What an edit costs

| change | deepest surviving cache | what runs |
|---|---|---|
| a property the engine models nothing of | the display list | nothing |
| margin box content, the face a folio is set in | the page boxes | the furniture |
| `@page` geometry, counters, named pages | the lines | fragmentation, then the furniture |
| face, size, measure, leading | the style tree | line breaking, and everything under it |
| content | nothing | all of it |

The middle rows are where the saving is. On a 333-page manuscript, line breaking takes around 130 ms and fragmentation around 5 ms, so a sheet that only moves the page box costs the second number instead of the first.

`Session::stages()` reports how many times each stage has run, so a host (or a test) can see what an edit cost without timing it.

## Editing content

`set_content` replaces the book. `replace_source(name, sections)` replaces every section that came from one file, which is the unit a host names: one markdown file may split into several sections, and all of them go together. A name the book does not carry appends instead, which is how a new file arrives.

Node ids belong to the engine. The tree is renumbered on the way in, so sections built by hand need no ids of their own, and nothing downstream is keyed on an id that renumbering will move.

The session breaks only the sections a content edit changed. The rest keep their lines, and the whole book is fragmented from the top, which costs less than working out which pages moved. Page assembly is also where counters, recto opens, running heads and blank leaves are resolved, so it has to run anyway.

## What the cache holds

Breaks, shaped glyph runs and advance widths, and no coordinates at all. Where a line breaks depends on the measure, the face and the text. Which page it lands on and at what baseline is fragmentation's answer, and fragmentation is asked again every time. So a chapter that an edit above it pushed onto a different page paints at new coordinates with the same breaks.

Two preconditions make that sound, and the session checks both rather than trusting them.

The first is a single measure. Masters with different content widths, such as asymmetric `@page :left` and `@page :right` margins or a named master set narrower, make where a line breaks depend on which page it lands on, and that depends on everything before it. Mirrored margins are not this case: the built-in sheet mirrors the spine margin across the spread, and both sides come to the same measure.

The second is that no prose depends on pagination. `counter(page)` and `string()` are legal only inside a margin box, so nothing in the text can depend on where the text fell. `target-counter()` would end that. An index, or a table of contents with real page numbers, makes inline text depend on pagination and pagination on breaking, which is a fixpoint rather than a cache invalidation.

When either precondition fails, `reuses_sections()` goes false and every edit re-breaks the whole book instead of serving a line broken against a measure it may not land on.

## What a session costs to hold

A session holds every stage at once: the lines of every section, next to the display list they were flowed into. A one-shot `layout_book` holds one section's lines at a time and drops them as they are flowed, so the two carry separate memory ceilings in the perf harness. On the gate book a throwaway pass peaks around 12 MiB and a session holds around 29 MiB.

## Fonts

The registry is fixed for the session's life. A computed style can only resolve to a face the registry already holds, so a sheet that brings its own `@font-face` needs the host to register it before the session is made. See [fonts](fonts.md).
