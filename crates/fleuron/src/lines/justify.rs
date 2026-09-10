//! What the glue on a line gives and takes when the line is set to
//! the measure it was broken to.

use super::line::ShapedRun;
use super::paragraph::LineBreakOptions;

/// What a word space may give and take, as a fraction of its own
/// width: TeX's interword glue, half of stretch and a third of
/// shrink.
pub(super) const SPACE_STRETCH: f32 = 0.5;

pub(super) const SPACE_SHRINK: f32 = 1.0 / 3.0;

/// The same for the space between letters, which only
/// `text-justify: inter-character` opens up. Small on purpose: the
/// eye reads a word by its shape, and a word spaced wider than this
/// stops being one.
pub(super) const LETTER_STRETCH: f32 = 0.02;

pub(super) const LETTER_SHRINK: f32 = 0.01;

/// Spreads one span's adjustment over the glue it was measured
/// with. The ratio was chosen against the shaped advances, so it
/// lands on them: a painter is handed positions, not a rule for
/// working them out.
///
/// The residue is carried from glyph to glyph rather than dropped,
/// so a span of rounded advances still totals its width.
pub(super) fn adjust(runs: &mut [ShapedRun], text: &str, ratio: f32, options: LineBreakOptions) {
    if !options.justify || !ratio.is_finite() || ratio == 0.0 {
        return;
    }
    let ratio = ratio.max(-1.0);
    let (space, letter) = if ratio > 0.0 {
        (SPACE_STRETCH, LETTER_STRETCH)
    } else {
        (SPACE_SHRINK, LETTER_SHRINK)
    };
    let letter = if options.inter_character { letter } else { 0.0 };
    let bytes = text.as_bytes();
    let (mut wanted, mut applied) = (0.0f32, 0i64);
    for run in runs {
        let mut advance = 0i64;
        for glyph in &mut run.glyphs {
            let is_space = bytes.get(glyph.cluster as usize) == Some(&b' ');
            let share = if is_space { space } else { letter };
            wanted += ratio * share * glyph.x_advance as f32;
            let step = wanted.round() as i64 - applied;
            applied += step;
            let width = (glyph.x_advance as i64 + step).max(0);
            advance += width - glyph.x_advance as i64;
            glyph.x_advance = width as u32;
        }
        run.advance = (run.advance as i64 + advance).max(0) as u32;
    }
}

/// Number of trailing ASCII spaces in `[start, end)`.
pub(super) fn trailing_spaces(text: &str, start: usize, end: usize) -> usize {
    let bytes = &text.as_bytes()[start..end];
    bytes.iter().rev().take_while(|b| **b == b' ').count()
}

pub(super) fn skip_spaces(text: &str, mut at: usize) -> usize {
    while at < text.len() && text.as_bytes()[at] == b' ' {
        at += 1;
    }
    at
}

#[cfg(test)]
mod tests {
    use crate::lines::testing::{body, justified, layout_body_opts, registry, units_per_em};
    use crate::lines::{Line, LineBreakOptions};

    /// Justified: every line but the last reaches the right edge of
    /// the measure. The tolerance is the rounding the shaper's
    /// integer advances force: the adjustment is spread over the
    /// line's spaces and each lands on a whole font unit, so a line
    /// can miss by half a unit, which at 11pt is 0.006pt.
    #[test]
    fn justified_lines_fill_the_measure() {
        let text = "My father had a small estate in Nottinghamshire, and I was the \
                    third of five sons. He sent me to Emanuel College in Cambridge \
                    at fourteen years old, where I resided three years.";
        let lines = layout_body_opts(text, 140.0, justified());
        assert!(lines.len() > 3, "expected several lines: {lines:?}");
        for line in &lines[..lines.len() - 1] {
            let width = line.width as f32 / units_per_em() as f32 * body().size;
            assert!(
                (width - 140.0).abs() < 0.01,
                "justified line is {width}pt against a 140pt measure",
            );
        }
        let last = lines.last().unwrap();
        let width = last.width as f32 / units_per_em() as f32 * body().size;
        assert!(width < 140.0, "the last line was stretched to {width}pt");
    }

    /// The adjustment lands on the spaces. Ragged and justified
    /// settings of the same line set the same letters at the same
    /// advances; only what is between the words moves.
    #[test]
    fn justification_opens_the_spaces_and_nothing_else() {
        let text = "one two three four five six seven eight nine ten";
        let ragged = layout_body_opts(text, 100.0, Default::default());
        let justified = layout_body_opts(text, 100.0, justified());
        assert_eq!(ragged.len(), justified.len());
        let space = registry().char_glyph(0, ' ').unwrap();
        let glyphs = |line: &Line| -> Vec<(u32, u32)> {
            line.runs
                .iter()
                .flat_map(|run| run.glyphs.iter())
                .map(|glyph| (glyph.id, glyph.x_advance))
                .collect()
        };
        let (before, after) = (glyphs(&ragged[0]), glyphs(&justified[0]));
        assert_eq!(
            before.len(),
            after.len(),
            "justification changed the glyphs on the line",
        );
        for (was, now) in before.iter().zip(after.iter()) {
            assert_eq!(was.0, now.0, "justification reshaped the line");
            if was.0 == space {
                assert!(now.1 > was.1, "the spaces did not open: {was:?} {now:?}");
            } else {
                assert_eq!(was.1, now.1, "a letter moved: {was:?} {now:?}");
            }
        }
    }

    /// Inter-letter spacing is opt-in: the same line justified with
    /// `text-justify: inter-character` widens its letters, and the
    /// default leaves them alone.
    #[test]
    fn inter_letter_spacing_is_off_until_asked_for() {
        let text = "one two three four five six seven eight nine ten";
        let letters = |line: &Line| -> u32 {
            let space = registry().char_glyph(0, ' ').unwrap();
            line.runs
                .iter()
                .flat_map(|run| run.glyphs.iter())
                .filter(|glyph| glyph.id != space)
                .map(|glyph| glyph.x_advance)
                .sum()
        };
        let words = layout_body_opts(text, 100.0, justified());
        let characters = layout_body_opts(
            text,
            100.0,
            LineBreakOptions {
                inter_character: true,
                ..justified()
            },
        );
        assert!(letters(&characters[0]) > letters(&words[0]));
        let width = |line: &Line| line.width as f32 / units_per_em() as f32 * body().size;
        assert!(
            (width(&characters[0]) - 100.0).abs() < 0.01,
            "the line stopped filling the measure: {}pt",
            width(&characters[0]),
        );
    }
}
