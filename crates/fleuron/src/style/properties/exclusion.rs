//! A block placed against the page rather than in the flow, and the
//! contour the prose around it keeps clear of.

use serde::Serialize;

use super::edges::Edges;
use super::value::Length;

/// Whether an element sits in the flow or against the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Position {
    /// `static`: the element sits where the flow puts it.
    Static,
    /// `relative`: the element keeps its place in the flow, and the
    /// engine draws it moved by its insets. Nothing around it moves.
    Relative,
    /// `absolute`: the element comes out of the flow and sits against
    /// the page area, at the insets it declares.
    Absolute,
}

/// How far one edge of a positioned box sits from the matching edge of
/// the page area, from `top`, `right`, `bottom` and `left`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Inset {
    /// `auto`: the opposite edge places the box. Where that edge is
    /// `auto` as well, the box sits at the edge of the page area.
    Auto,
    /// A length in points, negative where the box reaches into the
    /// margin.
    Points(f32),
    /// A percentage of the page area's width, for `left` and `right`,
    /// or of its height, for `top` and `bottom`. It stays a percentage
    /// until layout resolves it against the page area.
    Percent(f32),
}

impl Inset {
    /// The inset in points across a page area `extent` wide or tall,
    /// or `None` where it is `auto`.
    pub fn resolve(self, extent: f32) -> Option<f32> {
        match self {
            Inset::Auto => None,
            Inset::Points(points) => Some(points),
            Inset::Percent(percent) => Some(percent / 100.0 * extent),
        }
    }
}

impl Edges<Inset> {
    /// How far `position: relative` moves a box across and down a page
    /// area of `size`. `left` outranks `right` and `top` outranks
    /// `bottom`, as in CSS.
    pub fn offset(self, (width, height): (f32, f32)) -> (f32, f32) {
        let axis = |start: Inset, end: Inset, extent: f32| match (
            start.resolve(extent),
            end.resolve(extent),
        ) {
            (Some(start), _) => start,
            (None, Some(end)) => -end,
            (None, None) => 0.0,
        };
        (
            axis(self.left, self.right, width),
            axis(self.top, self.bottom, height),
        )
    }
}

/// Which side of an exclusion the prose sets on, from `wrap-flow`. An
/// exclusion is a box that the text keeps clear of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WrapFlow {
    /// `auto`: the box excludes nothing, and the text runs under it.
    Auto,
    /// `both`: the prose sets on either side of it.
    Both,
    /// `start`: the prose sets on the side the line starts at.
    Start,
    /// `end`: the prose sets on the side the line ends at.
    End,
}

/// `shape-outside` as the sheet wrote it, before the cascade knows
/// the font size its lengths are relative to.
#[derive(Debug, Clone, PartialEq)]
pub enum ShapeSource {
    /// `none`.
    None,
    /// `auto`.
    Auto,
    /// `polygon(…)`, as pairs of written lengths.
    Polygon(Vec<(Length, Length)>),
}

/// The contour prose sets around, from `shape-outside`.
///
/// A contour is the outline the text keeps clear of, in place of the
/// box.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeOutside {
    /// `none`: the prose keeps clear of the box itself.
    None,
    /// `auto`: the contour comes from the image's own alpha channel.
    /// A format that carries no alpha contributes its box.
    Auto,
    /// `polygon(…)`: the points the sheet wrote, read against the
    /// margin box.
    Polygon(Vec<ShapePoint>),
}

/// One point of a `polygon()`, from the top left of the margin box.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ShapePoint {
    /// Across the box.
    pub x: Coord,
    /// Down the box.
    pub y: Coord,
}

/// One coordinate of a shape: a length, or a fraction of the box the
/// shape is read against.
///
/// `em` and `rem` are points by the time the cascade is done. A
/// percentage is not, because the box it measures against is the
/// image's, and the image is sized in the layout pass.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Coord {
    /// An absolute length in points.
    Points(f32),
    /// A percentage of the box's own width or height.
    Percent(f32),
}

impl Coord {
    /// What the cascade makes of one written length: `em` and `rem`
    /// against the font size in force, a percentage kept as one.
    pub fn of(length: Length, size: f32, root: f32) -> Coord {
        match length {
            Length::Percent(percent) => Coord::Percent(percent),
            other => Coord::Points(other.to_points(size, root)),
        }
    }

    /// The coordinate in points, across a box `extent` wide or tall.
    pub fn to_points(self, extent: f32) -> f32 {
        match self {
            Coord::Points(points) => points,
            Coord::Percent(percent) => percent / 100.0 * extent,
        }
    }
}
