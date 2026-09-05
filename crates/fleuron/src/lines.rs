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

use std::ops::Range;

use crate::content::{Inline, NodeId};
use crate::fonts::{Features, FontRegistry, ShapedGlyph};
use crate::linebox::{LineBox, Strut};
use crate::style::{Color, FontVariantCaps, TextTransform};
use icu_segmenter::{WordSegmenter, options::WordBreakInvariantOptions};
use serde::Serialize;
use unicode_linebreak::{BreakOpportunity, linebreaks};

/// What a word space may give and take, as a fraction of its own
/// width: TeX's interword glue, half of stretch and a third of
/// shrink.
const SPACE_STRETCH: f32 = 0.5;
const SPACE_SHRINK: f32 = 1.0 / 3.0;

/// The same for the space between letters, which only
/// `text-justify: inter-character` opens up. Small on purpose: the
/// eye reads a word by its shape, and a word spaced wider than this
/// stops being one.
const LETTER_STRETCH: f32 = 0.02;
const LETTER_SHRINK: f32 = 0.01;

/// What a synthesized small capital is set at, as a fraction of the
/// size around it. A face's own small caps sit a little above the
/// x-height, and four-fifths of the cap height is about where that
/// lands.
const SMALL_CAPS_RATIO: f32 = 0.8;

/// What a ragged line may leave at the right, as a fraction of the
/// measure, before it counts as loose. Ragged setting has no glue to
/// stretch, so badness has nothing else to read the gap against.
const RAGGED_STRETCH: f32 = 0.1;

/// Knuth's demerit weights. A line costs `(line + badness)^2`, so a
/// paragraph of evenly loose lines beats one tight line and one
/// gaping one; the rest are the surcharges for breaking a word, for
/// doing it twice running, and for setting a tight line under a
/// loose one.
const LINE_PENALTY: f64 = 10.0;
const HYPHEN_PENALTY: f64 = 50.0;
const DOUBLE_HYPHEN_DEMERITS: f64 = 10_000.0;
const ADJACENT_DEMERITS: f64 = 10_000.0;

/// What breaking before a dash costs. UAX #14 allows a line to end
/// on either side of one; a book only ever ends on the far side, so
/// the near side has to be worth something to be taken.
const DASH_PENALTY: f64 = 200.0;

/// The worst a line may be counted as. Without a ceiling a single
/// unbreakable line swamps every other term in the paragraph.
const MAX_BADNESS: f64 = 10_000.0;

/// What a line that overflows the measure costs, per em it overflows
/// by and once besides. Larger than any feasible paragraph, so text
/// runs into the margin only where nothing else will set, and the
/// least of it wins.
const OVERFULL_DEMERITS: f64 = 1e12;

/// Hyphenated line ends allowed in a row.
const MAX_CONSECUTIVE_HYPHENS: u8 = 2;

/// Everything one paragraph's layout depends on, and the colour its
/// runs are painted in. The style tree compiles down to this.
#[derive(Debug, Clone, Copy)]
pub struct ParagraphStyle {
    /// Face id from the font registry.
    pub font_id: u16,
    /// Font size in points.
    pub size: f32,
    /// Line height as a unitless multiple of `size`, as in CSS
    /// `line-height: <number>`.
    pub line_height: f32,
    /// Extra advance between glyphs, in points, from
    /// `letter-spacing`. A line ends at its last glyph's own edge, so
    /// nothing is added after it.
    pub letter_spacing: f32,
    /// Which capitals the text is drawn with.
    pub caps: FontVariantCaps,
    /// What the text is transformed to before it is shaped.
    pub transform: TextTransform,
    /// What the run is painted in. Nothing measures it: a run
    /// carries it from here to the display structure.
    pub color: Color,
}

/// Where line layout gets the style of one inline node.
///
/// The style tree answers by node id. `Inherited` answers with the
/// block's own style, which is what a run of uniform text needs and
/// what a caller with no tree in hand can supply.
pub trait InlineStyles {
    /// The style of `id`, given the style of the block it sits in.
    fn style(&self, id: NodeId, block: ParagraphStyle) -> ParagraphStyle;
}

/// Every inline takes the style of the block around it.
pub struct Inherited;

impl InlineStyles for Inherited {
    fn style(&self, _id: NodeId, block: ParagraphStyle) -> ParagraphStyle {
        block
    }
}

/// The width lines break to.
///
/// Uniform for a paragraph on its own; a drop cap shortens the lines
/// it sits beside and leaves the rest of the paragraph at the full
/// measure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measure {
    /// Points the lines past the shortened ones break to.
    pub full: f32,
    /// Points the first `shortened` lines break to.
    pub narrow: f32,
    /// How many lines break to `narrow`.
    pub shortened: usize,
}

impl Measure {
    /// One width for every line.
    pub fn uniform(points: f32) -> Measure {
        Measure {
            full: points,
            narrow: points,
            shortened: 0,
        }
    }

    /// The width line `index` breaks to.
    pub fn at(self, index: usize) -> f32 {
        if index < self.shortened {
            self.narrow
        } else {
            self.full
        }
    }
}

impl From<f32> for Measure {
    fn from(points: f32) -> Measure {
        Measure::uniform(points)
    }
}

/// Which marks may hang past the measure, from
/// `hanging-punctuation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
pub struct HangingPunctuation {
    /// `first`: opening punctuation hangs into the margin the line
    /// starts at.
    pub first: bool,
    /// `allow-end` or `force-end`.
    pub end: HangEnd,
    /// `last`: the mark a paragraph ends on hangs past the measure.
    pub last: bool,
}

impl HangingPunctuation {
    /// Nothing hangs.
    pub const NONE: HangingPunctuation = HangingPunctuation {
        first: false,
        end: HangEnd::None,
        last: false,
    };
}

/// How far `hanging-punctuation` goes at the end of a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HangEnd {
    /// Nothing hangs at a line end.
    #[default]
    None,
    /// `allow-end`: a mark hangs only where hanging is what makes the
    /// line fit.
    Allow,
    /// `force-end`: a mark at a line end always hangs.
    Force,
}

/// How a paragraph is broken and filled. The defaults are ragged
/// right, no hyphenation, and no mark hanging past the measure.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LineBreakOptions {
    /// Whether `hyphens: auto` is in force.
    pub hyphenate: bool,
    /// Whether the lines fill the measure, from
    /// `text-align: justify`. Left, right and centred text all break
    /// the same way; where the line then sits is the caller's.
    pub justify: bool,
    /// Whether justification also opens the space between letters,
    /// from `text-justify: inter-character`.
    pub inter_character: bool,
    /// Which marks hang past the measure.
    pub hanging: HangingPunctuation,
}

/// A run of glyphs sharing one font and size — the paintable unit.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapedRun {
    /// Index into the registry that shaped the run.
    pub font_id: u16,
    /// Em size in points.
    pub size: f32,
    /// The text the run was shaped from. Glyph ids alone do not
    /// spell anything, and the correspondence exists only in the
    /// shaper's output.
    pub text: String,
    /// What the author wrote, where a transform made that differ
    /// from what was shaped, and empty where the two are the same.
    pub source: String,
    /// The offset in `source` of every byte boundary of `text`.
    /// Empty alongside `source`.
    pub source_map: Vec<u32>,
    /// Byte offset of `text` in the paragraph the glyphs' clusters
    /// index.
    pub text_start: u32,
    /// The features the run was shaped with.
    pub features: Features,
    /// What the run is painted in.
    pub color: Color,
    /// The glyphs, in visual order.
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
    /// The line's runs, in visual order.
    pub runs: Vec<ShapedRun>,
    /// Advance of the line's glyphs, trailing spaces excluded; a
    /// hyphenated line's hyphen is charged here even though the glyph
    /// joins the runs when the structured outpur paints it. What hangs
    /// past the measure is charged here too, and taken off again by
    /// `overhang` and `protrusion`.
    pub width: u32,
    /// Points the line's last glyph hangs past the measure.
    pub overhang: f32,
    /// Points the line's first glyph hangs before the line's origin.
    pub protrusion: f32,
    /// The line's vertical geometry — computed here, in points;
    /// downstream stages position against it, never re-measure.
    pub box_: LineBox,
}

/// Flattened paragraph content: the text as it is shaped, the text
/// the author wrote under it, and, per style span, the byte range it
/// covers. Style boundaries are segmentation boundaries — a shaped
/// run never spans two fonts.
#[derive(Default)]
struct FlatParagraph {
    /// What is shaped, measured and broken. `text-transform` and
    /// synthesized small capitals have already been applied.
    text: String,
    /// What the author wrote. Kept only from the point something
    /// first transforms; until then `text` is the source.
    source: String,
    /// How much of `source` the shaped text up to each of its bytes
    /// accounts for. Kept alongside `source`, and rising with it, so
    /// the stretch between two boundaries is the source that the
    /// text between them was made from.
    map: Vec<u32>,
    /// Whether anything has been written that differs from its
    /// source.
    transformed: bool,
    /// Whether the next character opens a word: what `capitalize`
    /// reads.
    word_start: bool,
    /// The style spans, in document order.
    spans: Vec<Span>,
}

/// One span of uniform shaping: a face, a size, the tracking after
/// each of its clusters, and the features it is shaped with.
struct Span {
    font_id: u16,
    size: f32,
    /// Extra advance after each cluster, in points.
    tracking: f32,
    features: Features,
    color: Color,
    /// Byte range in the paragraph's shaped text.
    range: Range<usize>,
}

/// How a span gets its small capitals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SmallCaps {
    /// It does not: the text is set in the letters it was written in.
    Off,
    /// The face draws them, and the shaper is asked for `smcp`.
    Feature,
    /// The face does not, so lowercase letters are set as capitals at
    /// a reduced size.
    Synthesized,
}

impl FlatParagraph {
    /// An empty paragraph, at a word boundary because nothing has
    /// been written yet.
    fn new() -> FlatParagraph {
        FlatParagraph {
            word_start: true,
            ..FlatParagraph::default()
        }
    }

    /// What the author wrote under one stretch of the shaped text,
    /// and how much of it the text up to each byte accounts for.
    ///
    /// Both are empty where the two stretches are the same word for
    /// word, which is every run of a book nothing transformed and
    /// the untouched runs of a line that has one.
    fn source_of(&self, range: Range<usize>) -> (String, Vec<u32>) {
        if !self.transformed {
            return (String::new(), Vec::new());
        }
        let at = |byte: usize| match self.map.get(byte) {
            Some(offset) => *offset as usize,
            None => self.source.len(),
        };
        let (from, to) = (at(range.start), at(range.end).max(at(range.start)));
        let source = &self.source[from..to];
        if source == &self.text[range.clone()] {
            return (String::new(), Vec::new());
        }
        let map = (range.start..=range.end)
            .map(|byte| (at(byte).clamp(from, to) - from) as u32)
            .collect();
        (source.to_string(), map)
    }

    /// Starts keeping the source text, backfilling what has been
    /// written so far, all of which stood as it was written.
    fn start_mapping(&mut self) {
        if self.transformed {
            return;
        }
        self.transformed = true;
        self.source.push_str(&self.text);
        self.map.extend(0..self.text.len() as u32);
    }

    /// Appends a stretch of text nothing transformed.
    fn push_verbatim(&mut self, value: &str) {
        if self.transformed {
            let at = self.source.len() as u32;
            self.map
                .extend((0..value.len() as u32).map(|byte| at + byte));
            self.source.push_str(value);
        }
        self.text.push_str(value);
    }

    /// Appends one character shaped as something other than itself.
    ///
    /// The whole character is accounted for at the first byte it was
    /// written as, so the first glyph of the pair `ß` shapes as
    /// stands for the letter and the second stands for nothing.
    /// Extraction walks the glyphs in order and reads the source back
    /// once.
    fn push_mapped(&mut self, letter: char, written: &str) {
        self.start_mapping();
        let at = self.source.len() as u32;
        self.source.push(letter);
        let after = self.source.len() as u32;
        self.map.push(at);
        self.map
            .extend(std::iter::repeat_n(after, written.len() - 1));
        self.text.push_str(written);
    }

    /// Appends one styled stretch of text: `text-transform` first,
    /// then small capitals over what it produced, and the spans they
    /// come to.
    fn push_styled(&mut self, value: &str, style: ParagraphStyle, caps: SmallCaps) {
        if style.transform == TextTransform::None && caps != SmallCaps::Synthesized {
            let start = self.text.len();
            self.push_verbatim(value);
            if let Some(last) = value.chars().next_back() {
                self.word_start = !continues_word(last);
            }
            self.span(style, caps, false, start);
            return;
        }
        // A span breaks where the reduced size does, so the letters a
        // synthesis raised are shaped apart from the ones it left.
        let mut open: Option<(bool, usize)> = None;
        let mut written = String::new();
        for letter in value.chars() {
            written.clear();
            let mut changed = style.transform.write(letter, self.word_start, &mut written);
            self.word_start = !continues_word(letter);
            let small = raise(caps, &mut written, &mut changed);
            if open.map(|(was, _)| was) != Some(small) {
                if let Some((was, at)) = open {
                    self.span(style, caps, was, at);
                }
                open = Some((small, self.text.len()));
            }
            if changed {
                self.push_mapped(letter, &written);
            } else {
                self.push_verbatim(&written);
            }
        }
        if let Some((was, at)) = open {
            self.span(style, caps, was, at);
        }
    }

    /// Records the span that ends where the text now does.
    fn span(&mut self, style: ParagraphStyle, caps: SmallCaps, small: bool, start: usize) {
        if start >= self.text.len() {
            return;
        }
        self.spans.push(Span {
            font_id: style.font_id,
            size: if small {
                style.size * SMALL_CAPS_RATIO
            } else {
                style.size
            },
            tracking: style.letter_spacing,
            features: Features {
                small_caps: caps == SmallCaps::Feature,
            },
            color: style.color,
            range: start..self.text.len(),
        });
    }
}

/// Raises one character to a capital where a synthesis has to draw a
/// small one, and answers whether it is set at the reduced size. Only
/// what was lowercase is: a face's own small capitals leave the word
/// space and the comma the size they were.
fn raise(caps: SmallCaps, written: &mut String, changed: &mut bool) -> bool {
    if caps != SmallCaps::Synthesized || !written.chars().any(char::is_lowercase) {
        return false;
    }
    *written = written.to_uppercase();
    *changed = true;
    true
}

/// Whether a character continues a word rather than ending it.
/// `capitalize` raises the letter after every other kind, which is
/// why `well-known` comes out with two capitals and `don't` with one.
fn continues_word(letter: char) -> bool {
    letter.is_alphanumeric() || letter == '\'' || letter == '\u{2019}'
}

/// A candidate line end: the exclusive byte offset where a line may
/// end, plus whether the break falls inside a word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Opportunity {
    /// Exclusive end of the line's text.
    end: usize,
    /// True when this break sits inside a word and the line must be
    /// charged for a hyphen glyph.
    hyphen: bool,
}

/// One place a line may end, with everything the breaker measures it
/// by. Widths are read out of the prefix tables at these offsets.
#[derive(Debug, Clone, Copy)]
struct Break {
    /// Where the line's paintable text ends: `end` less the spaces
    /// the break swallows.
    content_end: usize,
    /// Where the line after this one starts.
    next: usize,
    /// Whether taking this break puts a hyphen on the line.
    hyphen: bool,
    /// Whether the line after this break would open with a dash.
    dash: bool,
    /// Font units of the last glyph that may hang past the measure.
    hang_end: f32,
    /// Font units the first glyph of the following line may hang
    /// before the measure.
    hang_start: f32,
}

/// A chosen line end, with the adjustment its glue takes.
#[derive(Debug, Clone, Copy)]
struct Fitted {
    /// Index into the break list.
    at: usize,
    /// The line's adjustment ratio: what fraction of its stretch or
    /// shrink the glue gives up to reach the measure.
    ratio: f32,
    /// Font units hanging past the measure at the line's end.
    overhang: f32,
    /// Font units hanging before the line's origin.
    protrusion: f32,
}

/// Prefix sums over the paragraph's shaped glyphs, in the
/// paragraph's own font units: the width of any byte range is one
/// subtraction.
struct Widths {
    /// Advance of every glyph whose cluster starts before byte `i`.
    text: Vec<f32>,
    /// The same for space glyphs alone, which is where the glue is.
    spaces: Vec<f32>,
    /// Whether a glyph's cluster starts at byte `i`. A break inside
    /// a cluster would cut a ligature in half and lose it.
    starts: Vec<bool>,
    /// Tracking charged to the last cluster starting before byte `i`.
    /// Empty where nothing is tracked, which is most paragraphs.
    trailing: Vec<f32>,
}

impl Widths {
    fn build(text: &str, shaped: &[ShapedSpan]) -> Widths {
        let tracked = shaped.iter().any(|span| span.tracking != 0.0);
        let mut widths = Widths {
            text: vec![0.0; text.len() + 1],
            spaces: vec![0.0; text.len() + 1],
            starts: vec![false; text.len() + 1],
            trailing: if tracked {
                vec![0.0; text.len() + 1]
            } else {
                Vec::new()
            },
        };
        let bytes = text.as_bytes();
        for span in shaped {
            for glyph in &span.glyphs {
                let at = (span.range.start + glyph.cluster as usize).min(text.len());
                let advance = glyph.x_advance as f32 * span.scale;
                widths.text[at] += advance;
                if bytes.get(at) == Some(&b' ') {
                    widths.spaces[at] += advance;
                }
                widths.starts[at] = true;
                if tracked {
                    widths.trailing[at] = span.tracking;
                }
            }
        }
        // Exclusive prefixes: entry `i` totals the glyphs that
        // start before byte `i`, which is exactly the glyphs on a line
        // ending there.
        let (mut text_total, mut space_total, mut track) = (0.0, 0.0, 0.0);
        for at in 0..widths.text.len() {
            let (here, space) = (widths.text[at], widths.spaces[at]);
            widths.text[at] = text_total;
            widths.spaces[at] = space_total;
            text_total += here;
            space_total += space;
            if tracked {
                let charged = widths.trailing[at];
                widths.trailing[at] = track;
                if widths.starts[at] {
                    track = charged;
                }
            }
        }
        widths
    }

    /// The advance of the glyphs in `[from, to)`, less the tracking
    /// charged after the last of them: what runs between two letters
    /// does not run past the last one.
    fn advance(&self, from: usize, to: usize) -> f32 {
        if to <= from {
            return 0.0;
        }
        self.text[to] - self.text[from] - self.trailing.get(to).copied().unwrap_or(0.0)
    }
}

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
        self.layout_styled(inlines, style, &Inherited, measure, options)
    }

    /// The same, with the style tree answering for each inline.
    pub fn layout_styled(
        &self,
        inlines: &[Inline],
        style: ParagraphStyle,
        styles: &dyn InlineStyles,
        measure: impl Into<Measure>,
        options: LineBreakOptions,
    ) -> Vec<Line> {
        let measure = measure.into();
        let flat = self.flatten(inlines, style, styles);
        if flat.text.is_empty() {
            return Vec::new();
        }
        let Some(metrics) = self.registry.metrics(style.font_id) else {
            return Vec::new();
        };
        let upem = metrics.units_per_em as f32;
        // Points → font units: measure / size gives ems, ems *
        // units_per_em gives font units.
        let to_points = |units: f32| units / upem * style.size;

        let shaped = self.shape_spans(&flat, style, upem);
        let widths = Widths::build(&flat.text, &shaped);
        let hyphen = self.hyphen_advance(style) as f32;
        let breaks = self.break_points(&flat.text, &widths, hyphen, options);
        let breaker = Breaker {
            breaks: &breaks,
            widths: &widths,
            measure,
            upem,
            size: style.size,
            hyphen,
            options,
        };

        let mut lines = Vec::new();
        let mut start = 0usize;
        for fit in breaker.run() {
            let at = &breaks[fit.at];
            if at.content_end > start {
                let mut line = self.cut(&flat, &shaped, start, at.content_end, style);
                self.adjust(&mut line, &flat.text, fit.ratio, options);
                if at.hyphen {
                    self.hyphenate(&mut line, style);
                }
                line.overhang = to_points(fit.overhang);
                line.protrusion = to_points(fit.protrusion);
                lines.push(line);
            }
            start = at.next;
        }
        lines
    }

    /// One string as shaped runs, set the way `style` asks for it.
    /// The counterpart of `layout` for text that is not broken into
    /// lines: page furniture, an ornament, an initial letter.
    pub fn shape(&self, text: &str, style: ParagraphStyle) -> Option<Vec<ShapedRun>> {
        let upem = self.registry.metrics(style.font_id)?.units_per_em as f32;
        let mut flat = FlatParagraph::new();
        flat.push_styled(text, style, self.small_caps(style));
        let shaped = self.shape_spans(&flat, style, upem);
        Some(cut_runs(&flat, &shaped, 0, flat.text.len()))
    }

    /// Flattens a paragraph's inlines, each one styled as the tree
    /// says. A style boundary is a span boundary, so a run never
    /// spans two faces.
    fn flatten(
        &self,
        inlines: &[Inline],
        style: ParagraphStyle,
        styles: &dyn InlineStyles,
    ) -> FlatParagraph {
        let mut flat = FlatParagraph::new();
        self.walk_inlines(inlines, style, styles, &mut flat);
        flat
    }

    fn walk_inlines(
        &self,
        inlines: &[Inline],
        style: ParagraphStyle,
        styles: &dyn InlineStyles,
        flat: &mut FlatParagraph,
    ) {
        for inline in inlines {
            match inline {
                Inline::Text { value, .. } => {
                    flat.push_styled(value, style, self.small_caps(style))
                }
                Inline::Code { id, value, .. } => {
                    let code = styles.style(*id, style);
                    flat.push_styled(value, code, self.small_caps(code));
                }
                Inline::Emphasis { id, children, .. }
                | Inline::Strong { id, children, .. }
                | Inline::Link { id, children, .. } => {
                    self.walk_inlines(children, styles.style(*id, style), styles, flat);
                }
            }
        }
    }

    /// Where a style's small capitals come from: the face's own where
    /// it has them, and a synthesis where it does not.
    fn small_caps(&self, style: ParagraphStyle) -> SmallCaps {
        match style.caps {
            FontVariantCaps::Normal => SmallCaps::Off,
            FontVariantCaps::SmallCaps if self.registry.has_small_caps(style.font_id) => {
                SmallCaps::Feature
            }
            FontVariantCaps::SmallCaps => SmallCaps::Synthesized,
        }
    }

    /// Shapes each span. A glyph's cluster is an offset into the
    /// span, which indexes the paragraph text once offset by the
    /// span's start. A span set at another size measures in its own
    /// font units, so `scale` takes it into the paragraph's.
    ///
    /// Tracking is added here, to the last glyph of every cluster, so
    /// every pass downstream measures it without being told about it.
    fn shape_spans(
        &self,
        flat: &FlatParagraph,
        style: ParagraphStyle,
        upem: f32,
    ) -> Vec<ShapedSpan> {
        flat.spans
            .iter()
            .map(|span| {
                let mut glyphs = self
                    .registry
                    .shape_with(span.font_id, &flat.text[span.range.clone()], span.features)
                    .unwrap_or_default();
                let track = self.tracking_units(span);
                if track != 0 {
                    for at in 0..glyphs.len() {
                        let last = glyphs
                            .get(at + 1)
                            .is_none_or(|next| next.cluster != glyphs[at].cluster);
                        if last {
                            glyphs[at].x_advance =
                                (glyphs[at].x_advance as i64 + track).max(0) as u32;
                        }
                    }
                }
                let scale = self.scale(span.font_id, span.size, style, upem);
                ShapedSpan {
                    range: span.range.clone(),
                    scale,
                    track,
                    tracking: track as f32 * scale,
                    glyphs,
                }
            })
            .collect()
    }

    /// A span's tracking in its own font units.
    fn tracking_units(&self, span: &Span) -> i64 {
        if span.tracking == 0.0 || span.size <= 0.0 {
            return 0;
        }
        let upem = self
            .registry
            .metrics(span.font_id)
            .map(|m| m.units_per_em as f32)
            .unwrap_or(1000.0);
        (span.tracking / span.size * upem).round() as i64
    }

    /// What a span's own font units are worth in the paragraph's.
    /// One em of a 6pt face is not one em of an 11pt one, and the
    /// measure is written in the paragraph's.
    fn scale(&self, font_id: u16, size: f32, style: ParagraphStyle, upem: f32) -> f32 {
        let span_upem = self
            .registry
            .metrics(font_id)
            .map(|m| m.units_per_em as f32)
            .unwrap_or(upem);
        if span_upem <= 0.0 || style.size <= 0.0 {
            return 1.0;
        }
        size / span_upem * upem / style.size
    }

    /// One line's runs plus the box they occupy: a run taller than
    /// the paragraph's strut grows the line around the baseline.
    fn cut(
        &self,
        flat: &FlatParagraph,
        shaped: &[ShapedSpan],
        start: usize,
        end: usize,
        style: ParagraphStyle,
    ) -> Line {
        let runs = cut_runs(flat, shaped, start, end);
        Line {
            width: runs.iter().map(|run| run.advance).sum(),
            overhang: 0.0,
            protrusion: 0.0,
            box_: self.line_box(&runs, style),
            runs,
        }
    }

    /// Break opportunities for the paragraph: UAX #14 always, UAX #29
    /// word boundaries to bound hyphenation, `hypher` for syllables
    /// when enabled.
    fn opportunities(
        &self,
        text: &str,
        widths: &Widths,
        options: LineBreakOptions,
    ) -> Vec<Opportunity> {
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
            self.add_hyphenation(text, widths, &mut opportunities);
        }
        // A hyphen and a space at the same offset are the same
        // break, and the one that costs nothing wins it.
        opportunities.sort_unstable_by_key(|o| (o.end, o.hyphen));
        opportunities.dedup_by_key(|o| o.end);
        opportunities
    }

    fn add_hyphenation(&self, text: &str, widths: &Widths, opportunities: &mut Vec<Opportunity>) {
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
                    // A syllable boundary inside a ligature is not a
                    // place a line can end: the glyph belongs to
                    // neither half on its own.
                    if offset > start && offset < boundary && widths.starts[offset] {
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

    /// The paragraph's break list: every opportunity, with the text
    /// it leaves behind and the marks that may hang at either end.
    fn break_points(
        &self,
        text: &str,
        widths: &Widths,
        hyphen: f32,
        options: LineBreakOptions,
    ) -> Vec<Break> {
        let hangs = options.hanging != HangingPunctuation::NONE;
        let start_hang = |at: usize| {
            if !hangs {
                return 0.0;
            }
            match text[at..].chars().next() {
                Some(first) if widths.starts[at] => {
                    hang_start(first) * widths.advance(at, at + first.len_utf8())
                }
                _ => 0.0,
            }
        };
        let mut breaks = vec![Break {
            content_end: 0,
            next: 0,
            hyphen: false,
            dash: false,
            hang_end: 0.0,
            hang_start: start_hang(0),
        }];
        for opportunity in self.opportunities(text, widths, options) {
            let content_end = opportunity.end - trailing_spaces(text, 0, opportunity.end);
            let next = skip_spaces(text, opportunity.end);
            let hang = if !hangs {
                0.0
            } else if opportunity.hyphen {
                hang_end('-') * hyphen
            } else {
                match text[..content_end].chars().next_back() {
                    Some(last) if widths.starts[content_end - last.len_utf8()] => {
                        hang_end(last) * widths.advance(content_end - last.len_utf8(), content_end)
                    }
                    _ => 0.0,
                }
            };
            breaks.push(Break {
                content_end,
                next,
                hyphen: opportunity.hyphen,
                dash: matches!(
                    text[next..].chars().next(),
                    Some('-' | '\u{2010}' | '\u{2013}' | '\u{2014}')
                ),
                hang_end: hang,
                hang_start: start_hang(next),
            });
        }
        breaks
    }

    /// Spreads a line's adjustment over the glue it was measured
    /// with. The ratio was chosen against the shaped advances, so it
    /// lands on them: a painter is handed positions, not a rule for
    /// working them out.
    ///
    /// The residue is carried from glyph to glyph rather than
    /// dropped, so a line of rounded advances still totals the
    /// measure.
    fn adjust(&self, line: &mut Line, text: &str, ratio: f32, options: LineBreakOptions) {
        if !options.justify || !ratio.is_finite() || ratio == 0.0 {
            return;
        }
        let ratio = ratio.max(-1.0);
        let (space, letter) = if ratio > 0.0 {
            (SPACE_STRETCH, LETTER_STRETCH)
        } else {
            (SPACE_SHRINK, LETTER_SHRINK)
        };
        let letter = if options.inter_character { letter } else { 0.0 };
        let bytes = text.as_bytes();
        let (mut wanted, mut applied) = (0.0f32, 0i64);
        for run in &mut line.runs {
            let mut advance = 0i64;
            for glyph in &mut run.glyphs {
                let is_space = bytes.get(glyph.cluster as usize) == Some(&b' ');
                let share = if is_space { space } else { letter };
                wanted += ratio * share * glyph.x_advance as f32;
                let step = wanted.round() as i64 - applied;
                applied += step;
                let width = (glyph.x_advance as i64 + step).max(0);
                advance += width - glyph.x_advance as i64;
                glyph.x_advance = width as u32;
            }
            run.advance = (run.advance as i64 + advance).max(0) as u32;
        }
        line.width = line.runs.iter().map(|run| run.advance).sum();
    }

    /// Draws the hyphen a break inside a word leaves behind.
    ///
    /// The break was charged for it when it was chosen, so it is
    /// drawn in the same face the charge was read from: a hyphen
    /// taken from one face and paid for out of another is a line
    /// that measures one width and paints a different one.
    fn hyphenate(&self, line: &mut Line, style: ParagraphStyle) {
        let Some(id) = self.registry.char_glyph(style.font_id, '-') else {
            return;
        };
        let advance = self
            .registry
            .advance_width(style.font_id, id)
            .unwrap_or_default() as u32;
        let ending = line
            .runs
            .last()
            .map(|run| run.text_start + run.text.len() as u32)
            .unwrap_or_default();
        // The hyphen takes the last run's colour, because colour
        // costs no width, and the paragraph's face, because that is
        // where its width was charged.
        let color = line.runs.last().map_or(style.color, |run| run.color);
        match line.runs.last_mut() {
            Some(run) if run.font_id == style.font_id && run.size == style.size => {
                run.text.push('-');
                // A hyphen the breaker drew stands for nothing the
                // author wrote, so it maps to an empty stretch of the
                // source and extraction reads straight past it.
                if let Some(end) = run.source_map.last().copied() {
                    run.source_map.push(end);
                }
                run.glyphs.push(ShapedGlyph {
                    id,
                    x_advance: advance,
                    cluster: ending,
                });
                run.advance += advance;
            }
            // A break inside an emphasised word: the hyphen is the
            // paragraph's own, so it goes in a run of its own rather
            // than into a face it was not measured in.
            _ => line.runs.push(ShapedRun {
                font_id: style.font_id,
                size: style.size,
                text: "-".to_string(),
                source: String::new(),
                source_map: Vec::new(),
                text_start: ending,
                features: Features::NONE,
                color,
                glyphs: vec![ShapedGlyph {
                    id,
                    x_advance: advance,
                    cluster: ending,
                }],
                advance,
            }),
        }
        line.width += advance;
    }

    fn hyphen_advance(&self, style: ParagraphStyle) -> u32 {
        self.registry
            .char_glyph(style.font_id, '-')
            .and_then(|g| self.registry.advance_width(style.font_id, g))
            .unwrap_or(0) as u32
    }
}

/// How much of a mark hangs past the measure it ends a line at, as a
/// fraction of its advance. The lighter the mark, the further it
/// goes: a full stop leaves a hole in the margin that the eye reads
/// as a ragged edge, a colon does not.
fn hang_end(mark: char) -> f32 {
    match mark {
        '.' | ',' => 0.7,
        '-' | '\u{2010}' | '\u{2013}' => 0.5,
        '"' | '\'' | '\u{201d}' | '\u{2019}' | '\u{00bb}' => 0.4,
        '\u{2014}' => 0.25,
        ';' | ':' | '!' | '?' => 0.2,
        _ => 0.0,
    }
}

/// The same for the mark a line opens with, hanging back into the
/// margin the line starts at.
fn hang_start(mark: char) -> f32 {
    match mark {
        '"' | '\'' | '\u{201c}' | '\u{2018}' | '\u{00ab}' => 0.4,
        '(' | '[' | '\u{2013}' | '\u{2014}' => 0.25,
        _ => 0.0,
    }
}

/// One paragraph's total-fit pass: break list in, chosen line ends
/// out.
struct Breaker<'a> {
    breaks: &'a [Break],
    widths: &'a Widths,
    measure: Measure,
    upem: f32,
    size: f32,
    /// Font units a hyphenated break is charged for.
    hyphen: f32,
    options: LineBreakOptions,
}

/// How one candidate line comes out: how far its glue is from its
/// natural width, and what that costs.
struct Fit {
    ratio: f32,
    badness: f64,
    /// Font units the line runs past the measure by, if it does.
    overflow: f32,
    overhang: f32,
    protrusion: f32,
}

/// A breakpoint reached by a path, and the best path to it.
struct Node {
    /// Index into the break list.
    at: usize,
    /// Lines the paragraph has taken to get here.
    line: usize,
    /// Which of the four fitness classes the line ending here fell
    /// in.
    fitness: u8,
    /// Hyphenated line ends in a row up to and including this one.
    hyphens: u8,
    demerits: f64,
    ratio: f32,
    overhang: f32,
    protrusion: f32,
    /// The node this one was reached from; the root has none.
    previous: Option<usize>,
}

/// A line end worth keeping, before it becomes a node.
struct Candidate {
    at: usize,
    line: usize,
    fitness: u8,
    hyphens: u8,
    demerits: f64,
    ratio: f32,
    overhang: f32,
    protrusion: f32,
    previous: usize,
}

impl Breaker<'_> {
    /// Points → the paragraph's font units.
    fn units(&self, points: f32) -> f32 {
        if self.size > 0.0 {
            points / self.size * self.upem
        } else {
            0.0
        }
    }

    /// The last breakpoint, which every path has to reach.
    fn end(&self) -> usize {
        self.breaks.len() - 1
    }

    /// Measures the line that runs from break `a` to break `b`.
    fn fit(&self, a: usize, b: usize, line: usize) -> Fit {
        let start = self.breaks[a].next;
        let end = self.breaks[b].content_end.max(start);
        let measure = self.units(self.measure.at(line - 1));
        let text = self.widths.advance(start, end);
        let spaces = self.widths.spaces[end] - self.widths.spaces[start];
        let hyphen = if self.breaks[b].hyphen {
            self.hyphen
        } else {
            0.0
        };
        let protrusion = if self.options.hanging.first {
            self.breaks[a].hang_start
        } else {
            0.0
        };
        let natural = text + hyphen - protrusion;
        let overhang = self.overhang(b, natural, measure);
        let width = natural - overhang;

        let last = b == self.end();
        // The last line of a paragraph fills whatever it fills: the
        // glue that finishes it stretches without limit.
        let (stretch, shrink) = if last {
            let shrink = if self.options.justify {
                spaces * SPACE_SHRINK
            } else {
                0.0
            };
            (f32::INFINITY, shrink)
        } else if self.options.justify {
            let letters = if self.options.inter_character {
                text - spaces
            } else {
                0.0
            };
            (
                spaces * SPACE_STRETCH + letters * LETTER_STRETCH,
                spaces * SPACE_SHRINK + letters * LETTER_SHRINK,
            )
        } else {
            // Ragged setting has no glue to open, so the gap at the
            // right is read against a fraction of the measure.
            (measure * RAGGED_STRETCH, 0.0)
        };

        let gap = measure - width;
        let ratio = if gap > 0.0 {
            if stretch > 0.0 {
                gap / stretch
            } else {
                f32::INFINITY
            }
        } else if gap < 0.0 {
            if shrink > 0.0 {
                gap / shrink
            } else {
                f32::NEG_INFINITY
            }
        } else {
            0.0
        };
        Fit {
            ratio,
            badness: badness(ratio),
            overflow: (-gap).max(0.0),
            overhang,
            protrusion,
        }
    }

    /// What hangs past the measure at break `b`, given how wide the
    /// line would otherwise be.
    fn overhang(&self, b: usize, natural: f32, measure: f32) -> f32 {
        let hang = self.breaks[b].hang_end;
        if hang <= 0.0 {
            return 0.0;
        }
        if b == self.end() && self.options.hanging.last {
            return hang;
        }
        match self.options.hanging.end {
            HangEnd::Force => hang,
            HangEnd::Allow if natural > measure && natural - hang <= measure => hang,
            _ => 0.0,
        }
    }

    /// The chosen line ends, first to last.
    fn run(&self) -> Vec<Fitted> {
        let mut nodes = vec![Node {
            at: 0,
            line: 0,
            fitness: 1,
            hyphens: 0,
            demerits: 0.0,
            ratio: 0.0,
            overhang: 0.0,
            protrusion: 0.0,
            previous: None,
        }];
        let mut active = vec![0usize];
        let mut candidates: Vec<Candidate> = Vec::new();

        for b in 1..self.breaks.len() {
            let forced = b == self.end();
            // The cheapest way to break here anyway, for a paragraph
            // that cannot be set inside the measure at all.
            let mut overfull: Option<Candidate> = None;
            let mut index = 0;
            while index < active.len() {
                let a = active[index];
                let line = nodes[a].line + 1;
                let fit = self.fit(nodes[a].at, b, line);
                if let Some(candidate) = self.candidate(&nodes[a], a, b, line, &fit) {
                    self.keep_best(&mut candidates, candidate);
                }
                let long = fit.ratio < -1.0;
                if long {
                    let candidate = Candidate {
                        demerits: nodes[a].demerits
                            + OVERFULL_DEMERITS * (1.0 + (fit.overflow / self.upem) as f64),
                        ratio: -1.0,
                        fitness: 0,
                        ..self.forced(&nodes[a], a, b, line, &fit)
                    };
                    if overfull
                        .as_ref()
                        .is_none_or(|best| candidate.demerits < best.demerits)
                    {
                        overfull = Some(candidate);
                    }
                }
                if long || forced {
                    active.remove(index);
                } else {
                    index += 1;
                }
            }
            if candidates.is_empty() {
                if !active.is_empty() {
                    continue;
                }
                // Nothing fits and nothing is left to try: overflow
                // the measure rather than drop the text.
                candidates.push(overfull.expect("a line was too long to set"));
            }
            for candidate in candidates.drain(..) {
                nodes.push(Node {
                    at: candidate.at,
                    line: candidate.line,
                    fitness: candidate.fitness,
                    hyphens: candidate.hyphens,
                    demerits: candidate.demerits,
                    ratio: candidate.ratio,
                    overhang: candidate.overhang,
                    protrusion: candidate.protrusion,
                    previous: Some(candidate.previous),
                });
                active.push(nodes.len() - 1);
            }
        }

        let best = nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.at == self.end())
            .min_by(|(_, one), (_, other)| one.demerits.total_cmp(&other.demerits))
            .map(|(index, _)| index);
        let mut chosen = Vec::new();
        let mut at = best;
        while let Some(index) = at {
            let node = &nodes[index];
            if node.previous.is_none() {
                break;
            }
            chosen.push(Fitted {
                at: node.at,
                ratio: node.ratio,
                overhang: node.overhang,
                protrusion: node.protrusion,
            });
            at = node.previous;
        }
        chosen.reverse();
        chosen
    }

    /// Keeps the cheapest candidate of each kind. Two paths that
    /// reach the same break and are alike in everything the rest of
    /// the paragraph can see are interchangeable, so only the cheaper
    /// survives, which is what keeps the active list from growing
    /// with the paragraph.
    fn keep_best(&self, candidates: &mut Vec<Candidate>, candidate: Candidate) {
        let key = |candidate: &Candidate| {
            (
                candidate.fitness,
                candidate.hyphens,
                // Only a shortened measure makes the line number
                // visible from here on; past it every line is the
                // same width.
                candidate.line.min(self.measure.shortened + 1),
            )
        };
        match candidates
            .iter_mut()
            .find(|kept| key(kept) == key(&candidate))
        {
            Some(kept) if kept.demerits <= candidate.demerits => {}
            Some(kept) => *kept = candidate,
            None => candidates.push(candidate),
        }
    }

    /// Break `b` reached from node `a` whatever it costs: the line
    /// between them is wider than the measure, and setting it is
    /// still better than losing the words.
    fn forced(&self, from: &Node, a: usize, b: usize, line: usize, fit: &Fit) -> Candidate {
        Candidate {
            at: b,
            line,
            fitness: 0,
            hyphens: self.hyphens(from, b),
            demerits: from.demerits,
            ratio: fit.ratio,
            overhang: fit.overhang,
            protrusion: fit.protrusion,
            previous: a,
        }
    }

    /// Hyphenated line ends in a row, counting the one break `b`
    /// would add.
    fn hyphens(&self, from: &Node, b: usize) -> u8 {
        if self.breaks[b].hyphen {
            from.hyphens + 1
        } else {
            0
        }
    }

    /// Break `b` reached from node `a`, if the line between them can
    /// be set at all.
    fn candidate(
        &self,
        from: &Node,
        a: usize,
        b: usize,
        line: usize,
        fit: &Fit,
    ) -> Option<Candidate> {
        if fit.ratio < -1.0 {
            return None;
        }
        let hyphens = self.hyphens(from, b);
        if hyphens > MAX_CONSECUTIVE_HYPHENS {
            return None;
        }
        // The last line is not stretched to the measure, so what is
        // left at its right is not a fault to be charged for.
        let ratio = if b == self.end() && fit.ratio > 0.0 {
            0.0
        } else {
            fit.ratio
        };
        let penalty = if self.breaks[b].hyphen {
            HYPHEN_PENALTY
        } else if self.breaks[b].dash {
            DASH_PENALTY
        } else {
            0.0
        };
        let fitness = fitness(ratio);
        let mut demerits = (LINE_PENALTY + fit.badness + penalty).powi(2);
        if hyphens > 1 {
            demerits += DOUBLE_HYPHEN_DEMERITS;
        }
        if fitness.abs_diff(from.fitness) > 1 {
            demerits += ADJACENT_DEMERITS;
        }
        Some(Candidate {
            at: b,
            line,
            fitness,
            hyphens,
            demerits: from.demerits + demerits,
            ratio,
            overhang: fit.overhang,
            protrusion: fit.protrusion,
            previous: a,
        })
    }
}

/// How bad a line of this adjustment ratio is. Cubic, so one gaping
/// line costs more than several slightly loose ones.
fn badness(ratio: f32) -> f64 {
    if !ratio.is_finite() {
        return MAX_BADNESS;
    }
    (100.0 * (ratio.abs() as f64).powi(3)).min(MAX_BADNESS)
}

/// Knuth's four fitness classes: tight, decent, loose, very loose. A
/// tight line under a very loose one reads as a mistake even when
/// each of them on its own does not, which is why the class and not
/// the ratio is what the demerits compare.
fn fitness(ratio: f32) -> u8 {
    if ratio < -0.5 {
        0
    } else if ratio <= 0.5 {
        1
    } else if ratio < 1.0 {
        2
    } else {
        3
    }
}

/// One shaped span, its glyph clusters still relative to the span's
/// own text.
struct ShapedSpan {
    /// Byte range of the span in the paragraph text.
    range: Range<usize>,
    /// What one of this span's font units is worth in the
    /// paragraph's.
    scale: f32,
    /// Tracking charged after each of its clusters, in its own font
    /// units.
    track: i64,
    /// The same in the paragraph's font units.
    tracking: f32,
    glyphs: Vec<ShapedGlyph>,
}

impl ShapedSpan {
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

/// Slices shaped spans into the runs of one line. `end` is where the
/// line's paintable text stops: the spaces a break swallows are
/// already off it.
///
/// A run records the text it was shaped from, which is what a
/// painter that draws characters draws, and beside it what the
/// author wrote, which is what extraction and copy and paste return.
fn cut_runs(
    flat: &FlatParagraph,
    shaped: &[ShapedSpan],
    start: usize,
    end: usize,
) -> Vec<ShapedRun> {
    let mut runs = Vec::new();
    let mut trailing = 0i64;
    for (span, spec) in shaped.iter().zip(flat.spans.iter()) {
        if span.range.start >= end || span.range.end <= start {
            continue;
        }
        let glyphs = span.glyphs_in(start, end);
        if glyphs.is_empty() {
            continue;
        }
        let advance = glyphs.iter().map(|g| g.x_advance).sum();
        let text_start = span.range.start.max(start);
        let text_end = span.range.end.min(end).max(text_start);
        trailing = span.track;
        let (source, source_map) = flat.source_of(text_start..text_end);
        runs.push(ShapedRun {
            font_id: spec.font_id,
            size: spec.size,
            text: flat.text[text_start..text_end].to_string(),
            source,
            source_map,
            text_start: text_start as u32,
            features: spec.features,
            color: spec.color,
            glyphs,
            advance,
        });
    }
    // Tracking goes between letters: the line's last glyph keeps its
    // own advance and nothing more.
    if trailing != 0
        && let Some(run) = runs.last_mut()
    {
        if let Some(glyph) = run.glyphs.last_mut() {
            glyph.x_advance = (glyph.x_advance as i64 - trailing).max(0) as u32;
        }
        run.advance = run.glyphs.iter().map(|g| g.x_advance).sum();
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::NodeId;

    fn registry() -> &'static FontRegistry {
        static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
        REGISTRY.get_or_init(|| crate::fonts::bundled_registry().expect("bundled font parses"))
    }

    /// The body style the built-in sheet computes.
    fn body() -> ParagraphStyle {
        crate::style::defaults(&crate::content::Book::default(), registry())
            .root()
            .paragraph()
    }

    fn layout_body(text: &str, measure_pt: f32) -> Vec<Line> {
        layout_body_opts(text, measure_pt, LineBreakOptions::default())
    }

    /// Justification on, everything else at its default.
    fn justified() -> LineBreakOptions {
        LineBreakOptions {
            justify: true,
            ..Default::default()
        }
    }

    /// Hyphenation on, everything else at its default.
    fn hyphenated() -> LineBreakOptions {
        LineBreakOptions {
            hyphenate: true,
            ..Default::default()
        }
    }

    fn layout_body_opts(text: &str, measure_pt: f32, options: LineBreakOptions) -> Vec<Line> {
        let layout = LineLayout::new(registry());
        let inlines = vec![Inline::Text {
            id: NodeId::UNASSIGNED,
            value: text.to_string(),
            position: None,
        }];
        layout.layout(&inlines, body(), measure_pt, options)
    }

    fn units_per_em() -> u16 {
        registry().metrics(0).unwrap().units_per_em
    }

    /// One paragraph laid out under a style of the caller's, ragged
    /// and unhyphenated.
    fn layout_style(text: &str, measure_pt: f32, style: ParagraphStyle) -> Vec<Line> {
        let layout = LineLayout::new(registry());
        let inlines = vec![Inline::Text {
            id: NodeId::UNASSIGNED,
            value: text.to_string(),
            position: None,
        }];
        layout.layout(&inlines, style, measure_pt, LineBreakOptions::default())
    }

    /// A line's text, concatenated from its runs.
    ///
    /// Read off the runs rather than sliced out of the paragraph: a
    /// hyphenated line ends in a character the paragraph never had.
    fn line_text(line: &Line) -> String {
        line.runs.iter().map(|run| run.text.as_str()).collect()
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

    /// Em-dash: UAX #14 allows the break after B2-class characters,
    /// so `word—word` has an opportunity mid-string.
    #[test]
    fn em_dash_provides_a_break_opportunity() {
        let text = "word—word word—word";
        let lines = layout_body(text, 34.0);
        assert!(lines.len() >= 2);
        let first = line_text(&lines[0]);
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
            let t = line_text(line);
            assert!(
                !t.starts_with('.') && !t.starts_with(','),
                "punctuation started a line: {t:?}"
            );
        }
        let first = line_text(&lines[0]);
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
        assert_eq!(line_text(&lines[lines.len() - 1]), "extraordinary");
    }

    /// Hyphenation on: a long word splits at syllable boundaries and
    /// no line exceeds the measure.
    #[test]
    fn hyphenation_splits_long_words() {
        let text = "extraordinarily";
        let lines = layout_body_opts(text, 44.0, hyphenated());
        assert!(lines.len() >= 2, "expected a split, got {lines:?}");
        for line in &lines {
            let width_pt = line.width as f32 / units_per_em() as f32 * body().size;
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
        let lines = layout_body_opts(text, 53.0, hyphenated());
        assert!(lines.len() >= 2, "expected a split, got {lines:?}");
        let first = line_text(&lines[0]);
        assert!(
            first.ends_with("extraordi-"),
            "greedy undercharged the hyphen: line 1 is {first:?}"
        );
    }

    /// The hyphen is painted as well as charged: the run has the
    /// character and a glyph for it, and the last line has neither.
    #[test]
    fn a_hyphenated_line_paints_the_hyphen_it_paid_for() {
        let text = "extraordinarily";
        let lines = layout_body_opts(text, 53.0, hyphenated());
        let first = &lines[0];
        assert_eq!(line_text(first), "extraordi-");

        let run = first.runs.last().expect("the line has no runs");
        assert_eq!(
            run.glyphs.len(),
            run.text.chars().count(),
            "the hyphen is in the text and not in the glyphs",
        );
        let ranges = run.glyph_ranges();
        let last = ranges.last().expect("the run has no glyphs");
        assert_eq!(
            &run.text[last.start as usize..last.end as usize],
            "-",
            "the last glyph does not stand for the hyphen",
        );

        // Charged and drawn are the same number: the line's width
        // covers the glyph it ends with.
        let hyphen = run.glyphs.last().expect("the run has no glyphs").x_advance;
        assert!(hyphen > 0, "the hyphen has no advance");
        assert_eq!(
            first.width,
            first.runs.iter().map(|r| r.advance).sum::<u32>(),
            "the line's width and its runs disagree",
        );

        let last_line = lines.last().expect("no lines");
        assert!(
            !line_text(last_line).ends_with('-'),
            "the last line was hyphenated: {:?}",
            line_text(last_line),
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
        let lines = layout.layout(&inlines, body(), 200.0, Default::default());
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].runs.len(), 2);
        assert_eq!(line_text(&lines[0]), "body code");
    }

    /// Emphasis is its own span: a paragraph of roman prose around
    /// italic dialogue breaks into runs at the markup's boundaries,
    /// each on the face its style resolved to, and a nested `strong`
    /// takes the bold italic cut.
    #[test]
    fn emphasis_shapes_on_its_own_face() {
        let mut book = crate::content::Book {
            metadata: Default::default(),
            sections: vec![crate::content::Section {
                blocks: vec![crate::content::Block::Paragraph {
                    id: NodeId::UNASSIGNED,
                    inlines: vec![
                        Inline::Text {
                            id: NodeId::UNASSIGNED,
                            value: "He said ".into(),
                            position: None,
                        },
                        Inline::Emphasis {
                            id: NodeId::UNASSIGNED,
                            children: vec![
                                Inline::Text {
                                    id: NodeId::UNASSIGNED,
                                    value: "never ".into(),
                                    position: None,
                                },
                                Inline::Strong {
                                    id: NodeId::UNASSIGNED,
                                    children: vec![Inline::Text {
                                        id: NodeId::UNASSIGNED,
                                        value: "again".into(),
                                        position: None,
                                    }],
                                    position: None,
                                },
                            ],
                            position: None,
                        },
                        Inline::Text {
                            id: NodeId::UNASSIGNED,
                            value: " to her.".into(),
                            position: None,
                        },
                    ],
                    position: None,
                }],
                ..Default::default()
            }],
        };
        book.assign_node_ids();
        let styles = crate::style::defaults(&book, registry());
        let crate::content::Block::Paragraph { id, inlines, .. } = &book.sections[0].blocks[0]
        else {
            unreachable!()
        };
        let lines = LineLayout::new(registry()).layout_styled(
            inlines,
            styles.paragraph(*id),
            &styles,
            400.0,
            Default::default(),
        );
        assert_eq!(lines.len(), 1);
        let runs: Vec<(u16, &str)> = lines[0]
            .runs
            .iter()
            .map(|run| (run.font_id, run.text.as_str()))
            .collect();
        let face = |italic, weight| {
            registry()
                .select(
                    "eb garamond",
                    crate::fonts::FaceAttributes { italic, weight },
                )
                .unwrap()
                .id
        };
        assert_eq!(
            runs,
            vec![
                (face(false, 400), "He said "),
                (face(true, 400), "never "),
                (face(true, 700), "again"),
                (face(false, 400), " to her."),
            ],
        );
    }

    /// A paragraph of only spaces produces no lines.
    #[test]
    fn spaces_only_paragraph_is_empty() {
        assert!(layout_body("   ", 200.0).is_empty());
    }

    /// A drop cap shortens the lines beside it: the first few break
    /// to a narrower measure, and the rest go back to the full one.
    #[test]
    fn a_shortened_measure_only_holds_for_the_lines_it_names() {
        let text = "one two three four five six seven eight nine ten eleven twelve";
        let measure = Measure {
            full: 120.0,
            narrow: 40.0,
            shortened: 2,
        };
        let layout = LineLayout::new(registry());
        let inlines = vec![Inline::Text {
            id: NodeId::UNASSIGNED,
            value: text.to_string(),
            position: None,
        }];
        let lines = layout.layout(&inlines, body(), measure, Default::default());
        assert!(lines.len() > 3, "expected several lines: {lines:?}");
        let width_pt = |line: &Line| line.width as f32 / units_per_em() as f32 * body().size;
        for (index, line) in lines.iter().enumerate() {
            let allowed = measure.at(index);
            assert!(
                width_pt(line) <= allowed,
                "line {index} is {}pt against a measure of {allowed}pt",
                width_pt(line),
            );
        }
        // The lines that were not shortened use the width the
        // shortened ones could not: nothing is lost, and the same
        // text set at one measure breaks differently.
        assert!(
            width_pt(&lines[2]) > measure.narrow,
            "the measure never widened"
        );
        let uniform = layout.layout(&inlines, body(), 120.0, Default::default());
        assert!(uniform.len() < lines.len());
        assert_eq!(
            lines.iter().map(line_text).collect::<Vec<_>>().join(" "),
            text,
        );
    }

    /// Justified: every line but the last reaches the right edge of
    /// the measure. The tolerance is the rounding the shaper's
    /// integer advances force: the adjustment is spread over the
    /// line's spaces and each lands on a whole font unit, so a line
    /// can miss by half a unit, which at 11pt is 0.006pt.
    #[test]
    fn justified_lines_fill_the_measure() {
        let text = "My father had a small estate in Nottinghamshire, and I was the \
                    third of five sons. He sent me to Emanuel College in Cambridge \
                    at fourteen years old, where I resided three years.";
        let lines = layout_body_opts(text, 140.0, justified());
        assert!(lines.len() > 3, "expected several lines: {lines:?}");
        for line in &lines[..lines.len() - 1] {
            let width = line.width as f32 / units_per_em() as f32 * body().size;
            assert!(
                (width - 140.0).abs() < 0.01,
                "justified line is {width}pt against a 140pt measure",
            );
        }
        let last = lines.last().unwrap();
        let width = last.width as f32 / units_per_em() as f32 * body().size;
        assert!(width < 140.0, "the last line was stretched to {width}pt");
    }

    /// The adjustment lands on the spaces. Ragged and justified
    /// settings of the same line set the same letters at the same
    /// advances; only what is between the words moves.
    #[test]
    fn justification_opens_the_spaces_and_nothing_else() {
        let text = "one two three four five six seven eight nine ten";
        let ragged = layout_body_opts(text, 100.0, Default::default());
        let justified = layout_body_opts(text, 100.0, justified());
        assert_eq!(ragged.len(), justified.len());
        let space = registry().char_glyph(0, ' ').unwrap();
        let glyphs = |line: &Line| -> Vec<(u32, u32)> {
            line.runs
                .iter()
                .flat_map(|run| run.glyphs.iter())
                .map(|glyph| (glyph.id, glyph.x_advance))
                .collect()
        };
        let (before, after) = (glyphs(&ragged[0]), glyphs(&justified[0]));
        assert_eq!(
            before.len(),
            after.len(),
            "justification changed the glyphs on the line",
        );
        for (was, now) in before.iter().zip(after.iter()) {
            assert_eq!(was.0, now.0, "justification reshaped the line");
            if was.0 == space {
                assert!(now.1 > was.1, "the spaces did not open: {was:?} {now:?}");
            } else {
                assert_eq!(was.1, now.1, "a letter moved: {was:?} {now:?}");
            }
        }
    }

    /// Inter-letter spacing is opt-in: the same line justified with
    /// `text-justify: inter-character` widens its letters, and the
    /// default leaves them alone.
    #[test]
    fn inter_letter_spacing_is_off_until_asked_for() {
        let text = "one two three four five six seven eight nine ten";
        let letters = |line: &Line| -> u32 {
            let space = registry().char_glyph(0, ' ').unwrap();
            line.runs
                .iter()
                .flat_map(|run| run.glyphs.iter())
                .filter(|glyph| glyph.id != space)
                .map(|glyph| glyph.x_advance)
                .sum()
        };
        let words = layout_body_opts(text, 100.0, justified());
        let characters = layout_body_opts(
            text,
            100.0,
            LineBreakOptions {
                inter_character: true,
                ..justified()
            },
        );
        assert!(letters(&characters[0]) > letters(&words[0]));
        let width = |line: &Line| line.width as f32 / units_per_em() as f32 * body().size;
        assert!(
            (width(&characters[0]) - 100.0).abs() < 0.01,
            "the line stopped filling the measure: {}pt",
            width(&characters[0]),
        );
    }

    /// Hanging punctuation: a mark at a line end is not charged to
    /// the measure, so a word that would not otherwise fit does.
    #[test]
    fn a_mark_at_a_line_end_hangs_past_the_measure() {
        // `My father had a small estate,` is 117.74pt: over a 116pt
        // measure by less than the comma hangs.
        let text = "My father had a small estate, and I was the third of five sons.";
        let hanging = LineBreakOptions {
            hanging: HangingPunctuation {
                end: HangEnd::Force,
                ..Default::default()
            },
            ..Default::default()
        };
        let flush = layout_body(text, 116.0);
        let hung = layout_body_opts(text, 116.0, hanging);
        assert_eq!(line_text(&flush[0]), "My father had a small");
        assert_eq!(line_text(&hung[0]), "My father had a small estate,");
        assert!(hung[0].overhang > 0.0, "the comma was still charged");
    }

    /// Margin kerning: a line opening on a quotation mark starts
    /// before the measure does, by part of the mark's own width.
    #[test]
    fn an_opening_mark_hangs_into_the_margin() {
        let text = "\"He said it would be so,\" and it was.";
        let kerned = LineBreakOptions {
            hanging: HangingPunctuation {
                first: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let lines = layout_body_opts(text, 200.0, kerned);
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].protrusion > 0.0,
            "the quotation mark was not pulled out",
        );
        let quote = registry()
            .advance_width(0, registry().char_glyph(0, '"').unwrap())
            .unwrap() as f32
            / units_per_em() as f32
            * body().size;
        assert!(
            lines[0].protrusion < quote,
            "the whole mark left the measure",
        );
    }

    /// Acceptance: hyphenation never runs to three line ends in a
    /// row, whatever the demerits would otherwise say. A narrow
    /// measure over long words is where a breaker would do it.
    #[test]
    fn hyphens_never_run_three_deep() {
        let text = "extraordinarily complicated administrative organisation \
                    demonstrably incomprehensible";
        let lines = layout_body_opts(text, 40.0, hyphenated());
        let hyphenated: Vec<bool> = lines
            .iter()
            .map(|line| line_text(line).ends_with('-'))
            .collect();
        assert!(
            hyphenated.iter().any(|end| *end),
            "nothing was hyphenated: {hyphenated:?}",
        );
        assert!(
            !hyphenated.windows(3).any(|run| run == [true, true, true]),
            "three hyphenated line ends in a row: {hyphenated:?}",
        );
    }

    /// Acceptance: the fixture paragraph breaks where total fit says
    /// it should, and at one of these measures that is not where
    /// greedy would have put it. The opening of Gulliver §2, widths
    /// derived independently of this module (per-word sums of
    /// hb-shape advances for EB Garamond).
    ///
    /// Per-word widths (pt): My 14.29, father 24.70, had 15.62,
    /// a 4.39, small 21.78, estate 23.43, in 8.50,
    /// Nottinghamshire: 75.57; space 2.20.
    #[test]
    fn breaks_match_hand_computed_reference() {
        let text = "My father had a small estate in Nottinghamshire:";
        let expected: &[(f32, &[&str])] = &[
            (
                50.0,
                &["My father", "had a small", "estate in", "Nottinghamshire:"],
            ),
            (
                60.0,
                &["My father", "had a small", "estate in", "Nottinghamshire:"],
            ),
            (
                80.0,
                &["My father had", "a small estate in", "Nottinghamshire:"],
            ),
            (
                120.0,
                &["My father had a small estate", "in Nottinghamshire:"],
            ),
            (250.0, &["My father had a small estate in Nottinghamshire:"]),
        ];
        for (measure, want_lines) in expected {
            let lines = layout_body(text, *measure);
            let got: Vec<String> = lines.iter().map(line_text).collect();
            assert_eq!(&got, want_lines, "measure {measure}: {lines:?}");
        }
    }

    /// The arithmetic behind the 80pt row above, which is the row
    /// where the two breakers part.
    ///
    /// Ragged badness is `100 * r^3`, where `r` is the gap left at
    /// the right over a tenth of the measure, and a line costs
    /// `(10 + badness)^2`, with 10,000 more when its fitness class is
    /// two off the line before it. The last line fills what it fills
    /// and costs 100.
    ///
    /// Total fit: 59.01 (r 2.62, 3.30M) + 64.70 (r 1.91, 0.50M)
    /// + 75.57 (100) = 3.82M, two class changes included.
    ///
    /// Greedy: 65.60 (r 1.80, 0.35M) + 58.11 (r 2.74, 4.24M)
    /// + 75.57 (100) = 4.61M, the same two.
    ///
    /// Greedy wins line one and loses the paragraph: the word it
    /// pulls up leaves a hole on line two larger than the one it
    /// filled.
    #[test]
    fn total_fit_beats_greedy_where_they_disagree() {
        let text = "My father had a small estate in Nottinghamshire:";
        let got: Vec<String> = layout_body(text, 80.0).iter().map(line_text).collect();
        assert_eq!(got[0], "My father had", "line 1: {got:?}");
        assert_ne!(
            got[0], "My father had a",
            "line 1 was packed greedily: {got:?}",
        );
    }

    /// Letter-spacing is advance on the shaper's glyphs: every gap
    /// between two glyphs opens by the tracking, and the last glyph
    /// keeps its own advance, so a tracked title measures the tracking
    /// times one fewer than its glyphs.
    #[test]
    fn letter_spacing_opens_the_gaps_between_glyphs() {
        let plain = layout_style("HANDGLOVES", 400.0, body());
        let tracking = 0.08 * body().size;
        let tracked = layout_style(
            "HANDGLOVES",
            400.0,
            ParagraphStyle {
                letter_spacing: tracking,
                ..body()
            },
        );
        let glyphs = plain[0].runs[0].glyphs.len();
        assert_eq!(glyphs, 10, "the title shaped to one glyph a letter");
        let units = tracking / body().size * units_per_em() as f32;
        assert_eq!(
            tracked[0].width - plain[0].width,
            (units * (glyphs - 1) as f32).round() as u32,
            "tracking did not open exactly the gaps between the glyphs",
        );
        for (loose, tight) in tracked[0].runs[0]
            .glyphs
            .iter()
            .zip(&plain[0].runs[0].glyphs)
            .take(glyphs - 1)
        {
            assert_eq!(
                loose.x_advance - tight.x_advance,
                units.round() as u32,
                "a glyph has no tracking of its own",
            );
        }
    }

    /// Small capitals come out of the face where the face has them:
    /// the run stays at the size around it and draws glyphs the plain
    /// text does not. A face with no substitutions of its own gets a
    /// synthesis instead, the letters raised to capitals and set at a
    /// fraction of the size, and the capitals already there are left
    /// alone.
    #[test]
    fn small_caps_take_the_feature_or_a_synthesis() {
        let style = ParagraphStyle {
            caps: FontVariantCaps::SmallCaps,
            ..body()
        };
        let plain = layout_style("hello", 400.0, body());
        let feature = layout_style("hello", 400.0, style);
        assert_eq!(feature[0].runs.len(), 1, "the feature split the run");
        assert_eq!(feature[0].runs[0].size, body().size);
        assert_ne!(
            feature[0].runs[0]
                .glyphs
                .iter()
                .map(|g| g.id)
                .collect::<Vec<_>>(),
            plain[0].runs[0]
                .glyphs
                .iter()
                .map(|g| g.id)
                .collect::<Vec<_>>(),
            "the face's small capitals drew the lowercase glyphs",
        );

        let bare = crate::fonts::registry_without_substitutions();
        assert!(!bare.has_small_caps(0), "the face still substitutes");
        let inlines = vec![Inline::Text {
            id: NodeId::UNASSIGNED,
            value: "hi Ho".to_string(),
            position: None,
        }];
        let synthesized =
            LineLayout::new(&bare).layout(&inlines, style, 400.0, LineBreakOptions::default());
        let runs: Vec<(f32, &str, &str)> = synthesized[0]
            .runs
            .iter()
            .map(|run| (run.size, run.text.as_str(), run.source.as_str()))
            .collect();
        assert_eq!(
            runs,
            vec![
                (body().size * SMALL_CAPS_RATIO, "HI", "hi"),
                (body().size, " H", ""),
                (body().size * SMALL_CAPS_RATIO, "O", "o"),
            ],
            "the synthesis did not raise what was lowercase, leave the rest, \
             and keep what the author wrote",
        );
    }

    /// `text-transform` changes what is shaped, and the run says so
    /// twice over: `text` is what was drawn, which is what a painter
    /// that draws characters draws, and `source` is what the author
    /// wrote, which is what extraction reads back.
    #[test]
    fn text_transform_shapes_one_text_and_reports_another() {
        let style = ParagraphStyle {
            transform: TextTransform::Uppercase,
            ..body()
        };
        let lines = layout_style("Lilliput", 400.0, style);
        assert_eq!(
            line_text(&lines[0]),
            "LILLIPUT",
            "the run was not drawn in capitals"
        );
        assert_eq!(
            lines[0].runs[0].source, "Lilliput",
            "the run lost the source"
        );
        let shouted = layout_style("LILLIPUT", 400.0, body());
        assert_eq!(
            lines[0].runs[0]
                .glyphs
                .iter()
                .map(|g| g.id)
                .collect::<Vec<_>>(),
            shouted[0].runs[0]
                .glyphs
                .iter()
                .map(|g| g.id)
                .collect::<Vec<_>>(),
            "the transform did not reach the shaper",
        );

        // A mapping that is not one for one: `ß` shapes as two
        // capitals, and both of them stand for the one letter.
        let lines = layout_style("Straße", 400.0, style);
        let run = &lines[0].runs[0];
        assert_eq!(
            (run.text.as_str(), run.source.as_str()),
            ("STRASSE", "Straße")
        );
        assert_eq!(run.glyphs.len(), 7, "STRASSE is seven glyphs");
        let sharp = run.source.find('ß').expect("the source keeps its ß") as u32;
        let through = |range: Range<u32>| {
            run.source_map[range.start as usize]..run.source_map[range.end as usize]
        };
        let ranges = run.glyph_ranges();
        assert_eq!(
            (through(ranges[4].clone()), through(ranges[5].clone())),
            (sharp..sharp + 2, sharp + 2..sharp + 2),
            "the pair of capitals does not stand for the ß once",
        );
        assert_eq!(
            run.source_map.last().copied(),
            Some(run.source.len() as u32),
            "the map does not run to the end of the source",
        );
    }

    /// `capitalize` raises the letter that opens a word, and a word
    /// continues through letters, digits and the apostrophe inside one.
    #[test]
    fn capitalize_raises_the_letter_that_opens_a_word() {
        let style = ParagraphStyle {
            transform: TextTransform::Capitalize,
            ..body()
        };
        let source = "the well-known don't of it";
        let lines = layout_style(source, 400.0, style);
        assert_eq!(line_text(&lines[0]), "The Well-Known Don't Of It");
        assert_eq!(lines[0].runs[0].source, source, "the run lost the source");
        let shaped = layout_style("The Well-Known Don't Of It", 400.0, body());
        assert_eq!(
            lines[0].runs[0]
                .glyphs
                .iter()
                .map(|g| g.id)
                .collect::<Vec<_>>(),
            shaped[0].runs[0]
                .glyphs
                .iter()
                .map(|g| g.id)
                .collect::<Vec<_>>(),
            "capitalize did not raise the letters a word opens with",
        );
    }
}
