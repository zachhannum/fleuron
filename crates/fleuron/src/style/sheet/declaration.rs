//! What a declaration of the novel subset is, and the table that
//! says which ones there are.

use cssparser::{
    AtRuleParser, CowRcStr, DeclarationParser, ParseError, Parser, ParserState,
    QualifiedRuleParser, RuleBodyItemParser, RuleBodyParser,
};

use crate::Warning;
use crate::style::properties::{BorderStyle, Declaration, Edge, Length, MEDIUM};

use super::color::{background_color, border_color, color};
use super::value::{
    break_value, count, counter_reset, decoration_break, edges, families, font_style, hanging,
    hyphens, inset, keyword_or, length, letter_spacing, line_height, line_style, line_width,
    ornament, page_name, positioning, property, shape_outside, string_set, text_align,
    text_justify, text_transform, variant_caps, weight, wrap_flow,
};
use super::vocabulary::FIRST_LINE_PROPERTIES;
use super::{Importance, StyleError, warning};

/// Every declaration in one style-rule body, plus a warning for each
/// one that fell outside the subset. `first_line` narrows the subset
/// to what `::first-line` takes.
pub(super) fn declarations(
    input: &mut Parser<'_, '_>,
    sheet: &str,
    first_line: bool,
) -> (Vec<(Declaration, Importance)>, Vec<Warning>) {
    let mut properties = Properties { first_line };
    let mut kept = Vec::new();
    let mut warnings = Vec::new();
    for result in RuleBodyParser::new(input, &mut properties) {
        match result {
            Ok((declarations, importance)) => {
                kept.extend(declarations.into_iter().map(|d| (d, importance)))
            }
            Err((error, _)) => warnings.push(warning(sheet, &error)),
        }
    }
    (kept, warnings)
}

/// Pins a declaration's diagnostic to where the declaration began.
/// A reader looks for the property name, not for the token the parser
/// gave up on.
pub(super) fn at<'i: 't, 't, T>(
    start: &ParserState,
    parse: impl FnOnce(&mut Parser<'i, 't>) -> Result<T, ParseError<'i, StyleError<'i>>>,
) -> impl FnOnce(&mut Parser<'i, 't>) -> Result<T, ParseError<'i, StyleError<'i>>> {
    let location = start.source_location();
    move |input| {
        parse(input).map_err(|error| ParseError {
            kind: error.kind,
            location,
        })
    }
}

/// The declaration parser for style rules.
struct Properties {
    /// Whether the rule this body belongs to selects `::first-line`.
    first_line: bool,
}

/// What one declaration expands to: a shorthand is several longhands.
type Longhands = (Vec<Declaration>, Importance);

impl<'i> DeclarationParser<'i> for Properties {
    type Declaration = Longhands;
    type Error = StyleError<'i>;

    fn parse_value<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
        start: &ParserState,
    ) -> Result<Self::Declaration, ParseError<'i, Self::Error>> {
        at(start, |input| {
            if self.first_line && !FIRST_LINE_PROPERTIES.contains(&&*name.to_ascii_lowercase()) {
                return Err(input.new_custom_error(StyleError::NotOnFirstLine(name.clone())));
            }
            let declarations = property(&name, input)?;
            let importance = if input.try_parse(cssparser::parse_important).is_ok() {
                Importance::Important
            } else {
                Importance::Normal
            };
            input.expect_exhausted()?;
            Ok((declarations, importance))
        })(input)
    }
}

impl<'i> AtRuleParser<'i> for Properties {
    type Prelude = ();
    type AtRule = Longhands;
    type Error = StyleError<'i>;
}

impl<'i> QualifiedRuleParser<'i> for Properties {
    type Prelude = ();
    type QualifiedRule = Longhands;
    type Error = StyleError<'i>;
}

impl<'i> RuleBodyItemParser<'i, Longhands, StyleError<'i>> for Properties {
    fn parse_declarations(&self) -> bool {
        true
    }

    fn parse_qualified(&self) -> bool {
        false
    }
}

/// One property the parser reads: the name it matches, what the
/// value may be, and the reader that turns the value into
/// declarations. The tables of these are the whole of what the
/// engine claims to understand, and the description in `subset` is
/// read off them.
///
/// `syntax` is CSS value-definition syntax. A bare word in it is a
/// keyword, `<name>` a type, `name()` a function. `examples` are
/// values that parse, one for each form the syntax allows.
pub(crate) struct Spec<D> {
    pub name: &'static str,
    /// Whether a child starts from the parent's value.
    pub inherited: bool,
    pub syntax: &'static str,
    pub examples: &'static [&'static str],
    pub(super) read: Reader<D>,
}

/// Reads one declaration's value, from after the colon to the end of
/// the declaration.
type Reader<D> = for<'i, 't> fn(
    &CowRcStr<'i>,
    &mut Parser<'i, 't>,
) -> Result<Vec<D>, ParseError<'i, StyleError<'i>>>;

impl<D> Spec<D> {
    /// The spec for `name`, whatever case it was written in.
    pub(crate) fn find<'a>(specs: &'a [Spec<D>], name: &str) -> Option<&'a Spec<D>> {
        specs
            .iter()
            .find(|spec| spec.name.eq_ignore_ascii_case(name))
    }

    /// Reads a declaration of this property.
    pub(crate) fn read<'i>(
        &self,
        name: &CowRcStr<'i>,
        input: &mut Parser<'i, '_>,
    ) -> Result<Vec<D>, ParseError<'i, StyleError<'i>>> {
        (self.read)(name, input)
    }
}

/// Reads one longhand: `parse` for the value, `wrap` for the
/// declaration it becomes.
pub(super) fn longhand<'i, T, D>(
    name: &CowRcStr<'i>,
    input: &mut Parser<'i, '_>,
    parse: fn(&mut Parser<'i, '_>) -> Option<T>,
    wrap: fn(T) -> D,
) -> Result<Vec<D>, ParseError<'i, StyleError<'i>>> {
    let value = keyword_or(input, parse, || StyleError::UnsupportedValue(name.clone()))?;
    Ok(vec![wrap(value)])
}

/// Reads a one-to-four value shorthand (`margin`, `padding`,
/// `border-width`) into its four longhands.
pub(super) fn sides<'i, T: Copy, D>(
    name: &CowRcStr<'i>,
    input: &mut Parser<'i, '_>,
    parse: fn(&mut Parser<'_, '_>) -> Option<T>,
    wrap: fn(Edge, T) -> D,
) -> Result<Vec<D>, ParseError<'i, StyleError<'i>>> {
    let edges = edges(input, parse)
        .ok_or_else(|| input.new_custom_error(StyleError::UnsupportedValue(name.clone())))?;
    Ok(edges
        .into_iter()
        .map(|(edge, value)| wrap(edge, value))
        .collect())
}

/// Reads `border` or one of its per-edge longhands: a width, a style
/// and a colour in any order, any of them left out. What is left out
/// goes back to its initial value, as the shorthand asks.
fn border<'i>(
    name: &CowRcStr<'i>,
    input: &mut Parser<'i, '_>,
    edges: &[Edge],
) -> Result<Vec<Declaration>, ParseError<'i, StyleError<'i>>> {
    let (mut width, mut style, mut color) = (None, None, None);
    loop {
        if width.is_none()
            && let Some(value) = input.try_parse(|input| line_width(input).ok_or(())).ok()
        {
            width = Some(value);
        } else if style.is_none()
            && let Some(value) = input.try_parse(|input| line_style(input).ok_or(())).ok()
        {
            style = Some(value);
        } else if color.is_none()
            && let Some(value) = input.try_parse(|input| self::color(input).ok_or(())).ok()
        {
            color = Some(value);
        } else {
            break;
        }
    }
    if width.is_none() && style.is_none() && color.is_none() {
        return Err(input.new_custom_error(StyleError::UnsupportedValue(name.clone())));
    }
    let width = width.unwrap_or(Length::Points(MEDIUM));
    let style = style.unwrap_or(BorderStyle::None);
    Ok(edges
        .iter()
        .flat_map(|edge| {
            [
                Declaration::BorderWidth(*edge, width),
                Declaration::BorderStyle(*edge, style),
                Declaration::BorderColor(*edge, color),
            ]
        })
        .collect())
}

/// Every edge, for the shorthands that set all four.
const ALL_EDGES: &[Edge] = &[Edge::Top, Edge::Right, Edge::Bottom, Edge::Left];

/// The properties of a style rule.
pub(crate) const PROPERTIES: &[Spec<Declaration>] = &[
    Spec {
        name: "font-family",
        inherited: true,
        syntax: "[ <family-name> | serif | sans-serif | monospace ]#",
        examples: &["\"Author Serif\", serif", "Times New Roman"],
        read: |_, input| Ok(vec![Declaration::FontFamily(families(input)?)]),
    },
    Spec {
        name: "font-size",
        inherited: true,
        syntax: "<length> | <percentage>",
        examples: &["11pt", "1.5em", "120%"],
        read: |name, input| longhand(name, input, length, Declaration::FontSize),
    },
    Spec {
        name: "font-style",
        inherited: true,
        syntax: "normal | italic | oblique",
        examples: &["italic"],
        read: |name, input| longhand(name, input, font_style, Declaration::FontStyle),
    },
    Spec {
        name: "font-weight",
        inherited: true,
        syntax: "normal | bold | <number [1,1000]>",
        examples: &["bold", "600"],
        read: |name, input| longhand(name, input, weight, Declaration::FontWeight),
    },
    Spec {
        name: "color",
        inherited: true,
        syntax: "<color>",
        examples: &[
            "darkslategray",
            "#369",
            "#336699",
            "rgb(51, 102, 153)",
            "rgb(20% 40% 60%)",
        ],
        read: |name, input| longhand(name, input, color, Declaration::Color),
    },
    Spec {
        name: "line-height",
        inherited: true,
        syntax: "normal | <number> | <length> | <percentage>",
        examples: &["normal", "1.4", "14pt", "140%"],
        read: |name, input| longhand(name, input, line_height, Declaration::LineHeight),
    },
    Spec {
        name: "letter-spacing",
        inherited: true,
        syntax: "normal | <length>",
        examples: &["normal", "0.05em"],
        read: |name, input| longhand(name, input, letter_spacing, Declaration::LetterSpacing),
    },
    Spec {
        name: "font-variant-caps",
        inherited: true,
        syntax: "normal | small-caps",
        examples: &["small-caps"],
        read: |name, input| longhand(name, input, variant_caps, Declaration::FontVariantCaps),
    },
    Spec {
        name: "text-transform",
        inherited: true,
        syntax: "none | uppercase | lowercase | capitalize",
        examples: &["uppercase"],
        read: |name, input| longhand(name, input, text_transform, Declaration::TextTransform),
    },
    Spec {
        name: "text-align",
        inherited: true,
        syntax: "left | right | center | justify | start | end",
        examples: &["justify"],
        read: |name, input| longhand(name, input, text_align, Declaration::TextAlign),
    },
    Spec {
        name: "text-justify",
        inherited: true,
        syntax: "auto | inter-word | inter-character | distribute",
        examples: &["inter-character"],
        read: |name, input| longhand(name, input, text_justify, Declaration::TextJustify),
    },
    Spec {
        name: "text-indent",
        inherited: true,
        syntax: "<length> | <percentage>",
        examples: &["1.2em", "5%"],
        read: |name, input| longhand(name, input, length, Declaration::TextIndent),
    },
    Spec {
        name: "hanging-punctuation",
        inherited: true,
        syntax: "none | [ first || [ force-end | allow-end ] || last ]",
        examples: &["none", "first", "first allow-end last", "force-end"],
        read: |name, input| longhand(name, input, hanging, Declaration::HangingPunctuation),
    },
    Spec {
        name: "hyphens",
        inherited: true,
        syntax: "none | manual | auto",
        examples: &["auto"],
        read: |name, input| longhand(name, input, hyphens, Declaration::Hyphens),
    },
    Spec {
        name: "orphans",
        inherited: true,
        syntax: "<integer>",
        examples: &["3"],
        read: |name, input| longhand(name, input, count, Declaration::Orphans),
    },
    Spec {
        name: "widows",
        inherited: true,
        syntax: "<integer>",
        examples: &["3"],
        read: |name, input| longhand(name, input, count, Declaration::Widows),
    },
    Spec {
        name: "page",
        inherited: true,
        syntax: "auto | <name>",
        examples: &["auto", "chapter"],
        read: |name, input| longhand(name, input, page_name, Declaration::Page),
    },
    Spec {
        name: "content",
        inherited: false,
        syntax: "none | <string>",
        examples: &["none", "\"\\2766\""],
        read: |name, input| longhand(name, input, ornament, Declaration::Content),
    },
    Spec {
        name: "string-set",
        inherited: false,
        syntax: "none | [ <name> [ content() | <string> ]+ ]#",
        examples: &[
            "none",
            "chapter content()",
            "part \"Part \" content(), chapter content()",
        ],
        read: |name, input| longhand(name, input, string_set, Declaration::StringSet),
    },
    Spec {
        name: "counter-reset",
        inherited: false,
        syntax: "none | page <integer>?",
        examples: &["none", "page", "page 1"],
        read: |name, input| longhand(name, input, counter_reset, Declaration::CounterReset),
    },
    Spec {
        name: "initial-letter",
        inherited: false,
        syntax: "<integer>",
        examples: &["3"],
        read: |name, input| longhand(name, input, count, Declaration::InitialLetter),
    },
    Spec {
        name: "position",
        inherited: false,
        syntax: "static | absolute",
        examples: &["absolute"],
        read: |name, input| longhand(name, input, positioning, Declaration::Position),
    },
    Spec {
        name: "top",
        inherited: false,
        syntax: "auto | <length>",
        examples: &["auto", "0", "54pt"],
        read: |name, input| {
            longhand(name, input, inset, |inset| {
                Declaration::Inset(Edge::Top, inset)
            })
        },
    },
    Spec {
        name: "right",
        inherited: false,
        syntax: "auto | <length>",
        examples: &["auto", "0", "54pt"],
        read: |name, input| {
            longhand(name, input, inset, |inset| {
                Declaration::Inset(Edge::Right, inset)
            })
        },
    },
    Spec {
        name: "bottom",
        inherited: false,
        syntax: "auto | <length>",
        examples: &["auto", "0", "54pt"],
        read: |name, input| {
            longhand(name, input, inset, |inset| {
                Declaration::Inset(Edge::Bottom, inset)
            })
        },
    },
    Spec {
        name: "left",
        inherited: false,
        syntax: "auto | <length>",
        examples: &["auto", "0", "54pt"],
        read: |name, input| {
            longhand(name, input, inset, |inset| {
                Declaration::Inset(Edge::Left, inset)
            })
        },
    },
    Spec {
        name: "wrap-flow",
        inherited: false,
        syntax: "auto | both | start | end",
        examples: &["end"],
        read: |name, input| longhand(name, input, wrap_flow, Declaration::WrapFlow),
    },
    Spec {
        name: "shape-outside",
        inherited: false,
        syntax: "none | auto | polygon( [ <length> | <percentage> ]{2} [ , [ <length> | <percentage> ]{2} ]* )",
        examples: &["auto", "polygon(0 0, 100% 0, 100% 100%)"],
        read: |name, input| longhand(name, input, shape_outside, Declaration::ShapeOutside),
    },
    Spec {
        name: "shape-margin",
        inherited: false,
        syntax: "<length> | <percentage>",
        examples: &["6pt"],
        read: |name, input| longhand(name, input, length, Declaration::ShapeMargin),
    },
    Spec {
        name: "margin",
        inherited: false,
        syntax: "[ <length> | <percentage> ]{1,4}",
        examples: &["1em", "1em 2em", "1em 2em 0", "54pt 42pt 54pt 54pt"],
        read: |name, input| sides(name, input, length, Declaration::Margin),
    },
    Spec {
        name: "margin-top",
        inherited: false,
        syntax: "<length> | <percentage>",
        examples: &["1em"],
        read: |name, input| {
            longhand(name, input, length, |length| {
                Declaration::Margin(Edge::Top, length)
            })
        },
    },
    Spec {
        name: "margin-right",
        inherited: false,
        syntax: "<length> | <percentage>",
        examples: &["1em"],
        read: |name, input| {
            longhand(name, input, length, |length| {
                Declaration::Margin(Edge::Right, length)
            })
        },
    },
    Spec {
        name: "margin-bottom",
        inherited: false,
        syntax: "<length> | <percentage>",
        examples: &["1em"],
        read: |name, input| {
            longhand(name, input, length, |length| {
                Declaration::Margin(Edge::Bottom, length)
            })
        },
    },
    Spec {
        name: "margin-left",
        inherited: false,
        syntax: "<length> | <percentage>",
        examples: &["1em"],
        read: |name, input| {
            longhand(name, input, length, |length| {
                Declaration::Margin(Edge::Left, length)
            })
        },
    },
    Spec {
        name: "padding",
        inherited: false,
        syntax: "[ <length> | <percentage> ]{1,4}",
        examples: &["12pt", "6pt 12pt", "6pt 12pt 0", "6pt 12pt 6pt 12pt"],
        read: |name, input| sides(name, input, length, Declaration::Padding),
    },
    Spec {
        name: "padding-top",
        inherited: false,
        syntax: "<length> | <percentage>",
        examples: &["6pt"],
        read: |name, input| {
            longhand(name, input, length, |length| {
                Declaration::Padding(Edge::Top, length)
            })
        },
    },
    Spec {
        name: "padding-right",
        inherited: false,
        syntax: "<length> | <percentage>",
        examples: &["6pt"],
        read: |name, input| {
            longhand(name, input, length, |length| {
                Declaration::Padding(Edge::Right, length)
            })
        },
    },
    Spec {
        name: "padding-bottom",
        inherited: false,
        syntax: "<length> | <percentage>",
        examples: &["6pt"],
        read: |name, input| {
            longhand(name, input, length, |length| {
                Declaration::Padding(Edge::Bottom, length)
            })
        },
    },
    Spec {
        name: "padding-left",
        inherited: false,
        syntax: "<length> | <percentage>",
        examples: &["6pt"],
        read: |name, input| {
            longhand(name, input, length, |length| {
                Declaration::Padding(Edge::Left, length)
            })
        },
    },
    Spec {
        name: "border",
        inherited: false,
        syntax: "[ <length> | thin | medium | thick ] || [ none | solid ] || <color>",
        examples: &["2pt solid", "thin solid crimson", "none"],
        read: |name, input| border(name, input, ALL_EDGES),
    },
    Spec {
        name: "border-top",
        inherited: false,
        syntax: "[ <length> | thin | medium | thick ] || [ none | solid ] || <color>",
        examples: &["2pt solid", "thin solid crimson", "none"],
        read: |name, input| border(name, input, &[Edge::Top]),
    },
    Spec {
        name: "border-right",
        inherited: false,
        syntax: "[ <length> | thin | medium | thick ] || [ none | solid ] || <color>",
        examples: &["2pt solid", "thin solid crimson", "none"],
        read: |name, input| border(name, input, &[Edge::Right]),
    },
    Spec {
        name: "border-bottom",
        inherited: false,
        syntax: "[ <length> | thin | medium | thick ] || [ none | solid ] || <color>",
        examples: &["2pt solid", "thin solid crimson", "none"],
        read: |name, input| border(name, input, &[Edge::Bottom]),
    },
    Spec {
        name: "border-left",
        inherited: false,
        syntax: "[ <length> | thin | medium | thick ] || [ none | solid ] || <color>",
        examples: &["2pt solid", "thin solid crimson", "none"],
        read: |name, input| border(name, input, &[Edge::Left]),
    },
    Spec {
        name: "border-width",
        inherited: false,
        syntax: "[ <length> | thin | medium | thick ]{1,4}",
        examples: &["2pt", "thin thick", "1pt 2pt 1pt 2pt"],
        read: |name, input| sides(name, input, line_width, Declaration::BorderWidth),
    },
    Spec {
        name: "border-style",
        inherited: false,
        syntax: "[ none | solid ]{1,4}",
        examples: &["solid", "none solid"],
        read: |name, input| sides(name, input, line_style, Declaration::BorderStyle),
    },
    Spec {
        name: "border-color",
        inherited: false,
        syntax: "<color>{1,4}",
        examples: &["crimson", "#369 black"],
        read: |name, input| sides(name, input, border_color, Declaration::BorderColor),
    },
    Spec {
        name: "background-color",
        inherited: false,
        syntax: "<color> | transparent",
        examples: &["#f4f1ea", "transparent"],
        read: |name, input| longhand(name, input, background_color, Declaration::BackgroundColor),
    },
    Spec {
        name: "box-decoration-break",
        inherited: false,
        syntax: "slice | clone",
        examples: &["slice", "clone"],
        read: |name, input| {
            longhand(
                name,
                input,
                decoration_break,
                Declaration::BoxDecorationBreak,
            )
        },
    },
    Spec {
        name: "break-before",
        inherited: false,
        syntax: "auto | avoid | avoid-page | avoid-column | column | page | always | left | right | recto | verso",
        examples: &["recto"],
        read: |name, input| longhand(name, input, break_value, Declaration::BreakBefore),
    },
    Spec {
        name: "break-after",
        inherited: false,
        syntax: "auto | avoid | avoid-page | avoid-column | column | page | always | left | right | recto | verso",
        examples: &["avoid"],
        read: |name, input| longhand(name, input, break_value, Declaration::BreakAfter),
    },
    Spec {
        name: "break-inside",
        inherited: false,
        syntax: "auto | avoid | avoid-page | avoid-column | column | page | always | left | right | recto | verso",
        examples: &["avoid"],
        read: |name, input| longhand(name, input, break_value, Declaration::BreakInside),
    },
];
