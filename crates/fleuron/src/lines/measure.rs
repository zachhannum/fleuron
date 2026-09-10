//! The bands a line is set in: where each one starts and how wide
//! it is.

/// One place a line may start and how wide it may run.
///
/// A band of the page is set in one span or in several, and a band
/// set in several is still one line: the text crosses from span to
/// span in reading order, and only the last of them ends where a
/// line ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    /// Points from the block's leading edge the span starts at.
    pub origin: f32,
    /// Points the span runs.
    pub width: f32,
    /// Whether the band ends here.
    pub ends_band: bool,
}

impl Span {
    /// A span that is a band on its own.
    pub fn band(origin: f32, width: f32) -> Span {
        Span {
            origin,
            width,
            ends_band: true,
        }
    }

    /// A band `width` wide ending where a band of `end` points ends.
    /// A first-line indent and a drop cap beside the line both leave
    /// one: the line is shortened at its start.
    pub fn ending(end: f32, width: f32) -> Span {
        Span::band(end - width, width)
    }
}

/// The spans a paragraph breaks to, in reading order.
///
/// One entry per span rather than per line, so a band set in several
/// spans is several entries. The listed spans run out and `rest`
/// answers for every one past them, so a profile covers a paragraph
/// before anything has broken it and found its length. `rest` is a
/// band of its own: a band that never ended would take the rest of
/// the paragraph.
#[derive(Debug, Clone, PartialEq)]
pub struct Measure {
    pub(super) leading: Vec<Span>,
    pub(super) rest: Span,
}

impl Measure {
    /// One band of one width for every line.
    pub fn uniform(points: f32) -> Measure {
        Measure {
            leading: Vec::new(),
            rest: Span::band(0.0, points),
        }
    }

    /// The spans a paragraph opens with, and the band every line
    /// past them is set in.
    pub fn new(leading: Vec<Span>, rest: Span) -> Measure {
        Measure {
            leading,
            rest: Span::band(rest.origin, rest.width),
        }
    }

    /// The band every span past the listed ones is set in.
    pub fn rest(&self) -> Span {
        self.rest
    }

    /// The span at `index`.
    pub fn at(&self, index: usize) -> Span {
        self.leading.get(index).copied().unwrap_or(self.rest)
    }

    /// Spans past which every span is the same one. Two paths that
    /// have reached here differ in nothing the rest of the paragraph
    /// can see.
    pub(super) fn settled(&self) -> usize {
        self.leading.len()
    }
}

impl From<f32> for Measure {
    fn from(points: f32) -> Measure {
        Measure::uniform(points)
    }
}

#[cfg(test)]
mod tests {

    use crate::content::{Attributes, Inline, NodeId};
    use crate::lines::testing::{
        OPENING, body, divided_band, justified, line_text, one_run, registry, span_text,
        span_width_pt, units_per_em,
    };

    use crate::lines::{Line, LineBreakOptions, LineLayout, Measure, Span};

    /// A band set in two spans sets text in both, in reading order:
    /// the paragraph crosses from the first to the second and comes
    /// back off them in the order it was written.
    #[test]
    fn a_band_of_two_spans_sets_text_in_both() {
        let layout = LineLayout::new(registry());
        let lines = layout.layout(
            &one_run(OPENING),
            body(),
            divided_band(80.0, 20.0),
            LineBreakOptions::default(),
        );
        let first = &lines[0];
        assert_eq!(first.spans.len(), 2, "the band was set in one span");
        assert!(
            !span_text(first, 0).is_empty() && !span_text(first, 1).is_empty(),
            "a span of the band holds no text: {first:?}"
        );
        assert_eq!(
            OPENING
                .replace(' ', "")
                .find(&span_text(first, 1).replace(' ', "")),
            Some(span_text(first, 0).replace(' ', "").len()),
            "the second span does not carry on from the first"
        );
        // The second span opens where the profile put it, which is
        // past the gutter rather than at the line's own edge.
        assert_eq!(first.spans[1].offset, 100.0);
        assert!(
            lines[1..].iter().all(|line| line.spans.len() == 1),
            "a band under the divided one was set in more than one span"
        );
    }

    /// Justification flushes an interior span at both edges: the
    /// text in the first span of a band fills it, and only the last
    /// line of a paragraph is left short.
    #[test]
    fn justification_flushes_an_interior_span() {
        let layout = LineLayout::new(registry());
        let lines = layout.layout(
            &one_run(OPENING),
            body(),
            divided_band(80.0, 20.0),
            justified(),
        );
        assert!(
            (span_width_pt(&lines[0], 0) - 80.0).abs() < 0.01,
            "the first span of the band is {}pt of 80pt",
            span_width_pt(&lines[0], 0),
        );
    }

    /// A drop cap shortens the lines beside it: the first few break
    /// to a narrower measure, and the rest go back to the full one.
    #[test]
    fn a_shortened_measure_only_holds_for_the_lines_it_names() {
        let text = "one two three four five six seven eight nine ten eleven twelve";
        let measure = Measure::new(vec![Span::band(80.0, 40.0); 2], Span::band(0.0, 120.0));
        let layout = LineLayout::new(registry());
        let inlines = vec![Inline::Text {
            id: NodeId::UNASSIGNED,
            value: text.to_string(),
            attributes: Attributes::default(),
            position: None,
            span: None,
        }];
        let lines = layout.layout(&inlines, body(), measure.clone(), Default::default());
        assert!(lines.len() > 3, "expected several lines: {lines:?}");
        let width_pt = |line: &Line| line.width as f32 / units_per_em() as f32 * body().size;
        for (index, line) in lines.iter().enumerate() {
            let allowed = measure.at(index).width;
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
            width_pt(&lines[2]) > measure.at(0).width,
            "the measure never widened"
        );
        let uniform = layout.layout(&inlines, body(), 120.0, Default::default());
        assert!(uniform.len() < lines.len());
        assert_eq!(
            lines.iter().map(line_text).collect::<Vec<_>>().join(" "),
            text,
        );
    }
}
