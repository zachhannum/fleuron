//! Property tests for page assembly: determinism, page-count
//! stability, content-box fit, spread sides.

use fleuron::content::{Block, Book, Inline, NodeId, Section};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::layout::{Paginator, margin_band};
use fleuron::lines::ParagraphStyle;
use fleuron::pages::{DrawItem, Page, Side};
use fleuron::style::{Band, MarginBox, PageQuery, PageStyle, Situation, StyleTree};
use proptest::prelude::*;

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

fn word_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z]{1,12}"
}

fn paragraph_strategy() -> impl Strategy<Value = String> {
    proptest::collection::vec(word_strategy(), 1..60).prop_map(|words| words.join(" "))
}

/// One chapter: a heading and the prose under it.
fn chapter_strategy() -> impl Strategy<Value = Section> {
    proptest::collection::vec(paragraph_strategy(), 1..20).prop_map(|paragraphs| {
        let blocks: Vec<Block> = std::iter::once(Block::Heading {
            id: NodeId::UNASSIGNED,
            level: fleuron::content::HeadingLevel::H1,
            inlines: vec![Inline::Text {
                id: NodeId::UNASSIGNED,
                value: "Chapter".into(),
                position: None,
                span: None,
            }],
            position: None,
            span: None,
        })
        .chain(paragraphs.into_iter().map(|value| Block::Paragraph {
            id: NodeId::UNASSIGNED,
            inlines: vec![Inline::Text {
                id: NodeId::UNASSIGNED,
                value,
                position: None,
                span: None,
            }],
            position: None,
            span: None,
        }))
        .collect();
        Section {
            id: NodeId::UNASSIGNED,
            source: None,
            title: None,
            blocks,
            position: None,
            span: None,
        }
    })
}

fn book_of(sections: Vec<Section>) -> Book {
    let mut book = Book {
        metadata: Default::default(),
        sections,
    };
    book.assign_node_ids();
    book
}

/// A book of one section per chapter, each a heading plus prose
/// paragraphs.
fn book_strategy() -> impl Strategy<Value = Book> {
    chapter_strategy().prop_map(|chapter| book_of(vec![chapter]))
}

/// The same, several chapters over: what a page with two of them on it
/// needs to exist at all.
fn chapters_strategy() -> impl Strategy<Value = Book> {
    proptest::collection::vec(chapter_strategy(), 2..5).prop_map(book_of)
}

fn paginate(book: &Book) -> Vec<Page> {
    let styles = fleuron::style::defaults(book, registry());
    Paginator::new(registry(), &styles).paginate(book)
}

/// A page box divided in two, with a gutter wide enough that a line
/// in one column cannot be mistaken for a line in the other.
const TWO_COLUMNS: &str = "@page { column-count: 2; column-gap: 18pt }";

fn paginate_styled(css: &str, book: &Book) -> Vec<Page> {
    let styles =
        fleuron::style::Stylesheets::parse(&[fleuron::style::Source::author("columns.css", css)])
            .compile(book, registry());
    Paginator::new(registry(), &styles).paginate(book)
}

/// The master a page of a two-column book resolves to.
fn column_master(situation: Situation) -> &'static PageStyle {
    static STYLES: std::sync::OnceLock<StyleTree> = std::sync::OnceLock::new();
    STYLES
        .get_or_init(|| {
            let mut book = Book {
                metadata: Default::default(),
                sections: vec![Section {
                    id: NodeId::UNASSIGNED,
                    source: None,
                    title: None,
                    blocks: vec![Block::Paragraph {
                        id: NodeId::UNASSIGNED,
                        inlines: vec![Inline::Text {
                            id: NodeId::UNASSIGNED,
                            value: "prose".into(),
                            position: None,
                            span: None,
                        }],
                        position: None,
                        span: None,
                    }],
                    position: None,
                    span: None,
                }],
            };
            book.assign_node_ids();
            fleuron::style::Stylesheets::parse(&[fleuron::style::Source::author(
                "columns.css",
                TWO_COLUMNS,
            )])
            .compile(&book, registry())
        })
        .page(PageQuery {
            name: Some("chapter"),
            situation,
        })
}

/// The built-in sheet's own answers, which is where these properties
/// get the geometry and sizes they check against.
fn ua() -> &'static StyleTree {
    static STYLES: std::sync::OnceLock<StyleTree> = std::sync::OnceLock::new();
    STYLES.get_or_init(|| {
        let mut book = Book {
            metadata: Default::default(),
            sections: vec![Section {
                id: NodeId::UNASSIGNED,
                source: None,
                title: None,
                blocks: vec![Block::Paragraph {
                    id: NodeId::UNASSIGNED,
                    inlines: vec![Inline::Text {
                        id: NodeId::UNASSIGNED,
                        value: "prose".into(),
                        position: None,
                        span: None,
                    }],
                    position: None,
                    span: None,
                }],
                position: None,
                span: None,
            }],
        };
        book.assign_node_ids();
        fleuron::style::defaults(&book, registry())
    })
}

/// The master a page of a one-chapter book resolves to.
fn master(situation: Situation) -> &'static PageStyle {
    ua().page(PageQuery {
        name: Some("chapter"),
        situation,
    })
}

/// The style the built-in sheet gives the folio.
fn folio_style() -> ParagraphStyle {
    master(Situation::Body(Side::Recto))
        .margin_box(MarginBox::BottomCenter)
        .expect("body pages have a folio")
        .style
        .paragraph()
}

/// Width of a folio run in points, from the registry's advances.
fn folio_width_pt(glyphs: &[fleuron::pages::Glyph], size: f32) -> f32 {
    let font = folio_style().font_id;
    let upem = registry().metrics(font).unwrap().units_per_em as f32;
    glyphs
        .iter()
        .map(|g| registry().advance_width(font, g.id).unwrap_or(0) as f32)
        .sum::<f32>()
        / upem
        * size
}

/// A folio run read back as digits, inverting the cmap over 0-9.
fn folio_digits(glyphs: &[fleuron::pages::Glyph]) -> String {
    let font = folio_style().font_id;
    glyphs
        .iter()
        .filter_map(|g| ('0'..='9').find(|c| registry().char_glyph(font, *c) == Some(g.id)))
        .collect()
}

/// True when a paint op is page furniture rather than content: the
/// folio is the only furniture that paints, and its size identifies
/// it.
fn is_folio(item: &DrawItem) -> bool {
    matches!(item, DrawItem::Text { size, .. } if *size == folio_style().size)
}

/// Every content paint op of every page lies inside that page's
/// content box. The folio is furniture and lives in the margin box —
/// `folios_sit_in_the_margin_box` covers where.
fn assert_pages_fit(pages: &[Page]) -> Result<(), TestCaseError> {
    for page in pages {
        let geometry = master(Situation::Body(page.side)).geometry;
        let (x, y) = geometry.content_origin();
        let (w, h) = geometry.content_size();
        for item in &page.items {
            if is_folio(item) {
                continue;
            }
            let DrawItem::Text {
                x: tx,
                y: ty,
                glyphs,
                ..
            } = item
            else {
                continue;
            };
            prop_assert!(
                *ty >= y - 1e-3 && *ty <= y + h + 1e-3,
                "page {}: baseline {ty} outside the content box",
                page.number
            );
            prop_assert!(
                *tx >= x - 1e-3 && *tx <= x + w + 1e-3,
                "page {}: run origin {tx} outside the content box",
                page.number
            );
            for glyph in glyphs {
                prop_assert!(
                    glyph.x >= x - 1e-3,
                    "page {}: glyph at {} left of the content box",
                    page.number,
                    glyph.x
                );
            }
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// Pagination is deterministic: two runs produce identical page
    /// counts, numbering, sides, and baselines — the page-count
    /// stability acceptance, checked run over run.
    #[test]
    fn pagination_is_deterministic(book in book_strategy()) {
        let first = paginate(&book);
        let second = paginate(&book);
        prop_assert_eq!(first.len(), second.len());
        for (a, b) in first.iter().zip(&second) {
            prop_assert_eq!(a.number, b.number);
            prop_assert_eq!(a.side, b.side);
            prop_assert_eq!(a.items.len(), b.items.len());
            for (item_a, item_b) in a.items.iter().zip(&b.items) {
                if let (DrawItem::Text { x: ax, y: ay, .. }, DrawItem::Text { x: bx, y: by, .. }) =
                    (item_a, item_b)
                {
                    prop_assert!((ax - bx).abs() < 1e-6);
                    prop_assert!((ay - by).abs() < 1e-6);
                }
            }
        }
    }

    /// The mapping from page to section is a layout result like any
    /// other: two runs name the same sections on the same pages.
    #[test]
    fn the_section_mapping_is_stable_across_runs(book in chapters_strategy()) {
        let first = paginate(&book);
        let second = paginate(&book);
        prop_assert_eq!(first.len(), second.len());
        for (a, b) in first.iter().zip(&second) {
            prop_assert_eq!(&a.sections, &b.sections);
        }
    }

    /// No line crosses a page boundary: every baseline sits inside
    /// its page's content box.
    #[test]
    fn no_line_crosses_a_page_boundary(book in book_strategy()) {
        let pages = paginate(&book);
        prop_assert!(!pages.is_empty());
        assert_pages_fit(&pages)?;
    }

    /// Numbering and sides: pages number densely from 1, odd is
    /// recto, and blank pages only ever appear as versos (a chapter
    /// that lands past a verso skips it).
    #[test]
    fn pages_number_denseley_and_alternate_sides(book in book_strategy()) {
        let pages = paginate(&book);
        for (i, page) in pages.iter().enumerate() {
            prop_assert_eq!(page.number, i as u32 + 1);
            prop_assert_eq!(page.side, Side::of_number(page.number));
            if page.items.is_empty() {
                prop_assert_eq!(page.side, Side::Verso, "blank recto at {}", page.number);
            }
        }
    }

    /// Baselines are monotonically increasing down each page, and no
    /// page overflows its content box (stacking leaves the box after
    /// the last baseline).
    #[test]
    fn baselines_increase_down_each_page(book in book_strategy()) {
        let pages = paginate(&book);
        for page in &pages {
            let geometry = master(Situation::Body(page.side)).geometry;
            let baselines: Vec<f32> = page
                .items
                .iter()
                .filter(|i| !is_folio(i))
                .filter_map(|i| match i {
                    DrawItem::Text { y, .. } => Some(*y),
                    _ => None,
                })
                .collect();
            let mut sorted = baselines.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            prop_assert_eq!(&baselines, &sorted, "page {}", page.number);
            let (_, top) = geometry.content_origin();
            if let Some(last) = baselines.last() {
                prop_assert!(*last <= top + geometry.content_size().1 + 1e-3);
            }
        }
    }

    /// Every folio, on any book, sits in the bottom margin box: below
    /// the content area, inside the folio band, centered on the trim —
    /// and reads as its own page number.
    #[test]
    fn folios_sit_in_the_margin_box(book in book_strategy()) {
        let pages = paginate(&book);
        let recto = master(Situation::Body(Side::Recto));
        let geometry = recto.geometry;
        let (band_top, band_height) = margin_band(recto, Band::Bottom, folio_style());
        let (_, content_top) = geometry.content_origin();
        let content_bottom = content_top + geometry.content_size().1;
        for page in &pages {
            let folios: Vec<&DrawItem> = page.items.iter().filter(|i| is_folio(i)).collect();
            prop_assert!(folios.len() <= 1, "page {} has {} folios", page.number, folios.len());
            for item in folios {
                let DrawItem::Text { x, y, glyphs, size, .. } = item else { continue };
                prop_assert!(
                    *y > content_bottom,
                    "page {}: folio baseline {y} inside the content area",
                    page.number
                );
                prop_assert!(
                    *y >= band_top && *y <= band_top + band_height,
                    "page {}: folio baseline {y} outside the margin box",
                    page.number
                );
                prop_assert!(
                    *y < geometry.height,
                    "page {}: folio baseline {y} off the trim",
                    page.number
                );
                let width = folio_width_pt(glyphs, *size);
                prop_assert!(
                    (x + width / 2.0 - geometry.width / 2.0).abs() < 1e-3,
                    "page {}: folio off-center at {}",
                    page.number,
                    x + width / 2.0
                );
                prop_assert_eq!(
                    folio_digits(glyphs),
                    page.number.to_string(),
                    "page {} shows the wrong folio",
                    page.number
                );
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    /// Acceptance: no line exceeds the column measure. Every glyph of
    /// every run sits between the leading edge of the column it was
    /// set in and that column's measure.
    #[test]
    fn no_line_exceeds_the_column_measure(book in book_strategy()) {
        let pages = paginate_styled(TWO_COLUMNS, &book);
        prop_assert!(!pages.is_empty());
        for page in &pages {
            let geometry = column_master(Situation::Body(page.side)).geometry;
            for (column, x, right) in glyph_edges(page, geometry) {
                let origin = geometry.column_origin(column).0;
                prop_assert!(
                    x >= origin - 1e-3,
                    "page {}: a glyph at {x} sits left of column {column}",
                    page.number
                );
                prop_assert!(
                    right <= origin + geometry.measure() + 1e-3,
                    "page {}: a line reaches {right}, past column {column}",
                    page.number
                );
            }
        }
    }

    /// Acceptance: nothing crosses a gutter. No glyph box falls
    /// inside `column-gap`.
    #[test]
    fn no_glyph_falls_in_a_gutter(book in book_strategy()) {
        let pages = paginate_styled(TWO_COLUMNS, &book);
        for page in &pages {
            let geometry = column_master(Situation::Body(page.side)).geometry;
            for column in 1..geometry.column_count() {
                let gutter = geometry.column_origin(column).0 - geometry.columns.gap;
                for (_, x, right) in glyph_edges(page, geometry) {
                    prop_assert!(
                        right <= gutter + 1e-3 || x >= gutter + geometry.columns.gap - 1e-3,
                        "page {}: a glyph box {x}..{right} lies in the gutter at {gutter}",
                        page.number
                    );
                }
            }
        }
    }

    /// Acceptance: a two-column book lays out the same twice: the
    /// same pages, the same columns, the same glyphs in the same
    /// places.
    #[test]
    fn a_divided_page_box_is_deterministic(book in book_strategy()) {
        let first = paginate_styled(TWO_COLUMNS, &book);
        let second = paginate_styled(TWO_COLUMNS, &book);
        prop_assert_eq!(first.len(), second.len());
        for (a, b) in first.iter().zip(&second) {
            prop_assert!(a == b, "page {} differs between runs", a.number);
        }
    }
}

/// Every glyph box of one page's content: the column it was set in,
/// its left edge, and its right. The folio is furniture and belongs
/// to no column.
fn glyph_edges(page: &Page, geometry: fleuron::style::PageGeometry) -> Vec<(u32, f32, f32)> {
    let mut edges = Vec::new();
    for item in &page.items {
        if is_folio(item) {
            continue;
        }
        let DrawItem::Text { glyphs, .. } = item else {
            continue;
        };
        let (Some(first), Some(last)) = (glyphs.first(), glyphs.last()) else {
            continue;
        };
        let column = (0..geometry.column_count())
            .rev()
            .find(|column| first.x >= geometry.column_origin(*column).0 - 1e-3)
            .unwrap_or(0);
        edges.push((column, first.x, last.x));
    }
    edges
}

/// Snapshot of the assembled display structure for a two-chapter book:
/// the wire-format shape of page assembly and page furniture — sides,
/// numbering, the first text baselines, and the folio each page does
/// or does not get.
#[test]
fn page_assembly_snapshot() {
    let prose = "My father had a small estate in Nottinghamshire; I was bred a surgeon. ";
    let paragraph = Block::Paragraph {
        id: NodeId::UNASSIGNED,
        inlines: vec![Inline::Text {
            id: NodeId::UNASSIGNED,
            value: prose.repeat(60),
            position: None,
            span: None,
        }],
        position: None,
        span: None,
    };
    let chapter = |title: &str| Section {
        id: NodeId::UNASSIGNED,
        source: None,
        title: None,
        blocks: vec![
            Block::Heading {
                id: NodeId::UNASSIGNED,
                level: fleuron::content::HeadingLevel::H1,
                inlines: vec![Inline::Text {
                    id: NodeId::UNASSIGNED,
                    value: title.into(),
                    position: None,
                    span: None,
                }],
                position: None,
                span: None,
            },
            paragraph.clone(),
        ],
        position: None,
        span: None,
    };
    let mut book = Book {
        metadata: Default::default(),
        sections: vec![chapter("Chapter One"), chapter("Chapter Two")],
    };
    book.assign_node_ids();
    let pages = paginate(&book);
    insta::assert_json_snapshot!(
        pages
            .iter()
            .map(|page| {
                let firsts: Vec<(f32, f32, f32, usize)> = page
                    .items
                    .iter()
                    .filter_map(|item| match item {
                        DrawItem::Text {
                            x, y, size, glyphs, ..
                        } => Some((*x, *y, *size, glyphs.len())),
                        _ => None,
                    })
                    .take(3)
                    .collect();
                let folio = page.items.iter().find(|i| is_folio(i)).map(|item| {
                    let DrawItem::Text {
                        x, y, size, glyphs, ..
                    } = item
                    else {
                        unreachable!()
                    };
                    (*x, *y, *size, folio_digits(glyphs))
                });
                serde_json::json!({
                    "number": page.number,
                    "side": page.side,
                    "sections": page.sections.iter().map(|id| id.get()).collect::<Vec<_>>(),
                    "items": page.items.len(),
                    "first_baselines": firsts,
                    "folio": folio,
                })
            })
            .collect::<Vec<_>>()
    );
}
