//! Art behind a page and behind a block: where it is painted, how
//! large, and in what order.
//!
//! One fixture, a chapter with a quotation in it, set under sheets
//! that put a scan behind the page and a tint and an image behind the
//! quotation.

use fleuron::content::{Attributes, Block, Book, HeadingLevel, Inline, NodeId, Section};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::images::{Assets, ImageLoader};
use fleuron::layout::layout_book;
use fleuron::pages::{DrawItem, Page, Side};
use fleuron::style::{Color, Source, StyleTree, Stylesheets};
use fleuron::{LayoutOutput, Warning};

/// A page small enough that the quotation carries over a page turn,
/// and a folio under it, so the page background has to sit under
/// furniture as well as under prose.
const PAGE_CSS: &str = r#"
@page {
  size: 240pt 180pt;
  margin: 24pt;
  @bottom-center { content: counter(page) }
}
@page :left { background-image: url(verso.png); background-size: cover }
@page :right { background-image: url(recto.png); background-size: cover }
section { break-before: auto }
blockquote { margin: 8pt 0 }
blockquote p { margin: 0 }
"#;

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

/// A PNG header of a given pixel size, at the CSS resolution. Layout
/// reads the header and nothing else, so this is a whole image as far
/// as the display structure is concerned.
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

/// The host side: two scans of different shapes, and an ornament.
struct Scans;

impl ImageLoader for Scans {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        match url {
            // Wider than the page is, in the page's own ratio's
            // opposite: `cover` has to crop it across.
            "verso.png" => Some(png(640, 240)),
            "recto.png" => Some(png(240, 640)),
            "ornament.png" => Some(png(24, 24)),
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

/// The fixture: a chapter opening and a quotation long enough to
/// carry over a page turn.
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
                    attributes: Attributes::default(),
                    position: None,
                    span: None,
                },
                Block::Blockquote {
                    id: NodeId::UNASSIGNED,
                    blocks: (0..5).map(|_| paragraph(quoted)).collect(),
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
    let styles =
        Stylesheets::parse(&[Source::author("background.css", css)]).compile(book, registry());
    assert!(
        styles.warnings().is_empty(),
        "the sheet is in the subset: {:?}",
        styles.warnings(),
    );
    styles
}

fn laid_out(css: &str) -> LayoutOutput {
    let book = fixture();
    let styles = styles(&book, css);
    let assets = Assets::probe(&book, &styles, &Scans);
    layout_book(&book, &styles, registry(), &assets)
}

fn pages(css: &str) -> Vec<Page> {
    let output = laid_out(css);
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
    output.pages
}

/// Every background one page paints: `(box, tile, repeat, asset)`.
type Painted = ((f32, f32, f32, f32), (f32, f32, f32, f32), bool, u32);

fn backgrounds(page: &Page) -> Vec<Painted> {
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Background {
                x,
                y,
                w,
                h,
                tile_x,
                tile_y,
                tile_w,
                tile_h,
                repeat,
                asset,
                ..
            } => Some((
                (*x, *y, *w, *h),
                (*tile_x, *tile_y, *tile_w, *tile_h),
                *repeat,
                *asset,
            )),
            _ => None,
        })
        .collect()
}

/// The url one asset index names.
fn named(output: &LayoutOutput, asset: u32) -> &str {
    &output.assets[asset as usize].url
}

/// Acceptance: `@page :left` paints its scan over the whole verso,
/// under the text and under the margin boxes.
#[test]
fn a_page_background_covers_the_whole_page_under_everything_on_it() {
    let pages = pages(PAGE_CSS);
    assert!(pages.len() > 1, "the fixture fills more than one page");
    for page in &pages {
        let painted = backgrounds(page);
        assert_eq!(painted.len(), 1, "page {}: {painted:?}", page.number);
        let ((x, y, w, h), ..) = painted[0];
        assert_eq!(
            (x, y, w, h),
            (0.0, 0.0, page.width, page.height),
            "page {}: the background is not the whole page box",
            page.number,
        );
        assert!(
            matches!(page.items.first(), Some(DrawItem::Background { .. })),
            "page {}: something is painted under the background",
            page.number,
        );
    }
}

/// Acceptance: the two sides take different images, so a spread shows
/// both.
#[test]
fn each_side_of_the_spread_shows_its_own_scan() {
    let output = laid_out(PAGE_CSS);
    let spread: Vec<(Side, &str)> = output
        .pages
        .iter()
        .map(|page| {
            let (.., asset) = backgrounds(page)[0];
            (page.side, named(&output, asset))
        })
        .collect();
    assert!(
        spread.iter().any(|(side, _)| *side == Side::Recto)
            && spread.iter().any(|(side, _)| *side == Side::Verso),
        "the book is one leaf: {spread:?}",
    );
    for (side, url) in spread {
        let wanted = match side {
            Side::Recto => "recto.png",
            Side::Verso => "verso.png",
        };
        assert_eq!(url, wanted, "a {side:?} took {url}");
    }
}

/// Acceptance: `cover` fills the box and the crop takes the
/// overflow; `contain` fits and leaves the remainder.
#[test]
fn cover_crops_the_overflow_and_contain_leaves_the_remainder() {
    let covering = pages(PAGE_CSS);
    for page in &covering {
        let ((_, _, w, h), (_, _, tile_w, tile_h), ..) = backgrounds(page)[0];
        assert!(
            tile_w >= w - 1e-3 && tile_h >= h - 1e-3,
            "page {}: cover left {w}x{h} uncovered by {tile_w}x{tile_h}",
            page.number,
        );
        assert!(
            tile_w > w + 1e-3 || tile_h > h + 1e-3,
            "page {}: this scan has to crop on one axis",
            page.number,
        );
    }

    let fitting = pages(&PAGE_CSS.replace("background-size: cover", "background-size: contain"));
    for page in &fitting {
        let ((_, _, w, h), (_, _, tile_w, tile_h), ..) = backgrounds(page)[0];
        assert!(
            tile_w <= w + 1e-3 && tile_h <= h + 1e-3,
            "page {}: contain drew {tile_w}x{tile_h} in {w}x{h}",
            page.number,
        );
        assert!(
            tile_w > w - 1e-3 || tile_h > h - 1e-3,
            "page {}: contain left room on both axes",
            page.number,
        );
    }
}

/// The sheet the block tests are set under: a tint and an ornament
/// behind the quotation, and nothing behind the page.
const BLOCK_CSS: &str = r#"
@page { size: 240pt 180pt; margin: 24pt }
section { break-before: auto }
blockquote {
  margin: 8pt 12pt;
  padding: 6pt;
  border: 1pt solid #8a7a5c;
  background-color: #f4f1ea;
  background-image: url(ornament.png);
  background-repeat: no-repeat;
  background-position: center;
}
blockquote p { margin: 0 }
"#;

/// Acceptance: a block background paints over the block's border box
/// and no wider.
#[test]
fn a_block_background_covers_its_border_box_and_no_wider() {
    let pages = pages(BLOCK_CSS);
    // The page box is 240pt wide with 24pt margins, and the quote
    // takes 12pt of margin on each side of the content box.
    let (left, width) = (24.0 + 12.0, 240.0 - 48.0 - 24.0);
    let mut painted = 0;
    for page in &pages {
        for ((x, _, w, _), ..) in backgrounds(page) {
            assert!(
                (x - left).abs() < 1e-3 && (w - width).abs() < 1e-3,
                "page {}: the quote's box is {x} wide {w}, not {left} wide {width}",
                page.number,
            );
            painted += 1;
        }
    }
    assert!(painted > 1, "the quotation did not carry over a page turn");
}

/// Acceptance: a tinted and imaged block paints the colour under the
/// image.
#[test]
fn a_tinted_and_imaged_block_paints_the_colour_under_the_image() {
    let tint = Color::rgb(0xf4, 0xf1, 0xea);
    let mut checked = 0;
    for page in pages(BLOCK_CSS) {
        // The last page of the fixture holds the paragraph after the
        // quotation and nothing of the quotation itself.
        let Some(image) = page
            .items
            .iter()
            .position(|item| matches!(item, DrawItem::Background { .. }))
        else {
            continue;
        };
        let filled = page
            .items
            .iter()
            .position(|item| matches!(item, DrawItem::Rect { color, .. } if *color == tint))
            .expect("the quote paints its tint");
        assert!(filled < image, "page {}: the tint is over it", page.number);
        checked += 1;
    }
    assert!(checked > 0, "the quotation painted nothing");
}

/// `background-repeat` reaches the display structure, and an image
/// centred in its box sits where the arithmetic puts it.
#[test]
fn repeat_and_position_reach_the_display_structure() {
    let once = pages(BLOCK_CSS);
    let (box_, tile, repeat, _) = backgrounds(&once[0])[0];
    assert!(!repeat, "no-repeat still repeats");
    assert!(
        (tile.0 - (box_.0 + (box_.2 - tile.2) / 2.0)).abs() < 1e-3,
        "the ornament is not centred: {tile:?} in {box_:?}",
    );

    let tiled =
        pages(&BLOCK_CSS.replace("background-repeat: no-repeat", "background-repeat: repeat"));
    let (.., repeat, _) = backgrounds(&tiled[0])[0];
    assert!(repeat, "repeat does not reach the display structure");
}

/// Acceptance: a url nothing resolves warns, naming the line and
/// column, and the page still sets.
#[test]
fn a_missing_url_warns_and_the_page_still_sets() {
    let css = "@page { size: 240pt 180pt; margin: 24pt }\nblockquote {\n  \
               background-image: url(nowhere.png);\n}\n";
    let output = laid_out(css);
    let complained: Vec<&Warning> = output
        .warnings
        .iter()
        .filter(|warning| warning.message.contains("nowhere.png"))
        .collect();
    assert_eq!(complained.len(), 1, "{:?}", output.warnings);
    assert_eq!(
        complained[0].origin.as_deref(),
        Some("background.css:3:3"),
        "the warning does not name where the url was written",
    );
    assert!(!output.pages.is_empty(), "the book set nothing");
    assert!(
        output.pages.iter().all(|page| backgrounds(page).is_empty()),
        "an image nothing resolved was painted anyway",
    );
    assert!(
        output.pages.iter().any(|page| page
            .items
            .iter()
            .any(|item| matches!(item, DrawItem::Text { .. }))),
        "the prose went with the image",
    );
}

/// A page that names no background paints none, so a book styled the
/// way the built-in sheet styles it is untouched.
#[test]
fn a_sheet_that_names_no_background_paints_none() {
    for page in pages("@page { size: 240pt 180pt; margin: 24pt }") {
        assert!(backgrounds(&page).is_empty(), "page {}", page.number);
    }
}
