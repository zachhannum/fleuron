//! Knuth-Plass total fit: the set of breaks with the fewest
//! demerits over the whole paragraph.

use super::justify::{LETTER_SHRINK, LETTER_STRETCH, SPACE_SHRINK, SPACE_STRETCH};
use super::line::Widths;
use super::measure::{Measure, Span};
use super::opportunity::Break;
use super::paragraph::{HangEnd, LineBreakOptions};

/// Knuth's demerit weights. A line costs `(line + badness)^2`, so a
/// paragraph of evenly loose lines beats one tight line and one
/// gaping one; the rest are the surcharges for breaking a word, for
/// doing it twice running, and for setting a tight line under a
/// loose one.
pub(super) const LINE_PENALTY: f64 = 10.0;

pub(super) const HYPHEN_PENALTY: f64 = 50.0;

pub(super) const DOUBLE_HYPHEN_DEMERITS: f64 = 10_000.0;

pub(super) const ADJACENT_DEMERITS: f64 = 10_000.0;

/// What breaking before a dash costs. UAX #14 allows a line to end
/// on either side of one; a book only ever ends on the far side, so
/// the near side has to be worth something to be taken.
pub(super) const DASH_PENALTY: f64 = 200.0;

/// The worst a line may be counted as. Without a ceiling a single
/// unbreakable line swamps every other term in the paragraph.
pub(super) const MAX_BADNESS: f64 = 10_000.0;

/// What a line that overflows the measure costs, per em it overflows
/// by and once besides. Larger than any feasible paragraph, so text
/// runs into the margin only where nothing else will set, and the
/// least of it wins.
pub(super) const OVERFULL_DEMERITS: f64 = 1e12;

/// Hyphenated line ends allowed in a row.
pub(super) const MAX_CONSECUTIVE_HYPHENS: u8 = 2;

/// What a ragged line may leave at the right, as a fraction of the
/// measure, before it counts as loose. Ragged setting has no glue to
/// stretch, so badness has nothing else to read the gap against.
pub(super) const RAGGED_STRETCH: f32 = 0.1;

/// A chosen span end, with the adjustment its glue takes.
#[derive(Debug, Clone, Copy)]
pub(super) struct Fitted {
    /// Index into the break list.
    pub(super) at: usize,
    /// The slot of the profile the text up to here fills.
    pub(super) slot: usize,
    /// The line's adjustment ratio: what fraction of its stretch or
    /// shrink the glue gives up to reach the measure.
    pub(super) ratio: f32,
    /// Font units hanging past the measure at the line's end.
    pub(super) overhang: f32,
    /// Font units hanging before the line's origin.
    pub(super) protrusion: f32,
}

/// One paragraph's total-fit pass: break list in, chosen line ends
/// out.
pub(super) struct Breaker<'a> {
    pub(super) breaks: &'a [Break],
    pub(super) widths: &'a Widths,
    pub(super) measure: &'a Measure,
    /// The span every slot past the profile's listed ones is set in,
    /// which is every slot of most paragraphs.
    pub(super) rest: Span,
    /// How many spans the profile lists.
    pub(super) settled: usize,
    /// The slot the paragraph's first band ends in.
    pub(super) first_band: usize,
    pub(super) upem: f32,
    pub(super) size: f32,
    /// Font units a hyphenated break is charged for.
    pub(super) hyphen: f32,
    pub(super) options: LineBreakOptions,
    /// The break the first line starts from. Everything before it is
    /// set already.
    pub(super) from: usize,
    /// Where the opening line has to end, when an opening style
    /// covers exactly that much of the text.
    pub(super) opening: Option<usize>,
}

/// How one candidate line comes out: how far its glue is from its
/// natural width, and what that costs.
pub(super) struct Fit {
    pub(super) ratio: f32,
    pub(super) badness: f64,
    /// Font units the line runs past the measure by, if it does.
    pub(super) overflow: f32,
    pub(super) overhang: f32,
    pub(super) protrusion: f32,
    /// Whether the span this fills ends the band it sits in.
    pub(super) ends_band: bool,
}

/// A breakpoint reached by a path, and the best path to it.
pub(super) struct Node {
    /// Index into the break list.
    pub(super) at: usize,
    /// Slots the paragraph has filled to get here.
    pub(super) slot: usize,
    /// Which of the four fitness classes the line ending here fell
    /// in.
    pub(super) fitness: u8,
    /// Hyphenated line ends in a row up to and including this one.
    pub(super) hyphens: u8,
    pub(super) demerits: f64,
    pub(super) ratio: f32,
    pub(super) overhang: f32,
    pub(super) protrusion: f32,
    /// The node this one was reached from; the root has none.
    pub(super) previous: Option<usize>,
}

/// A span end worth keeping, before it becomes a node.
pub(super) struct Candidate {
    pub(super) at: usize,
    pub(super) slot: usize,
    pub(super) fitness: u8,
    pub(super) hyphens: u8,
    pub(super) demerits: f64,
    pub(super) ratio: f32,
    pub(super) overhang: f32,
    pub(super) protrusion: f32,
    pub(super) previous: usize,
}

impl Breaker<'_> {
    /// The span slot `index` is set in.
    pub(super) fn span(&self, index: usize) -> Span {
        if index < self.settled {
            self.measure.at(index)
        } else {
            self.rest
        }
    }

    /// Points → the paragraph's font units.
    pub(super) fn units(&self, points: f32) -> f32 {
        if self.size > 0.0 {
            points / self.size * self.upem
        } else {
            0.0
        }
    }

    /// The last breakpoint, which every path has to reach.
    pub(super) fn end(&self) -> usize {
        self.breaks.len() - 1
    }

    /// Measures the text that runs from break `a` to break `b`, set
    /// in slot `slot`.
    pub(super) fn fit(&self, a: usize, b: usize, slot: usize) -> Fit {
        let span = self.span(slot - 1);
        let start = self.breaks[a].next;
        let end = self.breaks[b].content_end.max(start);
        let measure = self.units(span.width);
        let text = self.widths.advance(start, end);
        let spaces = self.widths.spaces[end] - self.widths.spaces[start];
        let hyphen = if self.breaks[b].hyphen {
            self.hyphen
        } else {
            0.0
        };
        // A mark hangs into the margin a band starts at and past the
        // one it ends at. The gap between two spans of a band is
        // neither.
        let protrusion = if self.options.hanging.first && self.opens_band(slot - 1) {
            self.breaks[a].hang_start
        } else {
            0.0
        };
        let natural = text + hyphen - protrusion;
        let overhang = if span.ends_band {
            self.overhang(b, natural, measure)
        } else {
            0.0
        };
        let width = natural - overhang;

        let last = b == self.end();
        // The last line of a paragraph fills whatever it fills: the
        // glue that finishes it stretches without limit.
        let (stretch, shrink) = if last {
            let shrink = if self.options.justify {
                spaces * SPACE_SHRINK
            } else {
                0.0
            };
            (f32::INFINITY, shrink)
        } else if self.options.justify {
            let letters = if self.options.inter_character {
                text - spaces
            } else {
                0.0
            };
            (
                spaces * SPACE_STRETCH + letters * LETTER_STRETCH,
                spaces * SPACE_SHRINK + letters * LETTER_SHRINK,
            )
        } else {
            // Ragged setting has no glue to open, so the gap at the
            // right is read against a fraction of the measure.
            (measure * RAGGED_STRETCH, 0.0)
        };

        let gap = measure - width;
        let ratio = if gap > 0.0 {
            if stretch > 0.0 {
                gap / stretch
            } else {
                f32::INFINITY
            }
        } else if gap < 0.0 {
            if shrink > 0.0 {
                gap / shrink
            } else {
                f32::NEG_INFINITY
            }
        } else {
            0.0
        };
        Fit {
            ratio,
            badness: badness(ratio),
            overflow: (-gap).max(0.0),
            overhang,
            protrusion,
            ends_band: span.ends_band,
        }
    }

    /// Whether the band the span at `index` sits in opens there.
    pub(super) fn opens_band(&self, index: usize) -> bool {
        index == 0 || self.span(index - 1).ends_band
    }

    /// What hangs past the measure at break `b`, given how wide the
    /// line would otherwise be.
    pub(super) fn overhang(&self, b: usize, natural: f32, measure: f32) -> f32 {
        let hang = self.breaks[b].hang_end;
        if hang <= 0.0 {
            return 0.0;
        }
        if b == self.end() && self.options.hanging.last {
            return hang;
        }
        match self.options.hanging.end {
            HangEnd::Force => hang,
            HangEnd::Allow if natural > measure && natural - hang <= measure => hang,
            _ => 0.0,
        }
    }

    /// The chosen line ends, first to last.
    pub(super) fn run(&self) -> Vec<Fitted> {
        let mut nodes = vec![Node {
            at: self.from,
            slot: 0,
            fitness: 1,
            hyphens: 0,
            demerits: 0.0,
            ratio: 0.0,
            overhang: 0.0,
            protrusion: 0.0,
            previous: None,
        }];
        let mut active = vec![0usize];
        let mut candidates: Vec<Candidate> = Vec::new();

        for b in self.from + 1..self.breaks.len() {
            let forced = b == self.end();
            // The cheapest way to break here anyway, for a paragraph
            // that cannot be set inside the measure at all.
            let mut overfull: Option<Candidate> = None;
            let mut index = 0;
            while index < active.len() {
                let a = active[index];
                let slot = nodes[a].slot + 1;
                // The opening line ends where the style over it does,
                // so the text set in that style is the text on it.
                if slot == self.first_band + 1 && self.opening.is_some_and(|end| end != b) {
                    index += 1;
                    continue;
                }
                let fit = self.fit(nodes[a].at, b, slot);
                if let Some(candidate) = self.candidate(&nodes[a], a, b, slot, &fit) {
                    self.keep_best(&mut candidates, candidate);
                }
                let long = fit.ratio < -1.0;
                if long {
                    let candidate = Candidate {
                        demerits: nodes[a].demerits
                            + OVERFULL_DEMERITS * (1.0 + (fit.overflow / self.upem) as f64),
                        ratio: -1.0,
                        fitness: 0,
                        ..self.forced(&nodes[a], a, b, slot, &fit)
                    };
                    if overfull
                        .as_ref()
                        .is_none_or(|best| candidate.demerits < best.demerits)
                    {
                        overfull = Some(candidate);
                    }
                }
                if long || forced {
                    active.remove(index);
                } else {
                    index += 1;
                }
            }
            if candidates.is_empty() {
                if !active.is_empty() {
                    continue;
                }
                // Nothing fits and nothing is left to try: overflow
                // the measure rather than drop the text.
                candidates.push(overfull.expect("a line was too long to set"));
            }
            for candidate in candidates.drain(..) {
                nodes.push(Node {
                    at: candidate.at,
                    slot: candidate.slot,
                    fitness: candidate.fitness,
                    hyphens: candidate.hyphens,
                    demerits: candidate.demerits,
                    ratio: candidate.ratio,
                    overhang: candidate.overhang,
                    protrusion: candidate.protrusion,
                    previous: Some(candidate.previous),
                });
                active.push(nodes.len() - 1);
            }
        }

        let best = nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.at == self.end())
            .min_by(|(_, one), (_, other)| one.demerits.total_cmp(&other.demerits))
            .map(|(index, _)| index);
        let mut chosen = Vec::new();
        let mut at = best;
        while let Some(index) = at {
            let node = &nodes[index];
            if node.previous.is_none() {
                break;
            }
            chosen.push(Fitted {
                at: node.at,
                slot: node.slot - 1,
                ratio: node.ratio,
                overhang: node.overhang,
                protrusion: node.protrusion,
            });
            at = node.previous;
        }
        chosen.reverse();
        chosen
    }

    /// Keeps the cheapest candidate of each kind. Two paths that
    /// reach the same break and are alike in everything the rest of
    /// the paragraph can see are interchangeable, so only the cheaper
    /// survives, which is what keeps the active list from growing
    /// with the paragraph.
    pub(super) fn keep_best(&self, candidates: &mut Vec<Candidate>, candidate: Candidate) {
        let key = |candidate: &Candidate| {
            (
                candidate.fitness,
                candidate.hyphens,
                // Only the spans the profile lists make the count
                // visible from here on; past them every band is the
                // same one.
                candidate.slot.min(self.settled + 1),
            )
        };
        match candidates
            .iter_mut()
            .find(|kept| key(kept) == key(&candidate))
        {
            Some(kept) if kept.demerits <= candidate.demerits => {}
            Some(kept) => *kept = candidate,
            None => candidates.push(candidate),
        }
    }

    /// Break `b` reached from node `a` whatever it costs: the line
    /// between them is wider than the measure, and setting it is
    /// still better than losing the words.
    pub(super) fn forced(
        &self,
        from: &Node,
        a: usize,
        b: usize,
        slot: usize,
        fit: &Fit,
    ) -> Candidate {
        Candidate {
            at: b,
            slot,
            fitness: 0,
            hyphens: self.hyphens(from, b),
            demerits: from.demerits,
            ratio: fit.ratio,
            overhang: fit.overhang,
            protrusion: fit.protrusion,
            previous: a,
        }
    }

    /// Hyphenated line ends in a row, counting the one break `b`
    /// would add.
    pub(super) fn hyphens(&self, from: &Node, b: usize) -> u8 {
        if self.breaks[b].hyphen {
            from.hyphens + 1
        } else {
            0
        }
    }

    /// Break `b` reached from node `a`, if the line between them can
    /// be set at all.
    pub(super) fn candidate(
        &self,
        from: &Node,
        a: usize,
        b: usize,
        slot: usize,
        fit: &Fit,
    ) -> Option<Candidate> {
        if fit.ratio < -1.0 {
            return None;
        }
        let hyphens = self.hyphens(from, b);
        if hyphens > MAX_CONSECUTIVE_HYPHENS {
            return None;
        }
        // The last line is not stretched to the measure, so what is
        // left at its right is not a fault to be charged for.
        let ratio = if b == self.end() && fit.ratio > 0.0 {
            0.0
        } else {
            fit.ratio
        };
        let penalty = if self.breaks[b].hyphen {
            HYPHEN_PENALTY
        } else if self.breaks[b].dash {
            DASH_PENALTY
        } else {
            0.0
        };
        let fitness = fitness(ratio);
        // Crossing from one span of a band to the next is not a line
        // break, and the surcharge a line costs is not charged there.
        let line_penalty = if fit.ends_band { LINE_PENALTY } else { 0.0 };
        let mut demerits = (line_penalty + fit.badness + penalty).powi(2);
        if hyphens > 1 {
            demerits += DOUBLE_HYPHEN_DEMERITS;
        }
        if fitness.abs_diff(from.fitness) > 1 {
            demerits += ADJACENT_DEMERITS;
        }
        Some(Candidate {
            at: b,
            slot,
            fitness,
            hyphens,
            demerits: from.demerits + demerits,
            ratio,
            overhang: fit.overhang,
            protrusion: fit.protrusion,
            previous: a,
        })
    }
}

/// How bad a line of this adjustment ratio is. Cubic, so one gaping
/// line costs more than several slightly loose ones.
pub(super) fn badness(ratio: f32) -> f64 {
    if !ratio.is_finite() {
        return MAX_BADNESS;
    }
    (100.0 * (ratio.abs() as f64).powi(3)).min(MAX_BADNESS)
}

/// Knuth's four fitness classes: tight, decent, loose, very loose. A
/// tight line under a very loose one reads as a mistake even when
/// each of them on its own does not, which is why the class and not
/// the ratio is what the demerits compare.
pub(super) fn fitness(ratio: f32) -> u8 {
    if ratio < -0.5 {
        0
    } else if ratio <= 0.5 {
        1
    } else if ratio < 1.0 {
        2
    } else {
        3
    }
}

#[cfg(test)]
mod tests {
    use crate::lines::testing::{hyphenated, layout_body, layout_body_opts, line_text};

    /// Acceptance: hyphenation never runs to three line ends in a
    /// row, whatever the demerits would otherwise say. A narrow
    /// measure over long words is where a breaker would do it.
    #[test]
    fn hyphens_never_run_three_deep() {
        let text = "extraordinarily complicated administrative organisation \
                    demonstrably incomprehensible";
        let lines = layout_body_opts(text, 40.0, hyphenated());
        let hyphenated: Vec<bool> = lines
            .iter()
            .map(|line| line_text(line).ends_with('-'))
            .collect();
        assert!(
            hyphenated.iter().any(|end| *end),
            "nothing was hyphenated: {hyphenated:?}",
        );
        assert!(
            !hyphenated.windows(3).any(|run| run == [true, true, true]),
            "three hyphenated line ends in a row: {hyphenated:?}",
        );
    }

    /// Acceptance: the fixture paragraph breaks where total fit says
    /// it should, and at one of these measures that is not where
    /// greedy would have put it. The opening of Gulliver §2, widths
    /// derived independently of this module (per-word sums of
    /// hb-shape advances for EB Garamond).
    ///
    /// Per-word widths (pt): My 14.29, father 24.70, had 15.62,
    /// a 4.39, small 21.78, estate 23.43, in 8.50,
    /// Nottinghamshire: 75.57; space 2.20.
    #[test]
    fn breaks_match_hand_computed_reference() {
        let text = "My father had a small estate in Nottinghamshire:";
        let expected: &[(f32, &[&str])] = &[
            (
                50.0,
                &["My father", "had a small", "estate in", "Nottinghamshire:"],
            ),
            (
                60.0,
                &["My father", "had a small", "estate in", "Nottinghamshire:"],
            ),
            (
                80.0,
                &["My father had", "a small estate in", "Nottinghamshire:"],
            ),
            (
                120.0,
                &["My father had a small estate", "in Nottinghamshire:"],
            ),
            (250.0, &["My father had a small estate in Nottinghamshire:"]),
        ];
        for (measure, want_lines) in expected {
            let lines = layout_body(text, *measure);
            let got: Vec<String> = lines.iter().map(line_text).collect();
            assert_eq!(&got, want_lines, "measure {measure}: {lines:?}");
        }
    }

    /// The arithmetic behind the 80pt row above, which is the row
    /// where the two breakers part.
    ///
    /// Ragged badness is `100 * r^3`, where `r` is the gap left at
    /// the right over a tenth of the measure, and a line costs
    /// `(10 + badness)^2`, with 10,000 more when its fitness class is
    /// two off the line before it. The last line fills what it fills
    /// and costs 100.
    ///
    /// Total fit: 59.01 (r 2.62, 3.30M) + 64.70 (r 1.91, 0.50M)
    /// + 75.57 (100) = 3.82M, two class changes included.
    ///
    /// Greedy: 65.60 (r 1.80, 0.35M) + 58.11 (r 2.74, 4.24M)
    /// + 75.57 (100) = 4.61M, the same two.
    ///
    /// Greedy wins line one and loses the paragraph: the word it
    /// pulls up leaves a hole on line two larger than the one it
    /// filled.
    #[test]
    fn total_fit_beats_greedy_where_they_disagree() {
        let text = "My father had a small estate in Nottinghamshire:";
        let got: Vec<String> = layout_body(text, 80.0).iter().map(line_text).collect();
        assert_eq!(got[0], "My father had", "line 1: {got:?}");
        assert_ne!(
            got[0], "My father had a",
            "line 1 was packed greedily: {got:?}",
        );
    }
}
