//! The EPUB writer through its public API: a manuscript read by the
//! shipped frontend, its sheets, and the archive that comes back.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fleuron::content::Book;
use fleuron::images::ImageLoader;
use fleuron::style::{FontLoader, Source, Stylesheets};
use fleuron_markdown::{Dialect, Options};

/// A manuscript that writes every block and every inline the
/// vocabulary has.
const EVERY_VARIANT: &str = r#"---
title: The Levant Papers
author: E. Marsh
language: en
---

# One {#one .opening}

He arrived *on a Tuesday*, **late**, with `a trunk`, a ~~cat~~ and
[a letter](#two) in a [plain hand]{.hand}.[^trunk]
The porter said nothing.\
Nobody asked him to.

[^trunk]: The trunk held *books*.

> A quotation, with a [link out](https://example.com).

```text
code & <markup>
```

---

\pagebreak

\columnbreak

![An ornament](ornament.png)

2. second
3. third

- loose

- items

| Left | Right |
|:-----|------:|
| a    | b     |

# Two

The second section.
"#;

/// Files beside the test, answered by name.
struct Files(BTreeMap<&'static str, Vec<u8>>);

impl Files {
    fn new() -> Files {
        let png = std::fs::read(fixtures().join("images/fleuron.png"))
            .expect("the fixture ornament is checked in");
        Files(BTreeMap::from([("ornament.png", png)]))
    }
}

impl ImageLoader for Files {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        self.0.get(url).cloned()
    }
}

impl FontLoader for Files {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        self.0.get(url).cloned()
    }
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn read(markdown: &str) -> Book {
    let options = Options {
        dialect: Dialect {
            gfm: true,
            ..Dialect::fleuron()
        },
        ..Options::default()
    };
    let (sections, warnings) = fleuron_markdown::to_sections(markdown, "levant.md", &options);
    assert!(warnings.is_empty(), "{warnings:?}");
    fleuron_markdown::assemble(fleuron_markdown::frontmatter(markdown), sections)
}

fn write(book: &Book, css: &str) -> fleuron_epub::Epub {
    let sheets = Stylesheets::parse(&[Source::author("author.css", css)]);
    let files = Files::new();
    fleuron_epub::write(book, &sheets, &files, &files)
}

/// The entries of a zip archive, by name, inflated.
fn entries(archive: &[u8]) -> Vec<(String, Vec<u8>)> {
    let end = archive.len() - 22;
    assert_eq!(&archive[end..end + 4], b"PK\x05\x06", "no end record");
    let le16 = |at: usize| u16::from_le_bytes([archive[at], archive[at + 1]]) as usize;
    let le32 = |at: usize| u32::from_le_bytes(archive[at..at + 4].try_into().unwrap()) as usize;
    let count = le16(end + 10);
    let mut at = le32(end + 16);
    let mut out = Vec::new();
    for _ in 0..count {
        assert_eq!(&archive[at..at + 4], b"PK\x01\x02");
        let method = le16(at + 10);
        let compressed = le32(at + 20);
        let name_len = le16(at + 28);
        let offset = le32(at + 42);
        let name = String::from_utf8(archive[at + 46..at + 46 + name_len].to_vec()).unwrap();
        let data = offset + 30 + le16(offset + 26) + le16(offset + 28);
        let held = &archive[data..data + compressed];
        let bytes = match method {
            0 => held.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec(held).expect("the entry inflates"),
            other => panic!("method {other}"),
        };
        out.push((name, bytes));
        at += 46 + name_len;
    }
    out
}

fn entry(archive: &[u8], name: &str) -> String {
    let (_, bytes) = entries(archive)
        .into_iter()
        .find(|(entry, _)| entry == name)
        .unwrap_or_else(|| panic!("no {name}"));
    String::from_utf8(bytes).expect("the entry is text")
}

/// Acceptance: every `Block` and every `Inline` variant has a mapping.
/// The manuscript writes each one, and the document holds the element
/// each one maps to. A new variant does not compile until it has one:
/// the crate denies wildcard arms, so each match names every variant.
#[test]
fn every_block_and_inline_has_a_mapping() {
    let book = read(EVERY_VARIANT);
    let epub = write(&book, "");
    assert!(epub.warnings.is_empty(), "{:?}", epub.warnings);
    let one = entry(&epub.bytes, "EPUB/section-001.xhtml");
    for element in [
        "<h1 id=\"one\" class=\"opening\">",
        "<p>",
        "<blockquote>",
        "<pre>code &amp; &lt;markup&gt;",
        "<hr></hr>",
        "<img src=\"media/image-1.png\" alt=\"An ornament\"></img>",
        "<ol start=\"2\">",
        "<ul>",
        "<li>second</li>",
        "<li><p>loose</p></li>",
        "<table>",
        "<thead>",
        "<th data-align=\"left\">",
        "<td data-align=\"right\">",
        "<em>on a Tuesday</em>",
        "<strong>late</strong>",
        "<code>a trunk</code>",
        "<s>cat</s>",
        "<span class=\"hand\">plain hand</span>",
        "<a href=\"section-002.xhtml#n",
        "<a href=\"https://example.com\">",
        "<br/>",
        "<a epub:type=\"noteref\" role=\"doc-noteref\" href=\"#note-1\">1</a>",
        "<aside id=\"note-1\" epub:type=\"footnote\" role=\"doc-footnote\">",
    ] {
        assert!(one.contains(element), "{element} is not in\n{one}");
    }
    insta::assert_snapshot!("every_variant", one);
}

/// The container holds the media type first and uncompressed, the
/// container file that points at the package, and a document per
/// section in the spine in the order of the sections.
#[test]
fn the_container_holds_a_document_per_section_in_order() {
    let epub = write(&read(EVERY_VARIANT), "");
    let names: Vec<String> = entries(&epub.bytes)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        names,
        [
            "mimetype",
            "META-INF/container.xml",
            "EPUB/package.opf",
            "EPUB/nav.xhtml",
            "EPUB/book.css",
            "EPUB/section-001.xhtml",
            "EPUB/section-002.xhtml",
            "EPUB/media/image-1.png",
        ]
    );
    assert_eq!(&epub.bytes[30..38], b"mimetype");
    assert_eq!(&epub.bytes[38..58], b"application/epub+zip");
    assert!(
        entry(&epub.bytes, "META-INF/container.xml").contains("full-path=\"EPUB/package.opf\"")
    );

    let package = entry(&epub.bytes, "EPUB/package.opf");
    let spine = &package[package.find("<spine>").unwrap()..];
    assert!(
        spine.find("section-1").unwrap() < spine.find("section-2").unwrap(),
        "{spine}"
    );
    assert!(package.contains("href=\"media/image-1.png\" media-type=\"image/png\""));
}

fn unzipped(book: &Book, css: &str) -> fleuron_epub::Files {
    let sheets = Stylesheets::parse(&[Source::author("author.css", css)]);
    let files = Files::new();
    fleuron_epub::files(book, &sheets, &files, &files)
}

/// Acceptance: every entry in the zip is in the unzipped files, with
/// the same path and the same bytes, and nothing else is.
#[test]
fn the_files_are_the_entries_of_the_zip() {
    let book = read(EVERY_VARIANT);
    let css = "h1 { background-image: url(ornament.png) }";
    let epub = write(&book, css);
    let unzipped = unzipped(&book, css);
    let files: Vec<(String, Vec<u8>)> = unzipped
        .files
        .iter()
        .map(|file| (file.path.clone(), file.bytes.clone()))
        .collect();
    assert_eq!(files, entries(&epub.bytes));
    assert_eq!(unzipped.zip(), epub.bytes);
    assert_eq!(unzipped.warnings, epub.warnings);

    let package = entry(&epub.bytes, "EPUB/package.opf");
    for file in &unzipped.files {
        let Some(href) = file.path.strip_prefix("EPUB/") else {
            continue;
        };
        if href == "package.opf" {
            continue;
        }
        let item = format!("href=\"{href}\" media-type=\"{}\"", file.media_type);
        assert!(package.contains(&item), "the manifest has no {item}");
    }
}

/// Acceptance: the unzipped files carry the spine order from the
/// package document, as paths in the container.
#[test]
fn the_files_carry_the_spine_of_the_package_document() {
    let book = read(&format!("{EVERY_VARIANT}\n# Three\n\nThe third section.\n"));
    let unzipped = unzipped(&book, "");
    let package = String::from_utf8(
        unzipped
            .files
            .iter()
            .find(|file| file.path == "EPUB/package.opf")
            .expect("the package document is a file")
            .bytes
            .clone(),
    )
    .unwrap();
    let attribute = |tag: &str, name: &str| -> String {
        let start = tag.find(&format!("{name}=\"")).unwrap() + name.len() + 2;
        tag[start..start + tag[start..].find('"').unwrap()].to_string()
    };
    let items: BTreeMap<String, String> = package
        .split("<item ")
        .skip(1)
        .map(|tag| (attribute(tag, "id"), attribute(tag, "href")))
        .collect();
    let spine: Vec<String> = package
        .split("<itemref ")
        .skip(1)
        .map(|tag| format!("EPUB/{}", items[&attribute(tag, "idref")]))
        .collect();
    assert_eq!(unzipped.spine, spine);
    assert_eq!(
        unzipped.spine,
        [
            "EPUB/section-001.xhtml",
            "EPUB/section-002.xhtml",
            "EPUB/section-003.xhtml"
        ]
    );
}

/// The metadata goes to its Dublin Core homes, and the keys the
/// package requires are there when the book names none.
#[test]
fn the_metadata_goes_to_dublin_core() {
    let mut book = read(EVERY_VARIANT);
    book.metadata
        .extra
        .insert("identifier".into(), "isbn:9780000000002".into());
    book.metadata
        .extra
        .insert("publisher".into(), "Fleuron & Co".into());
    book.metadata.extra.insert("shelf".into(), "top".into());
    let package = entry(&write(&book, "").bytes, "EPUB/package.opf");
    for element in [
        "<dc:title>The Levant Papers</dc:title>",
        "<dc:creator>E. Marsh</dc:creator>",
        "<dc:language>en</dc:language>",
        "<dc:identifier id=\"book-id\">isbn:9780000000002</dc:identifier>",
        "<dc:publisher>Fleuron &amp; Co</dc:publisher>",
        "<meta property=\"dcterms:modified\">",
    ] {
        assert!(package.contains(element), "{element} is not in\n{package}");
    }
    assert!(!package.contains("shelf"), "{package}");

    let bare = Book {
        sections: read(EVERY_VARIANT).sections,
        ..Book::default()
    };
    let package = entry(&write(&bare, "").bytes, "EPUB/package.opf");
    assert!(package.contains("<dc:identifier id=\"book-id\">urn:uuid:"));
    assert!(package.contains("<dc:language>und</dc:language>"));
    assert!(package.contains("<dc:title>One</dc:title>"));
}

/// The navigation document is the heading tree, each entry linking to
/// its heading in its section's document.
#[test]
fn the_navigation_is_the_heading_tree() {
    let book = read("# Part\n\n## One\n\nText.\n\n## Two\n\nText.\n\n# Coda\n\nText.\n");
    let nav = entry(&write(&book, "").bytes, "EPUB/nav.xhtml");
    let tree = &nav[nav.find("<ol>").unwrap()..nav.find("</nav>").unwrap()];
    insta::assert_snapshot!("navigation", tree);
}

/// Acceptance: two runs make the same archive, to the byte. Every
/// entry carries the same pinned date.
#[test]
fn two_runs_make_the_same_bytes() {
    let book = read(EVERY_VARIANT);
    let css = "blockquote { background-image: url(ornament.png) }";
    assert_eq!(write(&book, css).bytes, write(&book, css).bytes);
}

/// Acceptance: each kind of CSS that describes a page drops from the
/// EPUB with no warning when a host sheet has it.
#[test]
fn css_that_describes_a_page_drops_without_a_warning() {
    let css = r#"@page {
  size: a5;
  @top-center { content: string(chapter) }
}
@page chapter:first { margin-top: 40mm }
h1 {
  color: #111;
  page: chapter;
  break-before: page;
  break-after: avoid;
  break-inside: avoid;
  string-set: chapter content();
  counter-reset: page 1;
}
p {
  color: #333;
  orphans: 3;
  widows: 3;
  box-decoration-break: clone;
}
blockquote { column-span: all }
a.ref::after { content: " (page " target-counter(attr(href url), page) ")" }
img {
  position: absolute;
  wrap-flow: end;
  shape-outside: auto;
  shape-margin: 2mm;
}
notes { color: red }
section > pagebreak { color: red }
:is(p, columnbreak) { color: red }
"#;
    let epub = write(&read(EVERY_VARIANT), css);
    assert!(epub.warnings.is_empty(), "{:?}", epub.warnings);

    let sheet = entry(&epub.bytes, "EPUB/book.css");
    for kept in ["h1 {\n  color: #111;", "p {\n  color: #333;"] {
        assert!(sheet.contains(kept), "{kept} is not in\n{sheet}");
    }
    for gone in [
        "@page",
        "size: a5",
        "margin-top: 40mm",
        "page: chapter",
        "break-",
        "string-set",
        "counter-reset",
        "orphans",
        "widows",
        "box-decoration-break",
        "column-span",
        "target-counter",
        "position: absolute",
        "wrap-flow",
        "shape-",
        "color: red",
    ] {
        assert!(!sheet.contains(gone), "{gone} is in\n{sheet}");
    }
}

/// Acceptance: a property outside the subset still warns with a line
/// and a column, and does not reach the EPUB's sheet.
#[test]
fn a_property_outside_the_subset_warns_where_it_was_written() {
    let css = "p {\n  color: #333;\n  float: left;\n  orphans: 3;\n}\n";
    let epub = write(&read(EVERY_VARIANT), css);
    assert_eq!(epub.warnings.len(), 1, "{:?}", epub.warnings);
    assert_eq!(epub.warnings[0].origin.as_deref(), Some("author.css:3:3"));
    assert!(
        epub.warnings[0].message.contains("`float`"),
        "{:?}",
        epub.warnings
    );

    let sheet = entry(&epub.bytes, "EPUB/book.css");
    assert!(sheet.contains("color: #333;"), "{sheet}");
    assert!(!sheet.contains("float"), "{sheet}");
}

/// Acceptance: an image or a font file that is missing, or of a type
/// an EPUB cannot hold, still warns.
#[test]
fn a_missing_or_unknown_image_or_font_file_warns() {
    let css = r#"@font-face { font-family: "Gone"; src: url(gone.otf) }
@font-face { font-family: "Odd"; src: url(odd.otf) }
blockquote { background-image: url(gone.png) }
h1 { background-image: url(odd.png) }
"#;
    let sheets = Stylesheets::parse(&[Source::author("author.css", css)]);
    let mut files = Files::new();
    files.0.insert("odd.png", b"not an image".to_vec());
    files.0.insert("odd.otf", b"not a font".to_vec());
    let epub = fleuron_epub::write(&read(EVERY_VARIANT), &sheets, &files, &files);
    for url in ["gone.otf", "odd.otf", "gone.png", "odd.png"] {
        assert!(
            epub.warnings.iter().any(|w| w.message.contains(url)),
            "no warning names {url}: {:?}",
            epub.warnings
        );
    }
    assert_eq!(epub.warnings.len(), 4, "{:?}", epub.warnings);
}

/// The book is `body` and a note is `aside`, so a rule for either
/// reaches the element it reached in the PDF. A url in a declaration
/// points at the copy in the container.
#[test]
fn the_sheet_names_the_elements_the_documents_hold() {
    let css = "book { font-size: 12pt }\nnote p { font-size: 9pt }\nblockquote { background-image: url(ornament.png) }";
    let epub = write(&read(EVERY_VARIANT), css);
    assert!(epub.warnings.is_empty(), "{:?}", epub.warnings);
    let sheet = entry(&epub.bytes, "EPUB/book.css");
    for rule in [
        "body {\n  font-size: 12pt;",
        "aside p {\n  font-size: 9pt;",
        "background-image: url(\"media/image-1.png\");",
    ] {
        assert!(sheet.contains(rule), "{rule} is not in\n{sheet}");
    }
}

/// A column's alignment comes after the built-in rule for the cells
/// and before the author's, and weighs the same as either. So it
/// overrides the built-in sheet, and an author rule for the cells
/// overrides it.
#[test]
fn a_column_alignment_sits_between_the_built_in_and_the_author_rules() {
    let epub = write(&read(EVERY_VARIANT), "td { text-align: center }");
    let sheet = entry(&epub.bytes, "EPUB/book.css");
    let built_in = sheet
        .find("th,\ntd {")
        .expect("the built-in rule for the cells");
    let aligned = sheet
        .find("td:where([data-align=\"right\"])")
        .expect("the alignment rule");
    let author = sheet
        .find("td {\n  text-align: center;")
        .expect("the author rule");
    assert!(built_in < aligned && aligned < author, "{sheet}");
}

/// Acceptance: the margin of a paragraph in the EPUB is the margin in
/// the PDF. The built-in sheet gives `p` none, so the engine lays out
/// a paragraph with none, and a browser's `1em` has to be cancelled.
/// The rules that cancel it come before every other rule.
#[test]
fn a_paragraph_has_the_margin_the_pdf_gives_it() {
    let built_in = Stylesheets::parse(&[]);
    let written = built_in.written();
    let gives_margin = |element: &str| {
        written.iter().flat_map(|sheet| &sheet.rules).any(|rule| {
            rule.selectors.iter().any(|selector| selector == element)
                && rule.declarations.iter().any(|d| d.property == "margin")
        })
    };
    assert!(!gives_margin("p"), "the built-in sheet gives p a margin");

    let sheet = entry(&write(&read(EVERY_VARIANT), "").bytes, "EPUB/book.css");
    let reset = sheet.find("p, h1,").expect("the reset rule");
    assert_eq!(reset, 0, "the reset is not at the head of\n{sheet}");
    let rule = &sheet[..sheet.find('}').unwrap()];
    assert!(rule.contains("margin: 0;"), "{rule}");
    for element in [
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "li",
        "aside",
        "blockquote",
        "pre",
        "ul",
        "ol",
        "hr",
        "table",
    ] {
        assert!(
            rule.contains(&format!(" {element},")) || rule.contains(&format!(" {element} {{")),
            "{element} is not reset in\n{rule}"
        );
    }
    // Every element the engine lays out with no margin of its own is
    // reset, and every one with a margin gets it after the reset.
    for margin in ["margin: 1em 2em;", "margin: 0.5em 0;"] {
        let at = sheet.find(margin).expect(margin);
        assert!(at > reset, "{margin} comes before the reset");
    }
}

/// Acceptance: a book whose CSS sets a paragraph margin keeps it.
#[test]
fn a_paragraph_margin_in_the_book_stays_in_the_epub() {
    let epub = write(&read(EVERY_VARIANT), "p { margin: 0 0 1em }");
    assert!(epub.warnings.is_empty(), "{:?}", epub.warnings);
    let sheet = entry(&epub.bytes, "EPUB/book.css");
    let reset = sheet.find("margin: 0;").expect("the reset");
    let author = sheet
        .find("p {\n  margin: 0 0 1em;")
        .expect("the author rule");
    assert!(reset < author, "{sheet}");
}

/// A file the host cannot hand over warns, and the run goes on
/// without it.
#[test]
fn a_missing_image_warns_and_is_left_out() {
    let book = read("![A plate](plate.png)\n\nThe text goes on.\n");
    let epub = write(&book, "");
    assert_eq!(epub.warnings.len(), 1, "{:?}", epub.warnings);
    assert!(epub.warnings[0].message.contains("plate.png"));
    assert_eq!(epub.warnings[0].origin.as_deref(), Some("levant.md:1:1"));
    let one = entry(&epub.bytes, "EPUB/section-001.xhtml");
    assert!(!one.contains("<img"), "{one}");
    assert!(one.contains("The text goes on."));
}

/// Acceptance: `cover` in the metadata names an image, and its
/// manifest item carries `cover-image`. An image the book shows as
/// well is one item.
#[test]
fn the_cover_names_an_image_and_its_item_carries_cover_image() {
    let mut book = read("![A printer's ornament](ornament.png)\n\nThe text.\n");
    book.metadata
        .extra
        .insert("cover".into(), "ornament.png".into());
    let epub = write(&book, "");
    assert!(epub.warnings.is_empty(), "{:?}", epub.warnings);
    let package = entry(&epub.bytes, "EPUB/package.opf");
    let item = "<item id=\"media-1\" href=\"media/image-1.png\" media-type=\"image/png\" properties=\"cover-image\"/>";
    assert!(package.contains(item), "{package}");
    assert_eq!(package.matches("media/image-1.png").count(), 1, "{package}");

    let mut only = read("The text.\n");
    only.metadata
        .extra
        .insert("cover".into(), "ornament.png".into());
    let package = entry(&write(&only, "").bytes, "EPUB/package.opf");
    assert!(package.contains(item), "{package}");
}

/// Acceptance: a `cover` that names no image the host sent warns,
/// and the EPUB has no cover.
#[test]
fn a_cover_the_host_did_not_send_warns_and_the_epub_has_none() {
    let mut book = read("The text.\n");
    book.metadata
        .extra
        .insert("cover".into(), "jacket.png".into());
    let epub = write(&book, "");
    assert_eq!(epub.warnings.len(), 1, "{:?}", epub.warnings);
    assert_eq!(
        epub.warnings[0].message,
        "Cover image jacket.png did not load. It is left out of the EPUB."
    );
    let package = entry(&epub.bytes, "EPUB/package.opf");
    assert!(!package.contains("cover-image"), "{package}");
    assert!(!package.contains("media/"), "{package}");
}

/// Acceptance: the crate names no type from the stages that lay a
/// book out. It reads the content tree and the sheets, and nothing
/// downstream of them.
#[test]
fn the_writer_names_nothing_that_lays_a_book_out() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for entry in std::fs::read_dir(&src).expect("the sources are there") {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        for stage in [
            "layout", "lines", "linebox", "pages", "pdf", "session", "wire", "fonts",
        ] {
            assert!(
                !text.contains(&format!("fleuron::{stage}")),
                "{} names fleuron::{stage}",
                path.display()
            );
        }
    }
    let manifest =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    assert!(!manifest.contains("krilla"), "{manifest}");
}
