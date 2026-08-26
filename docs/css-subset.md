---
title: The novel subset
description: Every property, at-rule and selector the engine honours, and what it does with the rest.
---

fleuron understands the CSS a book needs and reports the rest. Every
property, at-rule and selector below is honoured by the pipeline;
anything outside them becomes a warning naming the line and column it
was written at, and the run continues.

## Selectors

Type, universal, descendant, child, adjacent and general sibling, the
structural pseudo-classes (`:first-child`, `:nth-child()`, `:only-child`,
`:empty`, `:root`), `:is()`, `:where()`, `:not()` and `:has()`.

Element names are the markdown vocabulary: `book`, `section`, `h1`–`h6`,
`p`, `blockquote`, `hr`, `img`, `em`, `strong`, `code`, `a`. There are no
classes, ids, attributes or namespaces — the content tree carries none —
and no pseudo-classes that describe a document being interacted with.

Text runs are not elements: they take the style of the inline or block
around them, and never count towards `:first-child`.

## Properties

Text, inherited:

| property | values |
|---|---|
| `font-family` | family names and `serif` / `sans-serif` / `monospace` |
| `font-size` | `<length>`, `<percentage>` of the parent |
| `font-style` | `normal`, `italic`, `oblique` |
| `font-weight` | `normal`, `bold`, `1`–`1000` |
| `line-height` | `normal`, `<number>`, `<length>`, `<percentage>` |
| `text-align` | `left`, `right`, `center`, `justify`, `start`, `end` |
| `text-indent` | `<length>` |
| `hyphens` | `none`, `manual`, `auto` |
| `orphans`, `widows` | `<integer>` |
| `page` | a page name, or `auto` |

Block box, not inherited:

| property | values |
|---|---|
| `margin` | one to four `<length>`s |
| `margin-top`, `margin-right`, `margin-bottom`, `margin-left` | `<length>` |
| `break-before`, `break-after`, `break-inside` | `auto`, `avoid`, `page`, `left`, `right`, `recto`, `verso` |

`recto` and `verso` are the book's names for `right` and `left`.

Lengths are `pt`, `px`, `pc`, `in`, `cm`, `mm`, `q`, `em`, `rem`.
Everything computes to points. `em` in `font-size` is a multiple of the
parent's size and elsewhere of the element's own; `rem` is always a
multiple of the root's.

## `@page`

```css
@page <name>? [:first | :blank | :left | :right]* {
  size: <length>{1,2} | a3 | a4 | a5 | b4 | b5 | letter | legal | ledger [portrait | landscape]?;
  margin: ...;
  @<margin-box> { content: none | counter(page) | <string>; /* text properties */ }
}
```

A page group begins wherever a section opens a page, so `:first` selects
a chapter's opening page rather than only the book's. `:blank` selects a
page inserted to square the sheet. `:left` and `:right` are verso and
recto.

Page selectors cascade like any other rule: origin first, then
specificity — the name outweighs `:first` and `:blank`, which outweigh
`:left` and `:right` — then source order.

All sixteen margin boxes parse. Six paint: `@top-left`, `@top-center`,
`@top-right` and their `@bottom-` counterparts. A `-center` box is
centred on the trim rather than on the content box, because a folio
belongs on the page's axis and mirrored margins put the content box off
it; `-left` and `-right` align to the content box's edges.

## `@font-face`

```css
@font-face {
  font-family: "Author Serif";
  src: url("faces/author.ttf") format("truetype"), local("Author Serif");
  font-style: normal;
  font-weight: 400;
}
```

`src` urls mean whatever the host says they mean: the engine hands each
one to the loader it was given and reads no path of its own. A face that
resolves is registered under the family the sheet declared, not the one
inside the file. A face that resolves nowhere is a warning, and text
falls back to the next family in the list.

A variable file registers one face per named instance, so a family
declared once holds every cut the file names. Declaring `font-style` or
`font-weight` overrides that: the sheet has said what this source is,
and it registers as that one cut.

## Font matching

`font-family` is tried in order and the first registered family answers.
Within it, slope decides before weight: a family with an italic cut
never answers an italic request with the upright one. Weight then
follows CSS, which looks up before it looks down between 400 and 500 and
away from that range elsewhere.

Nothing is synthesised. A family with no italic cut lays out upright and
says so, and a stack that resolves nothing falls back to the first
registered face and says that.

## Not in the subset

Colour, backgrounds, borders, padding, floats, tables, grid, flexbox,
transforms, media queries, custom properties, counters other than
`page`, and generated content beyond page margin boxes. The engine says
so, per declaration, where it was written.
