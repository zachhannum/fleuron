---
title: Content tree
description: The engine's input contract, and the semantic document it lays out.
---

The content tree is a semantic document, not markup. Everything downstream consumes these types, and nothing widens the vocabulary without a fixture and a test.

Most callers never build one. [Markdown](markdown.mdx) is the usual input, and the frontend produces this. The types are here for a host whose source is already structured, such as a CMS or a docx converter: it constructs a `Book` in Rust, typed and checked by the compiler, with no schema document to keep in step.

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
          "inlines": [{ "type": "text", "value": "CHAPTER I." }]
        },
        {
          "type": "paragraph",
          "inlines": [
            { "type": "text", "value": "My father had a small estate in " },
            { "type": "emphasis", "children": [{ "type": "text", "value": "Nottinghamshire" }] },
            { "type": "text", "value": "." }
          ],
          "position": { "line": 7, "column": 1 }
        }
      ]
    }
  ]
}
```

## `metadata`

`title` and `author` are the two the engine reads: the title for running heads, both for the PDF's document information. `extra` is a string map the frontend owns, holding language, ISBN, subtitle, anything. It is opaque to the engine; style may read it, layout does not.

All three are optional. A book with no metadata lays out.

## `sections`

A section is a chapter or a file. It is the unit of markdown input and the unit of source attribution for diagnostics. Sections are in reading order, and a section starts a new page.

| field | |
|---|---|
| `source` | The file the frontend read, such as `chapter-01.md`. Diagnostics name it. |
| `title` | A title supplied outside the body, from frontmatter `title:`. Implies heading level 1. |
| `blocks` | The section's blocks, in reading order. |
| `position` | Where in `source` the section began. |

## Blocks

`type` is the tag. Every block takes an optional `position`.

| type | |
|---|---|
| `heading` | `level` is 1 to 6, as markdown has them; a level outside that is rejected at parse. `inlines` is the heading's text. |
| `paragraph` | `inlines`. The unit line layout breaks. |
| `blockquote` | `blocks`, not inlines. Blockquotes nest. |
| `thematic_break` | `---`. A scene break, set as space or an ornament depending on the stylesheet. |
| `image` | `url` and `alt`. Whatever string you write is the name the image is matched under, and it never has to be a real URL: the engine neither resolves it nor decodes the image. `alt` is not laid out, is carried through for painters and for accessibility, and is not optional. |

## Inlines

| type | |
|---|---|
| `text` | `value`. Plain Unicode; the frontend has already decoded entities. |
| `emphasis` | `children`. Italic, in the built-in sheet. |
| `strong` | `children`. Bold, in the built-in sheet. |
| `code` | `value`. Literal, with no markup inside, monospace and never hyphenated. |
| `link` | `url` and `children`. The text lays out; the url is carried for painters that can express one. |

Text runs are not elements as far as CSS is concerned. They take the style of the inline or block around them, and never count towards `:first-child`.

## Node identity

Every node has an id, and the engine assigns it, so input cannot collide ids or forge a diagnostic origin. Ids are never serialized, so a tree built by hand holds unassigned ids until `Book::assign_node_ids` numbers them from 1 in document order. Numbering is pre-order: a node before its children, sections in reading order.

Call it once, after building the tree. Calling it again renumbers. A [session](../library/sessions.md) numbers what it is handed, so content set through one arrives numbered whatever the host did.

## Source positions

`position` is a 1-based line and column into the markdown the frontend read the node out of, exactly as its parser reported them. Paired with the section's `source`, it is what a diagnostic points at: `chapter-01.md:12:3`.

It is diagnostic data and never layout input. A missing position never fails a run. It degrades to the bare file name, and a node with neither still warns, without a location.
