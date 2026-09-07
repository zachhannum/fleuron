//! The box model: what a padded, bordered, tinted block paints, and
//! where.
//!
//! One fixture, a chapter opening and a quotation long enough to
//! carry over a page turn, set under a sheet that boxes the
//! quotation and rules the heading.

use fleuron::content::{Block, Book, HeadingLevel, Inline, NodeId, Section};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::layout::Paginator;
use fleuron::pages::{DrawItem, Page};
use fleuron::style::{Color, Source, StyleTree, Stylesheets};

/// A page small enough that the quotation breaks, a rule under the
/// heading, and a box around the quotation.
const BOXED_CSS: &str = r#"
@page { size: 240pt 180pt; margin: 24pt }

h1 {
  font-size: 14pt;
  margin: 0;
  padding-bottom: 6pt;
  border-bottom: 1pt solid #8a7a5c;
}

blockquote {
  margin: 12pt 0;
  padding: 8pt 10pt;
  border: 1pt solid #8a7a5c;
  background-color: #f4f1ea;
}

blockquote p { margin: 0 }
"#;

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

fn text(value: &str) -> Inline {
    Inline::Text {
        id: NodeId::UNASSIGNED,
        value: value.into(),
        position: None,
        span: None,
    }
}

fn paragraph(value: &str) -> Block {
    Block::Paragraph {
        id: NodeId::UNASSIGNED,
        inlines: vec![text(value)],
        position: None,
        span: None,
    }
}

/// The fixture: a chapter opening, and a quotation of four paragraphs
/// under it.
fn fixture() -> Book {
    let quoted = "The wind came off the water and the harbour lights went out, \
                  one after another, until the quay was as dark as the sea.";
    let mut book = Book {
        metadata: Default::default(),
        sections: vec![Section {
            id: NodeId::UNASSIGNED,
            source: None,
            title: None,
            blocks: vec![
                Block::Heading {
                    id: NodeId::UNASSIGNED,
                    level: HeadingLevel::H1,
                    inlines: vec![text("The Quay")],
                    position: None,
                    span: None,
                },
                Block::Blockquote {
                    id: NodeId::UNASSIGNED,
                    blocks: (0..4).map(|_| paragraph(quoted)).collect(),
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

fn pages(book: &Book, css: &str) -> Vec<Page> {
    let styles: StyleTree =
        Stylesheets::parse(&[Source::author("box.css", css)]).compile(book, registry());
    assert!(
        styles.warnings().is_empty(),
        "the sheet is in the subset: {:?}",
        styles.warnings(),
    );
    Paginator::new(registry(), &styles).paginate(book)
}

/// Every filled rect of one page, in paint order.
fn rects(page: &Page) -> Vec<&DrawItem> {
    page.items
        .iter()
        .filter(|item| matches!(item, DrawItem::Rect { .. }))
        .collect()
}

/// One page's rects, one line each, in paint order.
fn described(pages: &[Page]) -> Vec<Vec<String>> {
    pages
        .iter()
        .map(|page| {
            rects(page)
                .iter()
                .map(|item| match item {
                    DrawItem::Rect { x, y, w, h, color } => {
                        format!("rect {x:?} {y:?} {w:?} {h:?} {}", color.to_hex())
                    }
                    _ => unreachable!("only rects were kept"),
                })
                .collect()
        })
        .collect()
}

/// Every rect a boxed book paints, on every page it paints one.
#[test]
fn a_boxed_book_paints_what_the_sheet_asked_for() {
    let pages = pages(&fixture(), BOXED_CSS);
    insta::assert_json_snapshot!("a_boxed_book", described(&pages));
}

/// A background and a border paint before the text of the page they
/// are on: `DrawItem` order is paint order, and the display structure
/// has no layers.
#[test]
fn a_background_paints_before_the_text_it_sits_behind() {
    for page in pages(&fixture(), BOXED_CSS) {
        let last_rect = page
            .items
            .iter()
            .rposition(|item| matches!(item, DrawItem::Rect { .. }));
        let first_text = page
            .items
            .iter()
            .position(|item| matches!(item, DrawItem::Text { .. }));
        if let (Some(rect), Some(text)) = (last_rect, first_text) {
            assert!(rect < text, "page {}: text under the box", page.number);
        }
    }
}

/// A border edge with no colour of its own is painted in the
/// element's own `color`, whichever order the two were written in.
#[test]
fn a_border_with_no_colour_takes_the_elements_own() {
    let ink = Color::rgb(180, 30, 30);
    for css in [
        "blockquote { border: 1pt solid; color: rgb(180, 30, 30) }",
        "blockquote { color: rgb(180, 30, 30); border: 1pt solid }",
    ] {
        let pages = pages(&fixture(), &format!("{BOXED_CSS}\n{css}"));
        let painted: Vec<Color> = pages
            .iter()
            .flat_map(|page| rects(page))
            .filter_map(|item| match item {
                DrawItem::Rect { color, .. } => Some(*color),
                _ => None,
            })
            .collect();
        assert!(
            painted.contains(&ink),
            "{css}: nothing was painted in the element's colour: {painted:?}",
        );
    }
}
