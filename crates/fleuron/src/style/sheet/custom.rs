//! Custom properties and `var()`: a value kept as the text it was
//! written as, and the substitution that turns it into a value the
//! property parser reads.

use cssparser::{CowRcStr, Delimiter, ParseError, Parser, ParserInput, Token};

use crate::style::properties::{Declaration, Pending};

use super::declaration::{PROPERTIES, Spec};
use super::page::{MARGIN_BOX_PROPERTIES, PAGE_PROPERTIES};
use super::{MarginDeclaration, PageDeclaration};

/// Why a pending value became no declarations.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Unresolved {
    /// `var()` named a custom property that holds nothing, and gave no
    /// fallback.
    Missing(String),
    /// The value is not one the property takes.
    Unsupported,
}

/// Whether a value holds `var()` anywhere, inside another function
/// included. The input is left where it was.
pub(super) fn mentions_var(input: &mut Parser<'_, '_>) -> bool {
    let state = input.state();
    let found = scan(input);
    input.reset(&state);
    found
}

fn scan(input: &mut Parser<'_, '_>) -> bool {
    loop {
        let nested = match input.next() {
            Err(_) => return false,
            Ok(Token::Function(name)) if name.eq_ignore_ascii_case("var") => return true,
            Ok(
                Token::Function(_)
                | Token::ParenthesisBlock
                | Token::SquareBracketBlock
                | Token::CurlyBracketBlock,
            ) => true,
            Ok(_) => false,
        };
        let inside = nested
            && input
                .parse_nested_block(|input| {
                    let found = scan(input);
                    while input.next().is_ok() {}
                    Ok::<_, ParseError<'_, ()>>(found)
                })
                .unwrap_or(false);
        if inside {
            return true;
        }
    }
}

/// The rest of a declaration's value as written, up to `!important`.
pub(super) fn raw(input: &mut Parser<'_, '_>) -> String {
    let from = input.position();
    let _ = input.parse_until_before(Delimiter::Bang, |input| {
        while input.next().is_ok() {}
        Ok::<_, ParseError<'_, ()>>(())
    });
    input.slice_from(from).trim().to_string()
}

/// `css` with every `var()` replaced by what `lookup` says its name
/// holds, or by its fallback where the name holds nothing.
pub(crate) fn substitute(
    css: &str,
    lookup: &mut dyn FnMut(&str) -> Option<String>,
) -> Result<String, Unresolved> {
    let mut input = ParserInput::new(css);
    let mut parser = Parser::new(&mut input);
    let mut out = String::with_capacity(css.len());
    replace(&mut parser, lookup, &mut out)?;
    Ok(out)
}

fn replace(
    input: &mut Parser<'_, '_>,
    lookup: &mut dyn FnMut(&str) -> Option<String>,
    out: &mut String,
) -> Result<(), Unresolved> {
    loop {
        let start = input.position();
        let close = match input.next_including_whitespace_and_comments() {
            Err(_) => return Ok(()),
            Ok(Token::Function(name)) if name.eq_ignore_ascii_case("var") => None,
            Ok(Token::Function(_) | Token::ParenthesisBlock) => Some(")"),
            Ok(Token::SquareBracketBlock) => Some("]"),
            Ok(Token::CurlyBracketBlock) => Some("}"),
            Ok(_) => {
                out.push_str(input.slice_from(start));
                continue;
            }
        };
        match close {
            None => nested(input, |input| var(input, lookup, out))?,
            Some(close) => {
                out.push_str(input.slice_from(start));
                nested(input, |input| replace(input, lookup, out))?;
                out.push_str(close);
            }
        }
    }
}

/// Runs `read` over the block the parser just opened, and moves past
/// the end of the block whatever `read` left unread.
fn nested<'i>(
    input: &mut Parser<'i, '_>,
    read: impl FnOnce(&mut Parser<'i, '_>) -> Result<(), Unresolved>,
) -> Result<(), Unresolved> {
    let mut outcome = Ok(());
    let _ = input.parse_nested_block(|input| {
        outcome = read(input);
        while input.next().is_ok() {}
        Ok::<_, ParseError<'i, ()>>(())
    });
    outcome
}

/// The inside of one `var( --<name> [, <fallback> ]? )`.
fn var(
    input: &mut Parser<'_, '_>,
    lookup: &mut dyn FnMut(&str) -> Option<String>,
    out: &mut String,
) -> Result<(), Unresolved> {
    let name = match input.expect_ident() {
        Ok(name) if name.starts_with("--") => name.to_string(),
        _ => return Err(Unresolved::Unsupported),
    };
    let fallback = match input.next() {
        Err(_) => false,
        Ok(Token::Comma) => true,
        Ok(_) => return Err(Unresolved::Unsupported),
    };
    if let Some(value) = lookup(&name) {
        out.push_str(&value);
        return Ok(());
    }
    if fallback {
        input.skip_whitespace();
        return replace(input, lookup, out);
    }
    Err(Unresolved::Missing(name))
}

/// Reads `css` as a whole value of `property`.
fn read<D>(specs: &[Spec<D>], property: &str, css: &str) -> Option<Vec<D>> {
    let spec = Spec::find(specs, property)?;
    let mut input = ParserInput::new(css);
    let mut parser = Parser::new(&mut input);
    let name = CowRcStr::from(spec.name);
    parser.parse_entirely(|input| spec.read(&name, input)).ok()
}

/// What a declaration of `property` sets, read off a value that
/// parses. The values do not matter, only which longhands they are.
fn template<D>(specs: &[Spec<D>], property: &str) -> Vec<D> {
    Spec::find(specs, property)
        .and_then(|spec| read(specs, property, spec.examples.first()?))
        .unwrap_or_default()
}

/// The declarations of a style rule that `pending` comes to, once
/// `css` has replaced its value.
pub(crate) fn read_pending(pending: &Pending, css: &str) -> Option<Vec<Declaration>> {
    let mut declarations = read(PROPERTIES, &pending.property, css)?;
    for declaration in &mut declarations {
        pin(declaration, pending);
    }
    Some(declarations)
}

/// The same, for a declaration in a `@page` body.
pub(crate) fn read_pending_page(pending: &Pending, css: &str) -> Option<Vec<PageDeclaration>> {
    let mut declarations = read(PAGE_PROPERTIES, &pending.property, css)?;
    for declaration in &mut declarations {
        if let PageDeclaration::BackgroundImage(Some(url)) = declaration {
            url.origin = Some(pending.origin.clone());
        }
    }
    Some(declarations)
}

/// The same, for a declaration in a page margin box.
pub(crate) fn read_pending_margin(pending: &Pending, css: &str) -> Option<Vec<MarginDeclaration>> {
    if Spec::find(MARGIN_BOX_PROPERTIES, &pending.property).is_some() {
        return read(MARGIN_BOX_PROPERTIES, &pending.property, css);
    }
    let declarations = read_pending(pending, css)?;
    Some(
        declarations
            .into_iter()
            .map(MarginDeclaration::Style)
            .collect(),
    )
}

/// A url carries where it was written, for a warning about the image.
fn pin(declaration: &mut Declaration, pending: &Pending) {
    if let Declaration::BackgroundImage(Some(url)) = declaration {
        url.origin = Some(pending.origin.clone());
    }
}

/// The longhands a style rule's `property` sets.
pub(crate) fn longhands(property: &str) -> Vec<Declaration> {
    template(PROPERTIES, property)
}

/// The declarations a `@page` body's `property` sets.
pub(crate) fn page_longhands(property: &str) -> Vec<PageDeclaration> {
    template(PAGE_PROPERTIES, property)
}

/// The declarations a margin box's `property` sets.
pub(crate) fn margin_longhands(property: &str) -> Vec<MarginDeclaration> {
    match Spec::find(MARGIN_BOX_PROPERTIES, property) {
        Some(_) => template(MARGIN_BOX_PROPERTIES, property),
        None => longhands(property)
            .into_iter()
            .map(MarginDeclaration::Style)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn substituted(css: &str, names: &[(&str, &str)]) -> Result<String, Unresolved> {
        substitute(css, &mut |name| {
            names
                .iter()
                .find(|(known, _)| *known == name)
                .map(|(_, value)| value.to_string())
        })
    }

    /// Part: `var()` substitutes wherever a value is expected, inside
    /// another function included, and the rest of the value is kept as
    /// written.
    #[test]
    fn var_substitutes_inside_functions_and_keeps_the_rest() {
        let names = [("--accent", "#d6075e"), ("--gap", "12pt")];
        assert_eq!(
            substituted("1pt solid var(--accent)", &names).unwrap(),
            "1pt solid #d6075e"
        );
        assert_eq!(
            substituted("rgb(var(--r, 10), 20, 30)", &names).unwrap(),
            "rgb(10, 20, 30)"
        );
        assert_eq!(
            substituted("var(--gap) var( --gap )", &names).unwrap(),
            "12pt 12pt"
        );
        assert_eq!(
            substituted("\"var(--gap)\"", &names).unwrap(),
            "\"var(--gap)\""
        );
    }

    /// Part: a `var()` naming nothing takes its fallback, which can
    /// itself read a name, and with no fallback it names what was
    /// missing.
    #[test]
    fn a_var_naming_nothing_takes_its_fallback() {
        let names = [("--gap", "12pt")];
        assert_eq!(substituted("var(--missing, 12pt)", &names).unwrap(), "12pt");
        assert_eq!(
            substituted("var(--missing, var(--gap))", &names).unwrap(),
            "12pt"
        );
        assert_eq!(
            substituted("var(--missing)", &names),
            Err(Unresolved::Missing("--missing".into()))
        );
        assert_eq!(
            substituted("var(gap)", &names),
            Err(Unresolved::Unsupported)
        );
    }

    /// A shorthand's template names every longhand it sets.
    #[test]
    fn a_shorthand_template_names_its_longhands() {
        let longhands = longhands("margin");
        let names: Vec<&str> = longhands
            .iter()
            .map(|declaration| declaration.property())
            .collect();
        assert_eq!(
            names,
            ["margin-top", "margin-right", "margin-bottom", "margin-left"]
        );
    }
}
