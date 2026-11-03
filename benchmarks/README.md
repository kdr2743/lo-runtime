# GC benchmarks (Rust)

Compare the three P3-menu collectors on one machine:

| Collector | Cargo feature | Source |
|-----------|---------------|--------|
| Cheney (reference) | `gc_cheney` (default) | WS-2 semispace |
| Mark-compact | `gc_mark_compact` | sliding mark-compact |
| Generational | `gc_generational` | nursery + tenured Cheney |

## Quick start

From the repo root:

```sh
python3 scripts/bench_gc.py
```

Options:

```sh
python3 scripts/bench_gc.py --trials 5 --heap 4194304 --workloads churn,mixed
python3 scripts/bench_gc.py --iters 10000   # scale every workload down/up
```

Results land in `benchmarks/results/gc-bench-<UTC>.csv` and `benchmarks/results/latest.csv`.

## Manual single run

```sh
cd rust
cargo run --release --bin gc_bench -- churn
cargo run --release --no-default-features --features gc_mark_compact --bin gc_bench -- mixed
cargo run --release --no-default-features --features gc_generational --bin gc_bench -- explicit
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
- Workloads: `churn`, `long_lived`, `mixed`, `explicit` (see `rust/src/bin/gc_bench.rs`).
- The driver reports median wall time / collections across trials.

## Feature selection

Exactly one of `gc_cheney`, `gc_mark_compact`, `gc_generational` must be enabled.
Default consumers keep Cheney via `default = ["gc_cheney"]`. Benchmark builds always
pass `--no-default-features --features <algo>`.
