//! An image floated to one side of its column: the room it keeps, and
//! the blocks that start below it.
//!
//! A float is placed where the flow meets it, in the column being
//! filled, and takes no height there. Its rectangle is a hole in that
//! column for the paragraphs placed after it, which are broken again
//! against the bands it leaves, as they are beside an anchored image.

use crate::style::{Clear, Float};

use super::flow::{Flow, Placed};
use super::fragment::{Fragment, Piece};

/// The margin box of one float, from the leading edge of its column
/// and the top of its fragment.
#[derive(Debug, Clone, Copy)]
pub(super) struct Aside {
    pub(super) x: f32,
    pub(super) width: f32,
    pub(super) height: f32,
    pub(super) side: Float,
}

impl Placed {
    /// How far down the column it reaches, a float included.
    pub(super) fn foot(&self) -> f32 {
        self.top
            + self
                .float
                .map_or(self.height, |float| float.height.max(self.height))
    }
}

impl Flow<'_, '_> {
    /// Whether nothing but floats stands in the column being filled.
    /// What is placed next drops its lead, as it would in an empty
    /// column.
    pub(super) fn at_head(&self) -> bool {
        self.placed[self.column_start..]
            .iter()
            .all(|placed| placed.float.is_some())
    }

    /// Where `fragment`, a float, starts: at the cursor, or under the
    /// floats already in the column that it would overlap.
    pub(super) fn float_top(&self, fragment: &Fragment) -> f32 {
        let Piece::Float(float) = &fragment.piece else {
            return self.cursor;
        };
        let (left, right) = (fragment.x, fragment.x + float.outer);
        self.placed[self.column_start..]
            .iter()
            .filter(|placed| {
                placed
                    .float
                    .is_some_and(|other| other.x < right && left < other.x + other.width)
            })
            .map(Placed::foot)
            .fold(self.cursor, f32::max)
    }

    /// Moves the cursor down past the floats in the column being
    /// filled that `fragment` starts below: the ones its block clears,
    /// or every one for a fragment that is not a line of prose, which
    /// nothing sets beside a float.
    pub(super) fn clearance(&mut self, fragment: &Fragment) {
        let clear = match fragment.piece {
            Piece::Anchor(_) | Piece::Float(_) => return,
            _ if fragment.reflow.is_none() => Clear::Both,
            _ => fragment.clear,
        };
        if clear == Clear::None {
            return;
        }
        let foot = self.placed[self.column_start..]
            .iter()
            .filter(|placed| placed.float.is_some_and(|float| clear.clears(float.side)))
            .map(Placed::foot)
            .fold(f32::MIN, f32::max);
        let lead = self.lead(fragment);
        if self.cursor + lead < foot {
            self.cursor = foot - lead;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::layout::testing::{
        content_lines, heading, image, long_prose, painted, paragraph, quote, right_edge, section,
        styled_geometry, with_image,
    };
    use crate::pages::{Page, Side};
    use crate::style::Situation;

    /// The image the tests float is 144pt square.
    const IMAGE: f32 = 144.0;

    /// Words enough to set more lines than the image is tall.
    fn beside() -> crate::content::Block {
        paragraph(
            &"my father had a small estate in nottinghamshire and i was the third of five sons "
                .repeat(8),
        )
    }

    /// The one image a page paints, as `(x, y, width, height)`.
    fn the_image(page: &Page) -> (f32, f32, f32, f32) {
        let images = painted(page);
        assert_eq!(images.len(), 1, "the page paints {images:?}");
        images[0]
    }

    /// Every content line of a page as `(baseline, start, end)`.
    fn spans(page: &Page) -> Vec<(f32, f32, f32)> {
        content_lines(page)
            .into_iter()
            .map(|(baseline, runs)| (baseline, runs[0].0, right_edge(page, baseline)))
            .collect()
    }

    /// Every content run of a page as `(baseline, start, end)`, so the
    /// two columns on one baseline read apart.
    fn runs(page: &Page) -> Vec<(f32, f32, f32)> {
        use crate::layout::testing::{folio_size, registry};
        page.items
            .iter()
            .filter_map(|item| {
                let crate::pages::DrawItem::Text {
                    x,
                    y,
                    font_id,
                    size,
                    glyphs,
                    ..
                } = item
                else {
                    return None;
                };
                if *size == folio_size() {
                    return None;
                }
                let last = glyphs.last()?;
                let upem = registry().metrics(*font_id)?.units_per_em as f32;
                let advance = registry().advance_width(*font_id, last.id)? as f32;
                Some((*y, *x, last.x + advance / upem * size))
            })
            .collect()
    }

    /// Acceptance: prose sets beside an image floated left, and takes
    /// the whole measure back under it.
    #[test]
    fn prose_sets_beside_a_left_float_and_resumes_full_measure_below_it() {
        let css = "img { float: left; margin-right: 12pt } p { text-indent: 0 }";
        let page = &with_image(css, vec![section(vec![image(), beside()])]).pages[0];
        let geometry = styled_geometry(css, Situation::First(Side::Recto));
        let (left, top) = geometry.content_origin();
        let (x, y, w, h) = the_image(page);
        assert_eq!((x, y, w, h), (left, top, IMAGE, IMAGE));

        let lines = spans(page);
        let (near, far): (Vec<_>, Vec<_>) =
            lines.iter().partition(|(baseline, ..)| *baseline < y + h);
        assert!(near.len() > 2, "too few lines beside the image: {near:?}");
        assert!(far.len() > 2, "too few lines under the image: {far:?}");
        for (baseline, start, _) in &near {
            assert!(
                *start >= x + w + 12.0 - 1e-3,
                "a line at {baseline} starts at {start}, over the image"
            );
        }
        // Under the image a line starts at the edge of the column and
        // runs out to the far edge of the measure.
        let measure = geometry.measure();
        for (baseline, start, _) in far.iter().skip(1) {
            assert!(
                (*start - left).abs() < 1e-3,
                "a line at {baseline} starts at {start}, not at the edge"
            );
        }
        let widest = far.iter().map(|(_, _, end)| *end).fold(f32::MIN, f32::max);
        assert!(
            widest > left + measure - 12.0,
            "no line reached {widest} across"
        );
    }

    /// Part: prose sets to the left of an image floated right, which
    /// stands at the far edge of the column.
    #[test]
    fn prose_sets_beside_a_right_float_on_its_left() {
        let css = "img { float: right; margin-left: 12pt } p { text-indent: 0 }";
        let page = &with_image(css, vec![section(vec![image(), beside()])]).pages[0];
        let geometry = styled_geometry(css, Situation::First(Side::Recto));
        let (left, top) = geometry.content_origin();
        let (x, y, w, h) = the_image(page);
        assert_eq!(
            (x, y, w, h),
            (left + geometry.measure() - IMAGE, top, IMAGE, IMAGE)
        );
        let near: Vec<_> = spans(page)
            .into_iter()
            .filter(|(baseline, ..)| *baseline < y + h)
            .collect();
        assert!(near.len() > 2, "too few lines beside the image: {near:?}");
        for (baseline, start, end) in near {
            assert!(
                (start - left).abs() < 1e-3,
                "a line at {baseline} starts at {start}"
            );
            assert!(
                end <= x - 12.0 + 1e-3,
                "a line at {baseline} ends at {end}, over the image"
            );
        }
    }

    /// Acceptance: `clear` puts the cleared block below the float. A
    /// heading that does not clear it sets beside it.
    #[test]
    fn clear_puts_the_cleared_block_below_the_float() {
        let blocks = || vec![image(), paragraph("a short line"), heading("Chapter")];
        let heading_line = |css: &str| {
            let page = &with_image(css, vec![section(blocks())]).pages[0];
            let (_, y, _, h) = the_image(page);
            let lines = spans(page);
            let (baseline, start, _) = *lines.last().expect("the heading is set");
            (baseline, start, y + h)
        };
        let float = "img { float: left; margin-right: 12pt } p { text-indent: 0 }";
        let left = styled_geometry(float, Situation::First(Side::Recto))
            .content_origin()
            .0;

        let (baseline, start, foot) = heading_line(float);
        assert!(baseline < foot, "the heading did not set beside the float");
        assert!(
            start > left + IMAGE,
            "the heading starts at {start}, over the float"
        );

        for clear in ["both", "left"] {
            let (baseline, start, foot) = heading_line(&format!("{float} h1 {{ clear: {clear} }}"));
            assert!(
                baseline > foot,
                "clear: {clear} left the heading at {baseline}"
            );
            assert!(
                (start - left).abs() < 1e-3,
                "clear: {clear} starts at {start}"
            );
        }

        // A block that clears the other side still sets beside it.
        let (baseline, _, foot) = heading_line(&format!("{float} h1 {{ clear: right }}"));
        assert!(
            baseline < foot,
            "clear: right moved the heading below a left float"
        );
    }

    /// Acceptance: a float that does not fit what is left of the page
    /// moves whole to the next page, with the text that sets beside
    /// it. It is never split.
    #[test]
    fn a_float_near_the_foot_moves_whole_to_the_next_page() {
        // The quotation takes all but 10% of the page, which is less
        // than the image is tall.
        let css = "img { float: left; margin-right: 12pt } p { text-indent: 0 } \
                   blockquote { height: 90%; margin: 0 }";
        let blocks = vec![quote(vec![paragraph("lilliput")]), image(), beside()];
        let output = with_image(css, vec![section(blocks)]);
        let pages = &output.pages;
        assert!(pages.len() > 1, "the float never left the first page");
        assert!(
            painted(&pages[0]).is_empty(),
            "the float was set at the foot of the first page"
        );
        let (x, y, w, h) = the_image(&pages[1]);
        let geometry = styled_geometry(css, Situation::Body(pages[1].side));
        let (left, top) = geometry.content_origin();
        assert_eq!((x, y, w, h), (left, top, IMAGE, IMAGE));
        assert!(
            content_lines(&pages[0])
                .iter()
                .all(|(_, runs)| runs.iter().all(|run| run.2.contains("lilliput"))),
            "the text after the float stayed on the first page"
        );
        let first = spans(&pages[1])[0];
        assert!(
            first.1 >= x + w + 12.0 - 1e-3,
            "the text did not move with the float: {first:?}"
        );
    }

    /// Acceptance: the paragraph beside a float is broken by total fit
    /// against the bands the float leaves, not filled band by band.
    #[test]
    fn the_floated_paragraph_is_broken_by_total_fit() {
        crate::layout::testing::assert_broken_by_total_fit(
            "img { float: left; margin-right: 12pt } p { text-indent: 0 }",
            IMAGE + 12.0,
        );
    }

    /// Part: a float met while another stands at the same edge starts
    /// under it rather than over it, and the text still starts beside
    /// the first.
    #[test]
    fn a_second_float_at_the_same_edge_starts_under_the_first() {
        let css = "img { float: left; width: 30%; margin-right: 12pt } p { text-indent: 0 }";
        let page = &with_image(css, vec![section(vec![image(), image(), beside()])]).pages[0];
        let images = painted(page);
        assert_eq!(images.len(), 2, "the page paints {images:?}");
        let ((x, y, w, h), (next_x, next_y, ..)) = (images[0], images[1]);
        assert_eq!(next_x, x);
        assert!(
            (next_y - (y + h)).abs() < 1e-3,
            "the second float is at {next_y}"
        );
        let first = spans(page)[0];
        assert!(
            first.0 < y + h,
            "the text starts at {}, under the first float",
            first.0
        );
        assert!(
            first.1 >= x + w + 12.0 - 1e-3,
            "the text starts at {}",
            first.1
        );
    }

    /// The page box the column tests divide.
    const TWO_COLUMNS: &str = "@page { column-count: 2; column-gap: 18pt }";

    /// Acceptance: with columns on, a float stays inside the column it
    /// was placed in and nothing crosses the gutter. The other column
    /// sets as if there were no float.
    #[test]
    fn a_float_stays_inside_its_column() {
        let css = format!(
            "{TWO_COLUMNS} img {{ float: right; width: 50%; margin-left: 6pt }} \
             p {{ text-indent: 0 }}"
        );
        let blocks = std::iter::once(image()).chain(long_prose(14)).collect();
        let page = &with_image(&css, vec![section(blocks)]).pages[0];
        let geometry = styled_geometry(&css, Situation::First(Side::Recto));
        let measure = geometry.measure();
        let (first, second) = (geometry.column_origin(0).0, geometry.column_origin(1).0);
        let (x, y, w, h) = the_image(page);
        assert!((w - measure / 2.0).abs() < 1e-3, "the float is {w} wide");
        assert!(
            x >= first && x + w <= first + measure + 1e-3,
            "the float runs {x}..{} outside its column",
            x + w
        );

        let mut beside = 0;
        let mut over = 0;
        for (baseline, start, end) in runs(page) {
            if start >= second - 1e-3 {
                over += 1;
                assert!(
                    end <= second + measure + 1e-3,
                    "a line at {baseline} of the second column runs {start}..{end}"
                );
                continue;
            }
            assert!(
                end <= first + measure + 1e-3,
                "a line at {baseline} runs to {end}, into the gutter"
            );
            if baseline < y + h {
                beside += 1;
                assert!(
                    end <= x - 6.0 + 1e-3,
                    "a line at {baseline} ends over the float"
                );
            }
        }
        assert!(beside > 2, "too few lines beside the float");
        assert!(over > 2, "the prose never reached the second column");
    }

    /// Part: a float that does not fit the foot of a column moves to
    /// the head of the next column rather than the next page.
    #[test]
    fn a_float_near_the_foot_of_a_column_moves_to_the_next_column() {
        let css = format!(
            "{TWO_COLUMNS} img {{ float: left; width: 50% }} p {{ text-indent: 0 }} \
             blockquote {{ height: 90%; margin: 0 }}"
        );
        let blocks = vec![quote(vec![paragraph("lilliput")]), image(), beside()];
        let page = &with_image(&css, vec![section(blocks)]).pages[0];
        let geometry = styled_geometry(&css, Situation::First(Side::Recto));
        let (x, y, ..) = the_image(page);
        assert_eq!((x, y), geometry.column_origin(1));
    }

    /// Acceptance: a book with floats lays out the same way twice,
    /// over the same number of pages.
    #[test]
    fn a_book_with_floats_lays_out_the_same_way_twice() {
        let css = "img { float: left; width: 40%; margin: 0 12pt 6pt 0 } \
                   img:nth-of-type(even) { float: right; margin: 0 0 6pt 12pt } \
                   h1 { clear: both }";
        let book = || {
            (0..4)
                .map(|chapter| {
                    let mut blocks = vec![heading(&format!("Chapter {chapter}"))];
                    for prose in long_prose(12) {
                        blocks.push(image());
                        blocks.push(prose);
                    }
                    section(blocks)
                })
                .collect::<Vec<_>>()
        };
        let once = with_image(css, book());
        let twice = with_image(css, book());
        assert!(once.pages.len() > 4, "a book worth breaking");
        assert_eq!(once.pages.len(), twice.pages.len());
        assert_eq!(
            serde_json::to_string(&once.pages).expect("the pages encode"),
            serde_json::to_string(&twice.pages).expect("the pages encode"),
        );
    }
}
