//! The gate: stage timings, a memory high-water mark, and the budgets
//! CI checks them against.
//!
//! Criterion answers "how fast, and did that change" for one stage in
//! isolation. This answers the different question a release has to
//! answer: does a whole book still fit inside the budget on the
//! machine in front of us. It is one binary, no statistics beyond a
//! best-of-N, and it reports the same numbers natively and under wasm.
//!
//! Budgets are absolute ceilings, not comparisons against a stored
//! baseline: run-to-run comparison needs a baseline that has held
//! still for a while, and a CI runner's timings have not.

use std::fmt;
use std::hint::black_box;
use std::time::{Duration, Instant};

use fleuron::fonts::FontRegistry;
use fleuron::layout::{Fragment, Paginator, Piece};
use fleuron::pdf;
use fleuron::session::Session;
use fleuron::style::{Source, Stylesheets};

use crate::corpus::Corpus;

/// What #12 buys with the harness: the numbers a v0.1 release has to
/// hit on the gate book.
pub mod budget {
    use std::time::Duration;

    /// A book-scale manuscript goes from content tree to PDF bytes
    /// natively in under a second — the whole pipeline, since that is
    /// what a build step waits on.
    pub const NATIVE_END_TO_END: Duration = Duration::from_millis(1000);

    /// The same book laid out in the worker, where the budget is a
    /// span of a reader's patience rather than a build step, and
    /// where export is a separate act the reader asked for.
    pub const WASM_LAYOUT: Duration = Duration::from_millis(500);

    /// Bytes a book-scale layout may hold at its peak, over what the
    /// content tree already costs. The display list is the floor —
    /// every glyph of every page is retained — and a section's lines
    /// are the only thing held above it, so the ceiling sits at about
    /// twice the floor. Allocation counts are identical on every
    /// machine, which is why this one is a hard failure.
    pub const LAYOUT_PEAK: u64 = 32 * 1024 * 1024;

    /// The same for a retained session, which is a different
    /// question. A throwaway pass has one section's lines in memory
    /// at a time. A session has all of them, beside the display
    /// list, which is what saves measuring them again.
    pub const SESSION_PEAK: u64 = 64 * 1024 * 1024;

    /// What a style-only re-render costs a session on a book-scale
    /// manuscript. A sheet that moves the page box re-fragments over
    /// the lines it already has, and a reader dragging a margin
    /// should see the page turn over rather than wait on it.
    pub const STYLE_RERENDER: Duration = Duration::from_millis(20);
}

/// Where the harness is running. The budgets differ; the measurements
/// do not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The machine the gate is run on.
    Native,
    /// A WebAssembly runtime standing in for the worker.
    Wasm,
}

impl Target {
    /// The target the binary was compiled for.
    pub const fn current() -> Target {
        if cfg!(target_arch = "wasm32") {
            Target::Wasm
        } else {
            Target::Native
        }
    }

    /// What the target is timed on, and the ceiling it is held to.
    /// Natively that is the whole pipeline; in the worker it is
    /// layout, which is all the reader is waiting for.
    pub fn time_budget(self) -> (&'static str, Duration) {
        match self {
            Target::Native => ("end to end", budget::NATIVE_END_TO_END),
            Target::Wasm => ("layout", budget::WASM_LAYOUT),
        }
    }

    /// What reports call this target.
    pub fn name(self) -> &'static str {
        match self {
            Target::Native => "native",
            Target::Wasm => "wasm",
        }
    }
}

/// One book measured: the stages, end to end, plus what the run held.
#[derive(Debug, Clone)]
pub struct Report {
    /// The book measured.
    pub corpus: Corpus,
    /// Pages the book fragmented into.
    pub pages: usize,
    /// Lines the paragraph pass produced, across every section.
    pub lines: usize,
    /// Style compilation: parse, match, cascade.
    pub style: Duration,
    /// Line layout: shaping, breaking, measuring.
    pub line_layout: Duration,
    /// Fragmentation and page assembly, over already-laid-out lines.
    pub fragment: Duration,
    /// Both of the above, as one call — what a caller pays.
    pub layout: Duration,
    /// Display list to PDF bytes.
    pub pdf: Duration,
    /// Size of the PDF the run wrote.
    pub pdf_bytes: usize,
    /// Bytes held at the peak of the layout call, over the content
    /// tree it was given.
    pub layout_peak: u64,
    /// A style-only re-render over a retained session, measured with
    /// a sheet that moves the page box.
    pub style_rerender: Duration,
    /// Bytes a session occupies at its peak, over the content tree
    /// it was handed.
    pub session_peak: u64,
}

impl Report {
    /// Content tree to PDF bytes: what a build step waits on.
    pub fn end_to_end(&self) -> Duration {
        self.style + self.layout + self.pdf
    }

    /// The budgets this report is held to on `target`.
    pub fn checks(&self, target: Target) -> Vec<Check> {
        let (label, ceiling) = target.time_budget();
        let measured = match target {
            Target::Native => self.end_to_end(),
            Target::Wasm => self.layout,
        };
        vec![
            Check {
                label,
                measured: measured.as_secs_f64() * 1000.0,
                ceiling: ceiling.as_secs_f64() * 1000.0,
                unit: "ms",
            },
            Check {
                label: "layout peak",
                measured: self.layout_peak as f64 / (1024.0 * 1024.0),
                ceiling: budget::LAYOUT_PEAK as f64 / (1024.0 * 1024.0),
                unit: "MiB",
            },
            Check {
                label: "re-render",
                measured: self.style_rerender.as_secs_f64() * 1000.0,
                ceiling: budget::STYLE_RERENDER.as_secs_f64() * 1000.0,
                unit: "ms",
            },
            Check {
                label: "session peak",
                measured: self.session_peak as f64 / (1024.0 * 1024.0),
                ceiling: budget::SESSION_PEAK as f64 / (1024.0 * 1024.0),
                unit: "MiB",
            },
        ]
    }
}

/// One measured quantity against its ceiling.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Check {
    /// What the quantity is called in the report.
    pub label: &'static str,
    /// What this run measured.
    pub measured: f64,
    /// The budget it is held to.
    pub ceiling: f64,
    /// The unit both are in.
    pub unit: &'static str,
}

impl Check {
    /// True when the measurement is at or under its ceiling.
    pub fn passed(&self) -> bool {
        self.measured <= self.ceiling
    }

    /// How much of the ceiling is left, as a fraction of it. Negative
    /// once the budget is blown.
    pub fn headroom(&self) -> f64 {
        if self.ceiling == 0.0 {
            return 0.0;
        }
        (self.ceiling - self.measured) / self.ceiling
    }
}

impl fmt::Display for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:<14} {:>9.1} {:<4} ceiling {:>7.1} {:<4} {:>+6.0}% headroom  {}",
            self.label,
            self.measured,
            self.unit,
            self.ceiling,
            self.unit,
            self.headroom() * 100.0,
            if self.passed() { "ok" } else { "OVER" },
        )
    }
}

/// Measures one book, keeping the best time and the worst memory over
/// `runs`.
///
/// Best-of, not mean: the question is whether the engine can hit the
/// budget, and the slow runs are the machine's noise, not the engine's
/// work. Memory goes the other way — a ceiling is only met if it is
/// met every time.
pub fn measure(corpus: Corpus, registry: &FontRegistry, runs: usize) -> Report {
    let book = corpus.book();
    let styles = crate::styles(&book);
    let paginator = Paginator::new(registry, &styles);

    let mut style = Duration::MAX;
    let mut line_layout = Duration::MAX;
    let mut fragment = Duration::MAX;
    let mut layout = Duration::MAX;
    let mut pdf_time = Duration::MAX;
    let mut lines = 0;
    let mut pages = 0;
    let mut pdf_bytes = 0;
    let mut layout_peak = 0u64;
    let mut style_rerender = Duration::MAX;
    let mut session_peak = 0u64;

    for _ in 0..runs.max(1) {
        let start = Instant::now();
        black_box(crate::styles(&book));
        style = style.min(start.elapsed());

        let start = Instant::now();
        let flows: Vec<Vec<Fragment>> = black_box(
            book.sections
                .iter()
                .map(|section| paginator.section_fragments(section))
                .collect(),
        );
        line_layout = line_layout.min(start.elapsed());
        lines = flows
            .iter()
            .flatten()
            .filter(|fragment| matches!(fragment.piece, Piece::Line { .. }))
            .count();

        let start = Instant::now();
        black_box(paginator.flow(&book, &flows));
        fragment = fragment.min(start.elapsed());

        let (output, peak) = crate::alloc::measure(|| {
            let start = Instant::now();
            let output = fleuron::layout::layout_book(&book, &styles, registry);
            layout = layout.min(start.elapsed());
            output
        });
        layout_peak = layout_peak.max(peak as u64);
        pages = output.pages.len();

        let start = Instant::now();
        let bytes = black_box(
            pdf::write(&output, registry, &book.metadata).expect("fixture book writes PDF"),
        );
        pdf_time = pdf_time.min(start.elapsed());
        pdf_bytes = bytes.len();
    }

    for index in 0..runs.max(1) {
        // A session is handed a content tree it then owns, and the
        // ceiling is about what it builds over one, so the tree is
        // cloned before the measurement opens and moved in.
        let owned = book.clone();
        let (mut session, peak) = crate::alloc::measure(|| {
            let mut session = Session::new(registry);
            session.set_content(owned);
            session.preview();
            session
        });
        session_peak = session_peak.max(peak as u64);

        // A sheet that moves the page box and nothing else, which
        // is the middle tier: it re-fragments over cached lines. The
        // margin lands somewhere the built-in sheet did not, and
        // somewhere no other run put it, so every run measures a real
        // re-render rather than a cache that was already warm.
        let css = format!("@page {{ margin-bottom: {}pt }}", 60 + index);
        let sheets = Stylesheets::parse(&[Source::author("gate.css", &css)]);
        let start = Instant::now();
        session.set_style(sheets);
        black_box(session.preview());
        style_rerender = style_rerender.min(start.elapsed());
    }

    Report {
        corpus,
        pages,
        lines,
        style,
        line_layout,
        fragment,
        layout,
        pdf: pdf_time,
        pdf_bytes,
        layout_peak,
        style_rerender,
        session_peak,
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{} — {} pages, {} lines, {} KiB of PDF",
            self.corpus.slug(),
            self.pages,
            self.lines,
            self.pdf_bytes / 1024,
        )?;
        for (stage, duration) in [
            ("style", self.style),
            ("line layout", self.line_layout),
            ("fragment", self.fragment),
            ("layout", self.layout),
            ("pdf", self.pdf),
            ("end to end", self.end_to_end()),
            ("re-render", self.style_rerender),
        ] {
            writeln!(
                f,
                "  {:<14} {:>9.1} ms",
                stage,
                duration.as_secs_f64() * 1000.0
            )?;
        }
        for (what, bytes) in [("layout", self.layout_peak), ("session", self.session_peak)] {
            writeln!(
                f,
                "  {:<14} {:>9.1} MiB peak",
                what,
                bytes as f64 / (1024.0 * 1024.0)
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The budgets are ceilings: a measurement at the line passes, one
    /// past it does not, and the headroom says by how much.
    #[test]
    fn checks_pass_at_the_ceiling_and_fail_past_it() {
        let at = Check {
            label: "layout",
            measured: 1000.0,
            ceiling: 1000.0,
            unit: "ms",
        };
        assert!(at.passed());
        assert_eq!(at.headroom(), 0.0);

        let over = Check {
            measured: 1500.0,
            ..at
        };
        assert!(!over.passed());
        assert_eq!(over.headroom(), -0.5);

        let under = Check {
            measured: 250.0,
            ..at
        };
        assert!(under.passed());
        assert_eq!(under.headroom(), 0.75);
    }

    /// The two targets are timed on different things: a build step
    /// waits for PDF bytes, a reader waits for layout. A run that
    /// clears the native budget end to end can still blow the
    /// worker's on layout alone.
    #[test]
    fn each_target_is_timed_on_what_it_waits_for() {
        let report = Report {
            corpus: Corpus::GATE,
            pages: 300,
            lines: 10_000,
            style: Duration::from_millis(10),
            line_layout: Duration::from_millis(600),
            fragment: Duration::from_millis(20),
            layout: Duration::from_millis(700),
            pdf: Duration::from_millis(100),
            pdf_bytes: 0,
            layout_peak: 1024 * 1024,
            style_rerender: Duration::from_millis(6),
            session_peak: 2 * 1024 * 1024,
        };
        assert_eq!(report.end_to_end(), Duration::from_millis(810));

        let native = report.checks(Target::Native);
        assert_eq!(native[0].label, "end to end");
        assert_eq!(native[0].measured, 810.0);
        assert!(native[0].passed());

        let wasm = report.checks(Target::Wasm);
        assert_eq!(wasm[0].label, "layout");
        assert_eq!(wasm[0].measured, 700.0);
        assert!(
            !wasm[0].passed(),
            "700 ms of layout blows the worker budget"
        );

        // The memory ceiling does not move with the target.
        assert_eq!(native[1], wasm[1]);
    }
}
