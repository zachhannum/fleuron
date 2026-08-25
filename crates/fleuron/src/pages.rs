//! Page output: the display list.
//!
//! The engine's only product. Painters (SVG preview, PDF export) consume
//! this and never re-derive layout. Coordinates are page units (points),
//! origin top-left.

use serde::Serialize;

/// Which side of the spread a page falls on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Recto,
    Verso,
}

impl Side {
    /// Books open on a right-hand page: odd numbers are recto.
    pub fn of_number(number: u32) -> Side {
        if number % 2 == 1 {
            Side::Recto
        } else {
            Side::Verso
        }
    }
}

/// One typeset page: a number, a side, and what to paint on it.
#[derive(Debug, Serialize)]
pub struct Page {
    pub number: u32,
    pub side: Side,
    pub items: Vec<DrawItem>,
}

/// A single paint operation. Deliberately tiny: text, rules, images.
#[derive(Debug, Serialize)]
pub enum DrawItem {
    /// A run of shaped glyphs sharing a font, size, and baseline.
    Text {
        x: f32,
        y: f32,
        font_id: u16,
        size: f32,
        glyphs: Vec<Glyph>,
    },
    /// Filled rectangle: rules, borders, backgrounds.
    Rect { x: f32, y: f32, w: f32, h: f32 },
    /// Placed image; `asset` indexes the asset table.
    Image {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        asset: u32,
    },
}

/// One glyph: an id in its font and an absolute x. Kerning and
/// justification mean no two glyphs are uniformly spaced — the glyph is
/// the atom of layout, so positions are per-glyph.
#[derive(Debug, Serialize)]
pub struct Glyph {
    pub id: u32,
    pub x: f32,
}
