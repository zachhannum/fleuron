//! What the cascade did for one element or one page margin box: the
//! rules that matched it, which of their declarations won, and what
//! every property computed to.
//!
//! Nothing is kept for this between questions. The cascade runs again
//! for the one element asked about, over the sheets the tree was
//! compiled from, so the answer cannot disagree with what layout used.

use std::collections::{BTreeMap, HashMap, HashSet};

use cssparser::ToCss;
use selectors::context::SelectorCaches;
use serde::Serialize;

use crate::content::{Book, NodeId};
use crate::pages::PageBox;

use super::element::ElementTree;
use super::properties::Declaration;
use super::sheet::{
    self, Importance, MarginDeclaration, MarginRule, PageRule, SheetPosition, Written,
    margin_longhands,
};
use super::{
    MarginBox, PageQuery, Situation, StyleTree, Stylesheets, align_hint, applicable, level, selects,
};
use crate::pages::Side;

mod css;

/// What one element, or one page margin box, answers with.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Inspection {
    /// The content node the answer is about: the element asked about,
    /// or the element that holds the text node asked about. `None`
    /// for a page margin box.
    pub node: Option<NodeId>,
    /// The element name selectors match, or the margin box's at-rule,
    /// as `@top-left`.
    pub element: String,
    /// The id a selector reaches the element by.
    pub id: Option<String>,
    /// The classes a selector reaches the element by.
    pub classes: Vec<String>,
    /// The elements this one sits inside, the book first.
    pub ancestors: Vec<Ancestor>,
    /// For a margin box, the page it is on as a page selector writes
    /// it, as `@page chapter:first:right`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    /// The rules that matched, in cascade order. Where two set the
    /// same property, the later one wins.
    pub rules: Vec<MatchedRule>,
    /// The computed value of every property in the subset, written as
    /// CSS. Lengths are in points.
    pub computed: BTreeMap<String, String>,
    /// The border box on each page the element reaches.
    pub boxes: Vec<PageBox>,
}

/// One element an inspected element sits inside.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Ancestor {
    /// Its content node. `None` for the book, and for the `thead` and
    /// `tbody` of a table, which no content node stands behind.
    pub node: Option<NodeId>,
    /// The element name selectors match.
    pub element: String,
    /// The id a selector reaches it by.
    pub id: Option<String>,
    /// The classes a selector reaches it by.
    pub classes: Vec<String>,
}

/// One rule that matched.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MatchedRule {
    /// The name the sheet was handed in under.
    pub sheet: String,
    /// The line the rule begins on, counting from 1.
    pub line: u32,
    /// The column it begins at, counting from 1.
    pub column: u32,
    /// The selector as CSS writes it. For a margin box, the `@page`
    /// prelude.
    pub selector: String,
    /// Its specificity, as ids, then classes, then element names. For
    /// a margin box, as a page name, then `:first` or `:blank`, then
    /// `:left` or `:right`.
    pub specificity: [u32; 3],
    /// Its declarations, in the order they were written.
    pub declarations: Vec<InspectedDeclaration>,
}

/// One declaration of a matched rule.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InspectedDeclaration {
    /// The property, in lowercase.
    pub property: String,
    /// The value as it was written.
    pub value: String,
    /// Whether it was written `!important`.
    pub important: bool,
    /// Whether it won the cascade. A shorthand won where any longhand
    /// it sets did.
    pub applied: bool,
}

impl Stylesheets {
    /// What the cascade did for one node of `book`, whose styling
    /// `tree` is, compiled from these sheets. A text node answers for
    /// the element that holds it.
    ///
    /// The id of a box that `::before` or `::after` generates answers
    /// for the element the box belongs to.
    ///
    /// `None` for a node the book does not hold, and for the id the
    /// engine writes its own text under. The boxes are left empty,
    /// since only a laid-out book has them.
    pub fn inspect(&self, book: &Book, tree: &StyleTree, node: NodeId) -> Option<Inspection> {
        let node = node.element();
        book.subtree(node)?;
        let elements = ElementTree::build(book);
        let index = owner(&elements, book, node)?;
        let nodes = elements.nodes();
        let element = &nodes[index];

        let mut caches = SelectorCaches::default();
        let matched = applicable(&self.sheets, &elements, index, &mut caches, None);
        let declaration = |(sheet, rule, order): (usize, usize, usize)| {
            &self.sheets[sheet].rules[rule].declarations[order].0
        };

        // The winner of each longhand, where `None` is the alignment
        // a table cell's column wrote, which no rule holds.
        let mut hint = element.align.map(align_hint);
        let mut winners: HashMap<String, Option<(usize, usize, usize)>> = HashMap::new();
        for (level, _, sheet, rule, order) in &matched {
            if *level > 0
                && let Some(hint) = hint.take()
            {
                winners.insert(hint.property().to_string(), None);
            }
            let at = (*sheet, *rule, *order);
            for property in longhands(declaration(at)) {
                winners.insert(property, Some(at));
            }
        }
        if let Some(hint) = hint {
            winners.insert(hint.property().to_string(), None);
        }
        let won: HashSet<(usize, usize, usize)> = winners.into_values().flatten().collect();

        let mut order: Vec<(usize, usize, u32)> = Vec::new();
        for (_, specificity, sheet, rule, _) in &matched {
            if !order.iter().any(|(s, r, _)| (*s, *r) == (*sheet, *rule)) {
                order.push((*sheet, *rule, *specificity));
            }
        }
        let rules = order
            .into_iter()
            .map(|(sheet, index, specificity)| {
                let rule = &self.sheets[sheet].rules[index];
                matched_rule(
                    &rule.position,
                    rule.selectors.to_css_string(),
                    [
                        specificity >> 20,
                        (specificity >> 10) & 0x3ff,
                        specificity & 0x3ff,
                    ],
                    &rule.written,
                    |at| won.contains(&(sheet, index, at)),
                )
            })
            .collect();

        let mut ancestors = Vec::new();
        let mut parent = element.parent;
        while let Some(at) = parent {
            let above = &nodes[at];
            ancestors.push(Ancestor {
                node: (above.id != NodeId::UNASSIGNED).then_some(above.id),
                element: above.name.to_string(),
                id: above.attributes.id.clone(),
                classes: above.attributes.classes.clone(),
            });
            parent = above.parent;
        }
        ancestors.reverse();

        Some(Inspection {
            node: Some(element.id),
            element: element.name.to_string(),
            id: element.attributes.id.clone(),
            classes: element.attributes.classes.clone(),
            ancestors,
            page: None,
            rules,
            computed: css::computed(tree.style(element.id)),
            boxes: Vec::new(),
        })
    }

    /// What the cascade did for one margin box of a page `query`
    /// describes, whose master `tree` resolved from these sheets.
    ///
    /// `None` for a box no rule for that page names. The boxes are left
    /// empty, since only a laid-out book has them.
    pub fn inspect_margin_box(
        &self,
        tree: &StyleTree,
        query: PageQuery<'_>,
        which: MarginBox,
    ) -> Option<Inspection> {
        // A named page with no master of its own takes the unnamed one.
        let query = match tree.master(query.name, query.situation) {
            Some(_) => query,
            None => PageQuery {
                name: None,
                ..query
            },
        };
        let style = tree.page(query).boxes.iter().find(|b| b.which == which)?;

        let mut matching: Vec<(u8, &PageRule)> = self
            .sheets
            .iter()
            .flat_map(|sheet| {
                let level = level(sheet.origin, Importance::Normal);
                sheet.pages.iter().map(move |rule| (level, rule))
            })
            .filter(|(_, rule)| selects(rule, query))
            .collect();
        matching.sort_by_key(|(level, rule)| (*level, rule.specificity()));

        fn boxes(rule: &PageRule, which: MarginBox) -> impl Iterator<Item = (usize, &MarginRule)> {
            rule.boxes
                .iter()
                .enumerate()
                .filter(move |(_, margin)| margin.which == which)
        }
        let mut winners: HashMap<String, (usize, usize, usize)> = HashMap::new();
        for (index, (_, rule)) in matching.iter().enumerate() {
            for (at, margin) in boxes(rule, which) {
                for (order, declaration) in margin.declarations.iter().enumerate() {
                    let declarations = match declaration {
                        MarginDeclaration::Pending(pending) => margin_longhands(&pending.property),
                        declaration => vec![declaration.clone()],
                    };
                    for declaration in declarations {
                        let property = match declaration {
                            MarginDeclaration::Style(declaration) => {
                                declaration.property().to_string()
                            }
                            _ => "content".to_string(),
                        };
                        winners.insert(property, (index, at, order));
                    }
                }
            }
        }
        let won: HashSet<(usize, usize, usize)> = winners.into_values().collect();

        let mut rules = Vec::new();
        for (index, (_, rule)) in matching.iter().enumerate() {
            let (name, first, side) = rule.specificity();
            for (at, margin) in boxes(rule, which) {
                rules.push(matched_rule(
                    &rule.position,
                    rule.selector.clone(),
                    [name as u32, first as u32, side as u32],
                    &margin.written,
                    |order| won.contains(&(index, at, order)),
                ));
            }
        }

        let mut computed = css::computed(&style.style);
        computed.insert("content".into(), css::content(&style.content));
        Some(Inspection {
            node: None,
            element: format!("@{}", which.keyword()),
            id: None,
            classes: Vec::new(),
            ancestors: Vec::new(),
            page: Some(page_selector(query)),
            rules,
            computed,
            boxes: Vec::new(),
        })
    }
}

/// The longhands a declaration sets. One that reads a custom property
/// sets every longhand of its property, whatever the value comes to.
fn longhands(declaration: &Declaration) -> Vec<String> {
    match declaration {
        Declaration::Pending(pending) => sheet::longhands(&pending.property)
            .iter()
            .map(|longhand| longhand.property().to_string())
            .collect(),
        declaration => vec![declaration.property().to_string()],
    }
}

/// One matched rule, its declarations marked by whether they won.
fn matched_rule(
    position: &SheetPosition,
    selector: String,
    specificity: [u32; 3],
    written: &[Written],
    won: impl Fn(usize) -> bool,
) -> MatchedRule {
    MatchedRule {
        sheet: position.sheet.clone(),
        line: position.line,
        column: position.column,
        selector,
        specificity,
        declarations: written
            .iter()
            .map(|written| InspectedDeclaration {
                property: written.property.clone(),
                value: written.value.clone(),
                important: written.important,
                applied: written.longhands.clone().any(&won),
            })
            .collect(),
    }
}

/// The element that stands for one node of `book`: the node itself,
/// or for a text node, the element that holds it.
pub(crate) fn element_of(book: &Book, node: NodeId) -> Option<NodeId> {
    let node = node.element();
    book.subtree(node)?;
    let elements = ElementTree::build(book);
    owner(&elements, book, node).map(|index| elements.nodes()[index].id)
}

/// The element that stands for `node`: its own, or for a text node,
/// which is not an element, the innermost element holding it.
fn owner(elements: &ElementTree, book: &Book, node: NodeId) -> Option<usize> {
    let nodes = elements.nodes();
    if let Some(index) = nodes.iter().position(|element| element.id == node) {
        return Some(index);
    }
    let mut at = nodes
        .iter()
        .rposition(|element| element.id != NodeId::UNASSIGNED && element.id < node)?;
    loop {
        let element = &nodes[at];
        if book
            .subtree(element.id)
            .is_some_and(|held| held.contains(&node.get()))
        {
            return Some(at);
        }
        at = element.parent?;
    }
}

/// The page a query describes, as a page selector writes it.
fn page_selector(query: PageQuery<'_>) -> String {
    let (opening, side) = match query.situation {
        Situation::First(side) => (":first", side),
        Situation::Body(side) => ("", side),
        Situation::Blank => (":blank", Side::Verso),
    };
    let side = match side {
        Side::Recto => ":right",
        Side::Verso => ":left",
    };
    format!("@page {}{opening}{side}", query.name.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts::{FontRegistry, bundled_registry};
    use crate::style::{Source, sheet::PROPERTIES};

    fn registry() -> &'static FontRegistry {
        static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
        REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
    }

    /// A chapter: a heading, and an epigraph holding one paragraph
    /// with an id, two classes, and emphasis inside it.
    fn book() -> Book {
        let json = r#"{"metadata": {}, "sections": [{"blocks": [
            {"type": "heading", "level": 1, "inlines": [{"type": "text", "value": "One"}]},
            {"type": "blockquote", "attributes": {"classes": ["epigraph"]}, "blocks": [
                {"type": "paragraph",
                 "attributes": {"id": "motto", "classes": ["quiet", "small"]},
                 "inlines": [{"type": "emphasis", "children": [{"type": "text", "value": "Lo"}]}]}
            ]}
        ]}]}"#;
        let mut book: Book = serde_json::from_str(json).expect("the book reads");
        book.assign_node_ids();
        book
    }

    const CSS: &str = "p { color: red }
.quiet { color: blue; margin: 1em 2em }
#motto { color: green; margin-left: 0 }
blockquote p { font-size: 9pt }";

    /// The sheets, and the tree they compile to over `book`.
    fn compiled(book: &Book, css: &str) -> (Stylesheets, StyleTree) {
        let sheets = Stylesheets::parse(&[Source::author("author.css", css)]);
        let tree = sheets.compile(book, registry());
        (sheets, tree)
    }

    fn element(book: &Book, name: &str) -> NodeId {
        ElementTree::build(book)
            .nodes()
            .iter()
            .find(|element| element.name == name)
            .unwrap_or_else(|| panic!("no {name}"))
            .id
    }

    fn author_rules(inspection: &Inspection) -> Vec<&MatchedRule> {
        inspection
            .rules
            .iter()
            .filter(|rule| rule.sheet == "author.css")
            .collect()
    }

    #[test]
    fn an_element_answers_with_its_name_id_classes_and_ancestors() {
        let book = book();
        let (sheets, tree) = compiled(&book, CSS);
        let paragraph = element(&book, "p");
        let inspection = sheets
            .inspect(&book, &tree, paragraph)
            .expect("a paragraph");

        assert_eq!(inspection.node, Some(paragraph));
        assert_eq!(inspection.element, "p");
        assert_eq!(inspection.id.as_deref(), Some("motto"));
        assert_eq!(inspection.classes, ["quiet", "small"]);
        let names: Vec<&str> = inspection
            .ancestors
            .iter()
            .map(|ancestor| ancestor.element.as_str())
            .collect();
        assert_eq!(names, ["book", "section", "blockquote"]);
        assert_eq!(inspection.ancestors[0].node, None);
        assert_eq!(inspection.ancestors[1].node, Some(book.sections[0].id));
        assert_eq!(inspection.ancestors[2].classes, ["epigraph"]);

        let emphasis = element(&book, "em");
        let text = NodeId::new(emphasis.get() + 1);
        let held = sheets.inspect(&book, &tree, text).expect("a text node");
        assert_eq!(held.node, Some(emphasis), "text answers for its element");
        assert_eq!(held.element, "em");
    }

    #[test]
    fn matched_rules_come_in_cascade_order_with_where_they_were_written() {
        let book = book();
        let (sheets, tree) = compiled(&book, CSS);
        let inspection = sheets
            .inspect(&book, &tree, element(&book, "p"))
            .expect("a paragraph");

        let written: Vec<(&str, u32, u32, [u32; 3])> = author_rules(&inspection)
            .iter()
            .map(|rule| {
                (
                    rule.selector.as_str(),
                    rule.line,
                    rule.column,
                    rule.specificity,
                )
            })
            .collect();
        assert_eq!(
            written,
            [
                ("p", 1, 1, [0, 0, 1]),
                ("blockquote p", 4, 1, [0, 0, 2]),
                (".quiet", 2, 1, [0, 1, 0]),
                ("#motto", 3, 1, [1, 0, 0]),
            ]
        );
        let last_built_in = inspection
            .rules
            .iter()
            .rposition(|rule| rule.sheet == "user-agent.css");
        let first_author = inspection
            .rules
            .iter()
            .position(|rule| rule.sheet == "author.css");
        assert!(
            last_built_in < first_author,
            "the built-in sheet cascades under the author's"
        );
    }

    #[test]
    fn each_declaration_says_whether_it_won_the_cascade() {
        let book = book();
        let (sheets, tree) = compiled(&book, CSS);
        let inspection = sheets
            .inspect(&book, &tree, element(&book, "p"))
            .expect("a paragraph");

        let applied: Vec<(&str, &str, bool)> = author_rules(&inspection)
            .iter()
            .flat_map(|rule| {
                rule.declarations.iter().map(|declaration| {
                    (
                        rule.selector.as_str(),
                        declaration.property.as_str(),
                        declaration.applied,
                    )
                })
            })
            .collect();
        assert_eq!(
            applied,
            [
                ("p", "color", false),
                ("blockquote p", "font-size", true),
                (".quiet", "color", false),
                (".quiet", "margin", true),
                ("#motto", "color", true),
                ("#motto", "margin-left", true),
            ]
        );
        assert!(
            inspection
                .rules
                .iter()
                .filter(|rule| rule.sheet == "user-agent.css")
                .flat_map(|rule| &rule.declarations)
                .filter(|declaration| declaration.property.starts_with("margin"))
                .all(|declaration| !declaration.applied),
            "the author's margins override every built-in one"
        );
    }

    #[test]
    fn every_property_in_the_subset_has_a_computed_value() {
        let book = book();
        let (sheets, tree) = compiled(&book, CSS);
        let inspection = sheets
            .inspect(&book, &tree, element(&book, "p"))
            .expect("a paragraph");

        for spec in PROPERTIES {
            let value = inspection
                .computed
                .get(spec.name)
                .unwrap_or_else(|| panic!("no computed {}", spec.name));
            if spec.name != "border" {
                assert!(!value.is_empty(), "{} computed to nothing", spec.name);
            }
        }
        assert_eq!(inspection.computed.len(), PROPERTIES.len());
        assert_eq!(inspection.computed["color"], "#008000");
        assert_eq!(inspection.computed["font-size"], "9pt");
        assert_eq!(inspection.computed["margin-top"], "9pt");
        assert_eq!(inspection.computed["margin-right"], "18pt");
        assert_eq!(inspection.computed["margin-left"], "0pt");
        assert_eq!(inspection.computed["margin"], "9pt 18pt 9pt 0pt");
    }

    #[test]
    fn a_margin_box_answers_with_its_page_and_the_rules_that_set_it() {
        let book = book();
        let css = "@page :left { @top-left { content: \"Left\"; color: red } }
@page { @top-left { color: blue } }";
        let (sheets, tree) = compiled(&book, css);
        let left = PageQuery {
            name: None,
            situation: Situation::Body(Side::Verso),
        };
        let inspection = sheets
            .inspect_margin_box(&tree, left, MarginBox::TopLeft)
            .expect("the left page names @top-left");

        assert_eq!(inspection.element, "@top-left");
        assert_eq!(inspection.page.as_deref(), Some("@page :left"));
        assert_eq!(inspection.node, None);
        type Seen<'a> = (&'a str, u32, [u32; 3], Vec<(&'a str, &'a str, bool)>);
        let rules: Vec<Seen<'_>> = author_rules(&inspection)
            .iter()
            .map(|rule| {
                (
                    rule.selector.as_str(),
                    rule.line,
                    rule.specificity,
                    rule.declarations
                        .iter()
                        .map(|d| (d.property.as_str(), d.value.as_str(), d.applied))
                        .collect(),
                )
            })
            .collect();
        assert_eq!(
            rules,
            [
                ("@page", 2, [0, 0, 0], vec![("color", "blue", false)]),
                (
                    "@page :left",
                    1,
                    [0, 0, 1],
                    vec![("content", "\"Left\"", true), ("color", "red", true)]
                ),
            ]
        );
        assert_eq!(inspection.computed["content"], "\"Left\"");
        assert_eq!(inspection.computed["color"], "#ff0000");

        let right = PageQuery {
            name: None,
            situation: Situation::Body(Side::Recto),
        };
        let inspection = sheets
            .inspect_margin_box(&tree, right, MarginBox::TopLeft)
            .expect("every page names @top-left");
        assert_eq!(inspection.page.as_deref(), Some("@page :right"));
        assert_eq!(author_rules(&inspection).len(), 1);
        assert_eq!(inspection.computed["color"], "#0000ff");
    }

    #[test]
    fn a_synthesized_or_unknown_node_answers_nothing() {
        let book = book();
        let (sheets, tree) = compiled(&book, CSS);
        assert_eq!(sheets.inspect(&book, &tree, NodeId::UNASSIGNED), None);
        assert_eq!(sheets.inspect(&book, &tree, NodeId::new(u32::MAX)), None);
        let page = PageQuery {
            name: None,
            situation: Situation::Body(Side::Recto),
        };
        assert_eq!(
            sheets.inspect_margin_box(&tree, page, MarginBox::RightTop),
            None
        );
    }
}
