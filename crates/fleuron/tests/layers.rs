//! Layers: the paint order a sheet sets, over the one the flow
//! produced.
//!
//! One fixture — a chapter whose opening carries a scan against the
//! page, two tinted quotations under it, and prose after them — set
//! under sheets that raise and lower the blocks. What is checked is
//! the order of the page's items, because that order is what a
//! painter paints in.

use fleuron::content::{Attributes, Block, Book, HeadingLevel, Inline, NodeId, Section};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::images::{Assets, ImageLoader};
use fleuron::layout::layout_book;
use fleuron::pages::{DrawItem, Page};
use fleuron::style::{Source, StyleTree, Stylesheets};
use fleuron::{LayoutOutput, wire};

/// A page box with a scan behind it and a folio under it, so every
/// page carries something the engine paints below and above the
/// blocks of the book.
const PAGE_CSS: &str = r#"
@page {
  size: 300pt 240pt;
  margin: 24pt;
  background-image: url(scan.png);
  background-size: cover;
  @bottom-center { content: counter(page) }
}
section { break-before: auto }
blockquote { background-color: #e8e2d4; margin: 6pt 0; padding: 4pt }
blockquote p { margin: 0 }
img { position: absolute; top: 12pt; left: 12pt }
"#;

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

/// A PNG header of a given pixel size, at the CSS resolution. Layout
/// reads the header and nothing else.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend(13u32.to_be_bytes());
    bytes.extend(b"IHDR");
    bytes.extend(width.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes.extend([8, 6, 0, 0, 0]);
    bytes.extend([0, 0, 0, 0]);
    bytes.extend(0u32.to_be_bytes());
    bytes.extend(b"IEND");
    bytes.extend([0, 0, 0, 0]);
    bytes
}

/// The host side: the scan behind the page, and the plate against it.
struct Scans;

impl ImageLoader for Scans {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        match url {
            "scan.png" => Some(png(600, 480)),
            "plate.png" => Some(png(48, 48)),
            _ => None,
        }
    }
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

fn quotation(value: &str) -> Block {
    Block::Blockquote {
        id: NodeId::UNASSIGNED,
        blocks: vec![paragraph(value)],
        attributes: Attributes::default(),
        position: None,
        span: None,
    }
}

/// The fixture: a heading, a plate the sheet puts against the page,
/// two tinted quotations, and prose.
fn fixture() -> Book {
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
                    attributes: Attributes::default(),
                    position: None,
                    span: None,
                },
                Block::Image {
                    id: NodeId::UNASSIGNED,
                    url: "plate.png".into(),
                    alt: "a plate".into(),
                    attributes: Attributes::default(),
                    position: None,
                    span: None,
                },
                quotation("The harbour lights went out, one after another."),
                quotation("The tide turned before morning."),
                paragraph("Nobody was awake to see either of them."),
            ],
            position: None,
            span: None,
        }],
    };
    book.assign_node_ids();
    book
}

fn styles(book: &Book, css: &str) -> StyleTree {
    let styles = Stylesheets::parse(&[Source::author("layers.css", css)]).compile(book, registry());
    assert!(
        styles.warnings().is_empty(),
        "the sheet is in the subset: {:?}",
        styles.warnings(),
    );
    styles
}

/// The fixture under `PAGE_CSS` and whatever a test adds to it.
fn laid_out(css: &str) -> LayoutOutput {
    let book = fixture();
    let styles = styles(&book, &format!("{PAGE_CSS}{css}"));
    let assets = Assets::probe(&book, &styles, &Scans);
    let output = layout_book(&book, &styles, registry(), &assets);
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
    output
}

fn pages(css: &str) -> Vec<Page> {
    laid_out(css).pages
}

/// Where the tints the quotations paint sit in the page's items.
fn tints(page: &Page) -> Vec<usize> {
    page.items
        .iter()
        .enumerate()
        .filter(|(_, item)| matches!(item, DrawItem::Rect { .. }))
        .map(|(at, _)| at)
        .collect()
}

/// Where the plate the sheet put against the page sits in them.
fn plate(page: &Page) -> usize {
    page.items
        .iter()
        .position(|item| matches!(item, DrawItem::Image { .. }))
        .expect("the page carries the plate")
}

/// Where the scan behind the page sits in them.
fn scan(page: &Page) -> usize {
    page.items
        .iter()
        .position(|item| matches!(item, DrawItem::Background { .. }))
        .expect("the page carries its scan")
}

/// Where the prose of the page sits in them, folio and all.
fn runs(page: &Page) -> Vec<usize> {
    page.items
        .iter()
        .enumerate()
        .filter(|(_, item)| matches!(item, DrawItem::Text { .. }))
        .map(|(at, _)| at)
        .collect()
}

/// Acceptance: a block the sheet raises paints over a positioned
/// block that named no layer, whichever of the two the flow painted
/// first. The flow paints the plate over the quotations, and raising
/// the quotations turns that around.
#[test]
fn a_raised_block_paints_over_a_positioned_block_that_named_no_layer() {
    let flowed = pages("");
    assert!(
        plate(&flowed[0]) > tints(&flowed[0])[1],
        "the flow did not paint the plate over the quotations",
    );

    let raised = pages("blockquote { z-index: 10 }");
    assert!(
        tints(&raised[0])[0] > plate(&raised[0]),
        "the raised quotation did not paint over the plate",
    );

    // The other way about: the positioned block lowered under the
    // quotations the flow painted before it.
    let lowered = pages("img { z-index: -1 }");
    assert!(
        plate(&lowered[0]) < tints(&lowered[0])[0],
        "the lowered plate did not paint under the quotations",
    );
}

/// Acceptance: two blocks in one layer paint in the order the flow
/// produced them, which is the order they were written in.
#[test]
fn two_blocks_in_one_layer_paint_in_flow_order() {
    let raised = pages("blockquote { z-index: 10 }");
    let tints = tints(&raised[0]);
    assert_eq!(tints.len(), 2, "the page did not carry both quotations");
    assert!(
        tints[0] < tints[1],
        "the quotations swapped places inside their layer",
    );
    let (first, second) = (&raised[0].items[tints[0]], &raised[0].items[tints[1]]);
    let top = |item: &DrawItem| match item {
        DrawItem::Rect { y, .. } => *y,
        other => panic!("the tint is a rect: {other:?}"),
    };
    assert!(
        top(first) < top(second),
        "the tint painted first is not the one higher up the page",
    );
}

/// Acceptance: the text of a page paints over the art behind the
/// page, and the sheet says nothing about it.
#[test]
fn text_paints_over_the_art_behind_the_page() {
    for page in pages("") {
        assert_eq!(scan(&page), 0, "the scan is not the first thing painted");
        assert!(
            runs(&page).iter().all(|at| *at > scan(&page)),
            "a run paints under the art behind the page",
        );
        assert_eq!(
            page.items[0].layer(),
            DrawItem::PAGE_BACKGROUND,
            "the scan is not in the lowest layer",
        );
    }
}

/// Acceptance: a negative layer paints under the text of the page and
/// over the art behind it.
#[test]
fn a_negative_layer_paints_under_the_text_and_over_the_art() {
    let lowered = pages("blockquote { z-index: -1 }");
    let page = &lowered[0];
    let tints = tints(page);
    assert!(
        tints.iter().all(|at| *at > scan(page)),
        "a lowered tint paints under the art behind the page",
    );
    assert!(
        tints.iter().all(|at| runs(page).iter().all(|run| run > at)),
        "a lowered tint paints over the text of the page",
    );
}

/// Acceptance: layout is deterministic. Two runs over a layered book
/// write the same bytes.
#[test]
fn two_runs_over_a_layered_book_agree_to_the_byte() {
    let css = "blockquote { z-index: 10 } img { z-index: -1 }";
    let first = wire::encode(&laid_out(css)).expect("the wire writes");
    let second = wire::encode(&laid_out(css)).expect("the wire writes");
    assert_eq!(first, second, "two runs over one book wrote two structures");
}

/// Acceptance: a sheet that names no layer leaves the flow's own
/// order alone. Writing `z-index: auto` over every block of the book
/// says the same thing, to the byte.
#[test]
fn a_book_that_names_no_layer_is_the_book_the_flow_produced() {
    let named = wire::encode(&laid_out(
        "section, h1, p, blockquote, img { z-index: auto }",
    ))
    .expect("the wire writes");
    let silent = wire::encode(&laid_out("")).expect("the wire writes");
    assert_eq!(named, silent, "naming `auto` moved something");

    for page in pages("") {
        let flowed: Vec<i32> = page
            .items
            .iter()
            .map(DrawItem::layer)
            .filter(|layer| {
                *layer != DrawItem::PAGE_BACKGROUND && *layer != DrawItem::PAGE_FURNITURE
            })
            .collect();
        assert!(
            flowed.iter().all(|layer| *layer == 0),
            "a block of a book that names no layer left layer 0: {flowed:?}",
        );
    }
}

/// The list a painter reads is in paint order: the layers never go
/// back down it. The page's own art is below every block and the
/// page's own furniture above every one, so this holds over the whole
/// list rather than over the flow's part of it.
#[test]
fn every_page_hands_its_painter_one_ascending_list() {
    for css in [
        "",
        "blockquote { z-index: 10 }",
        "blockquote { z-index: -1 } img { z-index: 4 }",
        "h1 { z-index: -20 } p { z-index: 2 } img { z-index: 2 }",
    ] {
        for page in pages(css) {
            let layers: Vec<i32> = page.items.iter().map(DrawItem::layer).collect();
            assert!(
                layers.windows(2).all(|pair| pair[0] <= pair[1]),
                "{css}: page {} is not in paint order: {layers:?}",
                page.number,
            );
        }
    }
}
