//! A broken line: the runs on it, the spans it fills, and the
//! widths that measure it.

use std::ops::Range;

use crate::content::SourceRange;
use crate::fonts::{Features, ShapedGlyph};
use crate::linebox::LineBox;
use crate::style::Color;

use super::flatten::FlatParagraph;
use super::shape::ShapedSpan;

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
    /// Where the run was written: the node it was shaped from and
    /// the bytes of that node's own text it stands for. `None` for
    /// text no node was walked for: page furniture, an ornament.
    pub origin: Option<SourceRange>,
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

/// One span of a line: the runs set in it, and where they go.
#[derive(Debug, Clone, PartialEq)]
pub struct LineSpan {
    /// The runs of `Line::runs` set here.
    pub runs: Range<usize>,
    /// Points from the line's own leading edge the span is set at.
    pub offset: f32,
    /// Advance of the span's glyphs, in font units.
    pub width: u32,
}

/// The spans of one line, read as a slice either way. A band set
/// undivided holds its span inline: the ordinary line of a book does
/// not pay for an allocation.
#[derive(Debug, Clone, PartialEq)]
pub enum Spans {
    /// The band was set in one span.
    One(LineSpan),
    /// It was divided, and these are its spans in reading order.
    Many(Vec<LineSpan>),
}

impl std::ops::Deref for Spans {
    type Target = [LineSpan];

    fn deref(&self) -> &[LineSpan] {
        match self {
            Spans::One(span) => std::slice::from_ref(span),
            Spans::Many(spans) => spans,
        }
    }
}

impl std::ops::DerefMut for Spans {
    fn deref_mut(&mut self) -> &mut [LineSpan] {
        match self {
            Spans::One(span) => std::slice::from_mut(span),
            Spans::Many(spans) => spans,
        }
    }
}

/// One typeset line: shaped runs plus its width in font units.
///
/// A line is a band of the page, which is set in one span or in
/// several. The runs are in reading order across all of them.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// The line's runs, in visual order.
    pub runs: Vec<ShapedRun>,
    /// The spans the runs are divided between, in reading order.
    pub spans: Spans,
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

impl Line {
    /// Shaped runs as a line of one span: page furniture, an
    /// ornament, an initial letter, and anything else set rather
    /// than broken.
    pub fn of(runs: Vec<ShapedRun>, box_: LineBox) -> Line {
        let width = runs.iter().map(|run| run.advance).sum();
        Line {
            spans: Spans::One(LineSpan {
                runs: 0..runs.len(),
                offset: 0.0,
                width,
            }),
            runs,
            width,
            overhang: 0.0,
            protrusion: 0.0,
            box_,
        }
    }

    /// A band with nothing set in it yet.
    pub(super) fn empty() -> Line {
        Line {
            runs: Vec::new(),
            spans: Spans::Many(Vec::new()),
            width: 0,
            overhang: 0.0,
            protrusion: 0.0,
            box_: LineBox {
                height: 0.0,
                baseline: 0.0,
            },
        }
    }
}

/// Prefix sums over the paragraph's shaped glyphs, in the
/// paragraph's own font units: the width of any byte range is one
/// subtraction.
pub(super) struct Widths {
    /// Advance of every glyph whose cluster starts before byte `i`.
    pub(super) text: Vec<f32>,
    /// The same for space glyphs alone, which is where the glue is.
    pub(super) spaces: Vec<f32>,
    /// Whether a glyph's cluster starts at byte `i`. A break inside
    /// a cluster would cut a ligature in half and lose it.
    pub(super) starts: Vec<bool>,
    /// Tracking charged to the last cluster starting before byte `i`.
    /// Empty where nothing is tracked, which is most paragraphs.
    pub(super) trailing: Vec<f32>,
}

impl Widths {
    pub(super) fn build(text: &str, shaped: &[ShapedSpan]) -> Widths {
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
    pub(super) fn advance(&self, from: usize, to: usize) -> f32 {
        if to <= from {
            return 0.0;
        }
        self.text[to] - self.text[from] - self.trailing.get(to).copied().unwrap_or(0.0)
    }
}

/// One band's spans, leaving the buffer to the band after it.
pub(super) fn gather(spans: &mut Vec<LineSpan>) -> Spans {
    match spans.len() {
        1 => Spans::One(spans.pop().expect("a span")),
        _ => Spans::Many(std::mem::take(spans)),
    }
}

/// Slices shaped spans into the runs of one line. `end` is where the
/// line's paintable text stops: the spaces a break swallows are
/// already off it.
///
/// A run records the text it was shaped from, which is what a
/// painter that draws characters draws, and beside it what the
/// author wrote, which is what extraction and copy and paste return.
pub(super) fn cut_runs(
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
            // Where the run was written is settled once the
            // paragraph is broken, by `tile`.
            origin: None,
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

/// Says where each of a paragraph's runs was written, each range
/// running on to where the next one starts, so the space a break
/// swallowed belongs to a run rather than to nothing and the runs
/// naming one node tile that node's text.
pub(super) fn tile(lines: &mut [Line], flat: &FlatParagraph) {
    let starts: Vec<usize> = lines
        .iter()
        .flat_map(|line| &line.runs)
        .map(|run| run.text_start as usize)
        .collect();
    let mut ends = starts.iter().skip(1).copied().chain([flat.text.len()]);
    let mut after = None;
    for run in lines.iter_mut().flat_map(|line| &mut line.runs) {
        let to = ends.next().unwrap_or(flat.text.len());
        run.origin = flat.origin_of(after, run.text_start as usize, to);
        after = run.origin.as_ref().map(|origin| origin.node);
    }
}

#[cfg(test)]
mod tests {

    use crate::lines::testing::layout_body;

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
}
