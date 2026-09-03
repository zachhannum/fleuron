---
title: Library quickstart
description: Read a manuscript, compile styling against it, lay it out, write the PDF.
---

The sample program below does four things: reads a manuscript, compiles styling against it,
lays it out, and writes the PDF. This code can also be found at
[`crates/fleuron/examples/quickstart.rs`](https://github.com/zachhannum/fleuron/blob/main/crates/fleuron/examples/quickstart.rs).

```sh
cargo run --example quickstart -p fleuron
```

## Sample Code

```rust
use std::path::{Path, PathBuf};

use fleuron::fonts::bundled_registry;
use fleuron::images::{Assets, ImageLoader};
use fleuron::style::{FontLoader, Source, Stylesheets};
use fleuron_markdown::Options;

/// Resolves `@font-face` and image urls against one directory. The
/// engine reads no paths of its own, so the host supplies this half.
struct Files(PathBuf);

impl Files {
    fn read(&self, url: &str) -> Option<Vec<u8>> {
        std::fs::read(self.0.join(url)).ok()
    }
}

impl FontLoader for Files {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        self.read(url)
    }
}

impl ImageLoader for Files {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        self.read(url)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The frontend reads one source into sections. Assembly composes
    // sources into a book and numbers the tree in document order
    let source = "gulliver-excerpt.md";
    let markdown = std::fs::read_to_string(Path::new("fixtures").join(source))?;
    let (sections, complaints) =
        fleuron_markdown::to_sections(&markdown, source, &Options::default());
    let book = fleuron_markdown::assemble(fleuron_markdown::frontmatter(&markdown), sections);

    // The built-in sheet is always first. Author sheets cascade over it.
    let css = std::fs::read_to_string("fixtures/styled.css")?;
    let files = Files(PathBuf::from("fixtures"));
    let mut registry = bundled_registry()?;
    let mut sheets = Stylesheets::parse(&[Source::author("styled.css", &css)]);
    sheets.load_fonts(&mut registry, &files);
    let styles = sheets.compile(&book, &registry);

    // Every image the book content references.
    let assets = Assets::probe(&book, &files);

    let output = fleuron::layout::layout_book(&book, &styles, &registry, &assets);
    for warning in complaints.iter().chain(&output.warnings) {
        match &warning.origin {
            Some(origin) => eprintln!("warning: {origin}: {}", warning.message),
            None => eprintln!("warning: {}", warning.message),
        }
    }

    // The PDF is painted from the display structure.
    let bytes = fleuron::pdf::write(&output, &registry, &assets, &book.metadata)?;
    std::fs::write(Path::new("book.pdf"), bytes)?;
    println!("{} pages", output.pages.len());
    Ok(())
}
```

## What each call does

| call                 |                                                                                                                                                                                                                                   |
| -------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `to_sections`        | Reads one markdown source into sections, plus that source's diagnostics. [The markdown mapping](../reference/markdown.mdx) covers what each construct becomes, and `Options` is where the section policy and the dialect are set. |
| `frontmatter`        | Reads the `---` block at the top of a source.                                                                                                                                                                                     |
| `assemble`           | Composes sections into a book and numbers the tree in document order.                                                                                                                                                             |
| `bundled_registry`   | EB Garamond, upright and italic, registered as the default serif. See [fonts](fonts.md) for how to configure additional faces.                                                                                                    |
| `Stylesheets::parse` | Parses author sheets. The built-in user-agent sheet always goes first, so author CSS cascades over the defaults.                                                                                                                  |
| `load_fonts`         | Loads each `@font-face` url into the engine.                                                                                                                                                                                      |
| `compile`            | Resolves the cascade against the book.                                                                                                                                                                                            |
| `Assets::probe`      | Loads each image url into the engine and reads the header for the intrinsic size.                                                                                                                                                 |
| `layout_book`        | Style tree in, `LayoutOutput` out: pages of draw items, the font and asset tables those items index, and every warning the run collected. A book with no images can pass `Assets::none()`.                                        |
| `pdf::write`         | Paints the display structure into PDF bytes.                                                                                                                                                                                      |

Parsing, font loading and compiling are three calls rather than one because they have different lifetimes.

## Composing several files

Assembly is a step of its own because ordering sources and choosing metadata are the caller's 
decisions, not the parser's.

The manuscript above is a single file, so its frontmatter describes the book and goes straight to `assemble`. 
A host reading a chapter per file can build its own `Metadata` instead. The [markdown mapping](../reference/markdown.mdx) 
describes in more detail what the frontmatter represents in this context.

```rust
use fleuron::content::Metadata;
use fleuron_markdown::{Options, Sections, assemble, to_sections};

let reading = Options {
    sections: Sections::Whole,
    ..Options::default()
};

let mut sections = Vec::new();
let mut complaints = Vec::new();
for source in ["ch01.md", "ch02.md", "ch03.md"] {
    let markdown = std::fs::read_to_string(Path::new("manuscript").join(source))?;
    let (read, said) = to_sections(&markdown, source, &reading);
    sections.extend(read);
    complaints.extend(said);
}

let book = assemble(
    Metadata {
        title: Some("The Levant Papers".into()),
        author: Some("E. Marsh".into()),
        ..Metadata::default()
    },
    sections,
);
```

## Using the content tree directly

A host whose source is already structured, such as a CMS or a docx converter, can skip the frontend 
and hand the engine a `Book` directly. [The content tree](../reference/content-tree.md) shows that schema.

```rust
use fleuron::content::{Block, Book, Inline, Metadata, NodeId, Section};

let mut book = Book {
    metadata: Metadata {
        title: Some("The Levant Papers".into()),
        ..Metadata::default()
    },
    sections: vec![Section {
        title: Some("The Ambassador".into()),
        blocks: vec![Block::Paragraph {
            id: NodeId::UNASSIGNED,
            inlines: vec![Inline::Text {
                id: NodeId::UNASSIGNED,
                value: "He arrived at dusk.".into(),
                position: None,
            }],
            position: None,
        }],
        ..Section::default()
    }],
};

// Ids belong to the engine, so a tree built by hand is numbered before it is styled.
book.assign_node_ids();
```

## Styling without an author sheet

`Stylesheets::parse(&[])` compiles the built-in sheet alone: a trade paperback at 5.5×8.5 inches, 
EB Garamond at 11 points, justified, chapters opening recto. `fleuron::style::defaults(&book, &registry)` is the same thing in one call.

```rust
let registry = fleuron::fonts::bundled_registry()?;
let styles = fleuron::style::defaults(&book, &registry);
let output = fleuron::layout::layout_book(&book, &styles, &registry, &Assets::none());
```

[The CSS subset](../css-subset.mdx) is everything you can override it with.

## Laying out again

`layout_book` rebuilds every stage on every call. A program that lays the same book out over and over, 
such as a preview beside an editor, should use a [session](sessions.md) instead. 
A session remembers what each stage produced and re-runs only the ones an edit invalidates, 
so changing the page margins only re-runs fragmentation instead of another pass over every line.

```rust
use fleuron::session::Session;

let mut session = Session::new(&registry);
session.set_content(book);
session.set_style(Stylesheets::parse(&[Source::author("styled.css", &css)]));

let output = session.preview();
```

Set a new sheet and call `preview` again, and only the stages under the edit run.

## When things go wrong

The engine is designed to never panic, even with bad input. Unsupported CSS, an unresolvable 
font and a stack that matches nothing are all warnings, and the run will still finish. 
See [diagnostics](diagnostics.mdx) for what warns and what fails.
