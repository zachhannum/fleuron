//! Fixtures and measurement: the harness the engine is held to.
//!
//! Two real books, checked in as markdown and read into content trees
//! here; criterion benches that time one pipeline stage at a time; and
//! a gate binary that runs a whole book against absolute budgets, the
//! same way natively and under wasm.
//!
//! Nothing here ships. The crate exists so that a perf claim about
//! fleuron is a number somebody can reproduce.

#![deny(missing_docs)]

pub mod alloc;
pub mod corpus;
pub mod gate;
pub mod markdown;

pub use corpus::Corpus;

/// The bundled font registry, shared across benches and gate runs so
/// that font parsing is not counted as layout.
pub fn registry() -> &'static fleuron::fonts::FontRegistry {
    static REGISTRY: std::sync::OnceLock<fleuron::fonts::FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| fleuron::fonts::bundled_registry().expect("bundled font parses"))
}

/// One book's styling under the built-in sheet alone, compiled
/// against the shared registry. The default sheet loads no author
/// fonts, so nothing is added to it.
pub fn styles(book: &fleuron::content::Book) -> fleuron::style::StyleTree {
    fleuron::style::defaults(book, registry())
}

/// Every block of a book as the flat text one shaping call is handed,
/// in document order. v0.1 puts one face on everything, so a block is
/// a run and a run is a shaping call.
pub fn shaped_texts(book: &fleuron::content::Book) -> Vec<String> {
    use fleuron::content::Block;
    fn walk(blocks: &[Block], out: &mut Vec<String>) {
        for block in blocks {
            match block {
                Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => {
                    let text = markdown::flatten(inlines);
                    if !text.is_empty() {
                        out.push(text);
                    }
                }
                Block::Blockquote { blocks, .. } => walk(blocks, out),
                Block::ThematicBreak { .. } | Block::Image { .. } => {}
            }
        }
    }
    let mut out = Vec::new();
    for section in &book.sections {
        walk(&section.blocks, &mut out);
    }
    out
}

/// The tracker measures nothing unless it is the global allocator, and
/// the crate's own tests are the one place it can be installed without
/// imposing it on anything that links this.
#[cfg(test)]
#[global_allocator]
static ALLOCATOR: alloc::Tracking = alloc::Tracking;

#[cfg(test)]
mod tests {
    use super::*;

    /// The high-water mark follows a live allocation up and survives
    /// its release: what the ceiling asks is how much was held at
    /// once, not how much is held now.
    #[test]
    fn the_tracker_records_a_peak_that_outlives_the_allocation() {
        assert!(alloc::installed(), "tests run under the tracker");
        let (live_at_peak, peak) = alloc::measure(|| {
            let block: Vec<u8> = vec![7; 4 * 1024 * 1024];
            let live = alloc::live();
            drop(block);
            live
        });
        assert!(peak >= 4 * 1024 * 1024, "peak {peak} missed the allocation");
        assert!(
            live_at_peak >= 4 * 1024 * 1024,
            "live {live_at_peak} missed the allocation"
        );
        assert!(
            alloc::live() < live_at_peak,
            "the release should have brought live back down"
        );
    }

    /// A book-scale run is bounded: the gate book sets the ~300 pages
    /// the budgets are written against, and lays them out inside the
    /// memory ceiling. Timing verdicts stay with the gate binary,
    /// which warns rather than fails — a shared runner's clock is not
    /// evidence, but its allocator is.
    #[test]
    #[cfg_attr(
        debug_assertions,
        ignore = "book-scale run: meaningful only in release"
    )]
    fn a_book_scale_run_stays_inside_the_memory_ceiling() {
        let report = gate::measure(Corpus::GATE, registry(), 1);
        assert!(
            (300..400).contains(&report.pages),
            "{} pages: the gate book should set about 300",
            report.pages
        );
        assert!(report.pdf_bytes > 0, "the run painted nothing");

        let peak = report
            .checks(gate::Target::current())
            .into_iter()
            .find(|check| check.label == "layout peak")
            .expect("every target carries the memory ceiling");
        assert!(peak.passed(), "{peak}");
    }
}
