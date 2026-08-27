//! The fixture manuscript through the engine: the excerpt the e2e
//! renders, read by the shipped frontend.
//!
//! It lives here rather than beside the layout code because reading it
//! means the markdown crate, which depends on this one.

use fleuron::LayoutOutput;
use fleuron::content::{Block, Book, Inline};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::layout::layout_book;
use fleuron::pages::{DrawItem, Page};
use fleuron::style::{PageQuery, Situation, StyleTree};
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
    let output = layout_book(book, &styles, registry());
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
