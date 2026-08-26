//! Property tests for line breaking: measure fit, determinism,
//! content preservation.

use fleuron::content::{Inline, NodeId};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::lines::{Line, LineBreakOptions, LineLayout, ParagraphStyle};
use proptest::prelude::*;

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

/// The body style the built-in sheet computes: what these properties
/// hold for the styling a book gets by default.
fn body() -> ParagraphStyle {
    fleuron::style::defaults(&fleuron::content::Book::default(), registry())
        .root()
        .paragraph()
}

fn word_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z]{1,12}"
}

fn text_strategy() -> impl Strategy<Value = String> {
    proptest::collection::vec(word_strategy(), 1..20).prop_map(|words| words.join(" "))
}

fn inlines_of(text: &str) -> Vec<Inline> {
    vec![Inline::Text {
        id: NodeId::UNASSIGNED,
        value: text.to_string(),
        position: None,
    }]
}

fn width_pt(line: &Line) -> f32 {
    let upem = registry().metrics(0).unwrap().units_per_em;
    line.width as f32 / upem as f32 * body().size
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// No line exceeds the measure. The exception: a word wider than
    /// the whole measure overflows onto its own line rather than
    /// being dropped or split unhyphenated — such a line contains a
    /// single run with no space glyphs.
    #[test]
    fn no_line_exceeds_the_measure(text in text_strategy(), measure in 20.0f32..300.0) {
        let layout = LineLayout::new(registry());
        let lines = layout.layout(
            &inlines_of(&text),
            body(),
            measure,
            LineBreakOptions::default(),
        );
        let space_glyph = registry().char_glyph(0, ' ').unwrap();
        for (i, line) in lines.iter().enumerate() {
            let width = width_pt(line);
            let is_single_word = line.runs.len() == 1
                && line.runs[0].glyphs.iter().all(|g| g.id != space_glyph);
            prop_assert!(
                width <= measure || is_single_word,
                "line {i} is {width}pt, measure {measure}pt"
            );
        }
    }

    /// Layout is deterministic: two runs over the same input produce
    /// identical lines.
    #[test]
    fn layout_is_deterministic(text in text_strategy(), measure in 20.0f32..300.0) {
        let layout = LineLayout::new(registry());
        let first = layout.layout(
            &inlines_of(&text),
            body(),
            measure,
            LineBreakOptions::default(),
        );
        let second = layout.layout(
            &inlines_of(&text),
            body(),
            measure,
            LineBreakOptions::default(),
        );
        prop_assert_eq!(first, second);
    }

    /// Every glyph survives layout, in order: breaking never drops or
    /// reorders content. Stated over the shaper's output rather than
    /// the source letters, because ligatures merge letters into one
    /// glyph — `ff` is a single glyph carrying the cluster of the
    /// first `f`, so a letter count is not a glyph count. Layout is
    /// allowed to drop spaces at line edges and nothing else.
    #[test]
    fn every_glyph_survives_in_order(text in text_strategy(), measure in 30.0f32..300.0) {
        let layout = LineLayout::new(registry());
        let lines = layout.layout(
            &inlines_of(&text),
            body(),
            measure,
            LineBreakOptions::default(),
        );
        let space = registry().char_glyph(0, ' ').unwrap();
        let shaped: Vec<u32> = registry()
            .shape(0, &text)
            .unwrap()
            .iter()
            .filter(|g| g.id != space)
            .map(|g| g.cluster)
            .collect();
        let laid_out: Vec<u32> = lines
            .iter()
            .flat_map(|l| l.runs.iter().flat_map(|r| r.glyphs.iter()))
            .filter(|g| g.id != space)
            .map(|g| g.cluster)
            .collect();
        prop_assert_eq!(
            laid_out,
            shaped,
            "layout dropped or reordered the shaper's glyphs"
        );
    }

    /// Hyphenated layout fits the measure wherever a hyphenation
    /// point exists: a hyphenated break must charge for its hyphen.
    /// A word with no syllable break wider than the measure still
    /// overflows — hyphenation adds opportunities, it can't create
    /// them where the patterns have none.
    #[test]
    fn hyphenated_lines_fit_the_measure(text in text_strategy(), measure in 40.0f32..200.0) {
        let layout = LineLayout::new(registry());
        let lines = layout.layout(
            &inlines_of(&text),
            body(),
            measure,
            LineBreakOptions { hyphenate: true },
        );
        let space_glyph = registry().char_glyph(0, ' ').unwrap();
        for line in &lines {
            let width = width_pt(line);
            let is_single_word = line.runs.len() == 1
                && line.runs[0].glyphs.iter().all(|g| g.id != space_glyph);
            prop_assert!(
                width <= measure || is_single_word,
                "hyphenated line is {width}pt over measure {measure}pt"
            );
        }
    }
}
