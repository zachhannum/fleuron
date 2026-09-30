//! Reflowable EPUB for fleuron: a content tree and its stylesheets
//! in, an EPUB 3 out.
//!
//! A reflowable EPUB is XHTML and CSS in a zip, and the reading
//! system breaks the lines and makes the pages. So the writer reads
//! the top of the pipeline, the content tree and the sheets, and lays
//! nothing out. What the engine decides about pages does not reach
//! the device: this is a function over a tree, not a third way out
//! of a laid-out book beside the PDF and the preview.
//!
//! ```
//! use fleuron::content::{Block, Book, HeadingLevel, Inline, Section};
//! use fleuron::images::NoImages;
//! use fleuron::style::{NoFonts, Source, Stylesheets};
//!
//! let book = Book {
//!     sections: vec![Section {
//!         blocks: vec![Block::Paragraph {
//!             id: Default::default(),
//!             inlines: vec![Inline::Text {
//!                 id: Default::default(),
//!                 value: "My father had a small estate.".into(),
//!                 attributes: Default::default(),
//!                 position: None,
//!                 span: None,
//!             }],
//!             attributes: Default::default(),
//!             position: None,
//!             span: None,
//!         }],
//!         ..Default::default()
//!     }],
//!     ..Default::default()
//! };
//! let sheets = Stylesheets::parse(&[Source::author("book.css", "p { color: #333 }")]);
//! let epub = fleuron_epub::write(&book, &sheets, &NoImages, &NoFonts);
//! assert!(epub.bytes.starts_with(b"PK"));
//! assert!(epub.warnings.is_empty());
//! ```

#![deny(missing_docs)]
// Every variant of the content vocabulary has a mapping here, and a
// new one does not compile until it has one.
#![deny(clippy::wildcard_enum_match_arm)]

mod css;
mod media;
mod xhtml;
mod xml;
mod zip;

use fleuron::Warning;
use fleuron::content::{Block, Book, Metadata, text};
use fleuron::images::ImageLoader;
use fleuron::style::{FontLoader, Stylesheets};

use media::{Kind, Resources};
use xhtml::{Heading, Plan};
use zip::{Archive, Method};

/// Where the package lives inside the container.
const PACKAGE: &str = "EPUB";

/// What `dcterms:modified` says when the metadata gives no `modified`
/// key. A fixed date, so that one book always makes the same bytes.
const MODIFIED: &str = "2000-01-01T00:00:00Z";

/// The keys of `Metadata::extra` that have a Dublin Core element of
/// their own, each with its element. `identifier` and `language` are
/// written apart from these, because the package requires them.
const DUBLIN_CORE: &[(&str, &str)] = &[
    ("contributor", "dc:contributor"),
    ("coverage", "dc:coverage"),
    ("date", "dc:date"),
    ("description", "dc:description"),
    ("publisher", "dc:publisher"),
    ("relation", "dc:relation"),
    ("rights", "dc:rights"),
    ("source", "dc:source"),
    ("subject", "dc:subject"),
    ("type", "dc:type"),
];

/// One EPUB, and what writing it had to complain about.
#[derive(Debug, Clone, PartialEq)]
pub struct Epub {
    /// The whole file.
    pub bytes: Vec<u8>,
    /// Every CSS rule, declaration and file that the EPUB leaves out,
    /// and every link that reaches nothing, each with its position
    /// where one exists.
    pub warnings: Vec<Warning>,
}

/// The files of one EPUB, before they go into the zip.
#[derive(Debug, Clone, PartialEq)]
pub struct Files {
    /// Every file in the container, in the order of the zip.
    pub files: Vec<File>,
    /// The paths of the documents in reading order, as the spine of
    /// the package document lists them.
    pub spine: Vec<String>,
    /// The same warnings [`Epub::warnings`] holds.
    pub warnings: Vec<Warning>,
}

impl Files {
    /// The zip that [`write()`] makes of these files.
    pub fn zip(&self) -> Vec<u8> {
        let mut archive = Archive::default();
        for file in &self.files {
            // The reading system reads the media type at a fixed
            // offset, so this entry is not compressed.
            let method = if file.path == "mimetype" {
                Method::Stored
            } else {
                Method::Deflated
            };
            archive.add(&file.path, &file.bytes, method);
        }
        archive.finish()
    }
}

/// One file in the container.
#[derive(Debug, Clone, PartialEq)]
pub struct File {
    /// Where the file is in the container, such as
    /// `EPUB/section-001.xhtml`.
    pub path: String,
    /// The media type, such as `application/xhtml+xml`.
    pub media_type: String,
    /// The bytes, not compressed.
    pub bytes: Vec<u8>,
}

impl File {
    fn new(path: &str, media_type: &str, bytes: Vec<u8>) -> File {
        File {
            path: path.to_string(),
            media_type: media_type.to_string(),
            bytes,
        }
    }
}

/// Writes `book` as a reflowable EPUB 3, styled by `sheets`.
///
/// Each section is one XHTML document, in the order of
/// `book.sections`. The sheets become one stylesheet: the subset the
/// engine reads, less everything that describes a page. What parsing
/// the sheets warned about is among the warnings, and so is each
/// paged rule and declaration an author sheet wrote.
///
/// `images` loads the images the book and the sheets name, and
/// `fonts` the files `@font-face` names. Their bytes go into the
/// container as they are, with a media type read from their first
/// bytes. Nothing is decoded.
pub fn write(
    book: &Book,
    sheets: &Stylesheets,
    images: &dyn ImageLoader,
    fonts: &dyn FontLoader,
) -> Epub {
    let files = files(book, sheets, images, fonts);
    Epub {
        bytes: files.zip(),
        warnings: files.warnings,
    }
}

/// The files [`write()`] puts in the container, one by one and not
/// zipped, in the order the container holds them.
///
/// A host that shows the EPUB in a browser frame loads these files
/// as they are, and does not open the zip.
pub fn files(
    book: &Book,
    sheets: &Stylesheets,
    images: &dyn ImageLoader,
    fonts: &dyn FontLoader,
) -> Files {
    let mut book = book.clone();
    book.assign_node_ids();
    let anchors = book.anchors();
    let plan = Plan::of(&book, &anchors);
    let language = book.metadata.language().map(str::to_string);

    let mut warnings: Vec<Warning> = sheets.warnings().to_vec();
    let mut resources = Resources::default();

    let stylesheet = css::stylesheet(
        sheets,
        &mut css::Context {
            resources: &mut resources,
            images,
            fonts,
            warnings: &mut warnings,
        },
    );

    let mut documents = Vec::new();
    {
        let mut cx = xhtml::Context {
            plan: &plan,
            anchors: &anchors,
            language: language.as_deref(),
            resources: &mut resources,
            images,
            warnings: &mut warnings,
        };
        for (doc, section) in book.sections.iter().enumerate() {
            let title = section
                .title
                .clone()
                .or_else(|| first_heading(&section.blocks))
                .unwrap_or_else(|| format!("Section {}", doc + 1));
            documents.push(xhtml::document(section, &title, &mut cx));
        }
    }
    let mut hrefs = plan.hrefs.clone();
    // The spine has to hold a document, so an empty book is one empty
    // page of text.
    if documents.is_empty() {
        let mut empty = String::new();
        xhtml::head(
            &mut empty,
            "Section 1",
            language.as_deref(),
            Some("book.css"),
        );
        empty.push_str("<body>\n</body>\n</html>\n");
        documents.push(empty);
        hrefs.push("section-001.xhtml".to_string());
    }

    let cover = cover(&book.metadata, &mut resources, images, &mut warnings);

    let headings = xhtml::headings(&book, &plan);
    let navigation = navigation(&book, &headings, &hrefs, language.as_deref());
    let package = package(
        &book.metadata,
        &book,
        &hrefs,
        &resources,
        cover.as_deref(),
        language.as_deref(),
    );

    let mut files = vec![
        File::new("mimetype", "text/plain", b"application/epub+zip".to_vec()),
        File::new(
            "META-INF/container.xml",
            "application/xml",
            container().into_bytes(),
        ),
        File::new(
            &format!("{PACKAGE}/package.opf"),
            "application/oebps-package+xml",
            package.into_bytes(),
        ),
        File::new(
            &format!("{PACKAGE}/nav.xhtml"),
            "application/xhtml+xml",
            navigation.into_bytes(),
        ),
        File::new(
            &format!("{PACKAGE}/book.css"),
            "text/css",
            stylesheet.into_bytes(),
        ),
    ];
    let spine: Vec<String> = hrefs
        .iter()
        .map(|href| format!("{PACKAGE}/{href}"))
        .collect();
    for (path, document) in spine.iter().zip(documents) {
        files.push(File::new(
            path,
            "application/xhtml+xml",
            document.into_bytes(),
        ));
    }
    for file in resources.into_files() {
        files.push(File::new(
            &format!("{PACKAGE}/{}", file.href),
            file.media_type,
            file.bytes,
        ));
    }
    Files {
        files,
        spine,
        warnings,
    }
}

/// The path of the image that the `cover` key names, or `None` with
/// a warning when the host sent no such image.
fn cover(
    metadata: &Metadata,
    resources: &mut Resources,
    images: &dyn ImageLoader,
    warnings: &mut Vec<Warning>,
) -> Option<String> {
    let url = metadata
        .extra
        .get("cover")
        .map(|url| url.trim())
        .filter(|url| !url.is_empty())?;
    match resources.resolve(url, Kind::Image, |url| images.load(url)) {
        Ok(href) => Some(href.to_string()),
        Err(refused) => {
            warnings.push(Warning {
                message: xhtml::refusal("Cover image", url, refused),
                origin: None,
            });
            None
        }
    }
}

/// The words of a section's first heading, for a document with no
/// title of its own.
fn first_heading(blocks: &[Block]) -> Option<String> {
    blocks.iter().find_map(|block| match block {
        Block::Heading { inlines, .. } => {
            let title = text(inlines)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            (!title.is_empty()).then_some(title)
        }
        Block::Paragraph { .. }
        | Block::Blockquote { .. }
        | Block::CodeBlock { .. }
        | Block::ThematicBreak { .. }
        | Block::PageBreak { .. }
        | Block::ColumnBreak { .. }
        | Block::Image { .. }
        | Block::List { .. }
        | Block::Table { .. } => None,
    })
}

fn container() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">
  <rootfiles>
    <rootfile full-path=\"{PACKAGE}/package.opf\" media-type=\"application/oebps-package+xml\"/>
  </rootfiles>
</container>
"
    )
}

/// The navigation document: the headings, nested by level. A book
/// with no headings lists its sections instead.
fn navigation(
    book: &Book,
    headings: &[Heading],
    hrefs: &[String],
    language: Option<&str>,
) -> String {
    let mut entries: Vec<Heading> = headings.to_vec();
    if entries.is_empty() {
        entries = hrefs
            .iter()
            .enumerate()
            .map(|(doc, href)| Heading {
                level: 1,
                title: book
                    .sections
                    .get(doc)
                    .and_then(|section| section.title.clone())
                    .unwrap_or_else(|| format!("Section {}", doc + 1)),
                href: href.clone(),
            })
            .collect();
    }
    let mut out = String::new();
    xhtml::head(&mut out, "Contents", language, None);
    out.push_str(
        "<body>\n<nav epub:type=\"toc\" role=\"doc-toc\" id=\"toc\">\n<h1>Contents</h1>\n",
    );
    outline(&mut out, &entries);
    out.push_str("</nav>\n</body>\n</html>\n");
    out
}

/// One `ol` of the outline: each entry takes the entries after it
/// that are deeper, up to the next one at its level or above.
fn outline(out: &mut String, entries: &[Heading]) {
    out.push_str("<ol>\n");
    let mut at = 0;
    while at < entries.len() {
        let entry = &entries[at];
        let deeper = entries[at + 1..]
            .iter()
            .take_while(|next| next.level > entry.level)
            .count();
        out.push_str("<li><a href=\"");
        xml::attribute(out, &entry.href);
        out.push_str("\">");
        xml::text(out, &entry.title);
        out.push_str("</a>");
        if deeper > 0 {
            out.push('\n');
            outline(out, &entries[at + 1..at + 1 + deeper]);
        }
        out.push_str("</li>\n");
        at += 1 + deeper;
    }
    out.push_str("</ol>\n");
}

/// The package document: the metadata, every file in the container,
/// and the reading order.
fn package(
    metadata: &Metadata,
    book: &Book,
    hrefs: &[String],
    resources: &Resources,
    cover: Option<&str>,
    language: Option<&str>,
) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"book-id\">\n<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n",
    );
    let identifier = metadata
        .extra
        .get("identifier")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| identifier(book));
    element(
        &mut out,
        "dc:identifier id=\"book-id\"",
        "dc:identifier",
        &identifier,
    );
    let title = metadata
        .title
        .clone()
        .or_else(|| book.sections.iter().find_map(|s| first_heading(&s.blocks)))
        .unwrap_or_else(|| "Untitled".to_string());
    element(&mut out, "dc:title", "dc:title", &title);
    element(
        &mut out,
        "dc:language",
        "dc:language",
        language.unwrap_or("und"),
    );
    if let Some(author) = &metadata.author {
        element(&mut out, "dc:creator", "dc:creator", author);
    }
    for (key, name) in DUBLIN_CORE {
        if let Some(value) = metadata.extra.get(*key).filter(|v| !v.trim().is_empty()) {
            element(&mut out, name, name, value);
        }
    }
    let modified = metadata
        .extra
        .get("modified")
        .map(String::as_str)
        .unwrap_or(MODIFIED);
    element(
        &mut out,
        "meta property=\"dcterms:modified\"",
        "meta",
        modified,
    );
    out.push_str("</metadata>\n<manifest>\n");
    out.push_str(
        "<item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n",
    );
    out.push_str("<item id=\"css\" href=\"book.css\" media-type=\"text/css\"/>\n");
    for (doc, href) in hrefs.iter().enumerate() {
        out.push_str(&format!(
            "<item id=\"section-{}\" href=\"{href}\" media-type=\"application/xhtml+xml\"/>\n",
            doc + 1
        ));
    }
    for (index, file) in resources.files().iter().enumerate() {
        let properties = if cover == Some(file.href.as_str()) {
            " properties=\"cover-image\""
        } else {
            ""
        };
        out.push_str(&format!(
            "<item id=\"media-{}\" href=\"{}\" media-type=\"{}\"{properties}/>\n",
            index + 1,
            file.href,
            file.media_type
        ));
    }
    out.push_str("</manifest>\n<spine>\n");
    for doc in 0..hrefs.len() {
        out.push_str(&format!("<itemref idref=\"section-{}\"/>\n", doc + 1));
    }
    out.push_str("</spine>\n</package>\n");
    out
}

fn element(out: &mut String, open: &str, close: &str, value: &str) {
    out.push('<');
    out.push_str(open);
    out.push('>');
    xml::text(out, value);
    out.push_str("</");
    out.push_str(close);
    out.push_str(">\n");
}

/// An identifier for a book that names none: a UUID made from the
/// book itself, so it is the same on every run.
fn identifier(book: &Book) -> String {
    let tree = format!("{:?}{:?}", book.metadata, book.sections);
    let high = fnv(tree.as_bytes(), 0xcbf2_9ce4_8422_2325);
    let low = fnv(tree.as_bytes(), 0x6c62_272e_07bb_0142);
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&high.to_be_bytes());
    bytes[8..].copy_from_slice(&low.to_be_bytes());
    // A version 8 UUID: custom bits, in the RFC 9562 layout.
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "urn:uuid:{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn fnv(bytes: &[u8], basis: u64) -> u64 {
    let mut hash = basis;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}
