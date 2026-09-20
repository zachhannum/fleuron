//! Where a line may end: UAX #14, word boundaries from UAX #29, and
//! the syllable breaks hyphenation adds.
//!
//! Preformatted text keeps only the breaks UAX #14 makes mandatory,
//! which are the newlines the author wrote.

use unicode_linebreak::{BreakOpportunity, linebreaks};

use super::LineLayout;
use super::justify::{skip_spaces, trailing_spaces};
use super::line::Widths;
use super::paragraph::{HangingPunctuation, LineBreakOptions, Patterns, hang_end, hang_start};

/// A candidate line end: the exclusive byte offset where a line may
/// end, plus whether the break falls inside a word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Opportunity {
    /// Exclusive end of the line's text.
    end: usize,
    /// True when this break sits inside a word and the line must be
    /// charged for a hyphen glyph.
    hyphen: bool,
    /// True when the line has to end here.
    forced: bool,
}

/// One place a line may end, with everything the breaker measures it
/// by. Widths are read out of the prefix tables at these offsets.
#[derive(Debug, Clone, Copy)]
pub(super) struct Break {
    /// Where the line's paintable text ends: `end` less the spaces
    /// the break swallows.
    pub(super) content_end: usize,
    /// Where the line after this one starts.
    pub(super) next: usize,
    /// Whether taking this break puts a hyphen on the line.
    pub(super) hyphen: bool,
    /// Whether the line has to end here: at a hard break, and at the
    /// end of the paragraph.
    pub(super) forced: bool,
    /// Whether the line after this break would open with a dash.
    pub(super) dash: bool,
    /// Font units of the last glyph that may hang past the measure.
    pub(super) hang_end: f32,
    /// Font units the first glyph of the following line may hang
    /// before the measure.
    pub(super) hang_start: f32,
}

impl LineLayout<'_> {
    /// Break opportunities for the paragraph: UAX #14 always, UAX #29
    /// word boundaries to bound hyphenation, `hypher` for syllables
    /// when enabled.
    fn opportunities(
        &self,
        text: &str,
        widths: &Widths,
        options: LineBreakOptions,
    ) -> Vec<Opportunity> {
        let allowed = |kind: &BreakOpportunity| match options.preformatted {
            true => *kind == BreakOpportunity::Mandatory,
            false => matches!(
                kind,
                BreakOpportunity::Allowed | BreakOpportunity::Mandatory
            ),
        };
        let mut opportunities: Vec<Opportunity> = linebreaks(text)
            .filter(|(_, kind)| allowed(kind))
            .map(|(index, kind)| Opportunity {
                end: index,
                hyphen: false,
                forced: kind == BreakOpportunity::Mandatory,
            })
            .collect();
        if options.hyphenate {
            self.add_hyphenation(text, widths, options.patterns, &mut opportunities);
        }
        // A hyphen and a space at the same offset are the same
        // break, and the one that costs nothing wins it.
        opportunities.sort_unstable_by_key(|o| (o.end, o.hyphen));
        opportunities.dedup_by_key(|o| o.end);
        opportunities
    }

    fn add_hyphenation(
        &self,
        text: &str,
        widths: &Widths,
        patterns: Patterns,
        opportunities: &mut Vec<Opportunity>,
    ) {
        let Patterns(Some(lang)) = patterns else {
            return;
        };
        let mut start = 0usize;
        for boundary in self.segmenter.segment_str(text) {
            let word = &text[start..boundary];
            // Letters of any script, since the patterns are of any
            // language: German words carry umlauts and French ones
            // accents, and a word held to ASCII would never break.
            let is_word = !word.is_empty()
                && word
                    .chars()
                    .all(|c| c.is_alphabetic() || c == '\'' || c == '-');
            if is_word {
                let syllables: Vec<&str> = hypher::hyphenate(word, lang).collect();
                let mut offset = start;
                for syllable in syllables.iter().take(syllables.len().saturating_sub(1)) {
                    offset += syllable.len();
                    if offset > start && offset < boundary && widths.hyphenates(offset) {
                        opportunities.push(Opportunity {
                            end: offset,
                            hyphen: true,
                            forced: false,
                        });
                    }
                }
            }
            start = boundary;
        }
    }

    /// The paragraph's break list: every opportunity, with the text
    /// it leaves behind and the marks that may hang at either end.
    pub(super) fn break_points(
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
            forced: false,
            dash: false,
            hang_end: 0.0,
            hang_start: start_hang(0),
        }];
        for opportunity in self.opportunities(text, widths, options) {
            let end = opportunity.end - newline_before(text, opportunity.end);
            let content_end = end - trailing_spaces(text, 0, end);
            // Preformatted text starts its next line where the
            // author started it, spaces included.
            let next = match options.preformatted {
                true => opportunity.end,
                false => skip_spaces(text, opportunity.end),
            };
            let hard = opportunity.forced && opportunity.end < text.len();
            let hang = if !hangs || hard {
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
                forced: opportunity.forced,
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
}

/// Bytes of the line separator a line ends on at `end`, which is
/// never drawn: a newline, a carriage return and the newline after
/// it, or one of the other separators UAX #14 breaks after.
fn newline_before(text: &str, end: usize) -> usize {
    let before = &text[..end];
    if before.ends_with("\r\n") {
        return 2;
    }
    match before.chars().next_back() {
        Some(
            separator @ ('\n' | '\r' | '\u{b}' | '\u{c}' | '\u{85}' | '\u{2028}' | '\u{2029}'),
        ) => separator.len_utf8(),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use crate::lines::testing::{
        body, hyphenated, layout_body, layout_body_opts, layout_verse, line_text, units_per_em,
    };
    use crate::lines::{HangEnd, HangingPunctuation, LineBreakOptions};

    /// No mark hangs at a hard break. The same comma at a line end the
    /// measure chose hangs past it.
    #[test]
    fn no_mark_hangs_at_a_hard_break() {
        let forced = LineBreakOptions {
            hanging: HangingPunctuation {
                end: HangEnd::Force,
                ..Default::default()
            },
            ..Default::default()
        };
        let broken = layout_verse(
            "My father had a small estate,\nand I was the third of five sons.",
            400.0,
            forced,
        );
        assert_eq!(line_text(&broken[0]), "My father had a small estate,");
        assert_eq!(broken[0].overhang, 0.0, "the comma hung at a hard break");

        let wrapped = layout_body_opts(
            "My father had a small estate, and I was the third of five sons.",
            116.0,
            forced,
        );
        assert_eq!(line_text(&wrapped[0]), "My father had a small estate,");
        assert!(wrapped[0].overhang > 0.0, "the comma did not hang at all");
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

    /// A code span takes no hyphen, whatever `hyphens: auto` asks for
    /// around it, which is what the vocabulary says of one. The same
    /// word as prose does break.
    #[test]
    fn a_code_span_takes_no_hyphen() {
        use crate::content::{Inline, NodeId};
        use crate::lines::LineLayout;
        use crate::lines::testing::{code, hyphenated, one_run, registry};

        let layout = LineLayout::new(registry());
        let set = |inlines: &[Inline]| {
            layout
                .layout(inlines, &body(), 44.0, hyphenated())
                .iter()
                .map(line_text)
                .collect::<Vec<String>>()
        };
        let prose = set(&one_run("extraordinarily"));
        assert!(prose.len() > 1, "the word did not break: {prose:?}");
        let spanned = set(&[code(NodeId::new(3), "extraordinarily")]);
        assert_eq!(spanned, ["extraordinarily"], "the code span was broken");
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
}
