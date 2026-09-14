//! The four edges of a box, and the border on each of them.

use serde::{Deserialize, Serialize};

use crate::pages::{Corners, Radius};

use super::exclusion::Coord;
use super::value::Color;

/// The four edges of a box, in whatever the property resolves to:
/// points for `margin` and `padding`, a [`Border`] for `border`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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

    /// What the edge `which` holds.
    pub fn get(&self, which: Edge) -> T {
        match which {
            Edge::Top => self.top,
            Edge::Right => self.right,
            Edge::Bottom => self.bottom,
            Edge::Left => self.left,
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

/// How far one corner of a box is rounded, as the cascade computed it.
///
/// `x` runs along the top or bottom edge and `y` along the left or
/// right edge. A percentage is a percentage of the width of the box
/// for `x` and of its height for `y`, so it stays one until layout
/// knows the box.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CornerRadius {
    /// Along the top or bottom edge.
    pub x: Coord,
    /// Along the left or right edge.
    pub y: Coord,
}

impl CornerRadius {
    /// A corner that is not rounded.
    pub const SQUARE: CornerRadius = CornerRadius {
        x: Coord::Points(0.0),
        y: Coord::Points(0.0),
    };
}

/// The four corners of a box, each rounded by what `border-radius` set.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct BorderRadius {
    /// The top left corner.
    pub top_left: CornerRadius,
    /// The top right corner.
    pub top_right: CornerRadius,
    /// The bottom right corner.
    pub bottom_right: CornerRadius,
    /// The bottom left corner.
    pub bottom_left: CornerRadius,
}

impl BorderRadius {
    /// Four square corners: the initial value.
    pub const SQUARE: BorderRadius = BorderRadius {
        top_left: CornerRadius::SQUARE,
        top_right: CornerRadius::SQUARE,
        bottom_right: CornerRadius::SQUARE,
        bottom_left: CornerRadius::SQUARE,
    };

    /// What the corner `which` holds.
    pub fn get(&self, which: Corner) -> CornerRadius {
        match which {
            Corner::TopLeft => self.top_left,
            Corner::TopRight => self.top_right,
            Corner::BottomRight => self.bottom_right,
            Corner::BottomLeft => self.bottom_left,
        }
    }

    /// The corner `which`.
    pub fn corner(&mut self, which: Corner) -> &mut CornerRadius {
        match which {
            Corner::TopLeft => &mut self.top_left,
            Corner::TopRight => &mut self.top_right,
            Corner::BottomRight => &mut self.bottom_right,
            Corner::BottomLeft => &mut self.bottom_left,
        }
    }

    /// The corners in points, over a box `width` by `height`.
    ///
    /// A corner with a radius of zero on either axis is square. Where
    /// two corners on one edge would add up to more than the edge is
    /// long, all four scale down together until they fit.
    pub fn resolve(&self, width: f32, height: f32) -> Corners {
        let one = |corner: CornerRadius| {
            let radius = Radius {
                x: corner.x.to_points(width).max(0.0),
                y: corner.y.to_points(height).max(0.0),
            };
            if radius.is_square() {
                Radius::SQUARE
            } else {
                radius
            }
        };
        let [top_left, top_right, bottom_right, bottom_left] = [
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        ]
        .map(one);
        let fit = |edge: f32, sum: f32| {
            if sum > edge {
                (edge / sum).max(0.0)
            } else {
                1.0
            }
        };
        let scale = fit(width, top_left.x + top_right.x)
            .min(fit(width, bottom_left.x + bottom_right.x))
            .min(fit(height, top_left.y + bottom_left.y))
            .min(fit(height, top_right.y + bottom_right.y));
        let scaled = |radius: Radius| {
            let radius = Radius {
                x: radius.x * scale,
                y: radius.y * scale,
            };
            if radius.is_square() {
                Radius::SQUARE
            } else {
                radius
            }
        };
        Corners {
            top_left: scaled(top_left),
            top_right: scaled(top_right),
            bottom_right: scaled(bottom_right),
            bottom_left: scaled(bottom_left),
        }
    }
}

/// Which corner a one-cornered property sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    /// `-top-left-`
    TopLeft,
    /// `-top-right-`
    TopRight,
    /// `-bottom-right-`
    BottomRight,
    /// `-bottom-left-`
    BottomLeft,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Part: a percentage measures the width across and the height
    /// down, and two corners that would overlap scale down together.
    #[test]
    fn radii_resolve_against_the_box_and_scale_down_to_fit() {
        let all = |x: Coord, y: Coord| BorderRadius {
            top_left: CornerRadius { x, y },
            top_right: CornerRadius { x, y },
            bottom_right: CornerRadius { x, y },
            bottom_left: CornerRadius { x, y },
        };
        let halves = all(Coord::Percent(50.0), Coord::Percent(50.0)).resolve(80.0, 20.0);
        assert_eq!(halves.top_left, Radius { x: 40.0, y: 10.0 });
        assert_eq!(halves.bottom_right, Radius { x: 40.0, y: 10.0 });

        // 30 and 30 along an edge 40 long scale by two thirds.
        let crowded = all(Coord::Points(30.0), Coord::Points(30.0)).resolve(40.0, 100.0);
        let scale = 40.0 / 60.0;
        assert_eq!(
            crowded.top_left,
            Radius {
                x: 30.0 * scale,
                y: 30.0 * scale
            }
        );

        let flat = all(Coord::Points(6.0), Coord::Points(0.0)).resolve(80.0, 20.0);
        assert!(flat.is_square(), "{flat:?}");
    }
}

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
