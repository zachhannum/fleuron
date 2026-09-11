//! Line layout: text in, broken lines out.
//!
//! Knuth-Plass total fit. A paragraph is flattened into style runs,
//! shaped, and modelled as boxes, glue and penalties; the breaker
//! picks the set of breaks with the fewest demerits over the whole
//! paragraph rather than the most text on each line. The break
//! source is UAX #14 with word boundaries from UAX #29, plus
//! optional hyphenation, which enters as a flagged penalty.
//!
//! Justified text has its glue stretched or shrunk to the measure
//! here. The adjustment lands on the glyphs' own advances, so a
//! painter positions what it is given and never re-derives spacing.
//! A `Line` is shaped runs: measurement happened here.
//!
//! Units: advances come out of the shaper in font units; the measure
//! arrives in points and converts once, via `units_per_em * size`.
//!
//! The stages have a file each: `flatten` makes one string of styled
//! spans, `shape` turns spans into glyphs, `opportunity` says where a
//! line may end, `breaker` picks the breaks, `justify` moves the glue
//! on a line that was set to its measure, and `line` is what comes
//! out.

use crate::content::Inline;
use crate::fonts::FontRegistry;
use crate::linebox::{LineBox, Strut};
use icu_segmenter::{WordSegmenter, options::WordBreakInvariantOptions};

mod breaker;
mod flatten;
mod justify;
mod line;
mod measure;
mod opportunity;
mod paragraph;
mod shape;

#[cfg(test)]
mod testing;

pub use line::{Line, LineSpan, ShapedRun, Spans};
pub use measure::{Measure, Span};
pub use paragraph::{
    FirstLine, Generated, HangEnd, HangingPunctuation, Inherited, InlineStyles, LineBreakOptions,
    Opening, ParagraphStyle, Patterns,
};

use breaker::Breaker;
use flatten::FlatParagraph;
use justify::adjust;
use line::{Widths, cut_runs, gather, tile};
use paragraph::Lead;
use shape::ShapedSpan;

/// The layout pass: one paragraph → lines that fit the measure.
pub struct LineLayout<'a> {
    registry: &'a FontRegistry,
    segmenter: WordSegmenterBorrowedStatic,
}

/// The borrowed, 'static segmenter `WordSegmenter::new_auto` returns.
type WordSegmenterBorrowedStatic = icu_segmenter::WordSegmenterBorrowed<'static>;

impl<'a> LineLayout<'a> {
    /// A layout pass over the faces in `registry`.
    pub fn new(registry: &'a FontRegistry) -> Self {
        LineLayout {
            registry,
            segmenter: WordSegmenter::new_auto(WordBreakInvariantOptions::default()),
        }
    }
}

impl LineLayout<'_> {
    /// The paragraph's strut: the minimum box every one of its lines
    /// occupies, whatever the runs on it.
    pub fn strut(&self, style: ParagraphStyle) -> Strut {
        self.registry
            .metrics(style.font_id)
            .map(|m| Strut::from_metrics(m, style.size, style.line_height))
            .unwrap_or_default()
    }

    /// The box one line occupies: the strut, grown by any run taller
    /// than it around the shared baseline.
    pub fn line_box(&self, runs: &[ShapedRun], style: ParagraphStyle) -> LineBox {
        let strut = self.strut(style);
        let mut above = strut.above;
        let mut below = strut.below;
        for run in runs {
            let Some(metrics) = self.registry.metrics(run.font_id) else {
                continue;
            };
            let run_strut = Strut::from_metrics(metrics, run.size, style.line_height);
            above = above.max(run_strut.above);
            below = below.max(run_strut.below);
        }
        LineBox {
            baseline: above,
            height: above + below,
        }
    }

    /// Breaks one paragraph into lines of at most `measure_pt`
    /// points, every inline taking the block's own style.
    pub fn layout(
        &self,
        inlines: &[Inline],
        style: ParagraphStyle,
        measure: impl Into<Measure>,
        options: LineBreakOptions,
    ) -> Vec<Line> {
        self.layout_styled(
            inlines,
            style,
            &Inherited,
            &measure.into(),
            options,
            Opening::default(),
        )
    }

    /// The same, with the style tree answering for each inline and
    /// `opening` saying what sets the line the paragraph opens on
    /// apart from the rest of it.
    pub fn layout_styled(
        &self,
        inlines: &[Inline],
        style: ParagraphStyle,
        styles: &dyn InlineStyles,
        measure: &Measure,
        options: LineBreakOptions,
        opening: Opening,
    ) -> Vec<Line> {
        self.broken(inlines, style, styles, measure, options, opening)
            .0
    }

    /// The same, handing back the shaped paragraph beside the lines.
    ///
    /// A caller that can break this paragraph again keeps it.
    /// [`LineLayout::rebreak`] sets the same text to a different
    /// profile without shaping it twice.
    pub fn layout_shaped(
        &self,
        inlines: &[Inline],
        style: ParagraphStyle,
        styles: &dyn InlineStyles,
        measure: &Measure,
        options: LineBreakOptions,
        opening: Opening,
    ) -> (Broken, Option<Shaped>) {
        let (broken, _, shaped) =
            self.broken_shaped(inlines, style, styles, measure, options, opening);
        (broken, shaped)
    }

    /// Breaks a paragraph that was already shaped to `measure`,
    /// starting from the end of the line `from`, which is `0` for the
    /// whole of it.
    ///
    /// The style over the opening line is not held to the text it
    /// covered before. The profile decides where the lines end. A
    /// line pinned to a width it no longer has runs past that width.
    pub fn rebreak(&self, shaped: &Shaped, measure: &Measure, from: usize) -> Broken {
        self.break_shaped(shaped, measure, from, None)
    }

    /// Breaks one paragraph, and how many times the breaker ran.
    ///
    /// A first-line style takes two runs. What it sets changes the
    /// width of the opening run, the width moves where the paragraph
    /// breaks, and the break decides how much text the opening line
    /// holds. The first run sets the paragraph's leading run in the
    /// opening style to the end and breaks the whole of it. The
    /// second sets as far as the extent that break gave and breaks
    /// again with the opening line ending there, so the style covers
    /// the text the line comes to hold. The count is fixed at two,
    /// because a line count that depended on how long a paragraph
    /// took to settle would not be deterministic.
    fn broken(
        &self,
        inlines: &[Inline],
        style: ParagraphStyle,
        styles: &dyn InlineStyles,
        measure: &Measure,
        options: LineBreakOptions,
        opening: Opening,
    ) -> (Vec<Line>, u8) {
        let (broken, runs, _) =
            self.broken_shaped(inlines, style, styles, measure, options, opening);
        (broken.lines, runs)
    }

    /// The same, handing back what the paragraph was shaped from.
    fn broken_shaped(
        &self,
        inlines: &[Inline],
        style: ParagraphStyle,
        styles: &dyn InlineStyles,
        measure: &Measure,
        options: LineBreakOptions,
        opening: Opening,
    ) -> (Broken, u8, Option<Shaped>) {
        let lead = Lead {
            style: opening.first_line,
            extent: None,
            taken: opening.taken,
        };
        let once = |lead| {
            let shaped = self.shaped(inlines, style, styles, options, lead);
            let broken = shaped
                .as_ref()
                .map(|shaped| self.break_shaped(shaped, measure, 0, shaped.opening))
                .unwrap_or_default();
            (broken, shaped)
        };
        let (broken, shaped) = once(lead);
        if lead.style.is_none() || broken.extent == 0 {
            return (broken, 1, shaped);
        }
        let lead = Lead {
            extent: Some(broken.extent),
            ..lead
        };
        let (broken, shaped) = once(lead);
        (broken, 2, shaped)
    }

    /// One paragraph flattened and shaped, which is everything about
    /// it that the measure does not decide.
    ///
    /// `lead` is the style the paragraph opens in, how far it reaches
    /// in bytes of that text, and what a drop cap took.
    fn shaped(
        &self,
        inlines: &[Inline],
        style: ParagraphStyle,
        styles: &dyn InlineStyles,
        options: LineBreakOptions,
        lead: Lead,
    ) -> Option<Shaped> {
        let flat = self.flatten(inlines, style, styles, lead);
        if flat.text.is_empty() {
            return None;
        }
        let upem = self.registry.metrics(style.font_id)?.units_per_em as f32;
        let spans = self.shape_spans(&flat, style, upem);
        Some(Shaped {
            flat,
            spans,
            style,
            options,
            upem,
            opening: lead.extent,
        })
    }

    /// Breaks a shaped paragraph to `measure`, starting from the
    /// break `from`.
    ///
    /// `opening` is where the line the paragraph opens on has to end,
    /// which is the extent of the style over it.
    fn break_shaped(
        &self,
        shaped: &Shaped,
        measure: &Measure,
        from: usize,
        opening: Option<usize>,
    ) -> Broken {
        let Shaped {
            flat,
            spans,
            style,
            options,
            upem,
            ..
        } = shaped;
        let (style, options, upem) = (*style, *options, *upem);
        // Points → font units: measure / size gives ems, ems *
        // units_per_em gives font units.
        let to_points = |units: f32| units / upem * style.size;

        let widths = Widths::build(&flat.text, spans);
        let hyphen = self.hyphen_advance(style) as f32;
        let breaks = self.break_points(&flat.text, &widths, hyphen, options);
        let breaker = Breaker {
            breaks: &breaks,
            widths: &widths,
            measure,
            rest: measure.at(measure.settled()),
            settled: measure.settled(),
            first_band: (0..).find(|slot| measure.at(*slot).ends_band).unwrap_or(0),
            upem,
            size: style.size,
            hyphen,
            options,
            from,
            opening: opening
                .and_then(|extent| breaks.iter().position(|at| at.content_end == extent)),
        };

        let mut broken = Broken::default();
        let mut start = breaks[from.min(breaks.len() - 1)].next;
        // The band being filled, and where its first span was set:
        // a span's offset is from there, so a painter handed the
        // line's leading edge places the rest. One buffer gathers the
        // spans of every band.
        let mut band: Option<(Line, f32)> = None;
        let mut spans = Vec::new();
        for fit in breaker.run() {
            let at = &breaks[fit.at];
            let span = measure.at(fit.slot);
            if at.content_end > start {
                let (line, origin) = band.get_or_insert_with(|| {
                    let mut line = Line::empty();
                    line.protrusion = to_points(fit.protrusion);
                    (line, span.origin)
                });
                let first = line.runs.len();
                line.runs
                    .extend(cut_runs(flat, &shaped.spans, start, at.content_end));
                adjust(&mut line.runs[first..], &flat.text, fit.ratio, options);
                if at.hyphen {
                    self.hyphenate(&mut line.runs, style);
                }
                let width = line.runs[first..].iter().map(|run| run.advance).sum();
                spans.push(LineSpan {
                    runs: first..line.runs.len(),
                    offset: span.origin - *origin,
                    width,
                });
                line.width += width;
            }
            start = at.next;
            if !span.ends_band {
                continue;
            }
            let Some((mut line, _)) = band.take() else {
                continue;
            };
            line.overhang = to_points(fit.overhang);
            line.box_ = self.line_box(&line.runs, style);
            line.spans = gather(&mut spans);
            if broken.lines.is_empty() {
                broken.extent = at.content_end;
            }
            broken.lines.push(line);
            broken.ends.push(fit.at);
        }
        // A paragraph that ran out inside a band still sets what it
        // reached.
        if let Some((mut line, _)) = band.take() {
            line.box_ = self.line_box(&line.runs, style);
            line.spans = gather(&mut spans);
            broken.lines.push(line);
            broken.ends.push(breaker.end());
        }
        tile(&mut broken.lines, flat);
        broken
    }
}

/// A paragraph shaped once, and everything about it the measure does
/// not decide: the flattened text, the runs it shaped into, and the
/// style and options it was set with.
///
/// Shaping is the expensive half of setting a paragraph, and it does
/// not depend on where the lines end. A caller that can break the
/// same paragraph again against a different profile keeps this. It
/// breaks the paragraph through [`LineLayout::rebreak`].
pub struct Shaped {
    flat: FlatParagraph,
    spans: Vec<ShapedSpan>,
    style: ParagraphStyle,
    options: LineBreakOptions,
    upem: f32,
    /// Where the line the paragraph opens on has to end, when an
    /// opening style covers exactly that much of the text.
    opening: Option<usize>,
}

impl std::fmt::Debug for Shaped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shaped")
            .field("text", &self.flat.text)
            .finish_non_exhaustive()
    }
}

impl Shaped {
    /// The style the paragraph is set in.
    pub fn style(&self) -> ParagraphStyle {
        self.style
    }
}

/// One paragraph broken into lines: the lines themselves, and where
/// each of them ended.
#[derive(Debug, Default)]
pub struct Broken {
    /// The lines, in reading order.
    pub lines: Vec<Line>,
    /// Where each line ended, as the paragraph counts the places it
    /// can be broken. A break from the end of a line sets the text
    /// that line left.
    pub ends: Vec<usize>,
    /// Where the first line ended in the shaped text.
    extent: usize,
}

#[cfg(test)]
mod tests {
    use crate::lines::flatten::SMALL_CAPS_RATIO;
    use crate::lines::testing::{
        OPENING, body, drawn, layout_body, layout_first, line_text, one_run, registry, units_per_em,
    };
    use crate::lines::{FirstLine, Inherited, LineBreakOptions, LineLayout, Measure, Opening};
    use crate::style::{FontVariantCaps, TextTransform};

    /// A paragraph shaped once breaks to the same lines as the same
    /// text shaped and broken together. A break from the end of one
    /// line sets the text that line left.
    #[test]
    fn a_shaped_paragraph_breaks_again_to_the_same_lines() {
        let layout = LineLayout::new(registry());
        let measure = Measure::uniform(120.0);
        let (first, shaped) = layout.layout_shaped(
            &one_run(OPENING),
            body(),
            &Inherited,
            &measure,
            LineBreakOptions::default(),
            Opening::default(),
        );
        let shaped = shaped.expect("the paragraph shaped");
        let lines = first.lines;
        assert!(lines.len() > 3, "{} lines is too few to cut", lines.len());

        let again = layout.rebreak(&shaped, &measure, 0);
        assert_eq!(drawn(&again.lines), drawn(&lines));
        assert_eq!(again.ends.len(), again.lines.len());

        let tail = layout.rebreak(&shaped, &measure, again.ends[1]);
        assert_eq!(drawn(&tail.lines), drawn(&lines[2..]));
    }

    /// The same paragraph broken to a narrower measure holds the same
    /// words in more lines. No line runs past the measure it was set
    /// to.
    #[test]
    fn a_shaped_paragraph_breaks_again_to_a_narrower_measure() {
        let layout = LineLayout::new(registry());
        let (wide, shaped) = layout.layout_shaped(
            &one_run(OPENING),
            body(),
            &Inherited,
            &Measure::uniform(200.0),
            LineBreakOptions::default(),
            Opening::default(),
        );
        let shaped = shaped.expect("the paragraph shaped");
        let lines = wide.lines;
        let narrow = Measure::uniform(100.0);
        let again = layout.rebreak(&shaped, &narrow, 0);
        assert!(again.lines.len() > lines.len());
        assert_eq!(
            drawn(&again.lines).join(" ").replace("  ", " "),
            drawn(&lines).join(" ").replace("  ", " ")
        );
        let upem = units_per_em() as f32;
        for line in &again.lines {
            let width = line.width as f32 / upem * body().size;
            assert!(width <= 100.0 + 0.5, "{width} runs past the measure");
        }
    }

    /// Empty paragraph → no lines.
    #[test]
    fn empty_paragraph_yields_no_lines() {
        assert!(layout_body("", 200.0).is_empty());
    }

    /// A word that fits stays on one line, and its width is the
    /// shaped advance of exactly its glyphs.
    #[test]
    fn short_text_is_one_line() {
        let lines = layout_body("hello", 200.0);
        assert_eq!(lines.len(), 1);
        assert_eq!(line_text(&lines[0]), "hello");
        let glyph_count: usize = lines[0].runs.iter().map(|r| r.glyphs.len()).sum();
        assert_eq!(glyph_count, 5);
    }

    /// Words flow to later lines once the measure overflows; nothing
    /// is lost and nothing is reordered.
    #[test]
    fn text_wraps_and_preserves_every_word() {
        let text = "one two three four five six seven eight";
        let lines = layout_body(text, 60.0);
        assert!(lines.len() >= 2, "expected wrapping, got {lines:?}");
        let reconstructed: String = lines
            .iter()
            .map(|l| line_text(l).trim_end().to_string())
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(reconstructed, text);
    }

    /// A paragraph with only one sensible answer gets it: everything
    /// that fits on one line stays on it.
    #[test]
    fn a_paragraph_with_one_answer_gets_it() {
        let text = "aa bb cc";
        let lines = layout_body(text, 100.0);
        assert_eq!(lines.len(), 1, "everything fits: {lines:?}");
        let lines = layout_body(text, 30.0);
        assert_eq!(line_text(&lines[0]), "aa bb");
    }

    /// A first-line style takes two runs of the breaker and a
    /// paragraph without one takes a single run, whether or not the
    /// two runs agree on where the paragraph breaks.
    #[test]
    fn a_first_line_style_breaks_the_paragraph_twice() {
        let layout = LineLayout::new(registry());
        let inlines = one_run(OPENING);
        let broken = |first_line| {
            layout
                .broken(
                    &inlines,
                    body(),
                    &Inherited,
                    &Measure::uniform(160.0),
                    LineBreakOptions::default(),
                    Opening {
                        first_line,
                        taken: 0,
                    },
                )
                .1
        };
        assert_eq!(broken(None), 1);
        assert_eq!(
            broken(Some(FirstLine {
                caps: Some(FontVariantCaps::SmallCaps),
                ..FirstLine::default()
            })),
            2,
        );
    }

    /// Small capitals on the opening line stop at the break the
    /// second run chose: the last word of the first line is set in
    /// them throughout and the first word of the second line is none
    /// of it. What the author wrote comes back off the runs either
    /// way.
    #[test]
    fn a_first_line_is_small_capitals_as_far_as_the_break() {
        // The face with no substitutions of its own synthesises its
        // small capitals, which puts the boundary in the drawn text.
        let bare = crate::fonts::registry_without_substitutions();
        let layout = LineLayout::new(&bare);
        let lines = layout_first(
            &layout,
            160.0,
            Some(FirstLine {
                caps: Some(FontVariantCaps::SmallCaps),
                ..FirstLine::default()
            }),
        );
        assert!(lines.len() > 2, "the paragraph did not break");

        let opening = line_text(&lines[0]);
        let next = line_text(&lines[1]);
        let last_word = opening.split_whitespace().next_back().expect("a word");
        let first_word = next.split_whitespace().next().expect("a word");
        assert_eq!(
            last_word,
            last_word.to_uppercase(),
            "the first line's last word is not small capitals throughout: {opening:?}",
        );
        assert_eq!(
            first_word,
            first_word.to_lowercase(),
            "the small capitals ran past the first line: {next:?}",
        );
        assert_eq!(opening, opening.to_uppercase());
        assert_eq!(next, next.to_lowercase());
        assert!(
            lines[0]
                .runs
                .iter()
                .any(|run| run.size == body().size * SMALL_CAPS_RATIO),
            "nothing on the opening line was set at the reduced size",
        );
        assert!(
            lines[1].runs.iter().all(|run| run.size == body().size),
            "the reduced size reached the second line",
        );

        // What the author wrote is on the runs beside what was drawn.
        let written: String = lines[0]
            .runs
            .iter()
            .map(|run| match run.source.is_empty() {
                true => run.text.as_str(),
                false => run.source.as_str(),
            })
            .collect();
        assert!(
            OPENING.starts_with(&written),
            "the opening line lost the manuscript: {written:?}",
        );
    }

    /// Tracking on the opening line is width like any other, so the
    /// line the paragraph breaks at holds less than it does
    /// untracked.
    #[test]
    fn a_tracked_first_line_breaks_earlier() {
        let layout = LineLayout::new(registry());
        let plain = layout_first(&layout, 160.0, None);
        let tracked = layout_first(
            &layout,
            160.0,
            Some(FirstLine {
                letter_spacing: Some(0.12 * body().size),
                ..FirstLine::default()
            }),
        );
        assert!(
            line_text(&tracked[0]).len() < line_text(&plain[0]).len(),
            "the tracked line held as much: {:?} against {:?}",
            line_text(&tracked[0]),
            line_text(&plain[0]),
        );
    }

    /// A first line set larger grows its own line box around the
    /// baseline the paragraph shares, and leaves the lines under it
    /// the size they were.
    #[test]
    fn a_first_line_set_larger_grows_its_line_box() {
        let layout = LineLayout::new(registry());
        let lines = layout_first(
            &layout,
            240.0,
            Some(FirstLine {
                size: Some(body().size * 1.6),
                ..FirstLine::default()
            }),
        );
        assert!(lines.len() > 1, "the paragraph did not break");
        assert!(
            lines[0].box_.height > lines[1].box_.height,
            "the opening line did not grow: {:?} against {:?}",
            lines[0].box_,
            lines[1].box_,
        );
        assert!(
            lines[0].box_.baseline > lines[1].box_.baseline,
            "the opening line grew below the baseline alone",
        );
        assert_eq!(
            lines[0].runs[0].size,
            body().size * 1.6,
            "the opening run was not set larger",
        );
        assert_eq!(lines[1].runs[0].size, body().size);
    }

    /// `text-transform` on the opening line changes what is shaped
    /// and leaves what the author wrote on the run beside it, the way
    /// it does on a whole paragraph.
    #[test]
    fn a_transformed_first_line_keeps_the_manuscript() {
        let layout = LineLayout::new(registry());
        let lines = layout_first(
            &layout,
            160.0,
            Some(FirstLine {
                transform: Some(TextTransform::Uppercase),
                ..FirstLine::default()
            }),
        );
        let opening = line_text(&lines[0]);
        assert_eq!(opening, opening.to_uppercase());
        assert_eq!(line_text(&lines[1]), line_text(&lines[1]).to_lowercase());
        let written: String = lines[0]
            .runs
            .iter()
            .map(|run| run.source.as_str())
            .collect();
        assert!(
            OPENING.starts_with(&written) && written == written.to_lowercase(),
            "the opening line lost the manuscript: {written:?}",
        );
    }
}
