---
title: Library quickstart
description: Read a manuscript, compile styling against it, lay it out, write the PDF.
---

Four steps, in one direction: read a manuscript, compile styling against it, lay it out, write the PDF. The program below does all four. It is also `crates/fleuron/examples/quickstart.rs`, so it compiles and runs.

```sh
cargo run --example quickstart -p fleuron
```

## The whole program

```rust
use std::path::{Path, PathBuf};

use fleuron::fonts::bundled_registry;
use fleuron::style::{FontLoader, Source, Stylesheets};
use fleuron_markdown::Options;

/// Resolves `@font-face` urls against one directory. The engine reads
/// no paths of its own, so the host supplies this half.
struct Files(PathBuf);

impl FontLoader for Files {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        std::fs::read(self.0.join(url)).ok()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The frontend reads one source into sections. Assembly composes
    // sources into a book and numbers the tree in document order,
    // which is what diagnostics point at.
    let source = "gulliver-excerpt.md";
    let markdown = std::fs::read_to_string(Path::new("fixtures").join(source))?;
    let (sections, complaints) =
        fleuron_markdown::to_sections(&markdown, source, &Options::default());
    let book = fleuron_markdown::assemble(fleuron_markdown::frontmatter(&markdown), sections);

    // The built-in sheet is always first. Author sheets cascade over
    // it in the order given.
    let css = std::fs::read_to_string("fixtures/styled.css")?;
    let mut registry = bundled_registry()?;
    let mut sheets = Stylesheets::parse(&[Source::author("styled.css", &css)]);
    sheets.load_fonts(&mut registry, &Files(PathBuf::from("fixtures")));
    let styles = sheets.compile(&book, &registry);

    // One call from styled tree to pages of draw items.
    let output = fleuron::layout::layout_book(&book, &styles, &registry);
    for warning in complaints.iter().chain(&output.warnings) {
        match &warning.origin {
            Some(origin) => eprintln!("warning: {origin}: {}", warning.message),
            None => eprintln!("warning: {}", warning.message),
        }
    }

    // The PDF is painted from the display list. Nothing lays out twice.
    let bytes = fleuron::pdf::write(&output, &registry, &book.metadata)?;
    std::fs::write(Path::new("book.pdf"), bytes)?;
    println!("{} pages", output.pages.len());
    Ok(())
}
```

Against the manuscript in the repository, that prints `34 pages` and writes `book.pdf` into the working directory.

## What each call does

| call | |
|---|---|
| `to_sections` | Reads one markdown source into sections, plus that source's diagnostics. [The markdown mapping](../reference/markdown.mdx) covers what each construct becomes, and `Options` is where the section policy and the dialect are set. |
| `frontmatter` | Reads the `---` block at the top of a source. |
| `assemble` | Composes sections into a book and numbers the tree in document order. |
| `bundled_registry` | EB Garamond, upright and italic, registered as the default serif. Everything else arrives through `@font-face`. See [fonts](fonts.md). |
| `Stylesheets::parse` | Parses author sheets. The built-in user-agent sheet always goes first, so author CSS cascades over the defaults instead of replacing them. |
| `load_fonts` | Hands each `@font-face` url to your `FontLoader`. This is the only step that reaches outside the engine. |
| `compile` | Resolves the cascade against the book. |
| `layout_book` | Style tree in, `LayoutOutput` out: pages of draw items, the font table those items index, and every warning the run collected. |
| `pdf::write` | Paints the display list. It re-derives no layout, and it resolves font ids through the same registry that shaped the runs, so the embedded subset holds the outlines the shaper used. |

Parsing, font loading and compiling are three calls rather than one because they have different lifetimes. Sheets parse once and style many books.

## Composing several files

Assembly is a step of its own because ordering sources and choosing metadata are the caller's decisions, not the parser's.

The manuscript above is a single file, so its frontmatter describes the book and goes straight to `assemble`. A host reading a chapter per file builds its own `Metadata` instead: sixty chapter files have sixty frontmatter blocks and none of them names the work. Each file's `title:` becomes its own section's. [The markdown mapping](../reference/markdown.mdx) has the loop.

A host whose source is already structured, such as a CMS or a docx converter, can skip the frontend and hand the engine a `Book` directly. [The content tree](../reference/content-tree.md) is that schema.

## Styling without an author sheet

`Stylesheets::parse(&[])` compiles the built-in sheet alone: a trade paperback at 5.5×8.5 inches, EB Garamond at 11 points, justified, chapters opening recto. `fleuron::style::defaults(&book, &registry)` is the same thing in one call.

[The CSS subset](../css-subset.mdx) is everything you can override it with.

## Laying out again

`layout_book` rebuilds every stage on every call, which is what a program that renders one book and exits wants. A program that lays the same book out over and over, such as a preview beside an editor, wants a [session](sessions.md) instead. A session remembers what each stage produced and re-runs only the ones an edit invalidates, so changing the page margins costs fragmentation instead of another pass over every line.

## When things go wrong

Nothing above panics on bad input. Unsupported CSS, an unresolvable font and a stack that matches nothing are all warnings, and the run finishes. [Diagnostics](diagnostics.mdx) covers what warns and what fails.
