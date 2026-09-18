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
            let offset = line
                .spans
                .get(fragment.span)
                .map(|span| span.offset)
                .unwrap_or_default();
            items.extend(box_items(
                x + offset + fragment.x,
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

#[cfg(test)]
mod tests {
    use crate::layout::testing::{paginate_styled, quote, section, tagged_paragraph};
    use crate::pages::DrawItem;
    use crate::style::Color;

    /// A quotation with a tint of its own, holding a paragraph with a
    /// tinted tag in it.
    const NESTED_CSS: &str = "blockquote { background-color: #f4f1ea; padding: 6pt } \
                              code { background-color: #858585; padding: 2pt 4pt }";

    /// Part: a box an inline element paints goes over the background
    /// of the block around it and under the glyphs it sits behind.
    #[test]
    fn an_inline_box_paints_between_the_block_and_its_glyphs() {
        let pages = paginate_styled(
            NESTED_CSS,
            vec![section(vec![quote(vec![tagged_paragraph(
                "roll for ",
                "2d6",
                " and add the modifier",
            )])])],
        );
        let items = &pages[0].items;
        let rect = |ink: Color| {
            items
                .iter()
                .position(|item| matches!(item, DrawItem::Rect { color, .. } if *color == ink))
                .unwrap_or_else(|| panic!("nothing was painted in {}", ink.to_hex()))
        };
        let block = rect(Color::rgb(0xf4, 0xf1, 0xea));
        let chip = rect(Color::rgb(0x85, 0x85, 0x85));
        let tag = items
            .iter()
            .position(|item| matches!(item, DrawItem::Text { text, .. } if text == "2d6"))
            .expect("the tag was set");
        assert!(
            block < chip && chip < tag,
            "the order was block {block}, chip {chip}, tag {tag}",
        );
    }
}
