//! The sheets as one EPUB stylesheet: the subset the engine reads,
//! less everything that describes a page.
//!
//! A reading system makes the pages of a reflowable EPUB itself, so
//! `@page`, running heads, page numbers, break control and the
//! placing of a box against the page have nothing to act on. Each
//! one the author wrote is a warning at the line and column it was
//! written at. The built-in sheet has paged rules too, and those are
//! left out without a word, because the author did not write them.

use cssparser::{Parser, ParserInput, Token, serialize_string};

use fleuron::Warning;
use fleuron::style::{
    FontFace, FontLoader, FontStyle, Origin, SheetPosition, Src, Stylesheets, Written, WrittenRule,
};

use crate::media::{Kind, Resources};
use crate::xhtml::refusal;

/// Properties that only act on a page: where pages break, what a
/// page's margins print, and what is put against the page rather than
/// in the text.
const PAGED_PROPERTIES: &[&str] = &[
    "page",
    "break-before",
    "break-after",
    "break-inside",
    "orphans",
    "widows",
    "box-decoration-break",
    "column-span",
    "string-set",
    "counter-reset",
    "wrap-flow",
    "shape-outside",
    "shape-margin",
];

/// Elements that only exist on a page: the foot of it that notes are
/// set in, and the breaks between pages and columns.
const PAGED_ELEMENTS: &[&str] = &["notes", "pagebreak", "columnbreak"];

/// Element names the XHTML spells differently.
const RENAMED: &[(&str, &str)] = &[("book", "body"), ("note", "aside")];

/// The alignment a table's source writes on a column. It goes after
/// the built-in rules and before the author's, and it weighs the same
/// as one class, so an author rule for the cells still overrides it.
const ALIGNMENT: &str = "\
:where(th, td)[data-align=\"left\"] { text-align: left; }
:where(th, td)[data-align=\"center\"] { text-align: center; }
:where(th, td)[data-align=\"right\"] { text-align: right; }
";

/// What the stylesheet translation needs from the rest of the book.
pub struct Context<'a> {
    pub resources: &'a mut Resources,
    pub images: &'a dyn fleuron::images::ImageLoader,
    pub fonts: &'a dyn FontLoader,
    pub warnings: &'a mut Vec<Warning>,
}

/// The EPUB stylesheet for `sheets`.
pub fn stylesheet(sheets: &Stylesheets, cx: &mut Context<'_>) -> String {
    let written = sheets.written();
    let mut out = String::new();
    for sheet in &written {
        for face in &sheet.faces {
            font_face(face, &mut out, cx);
        }
    }
    let mut aligned = false;
    for sheet in &written {
        let author = sheet.origin == Origin::Author;
        if author && !aligned {
            out.push_str(ALIGNMENT);
            aligned = true;
        }
        if author {
            for page in &sheet.pages {
                cx.warnings.push(Warning {
                    message: format!(
                        "Paged rule `{}`. A reading system makes the pages of an EPUB, so the EPUB leaves the rule out.",
                        page.selector
                    ),
                    origin: Some(page.position.to_string()),
                });
            }
        }
        for rule in &sheet.rules {
            style_rule(rule, author, &mut out, cx);
        }
    }
    if !aligned {
        out.push_str(ALIGNMENT);
    }
    out
}

fn warn(cx: &mut Context<'_>, author: bool, message: String, at: &SheetPosition) {
    if author {
        cx.warnings.push(Warning {
            message,
            origin: Some(at.to_string()),
        });
    }
}

fn style_rule(rule: &WrittenRule, author: bool, out: &mut String, cx: &mut Context<'_>) {
    let mut selectors = Vec::new();
    for selector in &rule.selectors {
        match translate_selector(selector) {
            Ok(selector) => selectors.push(selector),
            Err(element) => warn(
                cx,
                author,
                format!(
                    "Paged selector `{selector}`. A reading system has no `{element}`, so the EPUB leaves the selector out."
                ),
                &rule.position,
            ),
        }
    }
    if selectors.is_empty() {
        return;
    }
    let mut declarations = String::new();
    for declaration in &rule.declarations {
        if let Some(what) = paged(declaration) {
            warn(
                cx,
                author,
                format!(
                    "Paged {what}. A reading system makes the pages of an EPUB, so the EPUB leaves the declaration out."
                ),
                &declaration.position,
            );
            continue;
        }
        let Some(value) = urls(&declaration.value, cx, author, &declaration.position) else {
            continue;
        };
        declarations.push_str("  ");
        declarations.push_str(&declaration.property);
        declarations.push_str(": ");
        declarations.push_str(&value);
        if declaration.important {
            declarations.push_str(" !important");
        }
        declarations.push_str(";\n");
    }
    if declarations.is_empty() {
        return;
    }
    out.push_str(&selectors.join(",\n"));
    out.push_str(" {\n");
    out.push_str(&declarations);
    out.push_str("}\n");
}

/// Why a declaration only acts on a page, or `None` when it acts on
/// the text wherever the text is.
fn paged(declaration: &Written) -> Option<String> {
    let property = declaration.property.as_str();
    if PAGED_PROPERTIES.contains(&property) {
        return Some(format!("property `{property}`"));
    }
    let value = declaration.value.to_ascii_lowercase();
    let paged_value = match property {
        "position" => value.trim() == "absolute",
        "content" => value.contains("target-counter("),
        _ => false,
    };
    paged_value.then(|| format!("value `{property}: {}`", declaration.value))
}

/// A selector with the element names the XHTML spells, or the paged
/// element it names.
fn translate_selector(selector: &str) -> Result<String, String> {
    let mut input = ParserInput::new(selector);
    let mut parser = Parser::new(&mut input);
    let mut names = Vec::new();
    type_names(&mut parser, &mut names);

    let mut out = String::new();
    let mut at = 0;
    for (range, name) in names {
        let lower = name.to_ascii_lowercase();
        if PAGED_ELEMENTS.contains(&lower.as_str()) {
            return Err(lower);
        }
        if let Some((_, to)) = RENAMED.iter().find(|(from, _)| *from == lower) {
            out.push_str(&selector[at..range.start]);
            out.push_str(to);
            at = range.end;
        }
    }
    out.push_str(&selector[at..]);
    Ok(out)
}

/// Every identifier in a type selector's place: not after `.`, `:`
/// or `::`, and inside `:is()` and its relatives as well.
fn type_names(parser: &mut Parser<'_, '_>, out: &mut Vec<(std::ops::Range<usize>, String)>) {
    let mut after_marker = false;
    loop {
        let start = parser.position().byte_index();
        let Ok(token) = parser.next_including_whitespace() else {
            return;
        };
        let token = token.clone();
        if let Token::Ident(name) = &token
            && !after_marker
        {
            let end = parser.position().byte_index();
            out.push((start..end, name.to_string()));
        } else if matches!(token, Token::Function(_) | Token::ParenthesisBlock) {
            let _ = parser.parse_nested_block(|nested| {
                type_names(nested, out);
                Ok::<(), cssparser::ParseError<'_, ()>>(())
            });
        }
        after_marker = matches!(token, Token::Delim('.') | Token::Colon);
    }
}

/// A value with every `url()` in it pointing into the container, or
/// `None` when a url names a file the container could not take.
fn urls(value: &str, cx: &mut Context<'_>, author: bool, at: &SheetPosition) -> Option<String> {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let mut out = String::new();
    let mut from = 0;
    loop {
        let start = parser.position().byte_index();
        let Ok(token) = parser.next_including_whitespace_and_comments() else {
            break;
        };
        let token = token.clone();
        let url = if let Token::UnquotedUrl(url) = &token {
            Some(url.to_string())
        } else if let Token::Function(name) = &token
            && name.eq_ignore_ascii_case("url")
        {
            parser
                .parse_nested_block(|nested| {
                    let url = nested.expect_string()?.to_string();
                    Ok::<String, cssparser::ParseError<'_, ()>>(url)
                })
                .ok()
        } else {
            None
        };
        let Some(url) = url else {
            continue;
        };
        let end = parser.position().byte_index();
        let images = cx.images;
        match cx
            .resources
            .resolve(&url, Kind::Image, |url| images.load(url))
        {
            Ok(href) => {
                out.push_str(&value[from..start]);
                out.push_str("url(");
                string(&mut out, href);
                out.push(')');
                from = end;
            }
            Err(refused) => {
                warn(cx, author, refusal("Image", &url, refused), at);
                return None;
            }
        }
    }
    out.push_str(&value[from..]);
    Some(out)
}

fn font_face(face: &FontFace, out: &mut String, cx: &mut Context<'_>) {
    let mut sources = Vec::new();
    for src in &face.src {
        match src {
            Src::Url(url) => {
                let fonts = cx.fonts;
                match cx.resources.resolve(url, Kind::Font, |url| fonts.load(url)) {
                    Ok(href) => {
                        let mut source = String::from("url(");
                        string(&mut source, href);
                        source.push(')');
                        sources.push(source);
                    }
                    Err(refused) => cx.warnings.push(Warning {
                        message: refusal("Font", url, refused),
                        origin: None,
                    }),
                }
            }
            Src::Local(name) => {
                let mut source = String::from("local(");
                string(&mut source, name);
                source.push(')');
                sources.push(source);
            }
        }
    }
    if sources.is_empty() {
        return;
    }
    out.push_str("@font-face {\n  font-family: ");
    string(out, &face.family);
    out.push_str(";\n");
    if let Some(style) = face.style {
        out.push_str(match style {
            FontStyle::Normal => "  font-style: normal;\n",
            FontStyle::Italic => "  font-style: italic;\n",
        });
    }
    if let Some(weight) = face.weight {
        out.push_str(&format!("  font-weight: {weight};\n"));
    }
    out.push_str("  src: ");
    out.push_str(&sources.join(", "));
    out.push_str(";\n}\n");
}

/// `value` as a quoted CSS string.
fn string(out: &mut String, value: &str) {
    serialize_string(value, out).expect("a String takes every write");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selector_is_spelled_with_the_names_the_xhtml_uses() {
        let cases = [
            ("book > section", "body > section"),
            ("BOOK p", "body p"),
            ("note p:first-child", "aside p:first-child"),
            (":is(note, blockquote) p", ":is(aside, blockquote) p"),
            ("p.book", "p.book"),
            ("#note", "#note"),
            ("p:nth-child(odd of p)", "p:nth-child(odd of p)"),
            ("h2 + p::first-letter", "h2 + p::first-letter"),
        ];
        for (from, to) in cases {
            assert_eq!(translate_selector(from).as_deref(), Ok(to), "{from}");
        }
        assert_eq!(translate_selector("notes").unwrap_err(), "notes");
        assert_eq!(
            translate_selector("section > pagebreak").unwrap_err(),
            "pagebreak"
        );
        assert_eq!(
            translate_selector(":is(p, columnbreak)").unwrap_err(),
            "columnbreak"
        );
    }

    fn written(property: &str, value: &str) -> Written {
        Written {
            property: property.into(),
            value: value.into(),
            important: false,
            longhands: 0..1,
            position: SheetPosition::default(),
        }
    }

    #[test]
    fn a_declaration_that_only_acts_on_a_page_is_paged() {
        for (property, value) in [
            ("break-before", "page"),
            ("orphans", "2"),
            ("string-set", "chapter content()"),
            ("position", "absolute"),
            (
                "content",
                "\" (page \" target-counter(attr(href url), page) \")\"",
            ),
            ("wrap-flow", "end"),
        ] {
            assert!(paged(&written(property, value)).is_some(), "{property}");
        }
        for (property, value) in [
            ("position", "relative"),
            ("content", "\"\\2766\""),
            ("font-size", "12pt"),
            ("--break-before", "page"),
        ] {
            assert!(paged(&written(property, value)).is_none(), "{property}");
        }
    }
}
