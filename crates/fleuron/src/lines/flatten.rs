//! One paragraph's inlines as a single string of styled spans, with
//! the manuscript each character came from kept beside it.

use std::ops::Range;

use crate::content::{Inline, NodeId, SourceRange};
use crate::fonts::Features;
use crate::style::{Color, FontVariantCaps, TextTransform};

use super::LineLayout;
use super::paragraph::{InlineStyles, Lead, ParagraphStyle};

/// Flattened paragraph content: the text as it is shaped, the text
/// the author wrote under it, and, per style span, the byte range it
/// covers. Style boundaries are segmentation boundaries — a shaped
/// run never spans two fonts.
#[derive(Default)]
pub(super) struct FlatParagraph {
    /// What is shaped, measured and broken. `text-transform` and
    /// synthesized small capitals have already been applied.
    pub(super) text: String,
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
    pub(super) spans: Vec<StyleSpan>,
    /// Which node each stretch of the source was written in, in
    /// document order.
    origins: Vec<Origin>,
    /// Bytes of the source still to be passed over before anything
    /// is written: what a drop cap took out of the paragraph.
    skip: usize,
}

/// One stretch of the paragraph's source, and the node it was
/// written in. Stretches are contiguous, so one origin runs to where
/// the next begins.
struct Origin {
    node: NodeId,
    /// Where the stretch starts in the paragraph's source.
    start: u32,
    /// Where it starts in the node's own text, which is past the
    /// letter wherever a drop cap took one.
    node_start: u32,
}

/// One span of uniform shaping: a face, a size, the tracking after
/// each of its clusters, and the features it is shaped with.
pub(super) struct StyleSpan {
    pub(super) font_id: u16,
    pub(super) size: f32,
    /// Extra advance after each cluster, in points.
    pub(super) tracking: f32,
    pub(super) features: Features,
    pub(super) color: Color,
    /// Byte range in the paragraph's shaped text.
    pub(super) range: Range<usize>,
}

impl StyleSpan {
    /// Whether two spans are set the same way, the text they cover
    /// aside.
    fn same_style(&self, other: &StyleSpan) -> bool {
        self.font_id == other.font_id
            && self.size == other.size
            && self.tracking == other.tracking
            && self.features == other.features
            && self.color == other.color
    }
}

/// How a span gets its small capitals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SmallCaps {
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
    pub(super) fn new() -> FlatParagraph {
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
    pub(super) fn source_of(&self, range: Range<usize>) -> (String, Vec<u32>) {
        if !self.transformed {
            return (String::new(), Vec::new());
        }
        let at = |byte: usize| self.source_at(byte);
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

    /// Where one byte of the shaped text falls in the source it was
    /// made from. The two run together until something transforms,
    /// and the map holds them together after that.
    fn source_at(&self, byte: usize) -> usize {
        if !self.transformed {
            return byte.min(self.text.len());
        }
        match self.map.get(byte) {
            Some(offset) => *offset as usize,
            None => self.source.len(),
        }
    }

    /// How much source the paragraph has been written from so far.
    fn source_len(&self) -> usize {
        if self.transformed {
            self.source.len()
        } else {
            self.text.len()
        }
    }

    /// Opens a stretch of the source written in `node`, starting
    /// `node_start` bytes into that node's own text.
    fn open(&mut self, node: NodeId, node_start: usize) {
        self.origins.push(Origin {
            node,
            start: self.source_len() as u32,
            node_start: node_start as u32,
        });
    }

    /// The stretch of the source one byte of the shaped text was
    /// written in: the node, where the stretch starts in that node's
    /// own text, and the source it covers.
    ///
    /// `None` where no node was walked: page furniture, an ornament.
    fn origin_at(&self, byte: usize) -> Option<(NodeId, u32, Range<usize>)> {
        let at = self.source_at(byte);
        let index = self
            .origins
            .partition_point(|origin| origin.start as usize <= at)
            .checked_sub(1)?;
        let origin = &self.origins[index];
        let end = self.origins[index + 1..]
            .first()
            .map_or(self.source_len(), |next| next.start as usize);
        Some((origin.node, origin.node_start, origin.start as usize..end))
    }

    /// The node one stretch of the shaped text was written in, and
    /// the bytes of that node's own text it stands for, running from
    /// `from` to wherever `to` reaches.
    ///
    /// `after` is the node the stretch before this one was written
    /// in. A stretch opening a node opens at that node's first byte,
    /// wherever the break before it fell, so the space a break
    /// swallowed at a node boundary belongs to the node that holds
    /// it. Both ends stop at the node.
    pub(super) fn origin_of(
        &self,
        after: Option<NodeId>,
        from: usize,
        to: usize,
    ) -> Option<SourceRange> {
        let (node, node_start, stretch) = self.origin_at(from)?;
        let start = if after == Some(node) {
            self.source_at(from).clamp(stretch.start, stretch.end)
        } else {
            stretch.start
        };
        let end = self.source_at(to).clamp(start, stretch.end);
        let at = |source: usize| node_start + (source - stretch.start) as u32;
        Some(SourceRange {
            node,
            range: at(start)..at(end),
        })
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
    pub(super) fn push_styled(&mut self, value: &str, style: ParagraphStyle, caps: SmallCaps) {
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
        let span = StyleSpan {
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
        };
        self.spans.push(span);
    }

    /// Joins two adjacent spans set the same way, from `from` to the
    /// end. Text written a character at a time is one run of one
    /// style as much as text written in a stretch, and a run shapes
    /// as a whole: kerning and ligatures do not reach across a span.
    fn merge_from(&mut self, from: usize) {
        let mut at = from + 1;
        while at < self.spans.len() {
            let joins = self.spans[at - 1].range.end == self.spans[at].range.start
                && self.spans[at - 1].same_style(&self.spans[at]);
            if joins {
                self.spans[at - 1].range.end = self.spans[at].range.end;
                self.spans.remove(at);
            } else {
                at += 1;
            }
        }
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

/// What a synthesized small capital is set at, as a fraction of the
/// size around it. A face's own small caps sit a little above the
/// x-height, and four-fifths of the cap height is about where that
/// lands.
pub(super) const SMALL_CAPS_RATIO: f32 = 0.8;

impl LineLayout<'_> {
    /// Flattens a paragraph's inlines, each one styled as the tree
    /// says. A style boundary is a span boundary, so a run never
    /// spans two faces.
    pub(super) fn flatten(
        &self,
        inlines: &[Inline],
        style: ParagraphStyle,
        styles: &dyn InlineStyles,
        lead: Lead,
    ) -> FlatParagraph {
        let mut flat = FlatParagraph::new();
        flat.skip = lead.taken;
        self.walk_inlines(inlines, style, styles, lead, &mut flat);
        flat
    }

    fn walk_inlines(
        &self,
        inlines: &[Inline],
        style: ParagraphStyle,
        styles: &dyn InlineStyles,
        lead: Lead,
        flat: &mut FlatParagraph,
    ) {
        for inline in inlines {
            match inline {
                Inline::Text { id, value, .. } => self.push_text(flat, *id, value, style, lead),
                Inline::Code { id, value, .. } => {
                    self.push_text(flat, *id, value, styles.style(*id, style), lead)
                }
                Inline::Emphasis { id, children, .. }
                | Inline::Strong { id, children, .. }
                | Inline::Link { id, children, .. } => {
                    self.walk_inlines(children, styles.style(*id, style), styles, lead, flat);
                }
            }
        }
    }

    /// Appends one node's text, in the opening style as far as it
    /// reaches and in `style` after that. What a drop cap took is
    /// passed over here, so the paragraph starts at the letter after
    /// the one the cap holds.
    ///
    /// The switch to `style` is on the length of the shaped text,
    /// which is what the extent a break gave is written in. Where the
    /// opening style still applies the text goes in a character at a
    /// time, so the switch can fall inside a word; the spans it
    /// writes merge back into one, and the whole of the opening line
    /// shapes together.
    fn push_text(
        &self,
        flat: &mut FlatParagraph,
        node: NodeId,
        value: &str,
        style: ParagraphStyle,
        lead: Lead,
    ) {
        let mut at = flat.skip.min(value.len());
        while !value.is_char_boundary(at) {
            at += 1;
        }
        flat.skip = flat.skip.saturating_sub(at);
        let (node_start, value) = (at, &value[at..]);
        if value.is_empty() {
            return;
        }
        flat.open(node, node_start);
        let (opening, extent) = match lead.style {
            Some(first) if flat.text.len() < lead.reach() => (first.over(style), lead.reach()),
            _ => {
                flat.push_styled(value, style, self.small_caps(style));
                return;
            }
        };
        let (opening_caps, caps) = (self.small_caps(opening), self.small_caps(style));
        let from = flat.spans.len();
        for (at, letter) in value.char_indices() {
            if flat.text.len() >= extent {
                flat.push_styled(&value[at..], style, caps);
                break;
            }
            flat.push_styled(&value[at..at + letter.len_utf8()], opening, opening_caps);
        }
        flat.merge_from(from);
    }

    /// Where a style's small capitals come from: the face's own where
    /// it has them, and a synthesis where it does not.
    pub(super) fn small_caps(&self, style: ParagraphStyle) -> SmallCaps {
        match style.caps {
            FontVariantCaps::Normal => SmallCaps::Off,
            FontVariantCaps::SmallCaps if self.registry.has_small_caps(style.font_id) => {
                SmallCaps::Feature
            }
            FontVariantCaps::SmallCaps => SmallCaps::Synthesized,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{Attributes, Inline, NodeId};
    use crate::lines::testing::{body, layout_body, layout_style, line_text, registry};
    use crate::lines::{LineLayout, ParagraphStyle};
    use crate::style::TextTransform;

    /// Two style runs (body + code): both land on the line in order.
    #[test]
    fn style_runs_stay_in_order() {
        let layout = LineLayout::new(registry());
        let inlines = vec![
            Inline::Text {
                id: NodeId::UNASSIGNED,
                value: "body ".into(),
                attributes: Attributes::default(),
                position: None,
                span: None,
            },
            Inline::Code {
                id: NodeId::UNASSIGNED,
                value: "code".into(),
                attributes: Attributes::default(),
                position: None,
                span: None,
            },
        ];
        let lines = layout.layout(&inlines, body(), 200.0, Default::default());
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].runs.len(), 2);
        assert_eq!(line_text(&lines[0]), "body code");
    }

    /// A paragraph of only spaces produces no lines.
    #[test]
    fn spaces_only_paragraph_is_empty() {
        assert!(layout_body("   ", 200.0).is_empty());
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
