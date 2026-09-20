//! `font-feature-settings` and the `font-variant` longhands: a sheet
//! asking a face for a feature it carries, and what the engine does
//! with a face that carries none.
//!
//! Each test sets one paragraph under a sheet of its own and reads
//! the glyphs back off the page, because a feature is a claim about
//! which glyphs a run is shaped from.

use fleuron::content::{Attributes, Block, Book, Inline, NodeId, Section};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::layout::Paginator;
use fleuron::lines::{Line, LineBreakOptions, LineLayout, Measure};
use fleuron::pages::{DrawItem, Page};
use fleuron::style::{Source, StyleTree, Stylesheets};

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

/// A page wide enough for the fixture to set on one line.
const PAGE_CSS: &str = "@page { size: 400pt 200pt; margin: 20pt }";

fn book_of(words: &str) -> Book {
    let mut book = Book {
        metadata: Default::default(),
        sections: vec![Section {
            attributes: Default::default(),
            id: NodeId::UNASSIGNED,
            source: None,
            title: None,
            blocks: vec![Block::Paragraph {
                id: NodeId::UNASSIGNED,
                inlines: vec![Inline::Text {
                    id: NodeId::UNASSIGNED,
                    value: words.into(),
                    attributes: Attributes::default(),
                    position: None,
                    span: None,
                }],
                attributes: Attributes::default(),
                position: None,
                span: None,
            }],
            position: None,
            span: None,
        }],
    };
    book.assign_node_ids();
    book
}

fn styles(book: &Book, css: &str) -> StyleTree {
    Stylesheets::parse(&[Source::author(
        "features.css",
        &format!("{PAGE_CSS}\n{css}"),
    )])
    .compile(book, registry())
}

/// The glyphs of the run holding `words`, with the text it was
/// shaped from.
fn run_of(page: &Page, words: &str) -> (String, Vec<u32>) {
    page.items
        .iter()
        .find_map(|item| match item {
            DrawItem::Text { text, glyphs, .. } if text.contains(words) => Some((
                text.clone(),
                glyphs.iter().map(|glyph| glyph.id).collect::<Vec<_>>(),
            )),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no run holding `{words}`: {:#?}", page.items))
}

/// The run holding `words`, set under `css`.
fn set(words: &str, css: &str) -> (String, Vec<u32>) {
    let book = book_of(words);
    let styles = styles(&book, css);
    let pages = Paginator::new(registry(), &styles).paginate(&book);
    run_of(&pages[0], words)
}

/// Acceptance: `font-feature-settings: "ss01" 1` draws the face's
/// first stylistic set.
#[test]
fn a_stylistic_set_draws_the_alternates_the_face_carries() {
    let words = "The quick brown fox jumps over the lazy dog";
    let (plain_text, plain) = set(words, "");
    let (text, alternates) = set(words, "p { font-feature-settings: \"ss01\" 1 }");
    assert_eq!(text, plain_text, "the text the glyphs stand for changed");
    assert_ne!(
        alternates, plain,
        "the stylistic set drew the glyphs the face draws by default",
    );
}

/// Acceptance: `font-variant-numeric: oldstyle-nums` draws old-style
/// figures from a face that has them, and the named form asks for
/// what the tag asks for.
#[test]
fn old_style_figures_come_of_the_named_form() {
    let (_, lining) = set("Printed in 1805", "");
    let (_, old_style) = set(
        "Printed in 1805",
        "p { font-variant-numeric: oldstyle-nums }",
    );
    let (_, by_tag) = set("Printed in 1805", "p { font-feature-settings: \"onum\" 1 }");
    assert_ne!(old_style, lining, "the figures are the lining ones");
    assert_eq!(old_style, by_tag, "the named form asked for another tag");
}

/// Acceptance: `font-feature-settings: "liga" 0` turns off a
/// ligature the shaper would otherwise form.
#[test]
fn a_setting_of_zero_turns_off_a_ligature_the_shaper_forms() {
    let (_, formed) = set("office", "");
    let (text, letters) = set("office", "p { font-feature-settings: \"liga\" 0 }");
    assert_eq!(text, "office");
    assert_eq!(letters.len(), 6, "a glyph for each letter");
    assert!(
        formed.len() < letters.len(),
        "the ffi ligature forms when nothing turns it off",
    );
}

/// Acceptance: a tag no face carries warns, naming the family, and
/// the run sets.
#[test]
fn a_tag_the_face_does_not_carry_warns_and_the_run_sets() {
    let words = "A run the face has no feature for";
    let book = book_of(words);
    let styles = styles(&book, "p { font-feature-settings: \"zero\" 1 }");
    let named: Vec<&str> = styles
        .warnings()
        .iter()
        .map(|warning| warning.message.as_str())
        .collect();
    assert_eq!(
        named,
        ["eb garamond has no `zero` feature. The text is set without it."],
        "{:?}",
        styles.warnings(),
    );
    let pages = Paginator::new(registry(), &styles).paginate(&book);
    assert_eq!(
        run_of(&pages[0], words),
        set(words, ""),
        "the run did not set the way it sets without the feature",
    );
}

/// Acceptance: a tracked and featured title breaks where its shaped
/// advances put it.
///
/// The total-fit pass measures the shaped run, so a feature that
/// changes an advance changes where the lines end, and every line
/// still fits the measure.
#[test]
fn a_tracked_and_featured_title_breaks_where_its_advances_put_it() {
    let words = "The first stately castle stands against the coast";
    let layout = LineLayout::new(registry());
    let book = book_of(words);
    let lines = |css: &str| {
        let styles = styles(&book, css);
        let node = styles
            .nodes()
            .iter()
            .find(|node| node.element == "p")
            .expect("a paragraph")
            .id;
        let paragraph = styles.paragraph(NodeId::new(node));
        let inlines = vec![Inline::Text {
            id: NodeId::UNASSIGNED,
            value: words.into(),
            attributes: Attributes::default(),
            position: None,
            span: None,
        }];
        layout.layout(
            &inlines,
            &paragraph,
            Measure::from(140.0),
            LineBreakOptions::default(),
        )
    };
    let tracked = "p { letter-spacing: 0.1em; font-size: 14pt }";
    let plain = lines(tracked);
    let featured = lines(&format!(
        "{tracked}\np {{ font-feature-settings: \"ss01\" 1 }}"
    ));
    let text_of = |lines: &[Line]| {
        lines
            .iter()
            .map(|line| {
                line.runs
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
    };
    assert_ne!(
        text_of(&featured),
        text_of(&plain),
        "the alternates the face drew did not reach the breaker",
    );
    for line in featured.iter().chain(&plain) {
        let width: f32 = line
            .runs
            .iter()
            .map(|run| {
                let upem = registry().metrics(run.font_id).unwrap().units_per_em as f32;
                run.advance as f32 / upem * run.size
            })
            .sum();
        assert!(width <= 140.0, "a line ran past the measure: {width}");
    }
}
