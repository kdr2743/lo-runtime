# GC benchmarks (Rust)

Compare Cheney, mark-compact, and generational collectors with the same
synthetic workloads.

| Collector | Cargo feature | Layout |
|-----------|---------------|--------|
| Cheney | `gc_cheney` (default) | two semispaces (~50% usable) |
| Mark-compact | `gc_mark_compact` | one contiguous heap (~100% usable) |
| Generational | `gc_generational` | nursery ~¼ + two tenured semispaces |

## Quick start

```sh
python3 scripts/bench_gc.py
```

Default heap is **4 MiB** so heap-relative workloads finish in a reasonable
time but still force collections. For a heavier run:

```sh
python3 scripts/bench_gc.py --trials 5 --heap 16777216
```

Outputs:

- `benchmarks/results/gc-bench-<UTC>.csv` — raw trials  
- `benchmarks/results/gc-bench-<UTC>-analysis.md` — methodology + interpretation  
- `benchmarks/results/latest.csv` and `latest-analysis.md` — copies of the last run  

## How we benchmark (short version)

1. Build `rust` binary `gc_bench` once per collector (`--no-default-features --features …`).
2. Run the same workloads: `churn`, `long_lived`, `mixed`, `explicit`.
3. Workloads allocate 24-byte linked-list `Node`s through the real ABI
   (`lo_alloc`, shadow-stack roots, `lo_gc_write_barrier`, `lo_gc_collect`).
4. Scale is **relative to heap size** by default (e.g. churn allocates ~8× the
   heap in dead Nodes). That is what forces GC. A small fixed `--iters` against
   a large heap often yields `collections=0` — that measures alloc speed, not GC.
5. The analysis file flags cells with no collections and ranks pause/wall where
   GC actually ran.

### Workload cheat sheet

| Workload | What it does | What “good” looks like |
|----------|--------------|------------------------|
| `churn` | 1 live root, ~8× heap dead allocs | Many collections; gen has minors ≫ majors, low pause |
| `long_lived` | Chain ≈ 25% heap, many forced collects | Pause tracks live-set copy/slide cost |
| `mixed` | Spine tenured via forced collect, then young churn + barriers | Gen `barrier_recorded > 0`, young junk die in minors |
| `explicit` | Live ≈ 10% heap, garbage bursts + forced collects | Controlled pause comparison |

## Trust checklist

Before quoting a result:

- [ ] `collections > 0` for every collector on that workload  
- [ ] For generational `mixed`: `barrier_recorded > 0` (spine was actually old)  
- [ ] Same `--heap` for all three collectors  
- [ ] Release build, no poison / heap-checker feature  

If `collections = 0`, **do not** conclude “generational didn’t help” — the GC
never ran.

## Manual single run

```sh
cd rust
LO_HEAP_SIZE=4194304 cargo run --release --bin gc_bench -- churn
LO_HEAP_SIZE=4194304 cargo run --release --no-default-features \
  --features gc_generational --bin gc_bench -- mixed
```

## Feature selection

Exactly one of `gc_cheney`, `gc_mark_compact`, `gc_generational`. Benchmark
builds always use `--no-default-features --features <algo>`.
