---
title: Sessions
description: A retained pipeline for live preview, re-running only the stages an edit invalidates.
---

Sessions in fleuron are useful when you want to render and re-render a book multiple times,
while only changing parts of the input: some of the prose or CSS rules. `layout_book`
rebuilds every stage on every call. This is useful for a program like the CLI that renders
and outputs the book once, but a live preview benefits from a session that keeps state in between renders.

A `Session` keeps the output of each stage and works out which stages an edit invalidates.

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

| change                                  | deepest surviving cache     | what runs                                |
| --------------------------------------- | --------------------------- | ---------------------------------------- |
| a property the engine models nothing of | the display structure       | nothing                                  |
| margin box content, page counters       | the page boxes              | the furniture                            |
| `@page` geometry, counters, named pages | the lines                   | fragmentation, then the furniture        |
| face, size, measure, leading            | the style tree              | line breaking, and everything under it   |
| one file's content                      | every other section's lines | that file's sections, then fragmentation |
| the whole book                          | nothing                     | all of it                                |

`Session::stages()` reports how many times each stage has run, so a host or a test can see what an edit cost without timing it.

## Editing content

`set_content` replaces the book. `replace_source(name, sections)` replaces every section that came from one file. 
One markdown file may split into several sections, and they all go together. A name the book does not already have 
appends instead.

```rust
use fleuron_markdown::{Options, to_sections};

let reading = Options::default();
let text = std::fs::read_to_string("ch03.md")?;
let (sections, warnings) = to_sections(&text, "ch03.md", &reading);

session.replace_source("ch03.md", sections);
let output = session.preview();
```

Nothing re-reads the files that did not change. `fleuron_markdown::Cache` stores each source's sections 
against its name and a hash of its bytes.

Node ids belong to the engine. The tree is renumbered on the way in, so sections built by hand need no ids of their own, 
and nothing downstream is keyed on an id that renumbering will move.

A content edit re-breaks only the sections it changed. The rest keep their lines, 
and the whole book is fragmented from the top. Page assembly then resolves counters, recto opens, 
running heads and blank leaves.

## What is in the cache

Breaks, shaped glyph runs and advance widths, and no coordinates at all. Where a line breaks 
depends on the measure, the font and the text. Which page it lands on and at what baseline is 
determined by fragmentation, and fragmentation runs every time. A chapter that an edit above it 
pushed onto a different page paints at new coordinates with the same breaks.

The session checks two preconditions to determine this.

The first is a single measure. Masters with different content widths, such as asymmetric `@page :left` and `@page :right` 
margins or a named master set narrower, make where a line breaks depend on which page it lands on, 
and that depends on everything before it. Mirrored margins are not this case: the built-in sheet 
mirrors the spine margin across the spread and both sides come to the same measure.

The second is that no prose depends on pagination. `counter(page)` and `string()` 
are legal only inside a margin box, so nothing in the text can depend on where the text fell. 
An index, or a table of contents with real page numbers, makes inline text depend on pagination and pagination on breaking, 
and that has no fixed point a cache can serve.

When either precondition fails, `reuses_sections()` becomes false and every edit re-breaks the whole book.
