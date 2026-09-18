//! The initial letter a paragraph opens with, sunk beside the lines
//! that follow it.

use unicode_general_category::get_general_category;

use crate::content::{Inline, NodeId, PseudoElement, SourceRange};
use crate::lines::{Line, ParagraphStyle};
use crate::style::ComputedStyle;

use super::Paginator;

impl Paginator<'_> {
    /// The initial letter one paragraph opens with, and the inlines
    /// left after it is taken out.
    ///
    /// The sink is what `initial-letter` asked for: the cap's own cap
    /// height spans that many lines, so its baseline lands on the
    /// last of them and its top on the first line's cap height.
    pub(super) fn drop_cap(
        &self,
        id: NodeId,
        computed: &ComputedStyle,
        inlines: &[Inline],
    ) -> Option<(Cap, usize)> {
        let cap_style = self.styles.first_letter(id)?;
        let sink = cap_style.initial_letter as usize;
        if sink < 2 {
            return None;
        }
        let initial = take_initial(inlines)?;
        let body = computed.paragraph();
        let cap_metrics = self.registry.metrics(cap_style.font_id)?;
        let cap_units = cap_height(cap_metrics);
        if cap_units <= 0.0 {
            return None;
        }
        let body_metrics = self.registry.metrics(body.font_id)?;
        let body_cap = cap_height(body_metrics) / body_metrics.units_per_em as f32 * body.size;
        let sunk = (sink - 1) as f32 * self.lines.strut(body).height() + body_cap;
        let style = ParagraphStyle {
            size: sunk * cap_metrics.units_per_em as f32 / cap_units,
            ..cap_style.paragraph()
        };
        let mut line = self.line_of(&initial.text, style)?;
        // The cap stands for the letter it was taken from, so a
        // cursor on the manuscript's first word lands on it.
        if let Some(run) = line.runs.first_mut() {
            run.origin = Some(initial.origin);
            run.pseudo_element = Some(id.pseudo(PseudoElement::FirstLetter));
        }
        // A word space of the body text separates the cap from the
        // lines it is sunk into.
        let gutter = self
            .registry
            .char_glyph(body.font_id, ' ')
            .and_then(|glyph| self.registry.advance_width(body.font_id, glyph))
            .unwrap_or(0) as f32
            / body_metrics.units_per_em as f32
            * body.size;
        Some((
            Cap {
                reserved: self.line_width(&line) + gutter,
                line,
                lines: sink,
            },
            initial.taken,
        ))
    }
}

/// The initial letter of one paragraph, sized and shaped, with the
/// text it was taken out of.
#[derive(Debug, Clone)]
pub(super) struct Cap {
    /// The letter, shaped at the size the sink works out to.
    pub(super) line: Line,
    /// Width the lines beside it give up, the gutter included.
    pub(super) reserved: f32,
    /// Lines it is sunk over.
    pub(super) lines: usize,
}

/// The text a drop cap is set from, where it was written, and how far
/// into the paragraph the rest of the prose starts.
struct Initial {
    /// The first letter with the punctuation on either side of it.
    text: String,
    /// The node the cap starts in, and the bytes of it the cap holds:
    /// its text and whatever space stood before it.
    origin: SourceRange,
    /// Bytes of the paragraph's text the cap holds.
    taken: usize,
}

/// The first letter of a run of inlines and the punctuation next to
/// it, wherever the markup has put them: a paragraph opening in
/// italic still opens with a letter.
fn take_initial(inlines: &[Inline]) -> Option<Initial> {
    fn texts<'a>(inlines: &'a [Inline], out: &mut Vec<(Option<NodeId>, &'a str)>) {
        for inline in inlines {
            match inline {
                Inline::Text { id, value, .. } | Inline::Code { id, value, .. } => {
                    out.push((Some(*id), value))
                }
                Inline::Break { .. } => out.push((None, "\n")),
                Inline::Emphasis { children, .. }
                | Inline::Strong { children, .. }
                | Inline::Link { children, .. }
                | Inline::Span { children, .. } => texts(children, out),
            }
        }
    }
    let mut pieces = Vec::new();
    texts(inlines, &mut pieces);

    let mut text = String::new();
    let mut origin: Option<SourceRange> = None;
    let mut lettered = false;
    let mut taken = 0;
    let mut before = 0;
    'pieces: for (node, value) in pieces {
        for (at, character) in value.char_indices() {
            if text.is_empty() && character.is_whitespace() {
                continue;
            }
            let joins = match Kind::of(character) {
                Kind::Punctuation => true,
                Kind::Letter => !std::mem::replace(&mut lettered, true),
                Kind::Other => false,
            };
            let Some(node) = node.filter(|_| joins) else {
                break 'pieces;
            };
            let end = (at + character.len_utf8()) as u32;
            let origin = origin.get_or_insert(SourceRange { node, range: 0..0 });
            if origin.node == node {
                origin.range.end = end;
            }
            text.push(character);
            taken = before + end as usize;
        }
        before += value.len();
    }
    Some(Initial {
        text,
        origin: origin.filter(|_| lettered)?,
        taken,
    })
}

/// What a character is to `::first-letter`.
enum Kind {
    /// Opening, closing, quotation, and other punctuation. Dashes and
    /// connectors are not in it.
    Punctuation,
    /// A letter, a number, or a symbol.
    Letter,
    Other,
}

impl Kind {
    fn of(character: char) -> Kind {
        match get_general_category(character).abbreviation().as_bytes() {
            [b'P', b's' | b'e' | b'i' | b'f' | b'o'] => Kind::Punctuation,
            [b'L' | b'N' | b'S', _] => Kind::Letter,
            _ => Kind::Other,
        }
    }
}

/// A face's cap height, falling back to its ascender when the file
/// declares none.
fn cap_height(metrics: crate::fonts::FontMetricsTable) -> f32 {
    if metrics.cap_height > 0 {
        metrics.cap_height as f32
    } else {
        metrics.ascender as f32
    }
}

#[cfg(test)]
mod tests {
    use crate::content::{Attributes, Inline, NodeId};
    use crate::layout::testing::{
        ContentLine, body_size, content_lines, master, origin_of, paginate_styled, paragraph,
        registry, section, small_caps_lines, ua, under_h3,
    };
    use crate::pages::DrawItem;
    use crate::style::Situation;

    use super::take_initial;

    fn words(id: u32, value: &str) -> Inline {
        Inline::Text {
            id: NodeId::new(id),
            value: value.into(),
            attributes: Attributes::default(),
            position: None,
            span: None,
        }
    }

    /// Acceptance: punctuation before the first letter joins the drop
    /// cap, and the first line opens after the letter.
    #[test]
    fn punctuation_before_the_first_letter_joins_the_drop_cap() {
        let prose = format!(
            "\u{201C}Sir,\u{201D} said he, {}",
            "my father had a small estate in nottinghamshire ".repeat(12)
        );
        let pages = paginate_styled(
            "p::first-letter { initial-letter: 3 }",
            vec![section(vec![paragraph(&prose)])],
        );
        let lines = content_lines(&pages[0]);
        let cap: Vec<&str> = lines
            .iter()
            .flat_map(|(_, runs)| runs)
            .filter(|(_, size, _)| *size > body_size())
            .map(|(_, _, text)| *text)
            .collect();
        assert_eq!(cap, ["\u{201C}S"]);
        assert!(
            lines[0].1[0].2.starts_with("ir,"),
            "the first line does not open after the letter: {:?}",
            lines[0].1,
        );
    }

    /// Acceptance: punctuation right after the first letter joins the
    /// drop cap.
    #[test]
    fn punctuation_right_after_the_first_letter_joins_the_drop_cap() {
        let initial = take_initial(&[words(1, "A. Gulliver")]).expect("a letter");
        assert_eq!(initial.text, "A.");
        assert_eq!(initial.taken, 2);
    }

    /// Acceptance: the drop cap takes punctuation and its letter across
    /// an inline boundary. Its origin is where the punctuation was
    /// written.
    #[test]
    fn the_drop_cap_takes_punctuation_and_its_letter_across_an_inline_boundary() {
        let inlines = [
            words(1, "\u{201C}"),
            Inline::Emphasis {
                id: NodeId::new(2),
                children: vec![words(3, "Sir, he said")],
                attributes: Attributes::default(),
                position: None,
                span: None,
            },
        ];
        let initial = take_initial(&inlines).expect("a letter");
        assert_eq!(initial.text, "\u{201C}S");
        assert_eq!(initial.origin.node, NodeId::new(1));
        assert_eq!(initial.origin.range, 0..3);
        assert_eq!(initial.taken, 4);
    }

    /// Acceptance: a paragraph with punctuation and no letter gets no
    /// drop cap.
    #[test]
    fn a_paragraph_with_punctuation_and_no_letter_gets_no_drop_cap() {
        assert!(take_initial(&[words(1, "\u{201C} \u{2026}")]).is_none());
        assert!(take_initial(&[words(1, "\u{2014}yes")]).is_none());
        assert!(take_initial(&[words(1, "  ")]).is_none());
    }

    /// Acceptance: a drop cap and an indent do not stack. The cap's
    /// reserved measure is what offsets its line; the indent the
    /// sheet asks for adds nothing on top of it.
    #[test]
    fn a_drop_cap_absorbs_the_indent() {
        let prose = "my father had a small estate in nottinghamshire ".repeat(12);
        let capped = |css: &str| {
            let pages = paginate_styled(css, vec![section(vec![paragraph(&prose)])]);
            let page = pages.first().expect("the paragraph set no pages");
            let (left, _) = origin_of(page);
            content_lines(page)
                .iter()
                .map(|(_, runs)| runs[0].0 - left)
                .collect::<Vec<f32>>()
        };
        let plain = capped("p::first-letter { initial-letter: 3 }");
        let indented = capped("p::first-letter { initial-letter: 3 } p { text-indent: 18pt }");
        assert!(plain.len() > 4, "not enough lines to sink into");
        assert_eq!(
            plain, indented,
            "the indent moved a line the cap had already displaced",
        );
    }

    /// Acceptance: a drop cap and a small-capitals first line set
    /// together. Both fall on the initial and `::first-letter` wins
    /// it: the cap is the one run set larger and it is not small
    /// capitals, while the line beside it is.
    #[test]
    fn a_drop_cap_takes_the_initial_from_the_first_line() {
        let prose = "my father had a small estate in nottinghamshire ".repeat(6);
        let pages = paginate_styled(
            "h3 + p::first-letter { initial-letter: 3 }
             h3 + p::first-line { font-variant-caps: small-caps }",
            under_h3(&prose),
        );
        let lines = small_caps_lines(&pages[0]);
        // The cap is the one run of the paragraph set larger than the
        // body; the heading above it is larger too.
        let cap = pages[0]
            .items
            .iter()
            .filter_map(|item| match item {
                DrawItem::Text {
                    size,
                    text,
                    features,
                    ..
                } if *size > 1.5 * body_size() && text != "A Voyage" => {
                    Some((text.as_str(), features.small_caps))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            cap,
            vec![("m", false)],
            "the initial is not one run of its own, out of the first line's small capitals",
        );

        let opening: Vec<(&str, bool)> = lines[1]
            .1
            .iter()
            .copied()
            .filter(|(text, _)| *text != "m")
            .collect();
        assert!(
            !opening.is_empty() && opening.iter().all(|(_, small)| *small),
            "the line beside the cap is not small capitals: {opening:?}",
        );
    }

    /// Acceptance: a three-line drop cap sits on the third baseline,
    /// its top on the first line's cap height, and the lines beside
    /// it are set to the measure it left them.
    #[test]
    fn a_drop_cap_aligns_to_the_third_baseline_and_shortens_three_lines() {
        let pages = paginate_styled(
            "p::first-letter { initial-letter: 3 }",
            vec![section(vec![paragraph(
                &"my father had a small estate in nottinghamshire ".repeat(12),
            )])],
        );
        let lines = content_lines(&pages[0]);
        assert!(lines.len() > 4, "not enough lines to sink into");

        let body = ua().root();
        let (left, _) = origin_of(&pages[0]);
        // The cap is the one run set larger than the body.
        let (cap_index, cap) = lines
            .iter()
            .enumerate()
            .find(|(_, (_, runs))| runs[0].1 > body.font_size)
            .map(|(index, (baseline, runs))| (index, (*baseline, runs[0])))
            .expect("a drop cap paints");
        let (cap_baseline, (cap_x, cap_size, _)) = cap;

        // Its baseline is the third line's, and it starts at the
        // content box's own leading edge.
        let prose: Vec<&ContentLine<'_>> = lines
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != cap_index)
            .map(|(_, line)| line)
            .collect();
        assert!(
            (cap_baseline - prose[2].0).abs() < 1e-3,
            "the cap is not on the third baseline"
        );
        assert!(
            (cap_x - left).abs() < 1e-3,
            "the cap is not at the leading edge"
        );

        // Its top sits on the first line's cap height.
        let cap_height = |font: u16, size: f32| {
            let metrics = registry().metrics(font).unwrap();
            metrics.cap_height as f32 / metrics.units_per_em as f32 * size
        };
        let top = cap_baseline - cap_height(body.font_id, cap_size);
        assert!(
            (top - (prose[0].0 - cap_height(body.font_id, body.font_size))).abs() < 1e-2,
            "the cap's top is not the first line's cap height",
        );

        // The three lines beside it start past the cap and are set to
        // the measure it left them; the fourth is back at the full one.
        let sunk = prose[0].1[0].0;
        assert!(sunk > left, "the first line was not moved aside");
        for line in prose.iter().take(3) {
            assert!(
                (line.1[0].0 - sunk).abs() < 1e-3,
                "a sunk line is not set to the shortened measure",
            );
        }
        assert!(
            (prose[3].1[0].0 - left).abs() < 1e-3,
            "the fourth line did not go back to the full measure",
        );
        for page in &pages {
            for item in &page.items {
                let DrawItem::Text { glyphs, .. } = item else {
                    continue;
                };
                for glyph in glyphs {
                    assert!(
                        glyph.x <= left + master(Situation::Body(page.side)).geometry.measure(),
                        "a sunk line ran past the measure",
                    );
                }
            }
        }
    }
}
