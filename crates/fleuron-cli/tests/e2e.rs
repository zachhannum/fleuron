//! The e2e definition: the fixture book through the CLI, and the PDF
//! that comes back validated three ways — structure, text round-trip,
//! page count.
//!
//! Author CSS travels the same path: a stylesheet on the command line
//! reaches layout, and one that asks for something outside the subset
//! is reported without stopping the run.
//!
//! Structure and text need `qpdf` and `pdftotext`. Where a tool is
//! missing its check is skipped; setting `FLEURON_E2E_REQUIRE_TOOLS`
//! makes the absence a failure, which is how CI keeps the checks from
//! quietly ceasing to run.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use fleuron::content::{Block, Book, Inline};

/// The fixture is checked in and layout is deterministic, so the page
/// count is a fact about the pipeline, not a range.
const EXPECTED_PAGES: usize = 20;

/// SHA-256 of the fixture book's PDF as v0.1 wrote it, before styling
/// became a compiled stylesheet. The built-in sheet says what the
/// constants said, so the bytes have not moved.
const V0_1_PDF: &str = "03a92971e3a5924e1fd4fd6d5bf85c9efb10b6353a58078c3cb2ca06233bb927";

#[test]
fn the_fixture_book_renders_a_pdf() {
    let (pdf, stderr) = render("renders", &[]);
    let bytes = std::fs::read(&pdf).expect("the CLI wrote its output");
    assert!(bytes.starts_with(b"%PDF-"), "no PDF header");
    assert!(
        bytes.ends_with(b"%%EOF\n") || bytes.ends_with(b"%%EOF"),
        "no PDF trailer"
    );
    assert!(
        stderr.contains(&format!("{EXPECTED_PAGES} pages")),
        "the run did not report its page count: {stderr}",
    );
}

/// The built-in sheet alone reproduces v0.1 byte for byte: the
/// defaults moved into CSS without moving a glyph.
#[test]
fn the_default_sheet_writes_the_pdf_v0_1_wrote() {
    let (pdf, _) = render("identical", &[]);
    let bytes = std::fs::read(&pdf).expect("the CLI wrote its output");
    assert_eq!(
        sha256(&bytes),
        V0_1_PDF,
        "the fixture book's PDF is no longer v0.1's",
    );
}

/// Author CSS reaches layout through the command line, and changes
/// what comes out.
#[test]
fn author_css_reaches_the_pdf() {
    let sheet = write_sheet("author", "book { font-size: 14pt }\n");
    let (pdf, stderr) = render("styled", &[sheet.as_path()]);
    let bytes = std::fs::read(&pdf).expect("the CLI wrote its output");
    assert_ne!(
        sha256(&bytes),
        V0_1_PDF,
        "a larger body size changed nothing",
    );
    let pages: usize = stderr
        .split_whitespace()
        .zip(stderr.split_whitespace().skip(1))
        .find_map(|(count, unit)| (unit == "pages").then(|| count.parse().ok())?)
        .expect("the run reports its page count");
    assert!(
        pages > EXPECTED_PAGES,
        "{pages} pages at 14pt, {EXPECTED_PAGES} at 11pt",
    );
}

/// CSS outside the subset is a diagnostic naming where it was
/// written, and the PDF is written anyway.
#[test]
fn unsupported_css_is_reported_and_the_run_continues() {
    let sheet = write_sheet("unsupported", "p {\n  text-shadow: 0 0 2px black;\n}\n");
    let (pdf, stderr) = render("warned", &[sheet.as_path()]);
    assert!(
        stderr.contains("unsupported property `text-shadow`"),
        "no diagnostic for text-shadow: {stderr}",
    );
    assert!(stderr.contains(":2:3"), "no source position: {stderr}");
    assert_eq!(
        sha256(&std::fs::read(&pdf).expect("the CLI wrote its output")),
        V0_1_PDF,
        "a sheet the engine ignored changed the output",
    );
}

#[test]
fn the_pdf_is_structurally_sound() {
    let (pdf, _) = render("structure", &[]);
    let Some(check) = tool("qpdf", &["--check".as_ref(), pdf.as_os_str()]) else {
        return;
    };
    assert!(
        check.status.success(),
        "qpdf --check: {}{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr),
    );
}

#[test]
fn the_pdf_holds_every_word_of_the_book() {
    let (pdf, _) = render("text", &[]);
    let Some(text) = extract_text(&pdf) else {
        return;
    };
    let rendered = squeeze(&strip_folios(&text));
    let expected = squeeze(&laid_out_text(&fixture_book()));
    assert!(!expected.is_empty(), "the fixture has no prose to check");
    if rendered != expected {
        panic!(
            "the PDF's prose is not the book's: {}",
            first_difference(&expected, &rendered)
        );
    }
}

#[test]
fn the_page_count_is_what_the_layout_says() {
    let (pdf, _) = render("pages", &[]);
    if let Some(text) = extract_text(&pdf) {
        assert_eq!(
            text.matches('\u{c}').count(),
            EXPECTED_PAGES,
            "pdftotext found a different number of pages",
        );
    }
    let Some(npages) = tool("qpdf", &["--show-npages".as_ref(), pdf.as_os_str()]) else {
        return;
    };
    assert!(npages.status.success(), "qpdf --show-npages failed");
    let counted: usize = String::from_utf8_lossy(&npages.stdout)
        .trim()
        .parse()
        .expect("qpdf reports a page count");
    assert_eq!(counted, EXPECTED_PAGES);
}

/// Runs the fixture book through the CLI exactly as the epic's
/// definition of done words it, and returns the PDF and what the run
/// had to say.
fn render(name: &str, css: &[&Path]) -> (PathBuf, String) {
    let output = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.pdf"));
    let mut command = Command::new(env!("CARGO_BIN_EXE_fleuron"));
    command.arg(fixture_path()).arg("-o").arg(&output);
    for sheet in css {
        command.arg("-c").arg(sheet);
    }
    let run = command.output().expect("the CLI runs");
    let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
    assert!(run.status.success(), "the CLI failed: {stderr}");
    (output, stderr)
}

/// A stylesheet on disk for the CLI to read, beside the PDFs.
fn write_sheet(name: &str, css: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.css"));
    std::fs::write(&path, css).expect("the sheet is writable");
    path
}

/// The digest the fixture PDF is held to, lowercase hex.
fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/book.json")
}

fn fixture_book() -> Book {
    let text = std::fs::read_to_string(fixture_path()).expect("the fixture book is checked in");
    serde_json::from_str(&text).expect("the fixture book parses")
}

/// The PDF's text, or `None` when `pdftotext` is not installed.
///
/// `-layout` keeps each line's own words together: without it poppler
/// rejoins words broken across lines and swallows the hyphen.
fn extract_text(pdf: &Path) -> Option<String> {
    let run = tool(
        "pdftotext",
        &["-layout".as_ref(), pdf.as_os_str(), "-".as_ref()],
    )?;
    assert!(run.status.success(), "pdftotext failed");
    Some(String::from_utf8(run.stdout).expect("pdftotext writes UTF-8"))
}

/// Runs a validation tool, or reports it missing — `None` when it is
/// absent and the run tolerates that.
fn tool(name: &str, args: &[&std::ffi::OsStr]) -> Option<Output> {
    match Command::new(name).args(args).output() {
        Ok(output) => Some(output),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            assert!(
                std::env::var_os("FLEURON_E2E_REQUIRE_TOOLS").is_none(),
                "{name} is required here and is not installed",
            );
            eprintln!("e2e: {name} is not installed; skipping its check");
            None
        }
        Err(e) => panic!("{name}: {e}"),
    }
}

/// Drops each page's folio, leaving the prose the book supplied.
fn strip_folios(text: &str) -> String {
    let mut prose = String::new();
    for (index, page) in text.split('\u{c}').enumerate() {
        let mut lines: Vec<&str> = page.lines().collect();
        while lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.pop();
        }
        if lines
            .last()
            .is_some_and(|line| line.trim() == (index + 1).to_string())
        {
            lines.pop();
        }
        prose.push_str(&lines.join("\n"));
        prose.push('\n');
    }
    prose
}

/// Everything v0.1 lays out, in reading order: headings and
/// paragraphs. Blockquotes, images and thematic breaks reach the box
/// tree and stop there, so the PDF is not expected to carry them.
fn laid_out_text(book: &Book) -> String {
    let mut text = String::new();
    for section in &book.sections {
        append_blocks(&section.blocks, &mut text);
    }
    text
}

fn append_blocks(blocks: &[Block], text: &mut String) {
    for block in blocks {
        match block {
            Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => {
                append_inlines(inlines, text);
            }
            Block::Blockquote { .. } | Block::ThematicBreak { .. } | Block::Image { .. } => {}
        }
    }
}

fn append_inlines(inlines: &[Inline], text: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text { value, .. } | Inline::Code { value, .. } => text.push_str(value),
            Inline::Emphasis { children, .. }
            | Inline::Strong { children, .. }
            | Inline::Link { children, .. } => append_inlines(children, text),
        }
    }
}

/// Where two texts part company, with enough either side to read.
fn first_difference(expected: &str, rendered: &str) -> String {
    let at = expected
        .chars()
        .zip(rendered.chars())
        .position(|(e, r)| e != r)
        .unwrap_or(expected.chars().count().min(rendered.chars().count()));
    let window =
        |text: &str| -> String { text.chars().skip(at.saturating_sub(40)).take(80).collect() };
    format!(
        "at character {at}\n  book: {}\n   pdf: {}",
        window(expected),
        window(rendered),
    )
}

/// Text with every space taken out. Line breaking decides where the
/// spaces fall — it splits `council-chamber` across two lines and runs
/// adjacent inlines together — so whitespace is the one thing a round
/// trip cannot compare. Everything else is compared character for
/// character, which is stricter than counting words.
fn squeeze(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}
