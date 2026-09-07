//! The CSS subset as data.
//!
//! A host with a style editor needs to know what the engine accepts
//! before a sheet is sent: which properties, which values, which
//! selectors. This module describes that from the tables the parser
//! dispatches through, so the description and the parser cannot
//! drift: a property is in the description because the parser reads
//! it, and reads it because it is in the table.
//!
//! The `syntax` strings use CSS value-definition syntax. A bare word
//! is a keyword, `<name>` is a type, `name()` is a function. The
//! keywords are also listed on their own, for a host that would
//! rather complete them than parse the grammar; where one stands in
//! the grammar is still the grammar's to say.

use serde::{Deserialize, Serialize};

use crate::style::element::ELEMENTS;
use crate::style::properties::{CounterStyle, MarginBox};
use crate::style::sheet::{
    COMBINATORS, COMPOUNDS, DECLARATION, FIRST_LINE_PROPERTIES, FONT_FACE_DESCRIPTORS,
    MARGIN_BOX_PROPERTIES, NAMED, PAGE_PROPERTIES, PAGE_SELECTORS, PAGE_SIZES, PROPERTIES,
    PSEUDO_CLASSES, PSEUDO_ELEMENTS, SELECTOR_LIST, Spec, UNITS,
};

/// What the engine accepts, from the version that produced it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Subset {
    /// The engine version this description came from.
    pub version: String,
    /// What a style rule may select.
    pub selectors: Selectors,
    /// The shape of one declaration, in value-definition syntax.
    pub declaration: String,
    /// The properties a style rule may declare.
    pub properties: Vec<Property>,
    /// The `@page` rule.
    pub page: Page,
    /// The `@font-face` rule.
    pub font_face: FontFace,
    /// The units a `<length>` may carry.
    pub units: Vec<String>,
    /// The names a `<color>` may be.
    pub color_names: Vec<String>,
}

/// The selector vocabulary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Selectors {
    /// The element names, as the content tree produces them.
    pub elements: Vec<String>,
    /// What a compound is made of besides pseudo-classes.
    pub compounds: Vec<Selector>,
    /// The combinators between two compounds.
    pub combinators: Vec<Selector>,
    /// How selectors join in a list.
    pub list: Selector,
    /// The pseudo-classes, functional ones with their parentheses.
    pub pseudo_classes: Vec<Selector>,
    /// The pseudo-elements.
    pub pseudo_elements: Vec<Selector>,
    /// The properties `::first-line` takes, out of the properties a
    /// style rule may declare.
    pub first_line_properties: Vec<String>,
}

/// One piece of selector syntax, with a selector that uses it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Selector {
    /// The syntax as it is written.
    pub name: String,
    /// A whole selector that parses.
    pub example: String,
}

/// One property.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Property {
    /// The property name.
    pub name: String,
    /// Whether a child starts from the parent's value.
    pub inherited: bool,
    /// The values it accepts, in CSS value-definition syntax.
    pub syntax: String,
    /// The keywords in `syntax`, wherever they stand in it. One that
    /// only follows another value, like `landscape` after a page
    /// size, is not a value on its own.
    pub keywords: Vec<String>,
    /// Values that parse.
    pub examples: Vec<String>,
}

/// One `@font-face` descriptor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Descriptor {
    /// The descriptor name.
    pub name: String,
    /// The values it accepts, in CSS value-definition syntax.
    pub syntax: String,
    /// The keywords in `syntax`, wherever they stand in it.
    pub keywords: Vec<String>,
    /// Values that parse.
    pub examples: Vec<String>,
}

/// The `@page` rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    /// What follows `@page`, in value-definition syntax: a page name,
    /// then any of the page selectors.
    pub prelude: String,
    /// The page selectors, without their colon.
    pub selectors: Vec<String>,
    /// The properties a page body may declare.
    pub properties: Vec<Property>,
    /// The margin boxes a page body may open.
    pub margin_boxes: Vec<MarginBoxDescription>,
    /// The properties a margin box declares on top of the style
    /// properties, which it also accepts.
    pub margin_box_properties: Vec<Property>,
    /// The named sheets `size` accepts, portrait.
    pub sizes: Vec<PageSize>,
    /// The counter styles `counter(page, ...)` accepts.
    pub counter_styles: Vec<String>,
}

/// One margin box.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarginBoxDescription {
    /// The at-rule name, without the `@`.
    pub name: String,
    /// Whether the engine paints it. A box it does not paint is read
    /// and dropped.
    pub paints: bool,
}

/// One named page size.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageSize {
    /// The keyword.
    pub name: String,
    /// Width in points, portrait.
    pub width: f32,
    /// Height in points, portrait.
    pub height: f32,
}

/// The `@font-face` rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontFace {
    /// The descriptors a face body may declare.
    pub descriptors: Vec<Descriptor>,
}

impl Subset {
    /// The subset this build of the engine accepts.
    pub fn describe() -> Subset {
        Subset {
            version: env!("CARGO_PKG_VERSION").to_string(),
            selectors: Selectors {
                elements: strings(&ELEMENTS),
                compounds: selectors(COMPOUNDS),
                combinators: selectors(COMBINATORS),
                list: Selector {
                    name: SELECTOR_LIST.0.to_string(),
                    example: SELECTOR_LIST.1.to_string(),
                },
                pseudo_classes: selectors(PSEUDO_CLASSES),
                pseudo_elements: selectors(PSEUDO_ELEMENTS),
                first_line_properties: strings(FIRST_LINE_PROPERTIES),
            },
            declaration: DECLARATION.to_string(),
            properties: properties(PROPERTIES),
            page: Page {
                prelude: format!(
                    "<name>? [ {} ]*",
                    PAGE_SELECTORS
                        .iter()
                        .map(|(name, _)| format!(":{name}"))
                        .collect::<Vec<_>>()
                        .join(" | ")
                ),
                selectors: PAGE_SELECTORS
                    .iter()
                    .map(|(name, _)| name.to_string())
                    .collect(),
                properties: properties(PAGE_PROPERTIES),
                margin_boxes: MarginBox::ALL
                    .iter()
                    .map(|margin_box| MarginBoxDescription {
                        name: margin_box.keyword().to_string(),
                        paints: margin_box.band().is_some(),
                    })
                    .collect(),
                margin_box_properties: properties(MARGIN_BOX_PROPERTIES),
                sizes: PAGE_SIZES
                    .iter()
                    .map(|(name, (width, height))| PageSize {
                        name: name.to_string(),
                        width: *width,
                        height: *height,
                    })
                    .collect(),
                counter_styles: CounterStyle::ALL
                    .iter()
                    .map(|style| style.keyword().to_string())
                    .collect(),
            },
            font_face: FontFace {
                descriptors: FONT_FACE_DESCRIPTORS
                    .iter()
                    .map(|spec| Descriptor {
                        name: spec.name.to_string(),
                        syntax: spec.syntax.to_string(),
                        keywords: keywords(spec.syntax),
                        examples: strings(spec.examples),
                    })
                    .collect(),
            },
            units: UNITS.iter().map(|(name, _)| name.to_string()).collect(),
            color_names: NAMED.iter().map(|(name, _)| name.to_string()).collect(),
        }
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

fn selectors(table: &[(&str, &str)]) -> Vec<Selector> {
    table
        .iter()
        .map(|(name, example)| Selector {
            name: name.to_string(),
            example: example.to_string(),
        })
        .collect()
}

fn properties<D>(specs: &[Spec<D>]) -> Vec<Property> {
    specs
        .iter()
        .map(|spec| Property {
            name: spec.name.to_string(),
            inherited: spec.inherited,
            syntax: spec.syntax.to_string(),
            keywords: keywords(spec.syntax),
            examples: strings(spec.examples),
        })
        .collect()
}

/// The keywords in a value-definition: the identifiers that are not
/// a type, not a function name, and not inside a function's
/// parentheses or a multiplier's braces.
fn keywords(syntax: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut depth = 0u32;
    let mut in_type = false;
    let mut in_braces = false;
    for c in syntax.chars() {
        if c.is_ascii_alphanumeric() || c == '-' {
            word.push(c);
            continue;
        }
        if !word.is_empty() {
            let keyword = !in_type
                && !in_braces
                && depth == 0
                && c != '('
                && !word.starts_with(|c: char| c.is_ascii_digit());
            if keyword && !found.contains(&word) {
                found.push(word.clone());
            }
            word.clear();
        }
        match c {
            '<' => in_type = true,
            '>' => in_type = false,
            '{' => in_braces = true,
            '}' => in_braces = false,
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    if !word.is_empty()
        && !in_type
        && !in_braces
        && depth == 0
        && !word.starts_with(|c: char| c.is_ascii_digit())
        && !found.contains(&word)
    {
        found.push(word);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::element::ElementTree;
    use crate::style::properties::{Declaration, Edge};
    use crate::style::sheet::{FaceDeclaration, MarginDeclaration, PageDeclaration};
    use cssparser::{CowRcStr, Parser, ParserInput};
    use std::collections::BTreeSet;

    #[test]
    fn keywords_skip_types_functions_and_multipliers() {
        assert_eq!(
            keywords(
                "none | <string> | counter(page) | counter(page, <counter-style>) | string(<name>)"
            ),
            vec!["none"]
        );
        assert_eq!(
            keywords("<length>{1,2} | <page-size> [ portrait | landscape ]?"),
            vec!["portrait", "landscape"]
        );
        assert_eq!(
            keywords("normal | bold | <number [1,1000]>"),
            vec!["normal", "bold"]
        );
        assert_eq!(
            keywords("[ <family-name> | serif | sans-serif | monospace ]#"),
            vec!["serif", "sans-serif", "monospace"]
        );
        assert_eq!(
            keywords("[ <url> format(<string>)? | local(<string>) ]#"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn version_is_the_crate_version() {
        assert_eq!(Subset::describe().version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn round_trips_through_json() {
        let subset = Subset::describe();
        let json = serde_json::to_string(&subset).unwrap();
        let read: Subset = serde_json::from_str(&json).unwrap();
        assert_eq!(read, subset);
    }

    /// Every example a row names reads, through that row, into a
    /// declaration of the property the row names: a shorthand into
    /// its longhands, a longhand into itself.
    #[test]
    fn examples_read_into_their_own_property() {
        fn check<D>(specs: &[Spec<D>], property_of: fn(&D) -> &'static str) {
            for spec in specs {
                for example in spec.examples {
                    let mut input = ParserInput::new(example);
                    let mut parser = Parser::new(&mut input);
                    let name = CowRcStr::from(spec.name);
                    let declarations = spec
                        .read(&name, &mut parser)
                        .unwrap_or_else(|error| panic!("{}: {example}: {error:?}", spec.name));
                    assert!(
                        parser.expect_exhausted().is_ok(),
                        "{}: {example} left input unread",
                        spec.name
                    );
                    assert!(!declarations.is_empty(), "{}: {example}", spec.name);
                    for declaration in &declarations {
                        let read = property_of(declaration);
                        let longhand_of = |shorthand: &str| {
                            read.strip_prefix(shorthand)
                                .is_some_and(|rest| rest.starts_with('-'))
                        };
                        assert!(
                            read == spec.name || longhand_of(spec.name),
                            "{}: {example} read as {read}",
                            spec.name
                        );
                    }
                }
            }
        }
        check(PROPERTIES, property_of);
        check(PAGE_PROPERTIES, |declaration| match declaration {
            PageDeclaration::Size(..) => "size",
            PageDeclaration::Margin(edge, _) => margin_of(*edge),
        });
        check(MARGIN_BOX_PROPERTIES, |declaration| match declaration {
            MarginDeclaration::Content(_) => "content",
            MarginDeclaration::Style(declaration) => property_of(declaration),
        });
        check(FONT_FACE_DESCRIPTORS, |declaration| match declaration {
            FaceDeclaration::Family(_) => "font-family",
            FaceDeclaration::Style(_) => "font-style",
            FaceDeclaration::Weight(_) => "font-weight",
            FaceDeclaration::Src(_) => "src",
        });
    }

    fn margin_of(edge: Edge) -> &'static str {
        match edge {
            Edge::Top => "margin-top",
            Edge::Right => "margin-right",
            Edge::Bottom => "margin-bottom",
            Edge::Left => "margin-left",
        }
    }

    fn property_of(declaration: &Declaration) -> &'static str {
        match declaration {
            Declaration::FontFamily(_) => "font-family",
            Declaration::Color(_) => "color",
            Declaration::FontSize(_) => "font-size",
            Declaration::FontStyle(_) => "font-style",
            Declaration::FontWeight(_) => "font-weight",
            Declaration::LineHeight(_) => "line-height",
            Declaration::LetterSpacing(_) => "letter-spacing",
            Declaration::FontVariantCaps(_) => "font-variant-caps",
            Declaration::TextTransform(_) => "text-transform",
            Declaration::TextAlign(_) => "text-align",
            Declaration::TextJustify(_) => "text-justify",
            Declaration::TextIndent(_) => "text-indent",
            Declaration::HangingPunctuation(_) => "hanging-punctuation",
            Declaration::Hyphens(_) => "hyphens",
            Declaration::Orphans(_) => "orphans",
            Declaration::Widows(_) => "widows",
            Declaration::Page(_) => "page",
            Declaration::Content(_) => "content",
            Declaration::StringSet(_) => "string-set",
            Declaration::CounterReset(_) => "counter-reset",
            Declaration::InitialLetter(_) => "initial-letter",
            Declaration::Margin(edge, _) => margin_of(*edge),
            Declaration::BreakBefore(_) => "break-before",
            Declaration::BreakAfter(_) => "break-after",
            Declaration::BreakInside(_) => "break-inside",
        }
    }

    /// The element names the tree produces are the ones the description
    /// lists, no more and no fewer.
    #[test]
    fn elements_are_the_ones_the_tree_builds() {
        let inlines = r#"[
            {"type": "text", "value": "a "},
            {"type": "emphasis", "children": [{"type": "text", "value": "b"}]},
            {"type": "strong", "children": [{"type": "text", "value": "c"}]},
            {"type": "code", "value": "d"},
            {"type": "link", "url": "x", "children": [{"type": "text", "value": "e"}]}
        ]"#;
        let headings: Vec<String> = (1..=6)
            .map(|level| {
                format!(r#"{{"type": "heading", "level": {level}, "inlines": {inlines}}}"#)
            })
            .collect();
        let json = format!(
            r#"{{"metadata": {{}}, "sections": [{{"blocks": [
                {},
                {{"type": "paragraph", "inlines": {inlines}}},
                {{"type": "blockquote", "blocks": [{{"type": "paragraph", "inlines": {inlines}}}]}},
                {{"type": "thematic_break"}},
                {{"type": "image", "url": "image.png", "alt": "alt"}}
            ]}}]}}"#,
            headings.join(",\n")
        );
        let mut book: crate::content::Book = serde_json::from_str(&json).unwrap();
        book.assign_node_ids();
        let built: BTreeSet<&str> = ElementTree::build(&book)
            .nodes()
            .iter()
            .map(|node| node.name)
            .collect();
        let listed: BTreeSet<&str> = ELEMENTS.iter().copied().collect();
        assert_eq!(built, listed);
    }
}
