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
