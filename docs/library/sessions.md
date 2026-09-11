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
| face, size, line width, leading         | the style tree              | line breaking, and everything under it   |
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
running heads and blank pages.

## Which pages a node is on

`Session::folios(nodes)` answers where each node's content is set, one answer per node, in the order asked about.

A new face repaginates the book. The chapter that opened on page 41 opens on 38, and a reader who was looking at page 41 is now looking at words that were somewhere else a moment ago. This is the question a host asks to put the reader back, to turn to a chapter, or to name the chapter on screen.

An answer names four numbers, because what is printed on a page and where that page falls in the book are two different things. `counter-reset: page` restarts the count, so page 1 of a chapter can be the fortieth page of the book.

| | |
|---|---|
| `first`, `last` | The folios the content runs between, as printed. This is what a host puts on screen. |
| `at`, `count` | Where those pages fall in the book, counting from 0. These are the numbers a host fetches the pages by. |

The following example prints the page range of every chapter of a book:

```rust
let chapters: Vec<NodeId> = session.book().sections.iter().map(|section| section.id).collect();
for (chapter, folios) in chapters.iter().zip(session.folios(&chapters)) {
    if let Some(folios) = folios {
        println!("node {} runs from page {} to {}", chapter.get(), folios.first, folios.last);
    }
}
```

A node covers itself and everything under it. A heading's text is a node inside the heading, so the heading answers with the page that text is on, and a chapter answers with the pages it runs across.

The answer is nothing for a node the book does not hold, for one the engine synthesized, and for one whose content reaches no page.

The answer is a walk over the pages the session already holds. Asking runs a stage only when an edit has left one to run.

## What is in the cache

Breaks, shaped glyph runs and advance widths, and no coordinates at all. Where a line breaks 
depends on the line width, the font and the text. Which page it lands on and at what baseline is 
determined by fragmentation, and fragmentation runs every time. A chapter that an edit above it 
pushed onto a different page paints at new coordinates with the same breaks.

The session checks two preconditions to determine this.

The first is a single line width. Masters with different line widths make where a line breaks depend on which page it 
lands on, and that depends on everything before it. Asymmetric `@page :left` and `@page :right` margins are this 
case, so is a named master set narrower, and so is one that divides its content box into a different number of 
columns. Mirrored margins are not: the built-in sheet mirrors the spine margin across the spread and both sides 
come to the same line width. The lines of a block with `column-span: all` break to the width of the whole content 
box. If a book has such a block, every master also needs the same content box width.

The second is that no prose depends on pagination. `counter(page)` and `string()` 
are legal only inside a margin box, so nothing in the text can depend on where the text fell. 
An index, or a table of contents with real page numbers, makes inline text depend on pagination and pagination on breaking, 
and that has no fixed point a cache can serve.

When either precondition fails, `reuses_sections()` becomes false and every edit re-breaks the whole book.
