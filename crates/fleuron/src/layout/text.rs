//! Shaped lines as paint ops, and the widths a placed line has.

use crate::lines::{Line, ParagraphStyle};
use crate::pages::{DrawItem, Glyph};

use super::Paginator;

impl Paginator<'_> {
    /// One string as a single shaped line: page furniture, and the
    /// ornaments and initial letters that are content but not prose.
    pub(super) fn line_of(&self, text: &str, style: ParagraphStyle) -> Option<Line> {
        let runs = self.lines.shape(text, style)?;
        let box_ = self.lines.line_box(&runs, style);
        Some(Line::of(runs, box_))
    }

    /// Design units per em of a face, for the one conversion that
    /// takes shaped advances into points.
    pub(super) fn upem(&self, font_id: u16) -> f32 {
        self.registry
            .metrics(font_id)
            .map(|m| m.units_per_em as f32)
            .unwrap_or(1000.0)
    }

    /// A line's width in points. Runs of different sizes each convert
    /// against their own face: font units do not commute across sizes.
    /// What hangs into a margin is not part of the width, which is the
    /// point of hanging it.
    pub(super) fn line_width(&self, line: &Line) -> f32 {
        line.runs
            .iter()
            .map(|run| run.advance as f32 / self.upem(run.font_id) * run.size)
            .sum::<f32>()
            - line.overhang
            - line.protrusion
    }

    /// One span's width in points. Runs of different sizes each
    /// convert against their own face: font units do not commute
    /// across sizes. A band hangs into the margins at its own two
    /// ends, never at a span boundary inside it.
    pub(super) fn span_width(&self, line: &Line, index: usize) -> f32 {
        let span = &line.spans[index];
        let ink = line.runs[span.runs.clone()]
            .iter()
            .map(|run| run.advance as f32 / self.upem(run.font_id) * run.size)
            .sum::<f32>();
        let overhang = if index + 1 == line.spans.len() {
            line.overhang
        } else {
            0.0
        };
        let protrusion = if index == 0 { line.protrusion } else { 0.0 };
        ink - overhang - protrusion
    }

    /// One line as paint ops: every run a `DrawItem::Text` at the
    /// baseline, glyphs placed at their accumulated advances, and
    /// each span of the line opened at its own origin.
    pub(super) fn text_items(&self, line: &Line, x: f32, baseline: f32) -> Vec<DrawItem> {
        let mut items = Vec::new();
        for span in line.spans.iter() {
            let mut x_cursor = x + span.offset;
            for run in &line.runs[span.runs.clone()] {
                let upem = self.upem(run.font_id);
                let mut glyphs = Vec::with_capacity(run.glyphs.len());
                let mut glyph_x = x_cursor;
                for (shaped, range) in run.glyphs.iter().zip(run.glyph_ranges()) {
                    glyphs.push(Glyph {
                        id: shaped.id,
                        x: glyph_x,
                        range,
                    });
                    glyph_x += shaped.x_advance as f32 / upem * run.size;
                }
                items.push(DrawItem::Text {
                    x: x_cursor,
                    y: baseline,
                    font_id: run.font_id,
                    size: run.size,
                    text: run.text.clone(),
                    source: run.source.clone(),
                    source_map: run.source_map.clone(),
                    origin: run.origin.clone(),
                    features: run.features,
                    color: run.color,
                    glyphs,
                });
                x_cursor = glyph_x;
            }
        }
        items
    }
}

#[cfg(test)]
mod tests {
    use crate::layout::testing::{
        heading, paginate, paginate_styled, paragraph, registry, section,
    };
    use crate::pages::DrawItem;

    /// A run's `text` is what was drawn, and its `source` is what was
    /// written. The two painters read different fields for different
    /// reasons: one draws characters and hands `text` to a browser,
    /// the other maps glyphs back and reads `source`. A run with
    /// only the manuscript in it would have the preview set a
    /// chapter title in the case the export does not.
    #[test]
    fn a_run_says_both_what_was_drawn_and_what_was_written() {
        let pages = paginate_styled(
            "h1 { text-transform: uppercase }",
            vec![section(vec![heading("A Voyage to Lilliput")])],
        );
        let title = pages[0]
            .items
            .iter()
            .find_map(|item| match item {
                DrawItem::Text {
                    text,
                    source,
                    source_map,
                    glyphs,
                    ..
                } => Some((text, source, source_map, glyphs)),
                _ => None,
            })
            .expect("the chapter set its title");
        let (text, source, source_map, glyphs) = title;
        assert_eq!(
            text, "A VOYAGE TO LILLIPUT",
            "the title was not drawn in capitals"
        );
        assert_eq!(
            source, "A Voyage to Lilliput",
            "the title lost its manuscript"
        );
        assert_eq!(
            source_map.len(),
            text.len() + 1,
            "the map does not cover every byte boundary of what was drawn",
        );
        // Every glyph's range indexes what was drawn, and taken
        // through the map it reads back the manuscript entire.
        let read: String = glyphs
            .iter()
            .map(|glyph| {
                let from = source_map[glyph.range.start as usize] as usize;
                let to = source_map[glyph.range.end as usize] as usize;
                &source[from..to]
            })
            .collect();
        assert_eq!(read, *source, "the glyphs do not read the manuscript back");

        // A book nothing transformed says nothing twice over.
        let plain = paginate(vec![section(vec![heading("A Voyage to Lilliput")])]);
        assert!(
            plain[0].items.iter().all(|item| !matches!(
                item,
                DrawItem::Text { source, .. } if !source.is_empty()
            )),
            "an untransformed run has a source of its own",
        );
    }

    /// Glyph positions accumulate advances: the first glyph paints at
    /// the item origin and each subsequent glyph sits one shaped
    /// advance past its predecessor, in points.
    #[test]
    fn glyphs_are_placed_at_their_advances() {
        let pages = paginate(vec![section(vec![paragraph("hello")])]);
        let DrawItem::Text {
            x, glyphs, size, ..
        } = &pages[0].items[0]
        else {
            panic!("expected text");
        };
        assert_eq!(glyphs.len(), 5);
        assert!((glyphs[0].x - *x).abs() < 1e-4);
        let shaped = registry().shape(0, "hello").unwrap();
        let upem = registry().metrics(0).unwrap().units_per_em as f32;
        let mut expected_x = *x;
        for (glyph, shaped_glyph) in glyphs.iter().zip(&shaped) {
            assert!(
                (glyph.x - expected_x).abs() < 1e-3,
                "glyph at {}, expected {expected_x}",
                glyph.x
            );
            expected_x += shaped_glyph.x_advance as f32 / upem * size;
        }
    }
}
