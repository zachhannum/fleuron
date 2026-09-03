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
use fleuron::images::Assets;
use fleuron::layout::{Fragment, Paginator, Piece};
use fleuron::pdf;
use fleuron::session::Session;
use fleuron::style::{Source, Stylesheets};
use fleuron::wire;

use crate::corpus::Corpus;

/// What #12 buys with the harness: the numbers a v0.1 release has to
/// hit on the gate book.
pub mod budget {
    use std::time::Duration;

    /// A book-scale manuscript goes from markdown to PDF bytes
    /// natively in under a second — the whole pipeline, since that is
    /// what a build step waits on.
    pub const NATIVE_END_TO_END: Duration = Duration::from_millis(1000);

    /// What reading the gate book costs inside that. Parse has a
    /// ceiling of its own because a host re-reads a chapter far more
    /// often than it renders a book, and a frontend that slowed down
    /// would otherwise hide inside a pipeline that had not.
    pub const PARSE: Duration = Duration::from_millis(20);

    /// The same book laid out in the worker and encoded for the wire,
    /// where the budget is a span of a reader's patience rather than
    /// a build step, and where export is a separate act the reader
    /// asked for. Serialization is inside it because a display structure
    /// the host cannot read yet is not a page anyone can see.
    pub const WASM_LAYOUT: Duration = Duration::from_millis(500);

    /// Bytes a book-scale layout may hold at its peak, over what the
    /// content tree already costs. The display structure is the floor —
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
            Target::Wasm => ("layout + wire", budget::WASM_LAYOUT),
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
    /// Markdown to content tree.
    pub parse: Duration,
    /// Style compilation: parse, match, cascade.
    pub style: Duration,
    /// Line layout: shaping, breaking, measuring.
    pub line_layout: Duration,
    /// Fragmentation and page assembly, over already-laid-out lines.
    pub fragment: Duration,
    /// Both of the above, as one call — what a caller pays.
    pub layout: Duration,
    /// display structure to the bytes that cross to the host.
    pub serialize: Duration,
    /// Size of the encoded display structure.
    pub wire_bytes: usize,
    /// What those bytes hash to. Layout is deterministic and the wire
    /// is positional, so this number is the same on every target that
    /// agrees about the book, which is how a run under wasm and a
    /// run natively are held to producing the same page.
    pub wire_digest: u64,
    /// display structure to PDF bytes.
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
    /// Markdown to PDF bytes: what a build step waits on.
    pub fn end_to_end(&self) -> Duration {
        self.parse + self.style + self.layout + self.pdf
    }

    /// The budgets this report is held to on `target`.
    pub fn checks(&self, target: Target) -> Vec<Check> {
        let (label, ceiling) = target.time_budget();
        let measured = match target {
            Target::Native => self.end_to_end(),
            Target::Wasm => self.layout + self.serialize,
        };
        vec![
            Check {
                label: "parse",
                measured: self.parse.as_secs_f64() * 1000.0,
                ceiling: budget::PARSE.as_secs_f64() * 1000.0,
                unit: "ms",
            },
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
    let markdown = corpus.markdown();
    let book = corpus.book();
    let styles = crate::styles(&book);
    let paginator = Paginator::new(registry, &styles);

    let mut parse = Duration::MAX;
    let mut style = Duration::MAX;
    let mut line_layout = Duration::MAX;
    let mut fragment = Duration::MAX;
    let mut layout = Duration::MAX;
    let mut pdf_time = Duration::MAX;
    let mut serialize = Duration::MAX;
    let mut wire_bytes = 0;
    let mut wire_digest = 0;
    let mut lines = 0;
    let mut pages = 0;
    let mut pdf_bytes = 0;
    let mut layout_peak = 0u64;
    let mut style_rerender = Duration::MAX;
    let mut session_peak = 0u64;

    for _ in 0..runs.max(1) {
        let start = Instant::now();
        black_box(corpus.parse(markdown));
        parse = parse.min(start.elapsed());

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
            let output = fleuron::layout::layout_book(&book, &styles, registry, &Assets::none());
            layout = layout.min(start.elapsed());
            output
        });
        layout_peak = layout_peak.max(peak as u64);
        pages = output.pages.len();

        let start = Instant::now();
        let encoded = black_box(wire::encode(&output).expect("a display structure encodes"));
        serialize = serialize.min(start.elapsed());
        wire_bytes = encoded.len();
        wire_digest = digest(&encoded);

        let start = Instant::now();
        let bytes = black_box(
            pdf::write(&output, registry, &Assets::none(), &book.metadata)
                .expect("fixture book writes PDF"),
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
        parse,
        style,
        line_layout,
        fragment,
        layout,
        serialize,
        wire_bytes,
        wire_digest,
        pdf: pdf_time,
        pdf_bytes,
        layout_peak,
        style_rerender,
        session_peak,
    }
}

/// FNV-1a over the encoded display structure: a number two runs can be
/// compared on without a hash crate in the harness.
fn digest(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
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
            ("parse", self.parse),
            ("style", self.style),
            ("line layout", self.line_layout),
            ("fragment", self.fragment),
            ("layout", self.layout),
            ("serialize", self.serialize),
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
        writeln!(
            f,
            "  {:<14} {:>9} KiB  digest {:016x}",
            "display structure",
            self.wire_bytes / 1024,
            self.wire_digest,
        )?;
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
    /// waits for PDF bytes, a reader waits for a page. A run that
    /// clears the native budget end to end can still blow the
    /// worker's on layout and the wire alone.
    #[test]
    fn each_target_is_timed_on_what_it_waits_for() {
        let report = Report {
            corpus: Corpus::GATE,
            pages: 300,
            lines: 10_000,
            parse: Duration::from_millis(30),
            style: Duration::from_millis(10),
            line_layout: Duration::from_millis(600),
            fragment: Duration::from_millis(20),
            layout: Duration::from_millis(700),
            serialize: Duration::from_millis(40),
            wire_bytes: 0,
            wire_digest: 0,
            pdf: Duration::from_millis(100),
            pdf_bytes: 0,
            layout_peak: 1024 * 1024,
            style_rerender: Duration::from_millis(6),
            session_peak: 2 * 1024 * 1024,
        };
        assert_eq!(report.end_to_end(), Duration::from_millis(840));

        let native = report.checks(Target::Native);
        assert_eq!(native[1].label, "end to end");
        assert_eq!(native[1].measured, 840.0);
        assert!(native[1].passed());

        let wasm = report.checks(Target::Wasm);
        assert_eq!(wasm[1].label, "layout + wire");
        assert_eq!(wasm[1].measured, 740.0);
        assert!(
            !wasm[1].passed(),
            "700 ms of layout and 40 of encoding blows the worker budget"
        );

        // Parse and the memory ceiling do not move with the target.
        assert_eq!(native[0], wasm[0]);
        assert_eq!(native[2], wasm[2]);
    }
}
