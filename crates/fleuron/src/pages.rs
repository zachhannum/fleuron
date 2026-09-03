//! Page output: the display structure.
//!
//! The engine's only product. Painters (SVG preview, PDF export) consume
//! this and never re-derive layout. Coordinates are page units (points),
//! origin top-left.

use std::ops::Range;

use serde::{Deserialize, Serialize};

/// Which side of the spread a page falls on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// A right-hand page.
    Recto,
    /// A left-hand page.
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

/// One typeset page: a number, a side, a trim size, and what to
/// paint on it.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Page {
    /// Folio, counting from 1.
    pub number: u32,
    /// Which side of the spread this page falls on.
    pub side: Side,
    /// Trimmed page width in points.
    pub width: f32,
    /// Trimmed page height in points.
    pub height: f32,
    /// What to paint, in paint order.
    pub items: Vec<DrawItem>,
}

/// A single paint operation. Deliberately tiny: text, rules, images.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub enum DrawItem {
    /// A run of shaped glyphs sharing a font, size, and baseline.
    Text {
        /// Left edge of the run.
        x: f32,
        /// The run's baseline.
        y: f32,
        /// Index into `LayoutOutput::fonts`.
        font_id: u16,
        /// Em size in points.
        size: f32,
        /// The text the glyphs were shaped from. Painters that map
        /// glyphs back to characters — PDF text extraction, copy and
        /// paste — read it through the glyphs' ranges; only the
        /// shaper knew the correspondence, so it travels with them.
        text: String,
        /// The glyphs, in visual order.
        glyphs: Vec<Glyph>,
    },
    /// Filled rectangle: rules, borders, backgrounds.
    Rect {
        /// Left edge.
        x: f32,
        /// Top edge.
        y: f32,
        /// Width in points.
        w: f32,
        /// Height in points.
        h: f32,
    },
    /// Placed image; `asset` indexes the asset table.
    Image {
        /// Left edge.
        x: f32,
        /// Top edge.
        y: f32,
        /// Width in points.
        w: f32,
        /// Height in points.
        h: f32,
        /// Index into the asset table.
        asset: u32,
    },
}

/// One glyph: an id in its font and an absolute x. Kerning and
/// justification mean no two glyphs are uniformly spaced — the glyph is
/// the atom of layout, so positions are per-glyph.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Glyph {
    /// Glyph id in the run's font.
    pub id: u32,
    /// Absolute x of the glyph's origin.
    pub x: f32,
    /// Byte range in the run's `text` this glyph stands for. A
    /// ligature spans several characters, a decomposed cluster puts
    /// several glyphs on one range.
    pub range: Range<u32>,
}
