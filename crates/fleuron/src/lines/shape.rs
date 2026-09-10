//! Text to glyphs: a span at a time, on the face the cascade named.

use std::ops::Range;

use crate::fonts::{Features, ShapedGlyph};

use super::LineLayout;
use super::flatten::{FlatParagraph, StyleSpan};
use super::line::{ShapedRun, cut_runs};
use super::paragraph::ParagraphStyle;

impl LineLayout<'_> {
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

    /// Shapes each span. A glyph's cluster is an offset into the
    /// span, which indexes the paragraph text once offset by the
    /// span's start. A span set at another size measures in its own
    /// font units, so `scale` takes it into the paragraph's.
    ///
    /// Tracking is added here, to the last glyph of every cluster, so
    /// every pass downstream measures it without being told about it.
    pub(super) fn shape_spans(
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
    pub(super) fn tracking_units(&self, span: &StyleSpan) -> i64 {
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
    pub(super) fn scale(&self, font_id: u16, size: f32, style: ParagraphStyle, upem: f32) -> f32 {
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

    /// Draws the hyphen a break inside a word leaves behind.
    ///
    /// The break was charged for it when it was chosen, so it is
    /// drawn in the same face the charge was read from: a hyphen
    /// taken from one face and paid for out of another is a line
    /// that measures one width and paints a different one.
    pub(super) fn hyphenate(&self, runs: &mut Vec<ShapedRun>, style: ParagraphStyle) {
        let Some(id) = self.registry.char_glyph(style.font_id, '-') else {
            return;
        };
        let advance = self
            .registry
            .advance_width(style.font_id, id)
            .unwrap_or_default() as u32;
        let ending = runs
            .last()
            .map(|run| run.text_start + run.text.len() as u32)
            .unwrap_or_default();
        // The hyphen takes the last run's colour, because colour
        // costs no width, and the paragraph's face, because that is
        // where its width was charged.
        let color = runs.last().map_or(style.color, |run| run.color);
        match runs.last_mut() {
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
            _ => runs.push(ShapedRun {
                font_id: style.font_id,
                size: style.size,
                text: "-".to_string(),
                source: String::new(),
                source_map: Vec::new(),
                text_start: ending,
                origin: None,
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
    }

    pub(super) fn hyphen_advance(&self, style: ParagraphStyle) -> u32 {
        self.registry
            .char_glyph(style.font_id, '-')
            .and_then(|g| self.registry.advance_width(style.font_id, g))
            .unwrap_or(0) as u32
    }
}

/// One shaped span, its glyph clusters still relative to the span's
/// own text.
pub(super) struct ShapedSpan {
    /// Byte range of the span in the paragraph text.
    pub(super) range: Range<usize>,
    /// What one of this span's font units is worth in the
    /// paragraph's.
    pub(super) scale: f32,
    /// Tracking charged after each of its clusters, in its own font
    /// units.
    pub(super) track: i64,
    /// The same in the paragraph's font units.
    pub(super) tracking: f32,
    pub(super) glyphs: Vec<ShapedGlyph>,
}

impl ShapedSpan {
    /// The glyphs whose clusters fall in `[start, end)`, clusters
    /// rebased to the paragraph text.
    pub(super) fn glyphs_in(&self, start: usize, end: usize) -> Vec<ShapedGlyph> {
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

#[cfg(test)]
mod tests {
    use crate::content::{Attributes, Inline, NodeId};
    use crate::lines::flatten::SMALL_CAPS_RATIO;
    use crate::lines::testing::{
        body, hyphenated, layout_body_opts, layout_style, line_text, registry, units_per_em,
    };
    use crate::lines::{LineBreakOptions, LineLayout, Measure, Opening, ParagraphStyle};
    use crate::style::FontVariantCaps;

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
                            attributes: Attributes::default(),
                            position: None,
                            span: None,
                        },
                        Inline::Emphasis {
                            id: NodeId::UNASSIGNED,
                            children: vec![
                                Inline::Text {
                                    id: NodeId::UNASSIGNED,
                                    value: "never ".into(),
                                    attributes: Attributes::default(),
                                    position: None,
                                    span: None,
                                },
                                Inline::Strong {
                                    id: NodeId::UNASSIGNED,
                                    children: vec![Inline::Text {
                                        id: NodeId::UNASSIGNED,
                                        value: "again".into(),
                                        attributes: Attributes::default(),
                                        position: None,
                                        span: None,
                                    }],
                                    attributes: Attributes::default(),
                                    position: None,
                                    span: None,
                                },
                            ],
                            attributes: Attributes::default(),
                            position: None,
                            span: None,
                        },
                        Inline::Text {
                            id: NodeId::UNASSIGNED,
                            value: " to her.".into(),
                            attributes: Attributes::default(),
                            position: None,
                            span: None,
                        },
                    ],
                    attributes: Attributes::default(),
                    position: None,
                    span: None,
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
            &Measure::uniform(400.0),
            Default::default(),
            Opening::default(),
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
            attributes: Attributes::default(),
            position: None,
            span: None,
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
}
