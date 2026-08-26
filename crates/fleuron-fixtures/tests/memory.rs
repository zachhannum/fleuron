//! The memory ceilings, measured in a process of their own.
//!
//! The tracker counts bytes through the global allocator, so a
//! reading is only about the work it wraps while nothing else in the
//! process is allocating — and a test harness runs its tests on
//! threads that allocate, a hundred bytes of which is enough to take
//! a reading below the work it was supposed to measure. This target
//! carries no harness: one thread, one thing at a time, which is what
//! it takes for the numbers to mean what they say.

use fleuron_fixtures::gate::{self, Target};
use fleuron_fixtures::{Corpus, alloc, registry};

/// The tracker measures nothing unless it is the global allocator,
/// and a binary is the one place it can be installed without imposing
/// it on everything that links the crate.
#[global_allocator]
static ALLOCATOR: alloc::Tracking = alloc::Tracking;

fn main() {
    assert!(alloc::installed(), "the tracker is the global allocator");
    a_peak_outlives_the_allocation_that_made_it();
    if cfg!(debug_assertions) {
        println!("book scale: skipped, meaningful only in release");
        return;
    }
    a_book_scale_run_stays_inside_the_memory_ceilings();
    println!("memory ceilings met");
}

/// The high-water mark follows a live allocation up and survives its
/// release: what the ceiling asks is how much was held at once, not
/// how much is held now.
fn a_peak_outlives_the_allocation_that_made_it() {
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

/// A book-scale run is bounded: the gate book sets the ~300 pages the
/// budgets are written against, and lays them out inside the memory
/// ceilings — the throwaway pass inside its own, and a session
/// holding every stage at once inside the one written for that.
///
/// Timing verdicts stay with the gate binary, which warns rather than
/// fails: a shared runner's clock is not evidence, but its allocator
/// is.
fn a_book_scale_run_stays_inside_the_memory_ceilings() {
    let report = gate::measure(Corpus::GATE, registry(), 1);
    assert!(
        (300..400).contains(&report.pages),
        "{} pages: the gate book should set about 300",
        report.pages
    );
    assert!(report.pdf_bytes > 0, "the run painted nothing");

    let held: Vec<gate::Check> = report
        .checks(Target::current())
        .into_iter()
        .filter(|check| check.unit == "MiB")
        .collect();
    assert_eq!(held.len(), 2, "a memory ceiling went unchecked");
    for peak in held {
        assert!(peak.passed(), "{peak}");
    }
}
