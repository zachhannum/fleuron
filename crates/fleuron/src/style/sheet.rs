//! Parsing: CSS text in, rules and diagnostics out.
//!
//! Nothing here matches or cascades. What it does decide is what the
//! engine claims to understand: a declaration outside the novel
//! subset does not become a rule, it becomes a warning carrying the
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
use crate::pages::Side;
use crate::style::element::Fleuron;
use crate::style::properties::{
    Break, Content, Declaration, Edge, Family, FontStyle, Hyphens, Length, LineHeight, MarginBox,
    TextAlign,
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
    /// A text property; margin boxes carry a line like any other.
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
        let (declarations, warnings) = declarations(input, &self.name);
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
        match_ignore_ascii_case! { &pseudo,
            "first" => rule.first = true,
            "blank" => rule.blank = true,
            "left" => rule.side = Some(Side::Verso),
            "right" => rule.side = Some(Side::Recto),
            _ => return Err(input.new_custom_error(StyleError::UnsupportedPageSelector)),
        }
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
}

/// Every declaration in one style-rule body, plus a warning for each
/// one that fell outside the subset.
fn declarations(
    input: &mut Parser<'_, '_>,
    sheet: &str,
) -> (Vec<(Declaration, Importance)>, Vec<Warning>) {
    let mut properties = Properties;
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
struct Properties;

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

/// One declaration of the novel subset, expanded to longhands.
fn property<'i>(
    name: &CowRcStr<'i>,
    input: &mut Parser<'i, '_>,
) -> Result<Vec<Declaration>, ParseError<'i, StyleError<'i>>> {
    let bad = || StyleError::UnsupportedValue(name.clone());
    let one = |declaration| Ok(vec![declaration]);
    match_ignore_ascii_case! { name,
        "font-family" => one(Declaration::FontFamily(families(input)?)),
        "font-size" => one(Declaration::FontSize(keyword_or(input, length, bad)?)),
        "font-style" => one(Declaration::FontStyle(keyword_or(input, font_style, bad)?)),
        "font-weight" => one(Declaration::FontWeight(keyword_or(input, weight, bad)?)),
        "line-height" => one(Declaration::LineHeight(keyword_or(input, line_height, bad)?)),
        "text-align" => one(Declaration::TextAlign(keyword_or(input, text_align, bad)?)),
        "text-indent" => one(Declaration::TextIndent(keyword_or(input, length, bad)?)),
        "hyphens" => one(Declaration::Hyphens(keyword_or(input, hyphens, bad)?)),
        "orphans" => one(Declaration::Orphans(keyword_or(input, count, bad)?)),
        "widows" => one(Declaration::Widows(keyword_or(input, count, bad)?)),
        "page" => one(Declaration::Page(keyword_or(input, page_name, bad)?)),
        "margin" => Ok(edges(input)
            .ok_or_else(|| input.new_custom_error(bad()))?
            .into_iter()
            .map(|(edge, length)| Declaration::Margin(edge, length))
            .collect()),
        "margin-top" => one(Declaration::Margin(Edge::Top, keyword_or(input, length, bad)?)),
        "margin-right" => one(Declaration::Margin(Edge::Right, keyword_or(input, length, bad)?)),
        "margin-bottom" => one(Declaration::Margin(Edge::Bottom, keyword_or(input, length, bad)?)),
        "margin-left" => one(Declaration::Margin(Edge::Left, keyword_or(input, length, bad)?)),
        "break-before" => one(Declaration::BreakBefore(keyword_or(input, break_value, bad)?)),
        "break-after" => one(Declaration::BreakAfter(keyword_or(input, break_value, bad)?)),
        "break-inside" => one(Declaration::BreakInside(keyword_or(input, break_value, bad)?)),
        _ => Err(input.new_custom_error(StyleError::UnsupportedProperty(name.clone()))),
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

/// The `margin` shorthand, in the CSS order: one to four lengths,
/// top/right/bottom/left filled in the usual way.
fn edges(input: &mut Parser<'_, '_>) -> Option<Vec<(Edge, Length)>> {
    let mut values = Vec::new();
    while values.len() < 4 {
        match input.try_parse(|input| length(input).ok_or(())) {
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
            match_ignore_ascii_case! { unit,
                "pt" => Some(Length::Points(value)),
                "px" => Some(Length::Points(value * 0.75)),
                "pc" => Some(Length::Points(value * 12.0)),
                "in" => Some(Length::Points(value * 72.0)),
                "cm" => Some(Length::Points(value * 72.0 / 2.54)),
                "mm" => Some(Length::Points(value * 72.0 / 25.4)),
                "q" => Some(Length::Points(value * 72.0 / 101.6)),
                "em" => Some(Length::Em(value)),
                "rem" => Some(Length::Rem(value)),
                _ => None,
            }
        }
        _ => None,
    }
}

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
            let bad = || StyleError::UnsupportedValue(name.clone());
            let items = match_ignore_ascii_case! { &name,
                "size" => {
                    let (width, height) = keyword_or(input, size, bad)?;
                    vec![PageDeclaration::Size(width, height)]
                },
                "margin" => edges(input)
                    .ok_or_else(|| input.new_custom_error(bad()))?
                    .into_iter()
                    .map(|(edge, value)| PageDeclaration::Margin(edge, value))
                    .collect(),
                "margin-top" => vec![PageDeclaration::Margin(Edge::Top, keyword_or(input, length, bad)?)],
                "margin-right" => vec![PageDeclaration::Margin(Edge::Right, keyword_or(input, length, bad)?)],
                "margin-bottom" => vec![PageDeclaration::Margin(Edge::Bottom, keyword_or(input, length, bad)?)],
                "margin-left" => vec![PageDeclaration::Margin(Edge::Left, keyword_or(input, length, bad)?)],
                _ => return Err(input.new_custom_error(StyleError::UnsupportedProperty(name.clone()))),
            };
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
            let bad = || StyleError::UnsupportedValue(name.clone());
            let declarations = if name.eq_ignore_ascii_case("content") {
                vec![MarginDeclaration::Content(keyword_or(input, content, bad)?)]
            } else {
                property(&name, input)?
                    .into_iter()
                    .map(MarginDeclaration::Style)
                    .collect()
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

/// What a margin box paints: nothing, the page number, or a literal.
fn content(input: &mut Parser<'_, '_>) -> Option<Content> {
    if let Ok(text) = input.try_parse(|input| input.expect_string().cloned()) {
        return Some(Content::Text(text.as_ref().to_string()));
    }
    if let Ok(keyword) = input.try_parse(|input| input.expect_ident().cloned()) {
        return keyword
            .eq_ignore_ascii_case("none")
            .then_some(Content::None);
    }
    // `counter(page)`: the folio, and the only counter there is.
    let function = input.expect_function().ok()?.clone();
    if !function.eq_ignore_ascii_case("counter") {
        return None;
    }
    input
        .parse_nested_block(|input| {
            input
                .expect_ident_matching("page")
                .map(|_| Content::PageNumber)
                .map_err(ParseError::<StyleError<'_>>::from)
        })
        .ok()
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
    let mm = |value: f32| value * 72.0 / 25.4;
    let inch = |value: f32| value * 72.0;
    match_ignore_ascii_case! { keyword,
        "a3" => Some((mm(297.0), mm(420.0))),
        "a4" => Some((mm(210.0), mm(297.0))),
        "a5" => Some((mm(148.0), mm(210.0))),
        "b4" => Some((mm(250.0), mm(353.0))),
        "b5" => Some((mm(176.0), mm(250.0))),
        "letter" => Some((inch(8.5), inch(11.0))),
        "legal" => Some((inch(8.5), inch(14.0))),
        "ledger" => Some((inch(11.0), inch(17.0))),
        _ => None,
    }
}

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
        match result {
            Ok(FaceDeclaration::Family(family)) => face.family = family,
            Ok(FaceDeclaration::Style(style)) => face.style = Some(style),
            Ok(FaceDeclaration::Weight(weight)) => face.weight = Some(weight),
            Ok(FaceDeclaration::Src(src)) => face.src = src,
            Err(error) => warnings.push(warning(sheet, &error)),
        }
    }
    (face, warnings)
}

/// The body of one `@font-face`.
struct FontFaceBody;

/// One `@font-face` descriptor.
enum FaceDeclaration {
    Family(String),
    Style(FontStyle),
    Weight(u16),
    Src(Vec<Src>),
}

impl<'i> DeclarationParser<'i> for FontFaceBody {
    type Declaration = FaceDeclaration;
    type Error = StyleError<'i>;

    fn parse_value<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
        start: &ParserState,
    ) -> Result<Self::Declaration, ParseError<'i, Self::Error>> {
        at(start, |input| {
            let bad = || StyleError::UnsupportedValue(name.clone());
            let declaration = match_ignore_ascii_case! { &name,
                "font-family" => FaceDeclaration::Family(match families(input)?.first() {
                    Some(Family::Named(family)) => family.clone(),
                    Some(Family::Generic(generic)) => generic.keyword().to_string(),
                    None => return Err(input.new_custom_error(bad())),
                }),
                "font-style" => FaceDeclaration::Style(keyword_or(input, font_style, bad)?),
                "font-weight" => FaceDeclaration::Weight(keyword_or(input, weight, bad)?),
                "src" => FaceDeclaration::Src(sources(input)?),
                _ => return Err(input.new_custom_error(StyleError::UnsupportedProperty(name.clone()))),
            };
            input.expect_exhausted()?;
            Ok(declaration)
        })(input)
    }
}

impl<'i> AtRuleParser<'i> for FontFaceBody {
    type Prelude = ();
    type AtRule = FaceDeclaration;
    type Error = StyleError<'i>;
}

impl<'i> QualifiedRuleParser<'i> for FontFaceBody {
    type Prelude = ();
    type QualifiedRule = FaceDeclaration;
    type Error = StyleError<'i>;
}

impl<'i> RuleBodyItemParser<'i, FaceDeclaration, StyleError<'i>> for FontFaceBody {
    fn parse_declarations(&self) -> bool {
        true
    }

    fn parse_qualified(&self) -> bool {
        false
    }
}

/// A `src` list: urls for the host to resolve, or faces it may
/// already hold. `format()` hints are read and dropped — the loader
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
