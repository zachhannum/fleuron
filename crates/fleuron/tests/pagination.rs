//! Property tests for page assembly: determinism, page-count
//! stability, content-box fit, spread sides.

use fleuron::content::{Block, Book, Inline, NodeId, Section};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::layout::{PageGeometry, Paginator};
use fleuron::pages::{DrawItem, Page, Side};
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

/// A book of one section per chapter, each a heading plus prose
/// paragraphs.
fn book_strategy() -> impl Strategy<Value = Book> {
    proptest::collection::vec(paragraph_strategy(), 1..20).prop_map(|paragraphs| {
        let blocks: Vec<Block> = std::iter::once(Block::Heading {
            id: NodeId::UNASSIGNED,
            level: fleuron::content::HeadingLevel::H1,
            inlines: vec![Inline::Text {
                id: NodeId::UNASSIGNED,
                value: "Chapter".into(),
                position: None,
            }],
            position: None,
        })
        .chain(paragraphs.into_iter().map(|value| Block::Paragraph {
            id: NodeId::UNASSIGNED,
            inlines: vec![Inline::Text {
                id: NodeId::UNASSIGNED,
                value,
                position: None,
            }],
            position: None,
        }))
        .collect();
        Book {
            metadata: Default::default(),
            sections: vec![Section {
                id: NodeId::UNASSIGNED,
                source: None,
                title: None,
                blocks,
                position: None,
            }],
        }
    })
}

fn paginate(book: &Book) -> Vec<Page> {
    Paginator::new(registry(), PageGeometry::trade_paperback()).paginate(book)
}

/// Every paint op of every page lies inside that page's content box.
fn assert_pages_fit(pages: &[Page]) -> Result<(), TestCaseError> {
    let geometry = PageGeometry::trade_paperback();
    for page in pages {
        let (x, y) = geometry.content_origin(page.side);
        let (w, h) = geometry.content_size();
        for item in &page.items {
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
        let geometry = PageGeometry::trade_paperback();
        for page in &pages {
            let baselines: Vec<f32> = page
                .items
                .iter()
                .filter_map(|i| match i {
                    DrawItem::Text { y, .. } => Some(*y),
                    _ => None,
                })
                .collect();
            let mut sorted = baselines.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            prop_assert_eq!(&baselines, &sorted, "page {}", page.number);
            let (_, top) = geometry.content_origin(page.side);
            if let Some(last) = baselines.last() {
                prop_assert!(*last <= top + geometry.content_size().1 + 1e-3);
            }
        }
    }
}

/// Snapshot of the assembled display list for a two-chapter book:
/// the wire-format shape of page assembly — sides, numbering, the
/// first text baselines of the opening pages.
#[test]
fn page_assembly_snapshot() {
    let prose = "My father had a small estate in Nottinghamshire; I was bred a surgeon. ";
    let paragraph = Block::Paragraph {
        id: NodeId::UNASSIGNED,
        inlines: vec![Inline::Text {
            id: NodeId::UNASSIGNED,
            value: prose.repeat(6),
            position: None,
        }],
        position: None,
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
                }],
                position: None,
            },
            paragraph.clone(),
        ],
        position: None,
    };
    let book = Book {
        metadata: Default::default(),
        sections: vec![chapter("Chapter One"), chapter("Chapter Two")],
    };
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
                serde_json::json!({
                    "number": page.number,
                    "side": page.side,
                    "items": page.items.len(),
                    "first_baselines": firsts,
                })
            })
            .collect::<Vec<_>>()
    );
}
