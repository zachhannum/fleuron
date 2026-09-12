//! What one node computes to: every property resolved, ready for
//! the box tree to ask.

use serde::Serialize;

use crate::fonts::GenericFamily;
use crate::lines::HangingPunctuation;

use super::Declaration;
use super::background::{
    Background, BackgroundPosition, BackgroundSize, SizeSource, no_background,
};
use super::counter::{Content, StringSet};
use super::edges::{Border, Edges};
use super::exclusion::{Coord, Inset, Position, ShapeOutside, ShapePoint, ShapeSource, WrapFlow};
use super::value::{
    BorderCollapse, BoxDecorationBreak, Break, Color, ColumnSpan, Family, FontStyle,
    FontVariantCaps, Hyphens, Length, NORMAL_LINE_HEIGHT, TextAlign, TextJustify, TextTransform,
    Width,
};

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
    /// What text and rules are painted in.
    pub color: Color,
    /// Line height as a unitless multiple of the font size.
    pub line_height: f32,
    /// Extra advance after every glyph, in points, from
    /// `letter-spacing`.
    pub letter_spacing: f32,
    /// Which capitals the run is drawn with.
    pub font_variant_caps: FontVariantCaps,
    /// What the run's letters are transformed to before shaping.
    pub text_transform: TextTransform,
    /// How lines fill the measure.
    pub text_align: TextAlign,
    /// What justification opens up to fill it.
    pub text_justify: TextJustify,
    /// Which marks may hang past the measure.
    pub hanging_punctuation: HangingPunctuation,
    /// First-line indent in points.
    pub text_indent: f32,
    /// Whether words may break at syllable boundaries.
    pub hyphens: Hyphens,
    /// Lines that must be left at the bottom of a fragment.
    pub orphans: u16,
    /// Lines that must be moved to the top of the next one.
    pub widows: u16,
    /// The named page this element's pages take, from `page`.
    pub page: Option<String>,
    /// What the element paints in place of children it has none of:
    /// the ornament a thematic break is set with.
    pub content: Content,
    /// The named strings this element sets when the flow reaches it,
    /// from `string-set`. This is where a running head's text comes
    /// from.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub string_set: Vec<StringSet>,
    /// The folio the page this element opens takes, from
    /// `counter-reset: page`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counter_reset: Option<u32>,
    /// Lines an initial letter is sunk over, from `initial-letter`.
    /// Fewer than two is no drop cap.
    pub initial_letter: u16,
    /// Whether the element sits in the flow or against the page.
    #[serde(skip_serializing_if = "in_flow")]
    pub position: Position,
    /// What the insets say, for an element that is against the page.
    #[serde(skip_serializing_if = "unplaced")]
    pub inset: Edges<Inset>,
    /// Which layer the block paints in, from `z-index`. Higher paints
    /// later, and `auto` is layer 0.
    #[serde(skip_serializing_if = "ground_layer")]
    pub z_index: i32,
    /// Which side of this element the prose sets on, from `wrap-flow`.
    #[serde(skip_serializing_if = "wraps_nothing")]
    pub wrap_flow: WrapFlow,
    /// The contour the prose sets around, from `shape-outside`.
    #[serde(skip_serializing_if = "wraps_its_box")]
    pub shape_outside: ShapeOutside,
    /// How far the prose keeps off that contour, in points, from
    /// `shape-margin`.
    #[serde(skip_serializing_if = "no_shape_margin")]
    pub shape_margin: f32,
    /// Margins in points.
    pub margin: Edges,
    /// Padding in points, between the border and the content.
    #[serde(skip_serializing_if = "no_padding")]
    pub padding: Edges,
    /// The four border edges.
    #[serde(skip_serializing_if = "no_border")]
    pub border: Edges<Border>,
    /// What is painted behind the block: a tint, and an image over
    /// it.
    #[serde(skip_serializing_if = "no_background")]
    pub background: Background,
    /// What the decoration does where a page break splits the block.
    #[serde(skip_serializing_if = "sliced")]
    pub box_decoration_break: BoxDecorationBreak,
    /// The width the box is asked to take, from `width`. A table reads
    /// it off the cells of its first row to size its columns.
    #[serde(skip_serializing_if = "auto_width")]
    pub width: Width,
    /// Whether a table's cells share their borders, from
    /// `border-collapse`.
    #[serde(skip_serializing_if = "separate")]
    pub border_collapse: BorderCollapse,
    /// Where a page break falls before this element.
    pub break_before: Break,
    /// Where one falls after it.
    pub break_after: Break,
    /// Whether this element may be split across pages.
    pub break_inside: Break,
    /// Whether this element is set across every column of a divided
    /// page, from `column-span`.
    #[serde(skip_serializing_if = "one_column")]
    pub column_span: ColumnSpan,
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
            color: Color::BLACK,
            line_height: NORMAL_LINE_HEIGHT,
            letter_spacing: 0.0,
            font_variant_caps: FontVariantCaps::Normal,
            text_transform: TextTransform::None,
            text_align: TextAlign::Left,
            text_justify: TextJustify::InterWord,
            hanging_punctuation: HangingPunctuation::NONE,
            text_indent: 0.0,
            hyphens: Hyphens::None,
            orphans: 2,
            widows: 2,
            page: None,
            content: Content::None,
            string_set: Vec::new(),
            counter_reset: None,
            initial_letter: 0,
            position: Position::Static,
            inset: Edges::all(Inset::Auto),
            z_index: 0,
            wrap_flow: WrapFlow::Auto,
            shape_outside: ShapeOutside::None,
            shape_margin: 0.0,
            margin: Edges::all(0.0),
            padding: Edges::all(0.0),
            border: Edges::all(Border::NONE),
            background: Background::NONE,
            box_decoration_break: BoxDecorationBreak::Slice,
            width: Width::Auto,
            border_collapse: BorderCollapse::Separate,
            break_before: Break::Auto,
            break_after: Break::Auto,
            break_inside: Break::Auto,
            column_span: ColumnSpan::None,
        }
    }

    /// A child's starting point: inherited properties as they stand,
    /// the rest back at their initial values.
    pub fn inherit(&self) -> ComputedStyle {
        ComputedStyle {
            margin: Edges::all(0.0),
            padding: Edges::all(0.0),
            border: Edges::all(Border::NONE),
            background: Background::NONE,
            box_decoration_break: BoxDecorationBreak::Slice,
            width: Width::Auto,
            content: Content::None,
            string_set: Vec::new(),
            counter_reset: None,
            initial_letter: 0,
            position: Position::Static,
            inset: Edges::all(Inset::Auto),
            z_index: 0,
            wrap_flow: WrapFlow::Auto,
            shape_outside: ShapeOutside::None,
            shape_margin: 0.0,
            break_before: Break::Auto,
            break_after: Break::Auto,
            break_inside: Break::Auto,
            column_span: ColumnSpan::None,
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
            Declaration::Color(color) => self.color = *color,
            Declaration::LineHeight(line_height) => {
                self.line_height = line_height.to_multiple(self.font_size, root_size)
            }
            Declaration::LetterSpacing(length) => {
                self.letter_spacing = length.to_points(self.font_size, root_size)
            }
            Declaration::FontVariantCaps(caps) => self.font_variant_caps = *caps,
            Declaration::TextTransform(transform) => self.text_transform = *transform,
            Declaration::TextAlign(align) => self.text_align = *align,
            Declaration::TextJustify(justify) => self.text_justify = *justify,
            Declaration::HangingPunctuation(hanging) => self.hanging_punctuation = *hanging,
            Declaration::TextIndent(length) => {
                self.text_indent = length.to_points(self.font_size, root_size)
            }
            Declaration::Hyphens(hyphens) => self.hyphens = *hyphens,
            Declaration::Orphans(lines) => self.orphans = *lines,
            Declaration::Widows(lines) => self.widows = *lines,
            Declaration::Page(name) => self.page = name.clone(),
            Declaration::Content(content) => self.content = content.clone(),
            Declaration::StringSet(sets) => self.string_set = sets.clone(),
            Declaration::CounterReset(folio) => self.counter_reset = *folio,
            Declaration::InitialLetter(lines) => self.initial_letter = *lines,
            Declaration::Position(position) => self.position = *position,
            Declaration::Inset(edge, length) => {
                *self.inset.edge(*edge) = match length {
                    None => Inset::Auto,
                    Some(length) => Inset::Points(length.to_points(self.font_size, root_size)),
                }
            }
            Declaration::ZIndex(layer) => self.z_index = *layer,
            Declaration::WrapFlow(wrap) => self.wrap_flow = *wrap,
            Declaration::ShapeOutside(shape) => {
                self.shape_outside = match shape {
                    ShapeSource::None => ShapeOutside::None,
                    ShapeSource::Auto => ShapeOutside::Auto,
                    ShapeSource::Polygon(points) => ShapeOutside::Polygon(
                        points
                            .iter()
                            .map(|(x, y)| ShapePoint {
                                x: Coord::of(*x, self.font_size, root_size),
                                y: Coord::of(*y, self.font_size, root_size),
                            })
                            .collect(),
                    ),
                }
            }
            Declaration::ShapeMargin(length) => {
                self.shape_margin = length.to_points(self.font_size, root_size).max(0.0)
            }
            Declaration::Margin(edge, length) => {
                *self.margin.edge(*edge) = length.to_points(self.font_size, root_size)
            }
            Declaration::Padding(edge, length) => {
                *self.padding.edge(*edge) = length.to_points(self.font_size, root_size).max(0.0)
            }
            Declaration::BorderStyle(edge, style) => self.border.edge(*edge).style = *style,
            Declaration::BorderWidth(edge, length) => {
                self.border.edge(*edge).width = length.to_points(self.font_size, root_size).max(0.0)
            }
            Declaration::BorderColor(edge, color) => self.border.edge(*edge).color = *color,
            Declaration::BackgroundColor(color) => self.background.color = *color,
            Declaration::BackgroundImage(url) => self.background.image = url.clone(),
            Declaration::BackgroundRepeat(repeat) => self.background.repeat = *repeat,
            Declaration::BackgroundSize(size) => {
                self.background.size = computed_size(*size, self.font_size, root_size)
            }
            Declaration::BackgroundPosition(x, y) => {
                self.background.position = BackgroundPosition {
                    x: Coord::of(*x, self.font_size, root_size),
                    y: Coord::of(*y, self.font_size, root_size),
                }
            }
            Declaration::BoxDecorationBreak(value) => self.box_decoration_break = *value,
            Declaration::Width(width) => {
                self.width = match width {
                    None => Width::Auto,
                    Some(Length::Percent(percent)) => Width::Percent(percent.max(0.0)),
                    Some(length) => {
                        Width::Points(length.to_points(self.font_size, root_size).max(0.0))
                    }
                }
            }
            Declaration::BorderCollapse(value) => self.border_collapse = *value,
            Declaration::BreakBefore(value) => self.break_before = *value,
            Declaration::BreakAfter(value) => self.break_after = *value,
            Declaration::BreakInside(value) => self.break_inside = *value,
            Declaration::ColumnSpan(value) => self.column_span = *value,
        }
    }

    /// What this style, computed for `::first-line`, changes about
    /// the element it was computed beside.
    pub fn first_line_over(&self, element: &ComputedStyle) -> crate::lines::FirstLine {
        fn set<T>(differs: bool, value: T) -> Option<T> {
            differs.then_some(value)
        }
        crate::lines::FirstLine {
            size: set(self.font_size != element.font_size, self.font_size),
            letter_spacing: set(
                self.letter_spacing != element.letter_spacing,
                self.letter_spacing,
            ),
            caps: set(
                self.font_variant_caps != element.font_variant_caps,
                self.font_variant_caps,
            ),
            transform: set(
                self.text_transform != element.text_transform,
                self.text_transform,
            ),
            color: set(self.color != element.color, self.color),
        }
    }

    /// This block's border box inside `measure`, laid out at `x` from
    /// the enclosing content box's leading edge: `(leading edge,
    /// width)`, with only the margins taken off.
    pub fn border_box(&self, x: f32, measure: f32) -> (f32, f32) {
        (
            x + self.margin.left,
            (measure - self.margin.inline()).max(0.0),
        )
    }

    /// Whether prose is set around this element rather than over it:
    /// the sheet has taken it out of the flow, and it excludes a side.
    ///
    /// This is the one place a contour is read, so it is also what
    /// decides whether one is traced. An element still in the flow
    /// takes a band of its own and nothing sets beside it, so a
    /// contour on it would answer a question nobody asks.
    pub fn excludes(&self) -> bool {
        self.position == Position::Absolute && self.wrap_flow != WrapFlow::Auto
    }

    /// This block's content box inside `measure`: `(leading edge, the
    /// measure its lines break to)`. Margin, border and padding come
    /// off both edges, and the leading edge moves in by what they
    /// take on the left.
    pub fn content_box(&self, x: f32, measure: f32) -> (f32, f32) {
        let border = self.border.widths();
        let leading = self.margin.left + border.left + self.padding.left;
        let trailing = self.margin.right + border.right + self.padding.right;
        (x + leading, (measure - leading - trailing).max(0.0))
    }

    /// Everything line layout needs from a style.
    pub fn paragraph(&self) -> crate::lines::ParagraphStyle {
        crate::lines::ParagraphStyle {
            font_id: self.font_id,
            size: self.font_size,
            color: self.color,
            line_height: self.line_height,
            letter_spacing: self.letter_spacing,
            caps: self.font_variant_caps,
            transform: self.text_transform,
        }
    }
}

/// What the cascade makes of a written `background-size`: `em` and
/// `rem` against the font size in force, a percentage kept as one,
/// because the box it measures against is sized in the layout pass.
pub(crate) fn computed_size(size: SizeSource, font_size: f32, root_size: f32) -> BackgroundSize {
    let axis =
        |length: Option<Length>| length.map(|length| Coord::of(length, font_size, root_size));
    match size {
        SizeSource::Auto => BackgroundSize::Auto,
        SizeSource::Cover => BackgroundSize::Cover,
        SizeSource::Contain => BackgroundSize::Contain,
        SizeSource::Fixed(width, height) => BackgroundSize::Fixed {
            width: axis(width),
            height: axis(height),
        },
    }
}

fn in_flow(position: &Position) -> bool {
    *position == Position::Static
}

fn unplaced(inset: &Edges<Inset>) -> bool {
    *inset == Edges::all(Inset::Auto)
}

fn ground_layer(layer: &i32) -> bool {
    *layer == 0
}

fn wraps_nothing(wrap: &WrapFlow) -> bool {
    *wrap == WrapFlow::Auto
}

fn no_padding(padding: &Edges) -> bool {
    *padding == Edges::all(0.0)
}

fn no_border(border: &Edges<Border>) -> bool {
    *border == Edges::all(Border::NONE)
}

fn sliced(value: &BoxDecorationBreak) -> bool {
    *value == BoxDecorationBreak::Slice
}

fn auto_width(width: &Width) -> bool {
    *width == Width::Auto
}

fn separate(collapse: &BorderCollapse) -> bool {
    *collapse == BorderCollapse::Separate
}

fn one_column(span: &ColumnSpan) -> bool {
    *span == ColumnSpan::None
}

fn no_shape_margin(margin: &f32) -> bool {
    *margin == 0.0
}

fn wraps_its_box(shape: &ShapeOutside) -> bool {
    *shape == ShapeOutside::None
}

#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::properties::{
        Coord, CounterStyle, Declaration, Length, ShapeOutside, ShapeSource,
    };

    /// Folios spell out in every style the subset supports, and a
    /// value a style has no spelling for falls back to decimal.
    #[test]
    fn counter_styles_spell_their_values() {
        let spell = |style: CounterStyle, values: [u32; 4]| {
            values.map(|value| style.format(value)).join(" ")
        };
        assert_eq!(spell(CounterStyle::Decimal, [1, 4, 9, 40]), "1 4 9 40");
        assert_eq!(spell(CounterStyle::LowerRoman, [1, 4, 9, 40]), "i iv ix xl");
        assert_eq!(spell(CounterStyle::UpperRoman, [1, 4, 9, 40]), "I IV IX XL");
        assert_eq!(spell(CounterStyle::LowerAlpha, [1, 4, 26, 27]), "a d z aa");
        assert_eq!(spell(CounterStyle::UpperAlpha, [1, 4, 26, 27]), "A D Z AA");
        assert_eq!(CounterStyle::LowerRoman.format(0), "0");
        assert_eq!(CounterStyle::LowerRoman.format(4000), "4000");
        assert_eq!(CounterStyle::LowerAlpha.format(0), "0");
    }

    /// A polygon point written as a length computes to points against
    /// the font size in force. One written as a percentage stays a
    /// percentage, because the box it measures against is the image's
    /// and the image is sized in the layout pass.
    #[test]
    fn a_polygon_keeps_its_percentages_and_computes_its_lengths() {
        let mut style = ComputedStyle::initial();
        style.font_size = 10.0;
        style.apply(
            &Declaration::ShapeOutside(ShapeSource::Polygon(vec![
                (Length::Em(2.0), Length::Percent(50.0)),
                (Length::Points(3.0), Length::Rem(1.0)),
                (Length::Percent(100.0), Length::Percent(100.0)),
            ])),
            10.0,
            16.0,
        );
        let ShapeOutside::Polygon(points) = &style.shape_outside else {
            panic!(
                "the polygon did not reach the style: {:?}",
                style.shape_outside
            );
        };
        assert_eq!(points[0].x, Coord::Points(20.0));
        assert_eq!(points[0].y, Coord::Percent(50.0));
        assert_eq!(points[1].x, Coord::Points(3.0));
        assert_eq!(points[1].y, Coord::Points(16.0));
        assert_eq!(points[2].x.to_points(80.0), 80.0);
        assert_eq!(points[2].y.to_points(40.0), 40.0);

        style.apply(&Declaration::ShapeMargin(Length::Em(1.5)), 10.0, 16.0);
        assert_eq!(style.shape_margin, 15.0);
    }
}
