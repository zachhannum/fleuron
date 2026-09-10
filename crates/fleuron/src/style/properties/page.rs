//! The page box: its trim, its margins, the columns it divides
//! into, and the margin boxes around it.

use serde::Serialize;

use super::edges::{BorderStyle, Edges, MEDIUM};

/// The rule painted down a page's gutters, from `column-rule-width`
/// and `column-rule-style`. It takes no room: the gutter is the room
/// it has.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ColumnRule {
    /// How the rule is drawn.
    pub style: BorderStyle,
    /// Thickness in points, whether or not it is drawn.
    pub width: f32,
}

impl ColumnRule {
    /// The initial rule: `medium` wide, and not drawn.
    pub const NONE: ColumnRule = ColumnRule {
        style: BorderStyle::None,
        width: MEDIUM,
    };

    /// The thickness the rule paints at: nothing unless it is drawn.
    pub fn used(self) -> f32 {
        match self.style {
            BorderStyle::None => 0.0,
            BorderStyle::Solid => self.width.max(0.0),
        }
    }
}

/// How a page's content box divides, from `column-count`,
/// `column-width` and `column-gap`.
///
/// Both `column-count` and `column-width` are what the author asked
/// for rather than what the page does: a content box only so wide
/// takes only so many columns of a given width, and
/// [`PageGeometry::column_count`] is where the two meet.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Columns {
    /// `column-count`, or `None` for `auto`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    /// `column-width`: the width a column is asked to have, or
    /// `None` for `auto`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<f32>,
    /// `column-gap`: the gutter between two columns.
    pub gap: f32,
    /// What is painted down each gutter.
    pub rule: ColumnRule,
}

impl Columns {
    /// One column, the whole content box: what a page that declares
    /// no column property divides into.
    pub const fn undivided(gap: f32) -> Columns {
        Columns {
            count: None,
            width: None,
            gap,
            rule: ColumnRule::NONE,
        }
    }

    /// Whether the page asked for no division at all. A page box
    /// that divides into nothing serializes without a `columns`
    /// field.
    pub fn single(&self) -> bool {
        self.count.is_none() && self.width.is_none()
    }
}

/// Page trim, margins and columns, in points: the resolved `@page`
/// box.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct PageGeometry {
    /// Trimmed page width.
    pub width: f32,
    /// Trimmed page height.
    pub height: f32,
    /// Margins, resolved from the page's own `margin`. Mirroring
    /// across the spread is `@page :left` and `@page :right` saying
    /// different things, not a property of its own.
    pub margin: Edges,
    /// How the content box divides.
    #[serde(skip_serializing_if = "Columns::single")]
    pub columns: Columns,
}

impl PageGeometry {
    /// Origin (top-left) of the content box, in page coordinates.
    pub fn content_origin(self) -> (f32, f32) {
        (self.margin.left, self.margin.top)
    }

    /// Size of the content box.
    pub fn content_size(self) -> (f32, f32) {
        (
            self.width - self.margin.left - self.margin.right,
            self.height - self.margin.top - self.margin.bottom,
        )
    }

    /// How many columns the content box divides into.
    ///
    /// A declared `column-width` is a preference: as many columns of
    /// that width as fit, and where `column-count` was declared too
    /// it is the ceiling on that. A box too narrow for one column of
    /// that width still divides into one.
    pub fn column_count(self) -> u32 {
        let available = self.content_size().0;
        let fitting = self.columns.width.map(|width| {
            let pitch = width.max(0.0) + self.columns.gap;
            if pitch <= 0.0 {
                return 1;
            }
            (((available + self.columns.gap) / pitch).floor() as i32).max(1) as u32
        });
        match (self.columns.count, fitting) {
            (Some(count), Some(fitting)) => count.max(1).min(fitting),
            (Some(count), None) => count.max(1),
            (None, Some(fitting)) => fitting,
            (None, None) => 1,
        }
    }

    /// The measure line layout breaks to: one column's width, which
    /// on an undivided page is the content box's own.
    pub fn measure(self) -> f32 {
        let count = self.column_count() as f32;
        let gutters = self.columns.gap * (count - 1.0);
        ((self.content_size().0 - gutters) / count).max(0.0)
    }

    /// Origin (top-left) of one column, in page coordinates.
    pub fn column_origin(self, index: u32) -> (f32, f32) {
        let (x, y) = self.content_origin();
        (x + index as f32 * (self.measure() + self.columns.gap), y)
    }
}

/// A page margin box, named as CSS names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MarginBox {
    /// `@top-left-corner`
    TopLeftCorner,
    /// `@top-left`
    TopLeft,
    /// `@top-center`
    TopCenter,
    /// `@top-right`
    TopRight,
    /// `@top-right-corner`
    TopRightCorner,
    /// `@left-top`
    LeftTop,
    /// `@left-middle`
    LeftMiddle,
    /// `@left-bottom`
    LeftBottom,
    /// `@right-top`
    RightTop,
    /// `@right-middle`
    RightMiddle,
    /// `@right-bottom`
    RightBottom,
    /// `@bottom-left-corner`
    BottomLeftCorner,
    /// `@bottom-left`
    BottomLeft,
    /// `@bottom-center`
    BottomCenter,
    /// `@bottom-right`
    BottomRight,
    /// `@bottom-right-corner`
    BottomRightCorner,
}

impl MarginBox {
    /// The at-rule name, without the `@`.
    pub fn keyword(self) -> &'static str {
        match self {
            MarginBox::TopLeftCorner => "top-left-corner",
            MarginBox::TopLeft => "top-left",
            MarginBox::TopCenter => "top-center",
            MarginBox::TopRight => "top-right",
            MarginBox::TopRightCorner => "top-right-corner",
            MarginBox::LeftTop => "left-top",
            MarginBox::LeftMiddle => "left-middle",
            MarginBox::LeftBottom => "left-bottom",
            MarginBox::RightTop => "right-top",
            MarginBox::RightMiddle => "right-middle",
            MarginBox::RightBottom => "right-bottom",
            MarginBox::BottomLeftCorner => "bottom-left-corner",
            MarginBox::BottomLeft => "bottom-left",
            MarginBox::BottomCenter => "bottom-center",
            MarginBox::BottomRight => "bottom-right",
            MarginBox::BottomRightCorner => "bottom-right-corner",
        }
    }

    /// Parses an at-rule name inside `@page`.
    pub fn parse(keyword: &str) -> Option<MarginBox> {
        Self::ALL
            .into_iter()
            .find(|candidate| candidate.keyword().eq_ignore_ascii_case(keyword))
    }

    /// Every margin box CSS defines.
    pub const ALL: [MarginBox; 16] = [
        MarginBox::TopLeftCorner,
        MarginBox::TopLeft,
        MarginBox::TopCenter,
        MarginBox::TopRight,
        MarginBox::TopRightCorner,
        MarginBox::LeftTop,
        MarginBox::LeftMiddle,
        MarginBox::LeftBottom,
        MarginBox::RightTop,
        MarginBox::RightMiddle,
        MarginBox::RightBottom,
        MarginBox::BottomLeftCorner,
        MarginBox::BottomLeft,
        MarginBox::BottomCenter,
        MarginBox::BottomRight,
        MarginBox::BottomRightCorner,
    ];

    /// The margin the box sits in, and where in it: `None` for the
    /// boxes the engine parses but does not paint.
    pub fn band(self) -> Option<(Band, Align)> {
        match self {
            MarginBox::TopLeft => Some((Band::Top, Align::Start)),
            MarginBox::TopCenter => Some((Band::Top, Align::Center)),
            MarginBox::TopRight => Some((Band::Top, Align::End)),
            MarginBox::BottomLeft => Some((Band::Bottom, Align::Start)),
            MarginBox::BottomCenter => Some((Band::Bottom, Align::Center)),
            MarginBox::BottomRight => Some((Band::Bottom, Align::End)),
            _ => None,
        }
    }
}

/// Which margin a painted margin box lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    /// The top margin: running heads.
    Top,
    /// The bottom margin: folios and running feet.
    Bottom,
}

/// Where in its band a margin box's content sits. `Center` centres on
/// the trim rather than on the content box: a folio belongs on the
/// page's axis, and mirrored margins put the content box off it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// The content box's leading edge.
    Start,
    /// The trim's axis.
    Center,
    /// The content box's trailing edge.
    End,
}
