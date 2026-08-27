---
title: Sessions
description: A retained pipeline for live preview, re-running only the stages an edit invalidates.
---

`layout_book` rebuilds every stage on every call. A program that renders one book and exits wants that. A live preview does not: the common event there is a small change to one input while the others stand still, and rebuilding everything makes every drag of a font-size slider cost a book-scale line-breaking pass.

A `Session` keeps the output of each stage and works out which stages an edit invalidates.

```rust
use fleuron::session::Session;
use fleuron::style::{Source, Stylesheets};

let mut session = Session::new(&registry);
session.set_content(book);
session.set_style(Stylesheets::parse(&[Source::author("book.css", &css)]));

let output = session.preview();      // the display list
let bytes = session.export()?;       // the same run, as PDF
```

`preview` and `export` are two painters over one set of stages, so an export cannot contradict the preview it came from. Both bring the session up to date first, and neither re-runs a stage the last edit left standing.

## What an edit costs

| change | deepest surviving cache | what runs |
|---|---|---|
| a property the engine models nothing of | the display list | nothing |
| margin box content, the face a folio is set in | the page boxes | the furniture |
| `@page` geometry, counters, named pages | the lines | fragmentation, then the furniture |
| face, size, measure, leading | the style tree | line breaking, and everything under it |
| one file's content | every other section's lines | that file's sections, then fragmentation |
| the whole book | nothing | all of it |

The middle rows are where the saving is. On a 333-page manuscript, line breaking takes around 130 ms and fragmentation around 5 ms, so a sheet that only moves the page box costs the second number instead of the first.

`Session::stages()` reports how many times each stage has run, so a host or a test can see what an edit cost without timing it.

## Editing content

`set_content` replaces the book. `replace_source(name, sections)` replaces every section that came from one file. One markdown file may split into several sections, and they all go together. A name the book does not already have appends instead, which is how a new file arrives.

```rust
use fleuron_markdown::{Options, to_sections};

let reading = Options::default();
let text = std::fs::read_to_string("ch03.md")?;
let (sections, warnings) = to_sections(&text, "ch03.md", &reading);

session.replace_source("ch03.md", sections);
let output = session.preview();
```

Pass `replace_source` the same name the sections were read under, since that is what each one carries as its `source`. Pass a different one and the sections append as a new file instead of replacing the old.

`set_source_warnings` carries the frontend's complaints into the run's diagnostics, so what the markdown reader said about a table comes back beside what styling and layout said. The session replaces the whole set; deciding which still apply after an edit is the caller's job.

Nothing re-reads the files that did not change. `fleuron_markdown::Cache` holds each source's sections against its name and a hash of its bytes, answering the same question as `replace_source` one layer down.

Node ids belong to the engine. The tree is renumbered on the way in, so sections built by hand need no ids of their own, and nothing downstream is keyed on an id that renumbering will move.

A content edit re-breaks only the sections it changed. The rest keep their lines, and the whole book is fragmented from the top, which costs less than working out which pages moved. Page assembly resolves counters, recto opens, running heads and blank leaves, so it has to run anyway.

## What is in the cache

Breaks, shaped glyph runs and advance widths, and no coordinates at all. Where a line breaks depends on the measure, the face and the text. Which page it lands on and at what baseline is fragmentation's answer, and fragmentation is asked again every time. A chapter that an edit above it pushed onto a different page paints at new coordinates with the same breaks.

Two preconditions make that sound, and the session checks both.

The first is a single measure. Masters with different content widths, such as asymmetric `@page :left` and `@page :right` margins or a named master set narrower, make where a line breaks depend on which page it lands on, and that depends on everything before it. Mirrored margins are not this case: the built-in sheet mirrors the spine margin across the spread and both sides come to the same measure.

The second is that no prose depends on pagination. `counter(page)` and `string()` are legal only inside a margin box, so nothing in the text can depend on where the text fell. `target-counter()` would end that. An index, or a table of contents with real page numbers, makes inline text depend on pagination and pagination on breaking, and that has no fixed point a cache can serve.

When either precondition fails, `reuses_sections()` goes false and every edit re-breaks the whole book.

## Memory

Every stage stays live at once: the lines of every section, beside the display list they were flowed into. A one-shot `layout_book` keeps one section's lines at a time and drops each as it is flowed, so the two get separate memory ceilings in the perf harness. On the gate book, a throwaway pass peaks around 12 MiB and a session sits at around 29 MiB.

## Fonts

A session over a borrowed registry lays out against the faces that registry holds, and a computed style can only resolve to one of them. A sheet that brings its own `@font-face` needs the host to register it before the session is made.

`Session::owning` takes the registry instead of borrowing it, and then `add_font` works. Faces arrive through the session, which re-runs what a new face can change: the cascade resolves a family it did not have before, so the styling is compiled again and the lines are broken again. A worker needs this, since the module is the only place a registry could live. See [fonts](fonts.md).

## Images

The asset table works the same way. `Session::with_assets` borrows a table the host probed with an `ImageLoader`; `Session::owning` keeps one of its own and `add_image` fills it, which is how images cross into a worker that has nothing to open.

An image that arrives is a box that was not reserved before it, so the sections are broken again. A url the session already holds costs nothing. A url the manuscript names and nobody supplies is a warning and a gap on the page.
