//! Peak-allocation tracking, for the memory ceiling.
//!
//! Resident set size is the wrong instrument: the allocator returns
//! pages to the OS on its own schedule, so RSS reports the allocator's
//! mood as much as the engine's appetite, and reports it differently
//! on every platform — including wasm, where there is no OS to ask.
//! Counting bytes in and out of the allocator answers the question the
//! ceiling actually asks: how much does a book-scale run hold at once?
//!
//! Installing the tracker is the binary's choice, never the library's:
//! a crate that sets a global allocator sets it for everything that
//! links it.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// A pass-through allocator that records how many bytes are live and
/// how high that has been.
///
/// Install it with `#[global_allocator] static A: Tracking = Tracking;`.
pub struct Tracking;

/// Whether the tracker is the global allocator. Without it every
/// reading is zero, and a memory ceiling nothing is measured against
/// is a ceiling that always passes.
pub fn installed() -> bool {
    INSTALLED.load(Ordering::Relaxed)
}

/// Bytes currently held.
pub fn live() -> usize {
    LIVE.load(Ordering::Relaxed)
}

/// The high-water mark since the last `reset_peak`.
pub fn peak() -> usize {
    PEAK.load(Ordering::Relaxed)
}

/// Drops the high-water mark to what is live now, so the next
/// measurement reports one phase's appetite rather than the whole
/// process's history.
pub fn reset_peak() {
    PEAK.store(LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
}

/// Runs `body`, returning its value and the bytes held at the peak
/// over what was already live when it started.
pub fn measure<T>(body: impl FnOnce() -> T) -> (T, usize) {
    reset_peak();
    let before = live();
    let value = body();
    (value, peak().saturating_sub(before))
}

fn record(bytes: usize) {
    INSTALLED.store(true, Ordering::Relaxed);
    let live = LIVE.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK.fetch_max(live, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for Tracking {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            record(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            record(layout.size());
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
            record(new_size);
        }
        new_ptr
    }
}
