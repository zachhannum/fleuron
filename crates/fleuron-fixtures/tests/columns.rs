//! A whole novel on a divided page box.
//!
//! The properties columns have to hold are about where a fragment
//! lands, and a book-scale run is where a flow that carries into the
//! wrong column shows: one page in three hundred is enough.

use fleuron::images::Assets;
use fleuron::layout::layout_book;
use fleuron::pages::{DrawItem, Page};
use fleuron::style::PageGeometry;
use fleuron_fixtures::gate::Division;
use fleuron_fixtures::{Corpus, registry, styles_on};

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
    let styles = styles_on(&book, Division::TwoColumn);
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
