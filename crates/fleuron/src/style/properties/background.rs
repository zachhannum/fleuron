//! What is painted behind a box: a tint, and an image over it.
//!
//! The same four properties answer for a block and for `@page`, and
//! the two paint the same way, so one value carries both.

use serde::Serialize;

use super::exclusion::Coord;
use super::value::{Color, Length};

/// A url the sheet named, and where it was written.
///
/// The position travels with the url because the sheet is the only
/// place that knows it: by the time a missing image is noticed, the
/// declaration it was written in is long parsed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct Url {
    /// The url as the sheet wrote it. The engine opens nothing
    /// itself: this is the name the host resolves.
    pub value: String,
    /// Sheet, line and column, as a diagnostic spells them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

impl Url {
    /// A url with no position on it, which is what a host building a
    /// style by hand has.
    pub fn new(value: impl Into<String>) -> Url {
        Url {
            value: value.into(),
            origin: None,
        }
    }
}

/// Whether the image repeats to fill the box, from
/// `background-repeat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackgroundRepeat {
    /// `repeat`: the image tiles across and down the box.
    Repeat,
    /// `no-repeat`: one copy of the image.
    NoRepeat,
}

/// How large the image is drawn, from `background-size`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundSize {
    /// `auto`: the size the image's own header asks for.
    Auto,
    /// `cover`: the smallest size that covers the box, cropped by it.
    Cover,
    /// `contain`: the largest size that fits inside the box.
    Contain,
    /// One or two lengths. An axis left `auto` follows the image's
    /// own ratio.
    Fixed {
        /// Across the box.
        #[serde(skip_serializing_if = "Option::is_none")]
        width: Option<Coord>,
        /// Down the box.
        #[serde(skip_serializing_if = "Option::is_none")]
        height: Option<Coord>,
    },
}

/// `background-size` as the sheet wrote it, before the cascade knows
/// the font size its lengths are relative to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SizeSource {
    /// `auto`.
    Auto,
    /// `cover`.
    Cover,
    /// `contain`.
    Contain,
    /// One or two written lengths, `None` on an axis written `auto`.
    Fixed(Option<Length>, Option<Length>),
}

/// Where the image sits in the box, from `background-position`.
///
/// A percentage aligns that fraction of the image with the same
/// fraction of the box, which is what makes `50%` centre it. A length
/// is an offset from the box's top left corner.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct BackgroundPosition {
    /// Across the box.
    pub x: Coord,
    /// Down the box.
    pub y: Coord,
}

impl BackgroundPosition {
    /// `0% 0%`: the image's top left corner in the box's own.
    pub const ORIGIN: BackgroundPosition = BackgroundPosition {
        x: Coord::Percent(0.0),
        y: Coord::Percent(0.0),
    };
}

/// What one box paints behind its content.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Background {
    /// What the box is tinted, from `background-color`. The image is
    /// painted over it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// The image, from `background-image`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<Url>,
    /// Whether that image repeats.
    #[serde(skip_serializing_if = "repeats")]
    pub repeat: BackgroundRepeat,
    /// How large it is drawn.
    #[serde(skip_serializing_if = "auto_size")]
    pub size: BackgroundSize,
    /// Where it sits.
    #[serde(skip_serializing_if = "at_origin")]
    pub position: BackgroundPosition,
}

impl Background {
    /// Nothing behind the box: the initial value of all four
    /// properties.
    pub const NONE: Background = Background {
        color: None,
        image: None,
        repeat: BackgroundRepeat::Repeat,
        size: BackgroundSize::Auto,
        position: BackgroundPosition::ORIGIN,
    };

    /// Whether anything is painted behind the box at all.
    pub fn paints(&self) -> bool {
        self.color.is_some() || self.image.is_some()
    }
}

impl Default for Background {
    fn default() -> Background {
        Background::NONE
    }
}

fn repeats(repeat: &BackgroundRepeat) -> bool {
    *repeat == BackgroundRepeat::Repeat
}

fn auto_size(size: &BackgroundSize) -> bool {
    *size == BackgroundSize::Auto
}

fn at_origin(position: &BackgroundPosition) -> bool {
    *position == BackgroundPosition::ORIGIN
}

/// Whether a background is the initial one, which is what keeps a
/// style that declares none out of the serialized tree.
pub(crate) fn no_background(background: &Background) -> bool {
    *background == Background::NONE
}
