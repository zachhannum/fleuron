//! `@font-face`: the descriptors of one face, and where its file
//! comes from.

use cssparser::{
    AtRuleParser, CowRcStr, DeclarationParser, ParseError, Parser, ParserState,
    QualifiedRuleParser, RuleBodyItemParser, RuleBodyParser,
};

use crate::Warning;
use crate::style::properties::{Family, FontStyle};

use super::declaration::{Spec, at, longhand};
use super::value::{families, font_style, weight};
use super::{StyleError, warning};

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

/// One `@font-face` body.
pub(super) fn font_face(input: &mut Parser<'_, '_>, sheet: &str) -> (FontFace, Vec<Warning>) {
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
