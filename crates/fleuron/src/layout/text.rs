//! Shaped lines as paint ops, and the widths a placed line has.

use crate::fonts::{FontMetricsTable, Rule};
use crate::lines::{Line, ParagraphStyle, ShapedRun};
use crate::pages::{DrawItem, Glyph};

use super::Paginator;
use super::flow::{fade, shift};
use super::fragment::{Fragment, Piece};

impl Paginator<'_> {
    /// One fragment as paint ops. `x` is the leading edge its own `x`
    /// is measured from, and `top` is where the top of its box falls.
    pub(super) fn fragment_items(&self, fragment: &Fragment, x: f32, top: f32) -> Vec<DrawItem> {
        let (x, top) = (x + fragment.offset.0, top + fragment.offset.1);
        let mut items = Vec::new();
        for marker in fragment.markers.iter().flat_map(|markers| markers.iter()) {
            // A marker sits on the baseline of the line its item opens
            // with. Beside anything else, it sits at the top.
            let baseline = match &fragment.piece {
                Piece::Line { line, .. } => line.box_.baseline,
                _ => marker.line.box_.baseline,
            };
            items.append(&mut self.text_items(
                &marker.line,
                x + marker.x,
                top + baseline,
                fragment.layer,
            ));
        }
        items.append(&mut match &fragment.piece {
            Piece::Line { line, cap } => {
                let baseline = top + line.box_.baseline;
                let mut items = self.inline_items(line, x + fragment.x, baseline, fragment.layer);
                items.append(&mut self.text_items(line, x + fragment.x, baseline, fragment.layer));
                if let Some(cap) = cap {
                    items.append(&mut self.text_items(
                        &cap.line,
                        x + cap.x,
                        baseline + cap.drop,
                        fragment.layer,
                    ));
                }
                items
            }
            Piece::Image {
                width,
                height,
                asset,
            } => vec![DrawItem::Image {
                x: x + fragment.x,
                y: top,
                w: *width,
                h: *height,
                asset: *asset,
                alpha: 255,
                layer: fragment.layer,
            }],
            // A row's items were painted with the layers of the
            // table they belong to.
            Piece::Row(row) => {
                let mut items = row.items.clone();
                shift(&mut items, x + fragment.x, top);
                items
            }
            Piece::Blank | Piece::Anchor(_) => Vec::new(),
        });
        fade(&mut items, fragment.opacity);
        items
    }

    /// One string as a single shaped line: page furniture, and the
    /// ornaments and initial letters that are content but not prose.
    pub(super) fn line_of(&self, text: &str, style: &ParagraphStyle) -> Option<Line> {
        // One line holds no hard break: the newline a heading's break
        // reads as is a word space here.
        let spaced;
        let text = if text.contains('\n') {
            spaced = text.replace('\n', " ");
            spaced.as_str()
        } else {
            text
        };
        let runs = self.lines.shape(text, style)?;
        let box_ = self.lines.line_box(&runs, style);
        Some(Line::of(runs, box_))
    }

    /// Design units per em of a face, for the one conversion that
    /// takes shaped advances into points.
    fn upem(&self, font_id: u16) -> f32 {
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
        line.runs.iter().map(|run| self.run_width(run)).sum::<f32>()
            - line.overhang
            - line.protrusion
    }

    /// One run in points, the edges of the inline boxes that open and
    /// close on it included: what it takes across the line.
    fn run_width(&self, run: &crate::lines::ShapedRun) -> f32 {
        run.lead + run.advance as f32 / self.upem(run.font_id) * run.size + run.trail
    }

    /// One span's width in points. Runs of different sizes each
    /// convert against their own face: font units do not commute
    /// across sizes. A band hangs into the margins at its own two
    /// ends, never at a span boundary inside it.
    pub(super) fn span_width(&self, line: &Line, index: usize) -> f32 {
        let span = &line.spans[index];
        let ink = line.runs[span.runs.clone()]
            .iter()
            .map(|run| self.run_width(run))
            .sum::<f32>();
        let overhang = if index + 1 == line.spans.len() {
            line.overhang
        } else {
            0.0
        };
        let protrusion = if index == 0 { line.protrusion } else { 0.0 };
        ink - overhang - protrusion
    }

    /// One line as paint ops in `layer`: every run a
    /// `DrawItem::Text` at the baseline, glyphs placed at their
    /// accumulated advances, and each span of the line opened at its
    /// own origin.
    pub(super) fn text_items(
        &self,
        line: &Line,
        x: f32,
        baseline: f32,
        layer: i32,
    ) -> Vec<DrawItem> {
        let mut items = Vec::new();
        for span in line.spans.iter() {
            let mut x_cursor = x + span.offset;
            for run in &line.runs[span.runs.clone()] {
                let upem = self.upem(run.font_id);
                // The leading edges of the inline boxes that open on
                // this run come before its first glyph.
                x_cursor += run.lead;
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
                    pseudo_element: run.pseudo_element,
                    features: run.features.clone(),
                    color: run.color,
                    glyphs,
                    layer,
                });
                items.append(&mut self.decoration_items(
                    run,
                    x_cursor,
                    baseline,
                    glyph_x - x_cursor,
                    layer,
                ));
                x_cursor = glyph_x + run.trail;
            }
        }
        items
    }

    /// The rules `text-decoration` draws across one run, each a rect
    /// over the advance the run's glyphs took. They come after the
    /// glyphs, so a rule is painted over the letters it crosses.
    ///
    /// A run is what one line holds, so a decoration a line break
    /// splits is drawn on both lines, each at that line's own width.
    fn decoration_items(
        &self,
        run: &ShapedRun,
        x: f32,
        baseline: f32,
        width: f32,
        layer: i32,
    ) -> Vec<DrawItem> {
        let decoration = run.decoration;
        if !decoration.draws() || width <= 0.0 {
            return Vec::new();
        }
        let metrics = self.registry.metrics(run.font_id);
        let color = decoration.color.unwrap_or(run.color);
        let mut items = Vec::new();
        let mut rules = Vec::new();
        if decoration.line.over {
            rules.push(overline(metrics, run.size));
        }
        if decoration.line.through {
            rules.push(line_through(metrics, run.size));
        }
        if decoration.line.under {
            rules.push(underline(metrics, run.size));
        }
        for (top, thickness) in rules {
            let thickness = decoration.thickness.unwrap_or(thickness).max(0.0);
            if thickness <= 0.0 {
                continue;
            }
            for rule in 0..decoration.style.rules() {
                items.push(DrawItem::Rect {
                    x,
                    y: baseline + top + rule as f32 * thickness * 2.0,
                    w: width,
                    h: thickness,
                    color,
                    layer,
                });
            }
        }
        items
    }
}

/// Where one rule sits under a face's own baseline, and how thick it
/// is, both in points at `size`. The offset is positive downward,
/// which is the direction the page measures in.
fn placed(metrics: Option<FontMetricsTable>, rule: Option<Rule>, size: f32) -> Option<(f32, f32)> {
    let metrics = metrics?;
    let rule = rule?;
    let scale = size / metrics.units_per_em.max(1) as f32;
    Some((-rule.offset as f32 * scale, rule.thickness as f32 * scale))
}

/// A rule under the text, where the face's `post` table puts it.
fn underline(metrics: Option<FontMetricsTable>, size: f32) -> (f32, f32) {
    placed(metrics, metrics.and_then(|m| m.underline), size)
        .unwrap_or((size * FALLBACK_UNDERLINE, size * FALLBACK_THICKNESS))
}

/// A rule across the text, where the face's `OS/2` table puts it.
fn line_through(metrics: Option<FontMetricsTable>, size: f32) -> (f32, f32) {
    placed(metrics, metrics.and_then(|m| m.strikeout), size)
        .unwrap_or((-size * FALLBACK_STRIKEOUT, size * FALLBACK_THICKNESS))
}

/// A rule over the text. No table declares one, so it sits at the
/// ascent, as thick as the underline is.
fn overline(metrics: Option<FontMetricsTable>, size: f32) -> (f32, f32) {
    let thickness = underline(metrics, size).1;
    let top = match metrics {
        Some(metrics) => -(metrics.ascender as f32) * size / metrics.units_per_em.max(1) as f32,
        None => -size * FALLBACK_ASCENT,
    };
    (top, thickness)
}

/// Where the rules fall on a face that declares none, as fractions
/// of the em. A face without a `post` table is rare, and a book is
/// better served by a rule a little out of place than by none.
const FALLBACK_UNDERLINE: f32 = 0.1;
const FALLBACK_STRIKEOUT: f32 = 0.25;
const FALLBACK_THICKNESS: f32 = 0.05;
const FALLBACK_ASCENT: f32 = 0.8;

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
