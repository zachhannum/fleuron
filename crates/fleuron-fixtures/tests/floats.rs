//! A whole novel with an image floated beside its prose every few
//! paragraphs.
//!
//! A float lands where the flow meets it, so a novel is where one
//! meets the foot of a page, a column boundary and a chapter heading
//! that clears it. One page in three hundred is enough.

use fleuron::content::{Attributes, Block, Book, NodeId};
use fleuron::layout::layout_book;
use fleuron::style::{PageQuery, Situation, StyleTree};
use fleuron_fixtures::gate::Division;
use fleuron_fixtures::{Corpus, anchored_images, image_boxes, registry, run_boxes};

/// The sheet that floats the image: to the right of its column, a
/// third of the measure wide, with every chapter heading below it.
const CSS: &str = "img { float: right; width: 33%; margin: 0 0 6pt 12pt } \
                   h1, h2, h3 { clear: both }";

/// The gate novel with the map floated before every eighth paragraph.
fn floated() -> Book {
    let mut book = Corpus::GATE.book();
    for section in &mut book.sections {
        let mut blocks = Vec::with_capacity(section.blocks.len());
        let mut paragraphs = 0;
        for block in section.blocks.drain(..) {
            if matches!(block, Block::Paragraph { .. }) {
                if paragraphs % 8 == 0 {
                    blocks.push(Block::Image {
                        id: NodeId::UNASSIGNED,
                        url: anchored_images::URL.into(),
                        alt: "the plate beside the prose".into(),
                        attributes: Attributes::default(),
                        position: None,
                        span: None,
                    });
                }
                paragraphs += 1;
            }
            blocks.push(block);
        }
        section.blocks = blocks;
    }
    book.assign_node_ids();
    book
}

fn styles(book: &Book, division: Division) -> StyleTree {
    let css = format!("{}{CSS}", division.css());
    fleuron::style::Stylesheets::parse(&[fleuron::style::Source::author("floats.css", &css)])
        .compile(book, registry())
}

/// Whether two boxes, as `[left, top, right, bottom]`, share any area.
fn overlap(one: [f32; 4], other: [f32; 4]) -> bool {
    one[0] < other[2] - 1e-3
        && other[0] < one[2] - 1e-3
        && one[1] < other[3] - 1e-3
        && other[1] < one[3] - 1e-3
}

/// The gate novel with a float every eighth paragraph, in one column
/// and in two: every float is painted whole inside one column, and no
/// run of prose is set over one.
#[test]
fn a_novel_of_floats_sets_every_page_clear_of_them() {
    let book = floated();
    let floats = book
        .sections
        .iter()
        .flat_map(|section| &section.blocks)
        .filter(|block| matches!(block, Block::Image { .. }))
        .count();
    assert!(floats > 100, "a book of {floats} floats");
    for division in Division::ALL {
        let styles = styles(&book, division);
        assert!(styles.warnings().is_empty(), "{:?}", styles.warnings());
        let assets = anchored_images::assets(&book, &styles);
        let output = layout_book(&book, &styles, registry(), &assets);
        assert!(output.warnings.is_empty(), "{:?}", output.warnings);

        let mut painted = 0;
        for (index, page) in output.pages.iter().enumerate() {
            let geometry = styles
                .page(PageQuery {
                    name: Some("chapter"),
                    situation: Situation::Body(page.side),
                })
                .geometry;
            let (_, top) = geometry.content_origin();
            let (_, height) = geometry.content_size();
            let columns: Vec<(f32, f32)> = (0..geometry.column_count())
                .map(|column| {
                    let left = geometry.column_origin(column).0;
                    (left, left + geometry.measure())
                })
                .collect();
            let runs = run_boxes(page);
            for image in image_boxes(page) {
                painted += 1;
                assert!(
                    columns
                        .iter()
                        .any(|(left, right)| image[0] >= left - 1e-3 && image[2] <= right + 1e-3),
                    "{division:?}, page {}: the float at {image:?} is in no one column",
                    index + 1,
                );
                assert!(
                    image[1] >= top - 1e-3 && image[3] <= top + height + 1e-3,
                    "{division:?}, page {}: the float at {image:?} is split by the page",
                    index + 1,
                );
                for run in &runs {
                    assert!(
                        !overlap(*run, image),
                        "{division:?}, page {}: a run at {run:?} is set over the float at \
                         {image:?}",
                        index + 1,
                    );
                }
            }
        }
        assert_eq!(painted, floats, "{division:?}: every float is painted once");
    }
}

/// The same book laid out twice comes out the same, page for page and
/// item for item: two runs are byte-identical and the page count does
/// not move.
#[test]
fn a_novel_of_floats_lays_out_the_same_way_twice() {
    let book = floated();
    let styles = styles(&book, Division::TwoColumn);
    let assets = anchored_images::assets(&book, &styles);
    let once = layout_book(&book, &styles, registry(), &assets);
    let twice = layout_book(&book, &styles, registry(), &assets);
    assert_eq!(once.pages.len(), twice.pages.len());
    assert_eq!(
        fleuron::wire::encode(&once).expect("a display structure encodes"),
        fleuron::wire::encode(&twice).expect("a display structure encodes"),
    );
}
