//! Alpha, opacity and rounded corners: what a sheet sets, and what the
//! display structure carries for it.
//!
//! One fixture, a chapter with a quotation in it, set under sheets
//! that tint the quotation, fade it, and round its corners.

use fleuron::content::{Attributes, Block, Book, HeadingLevel, Inline, NodeId, Section};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::images::{Assets, ImageLoader};
use fleuron::layout::layout_book;
use fleuron::pages::{Corners, DrawItem, Page, Radius};
use fleuron::style::{Color, Coord, CornerRadius, Edges, Source, StyleTree, Stylesheets};
use fleuron::wire;

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

/// A PNG header of a given pixel size. Layout reads the header and
/// nothing else, so this is a whole image as far as the display
/// structure is concerned.
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

/// The host side: one ornament.
struct Ornament;

impl ImageLoader for Ornament {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        (url == "ornament.png").then(|| png(24, 24))
    }
}

/// Four corners of one radius, in points.
fn round(radius: f32) -> Corners {
    let radius = Radius {
        x: radius,
        y: radius,
    };
    Corners {
        top_left: radius,
        top_right: radius,
        bottom_right: radius,
        bottom_left: radius,
    }
}

/// Every rounded box on a page that fills a ring, with its ring and
/// its colour.
fn rings(page: &Page) -> Vec<(Edges, Color)> {
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Rounded { ring, color, .. } if *ring != Edges::all(0.0) => {
                Some((*ring, *color))
            }
            _ => None,
        })
        .collect()
}

/// Part: `border-radius` and its per-corner longhands read into each
/// corner, a slash gives the radii down the sides, and a negative
/// radius is not a value.
#[test]
fn border_radius_and_its_longhands_read_into_each_corner() {
    let book = fixture();
    let styles = styles(
        &book,
        "blockquote { border-radius: 1pt 2pt 3pt / 4pt; border-bottom-left-radius: 10% 5pt }",
    );
    let Block::Blockquote { id, .. } = &book.sections[0].blocks[1] else {
        unreachable!("the fixture's second block is a quotation")
    };
    let radius = styles.style(*id).border_radius;
    let points = |x: f32, y: f32| CornerRadius {
        x: Coord::Points(x),
        y: Coord::Points(y),
    };
    assert_eq!(radius.top_left, points(1.0, 4.0));
    assert_eq!(radius.top_right, points(2.0, 4.0));
    assert_eq!(radius.bottom_right, points(3.0, 4.0));
    assert_eq!(
        radius.bottom_left,
        CornerRadius {
            x: Coord::Percent(10.0),
            y: Coord::Points(5.0),
        }
    );

    let negative = Stylesheets::parse(&[Source::author(
        "negative.css",
        "p {\n  border-radius: -1pt;\n}",
    )]);
    assert_eq!(negative.warnings().len(), 1, "{:?}", negative.warnings());
}

/// Acceptance: `border-radius: 3pt` rounds a tinted box's corners, and
/// the background image over the tint is clipped to the same curve.
#[test]
fn a_border_radius_rounds_a_tinted_box_and_clips_its_image_to_the_same_curve() {
    let book = fixture();
    let styles = styles(
        &book,
        "blockquote { border-radius: 3pt; background-color: #f4f1ea; \
         background-image: url(ornament.png) }",
    );
    let assets = Assets::probe(&book, &styles, &Ornament);
    let output = layout_book(&book, &styles, registry(), &assets);
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
    let page = &output.pages[0];

    let tint = page
        .items
        .iter()
        .find_map(|item| match item {
            DrawItem::Rounded {
                x,
                y,
                w,
                h,
                radii,
                ring,
                color,
                ..
            } if *color == TINT => Some(([*x, *y, *w, *h], *radii, *ring)),
            _ => None,
        })
        .expect("the tint is a rounded box");
    assert_eq!(tint.1, round(3.0));
    assert_eq!(tint.2, Edges::all(0.0), "the tint fills the whole box");
    assert!(tints(page).is_empty(), "the tint is painted square as well");

    let image = page
        .items
        .iter()
        .find_map(|item| match item {
            DrawItem::Background {
                x, y, w, h, radii, ..
            } => Some(([*x, *y, *w, *h], *radii)),
            _ => None,
        })
        .expect("the ornament is behind the quotation");
    assert_eq!(
        image,
        (tint.0, tint.1),
        "the image is not clipped to the tint's curve"
    );
}

/// A rounded border is one ring where its edges share a colour, and a
/// ring for each edge where they do not.
#[test]
fn a_rounded_border_is_one_ring_or_a_ring_per_edge() {
    let blue = Color::rgb(0x33, 0x66, 0x99);
    let red = Color::rgb(0xb4, 0x1e, 0x1e);
    let one = pages("blockquote { border-radius: 6pt; border: 2pt solid #336699 }");
    assert_eq!(rings(&one[0]), [(Edges::all(2.0), blue)]);

    let four = pages(
        "blockquote { border-radius: 6pt; border: 2pt solid; \
         border-color: #336699 #336699 #336699 #b41e1e }",
    );
    let none = Edges::all(0.0);
    assert_eq!(
        rings(&four[0]),
        [
            (Edges { top: 2.0, ..none }, blue),
            (Edges { right: 2.0, ..none }, blue),
            (
                Edges {
                    bottom: 2.0,
                    ..none
                },
                blue
            ),
            (Edges { left: 2.0, ..none }, red),
        ]
    );
}

/// Acceptance: a book whose sheet uses no alpha, opacity or radius
/// produces a display structure byte-identical to before the change.
/// The checked-in snapshots hold what it was. Here, the values every
/// colour and block start from change nothing: a sheet that writes them
/// out encodes to the same bytes as one that leaves them out, and a
/// style tree that uses none of them describes itself as it did.
#[test]
fn a_sheet_without_alpha_opacity_or_radius_sets_the_book_unchanged() {
    let book = fixture();
    let plain = "blockquote { background-color: #f4f1ea; border: 1pt solid #336699 }\n\
                 h1 { color: #b41e1e }";
    let written = "blockquote { background-color: rgba(244, 241, 234, 1); \
                   border: 1pt solid #336699ff; opacity: 1; border-radius: 0 }\n\
                   h1 { color: #b41e1eff }";
    let encoded = |css: &str| {
        let styles = styles(&book, css);
        let output = layout_book(&book, &styles, registry(), &Assets::none());
        assert!(
            output.pages[0]
                .items
                .iter()
                .all(|item| !matches!(item, DrawItem::Rounded { .. })),
            "a square box painted a rounded item"
        );
        wire::encode(&output).expect("a display structure encodes")
    };
    assert_eq!(encoded(plain), encoded(written));

    let described = serde_json::to_string(&styles(&book, plain)).expect("a style tree");
    assert!(!described.contains("\"opacity\""), "{described}");
    assert!(!described.contains("\"border_radius\""), "{described}");
}

/// The fixture with a quotation long enough to break across a page
/// turn.
fn long_quotation() -> Book {
    let mut book = fixture();
    let Block::Blockquote { blocks, .. } = &mut book.sections[0].blocks[1] else {
        unreachable!("the fixture's second block is a quotation")
    };
    *blocks = (0..12)
        .map(|_| {
            paragraph(
                "Quoted: the wind came off the water and the harbour lights went out, \
                 one after another, until the quay was dark.",
            )
        })
        .collect();
    book.assign_node_ids();
    book
}

/// Where a page turn breaks a rounded block, `slice` leaves the
/// corners at the break square, and `clone` rounds all four corners of
/// every part.
#[test]
fn a_page_turn_squares_the_corners_at_the_break_unless_the_box_is_cloned() {
    let book = long_quotation();
    let corners = |css: &str| -> Vec<Corners> {
        let styles = styles(&book, css);
        layout_book(&book, &styles, registry(), &Assets::none())
            .pages
            .iter()
            .flat_map(|page| page.items.iter())
            .filter_map(|item| match item {
                DrawItem::Rounded { radii, color, .. } if *color == TINT => Some(*radii),
                _ => None,
            })
            .collect()
    };
    let tint = "blockquote { border-radius: 3pt; background-color: #f4f1ea }";
    let sliced = corners(tint);
    assert!(sliced.len() >= 2, "the quotation did not break: {sliced:?}");
    let three = Radius { x: 3.0, y: 3.0 };
    assert_eq!(
        sliced[0],
        Corners {
            top_left: three,
            top_right: three,
            ..Corners::SQUARE
        }
    );
    assert_eq!(
        sliced[sliced.len() - 1],
        Corners {
            bottom_right: three,
            bottom_left: three,
            ..Corners::SQUARE
        }
    );
    assert!(sliced[1..sliced.len() - 1].iter().all(Corners::is_square));

    let cloned = corners(&format!(
        "{tint} blockquote {{ box-decoration-break: clone }}"
    ));
    assert_eq!(cloned.len(), sliced.len());
    assert!(
        cloned.iter().all(|radii| *radii == round(3.0)),
        "{cloned:?}"
    );
}
