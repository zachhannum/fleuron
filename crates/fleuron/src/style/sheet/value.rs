//! One property value at a time: a keyword, a length, a list, and
//! what each of them comes to.

use cssparser::{CowRcStr, ParseError, Parser, Token, match_ignore_ascii_case};

use crate::fonts::GenericFamily;
use crate::lines::{HangEnd, HangingPunctuation};
use crate::pages::Side;
use crate::style::properties::{
    BorderCollapse, BorderStyle, BoxDecorationBreak, Break, ColumnSpan, Content, ContentPiece,
    CounterStyle, Declaration, Edge, Family, FontStyle, FontVariantCaps, Hyphens, LINE_WIDTHS,
    Length, LineHeight, Position, ShapeSource, StringPiece, StringSet, Target, TextAlign,
    TextJustify, TextTransform, WrapFlow,
};

use super::StyleError;
use super::declaration::{PROPERTIES, Spec};

/// One declaration of the novel subset, expanded to longhands.
pub(super) fn property<'i>(
    name: &CowRcStr<'i>,
    input: &mut Parser<'i, '_>,
) -> Result<Vec<Declaration>, ParseError<'i, StyleError<'i>>> {
    match Spec::find(PROPERTIES, name) {
        Some(spec) => spec.read(name, input),
        None => Err(input.new_custom_error(StyleError::UnsupportedProperty(name.clone()))),
    }
}

/// Runs a value parser, turning "this is not a value I know" into the
/// diagnostic that names the property it was written against.
pub(super) fn keyword_or<'i, T>(
    input: &mut Parser<'i, '_>,
    parse: fn(&mut Parser<'i, '_>) -> Option<T>,
    bad: impl Fn() -> StyleError<'i>,
) -> Result<T, ParseError<'i, StyleError<'i>>> {
    match input.try_parse(|input| parse(input).ok_or(())) {
        Ok(value) => Ok(value),
        Err(()) => Err(input.new_custom_error(bad())),
    }
}

pub(super) fn font_style(input: &mut Parser<'_, '_>) -> Option<FontStyle> {
    let keyword = input.expect_ident().ok()?.clone();
    match_ignore_ascii_case! { &keyword,
        "normal" => Some(FontStyle::Normal),
        "italic" | "oblique" => Some(FontStyle::Italic),
        _ => None,
    }
}

/// `letter-spacing: normal | <length>`. `normal` is no tracking at
/// all, which is what a length of zero already says.
pub(super) fn letter_spacing(input: &mut Parser<'_, '_>) -> Option<Length> {
    if input
        .try_parse(|input| input.expect_ident_matching("normal"))
        .is_ok()
    {
        return Some(Length::Points(0.0));
    }
    length(input)
}

/// `<line-width>`: a length, or one of the three keywords a browser
/// gives a pixel width to.
pub(super) fn line_width(input: &mut Parser<'_, '_>) -> Option<Length> {
    if let Ok(keyword) = input.try_parse(|input| input.expect_ident().cloned()) {
        return LINE_WIDTHS
            .iter()
            .find(|(name, _)| keyword.eq_ignore_ascii_case(name))
            .map(|(_, points)| Length::Points(*points));
    }
    length(input)
}

/// `<line-style>`, of which the engine draws one.
pub(super) fn line_style(input: &mut Parser<'_, '_>) -> Option<BorderStyle> {
    let keyword = input.expect_ident().ok()?.clone();
    match_ignore_ascii_case! { &keyword,
        "none" => Some(BorderStyle::None),
        "solid" => Some(BorderStyle::Solid),
        _ => None,
    }
}

/// `column-count`: how many columns, or `auto` for as many as
/// `column-width` allows.
pub(super) fn column_count(input: &mut Parser<'_, '_>) -> Option<Option<u32>> {
    if input
        .try_parse(|input| input.expect_ident_matching("auto"))
        .is_ok()
    {
        return Some(None);
    }
    let count = input.expect_integer().ok()?;
    (count >= 1).then_some(Some(count as u32))
}

/// `column-width`: the width a column is asked to have, or `auto`
/// for whatever `column-count` divides the box into.
pub(super) fn column_width(input: &mut Parser<'_, '_>) -> Option<Option<Length>> {
    auto_or(input, length)
}

/// `column-gap`: the gutter, or `normal`, which is one em of the
/// book's own size.
pub(super) fn column_gap(input: &mut Parser<'_, '_>) -> Option<Option<Length>> {
    if input
        .try_parse(|input| input.expect_ident_matching("normal"))
        .is_ok()
    {
        return Some(None);
    }
    length(input).map(Some)
}

/// A value that may be written `auto`.
fn auto_or<T>(
    input: &mut Parser<'_, '_>,
    parse: fn(&mut Parser<'_, '_>) -> Option<T>,
) -> Option<Option<T>> {
    if input
        .try_parse(|input| input.expect_ident_matching("auto"))
        .is_ok()
    {
        return Some(None);
    }
    parse(input).map(Some)
}

/// `width`: `auto`, or a length or a percentage.
pub(super) fn width(input: &mut Parser<'_, '_>) -> Option<Option<Length>> {
    auto_or(input, length)
}

pub(super) fn border_collapse(input: &mut Parser<'_, '_>) -> Option<BorderCollapse> {
    let keyword = input.expect_ident().ok()?.clone();
    match_ignore_ascii_case! { &keyword,
        "separate" => Some(BorderCollapse::Separate),
        "collapse" => Some(BorderCollapse::Collapse),
        _ => None,
    }
}

pub(super) fn decoration_break(input: &mut Parser<'_, '_>) -> Option<BoxDecorationBreak> {
    let keyword = input.expect_ident().ok()?.clone();
    match_ignore_ascii_case! { &keyword,
        "slice" => Some(BoxDecorationBreak::Slice),
        "clone" => Some(BoxDecorationBreak::Clone),
        _ => None,
    }
}

pub(super) fn column_span(input: &mut Parser<'_, '_>) -> Option<ColumnSpan> {
    let keyword = input.expect_ident().ok()?.clone();
    match_ignore_ascii_case! { &keyword,
        "none" => Some(ColumnSpan::None),
        "all" => Some(ColumnSpan::All),
        _ => None,
    }
}

pub(super) fn variant_caps(input: &mut Parser<'_, '_>) -> Option<FontVariantCaps> {
    let keyword = input.expect_ident().ok()?.clone();
    match_ignore_ascii_case! { &keyword,
        "normal" => Some(FontVariantCaps::Normal),
        "small-caps" => Some(FontVariantCaps::SmallCaps),
        _ => None,
    }
}

pub(super) fn text_transform(input: &mut Parser<'_, '_>) -> Option<TextTransform> {
    let keyword = input.expect_ident().ok()?.clone();
    match_ignore_ascii_case! { &keyword,
        "none" => Some(TextTransform::None),
        "uppercase" => Some(TextTransform::Uppercase),
        "lowercase" => Some(TextTransform::Lowercase),
        "capitalize" => Some(TextTransform::Capitalize),
        _ => None,
    }
}

pub(super) fn text_align(input: &mut Parser<'_, '_>) -> Option<TextAlign> {
    let keyword = input.expect_ident().ok()?.clone();
    match_ignore_ascii_case! { &keyword,
        "left" | "start" => Some(TextAlign::Left),
        "right" | "end" => Some(TextAlign::Right),
        "center" => Some(TextAlign::Center),
        "justify" => Some(TextAlign::Justify),
        _ => None,
    }
}

pub(super) fn text_justify(input: &mut Parser<'_, '_>) -> Option<TextJustify> {
    let keyword = input.expect_ident().ok()?.clone();
    match_ignore_ascii_case! { &keyword,
        "auto" | "inter-word" => Some(TextJustify::InterWord),
        "inter-character" | "distribute" => Some(TextJustify::InterCharacter),
        _ => None,
    }
}

/// `hanging-punctuation: none | [ first || [ force-end | allow-end ]
/// || last ]`. Written as a set, so the keywords are read until the
/// declaration runs out.
pub(super) fn hanging(input: &mut Parser<'_, '_>) -> Option<HangingPunctuation> {
    let mut hanging = HangingPunctuation::NONE;
    let mut seen = false;
    while let Ok(keyword) = input.try_parse(|input| input.expect_ident().cloned()) {
        seen = true;
        match_ignore_ascii_case! { &keyword,
            "none" if hanging == HangingPunctuation::NONE => {},
            "first" => hanging.first = true,
            "force-end" => hanging.end = HangEnd::Force,
            "allow-end" => hanging.end = HangEnd::Allow,
            "last" => hanging.last = true,
            _ => return None,
        }
    }
    seen.then_some(hanging)
}

pub(super) fn hyphens(input: &mut Parser<'_, '_>) -> Option<Hyphens> {
    let keyword = input.expect_ident().ok()?.clone();
    match_ignore_ascii_case! { &keyword,
        "none" | "manual" => Some(Hyphens::None),
        "auto" => Some(Hyphens::Auto),
        _ => None,
    }
}

/// `position: static | absolute`. An absolute box comes out of the
/// flow and sits against the page area.
pub(super) fn positioning(input: &mut Parser<'_, '_>) -> Option<Position> {
    let keyword = input.expect_ident().ok()?.clone();
    match_ignore_ascii_case! { &keyword,
        "static" => Some(Position::Static),
        "absolute" => Some(Position::Absolute),
        _ => None,
    }
}

/// One inset: `auto`, or a length from the page area's edge.
pub(super) fn inset(input: &mut Parser<'_, '_>) -> Option<Option<Length>> {
    if input
        .try_parse(|input| input.expect_ident_matching("auto"))
        .is_ok()
    {
        return Some(None);
    }
    length(input).map(Some)
}

/// `wrap-flow: auto | both | start | end`: which side of an exclusion
/// the prose sets on.
pub(super) fn wrap_flow(input: &mut Parser<'_, '_>) -> Option<WrapFlow> {
    let keyword = input.expect_ident().ok()?.clone();
    match_ignore_ascii_case! { &keyword,
        "auto" => Some(WrapFlow::Auto),
        "both" => Some(WrapFlow::Both),
        "start" => Some(WrapFlow::Start),
        "end" => Some(WrapFlow::End),
        _ => None,
    }
}

/// `shape-outside: none | auto | polygon(…)`: the contour the prose
/// sets around, in place of the box.
///
/// The points are read from the top left of the margin box, and the
/// polygon closes itself. Three of them are the fewest that enclose
/// anything.
pub(super) fn shape_outside(input: &mut Parser<'_, '_>) -> Option<ShapeSource> {
    if let Ok(keyword) = input.try_parse(|input| input.expect_ident().cloned()) {
        return match_ignore_ascii_case! { &keyword,
            "none" => Some(ShapeSource::None),
            "auto" => Some(ShapeSource::Auto),
            _ => None,
        };
    }
    input
        .try_parse(|input| input.expect_function_matching("polygon"))
        .ok()?;
    let points = input
        .parse_nested_block(|input| {
            let mut points = Vec::new();
            loop {
                let x = length(input).ok_or_else(|| input.new_error_for_next_token::<()>())?;
                let y = length(input).ok_or_else(|| input.new_error_for_next_token::<()>())?;
                points.push((x, y));
                if input.try_parse(|input| input.expect_comma()).is_err() {
                    break;
                }
            }
            input.expect_exhausted()?;
            Ok(points)
        })
        .ok()?;
    (points.len() >= 3).then_some(ShapeSource::Polygon(points))
}

pub(super) fn page_name(input: &mut Parser<'_, '_>) -> Option<Option<String>> {
    let keyword = input.expect_ident().ok()?.clone();
    if keyword.eq_ignore_ascii_case("auto") {
        Some(None)
    } else {
        Some(Some(keyword.as_ref().to_string()))
    }
}

/// What `content` generates: nothing, or literals and references in
/// the order they are set. Literals written side by side are one
/// literal. The page counter belongs to margin boxes, not to prose.
pub(super) fn generated(input: &mut Parser<'_, '_>) -> Option<Content> {
    if input.try_parse(none_keyword).is_ok() {
        return Some(Content::None);
    }
    let mut pieces: Vec<ContentPiece> = Vec::new();
    loop {
        let piece = if let Ok(text) = input.try_parse(|input| input.expect_string().cloned()) {
            ContentPiece::Text(text.as_ref().to_string())
        } else if let Ok(piece) = input.try_parse(reference) {
            piece
        } else {
            break;
        };
        match (pieces.last_mut(), piece) {
            (Some(ContentPiece::Text(before)), ContentPiece::Text(text)) => before.push_str(&text),
            (_, piece) => pieces.push(piece),
        }
    }
    match pieces.as_slice() {
        [] => None,
        [ContentPiece::Text(text)] => Some(Content::Text(text.clone())),
        _ => Some(Content::Pieces(pieces)),
    }
}

/// `target-counter(<target>, page, <counter-style>?)` or
/// `target-text(<target>)`. `page` is the only counter there is.
fn reference<'i>(input: &mut Parser<'i, '_>) -> Result<ContentPiece, ParseError<'i, StyleError<'i>>> {
    let function = input.expect_function()?.clone();
    input.parse_nested_block(|input| {
        let target = target(input)?;
        let piece = if function.eq_ignore_ascii_case("target-text") {
            ContentPiece::TargetText { target }
        } else if function.eq_ignore_ascii_case("target-counter") {
            input.expect_comma()?;
            input.expect_ident_matching("page")?;
            let style = match input.try_parse(|input| input.expect_comma()) {
                Ok(()) => {
                    let keyword = input.expect_ident()?.clone();
                    CounterStyle::parse(&keyword).ok_or_else(|| {
                        input.new_custom_error(StyleError::UnsupportedValue(keyword))
                    })?
                }
                Err(_) => CounterStyle::Decimal,
            };
            ContentPiece::TargetCounter { target, style }
        } else {
            return Err(input.new_custom_error(StyleError::UnsupportedValue(function.clone())));
        };
        input.expect_exhausted()?;
        Ok(piece)
    })
}

/// The element a reference names: the link's own `href`, read with
/// `attr(href url)`, or a url written as a string.
fn target<'i>(input: &mut Parser<'i, '_>) -> Result<Target, ParseError<'i, StyleError<'i>>> {
    if let Ok(url) = input.try_parse(|input| input.expect_string().cloned()) {
        return Ok(Target::Url(url.as_ref().to_string()));
    }
    input.expect_function_matching("attr")?;
    input.parse_nested_block(|input| {
        input.expect_ident_matching("href")?;
        let _ = input.try_parse(|input| input.expect_ident_matching("url"));
        input.expect_exhausted()?;
        Ok(Target::Href)
    })
}

/// `string-set: none | <name> [content() | <string>]+ [, …]`. What a
/// running head picks up: the element's own text, literals, or both.
pub(super) fn string_set(input: &mut Parser<'_, '_>) -> Option<Vec<StringSet>> {
    if input.try_parse(none_keyword).is_ok() {
        return Some(Vec::new());
    }
    let mut sets = Vec::new();
    loop {
        let name = input.expect_ident().ok()?.as_ref().to_string();
        let mut value = Vec::new();
        loop {
            if let Ok(text) = input.try_parse(|input| input.expect_string().cloned()) {
                value.push(StringPiece::Text(text.as_ref().to_string()));
            } else if input.try_parse(content_function).is_ok() {
                value.push(StringPiece::Content);
            } else {
                break;
            }
        }
        if value.is_empty() {
            return None;
        }
        sets.push(StringSet { name, value });
        if input.try_parse(|input| input.expect_comma()).is_err() {
            return Some(sets);
        }
    }
}

/// `counter-reset: none | page <integer>?`. The page counter is the
/// only one there is, and the value is the folio the page this
/// element opens takes.
pub(super) fn counter_reset(input: &mut Parser<'_, '_>) -> Option<Option<u32>> {
    let keyword = input.expect_ident().ok()?.clone();
    if keyword.eq_ignore_ascii_case("none") {
        return Some(None);
    }
    if !keyword.eq_ignore_ascii_case("page") {
        return None;
    }
    let folio = input.try_parse(|input| input.expect_integer()).unwrap_or(1);
    Some(Some(folio.max(0) as u32))
}

/// `content()`, the element's own text, with no argument list the
/// engine has a second answer for.
fn content_function<'i>(input: &mut Parser<'i, '_>) -> Result<(), ParseError<'i, StyleError<'i>>> {
    let function = input.expect_function()?.clone();
    if !function.eq_ignore_ascii_case("content") {
        return Err(input.new_custom_error(StyleError::UnsupportedValue(function)));
    }
    input.parse_nested_block(|input| {
        input
            .expect_exhausted()
            .map_err(ParseError::<StyleError<'_>>::from)
    })
}

/// The `none` keyword, consumed only when that is what is there.
fn none_keyword<'i>(input: &mut Parser<'i, '_>) -> Result<(), ParseError<'i, StyleError<'i>>> {
    let keyword = input.expect_ident()?.clone();
    if keyword.eq_ignore_ascii_case("none") {
        Ok(())
    } else {
        Err(input.new_custom_error(StyleError::UnsupportedValue(keyword)))
    }
}

pub(super) fn count(input: &mut Parser<'_, '_>) -> Option<u16> {
    let value = input.expect_integer().ok()?;
    u16::try_from(value.max(0)).ok()
}

pub(super) fn weight(input: &mut Parser<'_, '_>) -> Option<u16> {
    if let Ok(number) = input.try_parse(|input| input.expect_number()) {
        return (1.0..=1000.0).contains(&number).then_some(number as u16);
    }
    let keyword = input.expect_ident().ok()?.clone();
    match_ignore_ascii_case! { &keyword,
        "normal" => Some(400),
        "bold" => Some(700),
        _ => None,
    }
}

pub(super) fn line_height(input: &mut Parser<'_, '_>) -> Option<LineHeight> {
    if let Ok(number) = input.try_parse(|input| input.expect_number()) {
        return Some(LineHeight::Number(number));
    }
    if input
        .try_parse(|input| input.expect_ident_matching("normal"))
        .is_ok()
    {
        return Some(LineHeight::Normal);
    }
    length(input).map(LineHeight::Length)
}

pub(super) fn break_value(input: &mut Parser<'_, '_>) -> Option<Break> {
    let keyword = input.expect_ident().ok()?.clone();
    match_ignore_ascii_case! { &keyword,
        "auto" => Some(Break::Auto),
        "avoid" | "avoid-page" | "avoid-column" => Some(Break::Avoid),
        "column" => Some(Break::Column),
        "page" | "always" => Some(Break::Page),
        "left" | "verso" => Some(Break::Side(Side::Verso)),
        "right" | "recto" => Some(Break::Side(Side::Recto)),
        _ => None,
    }
}

/// A one-to-four value shorthand, in the CSS order:
/// top/right/bottom/left filled in the usual way.
pub(super) fn edges<T: Copy>(
    input: &mut Parser<'_, '_>,
    parse: fn(&mut Parser<'_, '_>) -> Option<T>,
) -> Option<Vec<(Edge, T)>> {
    let mut values = Vec::new();
    while values.len() < 4 {
        match input.try_parse(|input| parse(input).ok_or(())) {
            Ok(value) => values.push(value),
            Err(()) => break,
        }
    }
    let (top, right, bottom, left) = match values[..] {
        [all] => (all, all, all, all),
        [block, inline] => (block, inline, block, inline),
        [top, inline, bottom] => (top, inline, bottom, inline),
        [top, right, bottom, left] => (top, right, bottom, left),
        _ => return None,
    };
    Some(vec![
        (Edge::Top, top),
        (Edge::Right, right),
        (Edge::Bottom, bottom),
        (Edge::Left, left),
    ])
}

/// A length in any unit the engine converts to points, or a
/// percentage of whatever the property is relative to.
pub(super) fn length(input: &mut Parser<'_, '_>) -> Option<Length> {
    match input.next().ok()? {
        Token::Number { value, .. } if *value == 0.0 => Some(Length::Points(0.0)),
        Token::Percentage { unit_value, .. } => Some(Length::Percent(unit_value * 100.0)),
        Token::Dimension { value, unit, .. } => {
            let value = *value;
            UNITS
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(unit))
                .map(|(_, convert)| convert(value))
        }
        _ => None,
    }
}

/// One unit's conversion into a `Length`.
type Convert = fn(f32) -> Length;

/// The units a `<length>` may carry, each with its conversion. The
/// absolute ones become points; the font-relative ones stay relative
/// until the cascade knows the font size.
pub(crate) const UNITS: &[(&str, Convert)] = &[
    ("pt", Length::Points),
    ("px", |value| Length::Points(value * 0.75)),
    ("pc", |value| Length::Points(value * 12.0)),
    ("in", |value| Length::Points(value * 72.0)),
    ("cm", |value| Length::Points(value * 72.0 / 2.54)),
    ("mm", |value| Length::Points(value * 72.0 / 25.4)),
    ("q", |value| Length::Points(value * 72.0 / 101.6)),
    ("em", Length::Em),
    ("rem", Length::Rem),
];

/// A `font-family` list: quoted names, bare names, generic keywords.
pub(super) fn families<'i>(
    input: &mut Parser<'i, '_>,
) -> Result<Vec<Family>, ParseError<'i, StyleError<'i>>> {
    input.parse_comma_separated(|input| {
        if let Ok(name) = input.try_parse(|input| input.expect_string().cloned()) {
            return Ok(Family::Named(name.as_ref().to_string()));
        }
        // A bare family name may be several identifiers: Times New Roman.
        let mut words = vec![input.expect_ident()?.as_ref().to_string()];
        while let Ok(word) = input.try_parse(|input| input.expect_ident().cloned()) {
            words.push(word.as_ref().to_string());
        }
        let name = words.join(" ");
        Ok(match GenericFamily::parse(&name) {
            Some(generic) => Family::Generic(generic),
            None => Family::Named(name),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cssparser::{Parser, ParserInput};

    /// One value through the `shape-outside` parser.
    fn shape(css: &str) -> Option<ShapeSource> {
        let mut input = ParserInput::new(css);
        let mut parser = Parser::new(&mut input);
        let value = shape_outside(&mut parser);
        value.filter(|_| parser.is_exhausted())
    }

    /// `shape-outside` reads its two keywords and a polygon of
    /// lengths and percentages. A polygon that encloses nothing, or
    /// that leaves a point half written, is no shape.
    #[test]
    fn shape_outside_reads_its_keywords_and_a_polygon() {
        assert_eq!(shape("none"), Some(ShapeSource::None));
        assert_eq!(shape("AUTO"), Some(ShapeSource::Auto));
        assert_eq!(
            shape("polygon(0 0, 100% 0, 1em 2em)"),
            Some(ShapeSource::Polygon(vec![
                (Length::Points(0.0), Length::Points(0.0)),
                (Length::Percent(100.0), Length::Points(0.0)),
                (Length::Em(1.0), Length::Em(2.0)),
            ])),
        );
        assert_eq!(
            shape("polygon(0 0, 100% 0)"),
            None,
            "two points enclose nothing"
        );
        assert_eq!(
            shape("polygon(0 0, 100% 0, 50%)"),
            None,
            "a point is a pair"
        );
        assert_eq!(shape("polygon()"), None);
        assert_eq!(shape("circle(4em)"), None);
        assert_eq!(shape("border-box"), None);
    }
}
