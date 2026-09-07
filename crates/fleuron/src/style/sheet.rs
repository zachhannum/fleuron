//! Parsing: CSS text in, rules and diagnostics out.
//!
//! Nothing here matches or cascades. What it does decide is what the
//! engine claims to understand: a declaration outside the novel
//! subset does not become a rule, it becomes a warning naming the
//! position it was written at, and the rest of the sheet parses on.

use cssparser::{
    AtRuleParser, BasicParseErrorKind, CowRcStr, DeclarationParser, ParseError, ParseErrorKind,
    Parser, ParserInput, ParserState, QualifiedRuleParser, RuleBodyItemParser, RuleBodyParser,
    SourceLocation, StyleSheetParser, Token, match_ignore_ascii_case,
};
use selectors::SelectorList;
use selectors::parser::{ParseRelative, SelectorParseErrorKind};

use crate::Warning;
use crate::fonts::GenericFamily;
use crate::lines::{HangEnd, HangingPunctuation};
use crate::pages::Side;
use crate::style::element::{Fleuron, PseudoElement};
use crate::style::properties::{
    BorderStyle, BoxDecorationBreak, Break, Color, Content, CounterStyle, Declaration, Edge,
    Family, FontStyle, FontVariantCaps, Hyphens, LINE_WIDTHS, Length, LineHeight, MEDIUM,
    MarginBox, StringPiece, StringSet, TextAlign, TextJustify, TextTransform,
};

/// Where a stylesheet came from. The cascade sorts by this before it
/// sorts by anything else: author CSS overrides the built-in sheet
/// however specific the built-in rule was.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    /// The built-in sheet.
    UserAgent,
    /// A sheet the host handed in.
    #[default]
    Author,
}

/// One stylesheet handed to the compiler: its text, a name for
/// diagnostics, and the origin it cascades at.
#[derive(Debug, Clone)]
pub struct Source<'a> {
    /// What diagnostics call this sheet.
    pub name: &'a str,
    /// The sheet's text.
    pub css: &'a str,
    /// Which origin it cascades at.
    pub origin: Origin,
}

impl<'a> Source<'a> {
    /// A sheet the author supplied.
    pub fn author(name: &'a str, css: &'a str) -> Source<'a> {
        Source {
            name,
            css,
            origin: Origin::Author,
        }
    }

    /// A sheet that cascades with the built-in defaults.
    pub fn user_agent(name: &'a str, css: &'a str) -> Source<'a> {
        Source {
            name,
            css,
            origin: Origin::UserAgent,
        }
    }
}

/// `!important` beats a normal declaration of the same property in
/// the same origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Importance {
    Normal,
    Important,
}

/// One style rule: what it matches, and what it says.
#[derive(Debug)]
pub struct StyleRule {
    pub selectors: SelectorList<Fleuron>,
    pub declarations: Vec<(Declaration, Importance)>,
}

/// A `@page` rule: which pages it selects, the page box it sets, and
/// the margin boxes it fills.
#[derive(Debug, Default, Clone)]
pub struct PageRule {
    /// The named page this rule is for, from `page: <name>`.
    pub name: Option<String>,
    /// `:first` — the page a page group opens on.
    pub first: bool,
    /// `:blank` — a page inserted to square the sheet.
    pub blank: bool,
    /// `:left` / `:right`, as the side of the spread.
    pub side: Option<Side>,
    pub declarations: Vec<PageDeclaration>,
    pub boxes: Vec<(MarginBox, Vec<MarginDeclaration>)>,
}

impl PageRule {
    /// CSS 2.1 page-selector specificity: the name outweighs `:first`
    /// and `:blank`, which outweigh `:left` and `:right`.
    pub fn specificity(&self) -> (u8, u8, u8) {
        (
            self.name.is_some() as u8,
            (self.first || self.blank) as u8,
            self.side.is_some() as u8,
        )
    }
}

/// A declaration inside `@page`.
#[derive(Debug, Clone, PartialEq)]
pub enum PageDeclaration {
    /// Trim size in points.
    Size(f32, f32),
    Margin(Edge, Length),
}

/// A declaration inside a page margin box.
#[derive(Debug, Clone, PartialEq)]
pub enum MarginDeclaration {
    Content(Content),
    /// A text property; margin boxes set a line like any other.
    Style(Declaration),
}

/// One `@font-face`: an identity, and the sources to try for it.
///
/// Slope and weight are what the sheet declared, not what the file
/// says about itself; a sheet that declares neither leaves the file
/// to describe its own cuts.
#[derive(Debug, Clone, PartialEq)]
pub struct FontFace {
    pub family: String,
    pub style: Option<FontStyle>,
    pub weight: Option<u16>,
    pub src: Vec<Src>,
}

/// One entry of a `@font-face` `src` list.
#[derive(Debug, Clone, PartialEq)]
pub enum Src {
    /// A url for the host loader to resolve. The engine opens
    /// nothing itself.
    Url(String),
    /// A face by name, for a host that has one installed.
    Local(String),
}

/// One parsed sheet.
#[derive(Debug, Default)]
pub struct Sheet {
    pub origin: Origin,
    pub rules: Vec<StyleRule>,
    pub pages: Vec<PageRule>,
    pub faces: Vec<FontFace>,
}

/// Why some fragment of CSS did not become a rule.
#[derive(Debug, Clone)]
pub enum StyleError<'i> {
    UnsupportedProperty(CowRcStr<'i>),
    NotOnFirstLine(CowRcStr<'i>),
    UnsupportedValue(CowRcStr<'i>),
    UnsupportedAtRule(CowRcStr<'i>),
    UnsupportedPageSelector,
    Selector(SelectorParseErrorKind<'i>),
}

impl<'i> From<SelectorParseErrorKind<'i>> for StyleError<'i> {
    fn from(kind: SelectorParseErrorKind<'i>) -> Self {
        StyleError::Selector(kind)
    }
}

/// Parses one sheet, returning what the engine understood and a
/// warning for everything else.
pub fn parse(source: &Source<'_>) -> (Sheet, Vec<Warning>) {
    let mut input = ParserInput::new(source.css);
    let mut parser = Parser::new(&mut input);
    let mut top = TopLevel {
        sheet: Sheet {
            origin: source.origin,
            ..Sheet::default()
        },
        warnings: Vec::new(),
        name: source.name.to_string(),
    };
    let rules = StyleSheetParser::new(&mut parser, &mut top);
    let collected: Vec<_> = rules
        .map(|result| result.map_err(|(error, _)| error))
        .collect();
    for result in collected {
        match result {
            Ok(rule) => top.keep(rule),
            Err(error) => top.warn(&error),
        }
    }
    (top.sheet, top.warnings)
}

/// A parsed top-level rule, before it is filed into the sheet.
pub enum Rule {
    Style(StyleRule),
    Page(PageRule),
    FontFace(FontFace),
}

struct TopLevel {
    sheet: Sheet,
    warnings: Vec<Warning>,
    name: String,
}

impl TopLevel {
    fn keep(&mut self, rule: Rule) {
        match rule {
            Rule::Style(rule) => self.sheet.rules.push(rule),
            Rule::Page(rule) => self.sheet.pages.push(rule),
            Rule::FontFace(face) => self.sheet.faces.push(face),
        }
    }

    fn warn(&mut self, error: &ParseError<'_, StyleError<'_>>) {
        self.warnings.push(warning(&self.name, error));
    }
}

/// One parse error as the diagnostic a reader can act on: what the
/// engine did not understand, and where it was written.
pub fn warning(sheet: &str, error: &ParseError<'_, StyleError<'_>>) -> Warning {
    let message = match &error.kind {
        ParseErrorKind::Custom(StyleError::UnsupportedProperty(name)) => {
            format!("unsupported property `{name}`")
        }
        ParseErrorKind::Custom(StyleError::NotOnFirstLine(name)) => {
            format!("unsupported property `{name}` on `::first-line`")
        }
        ParseErrorKind::Custom(StyleError::UnsupportedValue(name)) => {
            format!("unsupported value for `{name}`")
        }
        ParseErrorKind::Custom(StyleError::UnsupportedAtRule(name)) => {
            format!("unsupported at-rule `@{name}`")
        }
        ParseErrorKind::Custom(StyleError::UnsupportedPageSelector) => {
            "unsupported `@page` selector".to_string()
        }
        ParseErrorKind::Custom(StyleError::Selector(
            SelectorParseErrorKind::UnsupportedPseudoClassOrElement(name),
        )) => format!("unsupported selector `:{name}`"),
        ParseErrorKind::Custom(StyleError::Selector(_)) => "unsupported selector".to_string(),
        ParseErrorKind::Basic(BasicParseErrorKind::AtRuleInvalid(name)) => {
            format!("unsupported at-rule `@{name}`")
        }
        ParseErrorKind::Basic(_) => "malformed CSS, skipped".to_string(),
    };
    Warning {
        message,
        origin: Some(position(sheet, error.location)),
    }
}

/// A CSS position as diagnostics spell it: `author.css:12:3`.
fn position(sheet: &str, location: SourceLocation) -> String {
    format!("{sheet}:{}:{}", location.line + 1, location.column)
}

impl<'i> QualifiedRuleParser<'i> for TopLevel {
    type Prelude = SelectorList<Fleuron>;
    type QualifiedRule = Rule;
    type Error = StyleError<'i>;

    fn parse_prelude<'t>(
        &mut self,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
        SelectorList::parse(&Selectors, input, ParseRelative::No)
    }

    fn parse_block<'t>(
        &mut self,
        selectors: Self::Prelude,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::QualifiedRule, ParseError<'i, Self::Error>> {
        let first_line = selectors
            .slice()
            .iter()
            .all(|selector| selector.pseudo_element() == Some(&PseudoElement::FirstLine));
        let (declarations, warnings) = declarations(input, &self.name, first_line);
        self.warnings.extend(warnings);
        Ok(Rule::Style(StyleRule {
            selectors,
            declarations,
        }))
    }
}

impl<'i> AtRuleParser<'i> for TopLevel {
    type Prelude = AtRule;
    type AtRule = Rule;
    type Error = StyleError<'i>;

    fn parse_prelude<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
        match_ignore_ascii_case! { &name,
            "page" => Ok(AtRule::Page(page_selector(input)?)),
            "font-face" => Ok(AtRule::FontFace),
            _ => Err(input.new_custom_error(StyleError::UnsupportedAtRule(name.clone()))),
        }
    }

    fn parse_block<'t>(
        &mut self,
        prelude: Self::Prelude,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::AtRule, ParseError<'i, Self::Error>> {
        match prelude {
            AtRule::Page(mut rule) => {
                let mut body = PageBody {
                    name: self.name.clone(),
                    warnings: Vec::new(),
                };
                let items: Vec<_> = RuleBodyParser::new(input, &mut body)
                    .map(|result| result.map_err(|(error, _)| error))
                    .collect();
                self.warnings.append(&mut body.warnings);
                for item in items {
                    match item {
                        Ok(parsed) => {
                            for item in parsed {
                                match item {
                                    PageItem::Declaration(declaration) => {
                                        rule.declarations.push(declaration)
                                    }
                                    PageItem::Box(which, declarations) => {
                                        rule.boxes.push((which, declarations))
                                    }
                                }
                            }
                        }
                        Err(error) => self.warnings.push(warning(&self.name, &error)),
                    }
                }
                Ok(Rule::Page(rule))
            }
            AtRule::FontFace => {
                let (face, warnings) = font_face(input, &self.name);
                self.warnings.extend(warnings);
                Ok(Rule::FontFace(face))
            }
        }
    }
}

/// The at-rules the engine parses, once their prelude is read.
pub enum AtRule {
    Page(PageRule),
    FontFace,
}

/// `@page` prelude: an optional page name, then any of `:first`,
/// `:blank`, `:left`, `:right`.
fn page_selector<'i>(
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

/// The selector parser: the engine takes plain selectors and nothing
/// that assumes a live document.
struct Selectors;

impl<'i> selectors::Parser<'i> for Selectors {
    type Impl = Fleuron;
    type Error = StyleError<'i>;

    fn parse_is_and_where(&self) -> bool {
        true
    }

    fn parse_nth_child_of(&self) -> bool {
        true
    }

    fn parse_has(&self) -> bool {
        true
    }

    fn parse_pseudo_element(
        &self,
        location: SourceLocation,
        name: CowRcStr<'i>,
    ) -> Result<PseudoElement, ParseError<'i, StyleError<'i>>> {
        match_ignore_ascii_case! { &name,
            "first-letter" => Ok(PseudoElement::FirstLetter),
            "first-line" => Ok(PseudoElement::FirstLine),
            _ => Err(location.new_custom_error(
                SelectorParseErrorKind::UnsupportedPseudoClassOrElement(name.clone()),
            )),
        }
    }
}

/// Every declaration in one style-rule body, plus a warning for each
/// one that fell outside the subset. `first_line` narrows the subset
/// to what `::first-line` takes.
fn declarations(
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
fn at<'i: 't, 't, T>(
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
    read: Reader<D>,
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
fn longhand<'i, T, D>(
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
fn sides<'i, T: Copy, D>(
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
        syntax: "auto | avoid | avoid-page | page | always | left | right | recto | verso",
        examples: &["recto"],
        read: |name, input| longhand(name, input, break_value, Declaration::BreakBefore),
    },
    Spec {
        name: "break-after",
        inherited: false,
        syntax: "auto | avoid | avoid-page | page | always | left | right | recto | verso",
        examples: &["avoid"],
        read: |name, input| longhand(name, input, break_value, Declaration::BreakAfter),
    },
    Spec {
        name: "break-inside",
        inherited: false,
        syntax: "auto | avoid | avoid-page | page | always | left | right | recto | verso",
        examples: &["avoid"],
        read: |name, input| longhand(name, input, break_value, Declaration::BreakInside),
    },
];

/// One declaration of the novel subset, expanded to longhands.
fn property<'i>(
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
fn keyword_or<'i, T>(
    input: &mut Parser<'i, '_>,
    parse: fn(&mut Parser<'i, '_>) -> Option<T>,
    bad: impl Fn() -> StyleError<'i>,
) -> Result<T, ParseError<'i, StyleError<'i>>> {
    match input.try_parse(|input| parse(input).ok_or(())) {
        Ok(value) => Ok(value),
        Err(()) => Err(input.new_custom_error(bad())),
    }
}

fn font_style(input: &mut Parser<'_, '_>) -> Option<FontStyle> {
    let keyword = input.expect_ident().ok()?.clone();
    match_ignore_ascii_case! { &keyword,
        "normal" => Some(FontStyle::Normal),
        "italic" | "oblique" => Some(FontStyle::Italic),
        _ => None,
    }
}

/// `letter-spacing: normal | <length>`. `normal` is no tracking at
/// all, which is what a length of zero already says.
fn letter_spacing(input: &mut Parser<'_, '_>) -> Option<Length> {
    if input
        .try_parse(|input| input.expect_ident_matching("normal"))
        .is_ok()
    {
        return Some(Length::Points(0.0));
    }
    length(input)
}

/// `color: <named-colour> | #rgb | #rrggbb | rgb()`.
fn color(input: &mut Parser<'_, '_>) -> Option<Color> {
    if let Ok(color) = input.try_parse(rgb_function) {
        return Some(color);
    }
    match input.next().ok()? {
        Token::Hash(digits) | Token::IDHash(digits) => hex(digits),
        Token::Ident(name) => named(name),
        _ => None,
    }
}

/// `rgb(r, g, b)`, with either commas or spaces between the
/// channels. All three are numbers or all three are percentages.
fn rgb_function<'i>(input: &mut Parser<'i, '_>) -> Result<Color, ParseError<'i, StyleError<'i>>> {
    input.expect_function_matching("rgb")?;
    input.parse_nested_block(|input| {
        let (red, percentages) = channel(input, None)?;
        let commas = input.try_parse(|input| input.expect_comma()).is_ok();
        let (green, _) = channel(input, Some(percentages))?;
        if commas {
            input.expect_comma()?;
        }
        let (blue, _) = channel(input, Some(percentages))?;
        input.expect_exhausted()?;
        Ok(Color::rgb(red, green, blue))
    })
}

/// One channel of `rgb()`: `0` to `255`, or a percentage of it, and
/// which of the two it was. `written` is how the channels before it
/// were written, and a channel that disagrees is not a colour.
fn channel<'i>(
    input: &mut Parser<'i, '_>,
    written: Option<bool>,
) -> Result<(u8, bool), ParseError<'i, StyleError<'i>>> {
    let percentage = input.try_parse(|input| input.expect_percentage());
    if written.is_some_and(|percentages| percentages != percentage.is_ok()) {
        return Err(input.new_error_for_next_token());
    }
    let value = match percentage {
        Ok(percentage) => percentage * 255.0,
        Err(_) => input.expect_number()?,
    };
    Ok((value.round().clamp(0.0, 255.0) as u8, percentage.is_ok()))
}

/// The two hex forms. `#abc` is `#aabbcc`: each digit stands for a
/// pair of itself.
fn hex(digits: &str) -> Option<Color> {
    let spelled: String = match digits.len() {
        3 => digits.chars().flat_map(|digit| [digit, digit]).collect(),
        6 => digits.to_string(),
        _ => return None,
    };
    Color::from_hex(&format!("#{spelled}"))
}

/// One of the CSS colour names, whatever case it was written in.
fn named(name: &str) -> Option<Color> {
    let lowercase = name.to_ascii_lowercase();
    let at = NAMED
        .binary_search_by_key(&lowercase.as_str(), |(known, _)| known)
        .ok()?;
    Some(NAMED[at].1)
}

/// `background-color: <color> | transparent`, where `transparent` is
/// the initial value: nothing is painted behind the block.
fn background_color(input: &mut Parser<'_, '_>) -> Option<Option<Color>> {
    if input
        .try_parse(|input| input.expect_ident_matching("transparent"))
        .is_ok()
    {
        return Some(None);
    }
    color(input).map(Some)
}

/// `<line-width>`: a length, or one of the three keywords a browser
/// gives a pixel width to.
fn line_width(input: &mut Parser<'_, '_>) -> Option<Length> {
    if let Ok(keyword) = input.try_parse(|input| input.expect_ident().cloned()) {
        return LINE_WIDTHS
            .iter()
            .find(|(name, _)| keyword.eq_ignore_ascii_case(name))
            .map(|(_, points)| Length::Points(*points));
    }
    length(input)
}

/// `<line-style>`, of which the engine draws one.
fn line_style(input: &mut Parser<'_, '_>) -> Option<BorderStyle> {
    let keyword = input.expect_ident().ok()?.clone();
    match_ignore_ascii_case! { &keyword,
        "none" => Some(BorderStyle::None),
        "solid" => Some(BorderStyle::Solid),
        _ => None,
    }
}

/// A colour written where a longhand takes one, which never means
/// `currentColor`: only the shorthand leaves the colour out.
fn border_color(input: &mut Parser<'_, '_>) -> Option<Option<Color>> {
    color(input).map(Some)
}

fn decoration_break(input: &mut Parser<'_, '_>) -> Option<BoxDecorationBreak> {
    let keyword = input.expect_ident().ok()?.clone();
    match_ignore_ascii_case! { &keyword,
        "slice" => Some(BoxDecorationBreak::Slice),
        "clone" => Some(BoxDecorationBreak::Clone),
        _ => None,
    }
}

fn variant_caps(input: &mut Parser<'_, '_>) -> Option<FontVariantCaps> {
    let keyword = input.expect_ident().ok()?.clone();
    match_ignore_ascii_case! { &keyword,
        "normal" => Some(FontVariantCaps::Normal),
        "small-caps" => Some(FontVariantCaps::SmallCaps),
        _ => None,
    }
}

fn text_transform(input: &mut Parser<'_, '_>) -> Option<TextTransform> {
    let keyword = input.expect_ident().ok()?.clone();
    match_ignore_ascii_case! { &keyword,
        "none" => Some(TextTransform::None),
        "uppercase" => Some(TextTransform::Uppercase),
        "lowercase" => Some(TextTransform::Lowercase),
        "capitalize" => Some(TextTransform::Capitalize),
        _ => None,
    }
}

fn text_align(input: &mut Parser<'_, '_>) -> Option<TextAlign> {
    let keyword = input.expect_ident().ok()?.clone();
    match_ignore_ascii_case! { &keyword,
        "left" | "start" => Some(TextAlign::Left),
        "right" | "end" => Some(TextAlign::Right),
        "center" => Some(TextAlign::Center),
        "justify" => Some(TextAlign::Justify),
        _ => None,
    }
}

fn text_justify(input: &mut Parser<'_, '_>) -> Option<TextJustify> {
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
fn hanging(input: &mut Parser<'_, '_>) -> Option<HangingPunctuation> {
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

fn hyphens(input: &mut Parser<'_, '_>) -> Option<Hyphens> {
    let keyword = input.expect_ident().ok()?.clone();
    match_ignore_ascii_case! { &keyword,
        "none" | "manual" => Some(Hyphens::None),
        "auto" => Some(Hyphens::Auto),
        _ => None,
    }
}

fn page_name(input: &mut Parser<'_, '_>) -> Option<Option<String>> {
    let keyword = input.expect_ident().ok()?.clone();
    if keyword.eq_ignore_ascii_case("auto") {
        Some(None)
    } else {
        Some(Some(keyword.as_ref().to_string()))
    }
}

/// What an element paints in place of children: a literal, or
/// nothing. The page counter belongs to margin boxes, not to prose.
fn ornament(input: &mut Parser<'_, '_>) -> Option<Content> {
    if let Ok(text) = input.try_parse(|input| input.expect_string().cloned()) {
        return Some(Content::Text(text.as_ref().to_string()));
    }
    input
        .expect_ident()
        .ok()?
        .eq_ignore_ascii_case("none")
        .then_some(Content::None)
}

/// `string-set: none | <name> [content() | <string>]+ [, …]`. What a
/// running head picks up: the element's own text, literals, or both.
fn string_set(input: &mut Parser<'_, '_>) -> Option<Vec<StringSet>> {
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
fn counter_reset(input: &mut Parser<'_, '_>) -> Option<Option<u32>> {
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

fn count(input: &mut Parser<'_, '_>) -> Option<u16> {
    let value = input.expect_integer().ok()?;
    u16::try_from(value.max(0)).ok()
}

fn weight(input: &mut Parser<'_, '_>) -> Option<u16> {
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

fn line_height(input: &mut Parser<'_, '_>) -> Option<LineHeight> {
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

fn break_value(input: &mut Parser<'_, '_>) -> Option<Break> {
    let keyword = input.expect_ident().ok()?.clone();
    match_ignore_ascii_case! { &keyword,
        "auto" => Some(Break::Auto),
        "avoid" | "avoid-page" => Some(Break::Avoid),
        "page" | "always" => Some(Break::Page),
        "left" | "verso" => Some(Break::Side(Side::Verso)),
        "right" | "recto" => Some(Break::Side(Side::Recto)),
        _ => None,
    }
}

/// A one-to-four value shorthand, in the CSS order:
/// top/right/bottom/left filled in the usual way.
fn edges<T: Copy>(
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
fn length(input: &mut Parser<'_, '_>) -> Option<Length> {
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
fn families<'i>(input: &mut Parser<'i, '_>) -> Result<Vec<Family>, ParseError<'i, StyleError<'i>>> {
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

/// The pseudo-classes a selector may use, as they are written, with
/// one use of each that parses. The selector parser is the
/// `selectors` crate's, so this table is checked against it rather
/// than read by it.
pub(crate) const PSEUDO_CLASSES: &[(&str, &str)] = &[
    (":first-child", "p:first-child"),
    (":last-child", "p:last-child"),
    (":only-child", "p:only-child"),
    (":nth-child()", "p:nth-child(2n+1)"),
    (":nth-last-child()", "p:nth-last-child(2)"),
    (":first-of-type", "p:first-of-type"),
    (":last-of-type", "p:last-of-type"),
    (":only-of-type", "p:only-of-type"),
    (":nth-of-type()", "p:nth-of-type(2)"),
    (":nth-last-of-type()", "p:nth-last-of-type(2)"),
    (":empty", "p:empty"),
    (":root", ":root"),
    (":is()", ":is(h1, h2)"),
    (":where()", ":where(h1, h2)"),
    (":not()", "p:not(:first-child)"),
    (":has()", "p:has(em)"),
];

/// What a compound selector is made of besides pseudo-classes, with
/// one use of each that parses.
pub(crate) const COMPOUNDS: &[(&str, &str)] = &[("<element>", "p"), ("*", "section > *")];

/// How selectors join in a list, with one use that parses.
pub(crate) const SELECTOR_LIST: (&str, &str) = (",", "h1, h2");

/// The shape of one declaration.
pub(crate) const DECLARATION: &str = "<property>: <value> !important?";

/// The combinators between two compounds, by their CSS names, with
/// one use of each that parses.
pub(crate) const COMBINATORS: &[(&str, &str)] = &[
    ("descendant", "section p"),
    ("child", "section > p"),
    ("next-sibling", "h1 + p"),
    ("subsequent-sibling", "h1 ~ p"),
];

/// The pseudo-elements, as they are written, with one use of each
/// that parses.
pub(crate) const PSEUDO_ELEMENTS: &[(&str, &str)] = &[
    ("::first-letter", "p::first-letter"),
    ("::first-line", "p::first-line"),
];

/// What `::first-line` takes. Everything but `color` changes the
/// width of the shaped run, which is what the second breaking pass is
/// for; `color` is paint alone and breaks the same either way.
pub(crate) const FIRST_LINE_PROPERTIES: &[&str] = &[
    "color",
    "font-size",
    "font-variant-caps",
    "letter-spacing",
    "text-transform",
];

/// The body of one `@page` rule: page declarations and margin boxes.
struct PageBody {
    name: String,
    warnings: Vec<Warning>,
}

/// One item of a `@page` body.
enum PageItem {
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
            let items = spec.read(&name, input)?;
            input.expect_exhausted()?;
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
        let mut body = MarginBoxBody;
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
struct MarginBoxBody;

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
                None => property(&name, input)?
                    .into_iter()
                    .map(MarginDeclaration::Style)
                    .collect(),
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

/// One `@font-face` body.
fn font_face(input: &mut Parser<'_, '_>, sheet: &str) -> (FontFace, Vec<Warning>) {
    let mut body = FontFaceBody;
    let collected: Vec<_> = RuleBodyParser::new(input, &mut body)
        .map(|result| result.map_err(|(error, _)| error))
        .collect();
    let mut face = FontFace {
        family: String::new(),
        style: None,
        weight: None,
        src: Vec::new(),
    };
    let mut warnings = Vec::new();
    for result in collected {
        let declarations = match result {
            Ok(declarations) => declarations,
            Err(error) => {
                warnings.push(warning(sheet, &error));
                continue;
            }
        };
        for declaration in declarations {
            match declaration {
                FaceDeclaration::Family(family) => face.family = family,
                FaceDeclaration::Style(style) => face.style = Some(style),
                FaceDeclaration::Weight(weight) => face.weight = Some(weight),
                FaceDeclaration::Src(src) => face.src = src,
            }
        }
    }
    (face, warnings)
}

/// The descriptors of an `@font-face` rule.
pub(crate) const FONT_FACE_DESCRIPTORS: &[Spec<FaceDeclaration>] = &[
    Spec {
        name: "font-family",
        inherited: false,
        syntax: "<family-name>",
        examples: &["\"Author Serif\"", "Author Serif"],
        read: |name, input| {
            let family = match families(input)?.first() {
                Some(Family::Named(family)) => family.clone(),
                Some(Family::Generic(generic)) => generic.keyword().to_string(),
                None => {
                    return Err(input.new_custom_error(StyleError::UnsupportedValue(name.clone())));
                }
            };
            Ok(vec![FaceDeclaration::Family(family)])
        },
    },
    Spec {
        name: "font-style",
        inherited: false,
        syntax: "normal | italic | oblique",
        examples: &["italic"],
        read: |name, input| longhand(name, input, font_style, FaceDeclaration::Style),
    },
    Spec {
        name: "font-weight",
        inherited: false,
        syntax: "normal | bold | <number [1,1000]>",
        examples: &["bold", "600"],
        read: |name, input| longhand(name, input, weight, FaceDeclaration::Weight),
    },
    Spec {
        name: "src",
        inherited: false,
        syntax: "[ <url> format(<string>)? | local(<string>) ]#",
        examples: &[
            "url(fonts/serif.otf)",
            "url(\"fonts/serif.woff2\") format(\"woff2\")",
            "local(\"Author Serif\"), url(fonts/serif.otf)",
        ],
        read: |_, input| Ok(vec![FaceDeclaration::Src(sources(input)?)]),
    },
];

/// The body of one `@font-face`.
struct FontFaceBody;

/// One `@font-face` descriptor.
pub(crate) enum FaceDeclaration {
    Family(String),
    Style(FontStyle),
    Weight(u16),
    Src(Vec<Src>),
}

impl<'i> DeclarationParser<'i> for FontFaceBody {
    type Declaration = Vec<FaceDeclaration>;
    type Error = StyleError<'i>;

    fn parse_value<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
        start: &ParserState,
    ) -> Result<Self::Declaration, ParseError<'i, Self::Error>> {
        at(start, |input| {
            let Some(spec) = Spec::find(FONT_FACE_DESCRIPTORS, &name) else {
                return Err(input.new_custom_error(StyleError::UnsupportedProperty(name.clone())));
            };
            let declarations = spec.read(&name, input)?;
            input.expect_exhausted()?;
            Ok(declarations)
        })(input)
    }
}

impl<'i> AtRuleParser<'i> for FontFaceBody {
    type Prelude = ();
    type AtRule = Vec<FaceDeclaration>;
    type Error = StyleError<'i>;
}

impl<'i> QualifiedRuleParser<'i> for FontFaceBody {
    type Prelude = ();
    type QualifiedRule = Vec<FaceDeclaration>;
    type Error = StyleError<'i>;
}

impl<'i> RuleBodyItemParser<'i, Vec<FaceDeclaration>, StyleError<'i>> for FontFaceBody {
    fn parse_declarations(&self) -> bool {
        true
    }

    fn parse_qualified(&self) -> bool {
        false
    }
}

/// A `src` list: urls for the host to resolve, or faces it may
/// already have. `format()` hints are read and dropped — the loader
/// hands back bytes and the registry decides what they are.
fn sources<'i>(input: &mut Parser<'i, '_>) -> Result<Vec<Src>, ParseError<'i, StyleError<'i>>> {
    input.parse_comma_separated(|input| {
        if let Ok(url) = input.try_parse(|input| input.expect_url()) {
            let source = Src::Url(url.as_ref().to_string());
            let _ = input.try_parse(|input| {
                input.expect_function_matching("format")?;
                input.parse_nested_block(|input| {
                    input
                        .expect_string()
                        .map(|_| ())
                        .map_err(ParseError::<StyleError<'_>>::from)
                })
            });
            return Ok(source);
        }
        input.expect_function_matching("local")?;
        let name = input.parse_nested_block(|input| {
            input
                .expect_string()
                .map(|name| name.as_ref().to_string())
                .map_err(ParseError::from)
        })?;
        Ok(Src::Local(name))
    })
}

/// The CSS named colours, sorted for binary search.
pub(crate) const NAMED: [(&str, Color); 148] = [
    ("aliceblue", Color::rgb(240, 248, 255)),
    ("antiquewhite", Color::rgb(250, 235, 215)),
    ("aqua", Color::rgb(0, 255, 255)),
    ("aquamarine", Color::rgb(127, 255, 212)),
    ("azure", Color::rgb(240, 255, 255)),
    ("beige", Color::rgb(245, 245, 220)),
    ("bisque", Color::rgb(255, 228, 196)),
    ("black", Color::rgb(0, 0, 0)),
    ("blanchedalmond", Color::rgb(255, 235, 205)),
    ("blue", Color::rgb(0, 0, 255)),
    ("blueviolet", Color::rgb(138, 43, 226)),
    ("brown", Color::rgb(165, 42, 42)),
    ("burlywood", Color::rgb(222, 184, 135)),
    ("cadetblue", Color::rgb(95, 158, 160)),
    ("chartreuse", Color::rgb(127, 255, 0)),
    ("chocolate", Color::rgb(210, 105, 30)),
    ("coral", Color::rgb(255, 127, 80)),
    ("cornflowerblue", Color::rgb(100, 149, 237)),
    ("cornsilk", Color::rgb(255, 248, 220)),
    ("crimson", Color::rgb(220, 20, 60)),
    ("cyan", Color::rgb(0, 255, 255)),
    ("darkblue", Color::rgb(0, 0, 139)),
    ("darkcyan", Color::rgb(0, 139, 139)),
    ("darkgoldenrod", Color::rgb(184, 134, 11)),
    ("darkgray", Color::rgb(169, 169, 169)),
    ("darkgreen", Color::rgb(0, 100, 0)),
    ("darkgrey", Color::rgb(169, 169, 169)),
    ("darkkhaki", Color::rgb(189, 183, 107)),
    ("darkmagenta", Color::rgb(139, 0, 139)),
    ("darkolivegreen", Color::rgb(85, 107, 47)),
    ("darkorange", Color::rgb(255, 140, 0)),
    ("darkorchid", Color::rgb(153, 50, 204)),
    ("darkred", Color::rgb(139, 0, 0)),
    ("darksalmon", Color::rgb(233, 150, 122)),
    ("darkseagreen", Color::rgb(143, 188, 143)),
    ("darkslateblue", Color::rgb(72, 61, 139)),
    ("darkslategray", Color::rgb(47, 79, 79)),
    ("darkslategrey", Color::rgb(47, 79, 79)),
    ("darkturquoise", Color::rgb(0, 206, 209)),
    ("darkviolet", Color::rgb(148, 0, 211)),
    ("deeppink", Color::rgb(255, 20, 147)),
    ("deepskyblue", Color::rgb(0, 191, 255)),
    ("dimgray", Color::rgb(105, 105, 105)),
    ("dimgrey", Color::rgb(105, 105, 105)),
    ("dodgerblue", Color::rgb(30, 144, 255)),
    ("firebrick", Color::rgb(178, 34, 34)),
    ("floralwhite", Color::rgb(255, 250, 240)),
    ("forestgreen", Color::rgb(34, 139, 34)),
    ("fuchsia", Color::rgb(255, 0, 255)),
    ("gainsboro", Color::rgb(220, 220, 220)),
    ("ghostwhite", Color::rgb(248, 248, 255)),
    ("gold", Color::rgb(255, 215, 0)),
    ("goldenrod", Color::rgb(218, 165, 32)),
    ("gray", Color::rgb(128, 128, 128)),
    ("green", Color::rgb(0, 128, 0)),
    ("greenyellow", Color::rgb(173, 255, 47)),
    ("grey", Color::rgb(128, 128, 128)),
    ("honeydew", Color::rgb(240, 255, 240)),
    ("hotpink", Color::rgb(255, 105, 180)),
    ("indianred", Color::rgb(205, 92, 92)),
    ("indigo", Color::rgb(75, 0, 130)),
    ("ivory", Color::rgb(255, 255, 240)),
    ("khaki", Color::rgb(240, 230, 140)),
    ("lavender", Color::rgb(230, 230, 250)),
    ("lavenderblush", Color::rgb(255, 240, 245)),
    ("lawngreen", Color::rgb(124, 252, 0)),
    ("lemonchiffon", Color::rgb(255, 250, 205)),
    ("lightblue", Color::rgb(173, 216, 230)),
    ("lightcoral", Color::rgb(240, 128, 128)),
    ("lightcyan", Color::rgb(224, 255, 255)),
    ("lightgoldenrodyellow", Color::rgb(250, 250, 210)),
    ("lightgray", Color::rgb(211, 211, 211)),
    ("lightgreen", Color::rgb(144, 238, 144)),
    ("lightgrey", Color::rgb(211, 211, 211)),
    ("lightpink", Color::rgb(255, 182, 193)),
    ("lightsalmon", Color::rgb(255, 160, 122)),
    ("lightseagreen", Color::rgb(32, 178, 170)),
    ("lightskyblue", Color::rgb(135, 206, 250)),
    ("lightslategray", Color::rgb(119, 136, 153)),
    ("lightslategrey", Color::rgb(119, 136, 153)),
    ("lightsteelblue", Color::rgb(176, 196, 222)),
    ("lightyellow", Color::rgb(255, 255, 224)),
    ("lime", Color::rgb(0, 255, 0)),
    ("limegreen", Color::rgb(50, 205, 50)),
    ("linen", Color::rgb(250, 240, 230)),
    ("magenta", Color::rgb(255, 0, 255)),
    ("maroon", Color::rgb(128, 0, 0)),
    ("mediumaquamarine", Color::rgb(102, 205, 170)),
    ("mediumblue", Color::rgb(0, 0, 205)),
    ("mediumorchid", Color::rgb(186, 85, 211)),
    ("mediumpurple", Color::rgb(147, 112, 219)),
    ("mediumseagreen", Color::rgb(60, 179, 113)),
    ("mediumslateblue", Color::rgb(123, 104, 238)),
    ("mediumspringgreen", Color::rgb(0, 250, 154)),
    ("mediumturquoise", Color::rgb(72, 209, 204)),
    ("mediumvioletred", Color::rgb(199, 21, 133)),
    ("midnightblue", Color::rgb(25, 25, 112)),
    ("mintcream", Color::rgb(245, 255, 250)),
    ("mistyrose", Color::rgb(255, 228, 225)),
    ("moccasin", Color::rgb(255, 228, 181)),
    ("navajowhite", Color::rgb(255, 222, 173)),
    ("navy", Color::rgb(0, 0, 128)),
    ("oldlace", Color::rgb(253, 245, 230)),
    ("olive", Color::rgb(128, 128, 0)),
    ("olivedrab", Color::rgb(107, 142, 35)),
    ("orange", Color::rgb(255, 165, 0)),
    ("orangered", Color::rgb(255, 69, 0)),
    ("orchid", Color::rgb(218, 112, 214)),
    ("palegoldenrod", Color::rgb(238, 232, 170)),
    ("palegreen", Color::rgb(152, 251, 152)),
    ("paleturquoise", Color::rgb(175, 238, 238)),
    ("palevioletred", Color::rgb(219, 112, 147)),
    ("papayawhip", Color::rgb(255, 239, 213)),
    ("peachpuff", Color::rgb(255, 218, 185)),
    ("peru", Color::rgb(205, 133, 63)),
    ("pink", Color::rgb(255, 192, 203)),
    ("plum", Color::rgb(221, 160, 221)),
    ("powderblue", Color::rgb(176, 224, 230)),
    ("purple", Color::rgb(128, 0, 128)),
    ("rebeccapurple", Color::rgb(102, 51, 153)),
    ("red", Color::rgb(255, 0, 0)),
    ("rosybrown", Color::rgb(188, 143, 143)),
    ("royalblue", Color::rgb(65, 105, 225)),
    ("saddlebrown", Color::rgb(139, 69, 19)),
    ("salmon", Color::rgb(250, 128, 114)),
    ("sandybrown", Color::rgb(244, 164, 96)),
    ("seagreen", Color::rgb(46, 139, 87)),
    ("seashell", Color::rgb(255, 245, 238)),
    ("sienna", Color::rgb(160, 82, 45)),
    ("silver", Color::rgb(192, 192, 192)),
    ("skyblue", Color::rgb(135, 206, 235)),
    ("slateblue", Color::rgb(106, 90, 205)),
    ("slategray", Color::rgb(112, 128, 144)),
    ("slategrey", Color::rgb(112, 128, 144)),
    ("snow", Color::rgb(255, 250, 250)),
    ("springgreen", Color::rgb(0, 255, 127)),
    ("steelblue", Color::rgb(70, 130, 180)),
    ("tan", Color::rgb(210, 180, 140)),
    ("teal", Color::rgb(0, 128, 128)),
    ("thistle", Color::rgb(216, 191, 216)),
    ("tomato", Color::rgb(255, 99, 71)),
    ("turquoise", Color::rgb(64, 224, 208)),
    ("violet", Color::rgb(238, 130, 238)),
    ("wheat", Color::rgb(245, 222, 179)),
    ("white", Color::rgb(255, 255, 255)),
    ("whitesmoke", Color::rgb(245, 245, 245)),
    ("yellow", Color::rgb(255, 255, 0)),
    ("yellowgreen", Color::rgb(154, 205, 50)),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The name table is sorted, because the search over it is
    /// binary, and every name in it reads back.
    #[test]
    fn every_colour_name_is_in_order_and_reads_back() {
        for pair in NAMED.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
        }
        for (name, color) in NAMED {
            assert_eq!(named(name), Some(color), "{name}");
        }
        assert_eq!(named("REBECCAPURPLE"), Some(Color::rgb(102, 51, 153)));
        assert_eq!(named("octarine"), None);
    }
}
