---
title: CLI quickstart
description: Fixture book in, PDF out, in one command.
---

```sh
git clone https://github.com/zachhannum/fleuron
cd fleuron
cargo run --release -p fleuron-cli -- fixtures/book.json -o book.pdf -c fixtures/styled.css
```

```text
fleuron: fixtures/book.json → book.pdf: 31 pages
```

That is an excerpt of *Gulliver's Travels* — real prose, with dialogue, em-dashes and words a hyphenator has opinions about — laid out at 5.5×8.5 inches with mirrored margins, a folio in the bottom margin, and a running head on each chapter's opening page. The stylesheet doing all of that is 47 lines, and it is `fixtures/styled.css`.

## Without the clone

```sh
cargo install --git https://github.com/zachhannum/fleuron fleuron-cli
fleuron book.json -o book.pdf
```

With no `-c`, the built-in stylesheet is the whole of the styling: a trade paperback, justified, chapters opening recto. It is a real design rather than a placeholder, and a book that never overrides it is still a book.

## Adding your own styling

Stylesheets cascade in the order the command line gives them, over the built-in sheet rather than instead of it:

```sh
fleuron book.json -o book.pdf -c house.css -c series.css
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

Everything the engine understands is in [the CSS subset](../css-subset.md). Everything it does not is a warning on stderr naming the line and column, and the PDF is written regardless.

## Fonts

`@font-face` urls are resolved as paths, relative to the directory of the stylesheet that asked for them, and then relative to the working directory:

```css
@font-face {
  font-family: "House Serif";
  src: url("faces/house.ttf");
}
```

A face that resolves nowhere warns and the text falls back. See [fonts](../library/fonts.md).

## Where the input comes from

`book.json` is a content tree: a semantic document, not markup. [The content tree reference](../reference/content-tree.md) is the schema. Markdown frontends produce it from remark or rehype with a field rename rather than a conversion pass — the shape maps one-to-one onto mdast.

## What it tells you

The summary line goes to stderr, so `fleuron book.json -o /dev/stdout` is still a PDF on stdout. Warnings follow it, one per line, and then a count. Exit codes are in [the reference](reference.md).
