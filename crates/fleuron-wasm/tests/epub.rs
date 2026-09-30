//! A session writes its book as an EPUB: the same bytes the writer
//! makes over what the session holds, with no stage run to make them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fleuron::images::ImageLoader;
use fleuron::style::{FontLoader, Source, Stylesheets};
use fleuron::wire;
use fleuron_markdown::Options;
use fleuron_wasm::Session;

const BOOK: &str = "---
title: A Voyage to Lilliput
language: en
---

# Chapter One

My father had a small estate in Nottinghamshire. I was the third of
five sons.

![An ornament](ornament.png)

# Chapter Two

He sent me to Emanuel College in Cambridge at fourteen years old.
";

const CSS: &str = "@font-face { font-family: Fell; src: url(fell.ttf) }
p { font-family: Fell, serif }
h1 { background-image: url(plate.jpg) }
";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn fixture(path: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(path)).expect("the fixture is checked in")
}

/// What the host sends, by the url it sends it under.
struct Files(BTreeMap<&'static str, Vec<u8>>);

impl Files {
    fn new() -> Files {
        Files(BTreeMap::from([
            ("ornament.png", fixture("images/fleuron.png")),
            ("plate.jpg", fixture("images/plate.jpg")),
            ("fell.ttf", fixture("fonts/IMFellEnglishSC-Regular.ttf")),
        ]))
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

/// A session over the book, the sheet, and every file the two name.
fn session(markdown: &str, css: &str) -> Session {
    let files = Files::new();
    let mut session = Session::new().unwrap();
    session.set_markdown("lilliput.md", markdown);
    session
        .set_style(vec!["author.css".into()], vec![css.into()])
        .unwrap();
    for url in ["ornament.png", "plate.jpg"] {
        session.add_image(url, &files.0[url]).unwrap();
    }
    session
        .add_font_file("fell.ttf", &files.0["fell.ttf"])
        .unwrap();
    session
}

fn export(session: &Session) -> wire::Epub {
    wire::decode_epub(&session.export_epub().unwrap()).expect("the reply reads back")
}

/// The entries of a zip archive, by name, inflated.
fn entries(archive: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let end = archive.len() - 22;
    assert_eq!(&archive[end..end + 4], b"PK\x05\x06", "no end record");
    let le16 = |at: usize| u16::from_le_bytes([archive[at], archive[at + 1]]) as usize;
    let le32 = |at: usize| u32::from_le_bytes(archive[at..at + 4].try_into().unwrap()) as usize;
    let mut at = le32(end + 16);
    let mut out = BTreeMap::new();
    for _ in 0..le16(end + 10) {
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
        out.insert(name, bytes);
        at += 46 + name_len;
    }
    out
}

/// Acceptance: `want: 'epub'` returns the bytes `fleuron_epub::write`
/// makes for the session's book.
#[test]
fn the_reply_holds_what_the_writer_makes_for_the_book() {
    let session = session(BOOK, CSS);
    let (sections, _) = fleuron_markdown::to_sections(BOOK, "lilliput.md", &Options::default());
    let book = fleuron_markdown::assemble(fleuron_markdown::frontmatter(BOOK), sections);
    let files = Files::new();
    let mut sheets = Stylesheets::parse(&[Source::author("author.css", CSS)]);
    sheets.load_fonts(&mut fleuron::fonts::bundled_registry().unwrap(), &files);
    let written = fleuron_epub::write(&book, &sheets, &files, &files);

    let reply = export(&session);
    assert!(reply.bytes.starts_with(b"PK"), "that is not a zip");
    assert_eq!(reply.bytes, written.bytes);
    assert_eq!(reply.warnings, written.warnings);
}

/// Acceptance: the request runs no layout stage, whether or not the
/// book was laid out before it.
#[test]
fn an_epub_runs_no_stage() {
    let mut session = session(BOOK, CSS);
    let set = session.stages();
    export(&session);
    assert_eq!(session.stages(), set, "an EPUB of a book never laid out");

    session.preview(None, None).unwrap();
    session.update_markdown("lilliput.md", &BOOK.replace("fourteen", "fifteen"));
    let edited = session.stages();
    let reply = export(&session);
    assert_eq!(session.stages(), edited, "an EPUB after an edit");
    let documents = entries(&reply.bytes);
    let second = String::from_utf8(documents["EPUB/section-002.xhtml"].clone()).unwrap();
    assert!(second.contains("fifteen"), "the edit is not in\n{second}");
}

/// Acceptance: the images and the fonts the host sent are in the
/// container, as the host sent them.
#[test]
fn the_files_the_host_sent_are_in_the_container() {
    let reply = export(&session(BOOK, CSS));
    let held: Vec<Vec<u8>> = entries(&reply.bytes)
        .into_iter()
        .filter(|(name, _)| name.starts_with("EPUB/media/"))
        .map(|(_, bytes)| bytes)
        .collect();
    let files = Files::new();
    for (url, bytes) in &files.0 {
        assert!(held.contains(bytes), "{url} is not in the container");
    }
    assert_eq!(held.len(), files.0.len());
    assert!(reply.warnings.is_empty(), "{:?}", reply.warnings);
}

/// Acceptance: the writer's warnings come back on the channel a
/// render's warnings use, beside what reading the sources said.
#[test]
fn the_warnings_come_back_as_a_render_s_do() {
    let markdown = BOOK.replace("small estate", "small <b>estate</b>");
    let css = format!("{CSS}p {{ float: left }}\nh1 {{ background-image: url(gone.png) }}\n");
    let mut session = session(&markdown, &css);
    let reply = export(&session);
    let rendered = wire::decode(&session.preview(None, None).unwrap())
        .unwrap()
        .warnings;

    let read = rendered
        .iter()
        .find(|w| w.message.contains("Inline HTML"))
        .expect("reading the source warns on the render");
    assert_eq!(reply.warnings.first(), Some(read), "{:?}", reply.warnings);
    assert!(
        reply.warnings.iter().any(|w| w.message.contains("`float`")
            && w.origin.as_deref() == Some("author.css:4:5")),
        "{:?}",
        reply.warnings
    );
    assert!(
        reply
            .warnings
            .iter()
            .any(|w| w.message.contains("gone.png")),
        "{:?}",
        reply.warnings
    );
}

/// Acceptance: the wire version goes up, and the reply leads with it.
#[test]
fn the_reply_leads_with_the_wire_version() {
    assert_eq!(wire::VERSION, 18);
    let bytes = session(BOOK, CSS).export_epub().unwrap();
    assert_eq!(wire::version(&bytes).unwrap(), fleuron_wasm::wire_version());
}

/// Acceptance: every entry in the zip is in the files reply, with the
/// same path and the same bytes, and the reply has the spine order.
#[test]
fn the_files_reply_holds_every_entry_of_the_zip() {
    let session = session(BOOK, CSS);
    let zipped = export(&session);
    let reply = wire::decode_epub_files(&session.export_epub_files().unwrap())
        .expect("the reply reads back");
    let files: BTreeMap<String, Vec<u8>> = reply
        .files
        .iter()
        .map(|file| (file.path.clone(), file.bytes.clone()))
        .collect();
    assert_eq!(files, entries(&zipped.bytes));
    assert_eq!(files.len(), reply.files.len(), "a path is there twice");
    assert_eq!(reply.warnings, zipped.warnings);
    assert_eq!(
        reply.spine,
        ["EPUB/section-001.xhtml", "EPUB/section-002.xhtml"]
    );
    let font = reply
        .files
        .iter()
        .find(|file| file.bytes == Files::new().0["fell.ttf"])
        .expect("the font is a file");
    assert_eq!(font.media_type, "font/ttf");
}
