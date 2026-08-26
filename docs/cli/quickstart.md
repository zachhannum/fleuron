---
title: CLI quickstart
description: Markdown in, PDF out, in one command.
---

```sh
git clone https://github.com/zachhannum/fleuron
cd fleuron
cargo run --release -p fleuron-cli -- fixtures/gulliver-excerpt.md -o book.pdf -c fixtures/styled.css
```

```text
fleuron: fixtures/gulliver-excerpt.md → book.pdf: 34 pages
```

That is an excerpt of *Gulliver's Travels* — dialogue, em-dashes, hyphenation-prone words — laid out at 5.5×8.5 inches with mirrored margins, a folio in the bottom margin, and a running head on each chapter's opening page. The stylesheet doing all of that is `fixtures/styled.css`, and it is 47 lines.

## Without the clone

```sh
cargo install --git https://github.com/zachhannum/fleuron fleuron-cli
fleuron manuscript.md -o book.pdf
```

With no `-c`, the built-in stylesheet is the whole of the styling: a trade paperback, justified, chapters opening recto. It is a finished design, and a book that never overrides it still comes out looking like a book.

## Several files

Markdown files compose in the order the command line gives them:

```sh
fleuron front.md ch01.md ch02.md ch03.md -o book.pdf
```

Each file carries its own name into the tree, so a diagnostic points at the file the trouble is in rather than at the run. Metadata is read from each file's frontmatter, and the first file to set a field keeps it. A later one that disagrees says so and is ignored.

## Where sections begin

A section is what the fragmenter opens a page on, so where they begin decides the page count before any styling does. `--split` names the rule:

```sh
fleuron manuscript.md -o book.pdf --split 2
```

A heading at level 2 or shallower opens a section, which is the shape of a manuscript that sets parts with `#` and chapters with `##`. The default is `--split 1`. `--split none` opens no section at a heading at all, which is what a vault of one chapter per file wants: the file is the section.

`--dialect` says which markdown is being read: `commonmark`, `gfm` or `obsidian`. [The markdown mapping](../reference/markdown.md) is what each construct becomes, and which of them warn.

## Adding your own styling

Stylesheets cascade in the order the command line gives them, over the built-in sheet rather than instead of it:

```sh
fleuron manuscript.md -o book.pdf -c house.css -c series.css
```

So `house.css` sets the press's defaults and `series.css` overrides the handful of things this series does differently. Author CSS outranks the built-in sheet whatever the specificity of the built-in rule, so you never have to out-specify a default you are trying to replace.

```css
@page {
  size: 5.5in 8.5in;
  margin: 48pt 40pt 56pt 60pt;
  @bottom-center { content: counter(page); font-size: 8pt; }
}

book { font-size: 12pt; line-height: 1.5; }
h1, h2, h3 { font-size: 20pt; }
```

[The CSS subset](../css-subset.md) is everything the engine understands. Anything else becomes a warning on stderr naming the line and column, and the PDF is written regardless.

## Fonts

`@font-face` urls are resolved as paths, relative to the directory of the stylesheet that asked for them, and then relative to the working directory:

```css
@font-face {
  font-family: "House Serif";
  src: url("faces/house.ttf");
}
```

A face that resolves nowhere warns and the text falls back. See [fonts](../library/fonts.md).

## A content tree instead

A single `.json` argument is read as a [content tree](../reference/content-tree.md): the same document one stage later, for a host that builds one itself.

```sh
fleuron book.json -o book.pdf
```

It is also how to look at what the frontend did with a manuscript: dump the tree, read it, feed it back.

## What it tells you

The summary line goes to stderr, so `fleuron manuscript.md -o /dev/stdout` is still a PDF on stdout. Warnings follow it, one per line, and then a count. Exit codes are in [the reference](reference.md).
