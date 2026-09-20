//! The fixtures the line-layout tests are written against.

use crate::content::{Attributes, Inline, NodeId};
use crate::fonts::FontRegistry;

use super::{
    FirstLine, Inherited, InlineBox, InlineStyles, Line, LineBreakOptions, LineLayout, Measure,
    Opening, ParagraphStyle, Span,
};
use crate::style::Edges;

pub(super) fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| crate::fonts::bundled_registry().expect("bundled font parses"))
}

/// The body style the built-in sheet computes.
pub(super) fn body() -> ParagraphStyle {
    crate::style::defaults(&crate::content::Book::default(), registry())
        .root()
        .paragraph()
}

pub(super) fn layout_body(text: &str, measure_pt: f32) -> Vec<Line> {
    layout_body_opts(text, measure_pt, LineBreakOptions::default())
}

/// Justification on, everything else at its default.
pub(super) fn justified() -> LineBreakOptions {
    LineBreakOptions {
        justify: true,
        ..Default::default()
    }
}

/// Hyphenation on, everything else at its default.
pub(super) fn hyphenated() -> LineBreakOptions {
    LineBreakOptions {
        hyphenate: true,
        ..Default::default()
    }
}

pub(super) fn layout_body_opts(
    text: &str,
    measure_pt: f32,
    options: LineBreakOptions,
) -> Vec<Line> {
    let layout = LineLayout::new(registry());
    let inlines = vec![Inline::Text {
        id: NodeId::UNASSIGNED,
        value: text.to_string(),
        attributes: Attributes::default(),
        position: None,
        span: None,
    }];
    layout.layout(&inlines, &body(), measure_pt, options)
}

/// Text as one paragraph draws it, run by run.
pub(super) fn drawn(lines: &[Line]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            line.runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>()
        })
        .collect()
}

pub(super) fn units_per_em() -> u16 {
    registry().metrics(0).unwrap().units_per_em
}

/// One paragraph of one text run, as inlines.
pub(super) fn one_run(text: &str) -> Vec<Inline> {
    vec![Inline::Text {
        id: NodeId::UNASSIGNED,
        value: text.to_string(),
        attributes: Attributes::default(),
        position: None,
        span: None,
    }]
}

/// One paragraph with a hard break at every newline of `text`.
pub(super) fn with_breaks(text: &str) -> Vec<Inline> {
    let mut inlines = Vec::new();
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            inlines.push(Inline::Break {
                id: NodeId::UNASSIGNED,
                attributes: Attributes::default(),
                position: None,
                span: None,
            });
        }
        inlines.extend(one_run(line));
    }
    inlines
}

/// The same, laid out in the body style.
pub(super) fn layout_verse(text: &str, measure_pt: f32, options: LineBreakOptions) -> Vec<Line> {
    LineLayout::new(registry()).layout(&with_breaks(text), &body(), measure_pt, options)
}

/// One paragraph with `first_line` over the line it opens on,
/// ragged and unhyphenated.
pub(super) fn layout_first(
    layout: &LineLayout<'_>,
    measure_pt: f32,
    first_line: Option<FirstLine>,
) -> Vec<Line> {
    layout.layout_styled(
        &one_run(OPENING),
        &body(),
        &Inherited,
        &Measure::uniform(measure_pt),
        LineBreakOptions::default(),
        Opening {
            first_line,
            ..Opening::default()
        },
    )
}

/// One paragraph laid out under a style of the caller's, ragged
/// and unhyphenated.
pub(super) fn layout_style(text: &str, measure_pt: f32, style: &ParagraphStyle) -> Vec<Line> {
    let layout = LineLayout::new(registry());
    let inlines = vec![Inline::Text {
        id: NodeId::UNASSIGNED,
        value: text.to_string(),
        attributes: Attributes::default(),
        position: None,
        span: None,
    }];
    layout.layout(&inlines, style, measure_pt, LineBreakOptions::default())
}

/// A line's text, concatenated from its runs.
///
/// Read off the runs rather than sliced out of the paragraph: a
/// hyphenated line ends in a character the paragraph never had.
pub(super) fn line_text(line: &Line) -> String {
    line.runs.iter().map(|run| run.text.as_str()).collect()
}

/// A band of two spans of `width`, a gutter between them, and
/// one undivided band of the whole width under it.
pub(super) fn divided_band(width: f32, gutter: f32) -> Measure {
    Measure::new(
        vec![
            Span {
                origin: 0.0,
                width,
                ends_band: false,
            },
            Span::band(width + gutter, width),
        ],
        Span::band(0.0, width * 2.0 + gutter),
    )
}

/// One span's text, read off the runs set in it.
pub(super) fn span_text(line: &Line, index: usize) -> String {
    line.runs[line.spans[index].runs.clone()]
        .iter()
        .map(|run| run.text.as_str())
        .collect()
}

/// One span's width in points.
pub(super) fn span_width_pt(line: &Line, index: usize) -> f32 {
    line.spans[index].width as f32 / units_per_em() as f32 * body().size
}

/// Prose long enough to break several times at the measures the
/// first-line tests use, and lowercase throughout so small
/// capitals show in the drawn text.
pub(super) const OPENING: &str = "it was the best of times and the worst of them too, \
    and nobody in the whole of the parish could tell the one from the other";

/// A style tree that answers with a box for the inline elements it
/// was given one for, and with the block's own style for everything.
pub(super) struct Boxed(pub(super) Vec<(NodeId, InlineBox)>);

impl InlineStyles for Boxed {
    fn style(&self, _id: NodeId, block: &ParagraphStyle) -> ParagraphStyle {
        block.clone()
    }

    fn inline_box(&self, id: NodeId) -> Option<InlineBox> {
        self.0
            .iter()
            .find(|(node, _)| *node == id)
            .map(|(_, box_)| *box_)
    }
}

/// A box of `padding` points on all four edges, with no border.
pub(super) fn padded(padding: f32) -> InlineBox {
    InlineBox {
        padding: Edges::all(padding),
        border: Edges::all(0.0),
        cloned: false,
    }
}

/// A box of `width` points of border on all four edges, with no
/// padding.
pub(super) fn bordered(width: f32) -> InlineBox {
    InlineBox {
        padding: Edges::all(0.0),
        border: Edges::all(width),
        cloned: false,
    }
}

/// One code span, under `id`.
pub(super) fn code(id: NodeId, value: &str) -> Inline {
    Inline::Code {
        id,
        value: value.to_string(),
        attributes: Attributes::default(),
        position: None,
        span: None,
    }
}

/// One emphasis holding `children`, under `id`.
pub(super) fn emphasis(id: NodeId, children: Vec<Inline>) -> Inline {
    Inline::Emphasis {
        id,
        children,
        attributes: Attributes::default(),
        position: None,
        span: None,
    }
}

/// One paragraph broken to `measure_pt`, with the boxes `styles`
/// answers for.
pub(super) fn layout_boxed(inlines: &[Inline], measure_pt: f32, styles: &Boxed) -> Vec<Line> {
    LineLayout::new(registry()).layout_styled(
        inlines,
        &body(),
        styles,
        &Measure::uniform(measure_pt),
        LineBreakOptions::default(),
        Opening::default(),
    )
}
