//! The fixture manuscript through the engine: the excerpt the e2e
//! renders, read by the shipped frontend.
//!
//! It lives here rather than beside the layout code because reading it
//! means the markdown crate, which depends on this one.

use fleuron::LayoutOutput;
use fleuron::content::{Block, Book, Inline};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::images::Assets;
use fleuron::layout::layout_book;
use fleuron::pages::{DrawItem, Page};
use fleuron::style::{PageQuery, Situation, Source, StyleTree, Stylesheets};
use fleuron_markdown::Options;

const MANUSCRIPT: &str = include_str!("../../../fixtures/gulliver-excerpt.md");

/// The name the built-in sheet gives a section's pages.
const CHAPTER: Option<&str> = Some("chapter");

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

/// The fixture, read the way a lone markdown input is read: its
/// frontmatter is the book's.
fn fixture() -> Book {
    let (sections, warnings) =
        fleuron_markdown::to_sections(MANUSCRIPT, "gulliver-excerpt.md", &Options::default());
    assert!(warnings.is_empty(), "the excerpt is clean: {warnings:?}");
    fleuron_markdown::assemble(fleuron_markdown::frontmatter(MANUSCRIPT), sections)
}

fn lay_out(book: &Book) -> (StyleTree, LayoutOutput) {
    let styles = fleuron::style::defaults(book, registry());
    let output = layout_book(book, &styles, registry(), &Assets::none());
    (styles, output)
}

/// The folio one page carries: the text painted below its content
/// box.
fn folio(page: &Page, styles: &StyleTree) -> Option<String> {
    let geometry = styles
        .page(PageQuery {
            name: CHAPTER,
            situation: Situation::Body(page.side),
        })
        .geometry;
    let bottom = geometry.content_origin().1 + geometry.content_size().1;
    page.items.iter().find_map(|item| match item {
        DrawItem::Text { y, text, .. } if *y > bottom => Some(text.clone()),
        _ => None,
    })
}

/// The fixture paginates, and its font table is the registry's —
/// every cut, indexed by the id a run carries.
#[test]
fn the_fixture_manuscript_paginates() {
    let (_, output) = lay_out(&fixture());
    assert!(!output.pages.is_empty());
    assert_eq!(output.fonts.len(), registry().len());
}

/// The fixture carries folios: the e2e path exercises page furniture,
/// not just content flow.
#[test]
fn the_fixture_manuscript_carries_folios() {
    let (styles, output) = lay_out(&fixture());
    let with_folios = output
        .pages
        .iter()
        .filter(|page| folio(page, &styles).is_some())
        .count();
    assert!(
        with_folios >= output.pages.len() - 2,
        "only {with_folios} of {} fixture pages carry folios",
        output.pages.len(),
    );
    for page in &output.pages {
        if let Some(digits) = folio(page, &styles) {
            assert_eq!(digits, page.number.to_string());
        }
    }
}

/// The excerpt is the corpus the pipeline is checked against, so it
/// has to hold the constructs the vocabulary does: headings, a
/// quotation of more than one paragraph, and prose that opens italic.
#[test]
fn the_fixture_manuscript_exercises_the_vocabulary() {
    let book = fixture();
    assert_eq!(book.metadata.title.as_deref(), Some("Gulliver's Travels"));
    let blocks: Vec<&Block> = book
        .sections
        .iter()
        .flat_map(|section| section.blocks.iter())
        .collect();
    assert!(blocks.iter().any(|b| matches!(b, Block::Heading { .. })));
    let quote = blocks
        .iter()
        .find_map(|block| match block {
            Block::Blockquote { blocks, .. } => Some(blocks.len()),
            _ => None,
        })
        .expect("the excerpt quotes something");
    assert!(quote >= 2, "the quotation runs to several paragraphs");
    assert!(
        blocks.iter().any(|block| matches!(
            block,
            Block::Paragraph { inlines, .. }
                if matches!(inlines.first(), Some(Inline::Emphasis { .. }))
        )),
        "the chapter arguments open with an emphasis run",
    );
}

/// The fixture under an author sheet.
fn with_css(book: &Book, css: &str) -> LayoutOutput {
    let styles = Stylesheets::parse(&[Source::author("book.css", css)]).compile(book, registry());
    assert!(styles.warnings().is_empty(), "{:?}", styles.warnings());
    layout_book(book, &styles, registry(), &Assets::none())
}

/// The baselines of one page, each with the right edge of the last
/// glyph painted on it. Runs carry no advances, so the edge comes
/// back out of the face the glyph was shaped in.
fn line_ends(page: &Page, body_size: f32) -> Vec<f32> {
    let mut ends: Vec<(f32, f32)> = Vec::new();
    for item in &page.items {
        let DrawItem::Text {
            y,
            size,
            font_id,
            glyphs,
            ..
        } = item
        else {
            continue;
        };
        let Some(last) = glyphs.last() else { continue };
        if (*size - body_size).abs() > 0.01 {
            continue;
        }
        let upem = registry().metrics(*font_id).unwrap().units_per_em as f32;
        let advance = registry().advance_width(*font_id, last.id).unwrap_or(0) as f32;
        let edge = last.x + advance / upem * size;
        match ends.iter_mut().find(|(at, _)| at == y) {
            Some(held) => held.1 = held.1.max(edge),
            None => ends.push((*y, edge)),
        }
    }
    ends.into_iter().map(|(_, edge)| edge).collect()
}

/// Acceptance: hyphenation never runs to three line ends in a row
/// anywhere in the fixture book, and it does run to two somewhere.
/// A book that hyphenated nothing would pass the first half of that
/// for the wrong reason.
#[test]
fn hyphenation_never_runs_three_line_ends_deep() {
    let output = with_css(
        &fixture(),
        "book { text-align: justify; hyphens: auto; hanging-punctuation: first force-end }",
    );
    let mut longest = 0;
    for page in &output.pages {
        let mut ends: Vec<(f32, bool)> = Vec::new();
        for item in &page.items {
            let DrawItem::Text { y, text, .. } = item else {
                continue;
            };
            match ends.iter_mut().find(|(at, _)| at == y) {
                Some(end) => end.1 = text.ends_with('-'),
                None => ends.push((*y, text.ends_with('-'))),
            }
        }
        ends.sort_by(|one, other| one.0.total_cmp(&other.0));
        let mut run = 0;
        for (_, hyphenated) in &ends {
            run = if *hyphenated { run + 1 } else { 0 };
            longest = longest.max(run);
        }
    }
    assert!(longest >= 2, "the fixture book hyphenated almost nothing");
    assert!(longest <= 2, "{longest} hyphenated line ends in a row");
}

/// Justification reaches the page: the body lines of the fullest
/// page end at one right edge, which under the ragged setting the
/// same book takes by default they do not.
#[test]
fn justified_lines_end_at_one_right_edge() {
    let book = fixture();
    let body_size = fleuron::style::defaults(&book, registry()).root().font_size;
    let spread = |output: &LayoutOutput| {
        let page = output
            .pages
            .iter()
            .max_by_key(|page| page.items.len())
            .expect("the fixture paginates");
        let ends = line_ends(page, body_size);
        assert!(ends.len() > 10, "too few lines to say anything: {ends:?}");
        let last = ends.len() - 1;
        let (low, high) = ends[..last]
            .iter()
            .fold((f32::MAX, f32::MIN), |(low, high), edge| {
                (low.min(*edge), high.max(*edge))
            });
        high - low
    };
    let justified = spread(&with_css(&book, "book { text-align: justify }"));
    let ragged = spread(&with_css(&book, "book { text-align: left }"));
    assert!(
        justified < 0.05,
        "justified line ends spread over {justified}pt",
    );
    assert!(
        ragged > 10.0,
        "the ragged setting was flush too, at {ragged}pt",
    );
}
