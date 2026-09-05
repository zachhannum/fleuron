//! The style tree: resolved styling.
//!
//! Styling enters as CSS. A built-in user-agent stylesheet supplies
//! the defaults; author CSS cascades over it. What comes out is one
//! computed style per content node and one page master per situation
//! a page can be in, and that is the whole of what layout is told.
//!
//! ```text
//! content tree + CSS ─► style tree ─► box tree ─► …
//! ```
//!
//! Font faces reach the registry here too: `@font-face` names a
//! source, the host loader hands back bytes, and every node's face is
//! an id by the time layout sees it.

mod element;
mod properties;
mod sheet;

use std::collections::BTreeMap;

use selectors::context::{
    MatchingForInvalidation, MatchingMode, NeedsSelectorFlags, QuirksMode, SelectorCaches,
};
use selectors::matching::{MatchingContext, matches_selector};
use serde::Serialize;

use crate::Warning;
use crate::content::{Book, NodeId};
use crate::fonts::{FaceAttributes, FontRegistry, FontSource};
use crate::lines::{InlineStyles, ParagraphStyle};
use crate::pages::Side;

pub use properties::{
    Align, Band, Break, Color, ComputedStyle, Content, CounterStyle, Edge, Edges, Family,
    FontStyle, FontVariantCaps, Hyphens, Length, LineHeight, MarginBox, PageGeometry, StringPiece,
    StringSet, TextAlign, TextJustify, TextTransform,
};
pub use sheet::{Origin, Source};

use element::{ElementTree, PseudoElement};
use sheet::{FontFace, Importance, MarginDeclaration, PageDeclaration, PageRule, Sheet, Src};

/// The defaults, as a stylesheet. There are no style constants in the
/// engine; this file is where the trade paperback lives.
pub const USER_AGENT_CSS: &str = include_str!("style/ua.css");

/// Resolves `@font-face` sources to font bytes.
///
/// The engine reads no paths of its own. Whatever string the sheet
/// writes in `url()` is handed over as it is: it never has to be a
/// real URL, and a host that resolves nothing is a host with no
/// author fonts.
pub trait FontLoader {
    /// The bytes behind one `src` url, or `None` when the host cannot
    /// resolve it.
    fn load(&self, url: &str) -> Option<Vec<u8>>;
}

/// A loader that resolves nothing.
pub struct NoFonts;

impl FontLoader for NoFonts {
    fn load(&self, _url: &str) -> Option<Vec<u8>> {
        None
    }
}

/// One node's place in the tree: what it is, and which style it got.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NodeStyle {
    /// The content node this style belongs to.
    pub id: u32,
    /// The element name selectors matched against.
    pub element: &'static str,
    /// Index into the tree's distinct styles.
    pub style: u32,
    /// Index of the style `::first-letter` computed for this node,
    /// when a rule named one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_letter: Option<u32>,
}

/// The situation a page finds itself in, which is what `@page`
/// selects on beyond the page's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Situation {
    /// The page a page group opens on: `@page :first`.
    First(Side),
    /// Any later page of the group.
    Body(Side),
    /// A page inserted to square the sheet: `@page :blank`.
    Blank,
}

impl Situation {
    /// The side of the spread this situation falls on. Blanks are
    /// versos: a book never opens a leaf to a blank right-hand page.
    pub fn side(self) -> Side {
        match self {
            Situation::First(side) | Situation::Body(side) => side,
            Situation::Blank => Side::Verso,
        }
    }
}

/// Which page a `@page` lookup is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageQuery<'a> {
    /// The named page in force, from the `page` property.
    pub name: Option<&'a str>,
    /// Which page of the group, and which side.
    pub situation: Situation,
}

/// One resolved page master: the page box, and what its margin boxes
/// paint.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PageStyle {
    /// Trim size and margins.
    pub geometry: PageGeometry,
    /// The margin boxes the page's rules mentioned, in CSS order.
    pub boxes: Vec<MarginBoxStyle>,
}

impl PageStyle {
    /// The margin box `which`, when it paints anything.
    pub fn margin_box(&self, which: MarginBox) -> Option<&MarginBoxStyle> {
        self.boxes
            .iter()
            .find(|box_| box_.which == which && box_.content != Content::None)
    }
}

/// One page margin box: what it paints and the style it paints with.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MarginBoxStyle {
    /// Which of the sixteen boxes.
    pub which: MarginBox,
    /// What it paints.
    pub content: Content,
    /// The box's own text style, inherited from the book's root the
    /// way a block's would be.
    pub style: ComputedStyle,
}

/// One page master, named and situated.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PageMaster {
    /// The named page, or `None` for the unnamed default.
    pub page: Option<String>,
    /// The situation this master answers.
    pub situation: Situation,
    /// The page box and its margin boxes.
    pub style: PageStyle,
}

/// The compiled styling of one book.
#[derive(Debug, Clone, Serialize)]
pub struct StyleTree {
    /// The distinct computed styles this book uses. Nodes index into
    /// it: a book has thousands of nodes and a handful of styles.
    styles: Vec<ComputedStyle>,
    /// Every element, in document order.
    nodes: Vec<NodeStyle>,
    /// Every master a page of this book can take.
    masters: Vec<PageMaster>,
    /// Style index by raw node id. Index 0 is the root, which is also
    /// what an unassigned node resolves to.
    #[serde(skip)]
    by_node: Vec<u32>,
    /// `::first-letter` style index by raw node id, where there is one.
    #[serde(skip)]
    initial_by_node: Vec<Option<u32>>,
    warnings: Vec<Warning>,
}

impl StyleTree {
    /// The computed style of one content node. An id the tree does
    /// not know — an unassigned one, or a node from another book —
    /// gets the root style, which is the book's own defaults.
    pub fn style(&self, id: NodeId) -> &ComputedStyle {
        let index = self
            .by_node
            .get(id.get() as usize)
            .copied()
            .unwrap_or_default();
        &self.styles[index as usize]
    }

    /// Everything line layout needs about one node.
    pub fn paragraph(&self, id: NodeId) -> ParagraphStyle {
        self.style(id).paragraph()
    }

    /// The style `::first-letter` computed for one node, when a rule
    /// named one. This is where a drop cap's size and face come from.
    pub fn first_letter(&self, id: NodeId) -> Option<&ComputedStyle> {
        let index = (*self.initial_by_node.get(id.get() as usize)?)?;
        Some(&self.styles[index as usize])
    }

    /// The style of the book itself: the root of inheritance.
    pub fn root(&self) -> &ComputedStyle {
        &self.styles[0]
    }

    /// The distinct computed styles, in the order nodes index them.
    pub fn styles(&self) -> &[ComputedStyle] {
        &self.styles
    }

    /// Every element, in document order.
    pub fn nodes(&self) -> &[NodeStyle] {
        &self.nodes
    }

    /// What compilation had to complain about.
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    /// The page master one page resolves to. A named page with no
    /// master of its own falls back to the unnamed one.
    pub fn page(&self, query: PageQuery<'_>) -> &PageStyle {
        self.master(query.name, query.situation)
            .or_else(|| self.master(None, query.situation))
            .expect("every situation has an unnamed master")
    }

    /// The default page: an unnamed body recto. The measure every
    /// paragraph breaks to comes from here.
    pub fn default_page(&self) -> &PageStyle {
        self.page(PageQuery {
            name: None,
            situation: Situation::Body(Side::Recto),
        })
    }

    /// Every master this book can put on a page: one per named page
    /// and situation, in a stable order.
    pub fn masters(&self) -> &[PageMaster] {
        &self.masters
    }

    fn master(&self, name: Option<&str>, situation: Situation) -> Option<&PageStyle> {
        self.masters
            .iter()
            .find(|master| master.page.as_deref() == name && master.situation == situation)
            .map(|master| &master.style)
    }
}

/// Resolves every master the book can use: the unnamed default, plus
/// each named page, in each situation a page can be in.
fn resolve_masters(
    pages: &[(u8, PageRule)],
    styles: &[ComputedStyle],
    root: &ComputedStyle,
    registry: &FontRegistry,
    warnings: &mut Vec<Warning>,
) -> Vec<PageMaster> {
    let mut names: Vec<Option<String>> = vec![None];
    for style in styles {
        if let Some(name) = &style.page
            && !names.iter().any(|known| known.as_deref() == Some(name))
        {
            names.push(Some(name.clone()));
        }
    }
    let situations = [
        Situation::First(Side::Recto),
        Situation::First(Side::Verso),
        Situation::Body(Side::Recto),
        Situation::Body(Side::Verso),
        Situation::Blank,
    ];
    let mut masters = Vec::new();
    for name in names {
        for situation in situations {
            masters.push(PageMaster {
                page: name.clone(),
                situation,
                style: resolve_page(
                    pages,
                    PageQuery {
                        name: name.as_deref(),
                        situation,
                    },
                    root,
                    registry,
                    warnings,
                ),
            });
        }
    }
    masters
}

/// One page master: every `@page` rule that selects it, applied in
/// specificity then source order.
fn resolve_page(
    pages: &[(u8, PageRule)],
    query: PageQuery<'_>,
    root: &ComputedStyle,
    registry: &FontRegistry,
    warnings: &mut Vec<Warning>,
) -> PageStyle {
    let mut matching: Vec<&(u8, PageRule)> = pages
        .iter()
        .filter(|(_, rule)| selects(rule, query))
        .collect();
    // `@page` cascades like any other rule: origin first, then
    // specificity. The rules are already in source order, and a stable
    // sort leaves the later of two equal rules on top.
    matching.sort_by_key(|(level, rule)| (*level, rule.specificity()));

    let root_size = root.font_size;
    let mut geometry = PageGeometry {
        width: 612.0,
        height: 792.0,
        margin: Edges::all(0.0),
    };
    let mut boxes: BTreeMap<MarginBox, MarginBoxStyle> = BTreeMap::new();
    for (_, rule) in matching {
        for declaration in &rule.declarations {
            match declaration {
                PageDeclaration::Size(width, height) => {
                    geometry.width = *width;
                    geometry.height = *height;
                }
                PageDeclaration::Margin(edge, length) => {
                    let points = length.to_points(root_size, root_size);
                    match edge {
                        Edge::Top => geometry.margin.top = points,
                        Edge::Right => geometry.margin.right = points,
                        Edge::Bottom => geometry.margin.bottom = points,
                        Edge::Left => geometry.margin.left = points,
                    }
                }
            }
        }
        for (which, declarations) in &rule.boxes {
            let entry = boxes.entry(*which).or_insert_with(|| MarginBoxStyle {
                which: *which,
                content: Content::None,
                style: root.inherit(),
            });
            for declaration in declarations {
                match declaration {
                    MarginDeclaration::Content(content) => entry.content = content.clone(),
                    MarginDeclaration::Style(style) => {
                        entry.style.apply(style, root_size, root_size)
                    }
                }
            }
            let (font_id, warning) = resolve_face(&entry.style, registry);
            entry.style.font_id = font_id;
            report(warnings, warning);
        }
    }
    PageStyle {
        geometry,
        boxes: boxes.into_values().collect(),
    }
}

impl InlineStyles for StyleTree {
    fn style(&self, id: NodeId, block: ParagraphStyle) -> ParagraphStyle {
        match self.by_node.get(id.get() as usize) {
            Some(index) => self.styles[*index as usize].paragraph(),
            None => block,
        }
    }
}

/// Whether one `@page` rule selects the page a query describes.
fn selects(rule: &PageRule, query: PageQuery<'_>) -> bool {
    if rule.name.is_some() && rule.name.as_deref() != query.name {
        return false;
    }
    if rule.first && !matches!(query.situation, Situation::First(_)) {
        return false;
    }
    if rule.blank && query.situation != Situation::Blank {
        return false;
    }
    match rule.side {
        Some(side) => side == query.situation.side(),
        None => true,
    }
}

/// Parsed stylesheets: the built-in sheet, then whatever the host
/// added, ready to compile against a book.
///
/// Parsing is separate from compiling because a run styles several
/// books from one set of sheets, and separate from font loading
/// because loading is the one step that reaches outside the engine.
#[derive(Debug)]
pub struct Stylesheets {
    sheets: Vec<Sheet>,
    warnings: Vec<Warning>,
}

impl Stylesheets {
    /// Parses the built-in sheet, then `sources` in the order given.
    pub fn parse(sources: &[Source<'_>]) -> Stylesheets {
        let built_in = Source::user_agent("user-agent.css", USER_AGENT_CSS);
        let mut sheets = Vec::new();
        let mut warnings = Vec::new();
        for source in std::iter::once(&built_in).chain(sources) {
            let (sheet, mut sheet_warnings) = sheet::parse(source);
            warnings.append(&mut sheet_warnings);
            sheets.push(sheet);
        }
        Stylesheets { sheets, warnings }
    }

    /// Registers every `@font-face` the loader resolves, under the
    /// family the sheet gave it rather than the one inside the file:
    /// a stylesheet's name for a face is the one its selectors use.
    ///
    /// Run this before compiling — a face the registry does not have
    /// is a face no computed style can resolve to.
    pub fn load_fonts(&mut self, registry: &mut FontRegistry, loader: &dyn FontLoader) {
        for sheet in &self.sheets {
            for face in &sheet.faces {
                self.warnings.extend(register_face(face, registry, loader));
            }
        }
    }

    /// What parsing and font loading had to complain about.
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    /// Compiles one book's styling against the faces in the registry.
    pub fn compile(&self, book: &Book, registry: &FontRegistry) -> StyleTree {
        cascade(book, &self.sheets, registry, self.warnings.clone())
    }
}

/// One book's styling under the built-in sheet alone.
pub fn defaults(book: &Book, registry: &FontRegistry) -> StyleTree {
    Stylesheets::parse(&[]).compile(book, registry)
}

/// Registers one `@font-face`, or says why it could not be.
fn register_face(
    face: &FontFace,
    registry: &mut FontRegistry,
    loader: &dyn FontLoader,
) -> Vec<Warning> {
    let mut warnings = Vec::new();
    for src in &face.src {
        let Src::Url(url) = src else {
            continue;
        };
        let Some(bytes) = loader.load(url) else {
            continue;
        };
        match FontSource::from_bytes(bytes) {
            Ok(mut source) => {
                source.family = face.family.to_lowercase();
                source.declared = declared(face);
                return match registry.add(source) {
                    Ok(_) => warnings,
                    Err(error) => {
                        warnings.push(Warning {
                            message: format!("@font-face {}: {error}", face.family),
                            origin: Some(url.clone()),
                        });
                        warnings
                    }
                };
            }
            Err(error) => warnings.push(Warning {
                message: format!("@font-face {}: {error}", face.family),
                origin: Some(url.clone()),
            }),
        }
    }
    warnings.push(Warning {
        message: format!(
            "@font-face {}: no source resolved; text falls back to a registered face",
            face.family
        ),
        origin: None,
    });
    warnings
}

/// What a `@font-face` declared its source to be. A sheet that
/// declares neither slope nor weight is naming a family, not a cut,
/// and the file names the cuts.
fn declared(face: &FontFace) -> Option<FaceAttributes> {
    let (style, weight) = (face.style, face.weight);
    (style.is_some() || weight.is_some()).then(|| FaceAttributes {
        italic: style == Some(FontStyle::Italic),
        weight: weight.unwrap_or(FaceAttributes::REGULAR.weight),
    })
}

/// Matching and the cascade: every element gets the declarations that
/// match it, in cascade order, applied over what it inherited.
fn cascade(
    book: &Book,
    sheets: &[Sheet],
    registry: &FontRegistry,
    mut warnings: Vec<Warning>,
) -> StyleTree {
    let elements = ElementTree::build(book);
    let mut caches = SelectorCaches::default();

    let mut styles: Vec<ComputedStyle> = Vec::new();
    let mut nodes = Vec::new();
    let mut computed: Vec<u32> = Vec::with_capacity(elements.nodes().len());
    let mut first_letters: Vec<Option<u32>> = Vec::with_capacity(elements.nodes().len());
    let mut max_id = 0u32;

    for (index, node) in elements.nodes().iter().enumerate() {
        let parent = node
            .parent
            .map(|parent| styles[computed[parent] as usize].clone())
            .unwrap_or_else(ComputedStyle::initial);
        let mut style = parent.inherit();

        let matched = applicable(sheets, &elements, index, &mut caches, None);
        let root_size = styles.first().map(|style| style.font_size);
        let parent_size = parent.font_size;
        apply_all(&mut style, sheets, &matched, parent_size, root_size);
        let (font_id, warning) = resolve_face(&style, registry);
        style.font_id = font_id;
        report(&mut warnings, warning);
        report(&mut warnings, synthesized_small_caps(&style, registry));

        // `::first-letter` cascades over the element's own style, so
        // it is a second matching pass rather than a second element.
        let pseudo = applicable(
            sheets,
            &elements,
            index,
            &mut caches,
            Some(&PseudoElement::FirstLetter),
        );
        let first_letter = (!pseudo.is_empty()).then(|| {
            let mut initial = style.inherit();
            apply_all(&mut initial, sheets, &pseudo, style.font_size, root_size);
            let (font_id, warning) = resolve_face(&initial, registry);
            initial.font_id = font_id;
            report(&mut warnings, warning);
            report(&mut warnings, synthesized_small_caps(&initial, registry));
            initial
        });

        let index_of_style = intern(&mut styles, style);
        let index_of_initial = first_letter.map(|initial| intern(&mut styles, initial));
        computed.push(index_of_style);
        first_letters.push(index_of_initial);
        nodes.push(NodeStyle {
            id: node.id.get(),
            element: node.name,
            style: index_of_style,
            first_letter: index_of_initial,
        });
        max_id = max_id.max(node.id.get());
    }

    let root = computed.first().copied().unwrap_or(0);
    let mut by_node = vec![root; max_id as usize + 1];
    let mut initial_by_node = vec![None; max_id as usize + 1];
    for ((node, style), initial) in elements.nodes().iter().zip(&computed).zip(&first_letters) {
        by_node[node.id.get() as usize] = *style;
        initial_by_node[node.id.get() as usize] = *initial;
    }
    by_node[0] = root;

    if styles.is_empty() {
        styles.push(ComputedStyle::initial());
    }
    let pages: Vec<(u8, PageRule)> = sheets
        .iter()
        .flat_map(|sheet| {
            let level = level(sheet.origin, Importance::Normal);
            sheet.pages.iter().cloned().map(move |rule| (level, rule))
        })
        .collect();
    let masters = resolve_masters(
        &pages,
        &styles,
        &styles[root as usize].clone(),
        registry,
        &mut warnings,
    );

    StyleTree {
        styles,
        nodes,
        masters,
        by_node,
        initial_by_node,
        warnings,
    }
}

/// Where one applicable declaration lives, sorted into cascade order:
/// origin and importance, then specificity, then source order.
type Applicable = (u8, u32, usize, usize, usize);

/// Every declaration that matches one element, in cascade order.
/// `pseudo` selects the pass: `None` matches the element itself,
/// `Some(_)` matches only rules ending in that pseudo-element.
fn applicable(
    sheets: &[Sheet],
    elements: &ElementTree,
    index: usize,
    caches: &mut SelectorCaches,
    pseudo: Option<&PseudoElement>,
) -> Vec<Applicable> {
    let mode = match pseudo {
        Some(_) => MatchingMode::ForStatelessPseudoElement,
        None => MatchingMode::Normal,
    };
    let mut applicable = Vec::new();
    for (sheet_index, sheet) in sheets.iter().enumerate() {
        for (rule_index, rule) in sheet.rules.iter().enumerate() {
            let specificity = rule
                .selectors
                .slice()
                .iter()
                .filter(|selector| selector.pseudo_element() == pseudo)
                .filter(|selector| {
                    let mut context = MatchingContext::new(
                        mode,
                        None,
                        caches,
                        QuirksMode::NoQuirks,
                        NeedsSelectorFlags::No,
                        MatchingForInvalidation::No,
                    );
                    matches_selector(selector, 0, None, &elements.at(index), &mut context)
                })
                .map(|selector| selector.specificity())
                .max();
            let Some(specificity) = specificity else {
                continue;
            };
            for (order, (_, importance)) in rule.declarations.iter().enumerate() {
                applicable.push((
                    level(sheet.origin, *importance),
                    specificity,
                    sheet_index,
                    rule_index,
                    order,
                ));
            }
        }
    }
    applicable.sort_unstable();
    applicable
}

/// Applies matched declarations in cascade order.
fn apply_all(
    style: &mut ComputedStyle,
    sheets: &[Sheet],
    applicable: &[Applicable],
    parent_size: f32,
    root_size: Option<f32>,
) {
    for (_, _, sheet_index, rule_index, order) in applicable {
        let (declaration, _) = &sheets[*sheet_index].rules[*rule_index].declarations[*order];
        style.apply(declaration, parent_size, root_size.unwrap_or(parent_size));
    }
}

/// Where one declaration sits in the cascade, before specificity is
/// consulted: author CSS beats the built-in sheet, and `!important`
/// turns that round for the origin that used it.
fn level(origin: Origin, importance: Importance) -> u8 {
    match (origin, importance) {
        (Origin::UserAgent, Importance::Normal) => 0,
        (Origin::Author, Importance::Normal) => 1,
        (Origin::Author, Importance::Important) => 2,
        (Origin::UserAgent, Importance::Important) => 3,
    }
}

/// The index of a style in the table, adding it if it is new. Books
/// have thousands of nodes and a handful of styles.
fn intern(styles: &mut Vec<ComputedStyle>, style: ComputedStyle) -> u32 {
    match styles.iter().position(|known| *known == style) {
        Some(index) => index as u32,
        None => {
            styles.push(style);
            (styles.len() - 1) as u32
        }
    }
}

/// Records a face diagnostic once. Thousands of nodes share a
/// handful of styles, and a font stack that cannot be honoured is a
/// fact about the stack, not about each node that used it.
fn report(warnings: &mut Vec<Warning>, warning: Option<Warning>) {
    let Some(warning) = warning else {
        return;
    };
    if !warnings.iter().any(|seen| seen.message == warning.message) {
        warnings.push(warning);
    }
}

/// The face a computed style shapes with: the first family in the
/// registry, at the nearest slope and weight it has.
///
/// A family the registry does not have is skipped — that is what a
/// font stack is for — but a stack that resolves nothing, or a
/// family with no cut at the slope asked for, falls back to a face
/// that is visibly not the one requested. Both say so.
fn resolve_face(style: &ComputedStyle, registry: &FontRegistry) -> (u16, Option<Warning>) {
    let want = FaceAttributes {
        italic: style.font_style == FontStyle::Italic,
        weight: style.font_weight,
    };
    for family in &style.font_family {
        let name = match family {
            Family::Named(name) => Some(name.clone()),
            Family::Generic(generic) => registry
                .generic(*generic)
                .and_then(|id| registry.font_ref(id).map(|entry| entry.family.clone())),
        };
        let Some(found) = name.as_deref().and_then(|name| registry.select(name, want)) else {
            continue;
        };
        let warning = (found.attributes.italic != want.italic).then(|| Warning {
            message: format!(
                "{} has no {} face; {} used instead",
                name.unwrap_or_default(),
                slope(want.italic),
                slope(found.attributes.italic),
            ),
            origin: None,
        });
        return (found.id, warning);
    }
    (
        0,
        Some(Warning {
            message: format!(
                "no registered family matches {}; the first registered face used instead",
                stack(&style.font_family),
            ),
            origin: None,
        }),
    )
}

/// The diagnostic for a face with no small capitals of its own, one
/// per style that asks for them: an author who picked the face can
/// pick another one.
fn synthesized_small_caps(style: &ComputedStyle, registry: &FontRegistry) -> Option<Warning> {
    let asked = style.font_variant_caps == FontVariantCaps::SmallCaps;
    (asked && !registry.has_small_caps(style.font_id)).then(|| Warning {
        message: format!(
            "{} has no small capitals; reduced capitals used instead",
            registry
                .font_ref(style.font_id)
                .map(|entry| entry.family.clone())
                .unwrap_or_else(|| stack(&style.font_family)),
        ),
        origin: None,
    })
}

/// A slope as a stylesheet names it.
fn slope(italic: bool) -> &'static str {
    if italic { "italic" } else { "upright" }
}

/// A font stack as the sheet wrote it, for a diagnostic to quote.
fn stack(families: &[Family]) -> String {
    families
        .iter()
        .map(|family| match family {
            Family::Named(name) => name.clone(),
            Family::Generic(generic) => generic.keyword().to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{Block, HeadingLevel, Inline, Metadata, Section};
    use crate::fonts::{BUNDLED_FONT, FaceAttributes, GenericFamily, bundled_registry};
    use crate::lines::{HangEnd, HangingPunctuation};

    fn registry() -> &'static FontRegistry {
        static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
        REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
    }

    fn text(value: &str) -> Inline {
        Inline::Text {
            id: NodeId::UNASSIGNED,
            value: value.into(),
            position: None,
        }
    }

    /// A chapter: a heading, a paragraph opening with emphasis, a
    /// paragraph of plain prose, and a blockquote.
    fn sample() -> Book {
        let mut book = Book {
            metadata: Metadata::default(),
            sections: vec![Section {
                id: NodeId::UNASSIGNED,
                source: Some("chapter-01.md".into()),
                title: None,
                blocks: vec![
                    Block::Heading {
                        id: NodeId::UNASSIGNED,
                        level: HeadingLevel::H1,
                        inlines: vec![text("Chapter One")],
                        position: None,
                    },
                    Block::Paragraph {
                        id: NodeId::UNASSIGNED,
                        inlines: vec![
                            Inline::Emphasis {
                                id: NodeId::UNASSIGNED,
                                children: vec![text("In which")],
                                position: None,
                            },
                            text(" a drawer is opened."),
                        ],
                        position: None,
                    },
                    Block::Paragraph {
                        id: NodeId::UNASSIGNED,
                        inlines: vec![text("It was the kind of morning.")],
                        position: None,
                    },
                    Block::Blockquote {
                        id: NodeId::UNASSIGNED,
                        blocks: vec![Block::Paragraph {
                            id: NodeId::UNASSIGNED,
                            inlines: vec![text("\"Nobody's early here.\"")],
                            position: None,
                        }],
                        position: None,
                    },
                ],
                position: None,
            }],
        };
        book.assign_node_ids();
        book
    }

    /// The tree, with `css` cascading over the built-in sheet.
    fn compile(book: &Book, css: &str) -> StyleTree {
        Stylesheets::parse(&[Source::author("author.css", css)]).compile(book, registry())
    }

    /// The computed style of the nth element with a given name.
    fn nth(tree: &StyleTree, element: &str, index: usize) -> ComputedStyle {
        let node = tree
            .nodes()
            .iter()
            .filter(|node| node.element == element)
            .nth(index)
            .unwrap_or_else(|| panic!("no {element} #{index}"));
        tree.styles()[node.style as usize].clone()
    }

    fn first(tree: &StyleTree, element: &str) -> ComputedStyle {
        nth(tree, element, 0)
    }

    /// The built-in sheet sets the body, chapter and folio styles, and
    /// the trade-paperback page.
    #[test]
    fn the_built_in_sheet_sets_the_defaults() {
        let book = sample();
        let tree = defaults(&book, registry());

        let body = first(&tree, "p");
        assert_eq!(body.font_size, 11.0);
        assert_eq!(body.line_height, 1.4);
        assert_eq!(body.font_id, 0);
        assert_eq!(body.hyphens, Hyphens::None);
        assert_eq!(first(&tree, "h1").font_size, 18.0);

        let recto = tree.page(PageQuery {
            name: Some("chapter"),
            situation: Situation::Body(Side::Recto),
        });
        assert_eq!(recto.geometry.width, 432.0);
        assert_eq!(recto.geometry.height, 648.0);
        assert_eq!(recto.geometry.content_size(), (336.0, 540.0));
        let folio = recto
            .margin_box(MarginBox::BottomCenter)
            .expect("the folio is a margin box");
        assert_eq!(folio.content, Content::Counter(CounterStyle::Decimal));
        assert_eq!(folio.style.font_size, 9.0);
        assert_eq!(folio.style.line_height, 1.4);
    }

    /// Selectors run against the content tree: type, descendant,
    /// child, `:first-child` and `:is()` all pick out the elements a
    /// reader of the markdown would expect.
    #[test]
    fn selectors_match_the_content_tree() {
        let book = sample();
        let tree = compile(
            &book,
            "section > p { text-indent: 12pt }
             blockquote p { text-indent: 3pt }
             :is(h1, h2):first-child { text-indent: 6pt }
             :is(em, strong) { font-weight: 700 }",
        );
        // The section's own paragraphs are indented; the one inside
        // the blockquote is not a child of the section, so only the
        // descendant rule reaches it.
        assert_eq!(nth(&tree, "p", 0).text_indent, 12.0);
        assert_eq!(nth(&tree, "p", 1).text_indent, 12.0);
        assert_eq!(nth(&tree, "p", 2).text_indent, 3.0);
        // The heading opens the section, so it is a first child.
        assert_eq!(first(&tree, "h1").text_indent, 6.0);
        assert_eq!(first(&tree, "em").font_weight, 700);

        // Siblings, negation, counting and `:has()` all reach the
        // same tree.
        let tree = compile(
            &book,
            "h1 + p { text-indent: 1pt }
             h1 ~ p { widows: 3 }
             p:not(:first-child) { orphans: 4 }
             section:has(blockquote) > p:nth-child(3) { text-indent: 2pt }",
        );
        assert_eq!(nth(&tree, "p", 0).text_indent, 1.0);
        assert_eq!(nth(&tree, "p", 1).text_indent, 2.0);
        assert_eq!(nth(&tree, "p", 0).widows, 3);
        assert_eq!(nth(&tree, "p", 0).orphans, 4);
        assert_eq!(first(&tree, "h1").orphans, 2);
    }

    /// Author CSS overrides the built-in sheet, and specificity and
    /// source order decide between author rules.
    #[test]
    fn author_css_overrides_the_built_in_sheet() {
        let book = sample();
        // The built-in sheet's `book { font-size: 11pt }` is more
        // specific than nothing, and still loses to the author.
        let tree = compile(&book, "* { font-size: 13pt } p { font-size: 12pt }");
        assert_eq!(first(&tree, "p").font_size, 12.0);
        // Equal specificity: the later rule wins.
        let tree = compile(&book, "p { font-size: 12pt } p { font-size: 14pt }");
        assert_eq!(first(&tree, "p").font_size, 14.0);
        // `!important` in the built-in sheet would outrank the
        // author, and nothing in it uses one.
        let tree = compile(
            &book,
            "p { font-size: 12pt !important } p { font-size: 14pt }",
        );
        assert_eq!(first(&tree, "p").font_size, 12.0);
    }

    /// An unsupported property is a diagnostic naming its source
    /// position, and the rest of the rule still applies.
    #[test]
    fn unsupported_properties_warn_and_the_run_continues() {
        let book = sample();
        let tree = compile(
            &book,
            "p {\n  text-shadow: 0 0 2px black;\n  font-size: 13pt;\n}\n",
        );
        let warning = tree
            .warnings()
            .iter()
            .find(|warning| warning.message.contains("text-shadow"))
            .expect("text-shadow is outside the subset");
        assert_eq!(warning.message, "unsupported property `text-shadow`");
        assert_eq!(warning.origin.as_deref(), Some("author.css:2:3"));
        assert_eq!(
            first(&tree, "p").font_size,
            13.0,
            "the declaration after the bad one was dropped"
        );
    }

    /// The rest of the subset's edges: an at-rule, a selector and a
    /// value the engine does not know each warn where they were
    /// written, and the sheet keeps parsing.
    #[test]
    fn unsupported_at_rules_selectors_and_values_warn() {
        let book = sample();
        let tree = compile(
            &book,
            "@media print { p { font-size: 30pt } }\n\
             p:hover { font-size: 40pt }\n\
             p { text-align: sideways }\n\
             p { hanging-punctuation: sideways }\n\
             p { font-size: 15pt }\n",
        );
        let messages: Vec<&str> = tree
            .warnings()
            .iter()
            .map(|warning| warning.message.as_str())
            .collect();
        assert!(
            messages.contains(&"unsupported at-rule `@media`"),
            "{messages:?}"
        );
        assert!(
            messages.contains(&"unsupported selector `:hover`"),
            "{messages:?}"
        );
        assert!(
            messages.contains(&"unsupported value for `text-align`"),
            "{messages:?}"
        );
        assert!(
            messages.contains(&"unsupported value for `hanging-punctuation`"),
            "{messages:?}"
        );
        assert!(
            tree.warnings()
                .iter()
                .all(|warning| warning.origin.is_some()),
            "a diagnostic with no position: {:?}",
            tree.warnings()
        );
        assert_eq!(first(&tree, "p").font_size, 15.0);
    }

    /// `@page :first` and `:left`/`:right` select different masters
    /// for the same book: a chapter opening runs a blind folio, and
    /// the spread's margins mirror.
    #[test]
    fn page_pseudo_classes_select_different_masters() {
        let book = sample();
        let tree = defaults(&book, registry());
        let chapter = |situation| {
            tree.page(PageQuery {
                name: Some("chapter"),
                situation,
            })
        };
        let opening = chapter(Situation::First(Side::Recto));
        let body = chapter(Situation::Body(Side::Recto));
        let verso = chapter(Situation::Body(Side::Verso));
        let blank = chapter(Situation::Blank);

        assert!(opening.margin_box(MarginBox::BottomCenter).is_none());
        // A blank's master is a master like any other; that nothing
        // paints on a blank is the flow's rule, not the sheet's.
        assert_eq!(blank.geometry, verso.geometry);
        assert_eq!(
            body.margin_box(MarginBox::BottomCenter)
                .map(|box_| &box_.content),
            Some(&Content::Counter(CounterStyle::Decimal))
        );
        assert_eq!(body.geometry.margin.left, 54.0);
        assert_eq!(body.geometry.margin.right, 42.0);
        assert_eq!(verso.geometry.margin.left, 42.0);
        assert_eq!(verso.geometry.margin.right, 54.0);
        // Every situation of every named page has a master.
        assert_eq!(tree.masters().len(), 10);
    }

    /// The furniture grammar: what an element sets a running string
    /// to, where the folio restarts, and what a margin box resolves
    /// its content from.
    #[test]
    fn string_set_counter_reset_and_margin_box_content_compile() {
        let book = sample();
        let tree = compile(
            &book,
            "h1 { string-set: chapter \"— \" content() }
             section { counter-reset: page 7 }
             @page :left { @top-left { content: string(chapter) } }
             @page :right { @top-right { content: counter(page, upper-roman) } }",
        );

        assert_eq!(
            first(&tree, "h1").string_set,
            vec![StringSet {
                name: "chapter".into(),
                value: vec![StringPiece::Text("— ".into()), StringPiece::Content],
            }],
        );
        assert_eq!(first(&tree, "section").counter_reset, Some(7));
        // Not inherited: a paragraph inside the section restarts
        // nothing and sets nothing.
        assert_eq!(first(&tree, "p").counter_reset, None);
        assert!(first(&tree, "p").string_set.is_empty());

        let box_content = |situation, which| {
            tree.page(PageQuery {
                name: Some("chapter"),
                situation,
            })
            .margin_box(which)
            .map(|box_| box_.content.clone())
        };
        assert_eq!(
            box_content(Situation::Body(Side::Verso), MarginBox::TopLeft),
            Some(Content::String("chapter".into())),
        );
        assert_eq!(
            box_content(Situation::Body(Side::Recto), MarginBox::TopRight),
            Some(Content::Counter(CounterStyle::UpperRoman)),
        );
    }

    /// A counter style outside the subset is a diagnostic, not a
    /// silently decimal folio.
    #[test]
    fn an_unsupported_counter_style_warns() {
        let book = sample();
        let tree = compile(
            &book,
            "@page { @bottom-center { content: counter(page, georgian) } }",
        );
        assert!(
            tree.warnings()
                .iter()
                .any(|warning| warning.message == "unsupported value for `content`"),
            "{:?}",
            tree.warnings(),
        );
    }

    /// The `@page` grammar: named pages, sheet sizes, margins, and a
    /// margin box set to a literal.
    #[test]
    fn page_grammar_sets_size_margins_and_margin_boxes() {
        let book = sample();
        let tree = compile(
            &book,
            "section { page: chapter }
             @page { size: a5; margin: 2cm }
             @page chapter { margin-left: 3cm }
             @page chapter:first { @top-center { content: \"❦\"; font-size: 14pt } }",
        );
        let a5 = (148.0 * 72.0 / 25.4, 210.0 * 72.0 / 25.4);
        let body = tree.page(PageQuery {
            name: Some("chapter"),
            situation: Situation::Body(Side::Recto),
        });
        assert!((body.geometry.width - a5.0).abs() < 1e-3);
        assert!((body.geometry.height - a5.1).abs() < 1e-3);
        assert!((body.geometry.margin.top - 2.0 * 72.0 / 2.54).abs() < 1e-3);
        assert!((body.geometry.margin.left - 3.0 * 72.0 / 2.54).abs() < 1e-3);

        let opening = tree.page(PageQuery {
            name: Some("chapter"),
            situation: Situation::First(Side::Recto),
        });
        let ornament = opening
            .margin_box(MarginBox::TopCenter)
            .expect("the opening page has an ornament");
        assert_eq!(ornament.content, Content::Text("❦".into()));
        assert_eq!(ornament.style.font_size, 14.0);
        // An unnamed page never picked up the named rule's margin.
        let unnamed = tree.page(PageQuery {
            name: None,
            situation: Situation::Body(Side::Recto),
        });
        assert!((unnamed.geometry.margin.left - 2.0 * 72.0 / 2.54).abs() < 1e-3);
    }

    /// `@font-face` resolves its `src` through the host loader, and
    /// the family the sheet declared is the one selectors use.
    #[test]
    fn font_face_src_resolves_through_the_host_loader() {
        struct OneFace;
        impl FontLoader for OneFace {
            fn load(&self, url: &str) -> Option<Vec<u8>> {
                (url == "faces/garamond.ttf").then(|| BUNDLED_FONT.to_vec())
            }
        }

        let book = sample();
        let mut registry = bundled_registry().expect("bundled font parses");
        let css = "@font-face {
                     font-family: \"Author Serif\";
                     src: url(\"faces/garamond.ttf\") format(\"truetype\");
                   }
                   @font-face {
                     font-family: \"Missing\";
                     src: url(\"faces/nowhere.ttf\");
                   }
                   p { font-family: \"Author Serif\", serif }
                   h1 { font-family: \"Missing\", serif }";
        let mut sheets = Stylesheets::parse(&[Source::author("author.css", css)]);
        sheets.load_fonts(&mut registry, &OneFace);
        let tree = sheets.compile(&book, &registry);

        let author = registry
            .by_family("author serif")
            .expect("the loaded face joined the registry");
        assert_eq!(registry.len(), bundled_registry().unwrap().len() + 5);
        assert_eq!(first(&tree, "p").font_id, author);
        // A face nothing resolved falls back to the next family.
        assert_eq!(first(&tree, "h1").font_id, 0);
        assert!(
            tree.warnings()
                .iter()
                .any(|warning| warning.message.contains("Missing")),
            "an unresolved face should say so: {:?}",
            tree.warnings()
        );
    }

    /// The engine opens nothing itself: with a loader that resolves
    /// nothing, `@font-face` adds no face and says why.
    #[test]
    fn without_a_loader_no_font_face_resolves() {
        let book = sample();
        let mut registry = bundled_registry().expect("bundled font parses");
        let css = "@font-face { font-family: \"Author Serif\"; src: url(\"anywhere.ttf\") }";
        let mut sheets = Stylesheets::parse(&[Source::author("author.css", css)]);
        sheets.load_fonts(&mut registry, &NoFonts);
        let tree = sheets.compile(&book, &registry);
        assert_eq!(registry.len(), bundled_registry().unwrap().len());
        assert!(
            tree.warnings()
                .iter()
                .any(|w| w.message.contains("no source resolved"))
        );
    }

    /// Relative lengths compute against what CSS says they do:
    /// `em` in `font-size` against the parent, `rem` against the
    /// root, `em` elsewhere against the element's own size.
    #[test]
    fn relative_lengths_resolve_against_their_reference() {
        let book = sample();
        let tree = compile(
            &book,
            "book { font-size: 10pt }
             p { font-size: 1.2em; text-indent: 2em; margin-left: 1rem }
             em { font-size: 50% }",
        );
        let paragraph = first(&tree, "p");
        assert!((paragraph.font_size - 12.0).abs() < 1e-4);
        assert!((paragraph.text_indent - 24.0).abs() < 1e-4);
        assert!((paragraph.margin.left - 10.0).abs() < 1e-4);
        assert!((first(&tree, "em").font_size - 6.0).abs() < 1e-4);
    }

    /// Line height computes to a multiple whatever it was written as.
    #[test]
    fn line_height_computes_to_a_multiple() {
        let book = sample();
        let tree = compile(
            &book,
            "p { font-size: 10pt; line-height: 15pt }
             h1 { line-height: 150% }
             em { line-height: normal }",
        );
        assert!((first(&tree, "p").line_height - 1.5).abs() < 1e-4);
        assert!((first(&tree, "h1").line_height - 1.5).abs() < 1e-4);
        assert!((first(&tree, "em").line_height - 1.2).abs() < 1e-4);
    }

    /// Inheritance: text properties reach descendants, box properties
    /// do not.
    #[test]
    fn text_properties_inherit_and_box_properties_do_not() {
        let book = sample();
        let tree = compile(
            &book,
            "section { text-align: justify; margin-left: 20pt; hyphens: auto }",
        );
        let paragraph = first(&tree, "p");
        assert_eq!(paragraph.text_align, TextAlign::Justify);
        assert_eq!(paragraph.hyphens, Hyphens::Auto);
        assert_eq!(paragraph.margin.left, 0.0);
        assert_eq!(first(&tree, "section").margin.left, 20.0);
    }

    /// `text-justify` and `hanging-punctuation` read as CSS writes
    /// them: the first a keyword, the second a set of them in any
    /// order.
    #[test]
    fn justification_and_hanging_marks_read_as_css_writes_them() {
        let book = sample();
        let tree = compile(
            &book,
            "book { text-justify: inter-character;\
                    hanging-punctuation: last first allow-end }",
        );
        let paragraph = first(&tree, "p");
        assert_eq!(paragraph.text_justify, TextJustify::InterCharacter);
        assert_eq!(
            paragraph.hanging_punctuation,
            HangingPunctuation {
                first: true,
                end: HangEnd::Allow,
                last: true,
            },
        );
        let plain = compile(&book, "book { hanging-punctuation: none }");
        assert_eq!(
            first(&plain, "p").hanging_punctuation,
            HangingPunctuation::NONE,
        );
    }

    /// Generic families resolve through the registry's bindings, and
    /// slope and weight pick the nearest face in a family.
    #[test]
    fn families_resolve_through_the_registry() {
        let book = sample();
        let tree = compile(&book, "p { font-family: \"Nowhere\", monospace }");
        let paragraph = first(&tree, "p");
        assert_eq!(
            paragraph.font_family,
            vec![
                Family::Named("Nowhere".into()),
                Family::Generic(GenericFamily::Monospace),
            ]
        );
        assert_eq!(
            paragraph.font_id,
            registry().generic(GenericFamily::Monospace).unwrap()
        );
    }

    /// A book of one paragraph, a scene break and a quotation: the
    /// blocks whose defaults the built-in sheet has opinions about.
    fn furnished() -> Book {
        let mut book = Book {
            metadata: Metadata::default(),
            sections: vec![Section {
                blocks: vec![
                    Block::Paragraph {
                        id: NodeId::UNASSIGNED,
                        inlines: vec![text("Before the break.")],
                        position: None,
                    },
                    Block::ThematicBreak {
                        id: NodeId::UNASSIGNED,
                        position: None,
                    },
                    Block::Blockquote {
                        id: NodeId::UNASSIGNED,
                        blocks: vec![Block::Paragraph {
                            id: NodeId::UNASSIGNED,
                            inlines: vec![text("Quoted.")],
                            position: None,
                        }],
                        position: None,
                    },
                ],
                ..Default::default()
            }],
        };
        book.assign_node_ids();
        book
    }

    /// The built-in sheet sets a scene break in an ornament, centred,
    /// and keeps it off both ends of a page. An author sets it in
    /// something else, or in space.
    #[test]
    fn a_thematic_break_takes_its_ornament_from_the_cascade() {
        let book = furnished();
        let tree = defaults(&book, registry());
        let rule = first(&tree, "hr");
        assert_eq!(rule.content, Content::Text("\u{2766}".into()));
        assert_eq!(rule.text_align, TextAlign::Center);
        assert_eq!(rule.break_before, Break::Avoid);
        assert_eq!(rule.break_after, Break::Avoid);
        assert_eq!(rule.margin.top, 11.0);

        assert_eq!(
            first(&compile(&book, "hr { content: none }"), "hr").content,
            Content::None,
        );
        assert_eq!(
            first(&compile(&book, "hr { content: \"* * *\" }"), "hr").content,
            Content::Text("* * *".into()),
        );
    }

    /// A quotation is indented on both sides and set off above and
    /// below, and the indent is a multiple of its own size.
    #[test]
    fn a_blockquote_is_indented_by_the_built_in_sheet() {
        let book = furnished();
        let tree = defaults(&book, registry());
        let quote = first(&tree, "blockquote");
        assert_eq!(quote.margin.left, 2.0 * quote.font_size);
        assert_eq!(quote.margin.right, 2.0 * quote.font_size);
        assert_eq!(quote.margin.top, quote.font_size);
        // The paragraph inside has no indent of its own: the
        // quotation's box is what moves it.
        assert_eq!(nth(&tree, "p", 1).margin.left, 0.0);
    }

    /// `::first-letter` is a second pass over the element it belongs
    /// to: it starts from that element's own computed style, cascades
    /// the rules that named it over the top, and resolves a face of
    /// its own. Rules that name no pseudo-element never reach it, and
    /// it never reaches the element.
    #[test]
    fn first_letter_cascades_over_the_element_it_belongs_to() {
        let book = sample();
        let tree = compile(
            &book,
            "p { font-size: 12pt; line-height: 1.8 }
             p::first-letter { initial-letter: 3; font-family: monospace }",
        );
        let Block::Paragraph { id, .. } = &book.sections[0].blocks[1] else {
            panic!("the second block is a paragraph");
        };
        let initial = tree
            .first_letter(*id)
            .expect("a rule named the paragraph's first letter");
        assert_eq!(initial.initial_letter, 3);
        assert_eq!(initial.font_size, 12.0, "the element's size is inherited");
        assert_eq!(initial.line_height, 1.8);
        assert_eq!(
            initial.font_id,
            registry().generic(GenericFamily::Monospace).unwrap(),
            "the pseudo-element resolves its own face",
        );

        // The element keeps its own style, and an element no rule
        // named has no first-letter style at all.
        assert_eq!(first(&tree, "p").initial_letter, 0);
        let Block::Heading { id, .. } = &book.sections[0].blocks[0] else {
            panic!("the first block is a heading");
        };
        assert!(tree.first_letter(*id).is_none());
        assert!(defaults(&book, registry()).first_letter(*id).is_none());

        // Selectors in front of the pseudo-element still have to
        // match: only the paragraph after the heading is picked out.
        let tree = compile(&book, "h1 + p::first-letter { initial-letter: 2 }");
        assert!(tree.first_letter(*id).is_none());
        let Block::Paragraph { id: second, .. } = &book.sections[0].blocks[2] else {
            panic!("the third block is a paragraph");
        };
        assert!(tree.first_letter(*second).is_none());
    }

    /// A pseudo-element outside the subset is a diagnostic, and the
    /// sheet keeps parsing.
    #[test]
    fn an_unsupported_pseudo_element_warns() {
        let book = sample();
        let tree = compile(
            &book,
            "p::first-line { font-size: 30pt }\np { font-size: 15pt }\n",
        );
        assert!(
            tree.warnings()
                .iter()
                .any(|warning| warning.message == "unsupported selector `:first-line`"),
            "{:?}",
            tree.warnings(),
        );
        assert_eq!(first(&tree, "p").font_size, 15.0);
    }

    /// A book whose one paragraph nests `strong` inside `em`.
    fn nested() -> Book {
        let mut book = Book {
            metadata: Metadata::default(),
            sections: vec![Section {
                blocks: vec![Block::Paragraph {
                    id: NodeId::UNASSIGNED,
                    inlines: vec![
                        text("She said "),
                        Inline::Emphasis {
                            id: NodeId::UNASSIGNED,
                            children: vec![
                                text("never "),
                                Inline::Strong {
                                    id: NodeId::UNASSIGNED,
                                    children: vec![text("again")],
                                    position: None,
                                },
                            ],
                            position: None,
                        },
                        text("."),
                    ],
                    position: None,
                }],
                ..Default::default()
            }],
        };
        book.assign_node_ids();
        book
    }

    /// Slope and weight reach the face through the cascade: `em` and
    /// `strong` set properties like any other rule, and `strong`
    /// inside `em` inherits the slope on its way to a bold italic
    /// face.
    #[test]
    fn emphasis_and_strong_resolve_to_their_own_faces() {
        let book = nested();
        let tree = defaults(&book, registry());
        let face = |italic, weight| {
            registry()
                .select("eb garamond", FaceAttributes { italic, weight })
                .unwrap()
                .id
        };
        assert_eq!(first(&tree, "p").font_id, face(false, 400));
        assert_eq!(first(&tree, "em").font_id, face(true, 400));
        assert_eq!(first(&tree, "strong").font_id, face(true, 700));
        // The cascade, not the element name: an author who unsets
        // the slope gets the upright bold cut under the same markup.
        let tree = compile(&book, "em { font-style: normal }");
        assert_eq!(first(&tree, "strong").font_id, face(false, 700));
    }

    /// A face the registry cannot supply is a diagnostic and the
    /// nearest cut, once, however many nodes asked for it.
    #[test]
    fn a_face_the_registry_lacks_warns_and_lays_out_anyway() {
        let mut upright = FontRegistry::new();
        upright
            .add(crate::fonts::FontSource::from_bytes(BUNDLED_FONT.to_vec()).unwrap())
            .unwrap();
        upright
            .map_generic(GenericFamily::Serif, "eb garamond")
            .unwrap();
        let book = nested();
        let tree = Stylesheets::parse(&[]).compile(&book, &upright);
        assert_eq!(
            first(&tree, "em").font_id,
            first(&tree, "p").font_id,
            "there is no italic cut, so emphasis takes the upright one",
        );
        let complaints: Vec<&str> = tree
            .warnings()
            .iter()
            .map(|warning| warning.message.as_str())
            .filter(|message| message.contains("italic"))
            .collect();
        assert_eq!(
            complaints,
            vec!["eb garamond has no italic face; upright used instead"],
        );

        // A stack that resolves nothing at all says so, and the book
        // still lays out on the first registered face.
        let tree = compile(&book, "p { font-family: \"Nowhere\", \"Nor here\" }");
        assert_eq!(first(&tree, "p").font_id, 0);
        assert!(
            tree.warnings().iter().any(|warning| {
                warning.message
                    == "no registered family matches Nowhere, Nor here; \
                                    the first registered face used instead"
            }),
            "{:?}",
            tree.warnings(),
        );
    }

    /// The display-typography properties parse, cascade and inherit:
    /// a rule on the book reaches the paragraph inside it, a rule on
    /// the paragraph overrides it, and `em` inside picks up what the
    /// paragraph set without being named.
    #[test]
    fn display_typography_parses_cascades_and_inherits() {
        let book = nested();
        let tree = compile(
            &book,
            "book { letter-spacing: 0.1em; font-variant-caps: small-caps;
                    text-transform: uppercase }
             p { letter-spacing: 2pt }",
        );
        let paragraph = first(&tree, "p");
        assert_eq!(paragraph.letter_spacing, 2.0, "the closer rule lost");
        assert_eq!(paragraph.font_variant_caps, FontVariantCaps::SmallCaps);
        assert_eq!(paragraph.text_transform, TextTransform::Uppercase);
        let emphasis = first(&tree, "em");
        assert_eq!(
            (
                emphasis.letter_spacing,
                emphasis.font_variant_caps,
                emphasis.text_transform,
            ),
            (2.0, FontVariantCaps::SmallCaps, TextTransform::Uppercase,),
            "emphasis did not inherit what the paragraph set",
        );

        // `em` is a multiple of the element's own size, `normal` and
        // `none` are the initial values written out, and the defaults
        // are what a book gets when nothing says otherwise.
        let tree = compile(
            &book,
            "book { letter-spacing: 0.1em; text-transform: uppercase }
             p { font-size: 20pt; letter-spacing: 0.1em }
             em { letter-spacing: normal; text-transform: none;
                  font-variant-caps: normal }",
        );
        assert_eq!(first(&tree, "p").letter_spacing, 2.0);
        assert_eq!(first(&tree, "em").letter_spacing, 0.0);
        assert_eq!(first(&tree, "em").text_transform, TextTransform::None);
        let plain = first(&defaults(&book, registry()), "p");
        assert_eq!(plain.letter_spacing, 0.0);
        assert_eq!(plain.font_variant_caps, FontVariantCaps::Normal);
        assert_eq!(plain.text_transform, TextTransform::None);

        // A value none of the three has is a diagnostic, not a rule.
        let tree = compile(&book, "p { text-transform: full-width }");
        assert!(
            tree.warnings()
                .iter()
                .any(|warning| { warning.message == "unsupported value for `text-transform`" }),
            "{:?}",
            tree.warnings(),
        );
    }

    /// `color` reads in each of the four forms the subset takes, and
    /// a form outside it is a diagnostic rather than a rule.
    #[test]
    fn colour_reads_in_every_form_the_subset_takes() {
        let book = sample();
        let tree = compile(
            &book,
            "h1 { color: darkred }
             p { color: #036 }
             blockquote { color: #b41e1e }
             em { color: rgb(0% 20% 100%) }",
        );
        assert_eq!(first(&tree, "h1").color, Color::rgb(139, 0, 0));
        assert_eq!(first(&tree, "p").color, Color::rgb(0, 51, 102));
        assert_eq!(first(&tree, "blockquote").color, Color::rgb(180, 30, 30));
        assert_eq!(first(&tree, "em").color, Color::rgb(0, 51, 255));
        assert_eq!(
            first(&defaults(&book, registry()), "p").color,
            Color::BLACK,
            "a book nothing coloured is set in black",
        );

        // A name no colour has, and channels written half as
        // numbers and half as percentages, neither of which CSS
        // allows.
        for css in ["p { color: octarine }", "p { color: rgb(10, 20%, 30) }"] {
            let tree = compile(&book, css);
            assert!(
                tree.warnings()
                    .iter()
                    .any(|warning| warning.message == "unsupported value for `color`"),
                "{css}: {:?}",
                tree.warnings(),
            );
        }
    }

    /// Colour inherits: a rule on the section reaches the prose under
    /// it, and a rule on the heading beats what it inherited.
    #[test]
    fn a_colour_on_a_section_reaches_the_prose_under_it() {
        let book = sample();
        let tree = compile(
            &book,
            "section { color: #444444 }
             h1 { color: rgb(180 30 30) }",
        );
        assert_eq!(first(&tree, "p").color, Color::rgb(68, 68, 68));
        assert_eq!(first(&tree, "em").color, Color::rgb(68, 68, 68));
        assert_eq!(first(&tree, "h1").color, Color::rgb(180, 30, 30));
    }

    /// `color` sets the text of a box. `background-color` fills the
    /// box behind it and needs a box model, so it warns at the line
    /// and column it was written at.
    #[test]
    fn background_colour_warns_where_it_was_written() {
        let book = sample();
        let tree = compile(&book, "p {\n  color: teal;\n  background-color: teal;\n}\n");
        let warning = tree
            .warnings()
            .iter()
            .find(|warning| warning.message.contains("background-color"))
            .expect("background-color is outside the subset");
        assert_eq!(warning.message, "unsupported property `background-color`");
        assert_eq!(warning.origin.as_deref(), Some("author.css:3:3"));
        assert_eq!(first(&tree, "p").color, Color::rgb(0, 128, 128));
    }

    /// A face with no small capitals of its own is reported once,
    /// however many nodes asked for them, and the book is set anyway.
    /// A face that has them is not reported at all.
    #[test]
    fn a_face_without_small_capitals_says_so() {
        let book = nested();
        let bare = crate::fonts::registry_without_substitutions();
        let css = "p, em, strong { font-variant-caps: small-caps }";
        let tree = Stylesheets::parse(&[Source::author("author.css", css)]).compile(&book, &bare);
        let complaints: Vec<&str> = tree
            .warnings()
            .iter()
            .map(|warning| warning.message.as_str())
            .filter(|message| message.contains("small capitals"))
            .collect();
        assert_eq!(
            complaints,
            vec!["eb garamond has no small capitals; reduced capitals used instead"],
        );

        let tree = compile(&book, css);
        assert!(
            !tree
                .warnings()
                .iter()
                .any(|warning| warning.message.contains("small capitals")),
            "the bundled face has small capitals: {:?}",
            tree.warnings(),
        );
    }

    /// Styles are interned: a book of thousands of nodes computes a
    /// handful of distinct styles, and every node points at one.
    #[test]
    fn styles_are_shared_between_nodes() {
        let book = sample();
        let tree = defaults(&book, registry());
        assert!(
            tree.styles().len() < tree.nodes().len(),
            "{} styles for {} nodes",
            tree.styles().len(),
            tree.nodes().len()
        );
        for node in tree.nodes() {
            assert!((node.style as usize) < tree.styles().len());
        }
        // A real node id resolves to the style its element got: the
        // section is the second element, after the book itself.
        assert_eq!(
            tree.style(book.sections[0].id),
            &tree.styles()[tree.nodes()[1].style as usize]
        );
    }
}
