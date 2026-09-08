---
title: Content tree
description: The engine's input contract, and the semantic document it lays out.
---

The content tree is a semantic document, not markup. Everything downstream consumes these types.

Most callers never build one. [Markdown](markdown.mdx) is the usual input, and the frontend produces this. The types are here for a host whose source is already structured, such as a CMS or a docx converter, which constructs a `Book` in Rust directly.

The tree serializes, internally tagged so the shape maps one-to-one onto [mdast](https://github.com/syntax-tree/mdast). That is an output only: `fleuron manuscript.md --dump-tree` reads back what the frontend did, and nothing parses one back into a `Book`.

## Shape

The dump, abridged:

```json
{
  "metadata": {
    "title": "Gulliver's Travels",
    "author": "Jonathan Swift",
    "extra": { "language": "en", "year": "1726" }
  },
  "sections": [
    {
      "source": "chapter-01.md",
      "title": "PART I. A VOYAGE TO LILLIPUT.",
      "blocks": [
        {
          "type": "heading",
          "level": 1,
          "inlines": [{ "type": "text", "value": "CHAPTER I." }],
          "attributes": { "id": "opening", "classes": ["grand"] }
        },
        {
          "type": "paragraph",
          "inlines": [
            { "type": "text", "value": "My father had a small estate in " },
            { "type": "emphasis", "children": [{ "type": "text", "value": "Nottinghamshire" }] },
            { "type": "text", "value": "." }
          ],
          "position": { "line": 7, "column": 1 },
          "span": { "start": 143, "end": 216 }
        }
      ]
    }
  ]
}
```

## `metadata`

`title` and `author` are the two the engine reads: the title for running heads, both for the PDF's document information. `extra` is a string map the frontend owns: language, ISBN, subtitle, anything. `language` is the one key layout reads, for the hyphenation patterns. The rest is opaque to the engine, and style may read it.

All three are optional. A book with no metadata lays out.

## `sections`

A section is a chapter or a file. It is the unit of markdown input and the unit of source attribution for diagnostics. Sections are in reading order, and a section starts a new page.

| field | |
|---|---|
| `source` | The file the frontend read, such as `chapter-01.md`. Diagnostics name it. |
| `title` | A title supplied outside the body, from frontmatter `title:`. Implies heading level 1. |
| `blocks` | The section's blocks, in reading order. |
| `position` | Where in `source` the section began. |
| `span` | The bytes of `source` the section was read from: the heading that opened it to the end of its last block. |

## Blocks

`type` is the tag. Every block takes an optional `position`, `span` and `attributes`.

| type | |
|---|---|
| `heading` | `level` is 1 to 6, the range markdown defines; a level outside that is rejected at parse. `inlines` is the heading's text. |
| `paragraph` | `inlines`. The unit line layout breaks. |
| `blockquote` | `blocks`, not inlines. Blockquotes nest. |
| `thematic_break` | `---`. A scene break, set as space or an ornament depending on the stylesheet. |
| `image` | `url` and `alt`. The string in `url` is the name the image is matched under, and it does not have to be a real URL, since the engine neither resolves it nor decodes the image. `alt` is not laid out, reaches painters and accessibility tools unchanged, and is not optional. |

## Inlines

| type | |
|---|---|
| `text` | `value`. Plain Unicode; the frontend has already decoded entities. |
| `emphasis` | `children`. Italic, in the built-in sheet. |
| `strong` | `children`. Bold, in the built-in sheet. |
| `code` | `value`. Literal, with no markup inside, monospace and never hyphenated. |
| `link` | `url` and `children`. The text lays out; the url reaches painters that can express one. |

Text runs are not elements as far as CSS is concerned. They take the style of the inline or block around them, and never count towards `:first-child`.

## Naming a node

`attributes` is what a sheet names one node by: `classes`, any number of them, and `id`, at most one. The names are as CSS spells them, without the `.` or the `#`. A node that carries neither leaves the field out.

```json
{ "attributes": { "id": "frontispiece", "classes": ["plate"] } }
```

Specificity counts them in buckets of their own: `.plate` outranks `img`, and `#frontispiece` outranks `.plate`.

The frontend reads them from an [attribute line](markdown.mdx); a host with a structured source of its own sets them on the tree it builds. An id two nodes carry warns naming both, and both still match.

A text run carries the field like every other node, and a sheet reaches nothing through it, because a text run is not an element.

## Node identity

Every node has an id, and the engine assigns it, so input cannot collide ids or forge a diagnostic origin. Ids are never serialized, so the ids in a tree built by hand are unassigned until `Book::assign_node_ids` numbers them from 1 in document order. Numbering is pre-order: a node before its children, sections in reading order.

Call it once, after building the tree. Calling it again renumbers. A [session](../library/sessions.md) numbers what it is handed, so content set through a session arrives numbered.

## Source positions

`position` is a 1-based line and column into the markdown the frontend read the node out of, exactly as its parser reported them. Paired with the section's `source`, it is what a diagnostic points at: `chapter-01.md:12:3`.

Positions are diagnostic data and never layout input, so a missing one never fails a run. A node with no position degrades to the bare file name, and a node with neither still warns, without a location.

## Where a node was read from

`span` is the other half: the bytes of `source` the node was read from, markup and all. A source and the text of the nodes read from it are different bytes, because markup is not text, so the span is the node's extent rather than a letter-by-letter map. A byte of the file lands on the node written there, not on a letter of it. The node that holds another was read from a stretch that holds its own, and the innermost nodes tile the file, so one byte is one node.

Two questions are answered from it, and they are the two halves of a cursor's way onto a page and back:

| | |
|---|---|
| `Book::node_at(source, byte)` | The node one byte of one source was read into, innermost first: a byte of prose answers with the run it was typed into, a byte of markup with the construct it opens, a byte between two blocks with the section around them. |
| `Book::source_of(node)` | The source a node was read from, and the bytes of it. |

A cursor becomes a node, and the runs of the [display structure](display-structure.mdx) that name that node are on the page it is set on. A run under the pointer goes the other way. Only the sections read from the source asked about are looked at, so one file's cursor is answered by one file's nodes.

Both answers are about the book as it stands. Ids renumber whenever the book is set or one of its sources replaced, so a host that holds one across an edit asks again rather than reusing it.

A node the engine synthesized, or one from a tree built rather than parsed, was read from nothing, and both questions answer with nothing rather than guessing.
