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

/// Justification on, everything else at its default.
fn justified() -> LineBreakOptions {
    LineBreakOptions {
        justify: true,
        ..Default::default()
    }
}

/// A line with no space glyph on it is a single word: the one thing
/// layout may set wider than the measure rather than drop.
fn is_single_word(line: &Line) -> bool {
    let space = registry().char_glyph(0, ' ').unwrap();
    line.runs.len() == 1 && line.runs[0].glyphs.iter().all(|g| g.id != space)
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
        for (i, line) in lines.iter().enumerate() {
            let width = width_pt(line);
            prop_assert!(
                width <= measure || is_single_word(line),
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

    /// No justified line exceeds the measure either. Justification
    /// shrinks the glue as well as stretching it, so this is the
    /// property that says the shrink is bounded.
    #[test]
    fn no_justified_line_exceeds_the_measure(text in text_strategy(), measure in 20.0f32..300.0) {
        let layout = LineLayout::new(registry());
        let lines = layout.layout(&inlines_of(&text), body(), measure, justified());
        for (i, line) in lines.iter().enumerate() {
            let width = width_pt(line);
            prop_assert!(
                width <= measure + 0.01 || is_single_word(line),
                "line {i} is {width}pt, measure {measure}pt"
            );
        }
    }

    /// Justified lines other than the last measure the measure
    /// exactly, to within 0.01pt: the adjustment is spread over
    /// integer font units, so a line can miss by half a unit, which
    /// at 11pt over 1000 units to the em is 0.006pt.
    ///
    /// A line with nothing to stretch is exempt: a single word has no
    /// glue, and the measure is not something it can be made to fill.
    #[test]
    fn justified_lines_hit_the_measure(text in text_strategy(), measure in 60.0f32..300.0) {
        let layout = LineLayout::new(registry());
        let lines = layout.layout(&inlines_of(&text), body(), measure, justified());
        if lines.len() < 2 {
            return Ok(());
        }
        for (i, line) in lines[..lines.len() - 1].iter().enumerate() {
            if is_single_word(line) {
                continue;
            }
            let width = width_pt(line);
            prop_assert!(
                (width - measure).abs() < 0.01,
                "justified line {i} is {width}pt, measure {measure}pt"
            );
        }
    }

    /// Justified layout is deterministic too: the adjustment is a
    /// function of the line, not of anything the pass carries between
    /// runs.
    #[test]
    fn justified_layout_is_deterministic(text in text_strategy(), measure in 20.0f32..300.0) {
        let layout = LineLayout::new(registry());
        let first = layout.layout(&inlines_of(&text), body(), measure, justified());
        let second = layout.layout(&inlines_of(&text), body(), measure, justified());
        prop_assert_eq!(first, second);
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
            LineBreakOptions {
                hyphenate: true,
                ..Default::default()
            },
        );
        for line in &lines {
            let width = width_pt(line);
            prop_assert!(
                width <= measure || is_single_word(line),
                "hyphenated line is {width}pt over measure {measure}pt"
            );
        }
    }
}
