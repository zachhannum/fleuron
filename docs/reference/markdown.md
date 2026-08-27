---
title: Markdown mapping
description: What each markdown construct becomes in the content tree, and which of them warn.
---

The frontend reads markdown into the [content tree](content-tree.md). The mapping below is the whole of it: every decision here moves a page count, so it is written down rather than inferred from the code.

```rust
let (sections, warnings) = fleuron_markdown::to_sections(&text, "chapter-01.md", &Options::default());
let book = fleuron_markdown::assemble(fleuron_markdown::frontmatter(&text), sections);
```

The primitive is per-source, not per-book. One source yields one or more sections, because both directions are ordinary: a novel may arrive as one file that has to become sixty chapters, or as sixty files that each become one. Composing sources into a book is `assemble`, a step of its own, so the caller orders them and decides the metadata.

## Sections

`Options::sections` says where a section begins, and a section is what the fragmenter opens a page on.

| policy | |
|---|---|
| `Sections::AtHeading(level)` | A heading at that level or shallower opens a section. `AtHeading(H2)` cuts at `#` and `##` alike. |
| `Sections::Whole` | The source is one section. A file per chapter. |

Content before the first heading gets a section of its own rather than being folded into the one after it.

The two arrangements are the same book. A single file split at its chapter headings and one file per chapter, read whole, give the same tree, section for section and block for block. What differs is each section's `source`, and the positions inside it, which count from the top of whichever file the prose was read from.

## Metadata

A source's frontmatter belongs to that source, and what it means depends on whether the source is the book or a chapter of one.

`frontmatter` reads the `---` block at the top of a source. `title` and `author` are the named fields; every other scalar joins `extra`, which the engine carries and style may read. Values are scalars: a line that is not `key: value` is not metadata.

```markdown
---
title: Pride and Prejudice
author: Jane Austen
year: 1813
---
```

A source read whole is one chapter, so its `title:` becomes that section's `title` rather than the book's. Nothing lays a section title out today; it is carried through the tree for whatever does.

That leaves book metadata for the caller. `assemble(metadata, sections)` takes it as an argument, so a library or WASM host passes whatever it already knows and the CLI passes what its flags named.

Sixty chapter files have sixty frontmatter blocks and none of them describes the work. Reading the book's title out of whichever file came first is how a chapter ends up naming the book, so the frontend does not do it.

A book with no metadata lays out. The engine reads three fields: `title` and `author` reach the PDF's document information, and so does `extra["language"]`. Everything else in `extra` is carried for whoever wants it.

## Blocks

| markdown | content tree |
|---|---|
| `#` … `######` | `heading`, at that level |
| a paragraph | `paragraph` |
| `> …` | `blockquote`, nesting |
| `---` | `thematic_break` |
| `![alt](url)` | `image`, as a block |

An image is a block in the vocabulary and inline in markdown, so one written inside a paragraph becomes a block directly after it, alt text and all. That is a move, so it warns.

## Inlines

| markdown | content tree |
|---|---|
| text | `text`, entities decoded |
| `*em*` | `emphasis` |
| `**strong**` | `strong` |
| `` `code` `` | `code` |
| `[text](url)` | `link` |

A line wrapped in the source is a space in the tree; the shaper never sees the markdown's ragged column.

## What warns

These constructs have no counterpart in the content tree. The frontend sets them as prose and says where through the diagnostics channel, naming the file, line and column. Prose is never dropped, because a manuscript that quietly loses a paragraph is worse than one that warns about a table.

| construct | becomes |
|---|---|
| a list | one paragraph per item |
| a table | one paragraph per cell |
| a code block | a paragraph |
| a definition list | one paragraph per entry |
| a footnote | prose where the definition was written |
| strikethrough, superscript, subscript | plain text |
| math | plain text |
| an inline image | a block of its own |
| html, a footnote reference, a task marker | nothing; they carry no prose |

## Dialects

`Options::dialect` is a set of switches, so a host's departures from CommonMark are configuration rather than a second mapping to keep in step with this one.

| switch | |
|---|---|
| `frontmatter` | A leading `---` block is metadata rather than a scene break. On by default. |
| `gfm` | Tables, strikethrough, task lists. |
| `wikilinks` | `[[wikilinks]]`, which become links. |
| `smart_punctuation` | Dashes and curly quotes at parse rather than in the manuscript. |

`Dialect::common_mark()`, `Dialect::gfm()` and `Dialect::obsidian()` are the three combinations worth naming.

A switch that is off does not make its syntax an error: it makes it prose. `[[Another Note]]` without `wikilinks` is four brackets and a title.

## Reading a source twice

`Cache` holds a source's sections against its name and a hash of its bytes, which is what a host re-rendering on every keystroke wants. The key is deliberately not a node id: ids are assigned in document order over the whole book and renumber whenever a section is added or removed, so a cache keyed on one would miss on every edit while appearing to work.

Parsing is deterministic, so two readings of the same bytes give the same tree, and a page count that moves is a change somebody made.
