//! A whole novel on a divided page box, with and without images
//! anchored to it.
//!
//! The properties columns have to hold are about where a fragment
//! lands, and a book-scale run is where a flow that carries into the
//! wrong column shows: one page in three hundred is enough. The same
//! run with an image at the head of every chapter is where a column
//! that reads the page's exclusions rather than its own shows.

use fleuron::images::Assets;
use fleuron::layout::layout_book;
use fleuron::pages::{DrawItem, Page};
use fleuron::style::PageGeometry;
use fleuron_fixtures::gate::{Division, Illustration};
use fleuron_fixtures::{Corpus, anchored_images, image_boxes, registry, run_boxes, styles_on};

/// The page box the gate novel is set on, which is the same on both
/// sides of the spread but for where it starts.
fn geometry(styles: &fleuron::style::StyleTree, page: &Page) -> PageGeometry {
    styles
        .page(fleuron::style::PageQuery {
            name: Some("chapter"),
            situation: fleuron::style::Situation::Body(page.side),
        })
        .geometry
}

/// Every glyph box of one page's content, as `(column, left, right)`.
/// The folio is furniture and sits in the margin, below every column.
fn glyphs(page: &Page, geometry: PageGeometry, foot: f32) -> Vec<(u32, f32, f32)> {
    let mut boxes = Vec::new();
    for item in &page.items {
        let DrawItem::Text { y, glyphs, .. } = item else {
            continue;
        };
        if *y > foot {
            continue;
        }
        let (Some(first), Some(last)) = (glyphs.first(), glyphs.last()) else {
            continue;
        };
        let column = (0..geometry.column_count())
            .rev()
            .find(|column| first.x >= geometry.column_origin(*column).0 - 1e-3)
            .unwrap_or(0);
        boxes.push((column, first.x, last.x));
    }
    boxes
}

/// The gate novel in two columns: every line sits inside a column,
/// nothing lies in a gutter, and the columns of a page fill in order.
#[test]
fn a_novel_in_two_columns_fills_every_page_column_by_column() {
    let book = Corpus::GATE.book();
    let styles = styles_on(&book, Division::TwoColumn, Illustration::Bare);
    let output = layout_book(&book, &styles, registry(), &Assets::none());
    let mut divided = 0;
    for page in &output.pages {
        let geometry = geometry(&styles, page);
        assert_eq!(geometry.column_count(), 2);
        let (_, top) = geometry.content_origin();
        let foot = top + geometry.content_size().1;
        let measure = geometry.measure();
        let gutter = geometry.column_origin(1).0 - geometry.columns.gap;

        for (column, left, right) in glyphs(page, geometry, foot) {
            let origin = geometry.column_origin(column).0;
            assert!(
                left >= origin - 1e-3 && right <= origin + measure + 1e-3,
                "page {}: a glyph box {left}..{right} leaves column {column}",
                page.number,
            );
            assert!(
                right <= gutter + 1e-3 || left >= gutter + geometry.columns.gap - 1e-3,
                "page {}: a glyph box {left}..{right} lies in the gutter",
                page.number,
            );
        }

        // The second column of a page opens above the foot of the
        // first: the flow fills one before it fills the next.
        let baselines = |column: u32| {
            let origin = geometry.column_origin(column).0;
            page.items
                .iter()
                .filter_map(|item| match item {
                    DrawItem::Text { x, y, .. }
                        if *y <= foot && *x >= origin - 1e-3 && *x <= origin + measure + 1e-3 =>
                    {
                        Some(*y)
                    }
                    _ => None,
                })
                .collect::<Vec<f32>>()
        };
        let second = baselines(1);
        if second.is_empty() {
            continue;
        }
        divided += 1;
        let first = baselines(0);
        assert!(
            !first.is_empty(),
            "page {}: the second column filled before the first",
            page.number,
        );
        assert!(
            second[0] <= first[first.len() - 1] + 1e-3,
            "page {}: the second column opens below the foot of the first",
            page.number,
        );
    }
    assert!(
        divided * 4 > output.pages.len(),
        "only {divided} of {} pages reached a second column",
        output.pages.len(),
    );
}

/// The gate novel in two columns with an image at the head of every
/// chapter: the column properties hold with an exclusion on the page,
/// no line is set where an image stands, and the run is repeatable.
#[test]
fn a_two_column_novel_of_images_holds_the_column_and_the_exclusion() {
    let book = anchored_images::illustrated(&Corpus::GATE.book());
    let styles = styles_on(&book, Division::TwoColumn, Illustration::Anchored);
    let assets = anchored_images::assets(&book);
    let output = layout_book(&book, &styles, registry(), &assets);
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);

    let mut painted = 0;
    let mut beside = 0;
    for page in &output.pages {
        let geometry = geometry(&styles, page);
        assert_eq!(geometry.column_count(), 2);
        let (_, top) = geometry.content_origin();
        let foot = top + geometry.content_size().1;
        let measure = geometry.measure();
        let gutter = geometry.column_origin(1).0 - geometry.columns.gap;

        // Nothing crosses a gutter, the image included: a line sits
        // in the column it was set in, and the space between two
        // columns holds nothing.
        for (column, left, right) in glyphs(page, geometry, foot) {
            let origin = geometry.column_origin(column).0;
            assert!(
                left >= origin - 1e-3 && right <= origin + measure + 1e-3,
                "page {}: a glyph box {left}..{right} leaves column {column}",
                page.number,
            );
            assert!(
                right <= gutter + 1e-3 || left >= gutter + geometry.columns.gap - 1e-3,
                "page {}: a glyph box {left}..{right} lies in the gutter",
                page.number,
            );
        }

        // No line of either column is set where the image stands.
        let images = image_boxes(page);
        let runs = run_boxes(page);
        painted += images.len();
        for image in &images {
            for run in &runs {
                assert!(
                    run[0] >= image[2] - 1e-3
                        || image[0] >= run[2] - 1e-3
                        || run[1] >= image[3] - 1e-3
                        || image[1] >= run[3] - 1e-3,
                    "page {}: a run at {run:?} is set over the image at {image:?}",
                    page.number,
                );
                beside += usize::from(run[1] < image[3] && run[3] > image[1]);
            }
        }
    }
    assert_eq!(
        painted,
        book.sections.len(),
        "every chapter's image is painted once",
    );
    assert!(beside > 0, "no line was set beside an image at all");

    // The same book laid out again is the same bytes, page for page
    // and item for item.
    let twice = layout_book(&book, &styles, registry(), &assets);
    assert_eq!(output.pages.len(), twice.pages.len());
    assert_eq!(
        fleuron::wire::encode(&output).expect("a display structure encodes"),
        fleuron::wire::encode(&twice).expect("a display structure encodes"),
    );
}
