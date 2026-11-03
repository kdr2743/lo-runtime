# GC benchmark analysis

Generated: 2026-11-03 04:31:18Z
Heap (`LO_HEAP_SIZE`): **4194304** bytes
Trials per cell: **3**
Scale: **heap-relative defaults** in `rust/src/bin/gc_bench.rs` (churn ≈ 8× heap of dead Nodes, etc.)

## How we benchmarked

We do **not** run LO source programs. The driver builds the Rust `gc_bench` binary three times (Cheney, mark-compact, generational via cargo features) and runs the same synthetic linked-list workloads against each. Objects are 24-byte `Node`s with one pointer field; roots go through the shadow stack; pointer stores go through `lo_gc_write_barrier`.

Usable heap differs by collector even at the same `LO_HEAP_SIZE`:

| Collector | Approx. usable for new allocs |
|-----------|-------------------------------|
| Cheney | ~50% (one semispace) |
| Mark-compact | ~100% (contiguous) |
| Generational | nursery ~¼ for young allocs; tenure for survivors |

### Workload intent

- **`churn`** — Short-lived garbage only (one live root). Expect many collections; generational should win on pause time via cheap minors.
- **`long_lived`** — Large rooted chain + forced collections. Expect copying collectors to pay per live byte; mark-compact slides in place but its mark+repair pass is often slower here.
- **`mixed`** — Tenured spine + young churn + write-barrier stores. Expect generational barrier_recorded > 0 and minors reclaiming young junk; Cheney/mark-compact do full collections when space fills.
- **`explicit`** — Steady live set, garbage bursts, forced collects. Isolates pause cost with a controlled live fraction.

## Results (medians over non-OOM trials)

| Workload | Collector | Wall ms | GC pause ms | Collections | Minors | Majors | Barrier recorded | Promoted | Notes |
|----------|-----------|--------:|------------:|-------------:|-------:|-------:|-----------------:|---------:|-------|
| churn | cheney | 3.74 | 0.0024 | 16 | 0 | 0 | 0 | 0 | — |
| churn | mark_compact | 3.94 | 0.61 | 8 | 0 | 0 | 0 | 0 | — |
| churn | generational | 3.42 | 0.0093 | 32 | 32 | 0 | 0 | 1 | minors OK |
| long_lived | cheney | 3.87 | 3.65 | 17 | 0 | 0 | 0 | 0 | — |
| long_lived | mark_compact | 24.54 | 24.37 | 17 | 0 | 0 | 0 | 0 | — |
| long_lived | generational | 4.15 | 3.90 | 18 | 1 | 17 | 1 | 43690 | — |
| mixed | cheney | 44.62 | 0.22 | 5 | 0 | 0 | 0 | 0 | — |
| mixed | mark_compact | 37.68 | 1.12 | 3 | 0 | 0 | 0 | 0 | — |
| mixed | generational | 45.89 | 0.09 | 8 | 7 | 1 | 340821 | 0 | — |
| explicit | cheney | 5.95 | 3.98 | 49 | 0 | 0 | 0 | 0 | — |
| explicit | mark_compact | 31.46 | 29.53 | 49 | 0 | 0 | 0 | 0 | — |
| explicit | generational | 6.03 | 4.06 | 49 | 0 | 49 | 0 | 0 | — |

## What worked / what didn't (reading the numbers)

- **`churn` GC pause:** `cheney` lowest (0.0024 ms), `mark_compact` highest (0.61 ms).
- **`churn` wall time:** `generational` fastest (3.42 ms).
- **`churn` why:** generational should prefer many cheap minors over full-heap copies; check `minors` ≫ `majors` and pause sum vs Cheney.
- **`long_lived` GC pause:** `cheney` lowest (3.65 ms), `mark_compact` highest (24.37 ms).
- **`long_lived` wall time:** `cheney` fastest (3.87 ms).
- **`long_lived` why:** almost all bytes are live, so copying (Cheney / gen major) moves the whole chain every forced collect; mark-compact still walks+slides every live object and often loses on pause here despite not needing a to-space.
- **`mixed` GC pause:** `generational` lowest (0.09 ms), `mark_compact` highest (1.12 ms).
- **`mixed` wall time:** `mark_compact` fastest (37.68 ms).
- **`mixed` why:** `barrier_recorded` > 0 confirms old→young stores hit the remembered set; young garbage should die in minors without rewriting the tenured spine.
- **`explicit` GC pause:** `cheney` lowest (3.98 ms), `mark_compact` highest (29.53 ms).
- **`explicit` wall time:** `cheney` fastest (5.95 ms).
- **`explicit` why:** live fraction is fixed and collects are forced, so pause differences are mostly algorithm overhead per live set, not “did we fill the heap.”

