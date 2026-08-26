//! Property test for the cascade: author CSS overrides the
//! user-agent sheet, and specificity then source order decide the
//! rest.
//!
//! The winner is computed twice — once by the engine, once by the
//! rules as CSS states them — over selectors of known specificity in
//! a shuffled order.

use fleuron::content::{Block, Book, Inline, NodeId, Section};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::style::{Source, StyleTree, Stylesheets};
use proptest::prelude::*;

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

/// One paragraph inside one section: enough tree for every selector
/// below to have something to match.
fn book() -> Book {
    let mut book = Book {
        metadata: Default::default(),
        sections: vec![Section {
            id: NodeId::UNASSIGNED,
            source: None,
            title: None,
            blocks: vec![Block::Paragraph {
                id: NodeId::UNASSIGNED,
                inlines: vec![Inline::Text {
                    id: NodeId::UNASSIGNED,
                    value: "prose".into(),
                    position: None,
                }],
                position: None,
            }],
            position: None,
        }],
    };
    book.assign_node_ids();
    book
}

/// Selectors that all match the paragraph, with the specificity CSS
/// gives them as `(id, class, type)` packed the way the cascade
/// compares it.
const SELECTORS: [(&str, u32); 6] = [
    ("*", 0),
    ("p", 1),
    ("section p", 2),
    ("book section p", 3),
    ("p:first-child", 1 << 10 | 1),
    ("section p:first-child", 1 << 10 | 2),
];

/// The paragraph's computed size under one set of rules.
fn computed(tree: &StyleTree) -> f32 {
    let node = tree
        .nodes()
        .iter()
        .find(|node| node.element == "p")
        .expect("the book has a paragraph");
    tree.styles()[node.style as usize].font_size
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Author rules beat the built-in sheet however specific it was,
    /// and among author rules the most specific wins, ties going to
    /// the one written last.
    #[test]
    fn the_cascade_picks_by_specificity_then_source_order(
        chosen in proptest::collection::vec(0usize..SELECTORS.len(), 1..8),
    ) {
        let book = book();
        // Each rule sets a size nothing else does, so the computed
        // value names the rule that won.
        let css: String = chosen
            .iter()
            .enumerate()
            .map(|(order, which)| {
                format!("{} {{ font-size: {}pt }}\n", SELECTORS[*which].0, 20 + order)
            })
            .collect();

        let expected = chosen
            .iter()
            .enumerate()
            .max_by_key(|(order, which)| (SELECTORS[**which].1, *order))
            .map(|(order, _)| 20 + order)
            .expect("at least one rule");

        let tree = Stylesheets::parse(&[Source::author("author.css", &css)])
            .compile(&book, registry());
        prop_assert_eq!(
            computed(&tree),
            expected as f32,
            "cascade picked wrongly for:\n{}",
            css
        );
    }

    /// Origin outranks specificity: however specific a user-agent
    /// rule is, the plainest author rule still wins.
    #[test]
    fn author_css_outranks_the_user_agent_sheet(which in 0usize..SELECTORS.len()) {
        let book = book();
        let user_agent = format!("{} {{ font-size: 99pt }}", SELECTORS[which].0);
        let tree = Stylesheets::parse(&[
            Source::user_agent("host.css", &user_agent),
            Source::author("author.css", "* { font-size: 17pt }"),
        ])
        .compile(&book, registry());
        prop_assert_eq!(computed(&tree), 17.0);
    }
}
