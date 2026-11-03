//! GC microbenchmark driver. Builds a small linked-list class on the LO heap,
//! runs a named workload, and prints one parseable `[lo-gc-bench]` summary line
//! for `scripts/bench_gc.py`.
//!
//! Usage:
//!   LO_HEAP_SIZE=16777216 cargo run --release --bin gc_bench -- churn
//!   cargo run --release --no-default-features --features gc_mark_compact --bin gc_bench -- mixed
//!
//! Env:
//!   LO_HEAP_SIZE — total heap bytes (default 16 MiB, same as the runtime)
//!   LO_GC_BENCH_ITERS — workload iteration scale (default depends on workload)

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

fn parse_iters(default: usize) -> usize {
    env::var("LO_GC_BENCH_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(default)
}

/// Mostly short-lived objects: keep one live root, allocate many dead nodes.
fn workload_churn() {
    let iters = parse_iters(200_000);
    unsafe {
        let mut frame = BenchFrame::<1>::new();
        lo_push_frame(frame.as_frame());
        frame.roots[0] = lo_alloc(&NODE_CLASS);
        for _ in 0..iters {
            let _dead = lo_alloc(&NODE_CLASS);
        }
        // Keep the live root observable after pressure (and silence unused-assign).
        assert!(!frame.roots[0].is_null());
        lo_pop_frame();
    }
}

/// One long rooted chain that survives every collection.
fn workload_long_lived() {
    let len = parse_iters(50_000);
    unsafe {
        let mut frame = BenchFrame::<2>::new();
        lo_push_frame(frame.as_frame());
        let head = lo_alloc(&NODE_CLASS);
        frame.roots[0] = head;
        frame.roots[1] = head; // tip root — updated as we extend
        for _ in 1..len {
            let n = lo_alloc(&NODE_CLASS);
            let tip = frame.roots[1];
            store_next(tip, n);
            frame.roots[1] = n;
        }
        for _ in 0..8 {
            lo_gc_collect();
        }
        // Touch the chain so the compiler cannot DCE the live set.
        assert!(chain_len(frame.roots[0]) >= len);
        lo_pop_frame();
    }
}

/// Stable spine of live nodes plus a high rate of short-lived young objects,
/// with old→young stores through the write barrier.
fn workload_mixed() {
    let spine = parse_iters(2_000);
    let churn_per = 40;
    unsafe {
        // roots[0] = head, roots[1] = current spine node, roots[2] = its
        // successor (held across the young-churn window so nulling next does
        // not lose the rest of the chain to the collector).
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
        let len = chain_len(frame.roots[0]);
        for round in 0..len {
            let old = nth(frame.roots[0], round);
            assert!(!old.is_null(), "spine truncated before round {round}");
            frame.roots[1] = old;
            frame.roots[2] = load_next(old);
            for _ in 0..churn_per {
                let young = lo_alloc(&NODE_CLASS);
                let old_now = frame.roots[1];
                store_next(old_now, young);
                store_next(old_now, core::ptr::null_mut());
            }
            store_next(frame.roots[1], frame.roots[2]);
        }
        assert_eq!(chain_len(frame.roots[0]), len);
        lo_pop_frame();
    }
}

/// Steady live set with periodic explicit collections.
fn workload_explicit() {
    let live_count = parse_iters(5_000);
    let rounds = 32;
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
    print!(
        "{}",
        metrics.summary_line(collector_name(), &workload, heap, wall_ns)
    );
    lo_runtime_shutdown();
}
