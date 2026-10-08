//! The memory ceilings, measured in a process of their own.
//!
//! The tracker counts bytes through the global allocator, so a
//! reading is only about the work it wraps while nothing else in the
//! process is allocating. A test harness runs its tests on threads
//! that allocate, and a hundred bytes freed by one of them is enough
//! to take a reading below the work it was supposed to measure. This
//! target has no harness, so it runs on one thread and does one
//! thing at a time, which is what the numbers need to mean what they
//! say.

use fleuron::images::{Assets, ImageLoader};
use fleuron_fixtures::gate::{self, Target};
use fleuron_fixtures::{Corpus, alloc, anchored_images, registry, styles};

/// A swelled rule drawn as paths, 192 by 24 CSS pixels.
const TAILPIECE: &[u8] = include_bytes!("../../../fixtures/images/tailpiece.svg");

/// The tracker measures nothing unless it is the global allocator,
/// and a binary is the one place it can be installed without imposing
/// it on everything that links the crate.
#[global_allocator]
static ALLOCATOR: alloc::Tracking = alloc::Tracking;

fn main() {
    assert!(alloc::installed(), "the tracker is the global allocator");
    a_peak_outlives_the_allocation_that_made_it();
    sizing_an_svg_allocates_nothing();
    if cfg!(debug_assertions) {
        println!("book scale: skipped, meaningful only in release");
        return;
    }
    a_book_scale_run_stays_inside_the_memory_ceilings();
    a_book_that_places_an_svg_allocates_what_a_raster_book_does();
    println!("memory ceilings met");
}

/// The high-water mark follows a live allocation up and survives its
/// release. The ceiling asks how much was live at once, not how much
/// is live now.
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
/// budgets are written against, and lays them out inside both memory
/// ceilings, the throwaway pass inside its own and a session with
/// every stage live at once inside the one written for that.
///
/// Timing verdicts stay with the gate binary, which warns rather than
/// fails. A shared runner's clock is not evidence, but its allocator
/// is.
fn a_book_scale_run_stays_inside_the_memory_ceilings() {
    let report = gate::measure(Corpus::GATE, registry(), 1);
    assert!(
        (300..400).contains(&report.pages),
        "{} pages: the gate book should set about 300",
        report.pages
    );
    assert!(report.pdf_bytes > 0, "the run painted nothing");

    let ceilings: Vec<gate::Check> = report
        .checks(Target::current())
        .into_iter()
        .filter(|check| check.unit == "MiB")
        .collect();
    assert_eq!(ceilings.len(), 2, "a memory ceiling went unchecked");
    for peak in ceilings {
        assert!(peak.passed(), "{peak}");
    }
}

/// Layout decodes nothing. The size of an SVG is a read of its root
/// element, and that read allocates no byte.
fn sizing_an_svg_allocates_nothing() {
    let (intrinsic, peak) = alloc::measure(|| fleuron::images::probe(TAILPIECE));
    let intrinsic = intrinsic.expect("the tailpiece has a size");
    assert_eq!((intrinsic.width, intrinsic.height), (192, 24));
    assert_eq!(peak, 0, "the probe allocated {peak} bytes");
}

/// Acceptance: the allocation ceiling is unchanged for a book that
/// places an SVG. The gate book with the tailpiece at the head of
/// every chapter peaks where the same book peaks with a raster header
/// of the same size, and inside the layout ceiling.
fn a_book_that_places_an_svg_allocates_what_a_raster_book_does() {
    /// One file under both names the two books use.
    struct One(Vec<u8>);
    impl ImageLoader for One {
        fn load(&self, _url: &str) -> Option<Vec<u8>> {
            Some(self.0.clone())
        }
    }
    // A PNG header of the tailpiece's size, which is a whole image as
    // far as layout reads one.
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend(13u32.to_be_bytes());
    png.extend(b"IHDR");
    png.extend(192u32.to_be_bytes());
    png.extend(24u32.to_be_bytes());
    png.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);

    let peak = |url: &str, bytes: Vec<u8>| {
        let book = anchored_images::illustrated_with(&Corpus::GATE.book(), url);
        let styles = styles(&book);
        let assets = Assets::probe(&book, &styles, &One(bytes));
        assert_eq!(assets.assets().len(), 1, "{url} did not probe");
        assert!(assets.warnings().is_empty(), "{:?}", assets.warnings());
        let (output, peak) =
            alloc::measure(|| fleuron::layout::layout_book(&book, &styles, registry(), &assets));
        assert!(output.warnings.is_empty(), "{:?}", output.warnings);
        (output.pages.len(), peak)
    };
    let (vector_pages, vector) = peak("tail.svg", TAILPIECE.to_vec());
    let (raster_pages, raster) = peak("tail.png", png);
    assert_eq!(vector_pages, raster_pages);
    assert_eq!(
        vector, raster,
        "an SVG moved the layout peak: {vector} bytes against {raster}"
    );
    assert!(
        vector as u64 <= gate::budget::LAYOUT_PEAK,
        "{vector} bytes is over the layout ceiling"
    );
}
