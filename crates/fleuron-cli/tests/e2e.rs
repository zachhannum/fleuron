//! The e2e definition: the fixture book through the CLI, and the PDF
//! that comes back validated three ways — structure, text round-trip,
//! page count.
//!
//! Author CSS travels the same path, and is validated the same three
//! ways: `fixtures/styled.css` restyles the same book — a different
//! trim, mirrored margins, a head and a folio on the opening page —
//! and the PDF that comes back is checked for all of it.
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

/// Pages the fixture book sets under `fixtures/styled.css`: a smaller
/// trim and a larger body, so more of them.
const STYLED_PAGES: usize = 31;

/// The trim `fixtures/styled.css` asks for, in points, as `pdfinfo`
/// reports it.
const STYLED_TRIM: &str = "396 x 612 pts";

/// The head that sheet paints in the opening page's top margin box.
const STYLED_HEAD: &str = "STYLED BY FLEURON";

/// SHA-256 of the fixture book's PDF under the built-in sheet alone.
/// Layout is deterministic, so these bytes are a fact about the
/// pipeline: a digest that moves is a change someone meant to make.
const DEFAULT_PDF: &str = "c2345dfe50aedd4ffcebc4da16bea5ec5ce922fc0bd482134cbbcaa5a1d843fc";

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

/// The built-in sheet writes the bytes it is checked in as writing.
#[test]
fn the_default_sheet_writes_the_checked_in_bytes() {
    let (pdf, _) = render("identical", &[]);
    let bytes = std::fs::read(&pdf).expect("the CLI wrote its output");
    assert_eq!(
        sha256(&bytes),
        DEFAULT_PDF,
        "the fixture book's PDF is not the one checked in",
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
        DEFAULT_PDF,
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
        DEFAULT_PDF,
        "a sheet the engine ignored changed the output",
    );
}

/// The author sheet's `@page` reaches the trim: the PDF's pages are
/// the size the stylesheet asked for, not the default's.
#[test]
fn the_styled_pdf_takes_its_trim_from_at_page() {
    let (pdf, _) = render("styled-trim", &[&styled_sheet()]);
    let Some(info) = pdf_info(&pdf) else {
        return;
    };
    let size = info
        .lines()
        .find_map(|line| line.strip_prefix("Page size:"))
        .expect("pdfinfo reports a page size")
        .trim();
    assert_eq!(size, STYLED_TRIM, "the trim is not the sheet's");
}

/// The page masters paint what the sheet asked: the opening page
/// carries the head and its own folio — which the built-in sheet
/// blinds — and no later page carries the head.
#[test]
fn page_masters_paint_what_the_author_asked() {
    let (styled, _) = render("styled-masters", &[&styled_sheet()]);
    let (plain, _) = render("plain-masters", &[]);
    let (Some(styled), Some(plain)) = (extract_text(&styled), extract_text(&plain)) else {
        return;
    };

    let heads = styled.matches(STYLED_HEAD).count();
    assert_eq!(heads, 1, "the head belongs on the one chapter opening");
    assert!(
        pages_of(&styled)[0].contains(STYLED_HEAD),
        "the head is not on the opening page",
    );

    assert_eq!(
        folio_of(pages_of(&styled)[0]),
        Some("1".to_string()),
        "the author's folio rule did not outrank the built-in blinding",
    );
    assert_eq!(
        folio_of(pages_of(&styled)[1]),
        Some("2".to_string()),
        "the second page lost its folio",
    );
    assert_eq!(
        folio_of(pages_of(&plain)[0]),
        None,
        "the built-in sheet should blind the opening page's folio",
    );
}

/// Restyling moves the prose across different pages without losing a
/// word of it.
#[test]
fn the_styled_pdf_holds_every_word_of_the_book() {
    let (pdf, _) = render("styled-text", &[&styled_sheet()]);
    let Some(text) = extract_text(&pdf) else {
        return;
    };
    let rendered = squeeze(&strip_furniture(&text, Some(STYLED_HEAD)));
    let expected = squeeze(&laid_out_text(&fixture_book()));
    if rendered != expected {
        panic!(
            "the styled PDF's prose is not the book's: {}",
            first_difference(&expected, &rendered)
        );
    }
}

/// The styled book is a different book on the page: more of them, and
/// still structurally whole.
#[test]
fn the_styled_page_count_is_what_the_layout_says() {
    let (pdf, stderr) = render("styled-pages", &[&styled_sheet()]);
    assert!(
        stderr.contains(&format!("{STYLED_PAGES} pages")),
        "the run did not report its page count: {stderr}",
    );
    assert_ne!(
        STYLED_PAGES, EXPECTED_PAGES,
        "the sheet should change the pagination",
    );
    if let Some(text) = extract_text(&pdf) {
        assert_eq!(pages_of(&text).len(), STYLED_PAGES);
    }
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

/// `@font-face` resolves through the host, which here is the CLI: a
/// `src` it can open is loaded silently, and one it cannot is a
/// warning over a PDF that was written anyway.
#[test]
fn font_faces_resolve_through_the_host_and_say_when_they_cannot() {
    let fonts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fleuron/fonts");
    let roman = fonts.join("EBGaramond-VF.ttf");
    let italic = fonts.join("EBGaramond-Italic-VF.ttf");
    assert!(
        roman.exists() && italic.exists(),
        "the faces are checked in"
    );

    let resolved = write_sheet(
        "face-found",
        &format!(
            "@font-face {{ font-family: \"Host Serif\"; src: url(\"{}\") }}\n\
             @font-face {{ font-family: \"Host Serif\"; font-style: italic; \
             src: url(\"{}\") }}\n\
             p {{ font-family: \"Host Serif\", serif }}\n",
            roman.display(),
            italic.display(),
        ),
    );
    let (pdf, stderr) = render("face-found", &[resolved.as_path()]);
    assert!(
        !stderr.contains("warning"),
        "a face the host resolved should say nothing: {stderr}",
    );
    if let Some(fonts) = tool("pdffonts", &[pdf.as_os_str()]) {
        let listed = String::from_utf8_lossy(&fonts.stdout);
        assert!(
            listed.contains("EBGaramond"),
            "no embedded face in:\n{listed}",
        );
    }

    let missing = write_sheet(
        "face-missing",
        "@font-face { font-family: \"Nowhere\"; src: url(\"nowhere.ttf\") }\n\
         p { font-family: \"Nowhere\", serif }\n",
    );
    let (pdf, stderr) = render("face-missing", &[missing.as_path()]);
    assert!(
        stderr.contains("no source resolved"),
        "an unresolved face should say so: {stderr}",
    );
    assert_eq!(
        sha256(&std::fs::read(&pdf).expect("the CLI wrote its output")),
        DEFAULT_PDF,
        "falling back to the bundled face should lay the book out unchanged",
    );
}

/// Emphasis is a face, not a slant: the fixture book's italic
/// passages embed the italic cut beside the roman, and the prose
/// still comes back through `pdftotext` with both of them there.
#[test]
fn emphasis_embeds_a_second_face_and_keeps_every_word() {
    let (pdf, _) = render("emphasis", &[]);
    if let Some(fonts) = tool("pdffonts", &[pdf.as_os_str()]) {
        let listed = String::from_utf8_lossy(&fonts.stdout);
        for cut in ["EBGaramond-Regular", "EBGaramond-Italic"] {
            assert!(listed.contains(cut), "no {cut} in:\n{listed}");
        }
    }
    let Some(text) = extract_text(&pdf) else {
        return;
    };
    assert_eq!(
        squeeze(&strip_furniture(&text, None)),
        squeeze(&laid_out_text(&fixture_book())),
        "the italic passages did not survive the round trip",
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
    let rendered = squeeze(&strip_furniture(&text, None));
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

/// The author stylesheet the styled run is driven with.
fn styled_sheet() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/styled.css")
}

/// What `pdfinfo` says about a PDF, or `None` when it is not
/// installed.
fn pdf_info(pdf: &Path) -> Option<String> {
    let run = tool("pdfinfo", &[pdf.as_os_str()])?;
    assert!(run.status.success(), "pdfinfo failed");
    Some(String::from_utf8_lossy(&run.stdout).into_owned())
}

/// Extracted text split into its pages. `pdftotext` ends every page
/// with a form feed, including the last, so the tail is not a page.
fn pages_of(text: &str) -> Vec<&str> {
    let mut pages: Vec<&str> = text.split('\u{c}').collect();
    if pages.last().is_some_and(|tail| tail.trim().is_empty()) {
        pages.pop();
    }
    pages
}

/// The folio one extracted page carries: its last non-empty line,
/// when that line is only digits.
fn folio_of(page: &str) -> Option<String> {
    let last = page.lines().rfind(|line| !line.trim().is_empty())?;
    let last = last.trim();
    last.chars()
        .all(|c| c.is_ascii_digit())
        .then(|| last.to_string())
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

/// Drops each page's furniture — its folio, and the running head when
/// the sheet paints one — leaving the prose the book supplied.
fn strip_furniture(text: &str, head: Option<&str>) -> String {
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
        if let Some(head) = head {
            lines.retain(|line| line.trim() != head);
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
