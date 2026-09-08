//! The e2e definition: the fixture manuscript through the CLI, and
//! the PDF that comes back validated three ways — structure, text
//! round-trip, page count.
//!
//! Markdown in, a PDF out, which is the path a reader walks.
//!
//! Author CSS travels the same path, and is validated the same three
//! ways: `fixtures/styled.css` restyles the same book — a different
//! trim, mirrored margins, a head and a folio on the opening page —
//! and the PDF that comes back is checked for all of it.
//!
//! The book has a map and an ornament in it, so the same run covers
//! what images do to a PDF: a JPEG embedded as it arrived, a PNG's
//! transparency kept as a soft mask, and `qpdf --check` clean over
//! both.
//!
//! Structure and text need `qpdf` and `pdftotext`. Where a tool is
//! missing its check is skipped; setting `FLEURON_E2E_REQUIRE_TOOLS`
//! makes the absence a failure, which is how CI keeps the checks from
//! quietly ceasing to run.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use fleuron::content::{Block, Book, Inline};
use fleuron::images::{Assets, ImageLoader};
use fleuron::pages::{DrawItem, Page, Side};
use fleuron::style::Color;
use fleuron_markdown::Options;

/// The fixture is checked in and layout is deterministic, so the page
/// count is a fact about the pipeline, not a range.
const EXPECTED_PAGES: usize = 23;

/// Pages the fixture book sets under `fixtures/styled.css`: a smaller
/// trim and a larger body, so more of them.
const STYLED_PAGES: usize = 36;

/// The trim `fixtures/styled.css` asks for, in points, as `pdfinfo`
/// reports it.
const STYLED_TRIM: &str = "396 x 612 pts";

/// What the built-in sheet sets a thematic break in.
const ORNAMENT: &str = "\u{2766}";

/// SHA-256 of the fixture book's display structure under the built-in
/// sheet alone, encoded without the wire version in front of it, so
/// the digest is layout's own. Layout is deterministic, so these
/// bytes are a fact about the pipeline: a digest that moves is a
/// change someone meant to make.
///
/// The display structure rather than the PDF, because a PDF's object
/// numbering is the writer's own. krilla orders its font objects by
/// a hash taken over the build's dependency graph, so one
/// book comes out under two numberings on two build configurations,
/// and what the engine decided is the same under both.
const DEFAULT_DISPLAY_LIST: &str =
    "07e8908104e6b1300acde86ce6956864b7cf71fb36e355f3e331ca24d4830696";

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

/// The built-in sheet lays the book out to the display structure it is
/// checked in as laying it out to.
#[test]
fn the_default_sheet_lays_out_the_checked_in_display_list() {
    assert_eq!(
        sha256(&fixture_display_list()),
        DEFAULT_DISPLAY_LIST,
        "the fixture book's display structure is not the one checked in",
    );
}

/// The CLI run twice writes one file: nothing between the manuscript
/// and the bytes reads a clock or a hash of an address.
#[test]
fn two_runs_of_the_cli_write_the_same_pdf() {
    let (first, _) = render("twice-one", &[]);
    let (second, _) = render("twice-two", &[]);
    assert_eq!(
        std::fs::read(&first).expect("the CLI wrote its output"),
        std::fs::read(&second).expect("the CLI wrote its output"),
        "two runs over one book wrote two files",
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
        bytes,
        default_pdf("larger-body"),
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
        std::fs::read(&pdf).expect("the CLI wrote its output"),
        default_pdf("ignored-sheet"),
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

/// The page masters paint what the sheet asked: the opening page gets
/// its own folio, which the built-in sheet blinds.
#[test]
fn page_masters_paint_what_the_author_asked() {
    let (styled, _) = render("styled-masters", &[&styled_sheet()]);
    let (plain, _) = render("plain-masters", &[]);
    let (Some(styled), Some(plain)) = (extract_text(&styled), extract_text(&plain)) else {
        return;
    };

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

/// The furniture an author asks for reaches the PDF: a running head
/// naming the chapter each page belongs to, and folios counted in
/// roman.
#[test]
fn running_heads_and_roman_folios_reach_the_pdf() {
    let sheet = write_sheet(
        "furniture",
        "@page :left  { @top-left  { content: string(chapter); font-size: 8pt } }\n\
         @page :right { @top-right { content: string(chapter); font-size: 8pt } }\n\
         @page { @bottom-center { content: counter(page, lower-roman) } }\n",
    );
    let (pdf, _) = render("furniture", &[sheet.as_path()]);
    let Some(text) = extract_text(&pdf) else {
        return;
    };
    let pages = pages_of(&text);
    assert_eq!(pages.len(), EXPECTED_PAGES);

    let chapter = squeeze(&chapter_title(&fixture_book()));
    for (index, page) in pages.iter().enumerate().skip(1) {
        let first = page
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or_default();
        assert_eq!(
            squeeze(first),
            chapter,
            "page {} has no running head",
            index + 1,
        );
    }

    let folios: Vec<Option<String>> = pages
        .iter()
        .take(5)
        .map(|page| {
            page.lines()
                .rfind(|line| !line.trim().is_empty())
                .map(|line| line.trim().to_string())
        })
        .collect();
    assert_eq!(
        folios,
        ["i", "ii", "iii", "iv", "v"]
            .map(|numeral| Some(numeral.to_string()))
            .to_vec(),
        "the folios did not count in roman",
    );
}

/// The display-typography book: a title transformed to capitals, a
/// chapter title and a chapter's opening line in the face's own small
/// capitals, a small-capital running head and tracking throughout,
/// all through the CLI.
///
/// What a PDF can be asked is what a reader gets back, and what a
/// reader gets back is the manuscript: the transform and the small
/// capitals change which glyphs are drawn and neither changes a word.
/// That the drawn glyphs are the right ones is asked of the two
/// painters together, in the bindings' browser run.
#[test]
fn the_display_typography_book_extracts_as_it_was_written() {
    let source = fixtures().join("display-typography.md");
    let sheet = fixtures().join("display-typography.css");
    let (pdf, stderr) = run(
        "display-typography",
        &[source.as_path()],
        &[sheet.as_path()],
    );
    assert!(
        !stderr.contains("unsupported"),
        "the sheet is in the subset the engine honours: {stderr}",
    );

    if let Some(check) = tool("qpdf", &["--check".as_ref(), pdf.as_os_str()]) {
        assert!(
            check.status.success(),
            "qpdf --check: {}{}",
            String::from_utf8_lossy(&check.stdout),
            String::from_utf8_lossy(&check.stderr),
        );
    }

    let Some(text) = extract_text(&pdf) else {
        return;
    };
    // The title is drawn in capitals and the chapter title in small
    // capitals. Neither is what the author wrote, and what comes back
    // is what the author wrote.
    for written in [
        "A Voyage to Lilliput",
        "The Author Gives Some Account of Himself",
        "My father had a small estate in Nottinghamshire",
    ] {
        assert!(
            squeeze(&text).contains(&squeeze(written)),
            "the PDF does not read back {written:?} as it was written:\n{text}",
        );
    }
    assert!(
        !squeeze(&text).contains(&squeeze("A VOYAGE TO LILLIPUT")),
        "the PDF reads back the transform rather than the manuscript:\n{text}",
    );

    // The running head is the chapter string, so it comes back in the
    // case the heading set it in rather than the small capitals it is
    // drawn in.
    let pages = pages_of(&text);
    assert!(pages.len() >= 2, "the book runs past its opening page");
    let head = pages[1]
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default();
    assert_eq!(
        squeeze(head),
        squeeze("A Voyage to Lilliput"),
        "page 2 has no running head, or not the one that was written",
    );

    // Every word of the manuscript, through a sheet that changed the
    // case of two headings and the advance of every glyph.
    let book = {
        let markdown = std::fs::read_to_string(&source).expect("the fixture is checked in");
        let name = source.display().to_string();
        let (sections, warnings) =
            fleuron_markdown::to_sections(&markdown, &name, &Options::default());
        assert!(warnings.is_empty(), "the fixture is clean: {warnings:?}");
        fleuron_markdown::assemble(fleuron_markdown::frontmatter(&markdown), sections)
    };
    let rendered = squeeze(&strip_furniture(&text, Some("A Voyage to Lilliput")));
    let expected = squeeze(&laid_out_text(&book));
    if rendered != expected {
        panic!(
            "the PDF's prose is not the book's: {}",
            first_difference(&expected, &rendered)
        );
    }
}

/// The box model through the fixture book: `fixtures/styled.css`
/// puts the excerpt's inventory of the man-mountain's pockets in a
/// bordered, padded, tinted box, and a rule under every chapter
/// title.
///
/// The quotation is long enough to carry over a page turn, so the
/// box is resolved on more than one page: each piece paints over the
/// fragments its own page holds, and `box-decoration-break: clone`
/// closes the rule across the break.
#[test]
fn the_styled_book_paints_a_box_around_its_quotation() {
    const TINT: Color = Color::rgb(0xf4, 0xf1, 0xea);
    const INK: Color = Color::rgb(0x8a, 0x7a, 0x5c);

    let pages = styled_pages();
    let tinted: Vec<usize> = pages
        .iter()
        .enumerate()
        .filter(|(_, page)| fills(page, TINT).next().is_some())
        .map(|(index, _)| index)
        .collect();
    assert!(
        tinted.len() >= 2,
        "the quotation has to carry over a page turn: it is on {tinted:?}",
    );

    for index in &tinted {
        let page = &pages[*index];
        let (x, y, w, h, _) = fills(page, TINT).next().expect("the tint was found");
        // The tint is painted before the text it sits behind, and the
        // text sits inside it.
        let first = page
            .items
            .iter()
            .position(|item| matches!(item, DrawItem::Text { .. }));
        let tint = page
            .items
            .iter()
            .position(|item| matches!(item, DrawItem::Rect { color, .. } if *color == TINT));
        assert!(tint < first, "page {index}: the tint paints over the text");
        let inside = page.items.iter().any(|item| match item {
            DrawItem::Text {
                x: at, y: baseline, ..
            } => *at > x && *at < x + w && *baseline > y && *baseline < y + h,
            _ => false,
        });
        assert!(inside, "page {index}: nothing is set inside the box");

        // `clone` closes the rule on both pieces: four edges, whether
        // the page holds the whole quotation or a piece of it.
        let edges = fills(page, INK)
            .filter(|(rect_x, rect_y, rect_w, rect_h, _)| {
                *rect_x >= x - 1.0
                    && *rect_x + *rect_w <= x + w + 1.0
                    && *rect_y >= y - 1.0
                    && *rect_y + *rect_h <= y + h + 1.0
            })
            .count();
        assert_eq!(edges, 4, "page {index}: the box is not closed");
    }

    // The rule under a chapter title sits below its baseline and
    // above the prose the padding under it moved down.
    let opening = &pages[0];
    let title = opening
        .items
        .iter()
        .find_map(|item| match item {
            DrawItem::Text { y, text, .. } if text.contains("CHAPTER") => Some(*y),
            _ => None,
        })
        .expect("the chapter opens with its title");
    let rule = fills(opening, INK)
        .find(|(_, rule_y, rule_w, rule_h, _)| *rule_y > title && rule_w > rule_h)
        .expect("a rule under the chapter title");
    assert_eq!(rule.3, 1.0, "the rule is the width the sheet asked for");

    // And the same ink reaches the PDF: the tint behind the quotation
    // and the rule around it are both filled there.
    let (pdf, _) = render("box", &[&styled_sheet()]);
    let Some(filled) = fill_colours(&pdf) else {
        return;
    };
    for color in [TINT, INK] {
        let written = format!(
            "{} {} {} rg",
            channel(color.r),
            channel(color.g),
            channel(color.b)
        );
        assert!(
            filled.contains(&written),
            "the PDF fills nothing in {}: {filled}",
            color.to_hex(),
        );
    }
}

/// The sheet names one picture and moves it, and the picture beside
/// it stays where the built-in sheet put it. The class is written in
/// the manuscript, so this is the whole path: an attribute run in
/// markdown, a class selector in the author's CSS, one image on the
/// page set somewhere else.
#[test]
fn the_named_plate_is_set_where_the_sheet_put_it() {
    // What `fixtures/styled.css` leaves around the text: the wider
    // margin is the spine, so which edge is which follows the side
    // the picture landed on.
    const SPINE: f32 = 60.0;
    const FORE_EDGE: f32 = 40.0;

    let pages = styled_pages();
    let images: Vec<(Side, f32, f32, f32)> = pages
        .iter()
        .flat_map(|page| {
            page.items.iter().filter_map(|item| match item {
                DrawItem::Image { x, w, .. } => Some((page.side, page.width, *x, *w)),
                _ => None,
            })
        })
        .collect();
    let [plate, ornament] = images.as_slice() else {
        panic!("the fixture book has a map and an ornament: {images:?}");
    };

    let (side, width, x, w) = *plate;
    let far = match side {
        Side::Verso => width - SPINE,
        Side::Recto => width - FORE_EDGE,
    };
    assert!(w < far, "the plate fills the measure: {plate:?}");
    assert!(
        (x + w - far).abs() < 0.5,
        "the named plate is not against the far edge: {plate:?}",
    );

    let (side, _, x, _) = *ornament;
    let near = match side {
        Side::Verso => FORE_EDGE,
        Side::Recto => SPINE,
    };
    assert!(
        (x - near).abs() < 0.5,
        "the picture the sheet did not name moved: {ornament:?}",
    );
}

/// One channel as a PDF writes it: krilla's own rounding of a byte
/// into the unit interval.
fn channel(value: u8) -> String {
    format!("{}", (f64::from(value) / 255.0) as f32)
}

/// A PDF's content streams, uncompressed, or `None` when `qpdf` is
/// not installed.
fn fill_colours(pdf: &Path) -> Option<String> {
    let expanded = pdf.with_extension("qdf.pdf");
    let run = tool(
        "qpdf",
        &[
            "--qdf".as_ref(),
            "--object-streams=disable".as_ref(),
            pdf.as_os_str(),
            expanded.as_os_str(),
        ],
    )?;
    assert!(run.status.success(), "qpdf --qdf failed");
    let bytes = std::fs::read(&expanded).expect("qpdf wrote its output");
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Every filled rect of one colour on a page, in paint order.
fn fills(page: &Page, wanted: Color) -> impl Iterator<Item = (f32, f32, f32, f32, Color)> {
    page.items.iter().filter_map(move |item| match item {
        DrawItem::Rect { x, y, w, h, color } if *color == wanted => Some((*x, *y, *w, *h, *color)),
        _ => None,
    })
}

/// The fixture book laid out under `fixtures/styled.css`, the way the
/// CLI lays it out.
fn styled_pages() -> Vec<Page> {
    let registry = fleuron::fonts::bundled_registry().expect("the bundled face parses");
    let book = fixture_book();
    let css = std::fs::read_to_string(styled_sheet()).expect("the sheet is checked in");
    let sheets =
        fleuron::style::Stylesheets::parse(&[fleuron::style::Source::author("styled.css", &css)]);
    let styles = sheets.compile(&book, &registry);
    assert!(
        styles.warnings().is_empty(),
        "the sheet is in the subset: {:?}",
        styles.warnings(),
    );
    let assets = Assets::probe(&book, &Beside);
    fleuron::layout::layout_book(&book, &styles, &registry, &assets).pages
}

/// The heading the fixture book's one chapter opens with, which the
/// built-in sheet sets the `chapter` running string from.
fn chapter_title(book: &Book) -> String {
    let mut title = String::new();
    if let Some(Block::Heading { inlines, .. }) = book.sections[0].blocks.first() {
        append_inlines(inlines, &mut title);
    }
    assert!(!title.is_empty(), "the fixture opens with a heading");
    title
}

/// Restyling moves the prose across different pages without losing a
/// word of it.
#[test]
fn the_styled_pdf_holds_every_word_of_the_book() {
    let (pdf, _) = render("styled-text", &[&styled_sheet()]);
    let Some(text) = extract_text(&pdf) else {
        return;
    };
    let rendered = squeeze(&strip_furniture(&text, None));
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
        std::fs::read(&pdf).expect("the CLI wrote its output"),
        default_pdf("face-missing"),
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

/// The map is embedded as the file it came in as. PDF's `DCTDecode`
/// is the JPEG stream itself, so the bytes in the file are the bytes
/// on disk, verbatim, and a writer that re-encoded one would spend
/// the quality for nothing.
#[test]
fn the_fixture_jpeg_embeds_byte_for_byte() {
    let (pdf, _) = render("images", &[]);
    let bytes = std::fs::read(&pdf).expect("the CLI wrote its output");
    let map = std::fs::read(fixture_path().with_file_name("images/plate.jpg"))
        .expect("the map is checked in");
    assert!(
        bytes.windows(map.len()).any(|window| window == map),
        "the map's {} bytes are not in the PDF as they went in",
        map.len(),
    );
    let readable: String = bytes.iter().map(|b| *b as char).collect();
    assert!(readable.contains("/DCTDecode"), "the map was re-encoded");
    // The ornament's ground is transparent, and stays that way.
    assert!(readable.contains("/SMask"), "the ornament lost its alpha");
}

/// The document information dictionary names the book: the
/// frontmatter's title and author, the engine as producer, and the
/// book's own date rather than the hour the run started.
#[test]
fn the_document_info_names_the_fixture_book() {
    let (pdf, _) = render("info", &[]);
    let bytes = std::fs::read(&pdf).expect("the CLI wrote its output");
    let readable: String = bytes.iter().map(|b| *b as char).collect();
    // Read off the bytes rather than through a reader: 1726 is
    // before the epoch a viewer converts dates through, and more
    // than one of them prints the wrong century for it.
    assert!(
        readable.contains("/CreationDate (D:17261028"),
        "the creation date is not the book's own",
    );
    let Some(info) = pdf_info(&pdf) else {
        return;
    };
    let field = |name: &str| {
        info.lines()
            .find_map(|line| line.strip_prefix(&format!("{name}:")))
            .map(str::trim)
            .unwrap_or_default()
            .to_string()
    };
    assert_eq!(field("Title"), "Gulliver's Travels");
    assert_eq!(field("Author"), "Jonathan Swift");
    assert_eq!(field("Producer"), "fleuron");
}

/// A page box divided in two, with a rule down the gutter: the
/// column properties the same book is set under, checked the same
/// three ways.
const COLUMNS_CSS: &str = "@page {\n  column-count: 2;\n  column-gap: 18pt;\n  column-rule-style: solid;\n  column-rule-width: 0.5pt;\n}\n";

/// Pages the fixture book sets in two columns.
const COLUMN_PAGES: usize = 25;

/// The fixture book in two columns: structurally sound, every word of
/// it still there, and the page count the layout settled.
#[test]
fn a_two_column_book_reaches_the_pdf() {
    let sheet = write_sheet("columns", COLUMNS_CSS);
    let (pdf, stderr) = render("columns", &[&sheet]);
    assert!(
        !stderr.contains("warning"),
        "the column sheet is in the subset: {stderr}",
    );
    assert!(
        stderr.contains(&format!("{COLUMN_PAGES} pages")),
        "the run did not report its page count: {stderr}",
    );
    assert_ne!(
        COLUMN_PAGES, EXPECTED_PAGES,
        "dividing the page box should change the pagination",
    );
    if let Some(text) = extract_reading_order(&pdf) {
        assert_eq!(pages_of(&text).len(), COLUMN_PAGES);
        // Reading order joins the halves of a word a line broke at a
        // hyphen and drops the hyphen with it, so the comparison is
        // over text with none.
        let rendered = unhyphenated(&strip_folios(&text));
        let expected = unhyphenated(&laid_out_text(&fixture_book()));
        if rendered != expected {
            panic!(
                "the two-column PDF's prose is not the book's: {}",
                first_difference(&expected, &rendered)
            );
        }
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

/// The rule down the gutter is in the display structure the preview
/// paints from, one rect per filled gutter, and the export fills the
/// same rect in the PDF.
#[test]
fn the_column_rule_reaches_the_display_structure_and_the_pdf() {
    let pages = column_pages();
    let filled = pages
        .iter()
        .filter(|page| {
            page.items
                .iter()
                .any(|item| matches!(item, DrawItem::Rect { .. }))
        })
        .count();
    assert!(
        filled > 1,
        "only {filled} page(s) of the two-column book paint a rule",
    );
    for (index, page) in pages.iter().enumerate() {
        let rules: Vec<_> = fills(page, Color::BLACK).collect();
        assert!(
            rules.len() <= 1,
            "page {index} paints {} rules",
            rules.len()
        );
        let Some((x, y, w, h, _)) = rules.first().copied() else {
            continue;
        };
        assert_eq!(w, 0.5, "page {index}: the rule is not the width asked for");
        // The rule divides: something is set on either side of it.
        let sides = |left: bool| {
            page.items.iter().any(|item| match item {
                DrawItem::Text {
                    x: at, y: baseline, ..
                } => (*at < x) == left && *baseline >= y && *baseline <= y + h,
                _ => false,
            })
        };
        assert!(
            sides(true) && sides(false),
            "page {index}: the rule divides nothing"
        );
    }
    // The export paints the same rect: the PDF fills a path that
    // starts at the corner the display structure put the rule at.
    let sheet = write_sheet("columns-rule", COLUMNS_CSS);
    let (pdf, _) = render("columns-rule", &[&sheet]);
    let Some(written) = fill_colours(&pdf) else {
        return;
    };
    let (x, y, ..) = pages
        .iter()
        .find_map(|page| fills(page, Color::BLACK).next())
        .expect("a page paints a rule");
    let corner = format!("{x} {y} m");
    assert!(
        written.contains(&corner),
        "the PDF paints no rule at {corner}",
    );
}

/// The fixture book laid out in two columns, the way the CLI lays it
/// out under the same sheet.
fn column_pages() -> Vec<Page> {
    let registry = fleuron::fonts::bundled_registry().expect("the bundled face parses");
    let book = fixture_book();
    let sheets = fleuron::style::Stylesheets::parse(&[fleuron::style::Source::author(
        "columns.css",
        COLUMNS_CSS,
    )]);
    let styles = sheets.compile(&book, &registry);
    assert!(
        styles.warnings().is_empty(),
        "the sheet is in the subset: {:?}",
        styles.warnings(),
    );
    let assets = Assets::probe(&book, &Beside);
    fleuron::layout::layout_book(&book, &styles, &registry, &assets).pages
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

/// The manuscript reads clean, and its frontmatter is metadata
/// rather than the book's opening lines.
#[test]
fn the_manuscript_reads_clean_and_sets_no_frontmatter() {
    let (pdf, stderr) = render("manuscript", &[]);
    assert!(
        !stderr.contains("warning"),
        "the excerpt is clean: {stderr}"
    );
    let Some(text) = extract_text(&pdf) else {
        return;
    };
    assert!(
        text.contains("Lilliput"),
        "the manuscript's prose is not in the PDF",
    );
    assert!(
        !text.contains("title: Gulliver"),
        "the frontmatter was set as prose",
    );
}

/// The tree the frontend read is readable without a PDF in between,
/// and two dumps of one manuscript are the same bytes.
#[test]
fn dump_tree_emits_a_stable_tree() {
    let dumped = dump_tree();
    let tree: serde_json::Value =
        serde_json::from_str(&dumped).expect("the dump is a JSON content tree");
    assert_eq!(
        tree["metadata"]["title"], "Gulliver's Travels",
        "the frontmatter did not reach the tree",
    );
    let blocks = tree["sections"][0]["blocks"]
        .as_array()
        .expect("the section has blocks");
    assert!(
        blocks.iter().any(|block| block["type"] == "thematic_break"),
        "the scene break is not in the tree",
    );
    assert!(!dumped.contains("\"id\""), "ids do not travel");
    assert_eq!(dumped, dump_tree(), "the dump moved between runs");
}

/// The tree the CLI writes for the fixture manuscript.
fn dump_tree() -> String {
    let run = Command::new(env!("CARGO_BIN_EXE_fleuron"))
        .arg(fixture_path())
        .arg("--dump-tree")
        .output()
        .expect("the CLI runs");
    assert!(
        run.status.success(),
        "the CLI failed: {}",
        String::from_utf8_lossy(&run.stderr),
    );
    String::from_utf8(run.stdout).expect("the CLI writes UTF-8")
}

/// Several files compose in the order the command line gives them,
/// and a diagnostic names the file it came from, not the first one and
/// not the run.
#[test]
fn several_markdown_files_compose_in_argument_order() {
    let first = write_source(
        "compose-one",
        "---\ntitle: The Ambassador\n---\n\n# Chapter One\n\nThe first chapter.\n",
    );
    let second = write_source(
        "compose-two",
        "# Chapter Two\n\nThe second chapter.\n\n- a list item\n",
    );
    let (pdf, stderr) = run("composed", &[&first, &second], &[]);

    let warnings: Vec<&str> = stderr
        .lines()
        .filter(|line| line.contains("warning:"))
        .collect();
    assert_eq!(warnings.len(), 1, "{stderr}");
    assert!(
        warnings[0].contains("compose-two.md:5:1") && warnings[0].contains("a list"),
        "the diagnostic names the wrong source: {}",
        warnings[0],
    );

    let Some(text) = extract_text(&pdf) else {
        return;
    };
    let first_at = text.find("The first chapter").expect("chapter one is set");
    let second_at = text.find("The second chapter").expect("chapter two is set");
    assert!(first_at < second_at, "the files composed out of order");
    assert!(text.contains("a list item"), "the list lost its prose");

    // Reversed on the command line, reversed on the page.
    let (pdf, _) = run("composed-reversed", &[&second, &first], &[]);
    let Some(text) = extract_text(&pdf) else {
        return;
    };
    assert!(
        text.find("The second chapter") < text.find("The first chapter"),
        "argument order did not decide reading order",
    );
}

/// A chapter's frontmatter is the chapter's. The book is named on the
/// command line, not by whichever chapter came first.
#[test]
fn a_multi_file_book_is_named_on_the_command_line() {
    let first = write_source(
        "meta-one",
        "---\ntitle: The Ambassador\nstatus: draft\n---\n\nHe arrived on a Tuesday.\n",
    );
    let second = write_source(
        "meta-two",
        "---\ntitle: A Cold Reception\nstatus: revised\n---\n\nNobody met him at the gate.\n",
    );

    // Unnamed: nothing is promoted to the book, and nothing warns about
    // two chapters disagreeing over a title neither was claiming.
    let (pdf, stderr) = run_with("unnamed", &[&first, &second], &[], &["-s", "none"]);
    assert!(!stderr.contains("warning"), "{stderr}");
    if let Some(info) = pdf_info(&pdf) {
        assert!(
            !info.contains("The Ambassador"),
            "a chapter named the book:\n{info}",
        );
    }

    let (pdf, stderr) = run_with(
        "named",
        &[&first, &second],
        &[],
        &[
            "-s",
            "none",
            "--title",
            "The Levant Papers",
            "--author",
            "E. Marsh",
        ],
    );
    assert!(!stderr.contains("warning"), "{stderr}");
    let Some(info) = pdf_info(&pdf) else {
        return;
    };
    assert!(info.contains("The Levant Papers"), "no title:\n{info}");
    assert!(info.contains("E. Marsh"), "no author:\n{info}");
}

/// A source for the CLI to read, beside the PDFs.
fn write_source(name: &str, markdown: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.md"));
    std::fs::write(&path, markdown).expect("the source is writable");
    path
}

/// Runs the fixture book through the CLI exactly as the epic's
/// definition of done words it, and returns the PDF and what the run
/// had to say.
fn render(name: &str, css: &[&Path]) -> (PathBuf, String) {
    run(name, &[&fixture_path()], css)
}

/// The CLI, on whatever inputs and sheets the caller names.
fn run(name: &str, inputs: &[&Path], css: &[&Path]) -> (PathBuf, String) {
    run_with(name, inputs, css, &[])
}

/// The same, with further flags after the inputs.
fn run_with(name: &str, inputs: &[&Path], css: &[&Path], flags: &[&str]) -> (PathBuf, String) {
    let output = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.pdf"));
    let mut command = Command::new(env!("CARGO_BIN_EXE_fleuron"));
    command.args(inputs).arg("-o").arg(&output).args(flags);
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

/// The fixture book's PDF under the built-in sheet alone: the
/// baseline for a test that asserts some input changed nothing.
fn default_pdf(name: &str) -> Vec<u8> {
    let (pdf, _) = render(&format!("{name}-baseline"), &[]);
    std::fs::read(&pdf).expect("the CLI wrote its output")
}

/// The fixture book's display structure, built the way the CLI builds it:
/// the built-in sheet alone, and the images resolved against the
/// manuscript's own directory.
fn fixture_display_list() -> Vec<u8> {
    let registry = fleuron::fonts::bundled_registry().expect("the bundled face parses");
    let book = fixture_book();
    let styles = fleuron::style::Stylesheets::parse(&[]).compile(&book, &registry);
    let assets = Assets::probe(&book, &Beside);
    let output = fleuron::layout::layout_book(&book, &styles, &registry, &assets);
    postcard::to_stdvec(&output).expect("a display structure encodes")
}

/// Image urls resolved the way the CLI resolves them: against the
/// directory the manuscript was read from.
struct Beside;

impl ImageLoader for Beside {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        std::fs::read(fixture_path().with_file_name(url)).ok()
    }
}

/// The digest the fixture PDF is checked against, lowercase hex.
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

/// The folio on one extracted page: its last non-empty line, when
/// that line is only digits.
fn folio_of(page: &str) -> Option<String> {
    let last = page.lines().rfind(|line| !line.trim().is_empty())?;
    let last = last.trim();
    last.chars()
        .all(|c| c.is_ascii_digit())
        .then(|| last.to_string())
}

fn fixture_path() -> PathBuf {
    fixtures().join("gulliver-excerpt.md")
}

/// Where the checked-in manuscripts and sheets live.
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// The tree the CLI lays out, read the way the CLI reads it.
fn fixture_book() -> Book {
    let text = std::fs::read_to_string(fixture_path()).expect("the fixture book is checked in");
    let source = fixture_path().display().to_string();
    let (sections, warnings) = fleuron_markdown::to_sections(&text, &source, &Options::default());
    assert!(warnings.is_empty(), "the excerpt is clean: {warnings:?}");
    fleuron_markdown::assemble(fleuron_markdown::frontmatter(&text), sections)
}

/// The PDF's text, or `None` when `pdftotext` is not installed.
///
/// `-layout` keeps each line's own words together: without it poppler
/// rejoins words broken across lines and swallows the hyphen.
fn extract_text(pdf: &Path) -> Option<String> {
    extract_with(pdf, &["-layout"])
}

/// The same in reading order rather than physical layout, which is
/// how a reader walks a page whose content box is divided: down one
/// column, then down the next.
fn extract_reading_order(pdf: &Path) -> Option<String> {
    extract_with(pdf, &[])
}

fn extract_with(pdf: &Path, flags: &[&str]) -> Option<String> {
    let mut args: Vec<&std::ffi::OsStr> = flags.iter().map(AsRef::as_ref).collect();
    args.push(pdf.as_os_str());
    args.push("-".as_ref());
    let run = tool("pdftotext", &args)?;
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

/// Drops the folio from every page wherever it stands. Reading order
/// puts a centred folio between the columns it sits under rather than
/// at the foot of the page.
fn strip_folios(text: &str) -> String {
    let mut prose = String::new();
    for (index, page) in text.split('\u{c}').enumerate() {
        let folio = (index + 1).to_string();
        for line in page.lines().filter(|line| line.trim() != folio) {
            prose.push_str(line);
            prose.push('\n');
        }
    }
    prose
}

/// Everything the engine lays out, in reading order: headings,
/// paragraphs, the blocks a blockquote nests, and the ornament the
/// built-in sheet sets a thematic break in. An image has no text.
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
            Block::Blockquote { blocks, .. } => append_blocks(blocks, text),
            Block::ThematicBreak { .. } => text.push_str(ORNAMENT),
            Block::Image { .. } => {}
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

/// The same with the hyphens taken out too.
fn unhyphenated(text: &str) -> String {
    squeeze(text).replace('-', "")
}
