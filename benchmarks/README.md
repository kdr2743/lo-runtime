# GC benchmarks (Rust)

Compare the P3-menu collectors, plus the hardened generational "diddy" collector, on one machine:

| Collector | Cargo feature | Source |
|-----------|---------------|--------|
| Cheney (reference) | `gc_cheney` (default) | WS-2 semispace |
| Mark-compact | `gc_mark_compact` | sliding mark-compact |
| Generational | `gc_generational` | nursery + tenured Cheney (the earlier, as-pushed version) |
| Diddy (hardened generational) | `gc_diddy` | the PR #5-#7 collector: large objects allocated directly in tenured space, 1/16 nursery, `gc-debug` heap checker |

## Quick start

From the repo root:

```sh
python3 scripts/bench_gc.py
```

All four collectors are compared by default. Options:

```sh
python3 scripts/bench_gc.py --trials 5 --heap 4194304 --workloads churn,mixed
python3 scripts/bench_gc.py --iters 10000   # scale every workload down/up
python3 scripts/bench_gc.py --collectors cheney,diddy        # compare a subset
python3 scripts/bench_gc.py --heap 1048576,16777216          # sweep several heap sizes
```

Results land in `benchmarks/results/gc-bench-<UTC>.csv` and `benchmarks/results/latest.csv`.

## Manual single run

```sh
cd rust
cargo run --release --bin gc_bench -- churn
cargo run --release --no-default-features --features gc_mark_compact --bin gc_bench -- mixed
cargo run --release --no-default-features --features gc_generational --bin gc_bench -- explicit
cargo run --release --no-default-features --features gc_diddy --bin gc_bench -- long_lived
```

Each run prints one line:

```text
[lo-gc-bench] collector=cheney workload=churn heap=... wall_ns=... gc_pause_ns_sum=... ...
```

## Fairness notes

- Time **release** builds. Do not enable poison / heap-checker paths during timed runs.
- Use the same `LO_HEAP_SIZE` for all three collectors. Usable capacity still differs:
  - Cheney ≈ 50% (one semispace)
  - Mark-compact ≈ 100% of the contiguous heap
  - Generational ≈ nursery (~¼) + one tenured semispace
  - Diddy ≈ nursery (1/16) + one tenured semispace
- Workloads: `churn`, `long_lived`, `mixed`, `explicit` (see `rust/src/bin/gc_bench.rs`).
- The driver reports median wall time / collections across trials.

## Feature selection

Exactly one of `gc_cheney`, `gc_mark_compact`, `gc_generational`, `gc_diddy` must be enabled.
Default consumers keep Cheney via `default = ["gc_cheney"]`. Benchmark builds always
pass `--no-default-features --features <algo>`.

## Limits of these workloads

At the default defaults each workload runs 1-3 ms and, at a 16 MiB heap, `churn` and `mixed` do
zero or one collection, so wall-time differences between collectors are mostly process noise
and the first-collection cost (~1 ms for the generational collectors). Use a small `--heap`
to force collections, and see `bench/` on the `p3/phase-2-gc-bench` branch for a larger suite
that runs compiled LO programs, checks their output, and times the collector from inside.

`gc_diddy` also accepts the `gc-debug` and `gc-time` cargo features
(`--features gc_diddy,gc-debug`); the other variants ignore them.
