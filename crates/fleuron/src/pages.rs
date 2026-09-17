//! Page output: the display structure.
//!
//! The engine's only product. Painters (SVG preview, PDF export) consume
//! this and never re-derive layout. Coordinates are page units (points),
//! origin top-left.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::content::{NodeId, SourceRange};
use crate::fonts::Features;
use crate::style::{Color, Edges};

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
    /// in the order the blocks are written.
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

/// One box on one page, in points, origin top-left.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PageBox {
    /// Which page of the book the box is on, counting from 0.
    pub page: u32,
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width in points.
    pub width: f32,
    /// Height in points.
    pub height: f32,
}

impl PageBox {
    /// Whether a point on the box's page falls inside it.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        (self.x..=self.x + self.width).contains(&x) && (self.y..=self.y + self.height).contains(&y)
    }
}

/// A single paint operation. Deliberately tiny: text, rules, images,
/// and the rounded boxes a border radius draws.
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
        /// manuscript. The text of `::before` and `::after` names the
        /// id of that pseudo-element. Absent on text the engine adds
        /// to the book: a folio, a running head, a scene break's
        /// ornament, and a table's header row set again on a later
        /// page.
        origin: Option<SourceRange>,
        /// The pseudo-element the run was cut from: a drop cap, the
        /// line a paragraph opens on, or the text of `::before` or
        /// `::after`. `origin` still names the text it stands for.
        pseudo_element: Option<NodeId>,
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
        /// How much of the image shows, from 0 to 255, where 255 is
        /// all of it: `opacity` on the image and the blocks around it.
        alpha: u8,
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
        /// How far each corner of the box is rounded. The image is
        /// clipped to the rounded box.
        radii: Corners,
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
        /// How much of the image shows, from 0 to 255, where 255 is
        /// all of it: `opacity` on the blocks the box belongs to.
        alpha: u8,
        /// Which layer the image paints in.
        layer: i32,
    },
    /// A filled box with rounded corners: a background, or a border
    /// drawn as a ring.
    ///
    /// The outer shape is the box with its corners rounded by `radii`.
    /// Where `ring` is zero on all four edges, the whole shape is
    /// filled. Otherwise the fill is the band between the outer shape
    /// and an inner one: the box `ring` in from each edge, with the
    /// corners [`Corners::inside`] gives.
    Rounded {
        /// Left edge.
        x: f32,
        /// Top edge.
        y: f32,
        /// Width in points.
        w: f32,
        /// Height in points.
        h: f32,
        /// How far each corner is rounded.
        radii: Corners,
        /// How far in from each edge the fill reaches, in points. Zero
        /// on all four fills the whole shape.
        ring: Edges,
        /// What the shape is filled with.
        color: Color,
        /// Which layer the shape paints in.
        layer: i32,
    },
}

/// How far one corner of a box is rounded: the two radii of the
/// quarter ellipse the corner follows, in points.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Radius {
    /// Along the top or bottom edge.
    pub x: f32,
    /// Along the left or right edge.
    pub y: f32,
}

impl Radius {
    /// A corner that is not rounded.
    pub const SQUARE: Radius = Radius { x: 0.0, y: 0.0 };

    /// Whether the corner is rounded at all. A radius of zero on
    /// either axis leaves it square.
    pub fn is_square(self) -> bool {
        self.x <= 0.0 || self.y <= 0.0
    }
}

/// The four corners of a box, each rounded by its own radius.
///
/// The radii of two corners on one edge never add up to more than the
/// edge is long: layout scales all four down together until they fit.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Corners {
    /// The top left corner.
    pub top_left: Radius,
    /// The top right corner.
    pub top_right: Radius,
    /// The bottom right corner.
    pub bottom_right: Radius,
    /// The bottom left corner.
    pub bottom_left: Radius,
}

impl Corners {
    /// Four square corners.
    pub const SQUARE: Corners = Corners {
        top_left: Radius::SQUARE,
        top_right: Radius::SQUARE,
        bottom_right: Radius::SQUARE,
        bottom_left: Radius::SQUARE,
    };

    /// Whether no corner is rounded.
    pub fn is_square(&self) -> bool {
        [
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        ]
        .iter()
        .all(|radius| radius.is_square())
    }

    /// The corners of the box `ring` in from this one. Each radius
    /// loses the width of the edge it runs along, and none goes below
    /// zero, so a ring wider than a radius leaves that inner corner
    /// square.
    pub fn inside(&self, ring: Edges) -> Corners {
        let less = |radius: Radius, across: f32, down: f32| Radius {
            x: (radius.x - across).max(0.0),
            y: (radius.y - down).max(0.0),
        };
        Corners {
            top_left: less(self.top_left, ring.left, ring.top),
            top_right: less(self.top_right, ring.right, ring.top),
            bottom_right: less(self.bottom_right, ring.right, ring.bottom),
            bottom_left: less(self.bottom_left, ring.left, ring.bottom),
        }
    }
}

impl DrawItem {
    /// The layer a page's own background paints in: under every layer
    /// a stylesheet can name. Where a stylesheet names no `z-index`,
    /// the text of a page still covers the background. A stylesheet
    /// that names this number paints over the background as well. The
    /// page puts its own background in first, and one layer keeps the
    /// order it arrived in.
    pub const PAGE_BACKGROUND: i32 = i32::MIN;

    /// The layer a page's margin boxes paint in: over every layer a
    /// stylesheet can name. A page number and a running head stay
    /// visible whatever layer the blocks of the book are raised to.
    pub const PAGE_FURNITURE: i32 = i32::MAX;

    /// Which layer this item paints in. Higher paints later, over
    /// what a lower layer put down.
    pub fn layer(&self) -> i32 {
        match self {
            DrawItem::Text { layer, .. }
            | DrawItem::Rect { layer, .. }
            | DrawItem::Image { layer, .. }
            | DrawItem::Background { layer, .. }
            | DrawItem::Rounded { layer, .. } => *layer,
        }
    }
}

/// An eight-bit alpha scaled by `opacity`, from 0 to 1.
pub(crate) fn fade(alpha: u8, opacity: f32) -> u8 {
    (alpha as f32 * opacity.clamp(0.0, 1.0)).round() as u8
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

/// What a reader of the book on a screen follows: the links on its
/// pages and the outline of its headings.
///
/// A painter that can express a link or an outline reads this. One
/// that cannot paints the pages alone, and loses nothing a printed
/// page shows.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Navigation {
    /// Every link in the book, one for each line it is set on, in the
    /// order of the pages.
    pub links: Vec<Link>,
    /// The book's headings, nested by level. Empty for a book with no
    /// heading.
    pub outline: Vec<OutlineEntry>,
}

impl Navigation {
    /// Whether the book has no link and no heading.
    pub fn is_empty(&self) -> bool {
        self.links.is_empty() && self.outline.is_empty()
    }
}

/// One line of one link: the area its text covers on that line, and
/// where it goes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    /// The area of the link's text on one line: its glyphs across,
    /// and the ascent and descent of its face down.
    pub area: PageBox,
    /// Where the link goes.
    pub to: LinkTo,
}

/// Where a link goes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LinkTo {
    /// A place in the book: the box of the element the link names, on
    /// the page that element opens on.
    Place(PageBox),
    /// Something outside the book, by its url.
    Uri(String),
}

/// One heading in the outline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutlineEntry {
    /// The heading's words, markup discarded and a line break as a
    /// space.
    pub title: String,
    /// The heading's level, from 1 to 6.
    pub level: u8,
    /// The box of the heading, on the page it is set on.
    pub place: PageBox,
    /// The headings after this one and deeper than it, up to the next
    /// heading at its level or above.
    pub children: Vec<OutlineEntry>,
}
