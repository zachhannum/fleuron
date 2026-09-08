---
title: Sessions
description: A retained pipeline for live preview, re-running only the stages an edit invalidates.
---

A session lays the same book out over and over without paying for the whole book each time. `layout_book` rebuilds every stage on every call, which is what a program like the CLI wants, since it sets the book once. A live preview sets it again at every keystroke, and a `Session` keeps the output of each stage between renders and works out which stages an edit invalidates.

The following program opens a session over a book and a stylesheet, then takes both a preview and a PDF from it:

```rust
use fleuron::session::Session;
use fleuron::style::{Source, Stylesheets};

let mut session = Session::new(&registry);
session.set_content(book);
session.set_style(Stylesheets::parse(&[Source::author("book.css", &css)]));

let output = session.preview();      // the display structure
let bytes = session.export()?;       // the same run, as PDF
```

`preview` and `export` are two painters over one set of stages, so an export cannot contradict the preview it came from.

## What an edit costs

| change | deepest surviving cache | what runs |
| --- | --- | --- |
| a property the engine models nothing of | the display structure | nothing |
| margin box content, page counters | the page boxes | the margin boxes |
| `@page` geometry, counters, named pages | the lines | fragmentation, then the margin boxes |
| font, size, line width, line height | the style tree | line breaking, and everything under it |
| one file's content | every other section's lines | that file's sections, then fragmentation |
| the whole book | nothing | all of it |

`Session::stages()` reports how many times each stage has run, so a host or a test can see what an edit cost without timing it.

## Editing content

`set_content` replaces the book. `replace_source(name, sections)` replaces every section that came from one file, since one markdown file may split into several sections and they all go together. A name the book does not already have is appended instead.

The following program re-reads one chapter and lays the book out again:

```rust
use fleuron_markdown::{Options, to_sections};

let reading = Options::default();
let text = std::fs::read_to_string("ch03.md")?;
let (sections, warnings) = to_sections(&text, "ch03.md", &reading);

session.replace_source("ch03.md", sections);
let output = session.preview();
```

Nothing re-reads the files that did not change. `fleuron_markdown::Cache` stores each source's sections against its name and a hash of its bytes.

Node ids belong to the engine. The tree is renumbered on the way in, so sections built by hand need no ids of their own, and nothing downstream is keyed on an id that renumbering will move.

A content edit re-breaks only the sections it changed, and the rest keep the lines they already have. The whole book is then fragmented from the top, which resolves the counters, the chapters that open on a right-hand page, the running heads, and the blank pages between chapters.

## What is in the cache

The cache holds line breaks, shaped glyph runs, and advance widths. It holds no coordinates at all. Where a line breaks depends on the line width, the font and the text, and none of those depend on pagination. Which page a line lands on and at what baseline is fragmentation's answer, and fragmentation runs every time. A chapter that an edit above it pushed onto a different page is painted at new coordinates from the same breaks.

Two conditions have to hold for those breaks to be reusable, and the session checks both.

The first is that the book has one line width. Page rules with different widths make where a line breaks depend on which page it lands on, and that depends on everything before it. Asymmetric `@page :left` and `@page :right` margins are that case, so is a named page set narrower, and so is one that divides its content box into a different number of columns. Mirrored margins are not: the built-in sheet mirrors the spine margin across the spread, so both sides come to the same width.

The second is that no prose depends on pagination. `counter(page)` and `string()` are legal only inside a margin box, so nothing in the text can depend on where the text fell. An index, or a table of contents with real page numbers, would make inline text depend on pagination and pagination on line breaking, and that has no fixed point a cache can serve.

When either condition fails, `reuses_sections()` is false and every edit re-breaks the whole book.
