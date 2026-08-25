//! Line layout: text in, broken lines out.
//!
//! Greedy first-fit to a measure, ragged right. A paragraph is
//! flattened into style runs, shaped, and broken at opportunities;
//! the break source is UAX #14 with word boundaries from UAX #29,
//! plus optional hyphenation. `Line` carries shaped runs — measurement
//! happened here; downstream stages position, never re-measure.
//!
//! Units: advances come out of the shaper in font units; the measure
//! arrives in points and converts once, via `units_per_em * size`.

use std::ops::Range;

use crate::content::Inline;
use crate::fonts::{FontRegistry, ShapedGlyph};
use crate::linebox::{LineBox, Strut};
use icu_segmenter::{WordSegmenter, options::WordBreakInvariantOptions};
use unicode_linebreak::{BreakOpportunity, linebreaks};

/// Everything one paragraph's layout depends on. The style tree
/// compiles down to this; layout never reads settings.
#[derive(Debug, Clone, Copy)]
pub struct ParagraphStyle {
    /// Face id from the font registry.
    pub font_id: u16,
    /// Font size in points.
    pub size: f32,
    /// Line height as a unitless multiple of `size`, as in CSS
    /// `line-height: <number>`.
    pub line_height: f32,
}

/// v0.1 body text: the bundled serif at book scale.
impl ParagraphStyle {
    pub const BODY: ParagraphStyle = ParagraphStyle {
        font_id: 0,
        size: 11.0,
        line_height: 1.4,
    };

    /// v0.1 chapter openings: the bundled serif at display size.
    pub const CHAPTER: ParagraphStyle = ParagraphStyle {
        font_id: 0,
        size: 18.0,
        line_height: 1.4,
    };

    /// v0.1 page furniture: the bundled serif at folio scale.
    pub const FOLIO: ParagraphStyle = ParagraphStyle {
        font_id: 0,
        size: 9.0,
        line_height: 1.4,
    };
}

/// Hyphenation is off unless asked for; when on, lines may end
/// inside words, at syllable boundaries, with a hyphen charged to the
/// line.
#[derive(Debug, Clone, Copy, Default)]
pub struct LineBreakOptions {
    pub hyphenate: bool,
}

/// A run of glyphs sharing one font and size — the paintable unit.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapedRun {
    pub font_id: u16,
    pub size: f32,
    /// The run's own text. Glyph ids alone do not spell anything:
    /// text extraction and copy-paste need the characters back, and
    /// only the shaper knows which glyph came from which of them.
    pub text: String,
    /// Byte offset of `text` in the paragraph the glyphs' clusters
    /// index.
    pub text_start: u32,
    pub glyphs: Vec<ShapedGlyph>,
    /// Total advance of the run's glyphs, in font units.
    pub advance: u32,
}

impl ShapedRun {
    /// The byte range in `text` each glyph stands for, in glyph
    /// order. A glyph covers its cluster up to the next cluster that
    /// starts later — which is how a ligature comes to span the
    /// characters it swallowed.
    pub fn glyph_ranges(&self) -> Vec<Range<u32>> {
        let end = self.text.len() as u32;
        let starts: Vec<u32> = self
            .glyphs
            .iter()
            .map(|g| g.cluster.saturating_sub(self.text_start).min(end))
            .collect();
        starts
            .iter()
            .enumerate()
            .map(|(i, start)| {
                let next = starts[i + 1..]
                    .iter()
                    .find(|later| *later > start)
                    .copied()
                    .unwrap_or(end);
                *start..next.max(*start)
            })
            .collect()
    }
}

/// One typeset line: shaped runs plus its width in font units.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub runs: Vec<ShapedRun>,
    /// Advance of the line's glyphs, trailing spaces excluded; a
    /// hyphenated line's hyphen is charged here even though the glyph
    /// joins the runs when the display list paints it.
    pub width: u32,
    /// The line's vertical geometry — computed here, in points;
    /// downstream stages position against it, never re-measure.
    pub box_: LineBox,
}

/// Flattened paragraph content: plain text plus, per style span, the
/// byte range it covers. Style boundaries are segmentation
/// boundaries — a shaped run never spans two fonts.
struct FlatParagraph {
    text: String,
    /// `(font_id, size, byte range into text)` in document order.
    spans: Vec<(u16, f32, Range<usize>)>,
}

fn flatten(inlines: &[Inline], style: ParagraphStyle) -> FlatParagraph {
    let mut flat = FlatParagraph {
        text: String::new(),
        spans: Vec::new(),
    };
    walk_inlines(inlines, style, &mut flat);
    flat
}

fn walk_inlines(inlines: &[Inline], style: ParagraphStyle, flat: &mut FlatParagraph) {
    for inline in inlines {
        match inline {
            Inline::Text { value, .. } | Inline::Code { value, .. } => {
                let start = flat.text.len();
                flat.text.push_str(value);
                flat.spans
                    .push((style.font_id, style.size, start..flat.text.len()));
            }
            Inline::Emphasis { children, .. }
            | Inline::Strong { children, .. }
            | Inline::Link { children, .. } => {
                // v0.1: emphasis and strong shape with the body face;
                // italic/bold faces arrive with the style compiler (#7).
                walk_inlines(children, style, flat);
            }
        }
    }
}

/// A candidate line end: the exclusive byte offset where a line may
/// end, plus the width contribution of a hyphen when the break falls
/// inside a word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Opportunity {
    /// Exclusive end of the line's text.
    end: usize,
    /// True when this break sits inside a word and the line must be
    /// charged for a hyphen glyph.
    hyphen: bool,
}

/// The layout pass: one paragraph → lines that fit the measure.
pub struct LineLayout<'a> {
    registry: &'a FontRegistry,
    segmenter: WordSegmenterBorrowedStatic,
}

/// The borrowed, 'static segmenter `WordSegmenter::new_auto` returns.
type WordSegmenterBorrowedStatic = icu_segmenter::WordSegmenterBorrowed<'static>;

impl<'a> LineLayout<'a> {
    pub fn new(registry: &'a FontRegistry) -> Self {
        LineLayout {
            registry,
            segmenter: WordSegmenter::new_auto(WordBreakInvariantOptions::default()),
        }
    }

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

    /// Breaks one paragraph into lines of at most `measure_pt` points.
    pub fn layout(
        &self,
        inlines: &[Inline],
        style: ParagraphStyle,
        measure_pt: f32,
        options: LineBreakOptions,
    ) -> Vec<Line> {
        let flat = flatten(inlines, style);
        if flat.text.is_empty() {
            return Vec::new();
        }
        let Some(metrics) = self.registry.metrics(style.font_id) else {
            return Vec::new();
        };
        // Points → font units, once: measure_pt / size gives ems,
        // ems * units_per_em gives font units.
        let measure_units = measure_pt / style.size * metrics.units_per_em as f32;

        // Shape each span; glyphs carry cluster offsets into the span,
        // which index the paragraph text once offset by span start.
        let shaped: Vec<ShapedSpan> = flat
            .spans
            .iter()
            .map(|(font_id, _, range)| ShapedSpan {
                range: range.clone(),
                glyphs: self
                    .registry
                    .shape(*font_id, &flat.text[range.clone()])
                    .unwrap_or_default(),
            })
            .collect();
        // The space glyph's advance, for measuring candidate lines
        // without their trailing spaces.
        let space_advance = self
            .registry
            .char_glyph(style.font_id, ' ')
            .and_then(|g| self.registry.advance_width(style.font_id, g))
            .unwrap_or(0) as u32;

        // Every line of this paragraph carries the same box unless a
        // run on it is taller than the strut (v0.1: none — spans share
        // the paragraph style; mixed sizes arrive with the style
        // compiler).
        let line_box = self.line_box(&[], style);

        let opportunities = self.opportunities(&flat.text, options);
        let hyphen_advance = self.hyphen_advance(style);

        let mut lines = Vec::new();
        let mut line_start = 0usize;
        while line_start < flat.text.len() {
            // Candidate ends for this line, in order: the first
            // opportunity whose width fits wins. Trailing spaces are
            // not charged; a hyphenated candidate is.
            let mut last_fitting: Option<(usize, bool)> = None;
            for opportunity in opportunities.iter().filter(|o| o.end > line_start) {
                let mut width = spanned_width(&shaped, line_start, opportunity.end);
                width -=
                    trailing_spaces(&flat.text, line_start, opportunity.end) as u32 * space_advance;
                if opportunity.hyphen {
                    width += hyphen_advance;
                }
                if width <= measure_units as u32 {
                    last_fitting = Some((opportunity.end, opportunity.hyphen));
                } else {
                    break;
                }
            }
            // Greedy: keep the LAST candidate that fits — lines carry
            // as much text as the measure allows.
            match last_fitting {
                Some((end, _hyphen)) => {
                    let content_end = {
                        let mut e = end;
                        while e > line_start && flat.text.as_bytes()[e - 1] == b' ' {
                            e -= 1;
                        }
                        e
                    };
                    if content_end <= line_start {
                        break; // nothing paintable left (trailing spaces)
                    }
                    lines.push(cut_line(&flat, &shaped, line_start, end, line_box));
                    line_start = skip_spaces(&flat.text, end);
                }
                None => {
                    // No opportunity fits: a single unit longer than
                    // the measure. Overflow rather than drop text.
                    let end = opportunities
                        .iter()
                        .filter(|o| o.end > line_start)
                        .map(|o| o.end)
                        .min()
                        .unwrap_or(flat.text.len());
                    lines.push(cut_line(&flat, &shaped, line_start, end, line_box));
                    line_start = skip_spaces(&flat.text, end);
                }
            }
        }
        lines
    }

    /// Break opportunities for the paragraph: UAX #14 always, UAX #29
    /// word boundaries to bound hyphenation, `hypher` for syllables
    /// when enabled.
    fn opportunities(&self, text: &str, options: LineBreakOptions) -> Vec<Opportunity> {
        let mut opportunities: Vec<Opportunity> = linebreaks(text)
            .filter(|(_, kind)| {
                matches!(
                    kind,
                    BreakOpportunity::Allowed | BreakOpportunity::Mandatory
                )
            })
            .map(|(index, _)| Opportunity {
                end: index,
                hyphen: false,
            })
            .collect();
        if options.hyphenate {
            self.add_hyphenation(text, &mut opportunities);
        }
        opportunities.sort_unstable_by_key(|o| o.end);
        opportunities.dedup_by_key(|o| o.end);
        opportunities
    }

    fn add_hyphenation(&self, text: &str, opportunities: &mut Vec<Opportunity>) {
        use hypher::Lang;
        let mut start = 0usize;
        for boundary in self.segmenter.segment_str(text) {
            let word = &text[start..boundary];
            let is_word = !word.is_empty()
                && word
                    .chars()
                    .all(|c| c.is_ascii_alphabetic() || c == '\'' || c == '-');
            if is_word {
                let syllables: Vec<&str> = hypher::hyphenate(word, Lang::English).collect();
                let mut offset = start;
                for syllable in syllables.iter().take(syllables.len().saturating_sub(1)) {
                    offset += syllable.len();
                    if offset > start && offset < boundary {
                        opportunities.push(Opportunity {
                            end: offset,
                            hyphen: true,
                        });
                    }
                }
            }
            start = boundary;
        }
    }

    fn hyphen_advance(&self, style: ParagraphStyle) -> u32 {
        self.registry
            .char_glyph(style.font_id, '-')
            .and_then(|g| self.registry.advance_width(style.font_id, g))
            .unwrap_or(0) as u32
    }
}

/// One shaped span, its glyphs still carrying cluster offsets relative
/// to the span's own text.
struct ShapedSpan {
    /// Byte range of the span in the paragraph text.
    range: Range<usize>,
    glyphs: Vec<ShapedGlyph>,
}

impl ShapedSpan {
    /// Advance of the glyphs whose clusters fall in `[start, end)` of
    /// the paragraph text.
    fn width_in(&self, start: usize, end: usize) -> u32 {
        self.glyphs
            .iter()
            .filter(|g| {
                let cluster = self.range.start + g.cluster as usize;
                cluster >= start && cluster < end
            })
            .map(|g| g.x_advance)
            .sum()
    }

    /// The glyphs whose clusters fall in `[start, end)`, clusters
    /// rebased to the paragraph text.
    fn glyphs_in(&self, start: usize, end: usize) -> Vec<ShapedGlyph> {
        self.glyphs
            .iter()
            .filter(|g| {
                let cluster = self.range.start + g.cluster as usize;
                cluster >= start && cluster < end
            })
            .map(|g| ShapedGlyph {
                cluster: g.cluster + self.range.start as u32,
                ..*g
            })
            .collect()
    }
}

/// Width in font units of the text in `[start, end)`.
fn spanned_width(shaped: &[ShapedSpan], start: usize, end: usize) -> u32 {
    shaped
        .iter()
        .filter(|span| span.range.start < end && span.range.end > start)
        .map(|span| span.width_in(start, end))
        .sum()
}

/// Number of trailing ASCII spaces in `[start, end)`.
fn trailing_spaces(text: &str, start: usize, end: usize) -> usize {
    let bytes = &text.as_bytes()[start..end];
    bytes.iter().rev().take_while(|b| **b == b' ').count()
}

fn skip_spaces(text: &str, mut at: usize) -> usize {
    while at < text.len() && text.as_bytes()[at] == b' ' {
        at += 1;
    }
    at
}

/// Slices shaped spans into the runs of one line. Trailing spaces are
/// dropped from the runs — ragged right never paints them.
fn cut_line(
    flat: &FlatParagraph,
    shaped: &[ShapedSpan],
    start: usize,
    end: usize,
    line_box: LineBox,
) -> Line {
    let mut runs = Vec::new();
    for (span, (font_id, size, _)) in shaped.iter().zip(flat.spans.iter()) {
        if span.range.start >= end || span.range.end <= start {
            continue;
        }
        // Trim trailing spaces from the line's last run: re-slice
        // against the last non-space byte.
        let content_end = {
            let mut e = end;
            while e > start && flat.text.as_bytes()[e - 1] == b' ' {
                e -= 1;
            }
            e
        };
        let glyphs = span.glyphs_in(start, content_end);
        if glyphs.is_empty() {
            continue;
        }
        let advance = glyphs.iter().map(|g| g.x_advance).sum();
        let text_start = span.range.start.max(start);
        let text_end = span.range.end.min(content_end).max(text_start);
        runs.push(ShapedRun {
            font_id: *font_id,
            size: *size,
            text: flat.text[text_start..text_end].to_string(),
            text_start: text_start as u32,
            glyphs,
            advance,
        });
    }
    Line {
        width: runs.iter().map(|r| r.advance).sum(),
        runs,
        box_: line_box,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::NodeId;

    fn registry() -> &'static FontRegistry {
        static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
        REGISTRY.get_or_init(|| crate::fonts::bundled_registry().expect("bundled font parses"))
    }

    fn layout_body(text: &str, measure_pt: f32) -> Vec<Line> {
        layout_body_opts(text, measure_pt, LineBreakOptions::default())
    }

    fn layout_body_opts(text: &str, measure_pt: f32, options: LineBreakOptions) -> Vec<Line> {
        let layout = LineLayout::new(registry());
        let inlines = vec![Inline::Text {
            id: NodeId::UNASSIGNED,
            value: text.to_string(),
            position: None,
        }];
        layout.layout(&inlines, ParagraphStyle::BODY, measure_pt, options)
    }

    fn units_per_em() -> u16 {
        registry().metrics(0).unwrap().units_per_em
    }

    /// Reconstructs a line's text from its clusters — glyphs map back
    /// to the paragraph text, so a line's content is checkable.
    fn line_text<'t>(line: &Line, text: &'t str) -> &'t str {
        let first = line
            .runs
            .iter()
            .flat_map(|r| r.glyphs.iter())
            .map(|g| g.cluster as usize)
            .min()
            .unwrap_or(0);
        let last = line
            .runs
            .iter()
            .flat_map(|r| r.glyphs.iter())
            .map(|g| g.cluster as usize)
            .max()
            .unwrap_or(0);
        // Last glyph's cluster starts a grapheme; advance past it.
        let mut end = last + 1;
        while end < text.len() && !text.is_char_boundary(end) {
            end += 1;
        }
        &text[first..end]
    }

    /// A run's glyphs map back to the characters they were shaped
    /// from: the ffi ligature is one glyph spanning three bytes, and
    /// the ranges tile the run's text without gaps.
    #[test]
    fn glyph_ranges_cover_the_run_text() {
        let lines = layout_body("difficult", 200.0);
        let run = &lines[0].runs[0];
        assert_eq!(run.text, "difficult");
        let ranges = run.glyph_ranges();
        assert_eq!(
            ranges.first().cloned(),
            Some(0..1),
            "the first glyph stands for the first byte"
        );
        assert!(
            ranges.iter().any(|r| r.end - r.start == 3),
            "no glyph spans the three characters of the ffi ligature: {ranges:?}"
        );
        assert_eq!(
            ranges.last().map(|r| r.end),
            Some(run.text.len() as u32),
            "the last glyph runs to the end of the run's text"
        );
        for pair in ranges.windows(2) {
            assert!(
                pair[1].start == pair[0].end || pair[1].start == pair[0].start,
                "ranges neither tile nor share a cluster: {pair:?}"
            );
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
        assert_eq!(line_text(&lines[0], "hello"), "hello");
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
            .map(|l| line_text(l, text).trim_end())
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(reconstructed, text);
    }

    /// Greedy first-fit: line 1 carries as much text as fits.
    #[test]
    fn greedy_fits_the_maximum_on_line_one() {
        let text = "aa bb cc";
        let lines = layout_body(text, 100.0);
        assert_eq!(lines.len(), 1, "everything fits: {lines:?}");
        let lines = layout_body(text, 30.0);
        assert_eq!(line_text(&lines[0], text), "aa bb");
    }

    /// Em-dash: UAX #14 allows the break after B2-class characters,
    /// so `word—word` has an opportunity mid-string.
    #[test]
    fn em_dash_provides_a_break_opportunity() {
        let text = "word—word word—word";
        let lines = layout_body(text, 34.0);
        assert!(lines.len() >= 2);
        let first = line_text(&lines[0], text);
        assert!(
            first.ends_with("word—") || first.ends_with("—"),
            "line 1 should end at the em-dash: {first:?}"
        );
    }

    /// Dialogue punctuation: the closing quote may not begin a line
    /// (UAX #14 LB19: QU ×); the break lands after it.
    #[test]
    fn dialogue_punctuation_stays_with_its_word() {
        // A measure that fits `said."` but not `said." then`: if UAX
        // #14 were wrong here, `."` would start line 2.
        let text = "\"he said.\" then more words follow here";
        let lines = layout_body(text, 58.0);
        assert!(lines.len() >= 2);
        // An opening quote legitimately begins line 1; what UAX #14
        // forbids is a continuation line starting with the closing
        // punctuation stranded from its word.
        for line in lines.iter().skip(1) {
            let t = line_text(line, text);
            assert!(
                !t.starts_with('.') && !t.starts_with(','),
                "punctuation started a line: {t:?}"
            );
        }
        let first = line_text(&lines[0], text);
        assert!(first.ends_with("said.\""), "line 1: {first:?}");
    }

    /// Trailing spaces at a break don't count toward width and aren't
    /// painted.
    #[test]
    fn trailing_spaces_are_free() {
        let with = layout_body("hello   ", 200.0);
        let without = layout_body("hello", 200.0);
        assert_eq!(with.len(), 1);
        assert_eq!(with[0].width, without[0].width);
    }

    /// Hyphenation off by default: a word longer than the measure
    /// overflows onto its own line rather than splitting.
    #[test]
    fn long_word_overflows_unhyphenated() {
        let lines = layout_body("tick extraordinary", 40.0);
        assert!(lines.len() >= 2);
        assert_eq!(
            line_text(&lines[lines.len() - 1], "tick extraordinary"),
            "extraordinary"
        );
    }

    /// Hyphenation on: a long word splits at syllable boundaries and
    /// no line exceeds the measure.
    #[test]
    fn hyphenation_splits_long_words() {
        let text = "extraordinarily";
        let lines = layout_body_opts(text, 44.0, LineBreakOptions { hyphenate: true });
        assert!(lines.len() >= 2, "expected a split, got {lines:?}");
        for line in &lines {
            let width_pt = line.width as f32 / units_per_em() as f32 * ParagraphStyle::BODY.size;
            assert!(
                width_pt <= 44.0,
                "line {line:?} exceeds the measure at {width_pt}pt"
            );
        }
    }

    /// The hyphen glyph is charged to the line. At a 53pt measure,
    /// `extraordinar-` (51.6pt text + 3.0pt hyphen) overflows, so
    /// greedy must stop at `extraordi-` (37.8 + 3.0); without the
    /// charge, `extraordinar-` would be chosen.
    #[test]
    fn the_hyphen_is_charged_to_the_line() {
        let text = "extraordinarily";
        // 53.0 fits extraordi+hyphen (40.77) but not extraordinar+
        // hyphen (54.64).
        let lines = layout_body_opts(text, 53.0, LineBreakOptions { hyphenate: true });
        assert!(lines.len() >= 2, "expected a split, got {lines:?}");
        let first = line_text(&lines[0], text);
        assert!(
            first.ends_with("extraordi"),
            "greedy undercharged the hyphen: line 1 is {first:?}"
        );
    }

    /// Two style runs (body + code): both land on the line in order.
    #[test]
    fn style_runs_stay_in_order() {
        let layout = LineLayout::new(registry());
        let inlines = vec![
            Inline::Text {
                id: NodeId::UNASSIGNED,
                value: "body ".into(),
                position: None,
            },
            Inline::Code {
                id: NodeId::UNASSIGNED,
                value: "code".into(),
                position: None,
            },
        ];
        let lines = layout.layout(&inlines, ParagraphStyle::BODY, 200.0, Default::default());
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].runs.len(), 2);
        assert_eq!(line_text(&lines[0], "body code"), "body code");
    }

    /// A paragraph of only spaces produces no lines.
    #[test]
    fn spaces_only_paragraph_is_empty() {
        assert!(layout_body("   ", 200.0).is_empty());
    }

    /// Acceptance: fixture-paragraph breaks match a hand-computed
    /// reference. The opening of Gulliver §2, widths derived
    /// independently of this module (per-word sums of hb-shape
    /// advances for EB Garamond), greedy-packed by hand at five
    /// measures.
    #[test]
    fn breaks_match_hand_computed_reference() {
        let text = "My father had a small estate in Nottinghamshire:";
        // Per-word widths (pt): My 14.29, father 24.70, had 15.62,
        // a 4.39, small 21.78, estate 23.43, in 8.50,
        // Nottinghamshire: 75.57; space 2.2.
        let expected: &[(f32, &[&str])] = &[
            (
                50.0,
                &["My father", "had a small", "estate in", "Nottinghamshire:"],
            ),
            (
                60.0,
                &["My father had", "a small estate", "in", "Nottinghamshire:"],
            ),
            (
                80.0,
                &["My father had a", "small estate in", "Nottinghamshire:"],
            ),
            (
                120.0,
                &["My father had a small estate", "in Nottinghamshire:"],
            ),
            (250.0, &["My father had a small estate in Nottinghamshire:"]),
        ];
        for (measure, want_lines) in expected {
            let lines = layout_body(text, *measure);
            let got: Vec<&str> = lines.iter().map(|l| line_text(l, text)).collect();
            assert_eq!(&got, want_lines, "measure {measure}: {lines:?}");
        }
    }
}
