//! `@page`: its selectors, its own properties, and the margin
//! boxes inside it.

use cssparser::{
    AtRuleParser, CowRcStr, DeclarationParser, ParseError, Parser, ParserState,
    QualifiedRuleParser, RuleBodyItemParser, RuleBodyParser,
};

use crate::Warning;
use crate::pages::Side;
use crate::style::properties::{Content, CounterStyle, Edge, Length, MarginBox};

use super::color::background_color;
use super::declaration::{Spec, at, longhand, sides, written_at};
use super::value::{
    background_image, background_position, background_repeat, background_size, column_count,
    column_gap, column_width, length, line_style, line_width, property,
};
use super::{MarginDeclaration, PageDeclaration, PageRule, StyleError, warning};

/// `@page` prelude: an optional page name, then any of `:first`,
/// `:blank`, `:left`, `:right`.
pub(super) fn page_selector<'i>(
    input: &mut Parser<'i, '_>,
) -> Result<PageRule, ParseError<'i, StyleError<'i>>> {
    let mut rule = PageRule::default();
    if let Ok(name) = input.try_parse(|input| input.expect_ident().cloned()) {
        rule.name = Some(name.as_ref().to_string());
    }
    while !input.is_exhausted() {
        input.expect_colon()?;
        let pseudo = input.expect_ident()?.clone();
        let Some((_, narrow)) = PAGE_SELECTORS
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(&pseudo))
        else {
            return Err(input.new_custom_error(StyleError::UnsupportedPageSelector));
        };
        narrow(&mut rule);
    }
    Ok(rule)
}

/// The declarations of a `@page` body.
pub(crate) const PAGE_PROPERTIES: &[Spec<PageDeclaration>] = &[
    Spec {
        name: "size",
        inherited: false,
        syntax: "<length>{1,2} | <page-size> [ portrait | landscape ]?",
        examples: &[
            "432pt 648pt",
            "148mm",
            "a5",
            "letter landscape",
            "b5 portrait",
        ],
        read: |name, input| {
            longhand(name, input, size, |(width, height)| {
                PageDeclaration::Size(width, height)
            })
        },
    },
    Spec {
        name: "margin",
        inherited: false,
        syntax: "[ <length> | <percentage> ]{1,4}",
        examples: &["54pt", "54pt 42pt", "54pt 42pt 54pt 54pt"],
        read: |name, input| sides(name, input, length, PageDeclaration::Margin),
    },
    Spec {
        name: "margin-top",
        inherited: false,
        syntax: "<length> | <percentage>",
        examples: &["54pt"],
        read: |name, input| {
            longhand(name, input, length, |length| {
                PageDeclaration::Margin(Edge::Top, length)
            })
        },
    },
    Spec {
        name: "margin-right",
        inherited: false,
        syntax: "<length> | <percentage>",
        examples: &["42pt"],
        read: |name, input| {
            longhand(name, input, length, |length| {
                PageDeclaration::Margin(Edge::Right, length)
            })
        },
    },
    Spec {
        name: "margin-bottom",
        inherited: false,
        syntax: "<length> | <percentage>",
        examples: &["54pt"],
        read: |name, input| {
            longhand(name, input, length, |length| {
                PageDeclaration::Margin(Edge::Bottom, length)
            })
        },
    },
    Spec {
        name: "margin-left",
        inherited: false,
        syntax: "<length> | <percentage>",
        examples: &["54pt"],
        read: |name, input| {
            longhand(name, input, length, |length| {
                PageDeclaration::Margin(Edge::Left, length)
            })
        },
    },
    Spec {
        name: "background-color",
        inherited: false,
        syntax: "<color> | transparent",
        examples: &["#f4f1ea", "transparent"],
        read: |name, input| {
            longhand(
                name,
                input,
                background_color,
                PageDeclaration::BackgroundColor,
            )
        },
    },
    Spec {
        name: "background-image",
        inherited: false,
        syntax: "none | <url>",
        examples: &["none", "url(\"verso.webp\")", "url(art/plate.png)"],
        read: |name, input| {
            longhand(
                name,
                input,
                background_image,
                PageDeclaration::BackgroundImage,
            )
        },
    },
    Spec {
        name: "background-repeat",
        inherited: false,
        syntax: "repeat | no-repeat",
        examples: &["repeat", "no-repeat"],
        read: |name, input| {
            longhand(
                name,
                input,
                background_repeat,
                PageDeclaration::BackgroundRepeat,
            )
        },
    },
    Spec {
        name: "background-size",
        inherited: false,
        syntax: "auto | cover | contain | [ <length> | <percentage> | auto ]{1,2}",
        examples: &["auto", "cover", "contain", "120pt", "100% auto"],
        read: |name, input| {
            longhand(
                name,
                input,
                background_size,
                PageDeclaration::BackgroundSize,
            )
        },
    },
    Spec {
        name: "background-position",
        inherited: false,
        syntax: "[ left | center | right | <length> | <percentage> ] \
                 [ top | center | bottom | <length> | <percentage> ]?",
        examples: &["center", "right bottom", "50% 50%", "12pt 18pt", "top left"],
        read: |name, input| {
            longhand(name, input, background_position, |(x, y)| {
                PageDeclaration::BackgroundPosition(x, y)
            })
        },
    },
    Spec {
        name: "column-count",
        inherited: false,
        syntax: "auto | <integer>",
        examples: &["auto", "2"],
        read: |name, input| longhand(name, input, column_count, PageDeclaration::ColumnCount),
    },
    Spec {
        name: "column-width",
        inherited: false,
        syntax: "auto | <length>",
        examples: &["auto", "160pt"],
        read: |name, input| longhand(name, input, column_width, PageDeclaration::ColumnWidth),
    },
    Spec {
        name: "column-gap",
        inherited: false,
        syntax: "normal | <length>",
        examples: &["normal", "18pt"],
        read: |name, input| longhand(name, input, column_gap, PageDeclaration::ColumnGap),
    },
    Spec {
        name: "column-rule-width",
        inherited: false,
        syntax: "<length> | thin | medium | thick",
        examples: &["0.5pt", "thin", "medium", "thick"],
        read: |name, input| longhand(name, input, line_width, PageDeclaration::ColumnRuleWidth),
    },
    Spec {
        name: "column-rule-style",
        inherited: false,
        syntax: "none | solid",
        examples: &["none", "solid"],
        read: |name, input| longhand(name, input, line_style, PageDeclaration::ColumnRuleStyle),
    },
];

/// The declarations of a margin box that are its own. Every property
/// of a style rule parses in one too.
pub(crate) const MARGIN_BOX_PROPERTIES: &[Spec<MarginDeclaration>] = &[Spec {
    name: "content",
    inherited: false,
    syntax: "none | <string> | counter(page) | counter(page, <counter-style>) | string(<name>)",
    examples: &[
        "none",
        "\"Chapter\"",
        "counter(page)",
        "counter(page, lower-roman)",
        "string(chapter)",
    ],
    read: |name, input| longhand(name, input, content, MarginDeclaration::Content),
}];

/// What one page selector narrows a rule to.
type Narrow = fn(&mut PageRule);

/// The `@page` selectors, and what each one narrows the rule to.
pub(crate) const PAGE_SELECTORS: &[(&str, Narrow)] = &[
    ("first", |rule| rule.first = true),
    ("blank", |rule| rule.blank = true),
    ("left", |rule| rule.side = Some(Side::Verso)),
    ("right", |rule| rule.side = Some(Side::Recto)),
];

/// The body of one `@page` rule: page declarations and margin boxes.
pub(super) struct PageBody {
    pub(super) name: String,
    pub(super) warnings: Vec<Warning>,
}

/// One item of a `@page` body.
pub(super) enum PageItem {
    Declaration(PageDeclaration),
    Box(MarginBox, Vec<MarginDeclaration>),
}

impl<'i> DeclarationParser<'i> for PageBody {
    type Declaration = Vec<PageItem>;
    type Error = StyleError<'i>;

    fn parse_value<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
        start: &ParserState,
    ) -> Result<Self::Declaration, ParseError<'i, Self::Error>> {
        at(start, |input| {
            let Some(spec) = Spec::find(PAGE_PROPERTIES, &name) else {
                return Err(input.new_custom_error(StyleError::UnsupportedProperty(name.clone())));
            };
            let mut items = spec.read(&name, input)?;
            input.expect_exhausted()?;
            page_written_at(&mut items, &self.name, start);
            Ok(items.into_iter().map(PageItem::Declaration).collect())
        })(input)
    }
}

impl<'i> AtRuleParser<'i> for PageBody {
    type Prelude = MarginBox;
    type AtRule = Vec<PageItem>;
    type Error = StyleError<'i>;

    fn parse_prelude<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
        MarginBox::parse(&name)
            .ok_or_else(|| input.new_custom_error(StyleError::UnsupportedAtRule(name.clone())))
    }

    fn parse_block<'t>(
        &mut self,
        which: Self::Prelude,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::AtRule, ParseError<'i, Self::Error>> {
        let mut body = MarginBoxBody {
            sheet: self.name.clone(),
        };
        let collected: Vec<_> = RuleBodyParser::new(input, &mut body)
            .map(|result| result.map_err(|(error, _)| error))
            .collect();
        let mut declarations = Vec::new();
        for result in collected {
            match result {
                Ok(mut parsed) => declarations.append(&mut parsed),
                Err(error) => self.warnings.push(warning(&self.name, &error)),
            }
        }
        Ok(vec![PageItem::Box(which, declarations)])
    }
}

impl<'i> QualifiedRuleParser<'i> for PageBody {
    type Prelude = ();
    type QualifiedRule = Vec<PageItem>;
    type Error = StyleError<'i>;
}

impl<'i> RuleBodyItemParser<'i, Vec<PageItem>, StyleError<'i>> for PageBody {
    fn parse_declarations(&self) -> bool {
        true
    }

    fn parse_qualified(&self) -> bool {
        false
    }
}

/// The body of one page margin box: what it paints, and the style it
/// paints with.
struct MarginBoxBody {
    /// What diagnostics call the sheet this box was written in.
    sheet: String,
}

/// The `@page` twin of `written_at`: a url in a page body carries
/// where it was written the same way one in a style rule does.
fn page_written_at(declarations: &mut [PageDeclaration], sheet: &str, start: &ParserState) {
    for declaration in declarations {
        if let PageDeclaration::BackgroundImage(Some(url)) = declaration {
            url.origin = Some(super::position(sheet, start.source_location()));
        }
    }
}

impl<'i> DeclarationParser<'i> for MarginBoxBody {
    type Declaration = Vec<MarginDeclaration>;
    type Error = StyleError<'i>;

    fn parse_value<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
        start: &ParserState,
    ) -> Result<Self::Declaration, ParseError<'i, Self::Error>> {
        at(start, |input| {
            let declarations = match Spec::find(MARGIN_BOX_PROPERTIES, &name) {
                Some(spec) => spec.read(&name, input)?,
                None => {
                    let mut style = property(&name, input)?;
                    written_at(&mut style, &self.sheet, start);
                    style.into_iter().map(MarginDeclaration::Style).collect()
                }
            };
            input.expect_exhausted()?;
            Ok(declarations)
        })(input)
    }
}

impl<'i> AtRuleParser<'i> for MarginBoxBody {
    type Prelude = ();
    type AtRule = Vec<MarginDeclaration>;
    type Error = StyleError<'i>;
}

impl<'i> QualifiedRuleParser<'i> for MarginBoxBody {
    type Prelude = ();
    type QualifiedRule = Vec<MarginDeclaration>;
    type Error = StyleError<'i>;
}

impl<'i> RuleBodyItemParser<'i, Vec<MarginDeclaration>, StyleError<'i>> for MarginBoxBody {
    fn parse_declarations(&self) -> bool {
        true
    }

    fn parse_qualified(&self) -> bool {
        false
    }
}

/// What a margin box paints: nothing, the folio, a running string, or
/// a literal.
fn content(input: &mut Parser<'_, '_>) -> Option<Content> {
    if let Ok(text) = input.try_parse(|input| input.expect_string().cloned()) {
        return Some(Content::Text(text.as_ref().to_string()));
    }
    if let Ok(keyword) = input.try_parse(|input| input.expect_ident().cloned()) {
        return keyword
            .eq_ignore_ascii_case("none")
            .then_some(Content::None);
    }
    let function = input.expect_function().ok()?.clone();
    // `counter(page)`: the folio, and the only counter there is.
    if function.eq_ignore_ascii_case("counter") {
        return input
            .parse_nested_block(|input| {
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
                Ok::<_, ParseError<'_, StyleError<'_>>>(Content::Counter(style))
            })
            .ok();
    }
    // `string(name)`: the running string, at the value it stood at
    // when the page began.
    if function.eq_ignore_ascii_case("string") {
        return input
            .parse_nested_block(|input| {
                input
                    .expect_ident()
                    .map(|name| Content::String(name.as_ref().to_string()))
                    .map_err(ParseError::<StyleError<'_>>::from)
            })
            .ok();
    }
    None
}

/// `size`: one or two lengths, or a named sheet with an orientation.
fn size(input: &mut Parser<'_, '_>) -> Option<(f32, f32)> {
    if let Ok(Length::Points(width)) = input.try_parse(|input| length(input).ok_or(())) {
        let height = match input.try_parse(|input| length(input).ok_or(())) {
            Ok(Length::Points(height)) => height,
            Ok(_) => return None,
            Err(()) => width,
        };
        return Some((width, height));
    }
    let keyword = input.expect_ident().ok()?.clone();
    let (width, height) = named_size(&keyword)?;
    match input.try_parse(|input| input.expect_ident().cloned()) {
        Ok(orientation) if orientation.eq_ignore_ascii_case("landscape") => Some((height, width)),
        Ok(orientation) if orientation.eq_ignore_ascii_case("portrait") => Some((width, height)),
        Ok(_) => None,
        Err(_) => Some((width, height)),
    }
}

/// The sheet sizes CSS names, portrait, in points.
fn named_size(keyword: &str) -> Option<(f32, f32)> {
    PAGE_SIZES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(keyword))
        .map(|(_, size)| *size)
}

const fn mm(value: f32) -> f32 {
    value * 72.0 / 25.4
}

const fn inch(value: f32) -> f32 {
    value * 72.0
}

/// The sheet sizes `size` accepts by name, portrait, in points.
pub(crate) const PAGE_SIZES: &[(&str, (f32, f32))] = &[
    ("a3", (mm(297.0), mm(420.0))),
    ("a4", (mm(210.0), mm(297.0))),
    ("a5", (mm(148.0), mm(210.0))),
    ("b4", (mm(250.0), mm(353.0))),
    ("b5", (mm(176.0), mm(250.0))),
    ("letter", (inch(8.5), inch(11.0))),
    ("legal", (inch(8.5), inch(14.0))),
    ("ledger", (inch(11.0), inch(17.0))),
];
