//! Computed values written back as CSS, for an author to read.

use std::collections::BTreeMap;

use crate::lines::{HangEnd, HangingPunctuation};
use crate::pages::Side;
use crate::style::sheet::PROPERTIES;
use crate::style::{
    BackgroundRepeat, BackgroundSize, Border, BorderCollapse, BorderStyle, BoxDecorationBreak,
    Break, ColumnSpan, ComputedStyle, Content, ContentPiece, Coord, CounterStyle, Edges, Family,
    FontStyle, FontVariantCaps, Hyphens, Inset, ListStyleType, Position, ShapeOutside, StringPiece,
    StringSet, Target, TextAlign, TextJustify, TextTransform, Width, WrapFlow,
};

/// The computed value of every property a style rule can declare.
pub(super) fn computed(style: &ComputedStyle) -> BTreeMap<String, String> {
    PROPERTIES
        .iter()
        .map(|spec| (spec.name.to_string(), value(style, spec.name)))
        .collect()
}

fn value(style: &ComputedStyle, property: &str) -> String {
    let border = &style.border;
    let background = &style.background;
    match property {
        "font-family" => style
            .font_family
            .iter()
            .map(family)
            .collect::<Vec<_>>()
            .join(", "),
        "font-size" => points(style.font_size),
        "font-style" => match style.font_style {
            FontStyle::Normal => "normal",
            FontStyle::Italic => "italic",
        }
        .into(),
        "font-weight" => style.font_weight.to_string(),
        "color" => style.color.to_hex(),
        "line-height" => number(style.line_height),
        "letter-spacing" => points(style.letter_spacing),
        "font-variant-caps" => match style.font_variant_caps {
            FontVariantCaps::Normal => "normal",
            FontVariantCaps::SmallCaps => "small-caps",
        }
        .into(),
        "text-transform" => match style.text_transform {
            TextTransform::None => "none",
            TextTransform::Uppercase => "uppercase",
            TextTransform::Lowercase => "lowercase",
            TextTransform::Capitalize => "capitalize",
        }
        .into(),
        "text-align" => match style.text_align {
            TextAlign::Left => "left",
            TextAlign::Right => "right",
            TextAlign::Center => "center",
            TextAlign::Justify => "justify",
        }
        .into(),
        "text-justify" => match style.text_justify {
            TextJustify::InterWord => "inter-word",
            TextJustify::InterCharacter => "inter-character",
        }
        .into(),
        "text-indent" => points(style.text_indent),
        "hanging-punctuation" => hanging(style.hanging_punctuation),
        "hyphens" => match style.hyphens {
            Hyphens::None => "none",
            Hyphens::Auto => "auto",
        }
        .into(),
        "orphans" => style.orphans.to_string(),
        "widows" => style.widows.to_string(),
        "page" => style.page.clone().unwrap_or_else(|| "auto".into()),
        "border-collapse" => match style.border_collapse {
            BorderCollapse::Separate => "separate",
            BorderCollapse::Collapse => "collapse",
        }
        .into(),
        "list-style-type" => ListStyleType::keyword(style.list_style_type).into(),
        "content" => content(&style.content),
        "string-set" => string_set(&style.string_set),
        "counter-reset" => match style.counter_reset {
            None => "none".into(),
            Some(folio) => format!("page {folio}"),
        },
        "initial-letter" => style.initial_letter.to_string(),
        "position" => match style.position {
            Position::Static => "static",
            Position::Relative => "relative",
            Position::Absolute => "absolute",
        }
        .into(),
        "top" => inset(style.inset.top),
        "right" => inset(style.inset.right),
        "bottom" => inset(style.inset.bottom),
        "left" => inset(style.inset.left),
        "z-index" => style.z_index.to_string(),
        "wrap-flow" => match style.wrap_flow {
            WrapFlow::Auto => "auto",
            WrapFlow::Both => "both",
            WrapFlow::Start => "start",
            WrapFlow::End => "end",
        }
        .into(),
        "shape-outside" => match &style.shape_outside {
            ShapeOutside::None => "none".into(),
            ShapeOutside::Auto => "auto".into(),
            ShapeOutside::Polygon(points) => format!(
                "polygon({})",
                points
                    .iter()
                    .map(|point| format!("{} {}", coord(point.x), coord(point.y)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        },
        "shape-margin" => points(style.shape_margin),
        "width" => width(style.width),
        "height" => width(style.height),
        "min-height" => width(style.min_height),
        "margin" => sides(style.margin, |length| points(*length)),
        "margin-top" => points(style.margin.top),
        "margin-right" => points(style.margin.right),
        "margin-bottom" => points(style.margin.bottom),
        "margin-left" => points(style.margin.left),
        "padding" => sides(style.padding, |length| points(*length)),
        "padding-top" => points(style.padding.top),
        "padding-right" => points(style.padding.right),
        "padding-bottom" => points(style.padding.bottom),
        "padding-left" => points(style.padding.left),
        // The shorthand has one value for four edges, so edges that
        // differ have none.
        "border" => {
            let [top, right, bottom, left] = [border.top, border.right, border.bottom, border.left]
                .map(|edge| edge_of(style, edge));
            if top == right && top == bottom && top == left {
                top
            } else {
                String::new()
            }
        }
        "border-top" => edge_of(style, border.top),
        "border-right" => edge_of(style, border.right),
        "border-bottom" => edge_of(style, border.bottom),
        "border-left" => edge_of(style, border.left),
        "border-width" => sides(*border, |edge| points(edge.used())),
        "border-style" => sides(*border, |edge| line_style(edge.style).into()),
        "border-color" => sides(*border, |edge| edge.color.unwrap_or(style.color).to_hex()),
        "background-color" => background
            .color
            .map(|color| color.to_hex())
            .unwrap_or_else(|| "transparent".into()),
        "background-image" => background
            .image
            .as_ref()
            .map(|url| format!("url({})", quoted(&url.value)))
            .unwrap_or_else(|| "none".into()),
        "background-repeat" => match background.repeat {
            BackgroundRepeat::Repeat => "repeat",
            BackgroundRepeat::NoRepeat => "no-repeat",
        }
        .into(),
        "background-size" => match background.size {
            BackgroundSize::Auto => "auto".into(),
            BackgroundSize::Cover => "cover".into(),
            BackgroundSize::Contain => "contain".into(),
            BackgroundSize::Fixed { width, height } => {
                let axis = |axis: Option<Coord>| axis.map(coord).unwrap_or_else(|| "auto".into());
                format!("{} {}", axis(width), axis(height))
            }
        },
        "background-position" => format!(
            "{} {}",
            coord(background.position.x),
            coord(background.position.y)
        ),
        "box-decoration-break" => match style.box_decoration_break {
            BoxDecorationBreak::Slice => "slice",
            BoxDecorationBreak::Clone => "clone",
        }
        .into(),
        "break-before" => fragmentation(style.break_before).into(),
        "break-after" => fragmentation(style.break_after).into(),
        "break-inside" => fragmentation(style.break_inside).into(),
        "column-span" => match style.column_span {
            ColumnSpan::None => "none",
            ColumnSpan::All => "all",
        }
        .into(),
        _ => String::new(),
    }
}

/// What `content` generates, as CSS writes it.
pub(super) fn content(content: &Content) -> String {
    match content {
        Content::None => "none".into(),
        Content::Counter(style) => format!("counter(page{})", counter_style(*style)),
        Content::String(name) => format!("string({name})"),
        Content::Text(text) => quoted(text),
        Content::Pieces(pieces) => pieces
            .iter()
            .map(|piece| match piece {
                ContentPiece::Text(text) => quoted(text),
                ContentPiece::TargetCounter { target: at, style } => {
                    format!(
                        "target-counter({}, page{})",
                        target(at),
                        counter_style(*style)
                    )
                }
                ContentPiece::TargetText { target: at } => format!("target-text({})", target(at)),
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

/// A number with no more than three places, which is as fine as a
/// point needs.
fn number(value: f32) -> String {
    let rounded = (value * 1000.0).round() / 1000.0 + 0.0;
    format!("{rounded}")
}

fn points(value: f32) -> String {
    format!("{}pt", number(value))
}

fn coord(value: Coord) -> String {
    match value {
        Coord::Points(value) => points(value),
        Coord::Percent(value) => format!("{}%", number(value)),
    }
}

fn inset(value: Inset) -> String {
    match value {
        Inset::Auto => "auto".into(),
        Inset::Points(value) => points(value),
        Inset::Percent(value) => format!("{}%", number(value)),
    }
}

fn width(value: Width) -> String {
    match value {
        Width::Auto => "auto".into(),
        Width::Points(value) => points(value),
        Width::Percent(value) => format!("{}%", number(value)),
    }
}

/// Four edges as the shortest of the shorthand's forms: one value
/// where they agree, all four where they do not.
fn sides<T: Copy>(edges: Edges<T>, write: impl Fn(&T) -> String) -> String {
    let [top, right, bottom, left] =
        [edges.top, edges.right, edges.bottom, edges.left].map(|edge| write(&edge));
    if top == right && top == bottom && top == left {
        top
    } else {
        format!("{top} {right} {bottom} {left}")
    }
}

fn edge_of(style: &ComputedStyle, edge: Border) -> String {
    format!(
        "{} {} {}",
        points(edge.used()),
        line_style(edge.style),
        edge.color.unwrap_or(style.color).to_hex()
    )
}

fn line_style(style: BorderStyle) -> &'static str {
    match style {
        BorderStyle::None => "none",
        BorderStyle::Solid => "solid",
    }
}

fn family(family: &Family) -> String {
    match family {
        Family::Named(name) if name.contains(' ') => quoted(name),
        Family::Named(name) => name.clone(),
        Family::Generic(generic) => generic.keyword().into(),
    }
}

fn hanging(hanging: HangingPunctuation) -> String {
    let words: Vec<&str> = [
        hanging.first.then_some("first"),
        match hanging.end {
            HangEnd::None => None,
            HangEnd::Allow => Some("allow-end"),
            HangEnd::Force => Some("force-end"),
        },
        hanging.last.then_some("last"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if words.is_empty() {
        "none".into()
    } else {
        words.join(" ")
    }
}

fn string_set(sets: &[StringSet]) -> String {
    if sets.is_empty() {
        return "none".into();
    }
    sets.iter()
        .map(|set| {
            let pieces: Vec<String> = set
                .value
                .iter()
                .map(|piece| match piece {
                    StringPiece::Content => "content()".into(),
                    StringPiece::Text(text) => quoted(text),
                })
                .collect();
            format!("{} {}", set.name, pieces.join(" "))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn fragmentation(value: Break) -> &'static str {
    match value {
        Break::Auto => "auto",
        Break::Avoid => "avoid",
        Break::Column => "column",
        Break::Page => "page",
        Break::Side(Side::Recto) => "recto",
        Break::Side(Side::Verso) => "verso",
    }
}

/// `, <style>` after a counter, or nothing for the default.
fn counter_style(style: CounterStyle) -> String {
    match style {
        CounterStyle::Decimal => String::new(),
        style => format!(", {}", style.keyword()),
    }
}

fn target(target: &Target) -> String {
    match target {
        Target::Href => "attr(href url)".into(),
        Target::Url(url) => quoted(url),
    }
}

fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}
