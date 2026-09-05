//! Property tests for the line box model: baseline rhythm, strut
//! floor, height/leading identity.

use fleuron::content::{Inline, NodeId};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::linebox::LineBox;
use fleuron::lines::{LineBreakOptions, LineLayout, ParagraphStyle};
use proptest::prelude::*;

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

/// The body style the built-in sheet computes: what these properties
/// come to for the styling a book gets by default.
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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Consecutive baselines are spaced exactly `line_height × size`
    /// apart: stacking line boxes by height, baseline n sits one
    /// leading below baseline n−1, however the text broke.
    #[test]
    fn baselines_tick_at_the_leading(
        text in text_strategy(),
        measure in 30.0f32..300.0,
        line_height in 0.8f32..2.5,
    ) {
        let style = ParagraphStyle {
            line_height,
            ..body()
        };
        let layout = LineLayout::new(registry());
        let lines = layout.layout(&inlines_of(&text), style, measure, LineBreakOptions::default());
        let leading = line_height * style.size;
        let mut baselines = Vec::with_capacity(lines.len());
        let mut top = 0.0f32;
        for line in &lines {
            baselines.push(top + line.box_.baseline);
            top += line.box_.height;
        }
        for (i, window) in baselines.windows(2).enumerate() {
            let spacing = window[1] - window[0];
            prop_assert!((spacing - leading).abs() < 1e-4,
                "baselines {i}→{} are {spacing}pt apart, leading is {leading}pt", i + 1);
        }
    }

    /// No line is shorter than the strut: the paragraph's minimum
    /// geometry stands whatever lands on the line.
    #[test]
    fn no_line_is_shorter_than_the_strut(
        text in text_strategy(),
        measure in 30.0f32..300.0,
        line_height in 0.8f32..2.5,
    ) {
        let style = ParagraphStyle {
            line_height,
            ..body()
        };
        let layout = LineLayout::new(registry());
        let strut = layout.strut(style);
        let lines = layout.layout(&inlines_of(&text), style, measure, LineBreakOptions::default());
        for (i, line) in lines.iter().enumerate() {
            prop_assert!(line.box_.height >= strut.height() - 1e-5,
                "line {i} is {}pt, strut is {}pt", line.box_.height, strut.height());
        }
    }

    /// Mixed-size runs: the shared baseline sits at or below every
    /// run's own ascent, and the line never drops below the strut.
    #[test]
    fn mixed_runs_grow_the_line_around_one_baseline(
        big in 12.0f32..48.0,
        small in 6.0f32..11.0,
        line_height in 0.8f32..2.5,
    ) {
        let style = ParagraphStyle {
            line_height,
            ..body()
        };
        let layout = LineLayout::new(registry());
        let strut = layout.strut(style);
        let line_box = layout.line_box(&[run(big), run(small)], style);
        let big_strut = fleuron::linebox::Strut::from_metrics(
            registry().metrics(0).unwrap(), big, style.line_height);
        prop_assert!(line_box.baseline >= big_strut.above - 1e-5);
        prop_assert!(line_box.height >= strut.height() - 1e-5);
    }
}

fn run(size: f32) -> fleuron::lines::ShapedRun {
    fleuron::lines::ShapedRun {
        font_id: 0,
        size,
        text: String::new(),
        text_start: 0,
        glyphs: Vec::new(),
        advance: 0,
    }
}

/// LineBox implements the identity downstream positioning relies on:
/// baseline within height, always.
#[test]
fn line_box_baseline_sits_within_height() {
    let layout = LineLayout::new(registry());
    let line_box: LineBox = layout.line_box(&[], body());
    assert!(line_box.baseline > 0.0 && line_box.baseline < line_box.height);
}
