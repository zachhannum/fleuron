//! A whole novel with an image at the head of every chapter.
//!
//! The properties exclusions have to hold are about where an image
//! lands and what the prose beside it does, and a book-scale run is
//! where a flow that sets a line over an image shows: one page in
//! three hundred is enough.

use fleuron::layout::layout_book;
use fleuron::pages::{DrawItem, Page};
use fleuron_fixtures::gate::{Division, Illustration};
use fleuron_fixtures::{Corpus, anchored_images, registry, styles_on};

/// One page's images and one page's text, as the boxes they cover.
fn boxes(page: &Page) -> (Vec<[f32; 4]>, Vec<[f32; 4]>) {
    let mut images = Vec::new();
    let mut runs = Vec::new();
    for item in &page.items {
        match item {
            DrawItem::Image { x, y, w, h, .. } => images.push([*x, *y, *x + *w, *y + *h]),
            DrawItem::Text {
                x,
                y,
                font_id,
                size,
                glyphs,
                ..
            } => {
                let Some(metrics) = registry().metrics(*font_id) else {
                    continue;
                };
                let Some(last) = glyphs.last() else { continue };
                let upem = metrics.units_per_em as f32;
                let advance = registry().advance_width(*font_id, last.id).unwrap_or(0) as f32;
                runs.push([
                    *x,
                    y - metrics.ascender as f32 / upem * size,
                    last.x + advance / upem * size,
                    y - metrics.descender as f32 / upem * size,
                ]);
            }
            _ => {}
        }
    }
    (images, runs)
}

/// The gate novel with an image a chapter: every chapter's image is
/// painted, and no line of prose is set where one stands.
#[test]
fn a_novel_of_images_sets_every_page_clear_of_them() {
    let book = anchored_images::illustrated(&Corpus::GATE.book());
    let styles = styles_on(&book, Division::Undivided, Illustration::Anchored);
    let assets = anchored_images::assets(&book);
    let output = layout_book(&book, &styles, registry(), &assets);

    let mut painted = 0;
    for (index, page) in output.pages.iter().enumerate() {
        let (images, runs) = boxes(page);
        painted += images.len();
        for image in &images {
            for run in &runs {
                assert!(
                    run[0] >= image[2] - 1e-3
                        || image[0] >= run[2] - 1e-3
                        || run[1] >= image[3] - 1e-3
                        || image[1] >= run[3] - 1e-3,
                    "page {}: a run at {run:?} is set over the image at {image:?}",
                    index + 1,
                );
            }
        }
    }
    assert_eq!(
        painted,
        book.sections.len(),
        "every chapter's image is painted once",
    );
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
}

/// The same book laid out twice comes out the same, page for page and
/// item for item: two runs are byte-identical and the page count does
/// not move.
#[test]
fn a_novel_of_images_lays_out_the_same_way_twice() {
    let book = anchored_images::illustrated(&Corpus::GATE.book());
    let styles = styles_on(&book, Division::Undivided, Illustration::Anchored);
    let assets = anchored_images::assets(&book);
    let once = layout_book(&book, &styles, registry(), &assets);
    let twice = layout_book(&book, &styles, registry(), &assets);
    assert_eq!(once.pages.len(), twice.pages.len());
    assert_eq!(
        fleuron::wire::encode(&once).expect("a display structure encodes"),
        fleuron::wire::encode(&twice).expect("a display structure encodes"),
    );
}
