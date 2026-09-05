//! Property tests for line breaking: measure fit, determinism,
//! content preservation.

use fleuron::content::{Inline, NodeId};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::lines::{Line, LineBreakOptions, LineLayout, Measure, ParagraphStyle};
use fleuron::style::{FontVariantCaps, TextTransform};
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

/// Justification on, everything else at its default.
fn justified() -> LineBreakOptions {
    LineBreakOptions {
        justify: true,
        ..Default::default()
    }
}

/// The body style with a title's tracking on it.
fn tracked(letter_spacing: f32) -> ParagraphStyle {
    ParagraphStyle {
        letter_spacing,
        ..body()
    }
}

/// A line's width in points, as the measure sees it: every run
/// against its own face and size, since font units do not commute
/// across sizes.
fn width_of(line: &Line) -> f32 {
    line.runs
        .iter()
        .map(|run| {
            let upem = registry().metrics(run.font_id).unwrap().units_per_em as f32;
            run.advance as f32 / upem * run.size
        })
        .sum::<f32>()
        - line.overhang
        - line.protrusion
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
    /// glyph — `ff` is a single glyph at the cluster of the first
    /// `f`, so a letter count is not a glyph count. Layout is
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

    /// No line exceeds the measure when a first-line indent
    /// shortens it. The indent comes out of the measure rather than
    /// hanging off its end, so the first line, placed that far in,
    /// still finishes inside the block, ragged or justified.
    #[test]
    fn no_indented_line_exceeds_the_measure(
        text in text_strategy(),
        measure in 60.0f32..300.0,
        indent in 0.0f32..50.0,
    ) {
        let spec = Measure { full: measure, narrow: measure - indent, shortened: 1 };
        let layout = LineLayout::new(registry());
        for options in [LineBreakOptions::default(), justified()] {
            let lines = layout.layout(&inlines_of(&text), body(), spec, options);
            for (i, line) in lines.iter().enumerate() {
                if is_single_word(line) {
                    continue;
                }
                let width = width_pt(line);
                let allowed = spec.at(i);
                prop_assert!(
                    width <= allowed + 0.01,
                    "line {i} is {width}pt, measure {allowed}pt"
                );
                let start = if i == 0 { indent } else { 0.0 };
                prop_assert!(
                    start + width <= measure + 0.01,
                    "line {i} runs {}pt past a {measure}pt measure",
                    start + width - measure
                );
            }
        }
    }

    /// Justified layout is deterministic too: the adjustment is a
    /// function of the line, not of any state the pass keeps between
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

    /// Tracking is width the breaker measures. A title set with it is
    /// wider than the same title without, by the tracking times the
    /// gaps between its glyphs, and no line of tracked text runs past
    /// the measure.
    #[test]
    fn tracking_widens_the_line_and_still_fits_the_measure(
        text in text_strategy(),
        measure in 60.0f32..300.0,
        letter_spacing in 0.01f32..1.5,
    ) {
        let layout = LineLayout::new(registry());
        let style = tracked(letter_spacing);
        let plain = layout.layout(&inlines_of(&text), body(), 1.0e6, LineBreakOptions::default());
        let wide = layout.layout(&inlines_of(&text), style, 1.0e6, LineBreakOptions::default());
        // Tracking goes after a cluster, not after a glyph: a
        // ligature is one letter's worth of gap, not two.
        let mut clusters: Vec<u32> = plain[0]
            .runs
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.cluster))
            .collect();
        clusters.dedup();
        let gaps = (clusters.len() - 1) as f32;
        // Advances are integers, so each gap may be half a font unit
        // out; at 11pt over 1000 units to the em that is 0.006pt.
        let upem = registry().metrics(0).unwrap().units_per_em as f32;
        let slack = gaps * 0.5 / upem * body().size + 0.01;
        prop_assert!(
            (width_of(&wide[0]) - width_of(&plain[0]) - letter_spacing * gaps).abs() < slack,
            "a tracked title is not {gaps} gaps wider than an untracked one",
        );

        let lines = layout.layout(&inlines_of(&text), style, measure, LineBreakOptions::default());
        for (i, line) in lines.iter().enumerate() {
            let width = width_of(line);
            prop_assert!(
                width <= measure + 0.01 || is_single_word(line),
                "tracked line {i} is {width}pt, measure {measure}pt"
            );
        }
    }

    /// Tracking does not cost justification its exactness: the glue
    /// still opens to the measure, to the same tolerance the untracked
    /// paragraph keeps to.
    #[test]
    fn justified_tracked_lines_hit_the_measure(
        text in text_strategy(),
        measure in 60.0f32..300.0,
        letter_spacing in 0.01f32..1.5,
    ) {
        let layout = LineLayout::new(registry());
        let lines = layout.layout(&inlines_of(&text), tracked(letter_spacing), measure, justified());
        if lines.len() < 2 {
            return Ok(());
        }
        for (i, line) in lines[..lines.len() - 1].iter().enumerate() {
            if is_single_word(line) {
                continue;
            }
            let width = width_of(line);
            prop_assert!(
                (width - measure).abs() < 0.01,
                "justified tracked line {i} is {width}pt, measure {measure}pt"
            );
        }
    }

    /// Display typography is deterministic too: tracking, small
    /// capitals and a transform are functions of the style, not of
    /// anything a pass keeps between runs.
    #[test]
    fn display_typography_is_deterministic(
        text in text_strategy(),
        measure in 20.0f32..300.0,
        letter_spacing in 0.0f32..1.5,
    ) {
        let layout = LineLayout::new(registry());
        let style = ParagraphStyle {
            letter_spacing,
            caps: FontVariantCaps::SmallCaps,
            transform: TextTransform::Capitalize,
            ..body()
        };
        let first = layout.layout(&inlines_of(&text), style, measure, justified());
        let second = layout.layout(&inlines_of(&text), style, measure, justified());
        prop_assert_eq!(first, second);
    }
}
