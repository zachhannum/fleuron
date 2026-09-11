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
//! both. The ornament's transparency does a second job. The sheet
//! wraps the prose to the shape it traces, so the trace stage runs
//! on the way to the PDF as well.
//!
//! Structure and text need `qpdf` and `pdftotext`. Where a tool is
//! missing its check is skipped; setting `FLEURON_E2E_REQUIRE_TOOLS`
//! makes the absence a failure, which is how CI keeps the checks from
//! quietly ceasing to run.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use fleuron::content::{Block, Book, Inline, Row};
use fleuron::images::{Assets, ImageLoader};
use fleuron::pages::{DrawItem, Page, Side};
use fleuron::style::Color;
use fleuron_markdown::Options;

/// The fixture is checked in and layout is deterministic, so the page
/// count is a fact about the pipeline, not a range.
const EXPECTED_PAGES: usize = 24;

/// Pages the fixture book sets under `fixtures/styled.css`: a smaller
/// trim and a larger body, so more of them.
const STYLED_PAGES: usize = 37;

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
    "5439d996b5c63e8454a6a3b14c895e55e35e54bf264d467136243e51137e3986";

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
        stderr.contains("Unsupported property `text-shadow`. The declaration is ignored."),
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
    let rendered = strip_furniture(&text, Some("A Voyage to Lilliput"));
    if let Err(difference) = holds(&book, &rendered, squeeze, true) {
        panic!("the PDF's prose is not the book's: {difference}");
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
    let Some(filled) = content_streams(&pdf) else {
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

/// The sheet names one image and anchors it to the page. The image
/// beside it stays where the built-in sheet put it. The manuscript
/// holds a brace run, and the author's CSS holds a class selector. One
/// image sits against the page, with the prose of that page wrapped
/// down its right.
#[test]
fn the_named_image_is_set_against_the_page_and_the_prose_wraps() {
    // What `fixtures/styled.css` leaves around the text: the wider
    // margin is the spine, so which edge is which follows the side
    // the image landed on.
    const SPINE: f32 = 60.0;
    const FORE_EDGE: f32 = 40.0;
    const FOOT: f32 = 56.0;
    // The gutter the sheet keeps between the map and the prose.
    const GUTTER: f32 = 14.0;

    let pages = styled_pages();
    let images: Vec<(usize, f32, f32, f32, f32)> = pages
        .iter()
        .enumerate()
        .flat_map(|(index, page)| {
            page.items.iter().filter_map(move |item| match item {
                DrawItem::Image { x, y, w, h, .. } => Some((index, *x, *y, *w, *h)),
                _ => None,
            })
        })
        .collect();
    let [map, ornament] = images.as_slice() else {
        panic!("the fixture book has a map and an ornament: {images:?}");
    };

    // The map sits in the bottom corner of the page area, at the
    // insets the sheet gave it.
    let (index, x, y, w, h) = *map;
    let page = &pages[index];
    let near = match page.side {
        Side::Verso => FORE_EDGE,
        Side::Recto => SPINE,
    };
    assert!(
        (x - near).abs() < 0.5,
        "the map is not at the near edge: {map:?}"
    );
    assert!(
        (y + h - (page.height - FOOT)).abs() < 0.5,
        "the map is not against the foot of the page area: {map:?}",
    );

    // Every line the map reaches is set clear of it, and at least one
    // line reaches it.
    let mut beside = 0;
    for item in &page.items {
        let DrawItem::Text {
            x: run,
            y: baseline,
            ..
        } = item
        else {
            continue;
        };
        if *baseline <= y || *baseline > y + h {
            continue;
        }
        assert!(
            *run >= x + w + GUTTER - 0.5,
            "a line at {baseline} is set over the map: {run} against {}",
            x + w + GUTTER,
        );
        beside += 1;
    }
    assert!(beside > 0, "no line is set beside the map");

    let (index, x, ..) = *ornament;
    let near = match pages[index].side {
        Side::Verso => FORE_EDGE,
        Side::Recto => SPINE,
    };
    assert!(
        (x - near).abs() < 0.5,
        "the ornament is not at the near edge: {ornament:?}",
    );
}

/// The prose beside the ornament wraps to the shape the ornament's
/// own alpha channel traces, rather than to its box.
///
/// The ornament is a floral heart on a clear ground, so its contour
/// leaves the sides of its box empty and the prose sets into them.
/// The same sheet with the contour turned off holds the prose off the
/// whole box, and that is the difference this measures.
#[test]
fn the_prose_wraps_to_the_shape_the_ornament_traces() {
    let traced = beside_the_ornament(&styled_pages());
    let boxed = beside_the_ornament(&styled_pages_with(
        "img:not(.map) { shape-outside: none; shape-margin: 0 }",
    ));
    assert!(!traced.is_empty(), "no line is set beside the ornament");
    assert_eq!(
        traced.len(),
        boxed.len(),
        "the two runs set different lines"
    );
    for ((start, box_right), (off, _)) in traced.iter().zip(&boxed) {
        assert!(
            start < &(box_right - 1.0),
            "a line starts at {start}, off the ornament's whole box",
        );
        assert!(
            off - start > 1.0,
            "the contour did not move the line: {start} against {off}",
        );
    }
}

/// Every line set beside the ornament: where it starts, and where the
/// ornament's own box ends.
fn beside_the_ornament(pages: &[Page]) -> Vec<(f32, f32)> {
    let (page, x, y, w, h) = pages
        .iter()
        .find_map(|page| {
            page.items.iter().find_map(|item| match item {
                // The map is the wider of the two images; the
                // ornament is the one a line's height covers.
                DrawItem::Image { x, y, w, h, .. } if *w < 60.0 => Some((page, *x, *y, *w, *h)),
                _ => None,
            })
        })
        .expect("the ornament is placed");
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Text {
                x: run,
                y: baseline,
                ..
            } if *baseline > y && *baseline <= y + h => Some((*run, x + w)),
            _ => None,
        })
        .collect()
}

/// One channel as a PDF writes it: krilla's own rounding of a byte
/// into the unit interval.
fn channel(value: u8) -> String {
    format!("{}", (f64::from(value) / 255.0) as f32)
}

/// A PDF's content streams, uncompressed, or `None` when `qpdf` is
/// not installed.
fn content_streams(pdf: &Path) -> Option<String> {
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
    styled_pages_with("")
}

/// The same with one more sheet over it, for a test that measures
/// what a rule of the checked-in sheet is doing.
fn styled_pages_with(extra: &str) -> Vec<Page> {
    let registry = fleuron::fonts::bundled_registry().expect("the bundled face parses");
    let book = fixture_book();
    let css = std::fs::read_to_string(styled_sheet()).expect("the sheet is checked in");
    let sheets = fleuron::style::Stylesheets::parse(&[
        fleuron::style::Source::author("styled.css", &css),
        fleuron::style::Source::author("over.css", extra),
    ]);
    let styles = sheets.compile(&book, &registry);
    assert!(
        styles.warnings().is_empty(),
        "the sheet is in the subset: {:?}",
        styles.warnings(),
    );
    let assets = Assets::probe(&book, &styles, &Beside);
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
    // The sheet prints the page a link names after the link, which
    // is text the book does not hold.
    let printed = format!("(page{})", chapter_three_folio(&text));
    let rendered = squeeze(&strip_furniture(&text, None)).replacen(&printed, "", 1);
    if let Err(difference) = holds(&fixture_book(), &rendered, squeeze, true) {
        panic!("the styled PDF's prose is not the book's: {difference}");
    }
}

/// Acceptance: `pdftotext` round-trips the printed number. The fixture
/// book refers from the end of its second chapter to the heading its
/// third opens on, and `fixtures/styled.css` prints that page after
/// the link.
#[test]
fn a_link_prints_the_page_its_chapter_opens_on() {
    let (pdf, stderr) = render("reference", &[&styled_sheet()]);
    assert!(
        !stderr.contains("Nothing is generated"),
        "the reference names nothing: {stderr}",
    );
    let Some(text) = extract_text(&pdf) else {
        return;
    };
    let pages = pages_of(&text);
    let at = pages
        .iter()
        .position(|page| page.contains("CHAPTER III."))
        .expect("chapter III is set");
    let folio = chapter_three_folio(&text);
    assert_eq!(
        folio,
        (at + 1).to_string(),
        "the folio is not the page's own"
    );
    assert!(
        squeeze(&text).contains(&format!("chapterIII(page{folio})")),
        "the PDF does not read back the page the link names:\n{text}",
    );
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
        stderr.contains("No src url loaded for Nowhere."),
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
    if let Err(difference) = holds(
        &fixture_book(),
        &strip_furniture(&text, None),
        squeeze,
        true,
    ) {
        panic!("the italic passages did not survive the round trip: {difference}");
    }
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
const COLUMN_PAGES: usize = 27;

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
        // over text with none. It also reads a table a column at a
        // time, so the table is compared whole.
        if let Err(difference) = holds(&fixture_book(), &strip_folios(&text), unhyphenated, false) {
            panic!("the two-column PDF's prose is not the book's: {difference}");
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
        // The table's rules run across the page, and a column rule
        // runs down it.
        let rules: Vec<_> = fills(page, Color::BLACK)
            .filter(|(_, _, w, h, _)| h > w)
            .collect();
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
    let Some(written) = content_streams(&pdf) else {
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

/// The same page box with the map set against its leading edge: one
/// rectangle over the whole of the first column and the head of the
/// second, with the prose of each column set around its own side of
/// it.
const COLUMNS_WRAPPED_CSS: &str = "@page {\n  column-count: 2;\n  column-gap: 18pt;\n}\n\n.map {\n  position: absolute;\n  top: 0;\n  left: 0;\n  margin: 6pt;\n  wrap-flow: both;\n}\n";

/// Acceptance: the preview and the export agree over a two-column
/// page with the prose wrapped around an image.
///
/// The preview paints from the display structure, so what the two
/// have to agree about is where every run and every image goes. The
/// PDF names each of them once: a text matrix before a run, and an
/// image matrix before an image, both read back in the display
/// structure's own coordinates.
#[test]
fn a_two_column_wrapped_page_paints_the_same_in_the_preview_and_the_pdf() {
    let (pages, styles) = wrapped_column_pages();
    let (index, image) = pages
        .iter()
        .enumerate()
        .find_map(|(index, page)| {
            page.items.iter().find_map(|item| match item {
                DrawItem::Image { x, y, w, h, .. } => Some((index, (*x, *y, *w, *h))),
                _ => None,
            })
        })
        .expect("the map is placed");
    let page = &pages[index];
    let geometry = wrapped_geometry(&styles, page);
    assert_eq!(geometry.column_count(), 2);
    let (x, y, w, h) = image;
    let measure = geometry.measure();
    let second = geometry.column_origin(1).0;

    // The rectangle covers the first column and the head of the
    // second, so the first column is set under it and the second
    // beside it.
    assert!(x + w > second, "the map reaches no second column");
    assert!(x + w < second + measure, "the map covers both columns");
    let mut under = 0;
    let mut beside = 0;
    for item in &page.items {
        let DrawItem::Text {
            x: at, y: baseline, ..
        } = item
        else {
            continue;
        };
        if *baseline > y && *baseline <= y + h {
            assert!(
                *at >= x + w - 1e-3,
                "a line at {baseline} starts at {at}, over the map",
            );
            beside += 1;
        }
        if *baseline > y + h && *at < second {
            under += 1;
        }
    }
    assert!(beside > 0, "no line is set beside the map");
    assert!(under > 0, "the first column set nothing under the map");

    // The export puts the same runs and the same images at the same
    // points as the display structure the preview paints from.
    let sheet = write_sheet("columns-wrapped", COLUMNS_WRAPPED_CSS);
    let (pdf, _) = render("columns-wrapped", &[&sheet]);
    let Some(streams) = content_streams(&pdf) else {
        return;
    };
    let height = page.height;
    assert!(
        pages.iter().all(|page| page.height == height),
        "the pages are not one size, so one flip does not undo them all",
    );
    let painted: Vec<(f32, f32)> = pages
        .iter()
        .flat_map(|page| &page.items)
        .filter_map(|item| match item {
            DrawItem::Text { x, y, .. } => Some((*x, *y)),
            _ => None,
        })
        .collect();
    let written = placed_runs(&streams);
    assert!(written.len() > 100, "the PDF writes {} runs", written.len());
    assert_eq!(
        painted.len(),
        written.len(),
        "the preview paints {} runs and the PDF writes {}",
        painted.len(),
        written.len(),
    );
    for (index, (paints, writes)) in painted.iter().zip(&written).enumerate() {
        assert!(
            (paints.0 - writes.0).abs() < 1e-3 && (paints.1 - writes.1).abs() < 1e-3,
            "run {index}: the preview paints it at {paints:?} and the PDF at {writes:?}",
        );
    }
    let boxes: Vec<(f32, f32, f32, f32)> = pages
        .iter()
        .flat_map(|page| &page.items)
        .filter_map(|item| match item {
            DrawItem::Image { x, y, w, h, .. } => Some((*x, *y, *w, *h)),
            _ => None,
        })
        .collect();
    let placed = placed_images(&streams, height);
    assert_eq!(boxes.len(), 2, "the fixture book has a map and an ornament");
    assert_eq!(boxes.len(), placed.len(), "the two disagree about how many");
    for (index, (paints, writes)) in boxes.iter().zip(&placed).enumerate() {
        for (paints, writes) in [
            (paints.0, writes.0),
            (paints.1, writes.1),
            (paints.2, writes.2),
            (paints.3, writes.3),
        ] {
            assert!(
                (paints - writes).abs() < 1e-3,
                "image {index}: the preview paints {paints:?} and the PDF writes {writes:?}",
            );
        }
    }
}

/// The column sheet with the chapters run together, so a chapter
/// heading falls partway down a page, and the headings and the table
/// set across both columns.
const COLUMNS_SPANNING_CSS: &str = "@page {\n  column-count: 2;\n  column-gap: 18pt;\n  column-rule-style: solid;\n  column-rule-width: 0.5pt;\n}\n\nsection { break-before: auto }\n\nh2, h3, table { column-span: all }\n";

/// The fixture book with its headings and its table across both
/// columns: structurally sound, every word of it still there, and
/// the page count the layout settled.
#[test]
fn a_book_with_spanning_heads_reaches_the_pdf() {
    let sheet = write_sheet("columns-spanning", COLUMNS_SPANNING_CSS);
    let (pdf, stderr) = render("columns-spanning", &[&sheet]);
    assert!(
        !stderr.contains("warning"),
        "the spanning sheet is in the subset: {stderr}",
    );
    let pages = pages_under(COLUMNS_SPANNING_CSS);
    assert!(
        stderr.contains(&format!("{} pages", pages.len())),
        "the run did not report the {} pages the layout settled: {stderr}",
        pages.len(),
    );
    // Reading order guesses a page's blocks from where they sit, and
    // on a page of tiers it reads down the left side past a heading.
    // The writer puts runs in the order the columns fill.
    if let Some(text) = extract_content_order(&pdf) {
        assert_eq!(pages_of(&text).len(), pages.len());
        if let Err(difference) = holds(&fixture_book(), &strip_folios(&text), unhyphenated, true) {
            panic!("the spanning PDF's prose is not the book's: {difference}");
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

/// Acceptance: the preview and the export agree over a page with a
/// spanning heading on it, between a tier of columns above it and a
/// tier below.
///
/// The preview paints from the display structure, so what the two
/// have to agree about is where every run and every rule goes.
#[test]
fn a_page_with_a_spanning_heading_paints_the_same_in_the_preview_and_the_pdf() {
    let pages = pages_under(COLUMNS_SPANNING_CSS);
    let (index, heading) = pages
        .iter()
        .enumerate()
        .find_map(|(index, page)| {
            page.items.iter().find_map(|item| match item {
                DrawItem::Text { text, y, .. } if text.starts_with("CHAPTER II") => {
                    Some((index, *y))
                }
                _ => None,
            })
        })
        .expect("the second chapter has a heading");
    let page = &pages[index];
    assert!(
        page.items
            .iter()
            .any(|item| matches!(item, DrawItem::Text { y, .. } if *y < heading)),
        "the second chapter opens its page, so nothing stands above its heading",
    );
    let rules: Vec<_> = fills(page, Color::BLACK)
        .filter(|(_, _, w, h, _)| h > w)
        .collect();
    assert!(!rules.is_empty(), "the page paints no column rule");
    for (_, y, _, h, _) in &rules {
        assert!(
            y + h < heading || *y > heading,
            "a rule over {y}..{} runs through the heading on {heading}",
            y + h,
        );
    }

    // The export puts the same runs at the same points as the display
    // structure the preview paints from, and fills the same rules.
    let sheet = write_sheet("columns-spanning-preview", COLUMNS_SPANNING_CSS);
    let (pdf, _) = render("columns-spanning-preview", &[&sheet]);
    let Some(streams) = content_streams(&pdf) else {
        return;
    };
    let painted: Vec<(f32, f32)> = pages
        .iter()
        .flat_map(|page| &page.items)
        .filter_map(|item| match item {
            DrawItem::Text { x, y, .. } => Some((*x, *y)),
            _ => None,
        })
        .collect();
    let written = placed_runs(&streams);
    assert_eq!(
        painted.len(),
        written.len(),
        "the preview paints {} runs and the PDF writes {}",
        painted.len(),
        written.len(),
    );
    for (index, (paints, writes)) in painted.iter().zip(&written).enumerate() {
        assert!(
            (paints.0 - writes.0).abs() < 1e-3 && (paints.1 - writes.1).abs() < 1e-3,
            "run {index}: the preview paints it at {paints:?} and the PDF at {writes:?}",
        );
    }
    for (x, y, ..) in &rules {
        let corner = format!("{x} {y} m");
        assert!(
            streams.contains(&corner),
            "the PDF paints no rule at {corner}"
        );
    }
}

/// Every text run a PDF places, as `(x, baseline)` in the display
/// structure's own coordinates, in the order the writer wrote them.
///
/// The writer flips the page once and then sets one text matrix
/// before each run, so the matrix names the point the display
/// structure named.
fn placed_runs(streams: &str) -> Vec<(f32, f32)> {
    streams
        .lines()
        .filter_map(|line| line.trim().strip_suffix(" Tm")?.strip_prefix("1 0 0 -1 "))
        .filter_map(|matrix| {
            let (x, y) = matrix.split_once(' ')?;
            Some((x.parse().ok()?, y.parse().ok()?))
        })
        .collect()
}

/// Every image a PDF places, as `(x, y, width, height)` in the same
/// coordinates, in the same order.
///
/// An image matrix scales as well as it translates, which is what
/// tells one from the flip the page opens with.
fn placed_images(streams: &str, height: f32) -> Vec<(f32, f32, f32, f32)> {
    streams
        .lines()
        .filter_map(|line| line.trim().strip_suffix(" cm"))
        .filter_map(|matrix| {
            let values: Vec<f32> = matrix
                .split(' ')
                .map(|value| value.parse().ok())
                .collect::<Option<_>>()?;
            let [a, b, c, d, x, up] = values[..] else {
                return None;
            };
            (b == 0.0 && c == 0.0 && a > 0.0 && d > 0.0).then_some((x, height - up - d, a, d))
        })
        .collect()
}

/// The fixture book on a divided page box with the map against it,
/// the way the CLI lays it out under the same sheet.
fn wrapped_column_pages() -> (Vec<Page>, fleuron::style::StyleTree) {
    let registry = fleuron::fonts::bundled_registry().expect("the bundled face parses");
    let book = fixture_book();
    let sheets = fleuron::style::Stylesheets::parse(&[fleuron::style::Source::author(
        "columns-wrapped.css",
        COLUMNS_WRAPPED_CSS,
    )]);
    let styles = sheets.compile(&book, &registry);
    assert!(
        styles.warnings().is_empty(),
        "the sheet is in the subset: {:?}",
        styles.warnings(),
    );
    let assets = Assets::probe(&book, &styles, &Beside);
    let pages = fleuron::layout::layout_book(&book, &styles, &registry, &assets).pages;
    (pages, styles)
}

/// The page box one page of that run resolves to.
fn wrapped_geometry(
    styles: &fleuron::style::StyleTree,
    page: &Page,
) -> fleuron::style::PageGeometry {
    styles
        .page(fleuron::style::PageQuery {
            name: Some("chapter"),
            situation: fleuron::style::Situation::Body(page.side),
        })
        .geometry
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
    let assets = Assets::probe(&book, &styles, &Beside);
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

/// Acceptance: every word of the book comes back out of the PDF, and
/// every cell of the table with it, row by row.
#[test]
fn the_pdf_holds_every_word_of_the_book() {
    let book = fixture_book();
    let laid = laid_out(&book);
    assert!(
        laid.iter()
            .any(|part| matches!(part, Laid::Prose(prose) if !prose.is_empty())),
        "the fixture has no prose to check",
    );
    assert!(
        laid.iter()
            .any(|part| matches!(part, Laid::Table { body, .. } if !body.is_empty())),
        "the fixture has no table to check",
    );
    let (pdf, _) = render("text", &[]);
    let Some(text) = extract_text(&pdf) else {
        return;
    };
    if let Err(difference) = holds(&book, &strip_furniture(&text, None), squeeze, true) {
        panic!("the PDF's prose is not the book's: {difference}");
    }
}

/// The fixture book's pages under the built-in sheet and `css`, laid
/// out the way the CLI lays them out.
fn pages_under(css: &str) -> Vec<Page> {
    let registry = fleuron::fonts::bundled_registry().expect("the bundled face parses");
    let book = fixture_book();
    let styles =
        fleuron::style::Stylesheets::parse(&[fleuron::style::Source::author("table.css", css)])
            .compile(&book, &registry);
    assert!(styles.warnings().is_empty(), "{:?}", styles.warnings());
    let assets = Assets::probe(&book, &styles, &Beside);
    fleuron::layout::layout_book(&book, &styles, &registry, &assets).pages
}

/// Where the first run that says `text` starts.
fn run_x(pages: &[Page], text: &str) -> f32 {
    pages
        .iter()
        .flat_map(|page| &page.items)
        .find_map(|item| match item {
            DrawItem::Text { x, text: run, .. } if run == text => Some(*x),
            _ => None,
        })
        .unwrap_or_else(|| panic!("nothing says {text:?}"))
}

/// Acceptance: the alignment the delimiter row wrote reaches the PDF,
/// and a rule in the sheet beats it. The manuscript writes the last
/// column flush right and the first flush left, and
/// `td { text-align: right }` moves the first over to the right too.
/// The export places every run where the display structure does.
#[test]
fn the_table_alignment_reaches_the_pdf_and_a_rule_beats_it() {
    let rule = "td { text-align: right }";
    let written = pages_under("");
    let ruled = pages_under(rule);
    assert_eq!(
        run_x(&written, "A handkerchief"),
        run_x(&ruled, "A handkerchief")
    );
    assert!(
        run_x(&ruled, "The right coat-pocket") > run_x(&written, "The right coat-pocket") + 1.0,
        "the rule did not move the column the manuscript set flush left",
    );

    for (name, css, pages) in [("aligned", "", &written), ("aligned-right", rule, &ruled)] {
        let sheet = write_sheet(name, css);
        let (pdf, _) = render(name, &[&sheet]);
        let Some(streams) = content_streams(&pdf) else {
            return;
        };
        let painted: Vec<(f32, f32)> = pages
            .iter()
            .flat_map(|page| &page.items)
            .filter_map(|item| match item {
                DrawItem::Text { x, y, .. } => Some((*x, *y)),
                _ => None,
            })
            .collect();
        let placed = placed_runs(&streams);
        assert_eq!(painted.len(), placed.len(), "{name}");
        for (index, (paints, writes)) in painted.iter().zip(&placed).enumerate() {
            assert!(
                (paints.0 - writes.0).abs() < 1e-3 && (paints.1 - writes.1).abs() < 1e-3,
                "{name} run {index}: the display structure has it at {paints:?} and the PDF at \
                 {writes:?}",
            );
        }
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
    assert!(
        blocks.iter().any(|block| block["type"] == "table"),
        "the table is not in the tree",
    );
    // A node's number is the engine's and does not travel. The id a
    // manuscript names a node by is a string, and does.
    fn numbered(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(fields) => fields
                .iter()
                .any(|(key, value)| (key == "id" && value.is_number()) || numbered(value)),
            serde_json::Value::Array(items) => items.iter().any(numbered),
            _ => false,
        }
    }
    assert!(!numbered(&tree), "node numbers travel");
    assert!(
        dumped.contains("\"chapter-iii\""),
        "the id the manuscript names chapter III by is not in the tree",
    );
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
        warnings[0].contains("compose-two.md:5:1")
            && warnings[0].contains("Lists are not supported"),
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

/// Acceptance: the sample in `docs/cli/reference.md` is what the CLI
/// prints, message for message.
#[test]
fn the_cli_reference_shows_warnings_the_run_prints() {
    let reference = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/cli/reference.md")
        .canonicalize()
        .expect("the reference page is in the repository");
    let page = std::fs::read_to_string(&reference).expect("the reference page is readable");
    let samples: Vec<&str> = page
        .lines()
        .filter_map(|line| line.strip_prefix("fleuron: warning: "))
        .filter_map(|line| line.split_once(": ").map(|(_, message)| message))
        .collect();
    assert_eq!(samples.len(), 2, "the page stopped showing two warnings");

    let source = write_source("reference-sample", "# Chapter\n\n- one\n- two\n");
    let sheet = write_sheet(
        "reference-sample",
        "p {\n  text-shadow: 0 0 2px black;\n}\n",
    );
    let (_, stderr) = run("reference-sample", &[source.as_path()], &[sheet.as_path()]);
    for sample in samples {
        assert!(
            stderr.contains(sample),
            "the page shows `{sample}`:\n{stderr}"
        );
    }
    assert!(
        stderr.contains("2 warnings. The PDF was written anyway."),
        "the page shows the summary line:\n{stderr}",
    );
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
    let assets = Assets::probe(&book, &styles, &Beside);
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

/// The folio of the page chapter III opens on, which is the page
/// that carries its heading.
fn chapter_three_folio(text: &str) -> String {
    let pages = pages_of(text);
    let at = pages
        .iter()
        .position(|page| page.contains("CHAPTER III."))
        .expect("chapter III is set");
    folio_of(pages[at]).expect("the page chapter III opens on has a folio")
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

/// The same in the order the runs were written to the page.
fn extract_content_order(pdf: &Path) -> Option<String> {
    extract_with(pdf, &["-raw"])
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

/// What the engine lays out, in reading order.
enum Laid {
    /// One heading, one paragraph, or the ornament the built-in sheet
    /// sets a thematic break in. A blockquote is the blocks it nests,
    /// and an image has no text.
    Prose(String),
    /// A table, as the text of each of its rows.
    Table {
        head: Vec<String>,
        body: Vec<String>,
    },
}

fn laid_out(book: &Book) -> Vec<Laid> {
    let mut laid = Vec::new();
    for section in &book.sections {
        append_blocks(&section.blocks, &mut laid);
    }
    laid
}

fn append_blocks(blocks: &[Block], laid: &mut Vec<Laid>) {
    for block in blocks {
        match block {
            Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => {
                let mut text = String::new();
                append_inlines(inlines, &mut text);
                laid.push(Laid::Prose(text));
            }
            Block::Blockquote { blocks, .. } => append_blocks(blocks, laid),
            Block::ThematicBreak { .. } => laid.push(Laid::Prose(ORNAMENT.to_string())),
            Block::Image { .. } => {}
            Block::Table { head, body, .. } => laid.push(Laid::Table {
                head: head.iter().map(row_text).collect(),
                body: body.iter().map(row_text).collect(),
            }),
        }
    }
}

/// The text of every cell of one row, from the leading edge.
fn row_text(row: &Row) -> String {
    let mut text = String::new();
    for cell in &row.cells {
        let mut inner = Vec::new();
        append_blocks(&cell.blocks, &mut inner);
        for part in inner {
            if let Laid::Prose(prose) = part {
                text.push_str(&prose);
            }
        }
    }
    text
}

/// Whether extracted text holds everything a book lays out, in the
/// order it is laid out, or where the two part company.
///
/// `clean` makes the two comparable. Prose is compared character for
/// character. A table row is compared as the characters of its cells,
/// because an extraction reads the lines of a row across the cells in
/// whatever order they stand on the page. The header rows can come
/// back again where the table continues onto a new page.
///
/// `by_row` asks for each row in turn. Without it, the table and the
/// blocks after it are compared as one set of characters, up to where
/// the extraction returns to the book's own order. That is for an
/// extraction in reading order, which does not keep a table in order
/// with the prose around it.
fn holds(book: &Book, text: &str, clean: fn(&str) -> String, by_row: bool) -> Result<(), String> {
    let rendered: Vec<char> = clean(text).chars().collect();
    let laid = laid_out(book);
    let mut at = 0;
    let mut index = 0;
    while let Some(part) = laid.get(index) {
        index += 1;
        match part {
            Laid::Prose(prose) => {
                let expected: Vec<char> = clean(prose).chars().collect();
                let end = at + expected.len();
                if rendered.get(at..end) != Some(expected.as_slice()) {
                    return Err(first_difference(
                        &String::from_iter(&expected),
                        &String::from_iter(&rendered[at.min(rendered.len())..]),
                    ));
                }
                at = end;
            }
            Laid::Table { head, body } => {
                let head: Vec<Vec<char>> = head
                    .iter()
                    .map(|row| clean(row).chars().collect())
                    .collect();
                let body: Vec<Vec<char>> = body
                    .iter()
                    .map(|row| clean(row).chars().collect())
                    .collect();
                if by_row {
                    at = rows_at(&rendered, at, &head, &body)?;
                } else {
                    let after: Vec<Vec<char>> = laid[index..]
                        .iter()
                        .map_while(|part| match part {
                            Laid::Prose(prose) => Some(clean(prose).chars().collect()),
                            Laid::Table { .. } => None,
                        })
                        .collect();
                    let (end, taken) = table_at(&rendered, at, &head, &body, &after)?;
                    at = end;
                    index += taken;
                }
            }
        }
    }
    if at < rendered.len() {
        return Err(format!(
            "the PDF runs on past the book: {}",
            String::from_iter(&rendered[at..(at + 80).min(rendered.len())]),
        ));
    }
    Ok(())
}

/// Reads a table back one row at a time from `at`, and answers where
/// it ends.
fn rows_at(
    rendered: &[char],
    mut at: usize,
    head: &[Vec<char>],
    body: &[Vec<char>],
) -> Result<usize, String> {
    for row in head {
        at = take(rendered, at, row).ok_or_else(|| missing(rendered, at, row))?;
    }
    for row in body {
        if let Some(next) = take(rendered, at, row) {
            at = next;
            continue;
        }
        let mut again = at;
        for header in head {
            again = take(rendered, again, header).ok_or_else(|| missing(rendered, at, row))?;
        }
        at = take(rendered, again, row).ok_or_else(|| missing(rendered, again, row))?;
    }
    Ok(at)
}

/// Reads a table back from `at` together with the blocks `after` it.
/// The stretch holds the characters of every row, those of the header
/// rows once more for every page the table continued onto, and those
/// of the first few blocks after it, in any order. It ends where the
/// next block starts in the book's own order again. Answers where the
/// stretch ends and how many of the blocks after the table it took.
fn table_at(
    rendered: &[char],
    at: usize,
    head: &[Vec<char>],
    body: &[Vec<char>],
    after: &[Vec<char>],
) -> Result<(usize, usize), String> {
    let header: Vec<char> = head.concat();
    let mut expected: Vec<char> = head.iter().chain(body).flatten().copied().collect();
    for taken in 0..=after.len().min(12) {
        if taken > 0 {
            expected.extend(&after[taken - 1]);
        }
        let mut wanted = expected.clone();
        for _ in 0..=8 {
            let end = at + wanted.len();
            let Some(stretch) = rendered.get(at..end) else {
                break;
            };
            let resumes = after.get(taken).is_none_or(|next| {
                let opening = &next[..next.len().min(24)];
                rendered[end..].starts_with(opening)
            });
            if resumes && same_characters(stretch, &wanted) {
                return Ok((end, taken));
            }
            if header.is_empty() {
                break;
            }
            wanted.extend(&header);
        }
    }
    let until = (at + expected.len()).min(rendered.len());
    Err(format!(
        "a table did not come back whole: {}",
        String::from_iter(&rendered[at..until]),
    ))
}

/// Whether two stretches hold the same characters, in any order.
fn same_characters(one: &[char], other: &[char]) -> bool {
    let (mut one, mut other) = (one.to_vec(), other.to_vec());
    one.sort_unstable();
    other.sort_unstable();
    one == other
}

/// Where one row's characters end, when they stand at `at` in any
/// order.
fn take(rendered: &[char], at: usize, row: &[char]) -> Option<usize> {
    let end = at + row.len();
    same_characters(rendered.get(at..end)?, row).then_some(end)
}

/// A row that did not come back, beside what came back in its place.
fn missing(rendered: &[char], at: usize, row: &[char]) -> String {
    let until = (at + row.len() + 20).min(rendered.len());
    format!(
        "a table row did not come back\n   row: {}\n   pdf: {}",
        String::from_iter(row),
        String::from_iter(&rendered[at.min(until)..until]),
    )
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
