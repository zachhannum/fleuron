//! The supported property vocabulary, and what one node computes to.
//!
//! Everything the engine can honour has a variant here; everything
//! else is a diagnostic. Lengths compute to points, the engine's one
//! unit, and `line-height` computes to a unitless multiple of the
//! font size whatever it was written as.
//!
//! The vocabulary is split by kind: `value` holds the simple values,
//! `edges` the four edges of a box, `background` what is painted
//! behind one, `page` the page box, `counter` what a margin box
//! holds, `exclusion` a block placed against the page, and
//! `computed` what one node comes to.

use crate::lines::HangingPunctuation;

mod background;
mod computed;
mod counter;
mod edges;
mod exclusion;
mod page;
mod value;

pub use background::{
    Background, BackgroundPosition, BackgroundRepeat, BackgroundSize, SizeSource, Url,
};
pub use computed::ComputedStyle;

pub(crate) use computed::computed_size;
pub use counter::{
    Content, ContentPiece, CounterStyle, ListStyleType, StringPiece, StringSet, Target,
};
pub use edges::{Border, BorderStyle, Edge, Edges};
pub use exclusion::{Coord, Inset, Position, ShapeOutside, ShapePoint, ShapeSource, WrapFlow};
pub use page::{Align, AlignContent, Band, ColumnRule, Columns, MarginBox, PageGeometry};
pub use value::{
    BorderCollapse, BoxDecorationBreak, Break, Color, ColumnSpan, Family, FontStyle,
    FontVariantCaps, Hyphens, Length, LineHeight, TextAlign, TextJustify, TextTransform, Width,
};

pub(crate) use background::no_background;
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
    ZIndex(i32),
    WrapFlow(WrapFlow),
    ShapeOutside(ShapeSource),
    ShapeMargin(Length),
    Margin(Edge, Length),
    Padding(Edge, Length),
    BorderStyle(Edge, BorderStyle),
    BorderWidth(Edge, Length),
    BorderColor(Edge, Option<Color>),
    BackgroundColor(Option<Color>),
    BackgroundImage(Option<Url>),
    BackgroundRepeat(BackgroundRepeat),
    BackgroundSize(SizeSource),
    /// Across the box, then down it.
    BackgroundPosition(Length, Length),
    BoxDecorationBreak(BoxDecorationBreak),
    Width(Option<Length>),
    Height(Option<Length>),
    MinHeight(Option<Length>),
    /// `None` is `none`.
    MaxWidth(Option<Length>),
    /// `None` is `none`.
    MaxHeight(Option<Length>),
    BorderCollapse(BorderCollapse),
    ListStyleType(ListStyleType),
    BreakBefore(Break),
    BreakAfter(Break),
    BreakInside(Break),
    ColumnSpan(ColumnSpan),
}

impl Declaration {
    /// The longhand this declaration sets, as CSS names it. Of two
    /// declarations that name the same longhand, the later one in
    /// cascade order is the one that counts.
    pub fn property(&self) -> &'static str {
        let edge = |edge: &Edge, [top, right, bottom, left]: [&'static str; 4]| match edge {
            Edge::Top => top,
            Edge::Right => right,
            Edge::Bottom => bottom,
            Edge::Left => left,
        };
        match self {
            Declaration::FontFamily(_) => "font-family",
            Declaration::Color(_) => "color",
            Declaration::FontSize(_) => "font-size",
            Declaration::FontStyle(_) => "font-style",
            Declaration::FontWeight(_) => "font-weight",
            Declaration::LineHeight(_) => "line-height",
            Declaration::LetterSpacing(_) => "letter-spacing",
            Declaration::FontVariantCaps(_) => "font-variant-caps",
            Declaration::TextTransform(_) => "text-transform",
            Declaration::TextAlign(_) => "text-align",
            Declaration::TextJustify(_) => "text-justify",
            Declaration::TextIndent(_) => "text-indent",
            Declaration::HangingPunctuation(_) => "hanging-punctuation",
            Declaration::Hyphens(_) => "hyphens",
            Declaration::Orphans(_) => "orphans",
            Declaration::Widows(_) => "widows",
            Declaration::Page(_) => "page",
            Declaration::Content(_) => "content",
            Declaration::StringSet(_) => "string-set",
            Declaration::CounterReset(_) => "counter-reset",
            Declaration::InitialLetter(_) => "initial-letter",
            Declaration::Position(_) => "position",
            Declaration::Inset(side, _) => edge(side, ["top", "right", "bottom", "left"]),
            Declaration::ZIndex(_) => "z-index",
            Declaration::WrapFlow(_) => "wrap-flow",
            Declaration::ShapeOutside(_) => "shape-outside",
            Declaration::ShapeMargin(_) => "shape-margin",
            Declaration::Margin(side, _) => edge(
                side,
                ["margin-top", "margin-right", "margin-bottom", "margin-left"],
            ),
            Declaration::Padding(side, _) => edge(
                side,
                [
                    "padding-top",
                    "padding-right",
                    "padding-bottom",
                    "padding-left",
                ],
            ),
            Declaration::BorderWidth(side, _) => edge(
                side,
                [
                    "border-top-width",
                    "border-right-width",
                    "border-bottom-width",
                    "border-left-width",
                ],
            ),
            Declaration::BorderStyle(side, _) => edge(
                side,
                [
                    "border-top-style",
                    "border-right-style",
                    "border-bottom-style",
                    "border-left-style",
                ],
            ),
            Declaration::BorderColor(side, _) => edge(
                side,
                [
                    "border-top-color",
                    "border-right-color",
                    "border-bottom-color",
                    "border-left-color",
                ],
            ),
            Declaration::BackgroundColor(_) => "background-color",
            Declaration::BackgroundImage(_) => "background-image",
            Declaration::BackgroundRepeat(_) => "background-repeat",
            Declaration::BackgroundSize(_) => "background-size",
            Declaration::BackgroundPosition(..) => "background-position",
            Declaration::BoxDecorationBreak(_) => "box-decoration-break",
            Declaration::Width(_) => "width",
            Declaration::Height(_) => "height",
            Declaration::MinHeight(_) => "min-height",
            Declaration::MaxWidth(_) => "max-width",
            Declaration::MaxHeight(_) => "max-height",
            Declaration::BorderCollapse(_) => "border-collapse",
            Declaration::ListStyleType(_) => "list-style-type",
            Declaration::BreakBefore(_) => "break-before",
            Declaration::BreakAfter(_) => "break-after",
            Declaration::BreakInside(_) => "break-inside",
            Declaration::ColumnSpan(_) => "column-span",
        }
    }
}
