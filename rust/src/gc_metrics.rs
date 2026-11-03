//! Shared GC / allocator metrics for cross-collector benchmarking.
//!
//! Counters live in thread-local cells (the runtime is single-threaded). They
//! are cheap enough for release timed runs — no poison, no heap walker. The
//! bench binary prints a parseable `[lo-gc-bench]` summary from [`snapshot`].

use std::cell::Cell;
use std::time::Instant;

/// Which kind of collection just finished (generational splits minor/major;
/// Cheney and mark-compact always report [`CollectKind::Full`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // Minor/Major are only constructed by the generational variant.
pub enum CollectKind {
    Full,
    Minor,
    Major,
}

/// Point-in-time metrics since the last [`reset`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GcMetrics {
    pub collections: u64,
    pub minors: u64,
    pub majors: u64,
    pub gc_pause_ns_sum: u64,
    pub gc_pause_ns_max: u64,
    pub copied_objects: u64,
    pub copied_bytes: u64,
    pub reclaimed_bytes: u64,
    pub peak_live: u64,
    pub allocs: u64,
    pub alloc_bytes: u64,
    pub barrier_calls: u64,
    pub barrier_recorded: u64,
    pub promoted_objects: u64,
}

impl GcMetrics {
    /// One `key=value` line for the bench harness / driver script.
    pub fn summary_line(
        &self,
        collector: &str,
        workload: &str,
        heap: usize,
        wall_ns: u64,
    ) -> String {
        format!(
            "[lo-gc-bench] collector={collector} workload={workload} heap={heap} \
             wall_ns={wall_ns} gc_pause_ns_sum={} gc_pause_ns_max={} collections={} \
             minors={} majors={} copied={} copied_bytes={} reclaimed={} peak_live={} \
             allocs={} alloc_bytes={} barrier_calls={} barrier_recorded={} promoted={}\n",
            self.gc_pause_ns_sum,
            self.gc_pause_ns_max,
            self.collections,
            self.minors,
            self.majors,
            self.copied_objects,
            self.copied_bytes,
            self.reclaimed_bytes,
            self.peak_live,
            self.allocs,
            self.alloc_bytes,
            self.barrier_calls,
            self.barrier_recorded,
            self.promoted_objects,
        )
    }
}

struct Counters {
    collections: Cell<u64>,
    minors: Cell<u64>,
    majors: Cell<u64>,
    gc_pause_ns_sum: Cell<u64>,
    gc_pause_ns_max: Cell<u64>,
    copied_objects: Cell<u64>,
    copied_bytes: Cell<u64>,
    reclaimed_bytes: Cell<u64>,
    peak_live: Cell<u64>,
    allocs: Cell<u64>,
    alloc_bytes: Cell<u64>,
    barrier_calls: Cell<u64>,
    barrier_recorded: Cell<u64>,
    promoted_objects: Cell<u64>,
}

impl Counters {
    const fn new() -> Self {
        Self {
            collections: Cell::new(0),
            minors: Cell::new(0),
            majors: Cell::new(0),
            gc_pause_ns_sum: Cell::new(0),
            gc_pause_ns_max: Cell::new(0),
            copied_objects: Cell::new(0),
            copied_bytes: Cell::new(0),
            reclaimed_bytes: Cell::new(0),
            peak_live: Cell::new(0),
            allocs: Cell::new(0),
            alloc_bytes: Cell::new(0),
            barrier_calls: Cell::new(0),
            barrier_recorded: Cell::new(0),
            promoted_objects: Cell::new(0),
        }
    }

    fn reset(&self) {
        self.collections.set(0);
        self.minors.set(0);
        self.majors.set(0);
        self.gc_pause_ns_sum.set(0);
        self.gc_pause_ns_max.set(0);
        self.copied_objects.set(0);
        self.copied_bytes.set(0);
        self.reclaimed_bytes.set(0);
        self.peak_live.set(0);
        self.allocs.set(0);
        self.alloc_bytes.set(0);
        self.barrier_calls.set(0);
        self.barrier_recorded.set(0);
        self.promoted_objects.set(0);
    }

    fn snapshot(&self) -> GcMetrics {
        GcMetrics {
            collections: self.collections.get(),
            minors: self.minors.get(),
            majors: self.majors.get(),
            gc_pause_ns_sum: self.gc_pause_ns_sum.get(),
            gc_pause_ns_max: self.gc_pause_ns_max.get(),
            copied_objects: self.copied_objects.get(),
            copied_bytes: self.copied_bytes.get(),
            reclaimed_bytes: self.reclaimed_bytes.get(),
            peak_live: self.peak_live.get(),
            allocs: self.allocs.get(),
            alloc_bytes: self.alloc_bytes.get(),
            barrier_calls: self.barrier_calls.get(),
            barrier_recorded: self.barrier_recorded.get(),
            promoted_objects: self.promoted_objects.get(),
        }
    }
}

thread_local! {
    static COUNTERS: Counters = const { Counters::new() };
}

/// Active collector name for summary lines (matches cargo feature).
pub fn collector_name() -> &'static str {
    #[cfg(feature = "gc_cheney")]
    {
        "cheney"
    }
    #[cfg(feature = "gc_mark_compact")]
    {
        "mark_compact"
    }
    #[cfg(feature = "gc_generational")]
    {
        "generational"
    }
    #[cfg(feature = "gc_diddy")]
    {
        "diddy"
    }
}

/// Zero all counters. Call at the start of each bench trial.
pub fn reset() {
    COUNTERS.with(Counters::reset);
}

/// Current counter snapshot.
pub fn snapshot() -> GcMetrics {
    COUNTERS.with(Counters::snapshot)
}

/// Record a successful bump allocation of `bytes` (object size handed out).
#[inline]
pub fn record_alloc(bytes: usize) {
    COUNTERS.with(|c| {
        c.allocs.set(c.allocs.get() + 1);
        c.alloc_bytes
            .set(c.alloc_bytes.get().saturating_add(bytes as u64));
    });
}

/// Record a write-barrier call. `recorded` is true when an old→young edge was
/// entered into the remembered set (generational only).
#[inline]
pub fn record_barrier(recorded: bool) {
    COUNTERS.with(|c| {
        c.barrier_calls.set(c.barrier_calls.get() + 1);
        if recorded {
            c.barrier_recorded.set(c.barrier_recorded.get() + 1);
        }
    });
}

/// Start timing a collection. Pair with [`end_collect`].
#[inline]
pub fn begin_collect() -> Instant {
    Instant::now()
}

/// Finish a collection: pause time, reclaim, peak live, and optional copy stats
/// already accumulated via [`record_copy`] / [`record_promote`] during the cycle.
pub fn end_collect(start: Instant, kind: CollectKind, used_before: usize, used_after: usize) {
    let pause = start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
    let reclaimed = used_before.saturating_sub(used_after) as u64;
    COUNTERS.with(|c| {
        c.collections.set(c.collections.get() + 1);
        match kind {
            // Cheney / mark-compact: only `collections` is meaningful.
            CollectKind::Full => {}
            CollectKind::Minor => {
                c.minors.set(c.minors.get() + 1);
            }
            CollectKind::Major => {
                c.majors.set(c.majors.get() + 1);
            }
        }
        c.gc_pause_ns_sum
            .set(c.gc_pause_ns_sum.get().saturating_add(pause));
        if pause > c.gc_pause_ns_max.get() {
            c.gc_pause_ns_max.set(pause);
        }
        c.reclaimed_bytes
            .set(c.reclaimed_bytes.get().saturating_add(reclaimed));
        let live = used_after as u64;
        if live > c.peak_live.get() {
            c.peak_live.set(live);
        }
    });
}

/// Record one evacuated / compacted object of `bytes`.
#[inline]
pub fn record_copy(bytes: usize) {
    COUNTERS.with(|c| {
        c.copied_objects.set(c.copied_objects.get() + 1);
        c.copied_bytes
            .set(c.copied_bytes.get().saturating_add(bytes as u64));
    });
}

/// Record a nursery→tenure promotion (generational minors).
#[inline]
#[allow(dead_code)] // Used by the generational collector.
pub fn record_promote() {
    COUNTERS.with(|c| {
        c.promoted_objects.set(c.promoted_objects.get() + 1);
    });
}
