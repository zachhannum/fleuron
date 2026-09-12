//! Page output: the display structure.
//!
//! The engine's only product. Painters (SVG preview, PDF export) consume
//! this and never re-derive layout. Coordinates are page units (points),
//! origin top-left.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::content::{NodeId, SourceRange};
use crate::fonts::Features;
use crate::style::Color;

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
    /// The sections whose content appears on this page, in the order their
    /// content appears on it. A chapter that ends mid-page is followed
    /// there by the next one opening, so the page names both. A blank
    /// leaf names none.
    pub sections: Vec<NodeId>,
    /// What to paint, in paint order: by layer, and inside one layer
    /// in the order the flow produced it.
    pub items: Vec<DrawItem>,
}

impl Page {
    /// Puts the page's items in paint order. The sort is stable, so
    /// one layer keeps the order the flow produced it in.
    pub(crate) fn sort_by_layer(&mut self) {
        self.items.sort_by_key(DrawItem::layer);
    }
}

/// Where one node's content is set: the folios it runs between, and
/// the pages of the book those folios are.
///
/// `first` and `last` are what a page has printed on it, which is
/// what a host puts on screen. They are read in reading order rather
/// than compared, so a book whose page counter restarts still opens
/// at `first` and a node that fits on one page answers with the same
/// folio twice.
///
/// `at` and `count` are where those pages fall in the book, counting
/// from 0. A page counter that restarts makes them differ from the
/// folios, so they are answered rather than left to arithmetic, and
/// they are the numbers a host fetches the pages by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Folios {
    /// The folio the node's content begins on.
    pub first: u32,
    /// The folio it ends on.
    pub last: u32,
    /// Which page of the book that first folio is, counting from 0.
    pub at: u32,
    /// How many pages the node's content runs across, so `at` and
    /// `count` are every page it is on.
    pub count: u32,
}

/// A single paint operation. Deliberately tiny: text, rules, images.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
        /// The text the glyphs were shaped from, which the glyphs'
        /// ranges index. A painter that draws characters rather than
        /// glyphs draws these.
        text: String,
        /// What the author wrote, where `text-transform` or small
        /// capitals made that differ from what was shaped, and empty
        /// where the two are the same. Text extraction and copy and
        /// paste return this rather than `text`: a title set in
        /// capitals is read back in the case it was written in.
        source: String,
        /// The offset in `source` of every byte boundary of `text`,
        /// so `source_map[range.start]..source_map[range.end]` is the
        /// source a glyph stands for. Empty alongside `source`.
        source_map: Vec<u32>,
        /// Where the run was written: the content node it was shaped
        /// from and the bytes of that node's own text it stands for.
        /// The runs that name one node tile it, so a cursor in the
        /// manuscript lands on a run and a run lands back on the
        /// manuscript. Absent on text the engine adds to the book: a
        /// folio, a running head, a scene break's ornament, a table's
        /// header row set again on a later page, and the text of
        /// `::before` and `::after`.
        origin: Option<SourceRange>,
        /// The features the run was shaped with. A painter that draws
        /// characters asks the face for these; one that draws glyphs
        /// has the answer already.
        features: Features,
        /// What the glyphs are painted in.
        color: Color,
        /// The glyphs, in visual order.
        glyphs: Vec<Glyph>,
        /// Which layer the run paints in.
        layer: i32,
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
        /// What the rectangle is filled with.
        color: Color,
        /// Which layer the rectangle paints in.
        layer: i32,
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
        /// Which layer the image paints in.
        layer: i32,
    },
    /// An image painted behind a box: the page's own, or a block's
    /// border box.
    ///
    /// The box is what the image is clipped to, and the tile is
    /// where one copy of it is drawn, which may reach outside the
    /// box. A painter clips to the box, draws the tile, and repeats
    /// the tile across and down the box where `repeat` asks for it.
    Background {
        /// Left edge of the box the image is painted behind.
        x: f32,
        /// Its top edge.
        y: f32,
        /// Its width in points.
        w: f32,
        /// Its height in points.
        h: f32,
        /// Left edge of the first tile.
        tile_x: f32,
        /// Its top edge.
        tile_y: f32,
        /// The width one copy of the image is drawn at.
        tile_w: f32,
        /// The height one copy is drawn at.
        tile_h: f32,
        /// Whether the tile repeats to cover the box.
        repeat: bool,
        /// Index into the asset table.
        asset: u32,
        /// Which layer the image paints in.
        layer: i32,
    },
}

impl DrawItem {
    /// The layer a page's own background paints in: under every layer
    /// a sheet can name, so the text of a page sits over the art
    /// behind it with no rule in the sheet. A sheet that names this
    /// number still paints over the background, because the page
    /// paints its own first and one layer keeps the order it arrived
    /// in.
    pub const PAGE_BACKGROUND: i32 = i32::MIN;

    /// The layer a page's margin boxes paint in: over every layer a
    /// sheet can name, so a folio is on the page whatever the blocks
    /// of the book are raised over.
    pub const PAGE_FURNITURE: i32 = i32::MAX;

    /// Which layer this item paints in. Higher paints later, over
    /// what a lower layer put down.
    pub fn layer(&self) -> i32 {
        match self {
            DrawItem::Text { layer, .. }
            | DrawItem::Rect { layer, .. }
            | DrawItem::Image { layer, .. }
            | DrawItem::Background { layer, .. } => *layer,
        }
    }
}

/// One glyph: an id in its font and an absolute x. Kerning and
/// justification mean no two glyphs are uniformly spaced — the glyph is
/// the atom of layout, so positions are per-glyph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
