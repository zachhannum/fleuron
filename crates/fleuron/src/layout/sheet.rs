//! The sheet around the trim: the marks a printer cuts and registers
//! by, painted in the slug outside the bleed.

use crate::pages::{Corners, DrawItem, Radius};
use crate::style::{Color, Edges, Marks, PageGeometry};

/// How thick a mark is drawn, in points: a hairline.
const WEIGHT: f32 = 0.25;

/// How far past the bleed edge a mark begins, in points.
const GAP: f32 = 3.0;

/// How long a crop mark is, and how wide a registration cross is.
const LENGTH: f32 = 18.0;

/// The radius of the circle of a registration cross.
const RADIUS: f32 = 6.0;

/// The marks `geometry` asks for, around a trim of its size, in
/// trim coordinates. They paint over everything else on the page.
pub(super) fn marks(geometry: PageGeometry) -> Vec<DrawItem> {
    let Marks { crop, cross } = geometry.marks;
    let (width, height) = (geometry.width, geometry.height);
    let near = geometry.bleed() + GAP;
    let far = near + LENGTH;
    let mut items = Vec::new();
    if crop {
        for x in [0.0, width] {
            for y in [0.0, height] {
                // Out from the corner along the two edges that meet
                // there, starting past the bleed.
                let out_x = if x == 0.0 { -far } else { x + near };
                let out_y = if y == 0.0 { -far } else { y + near };
                items.push(line(out_x, y - WEIGHT / 2.0, LENGTH, WEIGHT));
                items.push(line(x - WEIGHT / 2.0, out_y, WEIGHT, LENGTH));
            }
        }
    }
    if cross {
        let middle = near + LENGTH / 2.0;
        for (x, y) in [
            (width / 2.0, -middle),
            (width / 2.0, height + middle),
            (-middle, height / 2.0),
            (width + middle, height / 2.0),
        ] {
            items.extend(target(x, y));
        }
    }
    items
}

/// A registration cross centered on one point: a circle and the two
/// lines through it.
fn target(x: f32, y: f32) -> [DrawItem; 3] {
    let half = LENGTH / 2.0;
    let round = Radius {
        x: RADIUS,
        y: RADIUS,
    };
    [
        line(x - half, y - WEIGHT / 2.0, LENGTH, WEIGHT),
        line(x - WEIGHT / 2.0, y - half, WEIGHT, LENGTH),
        DrawItem::Rounded {
            x: x - RADIUS,
            y: y - RADIUS,
            w: RADIUS * 2.0,
            h: RADIUS * 2.0,
            radii: Corners {
                top_left: round,
                top_right: round,
                bottom_right: round,
                bottom_left: round,
            },
            ring: Edges::all(WEIGHT),
            color: Color::BLACK,
            layer: DrawItem::PAGE_FURNITURE,
        },
    ]
}

fn line(x: f32, y: f32, w: f32, h: f32) -> DrawItem {
    DrawItem::Rect {
        x,
        y,
        w,
        h,
        color: Color::BLACK,
        layer: DrawItem::PAGE_FURNITURE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{AlignContent, Columns};

    fn geometry(bleed: Option<f32>, marks: Marks) -> PageGeometry {
        PageGeometry {
            width: 396.0,
            height: 612.0,
            margin: Edges::all(54.0),
            columns: Columns::undivided(11.0),
            align_content: AlignContent::Start,
            bleed,
            marks,
        }
    }

    /// Left, top, right and bottom of one item.
    fn bounds(item: &DrawItem) -> (f32, f32, f32, f32) {
        match item {
            DrawItem::Rect { x, y, w, h, .. } | DrawItem::Rounded { x, y, w, h, .. } => {
                (*x, *y, x + w, y + h)
            }
            other => panic!("a mark is a rule or a ring, not {other:?}"),
        }
    }

    fn overlaps(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> bool {
        a.0 <= b.2 && b.0 <= a.2 && a.1 <= b.3 && b.1 <= a.3
    }

    /// Acceptance: crop marks fall at the trim corners and touch
    /// neither the trim box nor each other.
    #[test]
    fn crop_marks_fall_at_the_corners_and_touch_nothing() {
        let crop = Marks {
            crop: true,
            cross: false,
        };
        for bleed in [Some(0.0), None, Some(8.5)] {
            let page = geometry(bleed, crop);
            let marks: Vec<_> = marks(page).iter().map(bounds).collect();
            assert_eq!(marks.len(), 8, "two marks at each of four corners");
            let trim = (0.0, 0.0, page.width, page.height);
            for (i, mark) in marks.iter().enumerate() {
                assert!(!overlaps(*mark, trim), "{mark:?} touches the trim");
                for other in &marks[i + 1..] {
                    assert!(!overlaps(*mark, *other), "{mark:?} touches {other:?}");
                }
                // Each one runs along a trim edge, out from a corner.
                let on_edge = |edge: f32, low: f32, high: f32| low <= edge && edge <= high;
                let along_x = [0.0, page.width]
                    .iter()
                    .any(|x| on_edge(*x, mark.0, mark.2));
                let along_y = [0.0, page.height]
                    .iter()
                    .any(|y| on_edge(*y, mark.1, mark.3));
                assert!(along_x || along_y, "{mark:?} is off every trim edge");
            }
        }
    }

    /// Part: crop and registration marks paint outside the bleed box,
    /// and inside the sheet.
    #[test]
    fn marks_paint_between_the_bleed_and_the_sheet_edge() {
        let both = Marks {
            crop: true,
            cross: true,
        };
        let page = geometry(Some(9.0), both);
        let (bleed, outset) = (page.bleed(), page.bleed() + page.slug());
        let bled = (-bleed, -bleed, page.width + bleed, page.height + bleed);
        let items = marks(page);
        assert_eq!(items.len(), 8 + 4 * 3);
        for item in &items {
            let mark = bounds(item);
            assert!(!overlaps(mark, bled), "{mark:?} is inside the bleed");
            assert!(mark.0 >= -outset && mark.1 >= -outset);
            assert!(mark.2 <= page.width + outset && mark.3 <= page.height + outset);
            assert_eq!(item.layer(), DrawItem::PAGE_FURNITURE);
        }
    }

    /// A page with no marks asks for none.
    #[test]
    fn no_marks_paints_nothing() {
        assert!(marks(geometry(Some(9.0), Marks::NONE)).is_empty());
    }
}
