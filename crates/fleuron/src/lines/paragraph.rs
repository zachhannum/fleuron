//! What a paragraph is set in: the style over its runs, the style
//! over the line it opens on, and what it hangs into the margin.

use crate::content::{Metadata, NodeId};
use crate::style::{Color, FontVariantCaps, TextTransform};
use serde::Serialize;

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

/// What `::first-line` changes about the paragraph it opens.
///
/// A field is set where the pseudo-element's style differs from the
/// element's own, so what it changes reaches an emphasis or a link on
/// the opening line without replacing the rest of their style.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FirstLine {
    /// `font-size`, in points.
    pub size: Option<f32>,
    /// `letter-spacing`, in points.
    pub letter_spacing: Option<f32>,
    /// `font-variant-caps`.
    pub caps: Option<FontVariantCaps>,
    /// `text-transform`.
    pub transform: Option<TextTransform>,
    /// `color`.
    pub color: Option<Color>,
}

impl FirstLine {
    /// The style one run of the opening line is set in.
    pub fn over(&self, style: ParagraphStyle) -> ParagraphStyle {
        ParagraphStyle {
            size: self.size.unwrap_or(style.size),
            letter_spacing: self.letter_spacing.unwrap_or(style.letter_spacing),
            caps: self.caps.unwrap_or(style.caps),
            transform: self.transform.unwrap_or(style.transform),
            color: self.color.unwrap_or(style.color),
            ..style
        }
    }
}

/// What sets a paragraph's opening apart from the rest of it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Opening {
    /// What `::first-line` changes about the line the paragraph
    /// opens on.
    pub first_line: Option<FirstLine>,
    /// Bytes of the paragraph's text a drop cap already set. They
    /// are neither shaped nor broken here; the cap carries the range
    /// they cover.
    pub taken: usize,
}

/// What one pass of the breaker is told about the paragraph's
/// opening: the style it is set in, how far that reaches, and what a
/// drop cap already took off the front of it.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Lead {
    pub(super) style: Option<FirstLine>,
    /// Bytes of the shaped text the style covers. `None` on the pass
    /// that has no break to read it off yet, where it covers the
    /// paragraph.
    pub(super) extent: Option<usize>,
    /// Bytes of the source the paragraph starts past.
    pub(super) taken: usize,
}

impl Lead {
    /// How far into the shaped text the opening style reaches.
    pub(super) fn reach(self) -> usize {
        self.extent.unwrap_or(usize::MAX)
    }
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

/// How a paragraph is broken and filled. The defaults are ragged
/// right, no hyphenation, and no mark hanging past the measure.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LineBreakOptions {
    /// Whether `hyphens: auto` is in force.
    pub hyphenate: bool,
    /// Which language's syllables `hyphenate` breaks at.
    pub patterns: Patterns,
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

/// How much of a mark hangs past the measure it ends a line at, as a
/// fraction of its advance. The lighter the mark, the further it
/// goes: a full stop leaves a hole in the margin that the eye reads
/// as a ragged edge, a colon does not.
pub(super) fn hang_end(mark: char) -> f32 {
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
pub(super) fn hang_start(mark: char) -> f32 {
    match mark {
        '"' | '\'' | '\u{201c}' | '\u{2018}' | '\u{00ab}' => 0.4,
        '(' | '[' | '\u{2013}' | '\u{2014}' => 0.25,
        _ => 0.0,
    }
}

/// The syllable patterns `hyphens: auto` breaks words by.
///
/// The book's declared language chooses them; a book that declares
/// none breaks by English.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Patterns(pub(super) Option<hypher::Lang>);

impl Default for Patterns {
    fn default() -> Patterns {
        Patterns::ENGLISH
    }
}

impl Patterns {
    /// No patterns at all: every word stays whole.
    pub const NONE: Patterns = Patterns(None);

    /// The English patterns, which a book that declares no language
    /// breaks by.
    pub const ENGLISH: Patterns = Patterns(Some(hypher::Lang::English));

    /// The patterns a book breaks by: the ones its declared
    /// language names, English where it declares none, and `NONE`
    /// where there are no patterns for the language it declares.
    pub fn of(metadata: &Metadata) -> Patterns {
        match metadata.language() {
            Some(tag) => Patterns::of_tag(tag).unwrap_or(Patterns::NONE),
            None => Patterns::default(),
        }
    }

    /// The patterns a BCP 47 tag names, read from its primary
    /// subtag, so `fr-CA` is French. `None` where there are no
    /// patterns for that language.
    pub fn of_tag(tag: &str) -> Option<Patterns> {
        let primary = tag.split(['-', '_']).next().unwrap_or_default();
        let code: [u8; 2] = primary.as_bytes().try_into().ok()?;
        hypher::Lang::from_iso(code.map(|byte| byte.to_ascii_lowercase()))
            .map(|lang| Patterns(Some(lang)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lines::testing::{
        body, hyphenated, layout_body, layout_body_opts, line_text, registry, units_per_em,
    };

    use crate::lines::{LineBreakOptions, Patterns};

    /// A BCP 47 tag is read by its primary subtag, whatever the
    /// case and whatever follows it. A tag with no patterns behind it
    /// resolves to nothing rather than to English.
    #[test]
    fn patterns_come_from_the_primary_subtag() {
        let french = Patterns::of_tag("fr");
        assert!(french.is_some());
        assert_eq!(Patterns::of_tag("fr-CA"), french);
        assert_eq!(Patterns::of_tag("FR_ca"), french);
        assert_ne!(Patterns::of_tag("de"), french);

        assert_eq!(Patterns::of_tag("xx"), None);
        assert_eq!(Patterns::of_tag("haw"), None);
        assert_eq!(Patterns::of_tag(""), None);
    }

    /// A book that declares nothing breaks by English, and one that
    /// declares a language with no patterns breaks nowhere.
    #[test]
    fn a_book_without_a_language_breaks_by_english() {
        let declared = |tag: &str| {
            Patterns::of(&Metadata {
                extra: [("language".to_string(), tag.to_string())]
                    .into_iter()
                    .collect(),
                ..Default::default()
            })
        };
        assert_eq!(Patterns::of(&Metadata::default()), Patterns::default());
        assert_eq!(declared("en"), Patterns::default());
        assert_eq!(declared("  "), Patterns::default());
        assert_eq!(declared("xx"), Patterns::NONE);
    }

    /// Without patterns nothing breaks inside a word: the same lines
    /// hyphenation off gives.
    #[test]
    fn no_patterns_leaves_every_word_whole() {
        let text = "extraordinarily inconsiderate";
        let whole = LineBreakOptions {
            patterns: Patterns::NONE,
            ..hyphenated()
        };
        assert_eq!(
            layout_body_opts(text, 44.0, whole)
                .iter()
                .map(line_text)
                .collect::<Vec<_>>(),
            layout_body(text, 44.0)
                .iter()
                .map(line_text)
                .collect::<Vec<_>>(),
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
}
