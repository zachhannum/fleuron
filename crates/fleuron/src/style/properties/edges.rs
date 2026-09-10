//! The four edges of a box, and the border on each of them.

use serde::Serialize;

use super::value::Color;

/// The four edges of a box, in whatever the property resolves to:
/// points for `margin` and `padding`, a [`Border`] for `border`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Edges<T = f32> {
    /// Top edge.
    pub top: T,
    /// Right edge.
    pub right: T,
    /// Bottom edge.
    pub bottom: T,
    /// Left edge.
    pub left: T,
}

impl<T: Copy> Edges<T> {
    /// All four edges the same.
    pub const fn all(value: T) -> Edges<T> {
        Edges {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    /// The edge `which`.
    pub fn edge(&mut self, which: Edge) -> &mut T {
        match which {
            Edge::Top => &mut self.top,
            Edge::Right => &mut self.right,
            Edge::Bottom => &mut self.bottom,
            Edge::Left => &mut self.left,
        }
    }
}

impl Edges<f32> {
    /// What the left and right edges take off a measure.
    pub fn inline(self) -> f32 {
        self.left + self.right
    }
}

/// How a border edge is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BorderStyle {
    /// `none`: the edge is not drawn, whatever width it was given.
    None,
    /// `solid`
    Solid,
}

/// One border edge: its style, its width and its colour.
///
/// The colour is what `border-color` set, and `None` is
/// `currentColor`: the element's own `color`, whichever order the two
/// were declared in.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Border {
    /// How the edge is drawn.
    pub style: BorderStyle,
    /// Thickness in points, whether or not the edge is drawn.
    pub width: f32,
    /// What it is painted in, or `None` for the element's `color`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
}

impl Border {
    /// The initial border: `medium` wide, and not drawn.
    pub const NONE: Border = Border {
        style: BorderStyle::None,
        width: MEDIUM,
        color: None,
    };

    /// The thickness this edge takes in the flow: nothing unless it
    /// is drawn.
    pub fn used(self) -> f32 {
        match self.style {
            BorderStyle::None => 0.0,
            BorderStyle::Solid => self.width.max(0.0),
        }
    }
}

/// `border-width: medium`, the initial value, in points.
pub(crate) const MEDIUM: f32 = 2.25;

impl Edges<Border> {
    /// The four used thicknesses.
    pub fn widths(self) -> Edges {
        Edges {
            top: self.top.used(),
            right: self.right.used(),
            bottom: self.bottom.used(),
            left: self.left.used(),
        }
    }

    /// Whether any edge is drawn.
    pub fn paints(self) -> bool {
        let widths = self.widths();
        widths.top + widths.right + widths.bottom + widths.left > 0.0
    }
}

/// `thin`, `medium` and `thick`, in points: the CSS pixel widths a
/// browser gives them.
pub(crate) const LINE_WIDTHS: [(&str, f32); 3] =
    [("thin", 0.75), ("medium", MEDIUM), ("thick", 3.75)];

/// Which edge a one-sided box property sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    /// `-top`
    Top,
    /// `-right`
    Right,
    /// `-bottom`
    Bottom,
    /// `-left`
    Left,
}
