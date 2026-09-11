//! Parsing: CSS text in, rules and diagnostics out.
//!
//! Nothing here matches or cascades. What it does decide is what the
//! engine claims to understand: a declaration outside the novel
//! subset does not become a rule, it becomes a warning naming the
//! position it was written at, and the rest of the sheet parses on.
//!
//! The parsers have a file each: `declaration` holds the property
//! table, `value` reads one value, `color` reads a colour, `page`
//! reads `@page`, `face` reads `@font-face`, and `vocabulary` is what
//! the subset document is written from.

use cssparser::{
    AtRuleParser, BasicParseErrorKind, CowRcStr, ParseError, ParseErrorKind, Parser, ParserInput,
    ParserState, QualifiedRuleParser, RuleBodyParser, SourceLocation, StyleSheetParser,
    match_ignore_ascii_case,
};
use selectors::SelectorList;
use selectors::parser::{ParseRelative, SelectorParseErrorKind};

use crate::Warning;
use crate::pages::Side;
use crate::style::element::{Fleuron, PseudoElement};
use crate::style::properties::{BorderStyle, Content, Declaration, Edge, Length, MarginBox};

mod color;
mod declaration;
mod face;
mod page;
mod value;
mod vocabulary;

pub use face::{FontFace, Src};

pub(crate) use color::NAMED;
pub(crate) use declaration::{PROPERTIES, Spec};
pub(crate) use face::FONT_FACE_DESCRIPTORS;
#[cfg(test)]
pub(crate) use face::FaceDeclaration;
pub(crate) use page::{MARGIN_BOX_PROPERTIES, PAGE_PROPERTIES, PAGE_SELECTORS, PAGE_SIZES};
pub(crate) use value::UNITS;
pub(crate) use vocabulary::{
    COMBINATORS, COMPOUNDS, DECLARATION, FIRST_LINE_PROPERTIES, PSEUDO_CLASSES, PSEUDO_ELEMENTS,
    SELECTOR_LIST,
};

use declaration::declarations;
use face::font_face;
use page::{PageBody, PageItem, page_selector};

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
    /// `column-count`, or `None` for `auto`.
    ColumnCount(Option<u32>),
    /// `column-width`, or `None` for `auto`.
    ColumnWidth(Option<Length>),
    /// `column-gap`, or `None` for `normal`.
    ColumnGap(Option<Length>),
    ColumnRuleWidth(Length),
    ColumnRuleStyle(BorderStyle),
}

/// A declaration inside a page margin box.
#[derive(Debug, Clone, PartialEq)]
pub enum MarginDeclaration {
    Content(Content),
    /// A text property; margin boxes set a line like any other.
    Style(Declaration),
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

/// What happens to a declaration the parser refuses.
const IGNORED_DECLARATION: &str = "The declaration is ignored.";

/// What happens to a rule the parser refuses.
const IGNORED_RULE: &str = "The rule is ignored.";

/// One parse error as the diagnostic a reader can act on: what the
/// engine did not understand, and where it was written.
pub fn warning(sheet: &str, error: &ParseError<'_, StyleError<'_>>) -> Warning {
    let message = match &error.kind {
        ParseErrorKind::Custom(StyleError::UnsupportedProperty(name)) => {
            format!("Unsupported property `{name}`. {IGNORED_DECLARATION}")
        }
        ParseErrorKind::Custom(StyleError::NotOnFirstLine(name)) => {
            format!("Unsupported property `{name}` on `::first-line`. {IGNORED_DECLARATION}")
        }
        ParseErrorKind::Custom(StyleError::UnsupportedValue(name)) => {
            format!("Unsupported value for `{name}`. {IGNORED_DECLARATION}")
        }
        ParseErrorKind::Custom(StyleError::UnsupportedAtRule(name)) => {
            format!("Unsupported at-rule `@{name}`. {IGNORED_RULE}")
        }
        ParseErrorKind::Custom(StyleError::UnsupportedPageSelector) => {
            format!("Unsupported `@page` selector. {IGNORED_RULE}")
        }
        ParseErrorKind::Custom(StyleError::Selector(
            SelectorParseErrorKind::UnsupportedPseudoClassOrElement(name),
        )) => format!("Unsupported selector `:{name}`. {IGNORED_RULE}"),
        ParseErrorKind::Custom(StyleError::Selector(_)) => {
            format!("Unsupported selector. {IGNORED_RULE}")
        }
        ParseErrorKind::Basic(BasicParseErrorKind::AtRuleInvalid(name)) => {
            format!("Unsupported at-rule `@{name}`. {IGNORED_RULE}")
        }
        ParseErrorKind::Basic(_) => format!("Malformed CSS. {IGNORED_DECLARATION}"),
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
            "before" => Ok(PseudoElement::Before),
            "after" => Ok(PseudoElement::After),
            _ => Err(location.new_custom_error(
                SelectorParseErrorKind::UnsupportedPseudoClassOrElement(name.clone()),
            )),
        }
    }
}
