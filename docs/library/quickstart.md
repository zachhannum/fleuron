---
title: Library quickstart
description: Book plus stylesheets plus fonts, to a LayoutOutput, to PDF bytes.
---

Four steps, in one direction: read a content tree, compile styling against it, lay it out, write the PDF. The whole of it is below, and it is also `crates/fleuron/examples/quickstart.rs` in the repository, so it compiles and runs.

```sh
cargo run --example quickstart -p fleuron
```

## The whole thing

```rust
use std::path::{Path, PathBuf};

use fleuron::content::Book;
use fleuron::fonts::bundled_registry;
use fleuron::style::{FontLoader, Source, Stylesheets};

/// Resolves `@font-face` urls against one directory. The engine reads
/// no paths of its own; this is the host half of that contract.
struct Files(PathBuf);

impl FontLoader for Files {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        std::fs::read(self.0.join(url)).ok()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Content enters as a tree. `assign_node_ids` numbers it in
    // document order, which is what diagnostics point at.
    let json = std::fs::read_to_string("fixtures/book.json")?;
    let mut book: Book = serde_json::from_str(&json)?;
    book.assign_node_ids();

    // Styling enters as CSS. The built-in sheet is always first;
    // author sheets cascade over it in the order given.
    let css = std::fs::read_to_string("fixtures/styled.css")?;
    let mut registry = bundled_registry()?;
    let mut sheets = Stylesheets::parse(&[Source::author("styled.css", &css)]);
    sheets.load_fonts(&mut registry, &Files(PathBuf::from("fixtures")));
    let styles = sheets.compile(&book, &registry);

    // One call from styled tree to pages of draw items.
    let output = fleuron::layout::layout_book(&book, &styles, &registry);
    for warning in &output.warnings {
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

Against the fixture book that ships with the repository, that prints `31 pages` and writes `book.pdf` into the working directory.

## What each step is for

**`Book` and `assign_node_ids`.** The content tree is the input contract; see [the content tree reference](../reference/content-tree.md) for the JSON it deserializes from. Node ids are engine-assigned, never supplied by the frontend, so a document cannot collide ids or forge a diagnostic origin. Fresh off the wire every id is unassigned; one call numbers the tree in document order.

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
