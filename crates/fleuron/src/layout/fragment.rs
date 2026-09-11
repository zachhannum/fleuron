//! What the flow can move: one line, one image, one ornament,
//! and what the blocks around it paint.

use std::sync::Arc;

use crate::content::NodeId;
use crate::lines::Line;
use crate::pages::DrawItem;
use crate::style::{BoxDecorationBreak, Break, Color, ComputedStyle, Edges};

use super::build::Reflow;

/// Whether a page may end above a fragment.
///
/// Everything the cascade says about fragmentation — `break-before`,
/// `break-after`, `break-inside`, `orphans`, `widows` — reaches the
/// flow as one of these, decided while the fragments are built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakPoint {
    /// A page break must fall here, on the side the break names.
    Forced(Break),
    /// A break may fall here.
    Allowed,
    /// A break may not fall here: this fragment moves with the one
    /// above it.
    Forbidden,
}

/// What one fragment paints.
#[derive(Debug, Clone)]
pub enum Piece {
    /// A laid-out line, and the initial letter sunk beside it.
    Line {
        /// The line itself, shaped and measured.
        line: Line,
        /// The drop cap set beside this line, on the first line of a
        /// paragraph that has one.
        cap: Option<DropCap>,
    },
    /// A placed image, sized against the content box.
    Image {
        /// Width in points, after any scaling.
        width: f32,
        /// Height in points, after any scaling.
        height: f32,
        /// Index into the asset table.
        asset: u32,
    },
    /// Space with nothing in it: what a thematic break set in space
    /// rather than in an ornament comes to.
    Blank,
    /// Where an image the sheet lifted out of the flow was written.
    /// It takes no space and paints nothing. The page the flow
    /// reaches here is the page that places the image.
    Anchor(NodeId),
    /// One row of a table, set whole.
    Row(Box<TableRow>),
}

/// One row of a table, set whole: what it paints, and what the flow
/// needs to set the table's header rows again on a new page.
#[derive(Debug, Clone)]
pub struct TableRow {
    /// What the row paints, from the top of the row and the leading
    /// edge of the content box.
    pub items: Vec<DrawItem>,
    /// Whether this is the first row of its table.
    pub opens: bool,
    /// Whether this is a header row, which is set again at the top of
    /// every page or column the table continues onto.
    pub head: bool,
    /// Whether the header rows are set above this row when it is the
    /// first thing on a page or in a column.
    pub repeats: bool,
}

/// An initial letter sunk beside the lines that follow it.
#[derive(Debug, Clone)]
pub struct DropCap {
    /// The letter, shaped at the size the sink works out to.
    pub line: Line,
    /// Leading edge, from the content box's own.
    pub x: f32,
    /// How far the cap's baseline sits below its line's.
    pub drop: f32,
}

/// One thing the flow can place: a line, an image, an ornament.
///
/// Everything horizontal is settled here — indentation, alignment,
/// the measure a drop cap left — so the flow only stacks.
#[derive(Debug, Clone)]
pub struct Fragment {
    /// Leading edge, from the content box's own.
    pub x: f32,
    /// Space above, from the margins around it. A page that opens on
    /// this fragment drops it.
    pub lead: f32,
    /// Space above that a page break does not drop and no margin
    /// collapses through: the borders and padding between this
    /// fragment and whatever is above it.
    pub fixed: f32,
    /// The fragment's own height.
    pub height: f32,
    /// Whether a page may end above it.
    pub break_before: BreakPoint,
    /// What it paints.
    pub piece: Piece,
    /// What it tells the page furniture when it lands. Boxed because
    /// a book has thousands of fragments and a handful of chapter
    /// headings.
    pub marks: Option<Box<Marks>>,
    /// The decorated blocks this fragment opens and closes. Boxed
    /// for the same reason: most fragments decorate nothing.
    pub decorations: Option<Box<Decorations>>,
    /// The paragraph this line came out of, shared by every line of
    /// it, and `None` on everything else. The flow reads it where an
    /// image narrows the bands the paragraph is set in. A book that
    /// anchors nothing keeps none of this.
    pub reflow: Option<Arc<Reflow>>,
}

impl Fragment {
    /// A fragment with nothing above it: what a block emits before
    /// the margins and breaks around it are folded in.
    pub(super) fn plain(x: f32, height: f32, piece: Piece) -> Fragment {
        Fragment {
            x,
            lead: 0.0,
            fixed: 0.0,
            height,
            break_before: BreakPoint::Allowed,
            piece,
            marks: None,
            decorations: None,
            reflow: None,
        }
    }
}

/// What one fragment does to the blocks decorated around it.
///
/// A decoration spans a range of fragments. The range is settled
/// while the flow is built and the geometry is not: the paginator
/// places fragments one at a time and moves what it has already
/// painted when one carries to the next page. So the range travels on
/// the fragments at its ends, and the paginator resolves it, per
/// page, into a border box over the fragments that landed there.
#[derive(Debug, Clone, Default)]
pub struct Decorations {
    /// The blocks whose first fragment this is, outermost first.
    pub opens: Vec<Decoration>,
    /// How many of the open blocks end with this fragment,
    /// innermost first.
    pub closes: u32,
}

/// One decorated block: what it paints, and where it sits around the
/// fragments it spans.
#[derive(Debug, Clone)]
pub struct Decoration {
    /// Leading edge of the border box, from the content box's own.
    pub x: f32,
    /// Width of the border box.
    pub width: f32,
    /// Distance from the top of the block's first fragment up to the
    /// top of its border box.
    pub above: f32,
    /// Distance from the bottom of its last fragment down to the
    /// bottom of its border box.
    pub below: f32,
    /// Border widths, zero on an edge that is not drawn.
    pub border: Edges,
    /// What each edge is painted in, `currentColor` resolved.
    pub colors: Edges<Color>,
    /// What is painted behind the whole border box.
    pub background: Option<Color>,
    /// Whether `box-decoration-break: clone` closes the two edges a
    /// page break cuts.
    pub cloned: bool,
}

/// What a fragment tells the page it lands on: the running strings
/// its element set, and the folio its page restarts at.
///
/// Both are captured from the content flow, so both are answers only
/// pagination has: which page a heading fell on is not known until it
/// falls there.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Marks {
    /// Named strings, resolved from the element's own text, in the
    /// order the cascade gave them.
    pub strings: Vec<(String, String)>,
    /// The folio of the page this fragment lands on.
    pub page_number: Option<u32>,
}

/// Whether a block paints anything behind or around its content.
pub(super) fn decorated(style: &ComputedStyle) -> bool {
    style.background_color.is_some() || style.border.paints()
}

/// The decoration one block paints, or `None` where it paints
/// nothing. `x` and `measure` are what the block was laid out
/// against; the border box takes its margins off them.
pub(super) fn decoration(style: &ComputedStyle, x: f32, measure: f32) -> Option<Decoration> {
    if !decorated(style) {
        return None;
    }
    let (left, width) = style.border_box(x, measure);
    let border = style.border.widths();
    let ink = |edge: crate::style::Border| edge.color.unwrap_or(style.color);
    Some(Decoration {
        x: left,
        width,
        above: 0.0,
        below: 0.0,
        border,
        colors: Edges {
            top: ink(style.border.top),
            right: ink(style.border.right),
            bottom: ink(style.border.bottom),
            left: ink(style.border.left),
        },
        background: style.background_color,
        cloned: style.box_decoration_break == BoxDecorationBreak::Clone,
    })
}
