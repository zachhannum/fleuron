//! Alpha, opacity and rounded corners: what a sheet sets, and what the
//! display structure carries for it.
//!
//! One fixture, a chapter with a quotation in it, set under sheets
//! that tint the quotation, fade it, and round its corners.

use fleuron::content::{Attributes, Block, Book, HeadingLevel, Inline, NodeId, Section};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::images::Assets;
use fleuron::layout::layout_book;
use fleuron::pages::{DrawItem, Page};
use fleuron::style::{Color, Source, StyleTree, Stylesheets};

/// A page large enough for the whole chapter, so every item is on
/// page one.
const PAGE_CSS: &str = "@page { size: 300pt 400pt; margin: 24pt }\n";

/// The quotation's own tint.
const TINT: Color = Color::rgb(0xf4, 0xf1, 0xea);

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

fn text(value: &str) -> Inline {
    Inline::Text {
        id: NodeId::UNASSIGNED,
        value: value.into(),
        attributes: Attributes::default(),
        position: None,
        span: None,
    }
}

fn paragraph(value: &str) -> Block {
    Block::Paragraph {
        id: NodeId::UNASSIGNED,
        inlines: vec![text(value)],
        attributes: Attributes::default(),
        position: None,
        span: None,
    }
}

/// The fixture: a heading, a quotation, and a paragraph after it.
fn fixture() -> Book {
    let mut book = Book {
        metadata: Default::default(),
        sections: vec![Section {
            attributes: Default::default(),
            id: NodeId::UNASSIGNED,
            source: None,
            title: None,
            blocks: vec![
                Block::Heading {
                    id: NodeId::UNASSIGNED,
                    level: HeadingLevel::H1,
                    inlines: vec![text("The Quay")],
                    attributes: Attributes::default(),
                    position: None,
                    span: None,
                },
                Block::Blockquote {
                    id: NodeId::UNASSIGNED,
                    blocks: vec![paragraph("Quoted: the wind came off the water.")],
                    attributes: Attributes::default(),
                    position: None,
                    span: None,
                },
                paragraph("The tide turned before morning."),
            ],
            position: None,
            span: None,
        }],
    };
    book.assign_node_ids();
    book
}

fn styles(book: &Book, css: &str) -> StyleTree {
    let styles = Stylesheets::parse(&[Source::author(
        "translucency.css",
        &format!("{PAGE_CSS}{css}"),
    )])
    .compile(book, registry());
    assert!(styles.warnings().is_empty(), "{:?}", styles.warnings());
    styles
}

fn pages(css: &str) -> Vec<Page> {
    let book = fixture();
    let styles = styles(&book, css);
    layout_book(&book, &styles, registry(), &Assets::none()).pages
}

/// The colour of every run whose text begins with `opening`.
fn runs(page: &Page, opening: &str) -> Vec<Color> {
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Text { text, color, .. } if text.starts_with(opening) => Some(*color),
            _ => None,
        })
        .collect()
}

/// The fill of every rect in the quotation's tint, whatever its alpha.
fn tints(page: &Page) -> Vec<Color> {
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Rect { color, .. } if Color { a: 255, ..*color } == TINT => Some(*color),
            _ => None,
        })
        .collect()
}

/// Part: `opacity` reads as a number or a percentage, is clamped to 0
/// to 1, and does not inherit.
#[test]
fn opacity_reads_as_a_number_or_a_percentage_and_does_not_inherit() {
    let book = fixture();
    let styles = styles(
        &book,
        "blockquote { opacity: 0.05 } h1 { opacity: 50% } p { opacity: 2 }",
    );
    let [
        Block::Heading { id: heading, .. },
        Block::Blockquote {
            id: quotation,
            blocks,
            ..
        },
        ..,
    ] = &book.sections[0].blocks[..]
    else {
        unreachable!("the fixture opens with a heading and a quotation")
    };
    let Block::Paragraph { id: quoted, .. } = &blocks[0] else {
        unreachable!("the quotation holds a paragraph")
    };
    assert_eq!(styles.style(*quotation).opacity, 0.05);
    assert_eq!(styles.style(*heading).opacity, 0.5);
    assert_eq!(styles.style(*quoted).opacity, 1.0);
    assert_eq!(styles.style(book.sections[0].id).opacity, 1.0);
}

/// Acceptance: `opacity: 0.05` on a block fades its text and its
/// background together, and nothing outside the block fades with it.
#[test]
fn opacity_fades_a_blocks_text_and_background_together() {
    let pages = pages(&format!(
        "blockquote {{ opacity: 0.05; background-color: {} }}",
        TINT.to_hex()
    ));
    let page = &pages[0];
    // 5% of 255 rounds to 13.
    let faded = TINT.faded(0.05);
    assert_eq!(faded.a, 13);
    assert_eq!(tints(page), [faded], "the tint did not fade");
    let quoted = runs(page, "Quoted");
    assert!(!quoted.is_empty(), "the quotation set no run");
    assert!(
        quoted
            .iter()
            .all(|color| *color == Color::BLACK.faded(0.05)),
        "the quotation's text did not fade with its tint: {quoted:?}"
    );
    assert_eq!(runs(page, "The Quay"), [Color::BLACK]);
    assert_eq!(runs(page, "The tide"), [Color::BLACK]);
}

/// The opacity of a block and of the blocks around it multiply, and a
/// colour's own alpha multiplies with both.
#[test]
fn nested_opacities_and_a_colours_alpha_multiply() {
    let pages = pages(
        "section { opacity: 0.5 } blockquote { opacity: 0.5 } \
         blockquote p { color: rgba(0, 0, 0, 0.5) }",
    );
    let page = &pages[0];
    assert_eq!(runs(page, "The Quay"), [Color::rgba(0, 0, 0, 128)]);
    // 255 at 0.5 is 128, and 128 at 0.25 is 32.
    assert!(
        runs(page, "Quoted")
            .iter()
            .all(|color| *color == Color::rgba(0, 0, 0, 32)),
        "{:?}",
        runs(page, "Quoted")
    );
}

/// `opacity: 1` is where every block starts, so writing it changes
/// nothing on the page.
#[test]
fn opacity_one_sets_the_book_unchanged() {
    assert_eq!(
        pages("blockquote { background-color: #f4f1ea }"),
        pages("blockquote { background-color: #f4f1ea; opacity: 1 }")
    );
}
