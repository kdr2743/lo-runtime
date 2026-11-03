//! `gc-time` feature: wall-clock time spent inside `collect`, reported at exit.
//!
//! This is the measurement hook for the benchmark suite in `bench/`. Process wall
//! time cannot separate collector cost from the mutator's own work, so the
//! collector times itself: every collection (minor, major, large-object forced,
//! explicit) is bracketed by one `Instant` pair and added to three process-wide
//! counters, and `atexit` prints one line to stderr:
//!
//! ```text
//! [lo-gc-time] collections=<n> total_ns=<sum> max_ns=<longest single pause>
//! ```
//!
//! Native only. A wasm32 module has no clock import and no exit hook, so there
//! the feature compiles to no-ops and prints nothing. The runtime is
//! single-threaded, so relaxed atomics are enough; they exist only to keep the
//! statics `Sync`. The wrapper costs two clock reads per collection.

#[cfg(not(target_family = "wasm"))]
mod imp {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
    use std::time::Instant;

    static COLLECTIONS: AtomicU64 = AtomicU64::new(0);
    static TOTAL_NS: AtomicU64 = AtomicU64::new(0);
    static MAX_NS: AtomicU64 = AtomicU64::new(0);
    static REGISTERED: AtomicBool = AtomicBool::new(false);

    extern "C" {
        fn atexit(callback: extern "C" fn()) -> i32;
    }

    extern "C" fn report() {
        let (n, total, max) = totals();
        eprintln!("[lo-gc-time] collections={n} total_ns={total} max_ns={max}");
    }

    pub(crate) struct Timer(Instant);

    pub(crate) fn start() -> Timer {
        if !REGISTERED.swap(true, Relaxed) {
            // SAFETY: `atexit` is libc's; `report` is a plain `extern "C" fn()`.
            unsafe { atexit(report) };
        }
        Timer(Instant::now())
    }

    pub(crate) fn stop(timer: Timer) {
        let ns = timer.0.elapsed().as_nanos() as u64;
        COLLECTIONS.fetch_add(1, Relaxed);
        TOTAL_NS.fetch_add(ns, Relaxed);
        MAX_NS.fetch_max(ns, Relaxed);
    }

    /// `(collections, total_ns, max_ns)` so far.
    pub(crate) fn totals() -> (u64, u64, u64) {
        (
            COLLECTIONS.load(Relaxed),
            TOTAL_NS.load(Relaxed),
            MAX_NS.load(Relaxed),
        )
    }
}

#[cfg(target_family = "wasm")]
mod imp {
    pub(crate) struct Timer;
    pub(crate) fn start() -> Timer {
        Timer
    }
    pub(crate) fn stop(_timer: Timer) {}
    #[allow(dead_code)]
    pub(crate) fn totals() -> (u64, u64, u64) {
        (0, 0, 0)
    }
}

pub(crate) use imp::*;
