//! What an inline element paints on one line: the tint behind its
//! runs, and the border around them.
//!
//! A block has one border box per page and an inline element has one
//! per line. Line layout works the extent out, because it is the
//! stage that knows where the runs of one element fell; this turns
//! each of them into paint ops, on the page the line landed on.

use crate::lines::Line;
use crate::pages::{DrawItem, Radius};
use crate::style::Edges;

use super::Paginator;
use super::flow::box_items;

impl Paginator<'_> {
    /// The boxes the inline elements on one line paint there, in
    /// `layer`. `x` is the line's own leading edge and `baseline` its
    /// baseline.
    ///
    /// They come out outermost first, and before the line's glyphs:
    /// a chip is behind the word it tints.
    pub(super) fn inline_items(
        &self,
        line: &Line,
        x: f32,
        baseline: f32,
        layer: i32,
    ) -> Vec<DrawItem> {
        let mut items = Vec::new();
        for fragment in &line.boxes {
            let (w, h) = (fragment.width, fragment.above + fragment.below);
            if w <= 0.0 || h <= 0.0 {
                continue;
            }
            let style = self.styles.style(fragment.node);
            let backdrop = self.backdrop(&style.background);
            let ink = |edge: crate::style::Border| edge.color.unwrap_or(style.color);
            let colors = Edges {
                top: ink(style.border.top),
                right: ink(style.border.right),
                bottom: ink(style.border.bottom),
                left: ink(style.border.left),
            };
            // The box goes on past an edge the line break left open,
            // so that edge is not drawn and its corners are square.
            let mut border = style.border.widths();
            let mut radii = style.border_radius.resolve(w, h);
            if !fragment.opens {
                border.left = 0.0;
                (radii.top_left, radii.bottom_left) = (Radius::SQUARE, Radius::SQUARE);
            }
            if !fragment.closes {
                border.right = 0.0;
                (radii.top_right, radii.bottom_right) = (Radius::SQUARE, Radius::SQUARE);
            }
            items.extend(box_items(
                x + fragment.x,
                baseline - fragment.above,
                w,
                h,
                radii,
                border,
                colors,
                &backdrop,
                layer,
            ));
        }
        items
    }
}
