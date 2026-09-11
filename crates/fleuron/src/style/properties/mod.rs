//! The supported property vocabulary, and what one node computes to.
//!
//! Everything the engine can honour has a variant here; everything
//! else is a diagnostic. Lengths compute to points, the engine's one
//! unit, and `line-height` computes to a unitless multiple of the
//! font size whatever it was written as.
//!
//! The vocabulary is split by kind: `value` holds the simple values,
//! `edges` the four edges of a box, `page` the page box, `counter`
//! what a margin box holds, `exclusion` a block placed against the
//! page, and `computed` what one node comes to.

use crate::lines::HangingPunctuation;

mod computed;
mod counter;
mod edges;
mod exclusion;
mod page;
mod value;

pub use computed::ComputedStyle;
pub use counter::{Content, CounterStyle, StringPiece, StringSet};
pub use edges::{Border, BorderStyle, Edge, Edges};
pub use exclusion::{Coord, Inset, Position, ShapeOutside, ShapePoint, ShapeSource, WrapFlow};
pub use page::{Align, Band, ColumnRule, Columns, MarginBox, PageGeometry};
pub use value::{
    BorderCollapse, BoxDecorationBreak, Break, Color, ColumnSpan, Family, FontStyle,
    FontVariantCaps, Hyphens, Length, LineHeight, TextAlign, TextJustify, TextTransform, Width,
};

pub(crate) use edges::{LINE_WIDTHS, MEDIUM};

/// One declaration the engine understood. The cascade applies these
/// in order, so a later one simply overwrites what an earlier one
/// said about the same property.
#[derive(Debug, Clone, PartialEq)]
pub enum Declaration {
    FontFamily(Vec<Family>),
    Color(Color),
    FontSize(Length),
    FontStyle(FontStyle),
    FontWeight(u16),
    LineHeight(LineHeight),
    LetterSpacing(Length),
    FontVariantCaps(FontVariantCaps),
    TextTransform(TextTransform),
    TextAlign(TextAlign),
    TextJustify(TextJustify),
    TextIndent(Length),
    HangingPunctuation(HangingPunctuation),
    Hyphens(Hyphens),
    Orphans(u16),
    Widows(u16),
    Page(Option<String>),
    Content(Content),
    StringSet(Vec<StringSet>),
    CounterReset(Option<u32>),
    InitialLetter(u16),
    Position(Position),
    Inset(Edge, Option<Length>),
    WrapFlow(WrapFlow),
    ShapeOutside(ShapeSource),
    ShapeMargin(Length),
    Margin(Edge, Length),
    Padding(Edge, Length),
    BorderStyle(Edge, BorderStyle),
    BorderWidth(Edge, Length),
    BorderColor(Edge, Option<Color>),
    BackgroundColor(Option<Color>),
    BoxDecorationBreak(BoxDecorationBreak),
    Width(Option<Length>),
    BorderCollapse(BorderCollapse),
    BreakBefore(Break),
    BreakAfter(Break),
    BreakInside(Break),
    ColumnSpan(ColumnSpan),
}
