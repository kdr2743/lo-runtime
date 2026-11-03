//! GC microbenchmark driver. Builds a small linked-list class on the LO heap,
//! runs a named workload sized relative to `heap_size()`, and prints one
//! parseable `[lo-gc-bench]` summary line for `scripts/bench_gc.py`.
//!
//! Workloads are intentionally sized to **overflow** the heap so collections
//! actually run. A fixed tiny iter count against a 16 MiB heap will often
//! finish with `collections=0` — that is not a meaningful GC benchmark.
//!
//! Usage:
//!   LO_HEAP_SIZE=4194304 cargo run --release --bin gc_bench -- churn
//!   cargo run --release --no-default-features --features gc_mark_compact --bin gc_bench -- mixed
//!
//! Env:
//!   LO_HEAP_SIZE — total heap bytes (default 16 MiB)
//!   LO_GC_BENCH_ITERS — optional absolute override of the workload's scale
//!                       knob (ignores heap-relative sizing when set)

use std::env;
use std::time::Instant;

use lo_runtime::{
    collector_name, gc_metrics_snapshot, heap_size, lo_alloc, lo_gc_collect, lo_gc_write_barrier,
    lo_pop_frame, lo_push_frame, lo_runtime_init, lo_runtime_shutdown, reset_gc_metrics,
    ClassDescriptor, Object, ShadowFrame,
};

/// Linked-list node: 16-byte header + one pointer field at offset 16.
static NODE_PTR_OFFSETS: [u32; 1] = [16];
static NODE_CLASS: ClassDescriptor = ClassDescriptor {
    name: c"Node".as_ptr() as *const u8,
    name_len: 4,
    parent: core::ptr::null(),
    instance_size: 24,
    pointer_offsets: &NODE_PTR_OFFSETS as *const u32,
    pointer_count: 1,
    vtable_size: 0,
    vtable: core::ptr::null(),
};

/// Aligned Node footprint (matches `lo_alloc`'s `align_up(instance_size, 8)`).
const NODE_BYTES: usize = 24;

#[repr(C)]
struct BenchFrame<const N: usize> {
    parent: *mut ShadowFrame,
    num_roots: u32,
    roots: [*mut Object; N],
}

impl<const N: usize> BenchFrame<N> {
    fn new() -> Self {
        Self {
            parent: core::ptr::null_mut(),
            num_roots: N as u32,
            roots: [core::ptr::null_mut(); N],
        }
    }

    fn as_frame(&mut self) -> *mut ShadowFrame {
        self as *mut Self as *mut ShadowFrame
    }
}

unsafe fn store_next(obj: *mut Object, next: *mut Object) {
    // Always go through the barrier so generational remembered-set traffic is
    // exercised under the same call pattern codegen would emit.
    lo_gc_write_barrier(obj, 16, next);
}

unsafe fn load_next(obj: *mut Object) -> *mut Object {
    *((obj as *mut u8).add(16) as *mut *mut Object)
}

/// Walk `index` steps from `head`. `head` must be a live rooted pointer.
unsafe fn nth(head: *mut Object, index: usize) -> *mut Object {
    let mut cur = head;
    for _ in 0..index {
        if cur.is_null() {
            return core::ptr::null_mut();
        }
        cur = load_next(cur);
    }
    cur
}

/// Count nodes reachable from `head`.
unsafe fn chain_len(head: *mut Object) -> usize {
    let mut n = 0usize;
    let mut cur = head;
    while !cur.is_null() {
        n += 1;
        cur = load_next(cur);
    }
    n
}

/// Absolute override via `LO_GC_BENCH_ITERS`, else a heap-relative count.
///
/// `heap_multiples` is how many times the total heap's worth of Nodes to use
/// as the scale knob. Callers that need a fraction pass a fractional multiple.
fn scale_nodes(heap_multiples: f64, minimum: usize) -> usize {
    if let Some(n) = env::var("LO_GC_BENCH_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n: &usize| n > 0)
    {
        return n;
    }
    let heap = heap_size().max(NODE_BYTES);
    let nodes = ((heap as f64 * heap_multiples) / NODE_BYTES as f64).ceil() as usize;
    nodes.max(minimum)
}

/// Mostly short-lived objects: keep one live root, allocate many dead nodes.
///
/// Default scale: **8× the total heap** in dead Nodes. That overflows even
/// mark-compact's full contiguous space several times, and forces many minors
/// on generational (nursery ≈ ¼ heap).
fn workload_churn() {
    let iters = scale_nodes(8.0, 50_000);
    unsafe {
        let mut frame = BenchFrame::<1>::new();
        lo_push_frame(frame.as_frame());
        frame.roots[0] = lo_alloc(&NODE_CLASS);
        for _ in 0..iters {
            let _dead = lo_alloc(&NODE_CLASS);
        }
        assert!(!frame.roots[0].is_null());
        lo_pop_frame();
    }
}

/// One long rooted chain that survives every collection.
///
/// Default: chain ≈ **25% of the heap** (fits Cheney's semispace and gen
/// tenure), then **16 forced collections** so pause cost over a large live
/// set is the signal — not allocation pressure.
fn workload_long_lived() {
    let len = scale_nodes(0.25, 2_000);
    let forced = 16usize;
    unsafe {
        let mut frame = BenchFrame::<2>::new();
        lo_push_frame(frame.as_frame());
        let head = lo_alloc(&NODE_CLASS);
        frame.roots[0] = head;
        frame.roots[1] = head;
        for _ in 1..len {
            let n = lo_alloc(&NODE_CLASS);
            let tip = frame.roots[1];
            store_next(tip, n);
            frame.roots[1] = n;
        }
        // Promote / settle the chain before timing forced collects' steady cost.
        lo_gc_collect();
        for _ in 0..forced {
            lo_gc_collect();
        }
        assert!(chain_len(frame.roots[0]) >= len);
        lo_pop_frame();
    }
}

/// Stable tenured spine + high young churn with old→young barrier stores.
///
/// Critical step: after building the spine we **force a collection** so those
/// nodes move into tenure (generational) / settle in from-space (Cheney) /
/// pack to the base (mark-compact). Only then do old→young stores hit the
/// remembered-set path. Without that, everything stays nursery-local and
/// `barrier_recorded` stays 0.
///
/// Default: spine ≈ 5% of heap; ≈ **2× heap** of young garbage across rounds.
fn workload_mixed() {
    let spine = scale_nodes(0.05, 256);
    // Young allocs total ≈ 2× heap, spread across spine rounds.
    let young_total = scale_nodes(2.0, 10_000);
    let churn_per = (young_total / spine).max(8);
    unsafe {
        // roots[0] = head, roots[1] = current spine node, roots[2] = successor
        // held across the young-churn window.
        let mut frame = BenchFrame::<3>::new();
        lo_push_frame(frame.as_frame());
        let head = lo_alloc(&NODE_CLASS);
        frame.roots[0] = head;
        frame.roots[1] = head;
        for _ in 1..spine {
            let n = lo_alloc(&NODE_CLASS);
            let tip = frame.roots[1];
            store_next(tip, n);
            frame.roots[1] = n;
        }

        // Tenure / settle the spine before young churn.
        lo_gc_collect();

        let len = chain_len(frame.roots[0]);
        assert!(len >= spine, "spine lost during tenure collect");
        for round in 0..len {
            let old = nth(frame.roots[0], round);
            assert!(!old.is_null(), "spine truncated before round {round}");
            frame.roots[1] = old;
            frame.roots[2] = load_next(old);
            for _ in 0..churn_per {
                let young = lo_alloc(&NODE_CLASS);
                let old_now = frame.roots[1];
                // Old→young (after tenure collect): generational records this.
                store_next(old_now, young);
                // Drop the young edge so it dies; keep successor rooted in [2].
                store_next(old_now, core::ptr::null_mut());
            }
            store_next(frame.roots[1], frame.roots[2]);
        }
        assert_eq!(chain_len(frame.roots[0]), len);
        lo_pop_frame();
    }
}

/// Steady live set with garbage bursts + forced collections.
///
/// Default: live chain ≈ 10% of heap; 48 rounds of (≈10% heap garbage +
/// explicit collect). Guarantees collections even when the mutator would
/// otherwise fit in free space.
fn workload_explicit() {
    let live_count = scale_nodes(0.10, 1_000);
    let rounds = 48usize;
    unsafe {
        let mut frame = BenchFrame::<2>::new();
        lo_push_frame(frame.as_frame());
        let head = lo_alloc(&NODE_CLASS);
        frame.roots[0] = head;
        frame.roots[1] = head;
        for _ in 1..live_count {
            let n = lo_alloc(&NODE_CLASS);
            let tip = frame.roots[1];
            store_next(tip, n);
            frame.roots[1] = n;
        }
        lo_gc_collect(); // settle live set
        for _ in 0..rounds {
            for _ in 0..live_count {
                let _dead = lo_alloc(&NODE_CLASS);
            }
            lo_gc_collect();
        }
        assert!(chain_len(frame.roots[0]) >= live_count);
        lo_pop_frame();
    }
}

fn main() {
    let workload = env::args().nth(1).unwrap_or_else(|| "churn".to_string());

    lo_runtime_init();
    reset_gc_metrics();
    let heap = heap_size();
    let wall = Instant::now();

    match workload.as_str() {
        "churn" => workload_churn(),
        "long_lived" => workload_long_lived(),
        "mixed" => workload_mixed(),
        "explicit" => workload_explicit(),
        other => {
            eprintln!("unknown workload '{other}'; expected churn|long_lived|mixed|explicit");
            lo_runtime_shutdown();
            std::process::exit(2);
        }
    }

    let wall_ns = wall.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
    let metrics = gc_metrics_snapshot();
    // Sanity flag for the analysis script: collections should be > 0 for every
    // default-scale workload. A zero here usually means LO_GC_BENCH_ITERS was
    // set too low relative to LO_HEAP_SIZE.
    print!(
        "{}",
        metrics.summary_line(collector_name(), &workload, heap, wall_ns)
    );
    lo_runtime_shutdown();
}
