---
title: EPUB
description: Write the same manuscript and stylesheets as a reflowable EPUB, and what the EPUB holds.
---

Fleuron can write a book as a reflowable EPUB 3 as well as a PDF. This page describes how to write one from Rust or from a worker, and what the EPUB holds. To write one from the command line, see the [CLI quickstart](../cli/quickstart.mdx#writing-an-epub).

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
| `fleuron_epub::files` | Takes the same arguments as `write`. It returns the files of the EPUB before they go into the zip, the spine, and the warnings. See [The files one by one](#the-files-one-by-one) and [A place in the book](#a-place-in-the-book). |

An EPUB needs no font registry, no style tree, and no layout. So the program does not call `load_fonts`, `compile`, `Assets::probe`, or `layout_book`.

The same manuscript and the same stylesheets give the same EPUB, byte for byte.

## From a worker

A host that runs fleuron in a worker asks the worker for the EPUB. `Client.exportEpub` sends a request with `want: 'epub'`. The worker writes the EPUB from what its session holds:

- The content tree, from the markdown sources or the `content` op.
- The stylesheets, from the `style` op.
- The images, from the `image` op.
- The fonts, from each `font` op that has a url.

The worker does not lay out the book to write the EPUB. `client.stages` is the same after the request as before it.

The following example asks for the EPUB of the book that the worker holds, and makes a file of it:

```js
const epub = await client.exportEpub();
if (epub !== null) {
  const file = new Blob([epub.bytes], { type: 'application/epub+zip' });
}
```

`epub.bytes` is the file that `fleuron_epub::write` makes. `epub.warnings` has the same shape as the `warnings` of a display structure. It holds the warnings for the markdown sources first, then the warnings of the writer. The answer is `null` when a later render overtook the request, as for a PDF. See [the wire](../wasm/wire.md#the-protocol).

A `Preview` has `exportEpub()` as well. Without a worker, `Session.exportEpub()` returns the same answer as bytes, and `decodeEpub` reads them.

## The files one by one

A host that shows the EPUB in a browser frame loads each file of the EPUB, not the zip. `Client.exportEpubFiles` asks the worker for the files. It sends a request with `want: 'epub'` and `unzipped: true`. The answer has these parts:

- `files`: every file that the zip holds, in the order of the zip. Each file has its `path` in the container, its `mediaType`, and its `bytes`.
- `spine`: the XHTML documents in reading order, from the spine of the package document. Each entry has the `path` of a document and the `section` that the document holds.
- `warnings`: the same warnings that `exportEpub` returns.

The files are the entries of the zip, with the same paths and the same bytes. The bytes are not compressed. The documents refer to the stylesheet and the images by relative paths, such as `book.css`. So a host that serves each file at its path keeps those links.

The worker transfers the answer as one buffer, and nothing copies it. The `bytes` of each file are a part of that buffer. The worker does not lay out the book to write the files.

The following example asks for the files of the EPUB, and finds the first document in reading order:

```js
const epub = await client.exportEpubFiles();
if (epub !== null) {
  const byPath = new Map(epub.files.map((file) => [file.path, file]));
  const first = byPath.get(epub.spine[0].path);
}
```

A `Preview` has `exportEpubFiles()` as well. Without a worker, `Session.exportEpubFiles()` returns the same answer as bytes, and `decodeEpubFiles` reads them. In Rust, `fleuron_epub::files` returns the same files, and `Files::zip` makes the zip that `write` returns.

## A place in the book

A node id is the number that fleuron gives to each node of the content tree. The files of the EPUB carry node ids. So a host can go from a place in the manuscript to a place in the EPUB, and back.

The `section` of a spine entry is the node id of the section that the document holds. A document of front matter has a `section`, as a chapter does. The `section` is `null` only for the empty document of a book with no sections.

Each block element in a document has its node id in the `data-node` attribute. The block elements are these:

```text
section  h1 to h6  p  blockquote  pre  hr  img
ol  ul  li  table  tr  th  td  aside
```

A block element also has an `id`. If the author set an id on the node, the element keeps that id. If not, the `id` is `n` and the node id, such as `n42`. Read the node id from `data-node`, because the `id` can be the id of the author.

`Client.sourceOf` takes a node id, and returns the source file and the bytes that the node came from. `Client.nodeAt` takes a source file and a byte, and returns the node id at that byte. See [the wire](../wasm/wire.md#the-protocol).

The node ids go up in reading order. So the document for a node is the last spine entry with a `section` that is not more than the node id.

The node from `nodeAt` can be an inline node, such as emphasis. An inline element has no `data-node`. The block element before it in the document has one.

The following example finds the document that holds one byte of a source file:

```js
const epub = await client.exportEpubFiles();
const node = await client.nodeAt('chapter-01.md', 812);
if (epub !== null && node !== null) {
  const entry = epub.spine.findLast((entry) => entry.section !== null && entry.section <= node);
}
```

The following example finds the place in the manuscript for an element that the reader selected:

```js
const block = element.closest('[data-node]');
const source = await client.sourceOf(Number(block.dataset.node));
```

In Rust, each entry of `Files::spine` has the same `path` and `section`. `Book::source_of` and `Book::node_at` answer for a book after `Book::assign_node_ids`.

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
| highlight | `mark` |
| span | `span` |
| link | `a` |
| hard break | `br` |
| note | `aside` |
| page break, column break | no element |

Each element keeps the classes and the id of its node. So a selector in a stylesheet selects the same element in the EPUB as in the PDF. A block element with no id gets one from its node id. See [A place in the book](#a-place-in-the-book).

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

### Cover

A book store or a reading system shows the cover image of an EPUB. The `cover` key of the metadata names that image by its url, as an image in the book does. Fleuron copies the image into the EPUB. It marks the image as the cover in the EPUB 3 form and in the EPUB 2 form.

The following frontmatter names the cover of a book:

```yaml
---
title: Gulliver's Travels
cover: images/plate.jpg
---
```

The CLI reads the url relative to the manuscript, as for an image in the book. A worker loads it from the `image` op with the same url.

If the image does not load, or is a type that an EPUB cannot hold, fleuron warns. The EPUB then has no cover.

The EPUB has no cover page. To show the cover at the start of the book, put the image at the top of the first section.

### Images and fonts

Fleuron copies each image that the book or a stylesheet names into the EPUB. It also copies each font that `@font-face` names. The two loaders supply the bytes. Each file goes in as it is. Fleuron does not decode it.

An EPUB can hold these types:

```text
images  PNG  JPEG  GIF  WebP  SVG
fonts   TrueType  OpenType  WOFF  WOFF2
```

If a file does not load, or is another type, fleuron warns. It then leaves out the file, and any declaration that names it.

### The stylesheet

The EPUB has one stylesheet. It starts with rules that give each element the margin and the weight that the PDF gives it. A reading system gives a paragraph a margin and a heading a bold weight, unless a rule says otherwise. The rules of the built-in stylesheet and of each author stylesheet follow, in the order of the cascade, so a rule of a book still applies. Fleuron leaves out the CSS that describes pages, because the reading system makes the pages. It leaves out this CSS with no warning, from every stylesheet. The same stylesheet makes the PDF, and the PDF uses this CSS. [CSS in an EPUB](../css-subset.mdx#css-in-an-epub) lists what fleuron leaves out.

## Warnings

`Epub::warnings` holds every warning of the run. It starts with the warnings of `Stylesheets::parse`. So CSS that fleuron does not support yet warns as it does for a PDF. Then come the warnings for files that did not go in, and for links that name nothing. CSS that describes pages does not warn. [Diagnostics](diagnostics.mdx) covers how to read a warning.
