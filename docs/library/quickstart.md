---
title: Library quickstart
description: Markdown plus stylesheets plus fonts, to a LayoutOutput, to PDF bytes.
---

Four steps, in one direction: read a manuscript, compile styling against it, lay it out, write the PDF. The whole of it is below, and it is also `crates/fleuron/examples/quickstart.rs` in the repository, so it compiles and runs.

```sh
cargo run --example quickstart -p fleuron
```

## The whole thing

```rust
use std::path::{Path, PathBuf};

use fleuron::fonts::bundled_registry;
use fleuron::style::{FontLoader, Source, Stylesheets};
use fleuron_markdown::Options;

/// Resolves `@font-face` urls against one directory. The engine reads
/// no paths of its own; this is the host half of that contract.
struct Files(PathBuf);

impl FontLoader for Files {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        std::fs::read(self.0.join(url)).ok()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Content enters as markdown. The frontend reads one source into
    // sections; assembly composes the sources into a book and numbers
    // it in document order, which is what diagnostics point at.
    let source = "gulliver-excerpt.md";
    let markdown = std::fs::read_to_string(Path::new("fixtures").join(source))?;
    let (sections, complaints) =
        fleuron_markdown::to_sections(&markdown, source, &Options::default());
    let book = fleuron_markdown::assemble(fleuron_markdown::frontmatter(&markdown), sections);

    // Styling enters as CSS. The built-in sheet is always first;
    // author sheets cascade over it in the order given.
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

    // The PDF is painted from the display list, not laid out again.
    let bytes = fleuron::pdf::write(&output, &registry, &book.metadata)?;
    std::fs::write(Path::new("book.pdf"), bytes)?;
    println!("{} pages", output.pages.len());
    Ok(())
}
```

Against the manuscript that ships with the repository, that prints `34 pages` and writes `book.pdf` into the working directory.

## What each step is for

**`to_sections` and `assemble`.** `fleuron-markdown` is the frontend: one source in, that source's sections and its diagnostics out. [The markdown mapping](../reference/markdown.md) is what it does with each construct, and `Options` is where the section policy and the dialect are named. Assembly is a step of its own because composing sources is the caller's decision, not the parser's: you order them and you decide the metadata. It also numbers the tree, in document order, which is what diagnostics point at.

A host with a tree of its own, such as a CMS or a docx converter, can skip the frontend and hand the engine a `Book` directly; [the content tree reference](../reference/content-tree.md) is that schema.

**`FontRegistry`.** `bundled_registry()` gives you EB Garamond, upright and italic, registered as the default serif. Everything else a stylesheet asks for arrives through `@font-face`. See [fonts](fonts.md).

**`Stylesheets`.** Parsing, font loading and compiling are three calls rather than one because they have different lifetimes: sheets parse once and style many books, and font loading is the single step that reaches outside the engine. `Stylesheets::parse` always puts the built-in user-agent sheet first, so author CSS is a cascade over defaults rather than a replacement for them.

**`layout_book`.** Style tree in, `LayoutOutput` out: pages of draw items, the font table those items index, and every warning the run collected.

**`pdf::write`.** A painter over the display list. It re-derives no layout, and it resolves font ids through the same registry that shaped the runs, so the embedded subset holds the outlines the shaper actually used.

## Styling without an author sheet

`Stylesheets::parse(&[])` compiles the built-in sheet alone, which is a trade paperback: 5.5×8.5 inches, EB Garamond at 11 points, justified, chapters opening recto. `fleuron::style::defaults(&book, &registry)` is the same thing in one call.

Everything the built-in sheet can be overridden with is in [the CSS subset](../css-subset.md).

## Laying out again

`layout_book` rebuilds every stage on every call, which is what a program that renders one book and exits wants. A program that lays the same book out over and over, such as a preview beside an editor, wants a [session](sessions.md) instead. It remembers what each stage produced and re-runs only the ones an edit invalidates, so a change to the page margins costs fragmentation instead of another pass over every line.

## Where things go wrong

Nothing above panics on bad input. Unsupported CSS, an unresolvable font, and a stack that matches nothing are all warnings, and the run finishes. [Diagnostics](diagnostics.md) covers what warns and what fails.
