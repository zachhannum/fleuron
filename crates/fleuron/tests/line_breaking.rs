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
    line.width as f32 / upem as f32 * ParagraphStyle::BODY.size
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
            ParagraphStyle::BODY,
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
            ParagraphStyle::BODY,
            measure,
            LineBreakOptions::default(),
        );
        let second = layout.layout(
            &inlines_of(&text),
            ParagraphStyle::BODY,
            measure,
            LineBreakOptions::default(),
        );
        prop_assert_eq!(first, second);
    }

    /// Every letter survives layout, in order: breaking never drops
    /// or reorders content.
    #[test]
    fn every_letter_survives_in_order(text in text_strategy(), measure in 30.0f32..300.0) {
        let layout = LineLayout::new(registry());
        let lines = layout.layout(
            &inlines_of(&text),
            ParagraphStyle::BODY,
            measure,
            LineBreakOptions::default(),
        );
        // Spaces may be consumed at line edges; letters may not be.
        let letters: usize = text.chars().filter(|c| c.is_ascii_alphabetic()).count();
        let total_glyphs: usize = lines
            .iter()
            .map(|l| l.runs.iter().map(|r| r.glyphs.len()).sum::<usize>())
            .sum();
        prop_assert!(total_glyphs >= letters, "dropped letters");
        // Clusters strictly increase down the paragraph.
        let clusters: Vec<u32> = lines
            .iter()
            .flat_map(|l| l.runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.cluster)))
            .collect();
        let mut sorted = clusters.clone();
        sorted.sort_unstable();
        prop_assert_eq!(clusters, sorted, "glyph order violated document order");
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
            ParagraphStyle::BODY,
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
