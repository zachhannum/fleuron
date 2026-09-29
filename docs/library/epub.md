---
title: EPUB
description: Write the same manuscript and stylesheets as a reflowable EPUB, and what the EPUB holds.
---

Fleuron can write a book as a reflowable EPUB 3 as well as a PDF. This page describes how to write one from Rust, and what the EPUB holds. To write one from the command line, see the [CLI quickstart](../cli/quickstart.mdx#writing-an-epub).

A reflowable EPUB is XHTML and CSS in a zip file. The reading system on the device breaks the lines and makes the pages. So fleuron does not lay out the book to write an EPUB. It writes the [content tree](../reference/content-tree.md) and the stylesheets. The reading system does the rest.

The pages of the PDF do not carry over to the EPUB. The reader can change the size of the text and the size of the screen. The reading system then makes new pages. The [preview](../wasm/preview.mdx) shows the pages of the PDF, not the pages a reading system makes.

## Sample code

The `fleuron-epub` crate writes the EPUB. The following example reads the fixture book and its stylesheet, and writes `book.epub`. The code is also at [`crates/fleuron-epub/examples/epub.rs`](https://github.com/zachhannum/fleuron/blob/main/crates/fleuron-epub/examples/epub.rs).

```sh
cargo run --example epub -p fleuron-epub
```

```rust
use std::path::{Path, PathBuf};

use fleuron::images::ImageLoader;
use fleuron::style::{FontLoader, Source, Stylesheets};
use fleuron_markdown::Options;

/// Resolves `@font-face` and image urls against one directory.
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
    let source = "gulliver-excerpt.md";
    let markdown = std::fs::read_to_string(Path::new("fixtures").join(source))?;
    let (sections, complaints) =
        fleuron_markdown::to_sections(&markdown, source, &Options::default());
    let book = fleuron_markdown::assemble(fleuron_markdown::frontmatter(&markdown), sections);

    // The same sheets as the PDF. Nothing is laid out, so there is no
    // font registry and no style tree.
    let css = std::fs::read_to_string("fixtures/styled.css")?;
    let sheets = Stylesheets::parse(&[Source::author("styled.css", &css)]);
    let files = Files(PathBuf::from("fixtures"));

    let epub = fleuron_epub::write(&book, &sheets, &files, &files);
    for warning in complaints.iter().chain(&epub.warnings) {
        match &warning.origin {
            Some(origin) => eprintln!("warning: {origin}: {}", warning.message),
            None => eprintln!("warning: {}", warning.message),
        }
    }
    std::fs::write("book.epub", &epub.bytes)?;
    Ok(())
}
```

## What each call does

| call | |
|---|---|
| `to_sections`, `frontmatter`, `assemble` | Read the manuscript into a book, as for a PDF. See the [library quickstart](quickstart.md). |
| `Stylesheets::parse` | Parses the author sheets. The built-in sheet goes first, as for a PDF. |
| `fleuron_epub::write` | Writes the book as an EPUB. It takes the book, the parsed sheets, a loader for images, and a loader for fonts. It returns the bytes of the file and the warnings. |

An EPUB needs no font registry, no style tree, and no layout. So the program does not call `load_fonts`, `compile`, `Assets::probe`, or `layout_book`.

The same manuscript and the same stylesheets give the same EPUB, byte for byte.

## What the EPUB holds

The EPUB holds one XHTML document for each section, in the order of the sections. The reading order of the EPUB is the same order. Each node of the content tree becomes the HTML element below:

| node | element |
|---|---|
| book | `body` |
| section | `section` |
| heading | `h1` to `h6` |
| paragraph | `p` |
| quotation | `blockquote` |
| code block | `pre` |
| thematic break | `hr` |
| image | `img` |
| list | `ol` or `ul`, and `li` |
| table | `table`, `thead`, `tbody`, `tr`, `th`, and `td` |
| emphasis, strong, code, strikethrough | `em`, `strong`, `code`, and `s` |
| span | `span` |
| link | `a` |
| hard break | `br` |
| note | `aside` |
| page break, column break | no element |

Each element keeps the classes and the id of its node. So a selector in a stylesheet selects the same element in the EPUB as in the PDF.

A tight list is a list with no blank lines between its items. A paragraph in an item of a tight list has no element, as for a PDF. Its text is the text of the `li`.

A page break and a column break have no element, because the reading system makes the pages.

### Notes

A note is an `aside` after the section that holds it. Where the note was written, a link with the number of the note goes to the `aside`. The numbers start again at 1 in each section. A reading system can show the note in a pop-up when the reader selects the number.

### Links

A link to a heading or an id in the book goes to that element, in the document that holds it. A link to a web address, such as `https://example.com`, goes to that address. If a link names nothing in the book, its text is not a link, and fleuron warns.

### Table of contents

The table of contents of the EPUB is the list of the headings of the book. A level 2 heading is under the level 1 heading before it. A heading in a quotation, a list, a table, or a note is not in the table of contents. A book with no headings lists its sections instead.

### Metadata

The metadata of the book goes to the Dublin Core elements of the EPUB. Dublin Core is the metadata vocabulary that EPUB uses.

| metadata | EPUB |
|---|---|
| `title` | `dc:title` |
| `author` | `dc:creator` |
| `language` | `dc:language` |
| `identifier` | `dc:identifier` |
| `modified` | `dcterms:modified` |

The keys below go to the `dc:` element of the same name. Fleuron leaves out other keys.

```text
contributor  coverage  date  description  publisher
relation  rights  source  subject  type
```

An EPUB must have a title, a language, an identifier, and a date of modification. If the metadata does not give one, fleuron writes the value below:

| element | value |
|---|---|
| `dc:title` | The text of the first heading, or `Untitled`. |
| `dc:language` | `und`, the code for a language that is not known. |
| `dc:identifier` | A UUID made from the content of the book. The same book gets the same UUID. |
| `dcterms:modified` | `2000-01-01T00:00:00Z` |

### Images and fonts

Fleuron copies each image that the book or a stylesheet names into the EPUB. It also copies each font that `@font-face` names. The two loaders supply the bytes. Each file goes in as it is. Fleuron does not decode it.

An EPUB can hold these types:

```text
images  PNG  JPEG  GIF  WebP  SVG
fonts   TrueType  OpenType  WOFF  WOFF2
```

If a file does not load, or is another type, fleuron warns. It then leaves out the file, and any declaration that names it.

### The stylesheet

The EPUB has one stylesheet. It holds the rules of the built-in stylesheet and of each author stylesheet, in the order of the cascade. Fleuron leaves out the CSS that describes pages, because the reading system makes the pages. [CSS in an EPUB](../css-subset.mdx#css-in-an-epub) lists what goes and what warns.

## Warnings

`Epub::warnings` holds every warning of the run. It starts with the warnings of `Stylesheets::parse`. So CSS that fleuron does not support yet warns as it does for a PDF. Then come the warnings for CSS in the author sheets that describes pages, for files that did not go in, and for links that name nothing. [Diagnostics](diagnostics.mdx) covers how to read a warning.
