//! What one node computes to: every property resolved, ready for
//! the box tree to ask.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::fonts::{FeatureSetting, GenericFamily};
use crate::lines::HangingPunctuation;

use super::Declaration;
use super::background::{
    Background, BackgroundPosition, BackgroundSize, SizeSource, no_background,
};
use super::counter::{Content, ListStyleType, StringSet};
use super::edges::{Border, BorderRadius, CornerRadius, Edges};
use super::exclusion::{Coord, Inset, Position, ShapeOutside, ShapePoint, ShapeSource, WrapFlow};
use super::value::{
    BorderCollapse, BoxDecorationBreak, Break, Color, ColumnSpan, DecorationLine, DecorationStyle,
    Family, FontStyle, FontVariantAlternates, FontVariantCaps, FontVariantLigatures,
    FontVariantNumeric, Hyphens, Length, NORMAL_LINE_HEIGHT, TextAlign, TextDecoration,
    TextJustify, TextTransform, Width,
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
    /// The features the sheet asked the face for, from
    /// `font-feature-settings`. They are asked for after the ones the
    /// `font-variant` longhands name, so a tag in both is set here.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub font_feature_settings: Vec<FeatureSetting>,
    /// Which ligatures the run is set with.
    #[serde(skip_serializing_if = "default_ligatures")]
    pub font_variant_ligatures: FontVariantLigatures,
    /// Which figures the run is set with.
    #[serde(skip_serializing_if = "default_numeric")]
    pub font_variant_numeric: FontVariantNumeric,
    /// Which alternate glyphs the run is set with.
    #[serde(skip_serializing_if = "default_alternates")]
    pub font_variant_alternates: FontVariantAlternates,
    /// What the run's letters are transformed to before shaping.
    pub text_transform: TextTransform,
    /// Which rules are drawn across the run, from
    /// `text-decoration-line`.
    #[serde(skip_serializing_if = "undecorated")]
    pub text_decoration_line: DecorationLine,
    /// What those rules are painted in, from
    /// `text-decoration-color`. `None` is the colour of the text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_decoration_color: Option<Color>,
    /// How each rule is drawn, from `text-decoration-style`.
    #[serde(skip_serializing_if = "solid")]
    pub text_decoration_style: DecorationStyle,
    /// How thick each rule is, in points, from
    /// `text-decoration-thickness`. `None` is the thickness the face
    /// declares.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_decoration_thickness: Option<f32>,
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
    /// The number the first note under this element takes, from
    /// `counter-reset: note`. On the footnote area it restarts the
    /// numbering on every page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_reset: Option<u32>,
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
    /// How much of what the block paints shows, from `opacity`: 1 is
    /// all of it and 0 is none. The blocks inside it show that much
    /// of their own.
    #[serde(skip_serializing_if = "opaque")]
    pub opacity: f32,
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
    /// How far each corner of the border box is rounded, from
    /// `border-radius`. The background and the border follow the
    /// corners.
    #[serde(skip_serializing_if = "square")]
    pub border_radius: BorderRadius,
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
    /// The height the content box is asked to take, from `height`.
    #[serde(skip_serializing_if = "auto_width")]
    pub height: Width,
    /// The least height the content box takes, from `min-height`.
    #[serde(skip_serializing_if = "auto_width")]
    pub min_height: Width,
    /// The most width an image takes, from `max-width`. `Auto` is
    /// `none`.
    #[serde(skip_serializing_if = "auto_width")]
    pub max_width: Width,
    /// The most height an image takes, from `max-height`. `Auto` is
    /// `none`.
    #[serde(skip_serializing_if = "auto_width")]
    pub max_height: Width,
    /// Whether a table's cells share their borders, from
    /// `border-collapse`.
    #[serde(skip_serializing_if = "separate")]
    pub border_collapse: BorderCollapse,
    /// The marker a list item is set with, from `list-style-type`.
    #[serde(skip_serializing_if = "disc")]
    pub list_style_type: ListStyleType,
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
    /// The custom properties in force, by name, each with every
    /// `var()` in its value already replaced. A child inherits them.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub custom: BTreeMap<String, String>,
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
            font_feature_settings: Vec::new(),
            font_variant_ligatures: FontVariantLigatures::NORMAL,
            font_variant_numeric: FontVariantNumeric::NORMAL,
            font_variant_alternates: FontVariantAlternates::Normal,
            text_transform: TextTransform::None,
            text_decoration_line: DecorationLine::NONE,
            text_decoration_color: None,
            text_decoration_style: DecorationStyle::Solid,
            text_decoration_thickness: None,
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
            note_reset: None,
            initial_letter: 0,
            position: Position::Static,
            inset: Edges::all(Inset::Auto),
            z_index: 0,
            opacity: 1.0,
            wrap_flow: WrapFlow::Auto,
            shape_outside: ShapeOutside::None,
            shape_margin: 0.0,
            margin: Edges::all(0.0),
            padding: Edges::all(0.0),
            border: Edges::all(Border::NONE),
            border_radius: BorderRadius::SQUARE,
            background: Background::NONE,
            box_decoration_break: BoxDecorationBreak::Slice,
            width: Width::Auto,
            height: Width::Auto,
            min_height: Width::Auto,
            max_width: Width::Auto,
            max_height: Width::Auto,
            border_collapse: BorderCollapse::Separate,
            list_style_type: ListStyleType::Disc,
            break_before: Break::Auto,
            break_after: Break::Auto,
            break_inside: Break::Auto,
            column_span: ColumnSpan::None,
            custom: BTreeMap::new(),
        }
    }

    /// A child's starting point: inherited properties as they stand,
    /// the rest back at their initial values.
    pub fn inherit(&self) -> ComputedStyle {
        ComputedStyle {
            margin: Edges::all(0.0),
            padding: Edges::all(0.0),
            border: Edges::all(Border::NONE),
            border_radius: BorderRadius::SQUARE,
            background: Background::NONE,
            box_decoration_break: BoxDecorationBreak::Slice,
            width: Width::Auto,
            height: Width::Auto,
            min_height: Width::Auto,
            max_width: Width::Auto,
            max_height: Width::Auto,
            content: Content::None,
            string_set: Vec::new(),
            counter_reset: None,
            note_reset: None,
            initial_letter: 0,
            position: Position::Static,
            inset: Edges::all(Inset::Auto),
            z_index: 0,
            opacity: 1.0,
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
            Declaration::FontFeatureSettings(settings) => {
                self.font_feature_settings = settings.clone()
            }
            Declaration::FontVariantLigatures(ligatures) => {
                self.font_variant_ligatures = *ligatures
            }
            Declaration::FontVariantNumeric(numeric) => self.font_variant_numeric = *numeric,
            Declaration::FontVariantAlternates(alternates) => {
                self.font_variant_alternates = *alternates
            }
            Declaration::TextTransform(transform) => self.text_transform = *transform,
            Declaration::TextDecorationLine(line) => self.text_decoration_line = *line,
            Declaration::TextDecorationColor(color) => self.text_decoration_color = *color,
            Declaration::TextDecorationStyle(style) => self.text_decoration_style = *style,
            Declaration::TextDecorationThickness(length) => {
                self.text_decoration_thickness =
                    length.map(|length| length.to_points(self.font_size, root_size).max(0.0))
            }
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
            Declaration::CounterReset(counters) => {
                self.counter_reset = counters.page;
                self.note_reset = counters.note;
            }
            Declaration::InitialLetter(lines) => self.initial_letter = *lines,
            Declaration::Position(position) => self.position = *position,
            Declaration::Inset(edge, length) => {
                *self.inset.edge(*edge) = match length {
                    None => Inset::Auto,
                    Some(Length::Percent(percent)) => Inset::Percent(*percent),
                    Some(length) => Inset::Points(length.to_points(self.font_size, root_size)),
                }
            }
            Declaration::ZIndex(layer) => self.z_index = *layer,
            Declaration::Opacity(opacity) => self.opacity = *opacity,
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
            Declaration::BorderRadius(corner, x, y) => {
                *self.border_radius.corner(*corner) = CornerRadius {
                    x: Coord::of(*x, self.font_size, root_size),
                    y: Coord::of(*y, self.font_size, root_size),
                }
            }
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
            Declaration::Width(width) => self.width = self.size(*width, root_size),
            Declaration::Height(height) => self.height = self.size(*height, root_size),
            Declaration::MinHeight(height) => self.min_height = self.size(*height, root_size),
            Declaration::MaxWidth(width) => self.max_width = self.size(*width, root_size),
            Declaration::MaxHeight(height) => self.max_height = self.size(*height, root_size),
            Declaration::BorderCollapse(value) => self.border_collapse = *value,
            Declaration::ListStyleType(value) => self.list_style_type = *value,
            Declaration::BreakBefore(value) => self.break_before = *value,
            Declaration::BreakAfter(value) => self.break_after = *value,
            Declaration::BreakInside(value) => self.break_inside = *value,
            Declaration::ColumnSpan(value) => self.column_span = *value,
            // The cascade resolves both before anything is applied.
            Declaration::Custom(_) | Declaration::Pending(_) => {}
        }
    }

    /// Sets the longhand `like` names back to what `base` holds for
    /// it. `base` is the style before any declaration applied, so a
    /// property that inherits takes its parent's value and one that
    /// does not takes its initial value.
    pub(crate) fn reset(&mut self, like: &Declaration, base: &ComputedStyle) {
        match like {
            Declaration::FontFamily(_) => self.font_family = base.font_family.clone(),
            Declaration::FontSize(_) => self.font_size = base.font_size,
            Declaration::FontStyle(_) => self.font_style = base.font_style,
            Declaration::FontWeight(_) => self.font_weight = base.font_weight,
            Declaration::Color(_) => self.color = base.color,
            Declaration::LineHeight(_) => self.line_height = base.line_height,
            Declaration::LetterSpacing(_) => self.letter_spacing = base.letter_spacing,
            Declaration::FontVariantCaps(_) => self.font_variant_caps = base.font_variant_caps,
            Declaration::FontFeatureSettings(_) => {
                self.font_feature_settings = base.font_feature_settings.clone()
            }
            Declaration::FontVariantLigatures(_) => {
                self.font_variant_ligatures = base.font_variant_ligatures
            }
            Declaration::FontVariantNumeric(_) => {
                self.font_variant_numeric = base.font_variant_numeric
            }
            Declaration::FontVariantAlternates(_) => {
                self.font_variant_alternates = base.font_variant_alternates
            }
            Declaration::TextTransform(_) => self.text_transform = base.text_transform,
            Declaration::TextDecorationLine(_) => {
                self.text_decoration_line = base.text_decoration_line
            }
            Declaration::TextDecorationColor(_) => {
                self.text_decoration_color = base.text_decoration_color
            }
            Declaration::TextDecorationStyle(_) => {
                self.text_decoration_style = base.text_decoration_style
            }
            Declaration::TextDecorationThickness(_) => {
                self.text_decoration_thickness = base.text_decoration_thickness
            }
            Declaration::TextAlign(_) => self.text_align = base.text_align,
            Declaration::TextJustify(_) => self.text_justify = base.text_justify,
            Declaration::HangingPunctuation(_) => {
                self.hanging_punctuation = base.hanging_punctuation
            }
            Declaration::TextIndent(_) => self.text_indent = base.text_indent,
            Declaration::Hyphens(_) => self.hyphens = base.hyphens,
            Declaration::Orphans(_) => self.orphans = base.orphans,
            Declaration::Widows(_) => self.widows = base.widows,
            Declaration::Page(_) => self.page = base.page.clone(),
            Declaration::Content(_) => self.content = base.content.clone(),
            Declaration::StringSet(_) => self.string_set = base.string_set.clone(),
            Declaration::CounterReset(_) => {
                self.counter_reset = base.counter_reset;
                self.note_reset = base.note_reset;
            }
            Declaration::InitialLetter(_) => self.initial_letter = base.initial_letter,
            Declaration::Position(_) => self.position = base.position,
            Declaration::Inset(edge, _) => *self.inset.edge(*edge) = base.inset.get(*edge),
            Declaration::ZIndex(_) => self.z_index = base.z_index,
            Declaration::Opacity(_) => self.opacity = base.opacity,
            Declaration::WrapFlow(_) => self.wrap_flow = base.wrap_flow,
            Declaration::ShapeOutside(_) => self.shape_outside = base.shape_outside.clone(),
            Declaration::ShapeMargin(_) => self.shape_margin = base.shape_margin,
            Declaration::Margin(edge, _) => *self.margin.edge(*edge) = base.margin.get(*edge),
            Declaration::Padding(edge, _) => *self.padding.edge(*edge) = base.padding.get(*edge),
            Declaration::BorderStyle(edge, _) => {
                self.border.edge(*edge).style = base.border.get(*edge).style
            }
            Declaration::BorderWidth(edge, _) => {
                self.border.edge(*edge).width = base.border.get(*edge).width
            }
            Declaration::BorderColor(edge, _) => {
                self.border.edge(*edge).color = base.border.get(*edge).color
            }
            Declaration::BorderRadius(corner, ..) => {
                *self.border_radius.corner(*corner) = base.border_radius.get(*corner)
            }
            Declaration::BackgroundColor(_) => self.background.color = base.background.color,
            Declaration::BackgroundImage(_) => {
                self.background.image = base.background.image.clone()
            }
            Declaration::BackgroundRepeat(_) => self.background.repeat = base.background.repeat,
            Declaration::BackgroundSize(_) => self.background.size = base.background.size,
            Declaration::BackgroundPosition(..) => {
                self.background.position = base.background.position
            }
            Declaration::BoxDecorationBreak(_) => {
                self.box_decoration_break = base.box_decoration_break
            }
            Declaration::Width(_) => self.width = base.width,
            Declaration::Height(_) => self.height = base.height,
            Declaration::MinHeight(_) => self.min_height = base.min_height,
            Declaration::MaxWidth(_) => self.max_width = base.max_width,
            Declaration::MaxHeight(_) => self.max_height = base.max_height,
            Declaration::BorderCollapse(_) => self.border_collapse = base.border_collapse,
            Declaration::ListStyleType(_) => self.list_style_type = base.list_style_type,
            Declaration::BreakBefore(_) => self.break_before = base.break_before,
            Declaration::BreakAfter(_) => self.break_after = base.break_after,
            Declaration::BreakInside(_) => self.break_inside = base.break_inside,
            Declaration::ColumnSpan(_) => self.column_span = base.column_span,
            Declaration::Custom(_) | Declaration::Pending(_) => {}
        }
    }

    /// What a written size computes to. A percentage stays one,
    /// because the box it measures against is sized in the layout
    /// pass.
    fn size(&self, written: Option<Length>, root_size: f32) -> Width {
        match written {
            None => Width::Auto,
            Some(Length::Percent(percent)) => Width::Percent(percent.max(0.0)),
            Some(length) => Width::Points(length.to_points(self.font_size, root_size).max(0.0)),
        }
    }

    /// Whether the block asks for a height of its own content box.
    pub fn sized(&self) -> bool {
        self.height != Width::Auto || self.min_height != Width::Auto
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
            decoration: set(self.decoration() != element.decoration(), self.decoration()),
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

    /// The box this style paints around the runs of an inline
    /// element, and `None` where it paints none and takes no width.
    ///
    /// Padding counts even where nothing is painted over it: it is
    /// width, and the line breaks against it.
    pub fn inline_box(&self) -> Option<crate::lines::InlineBox> {
        let padding = self.padding;
        let border = self.border.widths();
        let paints = self.background.paints() || self.border.paints();
        let spaced = padding != Edges::all(0.0);
        (paints || spaced).then_some(crate::lines::InlineBox {
            padding,
            border,
            cloned: self.box_decoration_break == BoxDecorationBreak::Clone,
        })
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
            features: self.features(),
            transform: self.text_transform,
            decoration: self.decoration(),
        }
    }

    /// Every feature this style asks the face for, in the order the
    /// shaper reads them: what the `font-variant` longhands name,
    /// then what `font-feature-settings` spells out. A tag in both
    /// takes the value written last, which is the one the sheet
    /// spelled.
    pub fn features(&self) -> Vec<FeatureSetting> {
        let mut settings = self.font_variant_ligatures.settings();
        settings.extend(self.font_variant_numeric.settings());
        settings.extend(self.font_variant_alternates.settings());
        settings.extend(self.font_feature_settings.iter().copied());
        settings
    }

    /// What is drawn across this style's runs, the four longhands
    /// gathered into one value.
    pub fn decoration(&self) -> TextDecoration {
        TextDecoration {
            line: self.text_decoration_line,
            color: self.text_decoration_color,
            style: self.text_decoration_style,
            thickness: self.text_decoration_thickness,
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

fn default_ligatures(ligatures: &FontVariantLigatures) -> bool {
    *ligatures == FontVariantLigatures::NORMAL
}

fn default_numeric(numeric: &FontVariantNumeric) -> bool {
    *numeric == FontVariantNumeric::NORMAL
}

fn default_alternates(alternates: &FontVariantAlternates) -> bool {
    *alternates == FontVariantAlternates::Normal
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

fn opaque(opacity: &f32) -> bool {
    *opacity == 1.0
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

fn square(radius: &BorderRadius) -> bool {
    *radius == BorderRadius::SQUARE
}

fn sliced(value: &BoxDecorationBreak) -> bool {
    *value == BoxDecorationBreak::Slice
}

fn undecorated(line: &DecorationLine) -> bool {
    !line.draws()
}

fn solid(style: &DecorationStyle) -> bool {
    *style == DecorationStyle::Solid
}

fn auto_width(width: &Width) -> bool {
    *width == Width::Auto
}

fn separate(collapse: &BorderCollapse) -> bool {
    *collapse == BorderCollapse::Separate
}

fn disc(marker: &ListStyleType) -> bool {
    *marker == ListStyleType::Disc
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

    /// Part: `ComputedStyle` carries the custom properties in force,
    /// and a child inherits them.
    #[test]
    fn a_child_inherits_the_custom_properties() {
        assert!(ComputedStyle::initial().custom.is_empty());
        let mut style = ComputedStyle::initial();
        style.custom.insert("--accent".into(), "#d6075e".into());
        assert_eq!(style.inherit().custom, style.custom);
    }

    /// Part: `ComputedStyle` carries the properties of an inline
    /// box. A style that paints nothing and takes no width has none,
    /// and padding alone is enough for one, because padding is width.
    #[test]
    fn an_inline_box_is_the_padding_border_and_background_of_a_style() {
        let initial = ComputedStyle::initial();
        assert_eq!(initial.inline_box(), None);

        let mut padded = initial.clone();
        padded.padding = Edges::all(4.0);
        let box_ = padded.inline_box().expect("padding alone makes a box");
        assert_eq!((box_.leading(), box_.above()), (4.0, 4.0));
        assert!(!box_.cloned);

        let mut tinted = initial.clone();
        tinted.background.color = Some(Color::BLACK);
        assert!(tinted.inline_box().is_some());

        let mut ruled = initial.clone();
        ruled.border = Edges::all(Border {
            style: super::super::edges::BorderStyle::Solid,
            width: 1.0,
            color: None,
        });
        ruled.box_decoration_break = BoxDecorationBreak::Clone;
        let box_ = ruled.inline_box().expect("a border makes a box");
        assert_eq!(box_.border, Edges::all(1.0));
        assert!(box_.cloned);
    }

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

    /// Part: `height` and `min-height` compute as lengths and
    /// percentages. A length in `em` resolves against the font size in
    /// force, and a percentage stays a percentage until layout
    /// resolves it against the content box.
    #[test]
    fn height_and_min_height_compute_as_lengths_and_percentages() {
        let mut style = ComputedStyle::initial();
        style.font_size = 10.0;
        assert!(!style.sized());
        style.apply(&Declaration::Height(Some(Length::Em(3.0))), 10.0, 16.0);
        style.apply(
            &Declaration::MinHeight(Some(Length::Percent(25.0))),
            10.0,
            16.0,
        );
        assert_eq!(style.height, Width::Points(30.0));
        assert_eq!(style.min_height, Width::Percent(25.0));
        assert_eq!(style.min_height.resolve(540.0), Some(135.0));
        assert!(style.sized());
        assert_eq!(style.inherit().height, Width::Auto);
        assert_eq!(style.inherit().min_height, Width::Auto);
        style.apply(&Declaration::Height(None), 10.0, 16.0);
        assert_eq!(style.height, Width::Auto);
    }

    /// Part: `width`, `height`, `max-width` and `max-height` compute
    /// as lengths and percentages, and a child does not inherit them.
    #[test]
    fn image_sizes_compute_as_lengths_and_percentages_and_do_not_inherit() {
        let mut style = ComputedStyle::initial();
        style.font_size = 10.0;
        style.apply(&Declaration::Width(Some(Length::Points(200.0))), 10.0, 16.0);
        style.apply(
            &Declaration::Height(Some(Length::Percent(50.0))),
            10.0,
            16.0,
        );
        style.apply(&Declaration::MaxWidth(Some(Length::Em(12.0))), 10.0, 16.0);
        style.apply(
            &Declaration::MaxHeight(Some(Length::Percent(40.0))),
            10.0,
            16.0,
        );
        assert_eq!(style.width, Width::Points(200.0));
        assert_eq!(style.height, Width::Percent(50.0));
        assert_eq!(style.max_width, Width::Points(120.0));
        assert_eq!(style.max_height, Width::Percent(40.0));

        let child = style.inherit();
        assert_eq!(
            [child.width, child.height, child.max_width, child.max_height],
            [Width::Auto; 4]
        );

        style.apply(&Declaration::MaxWidth(None), 10.0, 16.0);
        assert_eq!(style.max_width, Width::Auto);
    }

    /// Part: `position: relative` computes, and an inset written as a
    /// percentage stays a percentage until layout resolves it against
    /// the page area.
    /// `left` outranks `right` and `top` outranks `bottom`.
    #[test]
    fn insets_keep_their_percentages_and_offset_a_relative_box() {
        use crate::style::properties::{Edge, Inset, Position};
        let mut style = ComputedStyle::initial();
        style.font_size = 10.0;
        style.apply(&Declaration::Position(Position::Relative), 10.0, 16.0);
        style.apply(
            &Declaration::Inset(Edge::Top, Some(Length::Em(-1.2))),
            10.0,
            16.0,
        );
        style.apply(
            &Declaration::Inset(Edge::Bottom, Some(Length::Points(40.0))),
            10.0,
            16.0,
        );
        style.apply(
            &Declaration::Inset(Edge::Right, Some(Length::Percent(10.0))),
            10.0,
            16.0,
        );
        assert_eq!(style.position, Position::Relative);
        assert_eq!(style.inset.top, Inset::Points(-12.0));
        assert_eq!(style.inset.right, Inset::Percent(10.0));
        assert_eq!(style.inset.right.resolve(300.0), Some(30.0));
        assert_eq!(style.inset.left.resolve(300.0), None);
        assert_eq!(style.inset.offset((300.0, 500.0)), (-30.0, -12.0));
    }
}
