//! The boxes the inline elements on a line paint there.
//!
//! An inline element has no extent of its own: it has one per line
//! its runs reach, and the geometry is known only once the line is
//! broken and its runs are cut. This works that out from the runs of
//! one span of one line, and charges each run with the edges that
//! open and close on it, so a painter placing glyphs walks past the
//! padding rather than under it.

use std::ops::Range;

use crate::linebox::Strut;

use super::LineLayout;
use super::flatten::{BoxSpan, FlatParagraph};
use super::line::{InlineFragment, ShapedRun};

/// One box of `flat` over the runs of one span: the box, and the
/// first and last of those runs it covers.
struct Covering<'a> {
    span: &'a BoxSpan,
    first: usize,
    last: usize,
}

/// A box whose leading edge has been placed and whose trailing edge
/// has not: the fragment it is filling, and the run it closes on.
struct Open<'a> {
    at: usize,
    span: &'a BoxSpan,
    last: usize,
}

impl LineLayout<'_> {
    /// The boxes the inline elements covering `runs` paint there,
    /// outermost first.
    ///
    /// `runs` are the runs of one span of one line, `offset` is where
    /// that span sits from the line's own leading edge, and `text` is
    /// the bytes of the paragraph the span holds. Each run comes out
    /// charged with the edges that fall on it.
    pub(super) fn inline_fragments(
        &self,
        flat: &FlatParagraph,
        runs: &mut [ShapedRun],
        offset: f32,
        text: Range<usize>,
    ) -> Vec<InlineFragment> {
        let covering: Vec<Covering<'_>> = flat
            .boxes
            .iter()
            .filter(|span| !span.range.is_empty())
            .filter_map(|span| {
                let (first, last) = covered(runs, &span.range)?;
                Some(Covering { span, first, last })
            })
            .collect();
        if covering.is_empty() {
            return Vec::new();
        }
        let mut fragments: Vec<InlineFragment> = Vec::new();
        let mut open: Vec<Open<'_>> = Vec::new();
        let mut x = offset;
        for index in 0..runs.len() {
            for covering in covering.iter().filter(|covering| covering.first == index) {
                let box_ = &covering.span.box_;
                let opens = box_.cloned || covering.span.range.start >= text.start;
                let closes = box_.cloned || covering.span.range.end <= text.end;
                let leading = if opens { box_.leading() } else { 0.0 };
                runs[index].lead += leading;
                let (above, below) = self.content_area(&runs[index..=covering.last]);
                fragments.push(InlineFragment {
                    node: covering.span.node,
                    x,
                    width: 0.0,
                    above: above + box_.above(),
                    below: below + box_.below(),
                    opens,
                    closes,
                });
                x += leading;
                open.push(Open {
                    at: fragments.len() - 1,
                    span: covering.span,
                    last: covering.last,
                });
            }
            x += self.run_width(&runs[index]);
            // The innermost box closes first, so its trailing edge
            // falls inside the one around it.
            while open.last().is_some_and(|open| open.last == index) {
                let open = open.pop().expect("a box was open");
                let trailing = match fragments[open.at].closes {
                    true => open.span.box_.trailing(),
                    false => 0.0,
                };
                runs[index].trail += trailing;
                x += trailing;
                fragments[open.at].width = x - fragments[open.at].x;
            }
        }
        fragments
    }

    /// The content area the runs of one inline element make: the
    /// tallest ascent and the deepest descent of the faces they are
    /// set in, in points from the baseline. A face's leading is no
    /// part of it, so the box holds the letters and nothing more.
    fn content_area(&self, runs: &[ShapedRun]) -> (f32, f32) {
        let (mut above, mut below) = (0.0f32, 0.0f32);
        for run in runs {
            let Some(metrics) = self.registry.metrics(run.font_id) else {
                continue;
            };
            let area = Strut::content(metrics, run.size);
            above = above.max(area.above);
            below = below.max(area.below);
        }
        (above, below)
    }

    /// One run's glyphs in points.
    fn run_width(&self, run: &ShapedRun) -> f32 {
        let upem = self
            .registry
            .metrics(run.font_id)
            .map(|metrics| metrics.units_per_em as f32)
            .unwrap_or(1000.0);
        run.advance as f32 / upem * run.size
    }
}

/// The first and last of `runs` one box covers, and `None` where it
/// covers none of them.
fn covered(runs: &[ShapedRun], range: &Range<usize>) -> Option<(usize, usize)> {
    let mut first = None;
    let mut last = 0;
    for (index, run) in runs.iter().enumerate() {
        let start = run.text_start as usize;
        if start < range.end && start + run.text.len() > range.start {
            first.get_or_insert(index);
            last = index;
        }
    }
    first.map(|first| (first, last))
}

#[cfg(test)]
mod tests {
    use crate::content::NodeId;
    use crate::lines::testing::{Boxed, bordered, code, emphasis, layout_boxed, one_run, padded};

    /// The tag of the fixtures below.
    fn tag() -> NodeId {
        NodeId::new(7)
    }

    /// A paragraph of prose with a tagged run in the middle of it.
    fn tagged() -> Vec<crate::content::Inline> {
        let mut inlines = one_run("roll for ");
        inlines.push(code(tag(), "2d6"));
        inlines.extend(one_run(" and add the modifier"));
        inlines
    }

    /// Part: horizontal padding and border widths are charged to the
    /// breaker, so a paragraph that fits on one line without them
    /// needs a wider measure with them.
    #[test]
    fn the_edges_of_a_box_are_width_the_breaker_measures() {
        assert_eq!(
            layout_boxed(&tagged(), 200.0, &Boxed(Vec::new())).len(),
            1,
            "the fixture did not fit on one line",
        );
        for box_ in [padded(4.0), bordered(4.0)] {
            let styles = Boxed(vec![(tag(), box_)]);
            let narrowest = |styles: &Boxed| -> f32 {
                (400..1000)
                    .map(|steps| steps as f32 * 0.25)
                    .find(|measure| layout_boxed(&tagged(), *measure, styles).len() == 1)
                    .expect("the paragraph sets in one line at some measure")
            };
            let grew = narrowest(&styles) - narrowest(&Boxed(Vec::new()));
            assert!(
                (grew - 8.0).abs() <= 0.25,
                "the measure grew by {grew}pt, not by the two edges of the box",
            );
        }
    }

    /// Part: a line yields one border box per inline element it
    /// covers, outermost first, and a line with no box on it yields
    /// none.
    #[test]
    fn a_line_yields_one_box_per_inline_element() {
        let outer = NodeId::new(3);
        let inlines = vec![emphasis(outer, {
            let mut children = one_run("roll ");
            children.push(code(tag(), "2d6"));
            children
        })];
        let styles = Boxed(vec![(outer, padded(2.0)), (tag(), padded(4.0))]);
        let lines = layout_boxed(&inlines, 200.0, &styles);
        assert_eq!(lines.len(), 1);
        let boxes = &lines[0].boxes;
        assert_eq!(
            boxes.iter().map(|box_| box_.node).collect::<Vec<_>>(),
            [outer, tag()],
            "the boxes are not outermost first",
        );
        assert!(
            boxes[0].x < boxes[1].x && boxes[0].width > boxes[1].width,
            "the nested box is not inside the one around it: {boxes:?}",
        );
        assert!(boxes.iter().all(|box_| box_.opens && box_.closes));

        assert!(
            layout_boxed(&tagged(), 200.0, &Boxed(Vec::new()))[0]
                .boxes
                .is_empty(),
            "a line with no box on it carries one",
        );
    }

    /// Part: padding above and below a run is no part of the line's
    /// own height, and a box deep enough reaches past the line.
    #[test]
    fn a_box_does_not_grow_the_line_it_is_on() {
        let plain = layout_boxed(&tagged(), 200.0, &Boxed(Vec::new()));
        let styles = Boxed(vec![(tag(), padded(8.0))]);
        let padded = layout_boxed(&tagged(), 200.0, &styles);
        assert_eq!(padded[0].box_, plain[0].box_, "the box grew the line");

        let box_ = padded[0].boxes[0];
        assert!(
            box_.above > padded[0].box_.baseline,
            "a box 8pt deep does not reach past the line above it: {box_:?}",
        );
    }

    /// A cloned box paints an edge at the break, and that edge is
    /// width. The line under the break opens with a leading edge the
    /// sliced box does not have, so the cloned tag asks for that much
    /// more measure to stay in two lines.
    #[test]
    fn the_edges_a_cloned_box_adds_at_a_break_are_width_too() {
        let inlines = vec![code(tag(), "2d6 plus the modifier you wrote down")];
        let box_ = padded(4.0);
        let cloned = crate::lines::InlineBox {
            cloned: true,
            ..box_
        };
        let lines =
            |box_, measure| layout_boxed(&inlines, measure, &Boxed(vec![(tag(), box_)])).len();
        let narrowest = |box_| -> f32 {
            (200..1000)
                .map(|steps| steps as f32 * 0.25)
                .find(|measure| lines(box_, *measure) == 2)
                .expect("the tag sets in two lines at some measure")
        };
        let grew = narrowest(cloned) - narrowest(box_);
        assert!(
            (grew - 4.0).abs() <= 0.25,
            "the cloned tag asked for {grew}pt more, not for the edge at the break",
        );
    }

    /// A box that a line break cuts opens on the first of its lines
    /// and closes on the last, and `clone` closes both edges of both.
    #[test]
    fn a_split_box_opens_and_closes_where_the_break_left_it() {
        let inlines = vec![code(tag(), "2d6 plus the modifier you wrote down")];
        let sliced = layout_boxed(&inlines, 100.0, &Boxed(vec![(tag(), padded(4.0))]));
        assert!(sliced.len() > 1, "the tag did not break");
        let edges: Vec<(bool, bool)> = sliced
            .iter()
            .flat_map(|line| &line.boxes)
            .map(|box_| (box_.opens, box_.closes))
            .collect();
        assert_eq!(edges.first().copied(), Some((true, false)));
        assert_eq!(edges.last().copied(), Some((false, true)));

        let cloned = crate::lines::InlineBox {
            cloned: true,
            ..padded(4.0)
        };
        let cloned = layout_boxed(&inlines, 100.0, &Boxed(vec![(tag(), cloned)]));
        assert!(
            cloned
                .iter()
                .flat_map(|line| &line.boxes)
                .all(|box_| box_.opens && box_.closes),
            "a cloned box left an edge open",
        );
    }
}
