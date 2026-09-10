//! Property tests for anchored images: the prose keeps clear of the
//! image, the flow terminates, and a paragraph is broken again a
//! bounded number of times.
//!
//! The same properties hold where the prose keeps clear of a traced
//! contour rather than of a box. The image those are checked over
//! covers the leading half of every one of its rows, so its contour
//! is exactly half its box and what the prose may reach is a
//! rectangle rather than an arithmetic exercise.

use fleuron::content::{Attributes, Block, Book, Inline, NodeId, Section};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::images::{Assets, Contours, ImageLoader};
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
        if url != "image.png" {
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

/// An RGBA PNG whose alpha covers the leading half of every row.
struct HalfAlpha(u32);

impl ImageLoader for HalfAlpha {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        if url != "image.png" {
            return None;
        }
        let side = self.0;
        let mut pixels = Vec::with_capacity((side * side * 4) as usize);
        for _ in 0..side {
            for x in 0..side {
                pixels.extend([0x22, 0x33, 0x44, if x < side / 2 { 0xFF } else { 0 }]);
            }
        }
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, side, side);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("the header writes");
        writer.write_image_data(&pixels).expect("the pixels write");
        writer.finish().expect("the file closes");
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

fn image() -> Block {
    Block::Image {
        id: NodeId::UNASSIGNED,
        url: "image.png".into(),
        alt: "an image".into(),
        attributes: Attributes::default(),
        position: None,
        span: None,
    }
}

/// Words short enough for the narrowest band these sheets leave. A word
/// wider than the band it is set in overflows the band rather than
/// falls out of the text. That is the engine's answer everywhere, and
/// it is not what these properties are about.
fn word_strategy() -> impl Strategy<Value = String> {
    "[a-z]{1,6}"
}

fn paragraph_strategy() -> impl Strategy<Value = String> {
    proptest::collection::vec(word_strategy(), 1..60).prop_map(|words| words.join(" "))
}

/// One book of prose with images written into it at `at`.
fn book_of(paragraphs: Vec<String>, at: Vec<usize>) -> Book {
    let mut blocks: Vec<Block> = paragraphs.into_iter().map(paragraph).collect();
    let mut written: Vec<usize> = at.iter().map(|index| index % (blocks.len() + 1)).collect();
    written.sort_unstable();
    written.dedup();
    for (offset, index) in written.into_iter().enumerate() {
        blocks.insert(index + offset, image());
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

/// A book of prose with one or two images anchored in it.
fn illustrated_strategy() -> impl Strategy<Value = Book> {
    (
        proptest::collection::vec(paragraph_strategy(), 1..12),
        proptest::collection::vec(0usize..24, 1..3),
    )
        .prop_map(|(paragraphs, at)| book_of(paragraphs, at))
}

/// The sheet under test: one page box on both sides of the spread. The
/// image is anchored at a corner of the page area, with the prose set
/// on one side of it.
fn sheet(inset: &str, wrap: &str, margin: f32) -> String {
    format!(
        "@page {{ margin: 54pt }} \
         img {{ position: absolute; {inset}; margin: {margin}pt; wrap-flow: {wrap} }}"
    )
}

fn styles(book: &Book, css: &str) -> StyleTree {
    Stylesheets::parse(&[fleuron::style::Source::author("anchored.css", css)])
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

/// The same over an image whose contour is traced from its alpha.
fn paginate_traced(book: &Book, css: &str, side: u32) -> (Vec<Page>, u32) {
    let styles = styles(book, css);
    let assets = Assets::probe(book, &HalfAlpha(side));
    let mut contours = Contours::none();
    contours.update(book, &styles, &assets);
    let paginator = Paginator::with_contours(registry(), &styles, &assets, &contours);
    let pages = paginator.paginate(book);
    (pages, paginator.rebreaks())
}

/// The leading half of every image on a page, which is what the
/// traced contour covers.
fn contoured(page: &Page) -> Vec<Rect> {
    painted(page)
        .into_iter()
        .map(|(left, top, right, bottom)| (left, top, (left + right) / 2.0, bottom))
        .collect()
}

/// The sheets the contour properties are checked over, which are the
/// exclusion sheets with the contour turned on.
fn traced_sheets() -> Vec<String> {
    sheets()
        .iter()
        .take(4)
        .map(|css| format!("{css} img {{ shape-outside: auto }}"))
        .collect()
}

/// A rectangle: `(left, top, right, bottom)`.
type Rect = (f32, f32, f32, f32);

fn painted(page: &Page) -> Vec<Rect> {
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

/// The insets and wrap side the properties are checked over: an image
/// at either edge of the page area, with the prose beside it.
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

    /// No line is set where an image stands: every run of every line is
    /// clear of every image on its page, whichever side the sheet sets
    /// the prose on.
    ///
    /// An image that excludes nothing is the one exception, and the
    /// sheet that asks for that is not in this list.
    #[test]
    fn no_line_is_set_where_an_anchored_image_stands(book in illustrated_strategy()) {
        for css in sheets().iter().take(4) {
            let (pages, _) = paginate(&book, css, (192, 192));
            for page in &pages {
                let images = painted(page);
                for run in inked(page) {
                    for image in &images {
                        prop_assert!(
                            !overlaps(run, *image),
                            "{css}\na run at {run:?} is set over the image at {image:?}",
                        );
                    }
                }
            }
        }
    }

    /// Every line stays inside the measure it was set to, image or no
    /// image.
    #[test]
    fn no_line_runs_past_the_measure(book in illustrated_strategy()) {
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
        book in illustrated_strategy(),
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

    /// An illustrated book lays out the same way twice, page for page
    /// and item for item.
    #[test]
    fn an_illustrated_book_lays_out_the_same_way_twice(book in illustrated_strategy()) {
        let css = sheet("top: 0; left: 0", "end", 6.0);
        let (once, _) = paginate(&book, &css, (192, 192));
        let (twice, _) = paginate(&book, &css, (192, 192));
        prop_assert_eq!(once.len(), twice.len());
        prop_assert_eq!(
            serde_json::to_string(&once).unwrap(),
            serde_json::to_string(&twice).unwrap(),
        );
    }

    /// No line is set where a traced contour stands. The image's
    /// alpha covers the leading half of it, so the prose may reach
    /// the other half and none of this one.
    #[test]
    fn no_line_is_set_where_a_traced_contour_stands(book in illustrated_strategy()) {
        for css in traced_sheets() {
            let (pages, _) = paginate_traced(&book, &css, 192);
            for page in &pages {
                let contours = contoured(page);
                for run in inked(page) {
                    for contour in &contours {
                        prop_assert!(
                            !overlaps(run, *contour),
                            "{css}\na run at {run:?} is set over the contour at {contour:?}",
                        );
                    }
                }
            }
        }
    }

    /// Every line stays inside the measure beside a contour too, and
    /// the flow terminates.
    #[test]
    fn no_line_runs_past_the_measure_beside_a_contour(book in illustrated_strategy()) {
        for css in traced_sheets() {
            let (pages, rebreaks) = paginate_traced(&book, &css, 192);
            let paragraphs = book.sections[0]
                .blocks
                .iter()
                .filter(|block| matches!(block, Block::Paragraph { .. }))
                .count();
            prop_assert!(
                rebreaks as usize <= paragraphs + pages.len(),
                "{css}: {rebreaks} breaks over {paragraphs} paragraphs",
            );
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

    /// A book set around a traced contour lays out the same way
    /// twice, page for page and item for item.
    #[test]
    fn a_contoured_book_lays_out_the_same_way_twice(book in illustrated_strategy()) {
        let css = format!("{} img {{ shape-outside: auto; shape-margin: 4pt }}",
            sheet("top: 0; left: 0", "end", 6.0));
        let (once, _) = paginate_traced(&book, &css, 192);
        let (twice, _) = paginate_traced(&book, &css, 192);
        prop_assert_eq!(
            serde_json::to_string(&once).unwrap(),
            serde_json::to_string(&twice).unwrap(),
        );
    }
}
