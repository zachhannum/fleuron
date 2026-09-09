//! Property tests for exclusions: prose keeps clear of the plate, the
//! flow terminates, and a paragraph is broken again a bounded number
//! of times.

use fleuron::content::{Attributes, Block, Book, Inline, NodeId, Section};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::images::{Assets, ImageLoader};
use fleuron::layout::Paginator;
use fleuron::pages::{DrawItem, Page, Side};
use fleuron::style::{PageQuery, Situation, StyleTree, Stylesheets};
use proptest::prelude::*;

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

/// A PNG header and nothing else: layout sizes an image from it and
/// decodes nothing.
struct Png(u32, u32);

impl ImageLoader for Png {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        if url != "plate.png" {
            return None;
        }
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend(13u32.to_be_bytes());
        bytes.extend(b"IHDR");
        bytes.extend(self.0.to_be_bytes());
        bytes.extend(self.1.to_be_bytes());
        bytes.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        Some(bytes)
    }
}

fn text(value: &str) -> Vec<Inline> {
    vec![Inline::Text {
        id: NodeId::UNASSIGNED,
        value: value.to_string(),
        attributes: Attributes::default(),
        position: None,
        span: None,
    }]
}

fn paragraph(value: String) -> Block {
    Block::Paragraph {
        id: NodeId::UNASSIGNED,
        inlines: text(&value),
        attributes: Attributes::default(),
        position: None,
        span: None,
    }
}

fn plate() -> Block {
    Block::Image {
        id: NodeId::UNASSIGNED,
        url: "plate.png".into(),
        alt: "a plate".into(),
        attributes: Attributes::default(),
        position: None,
        span: None,
    }
}

/// Words short enough for the narrowest band these sheets leave. A
/// word wider than the band it is set in overflows it rather than
/// being dropped, which is the engine's answer to that everywhere,
/// and it is not what these properties are about.
fn word_strategy() -> impl Strategy<Value = String> {
    "[a-z]{1,6}"
}

fn paragraph_strategy() -> impl Strategy<Value = String> {
    proptest::collection::vec(word_strategy(), 1..60).prop_map(|words| words.join(" "))
}

/// One book of prose with plates written into it at `at`.
fn book_of(paragraphs: Vec<String>, at: Vec<usize>) -> Book {
    let mut blocks: Vec<Block> = paragraphs.into_iter().map(paragraph).collect();
    let mut written: Vec<usize> = at.iter().map(|index| index % (blocks.len() + 1)).collect();
    written.sort_unstable();
    written.dedup();
    for (offset, index) in written.into_iter().enumerate() {
        blocks.insert(index + offset, plate());
    }
    let mut book = Book {
        metadata: Default::default(),
        sections: vec![Section {
            id: NodeId::UNASSIGNED,
            source: None,
            title: None,
            blocks,
            position: None,
            span: None,
        }],
    };
    book.assign_node_ids();
    book
}

/// A book of prose with one or two plates anchored in it.
fn plated_strategy() -> impl Strategy<Value = Book> {
    (
        proptest::collection::vec(paragraph_strategy(), 1..12),
        proptest::collection::vec(0usize..24, 1..3),
    )
        .prop_map(|(paragraphs, at)| book_of(paragraphs, at))
}

/// The sheet under test: one page box on both sides of the spread,
/// and the plate anchored at a corner of the page area with prose set
/// on one side of it.
fn sheet(inset: &str, wrap: &str, margin: f32) -> String {
    format!(
        "@page {{ margin: 54pt }} \
         img {{ position: absolute; {inset}; margin: {margin}pt; wrap-flow: {wrap} }}"
    )
}

fn styles(book: &Book, css: &str) -> StyleTree {
    Stylesheets::parse(&[fleuron::style::Source::author("plates.css", css)])
        .compile(book, registry())
}

/// One book laid out: its pages, and how many times the flow had to
/// set a paragraph again.
fn paginate(book: &Book, css: &str, size: (u32, u32)) -> (Vec<Page>, u32) {
    let styles = styles(book, css);
    let assets = Assets::probe(book, &Png(size.0, size.1));
    let paginator = Paginator::with_assets(registry(), &styles, &assets);
    let pages = paginator.paginate(book);
    (pages, paginator.rebreaks())
}

/// A rectangle: `(left, top, right, bottom)`.
type Rect = (f32, f32, f32, f32);

fn plates(page: &Page) -> Vec<Rect> {
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Image { x, y, w, h, .. } => Some((*x, *y, *x + *w, *y + *h)),
            _ => None,
        })
        .collect()
}

/// Every run of text on a page, as the box its ink covers.
fn inked(page: &Page) -> Vec<Rect> {
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Text {
                x,
                y,
                font_id,
                size,
                glyphs,
                ..
            } => {
                let metrics = registry().metrics(*font_id)?;
                let upem = metrics.units_per_em as f32;
                let last = glyphs.last()?;
                let advance = registry().advance_width(*font_id, last.id).unwrap_or(0) as f32;
                Some((
                    *x,
                    y - metrics.ascender as f32 / upem * size,
                    last.x + advance / upem * size,
                    y - metrics.descender as f32 / upem * size,
                ))
            }
            _ => None,
        })
        .collect()
}

fn overlaps(one: Rect, other: Rect) -> bool {
    one.0 < other.2 - 1e-3
        && other.0 < one.2 - 1e-3
        && one.1 < other.3 - 1e-3
        && other.1 < one.3 - 1e-3
}

/// The insets and wrap side the properties are checked over: a plate
/// at either edge of the page area, with prose beside it.
fn sheets() -> Vec<String> {
    vec![
        sheet("top: 0; left: 0", "end", 6.0),
        sheet("top: 0; right: 0", "start", 6.0),
        sheet("bottom: 0; left: 0", "end", 0.0),
        sheet("top: 72pt; left: 72pt", "both", 6.0),
        sheet("top: 0; left: 0", "auto", 6.0),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// No line is set where a plate stands: the ink of every run is
    /// clear of every plate on its page, whichever side the sheet
    /// sets prose on.
    ///
    /// A plate that excludes nothing is the one exception, and the
    /// sheet that asks for that is not in this list.
    #[test]
    fn no_line_is_set_where_a_plate_stands(book in plated_strategy()) {
        for css in sheets().iter().take(4) {
            let (pages, _) = paginate(&book, css, (192, 192));
            for page in &pages {
                let plates = plates(page);
                for run in inked(page) {
                    for plate in &plates {
                        prop_assert!(
                            !overlaps(run, *plate),
                            "{css}\na run at {run:?} is set over the plate at {plate:?}",
                        );
                    }
                }
            }
        }
    }

    /// Every line stays inside the measure it was set to, plate or no
    /// plate.
    #[test]
    fn no_line_runs_past_the_measure(book in plated_strategy()) {
        for css in sheets() {
            let (pages, _) = paginate(&book, &css, (192, 192));
            let geometry = styles(&book, &css)
                .page(PageQuery {
                    name: None,
                    situation: Situation::Body(Side::Recto),
                })
                .geometry;
            let left = geometry.content_origin().0;
            let width = geometry.content_size().0;
            for page in &pages {
                for run in inked(page) {
                    prop_assert!(run.0 >= left - 1.0, "{css}: a run starts at {}", run.0);
                    prop_assert!(
                        run.2 <= left + width + 1.0,
                        "{css}: a run reaches {} past the measure",
                        run.2,
                    );
                }
            }
        }
    }

    /// The flow terminates, and it sets a paragraph again at most
    /// once where the paragraph starts and once more for each page
    /// boundary it crosses.
    #[test]
    fn the_flow_terminates_and_breaks_a_paragraph_a_bounded_number_of_times(
        book in plated_strategy(),
    ) {
        let paragraphs = book.sections[0]
            .blocks
            .iter()
            .filter(|block| matches!(block, Block::Paragraph { .. }))
            .count();
        for css in sheets() {
            let (pages, rebreaks) = paginate(&book, &css, (192, 192));
            prop_assert!(
                rebreaks as usize <= paragraphs + pages.len(),
                "{css}: {rebreaks} breaks over {paragraphs} paragraphs and {} pages",
                pages.len(),
            );
        }
    }

    /// A plated book lays out the same way twice, page for page and
    /// item for item.
    #[test]
    fn a_plated_book_lays_out_the_same_way_twice(book in plated_strategy()) {
        let css = sheet("top: 0; left: 0", "end", 6.0);
        let (once, _) = paginate(&book, &css, (192, 192));
        let (twice, _) = paginate(&book, &css, (192, 192));
        prop_assert_eq!(once.len(), twice.len());
        prop_assert_eq!(
            serde_json::to_string(&once).unwrap(),
            serde_json::to_string(&twice).unwrap(),
        );
    }
}
