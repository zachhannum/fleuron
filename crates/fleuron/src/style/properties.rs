//! The supported property vocabulary, and what one node computes to.
//!
//! Everything the engine can honour has a variant here; everything
//! else is a diagnostic. Lengths compute to points, the engine's one
//! unit, and `line-height` computes to a unitless multiple of the
//! font size whatever it was written as.

use serde::Serialize;

use crate::fonts::GenericFamily;
use crate::pages::Side;

/// A CSS length, before it knows what it is relative to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    /// An absolute length, already in points.
    Points(f32),
    /// A multiple of the font size in force.
    Em(f32),
    /// A multiple of the root font size.
    Rem(f32),
    /// A fraction of the reference the property names.
    Percent(f32),
}

impl Length {
    /// The length in points. `relative` is what `em` and percentages
    /// are measured against — the parent font size for `font-size`,
    /// the element's own for everything else.
    pub fn to_points(self, relative: f32, root: f32) -> f32 {
        match self {
            Length::Points(pt) => pt,
            Length::Em(em) => em * relative,
            Length::Rem(rem) => rem * root,
            Length::Percent(percent) => percent / 100.0 * relative,
        }
    }
}

/// A font family as a stylesheet names it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    /// A family name to match against the registry.
    Named(String),
    /// A generic keyword the registry binds to a face.
    Generic(GenericFamily),
}

/// Upright or italic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FontStyle {
    /// `normal`
    Normal,
    /// `italic`, and `oblique` with it.
    Italic,
}

/// How a line's inline content is distributed across the measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    /// `left`
    Left,
    /// `right`
    Right,
    /// `center`
    Center,
    /// `justify`
    Justify,
}

/// Whether words may be broken at syllable boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Hyphens {
    /// `none`, and `manual` with it: only explicit soft hyphens break.
    None,
    /// `auto`
    Auto,
}

/// A fragmentation instruction, as `break-before`, `break-after` and
/// `break-inside` carry it. `recto` and `verso` are the book's names
/// for `right` and `left`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Break {
    /// `auto`
    Auto,
    /// `avoid`
    Avoid,
    /// `page`
    Page,
    /// Break to the next page that falls on the given side, leaving a
    /// blank behind if the flow sits on the wrong one.
    Side(Side),
}

/// Box edges in points, resolved.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Edges {
    /// Top edge in points.
    pub top: f32,
    /// Right edge in points.
    pub right: f32,
    /// Bottom edge in points.
    pub bottom: f32,
    /// Left edge in points.
    pub left: f32,
}

impl Edges {
    /// All four edges the same.
    pub const fn all(value: f32) -> Edges {
        Edges {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
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

/// Page trim and margins, in points: the resolved `@page` box.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct PageGeometry {
    /// Trimmed page width.
    pub width: f32,
    /// Trimmed page height.
    pub height: f32,
    /// Margins, as the page's own `margin` resolved them. Mirroring
    /// across the spread is `@page :left` and `@page :right` saying
    /// different things, not a property of its own.
    pub margin: Edges,
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

    /// The measure line layout breaks to: the content box width.
    pub fn measure(self) -> f32 {
        self.content_size().0
    }
}

/// A page margin box, named as CSS names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
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

/// What a page margin box paints.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Content {
    /// Nothing: the box is not generated.
    None,
    /// The page's own number.
    PageNumber,
    /// A literal string.
    Text(String),
}

/// One declaration the engine understood. The cascade applies these
/// in order, so a later one simply overwrites what an earlier one
/// said about the same property.
#[derive(Debug, Clone, PartialEq)]
pub enum Declaration {
    FontFamily(Vec<Family>),
    FontSize(Length),
    FontStyle(FontStyle),
    FontWeight(u16),
    LineHeight(LineHeight),
    TextAlign(TextAlign),
    TextIndent(Length),
    Hyphens(Hyphens),
    Orphans(u16),
    Widows(u16),
    Page(Option<String>),
    Content(Content),
    InitialLetter(u16),
    Margin(Edge, Length),
    BreakBefore(Break),
    BreakAfter(Break),
    BreakInside(Break),
}

/// `line-height`, before the font size it multiplies is known.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineHeight {
    /// The font's own idea of leading.
    Normal,
    /// A multiple of the font size.
    Number(f32),
    /// A length, which computes to the multiple it works out as.
    Length(Length),
}

impl LineHeight {
    /// The unitless multiple this computes to at `size`.
    pub fn to_multiple(self, size: f32, root: f32) -> f32 {
        match self {
            LineHeight::Normal => NORMAL_LINE_HEIGHT,
            LineHeight::Number(number) => number,
            LineHeight::Length(length) => {
                if size > 0.0 {
                    length.to_points(size, root) / size
                } else {
                    NORMAL_LINE_HEIGHT
                }
            }
        }
    }
}

/// What `line-height: normal` works out to. The strut takes its
/// ascent and descent from the font; this is the factor over them.
const NORMAL_LINE_HEIGHT: f32 = 1.2;

/// One node's resolved style: what every downstream pass reads.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ComputedStyle {
    /// The face this style shapes with, resolved against the
    /// registry. Family, slope and weight chose it; layout only needs
    /// the answer.
    pub font_id: u16,
    /// The families asked for, in order, kept for diagnostics and for
    /// the snapshot the style tree is reviewed through.
    pub font_family: Vec<Family>,
    /// Font size in points.
    pub font_size: f32,
    /// Upright or italic.
    pub font_style: FontStyle,
    /// Weight on the CSS 1–1000 scale.
    pub font_weight: u16,
    /// Line height as a unitless multiple of the font size.
    pub line_height: f32,
    /// How lines fill the measure.
    pub text_align: TextAlign,
    /// First-line indent in points.
    pub text_indent: f32,
    /// Whether words may break at syllable boundaries.
    pub hyphens: Hyphens,
    /// Lines that must be left at the bottom of a fragment.
    pub orphans: u16,
    /// Lines that must be carried to the top of the next one.
    pub widows: u16,
    /// The named page this element's pages take, from `page`.
    pub page: Option<String>,
    /// What the element paints in place of children it has none of:
    /// the ornament a thematic break is set with.
    pub content: Content,
    /// Lines an initial letter is sunk over, from `initial-letter`.
    /// Fewer than two is no drop cap.
    pub initial_letter: u16,
    /// Margins in points.
    pub margin: Edges,
    /// Where a page break falls before this element.
    pub break_before: Break,
    /// Where one falls after it.
    pub break_after: Break,
    /// Whether this element may be split across pages.
    pub break_inside: Break,
}

impl ComputedStyle {
    /// The initial value of every property: what the root computes to
    /// before any rule matches it.
    pub fn initial() -> ComputedStyle {
        ComputedStyle {
            font_id: 0,
            font_family: vec![Family::Generic(GenericFamily::Serif)],
            font_size: 12.0,
            font_style: FontStyle::Normal,
            font_weight: 400,
            line_height: NORMAL_LINE_HEIGHT,
            text_align: TextAlign::Left,
            text_indent: 0.0,
            hyphens: Hyphens::None,
            orphans: 2,
            widows: 2,
            page: None,
            content: Content::None,
            initial_letter: 0,
            margin: Edges::all(0.0),
            break_before: Break::Auto,
            break_after: Break::Auto,
            break_inside: Break::Auto,
        }
    }

    /// A child's starting point: inherited properties carried over,
    /// the rest back at their initial values.
    pub fn inherit(&self) -> ComputedStyle {
        ComputedStyle {
            margin: Edges::all(0.0),
            content: Content::None,
            initial_letter: 0,
            break_before: Break::Auto,
            break_after: Break::Auto,
            break_inside: Break::Auto,
            ..self.clone()
        }
    }

    /// Applies one declaration. `parent_size` is what `em` and
    /// percentages in `font-size` measure against; `root_size` is
    /// what `rem` does.
    pub fn apply(&mut self, declaration: &Declaration, parent_size: f32, root_size: f32) {
        match declaration {
            Declaration::FontFamily(families) => self.font_family = families.clone(),
            Declaration::FontSize(length) => {
                self.font_size = length.to_points(parent_size, root_size).max(0.0)
            }
            Declaration::FontStyle(style) => self.font_style = *style,
            Declaration::FontWeight(weight) => self.font_weight = *weight,
            Declaration::LineHeight(line_height) => {
                self.line_height = line_height.to_multiple(self.font_size, root_size)
            }
            Declaration::TextAlign(align) => self.text_align = *align,
            Declaration::TextIndent(length) => {
                self.text_indent = length.to_points(self.font_size, root_size)
            }
            Declaration::Hyphens(hyphens) => self.hyphens = *hyphens,
            Declaration::Orphans(lines) => self.orphans = *lines,
            Declaration::Widows(lines) => self.widows = *lines,
            Declaration::Page(name) => self.page = name.clone(),
            Declaration::Content(content) => self.content = content.clone(),
            Declaration::InitialLetter(lines) => self.initial_letter = *lines,
            Declaration::Margin(edge, length) => {
                let points = length.to_points(self.font_size, root_size);
                match edge {
                    Edge::Top => self.margin.top = points,
                    Edge::Right => self.margin.right = points,
                    Edge::Bottom => self.margin.bottom = points,
                    Edge::Left => self.margin.left = points,
                }
            }
            Declaration::BreakBefore(value) => self.break_before = *value,
            Declaration::BreakAfter(value) => self.break_after = *value,
            Declaration::BreakInside(value) => self.break_inside = *value,
        }
    }

    /// Everything line layout needs from a style.
    pub fn paragraph(&self) -> crate::lines::ParagraphStyle {
        crate::lines::ParagraphStyle {
            font_id: self.font_id,
            size: self.font_size,
            line_height: self.line_height,
        }
    }
}
