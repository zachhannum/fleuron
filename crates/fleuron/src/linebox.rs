//! The line box model: how tall a line is and where its baseline sits.
//!
//! Half-leading over font metrics: a face's box is its ascent and
//! descent, each widened by half the leading, where leading is what
//! `line_height` adds over the face's natural height (ascent +
//! descent + line gap). The line gap splits across the two halves so
//! a single-face line always measures exactly `line_height × size`.
//!
//! The strut is the paragraph's own box — the line's minimum
//! geometry, independent of the runs on it. Runs larger than
//! the strut grow the line around the shared baseline; runs smaller
//! never shrink it below the strut.
//!
//! Units: points throughout — runs of different sizes share one
//! baseline, and font units don't commute across sizes.

use crate::fonts::FontMetricsTable;

/// Vertical extents around a baseline, in points: a face's box when
/// computed for one font and size, the paragraph's strut when
/// computed for the paragraph's own style.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Strut {
    /// Distance from baseline to the top of the box.
    pub above: f32,
    /// Distance from baseline to the bottom of the box.
    pub below: f32,
}

impl Strut {
    /// The box a face makes at `size` under a unitless `line_height`.
    pub fn from_metrics(metrics: FontMetricsTable, size: f32, line_height: f32) -> Strut {
        let upem = metrics.units_per_em as f32;
        let ascent = metrics.ascender as f32 / upem * size;
        let descent = -metrics.descender as f32 / upem * size;
        let gap = metrics.line_gap as f32 / upem * size;
        let half_leading = (line_height * size - (ascent + descent + gap)) / 2.0;
        Strut {
            above: ascent + half_leading + gap / 2.0,
            below: descent + half_leading + gap / 2.0,
        }
    }

    /// Total height: `above + below`.
    pub fn height(self) -> f32 {
        self.above + self.below
    }
}

/// One line's vertical geometry, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineBox {
    /// Total height of the line.
    pub height: f32,
    /// Baseline offset from the top of the line. One baseline serves
    /// the whole line; every run paints here, whatever its size.
    pub baseline: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts::{FontRegistry, bundled_registry};
    use crate::lines::{LineLayout, ParagraphStyle, ShapedRun};

    fn layout() -> LineLayout<'static> {
        static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
        let registry = REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"));
        LineLayout::new(registry)
    }

    /// The body style the built-in sheet computes: 11pt over the
    /// bundled serif, at a line height of 1.4.
    fn body() -> ParagraphStyle {
        static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
        let registry = REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"));
        crate::style::defaults(&crate::content::Book::default(), registry)
            .root()
            .paragraph()
    }

    fn run(size: f32) -> ShapedRun {
        ShapedRun {
            font_id: 0,
            size,
            text: String::new(),
            text_start: 0,
            glyphs: Vec::new(),
            advance: 0,
        }
    }

    fn assert_close(got: f32, want: f32, what: &str) {
        assert!((got - want).abs() < 1e-3, "{what}: got {got}, want {want}");
    }

    /// EB Garamond: upem 1000, ascent 1007, descent −298, gap 0.
    /// Body (11pt, line-height 1.4): natural 14.355pt, leading 1.045,
    /// half 0.5225 — ascent 11.077 + 0.5225 above, 3.278 + 0.5225
    /// below.
    #[test]
    fn strut_splits_the_leading_in_half() {
        let strut = layout().strut(body());
        assert_close(strut.above, 11.5995, "above");
        assert_close(strut.below, 3.8005, "below");
        assert_close(strut.height(), 15.4, "height");
    }

    /// Above + below totals `line_height × size` whatever the knob —
    /// including below the natural height, where the distribution is
    /// symmetric negative leading.
    #[test]
    fn box_totals_line_height_at_any_knob() {
        let metrics = bundled_registry().unwrap().metrics(0).unwrap();
        for size in [6.0, 11.0, 24.0] {
            for line_height in [0.9, 1.305, 1.4, 2.0] {
                let strut = Strut::from_metrics(metrics, size, line_height);
                assert_close(
                    strut.height(),
                    line_height * size,
                    &format!("{size}pt at {line_height}"),
                );
            }
        }
    }

    /// A line of runs smaller than the strut stays strut-tall — the
    /// minimum height is independent of content.
    #[test]
    fn strut_is_the_minimum_independent_of_content() {
        let strut = layout().strut(body());
        let line_box = layout().line_box(&[run(6.0)], body());
        assert_close(line_box.baseline, strut.above, "baseline");
        assert_close(line_box.height, strut.height(), "height");
    }

    /// A line mixing 12pt and 24pt: both sit on one baseline, set by
    /// the larger run. 24pt at 1.4: ascent 25.308 above, 8.292 below,
    /// height 33.6 = 24 × 1.4; the 12pt run changes nothing.
    #[test]
    fn mixed_sizes_share_one_baseline() {
        let line_box = layout().line_box(&[run(12.0), run(24.0)], body());
        assert_close(line_box.baseline, 25.308, "baseline");
        assert_close(line_box.height, 33.6, "height");
        let alone = layout().line_box(&[run(24.0)], body());
        assert_eq!(alone, line_box, "the 12pt run moved the line");
    }

    /// No runs at all: the strut alone defines the line.
    #[test]
    fn empty_line_is_the_strut() {
        let strut = layout().strut(body());
        let line_box = layout().line_box(&[], body());
        assert_close(line_box.baseline, strut.above, "baseline");
        assert_close(line_box.height, strut.height(), "height");
    }
}
