# GC benchmark report

Run 2026-11-03 02:10:10 on VirtualApple @ 2.50GHz (x86_64, 12 cpus); 11 repetitions per cell (plain builds: 3), scale 1.0. Repetitions are interleaved across runtimes.

| runtime | build | archive sha256 (16) |
|---|---|---|
| cheney | timed | `e95cacb28c0a1b0a` |
| cheney | plain | `560c5e65f42cc216` |
| pushed | timed | `480329aae9f6a507` |
| pushed | plain | `6e90e730693efa75` |
| mine | timed | `b9f19b890fdefc24` |
| mine | plain | `bc22022320c1499c` |

## Validity checks

**V1. Output equals the independently computed expected value; exit codes.** A runtime that runs out of memory exits 137 with no output; that is reported as OOM, not as an error, because a smaller heap is part of the experiment.

| runtime | build | runs | output OK | output wrong | rc 0 | rc 137 (OOM) | other rc |
|---|---|---|---|---|---|---|---|
| cheney | timed | 396 | 374 | 0 | 374 | 22 | 0 |
| cheney | plain | 108 | 102 | 0 | 102 | 6 | 0 |
| pushed | timed | 396 | 341 | 0 | 341 | 55 | 0 |
| pushed | plain | 108 | 93 | 0 | 93 | 15 | 0 |
| mine | timed | 396 | 374 | 0 | 374 | 22 | 0 |
| mine | plain | 108 | 102 | 0 | 102 | 6 | 0 |

Runs with a wrong answer or an unexpected exit code: **none**

**V2. Collection counts are repeatable.** Cells whose count varied between repetitions: **0** of 108. (Collection counts are deterministic for a single-threaded program, so any variation means the harness is broken.)

**V3. GC time never exceeds wall time.** Violations: **0**.

**V4. The longest pause never exceeds total GC time.** Violations: **0**.

**V5. Timer overhead** (median wall, timed vs plain build, summed over cells that have both):

| runtime | plain ms | timed ms | overhead |
|---|---|---|---|
| cheney | 4629 | 4626 | -0.1% |
| pushed | 4421 | 4420 | -0.0% |
| mine | 4950 | 4989 | +0.8% |

**V6. Noise.** Coefficient of variation of GC time and of wall time across repetitions (timed builds):

| runtime | metric | cells | median CV | 90th pct CV | max CV | cells with CV > 15% |
|---|---|---|---|---|---|---|
| cheney | GC time | 34 | 2.1% | 9.3% | 66.3% | 2 |
| cheney | wall time | 34 | 1.5% | 4.6% | 130.6% | 1 |
| pushed | GC time | 31 | 3.5% | 11.8% | 16.6% | 1 |
| pushed | wall time | 31 | 2.3% | 6.8% | 13.1% | 0 |
| mine | GC time | 34 | 3.4% | 7.2% | 66.4% | 2 |
| mine | wall time | 34 | 2.2% | 5.4% | 96.6% | 1 |

Tiny GC times (under ~1 ms) are dominated by clock resolution and run-to-run jitter; compare totals and ratios on cells with at least a few ms of GC.

## Results

Each cell: median over repetitions. **GC ms** = total time inside `collect`; **max pause** = longest single collection; **GC %** = GC ms / wall ms of the same (timed) runs; **wall** = process wall time (min–max in brackets); **OOM** = exit 137. The **best** column names the runtime with the lowest GC ms; runtimes within max(10 %, 1.5 ms) of it are shown as a tie (`~`). The first collection of a generational runtime costs about 1 ms on its own (see *max pause* on garbage-only programs), which is why differences under 1.5 ms are not ranked.

### churn

garbage only; no live data. The floor: per-collection fixed cost.

**heap: default** (runtime default, 16 MiB)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| churn | 57 | 0.1 | 0.12 | 0% | 154 (152–161) | 114 | 1.1 | 1.12 | 1% | 157 (154–161) | 457 | 0.9 | 0.87 | 1% | 160 (154–166) | ~ cheney/mine/pushed |

**heap: 8M** (LO_HEAP_SIZE=8388608)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| churn | 114 | 0.1 | 0.12 | 0% | 147 (143–153) | 228 | 0.8 | 0.83 | 1% | 152 (149–154) | 915 | 0.6 | 0.57 | 0% | 146 (144–150) | ~ cheney/mine/pushed |

### live_list

a linked list stays live (32 B/node) while garbage is churned. Copying collectors pay for the live set on every collection.

**heap: default** (runtime default, 16 MiB)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| live10000 | 59 | 3.1 | 0.16 | 2% | 163 (158–1521) | 114 | 1.2 | 1.16 | 1% | 164 (154–228) | 458 | 1.3 | 1.12 | 1% | 171 (158–1067) | ~ pushed/mine |
| live50000 | 70 | 15.1 | 0.31 | 9% | 171 (168–177) | 114 | 1.4 | 1.36 | 1% | 157 (153–165) | 459 | 1.3 | 1.14 | 1% | 159 (155–188) | ~ mine/pushed |
| live100000 | 92 | 38.9 | 0.55 | 20% | 192 (188–194) | 115 | 69.9 | 1.67 | 31% | 227 (222–230) | 460 | 1.5 | 1.13 | 1% | 160 (158–164) | mine |

**heap: 8M** (LO_HEAP_SIZE=8388608)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| live10000 | 123 | 6.3 | 0.15 | 4% | 157 (154–190) | 229 | 0.9 | 0.84 | 1% | 153 (150–161) | 916 | 0.8 | 0.75 | 1% | 150 (147–159) | ~ mine/pushed |
| live50000 | 185 | 39.7 | 0.32 | 22% | 183 (181–191) | 229 | 69.8 | 1.05 | 32% | 219 (217–223) | 918 | 1.0 | 0.78 | 1% | 149 (147–151) | mine |
| live100000 | 482 | 222.1 | 0.65 | 62% | 356 (351–364) | OOM | OOM | OOM | OOM | 16 | 921 | 1.3 | 0.77 | 1% | 149 (146–159) | mine |

### live_tree

a complete binary tree stays live while garbage is churned (two pointers per node, so more tracing work per byte).

**heap: default** (runtime default, 16 MiB)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| depth12 | 59 | 3.4 | 0.17 | 2% | 159 (156–164) | 114 | 1.1 | 1.06 | 1% | 157 (153–163) | 458 | 1.1 | 1.02 | 1% | 158 (154–165) | ~ mine/pushed |
| depth14 | 67 | 14.0 | 0.38 | 8% | 172 (168–175) | 114 | 1.3 | 1.24 | 1% | 160 (156–162) | 459 | 1.2 | 1.14 | 1% | 159 (157–187) | ~ mine/pushed |
| depth16 | 152 | 122.4 | 1.20 | 45% | 270 (267–280) | 115 | 94.4 | 1.74 | 37% | 255 (252–263) | 462 | 1.9 | 1.12 | 1% | 163 (160–165) | mine |

**heap: 8M** (LO_HEAP_SIZE=8388608)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| depth12 | 124 | 6.9 | 0.16 | 4% | 155 (153–161) | 229 | 0.8 | 0.77 | 1% | 152 (150–158) | 916 | 0.8 | 0.74 | 1% | 149 (146–153) | ~ pushed/mine |
| depth14 | 166 | 33.4 | 0.32 | 19% | 177 (173–179) | 229 | 46.4 | 0.93 | 23% | 197 (195–245) | 918 | 1.0 | 0.76 | 1% | 148 (146–151) | mine |
| depth16 | OOM | OOM | OOM | OOM | 16 | OOM | OOM | OOM | OOM | 16 | OOM | OOM | OOM | OOM | 16 | — |

### queue_window

a sliding window of K linked nodes: each node lives for K allocations then dies. Window below the nursery size dies young; above it, nodes are promoted and then die in tenured space, and a dead tenured tail that points at young nodes keeps them alive (nepotism).

**heap: default** (runtime default, 16 MiB)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| window1000 | 22 | 0.2 | 0.07 | 0% | 138 (137–141) | 45 | 13.7 | 0.94 | 8% | 162 (158–165) | 183 | 33.4 | 0.96 | 19% | 177 (174–179) | cheney |
| window10000 | 23 | 1.0 | 0.13 | 1% | 142 (137–151) | 45 | 14.8 | 1.01 | 9% | 166 (160–198) | 183 | 34.7 | 1.03 | 19% | 180 (174–192) | cheney |
| window30000 | 25 | 3.7 | 0.22 | 3% | 144 (141–148) | 45 | 16.7 | 1.09 | 10% | 165 (163–170) | 183 | 38.1 | 1.15 | 21% | 181 (179–183) | cheney |
| window100000 | 36 | 17.3 | 0.56 | 11% | 156 (152–166) | 45 | 20.2 | 1.43 | 12% | 169 (165–193) | 183 | 54.3 | 1.19 | 27% | 200 (195–209) | cheney |
| window200000 | 93 | 82.6 | 0.98 | 40% | 208 (206–213) | OOM | OOM | OOM | OOM | 21 | 183 | 129.5 | 1.36 | 47% | 273 (271–277) | cheney |

**heap: 8M** (LO_HEAP_SIZE=8388608)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| window1000 | 46 | 0.3 | 0.07 | 0% | 133 (130–135) | 91 | 14.4 | 0.64 | 9% | 159 (157–175) | 366 | 33.5 | 0.67 | 20% | 168 (165–177) | cheney |
| window10000 | 49 | 2.2 | 0.13 | 2% | 137 (134–147) | 91 | 16.4 | 0.73 | 10% | 162 (160–194) | 366 | 36.6 | 0.76 | 21% | 176 (169–204) | cheney |
| window30000 | 59 | 8.6 | 0.21 | 6% | 140 (137–142) | 91 | 19.6 | 0.79 | 12% | 165 (163–170) | 366 | 43.9 | 0.78 | 25% | 178 (176–180) | cheney |
| window100000 | 189 | 89.5 | 0.62 | 42% | 211 (209–215) | OOM | OOM | OOM | OOM | 17 | 366 | 134.0 | 0.79 | 49% | 271 (266–277) | cheney |
| window200000 | OOM | OOM | OOM | OOM | 16 | OOM | OOM | OOM | OOM | 16 | OOM | OOM | OOM | OOM | 17 | — |

### old_to_young

M long-lived holder objects are overwritten round-robin with fresh objects: every store is old-to-young, so the write barrier and the remembered set are on the hot path.

**heap: default** (runtime default, 16 MiB)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| holders100 | 11 | 0.1 | 0.07 | 0% | 72 (70–72) | 22 | 1.1 | 1.03 | 1% | 109 (107–124) | 91 | 1.0 | 0.97 | 1% | 124 (119–164) | ~ cheney/mine/pushed |
| holders10000 | 12 | 1.3 | 0.19 | 2% | 75 (74–77) | 23 | 3.2 | 1.17 | 3% | 124 (123–167) | 92 | 8.4 | 1.09 | 6% | 140 (139–152) | cheney |
| holders50000 | 17 | 8.7 | 0.59 | 10% | 86 (84–88) | 23 | 14.3 | 1.67 | 9% | 152 (150–161) | 94 | 38.4 | 1.17 | 23% | 170 (168–185) | cheney |

**heap: 8M** (LO_HEAP_SIZE=8388608)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| holders100 | 22 | 0.1 | 0.07 | 0% | 70 (68–80) | 45 | 0.8 | 0.74 | 1% | 114 (107–118) | 183 | 0.9 | 0.70 | 1% | 122 (120–127) | ~ cheney/pushed/mine |
| holders10000 | 26 | 2.7 | 0.19 | 4% | 75 (74–77) | 46 | 5.0 | 0.87 | 4% | 126 (126–127) | 184 | 15.4 | 0.79 | 11% | 144 (142–148) | cheney |
| holders50000 | 68 | 35.0 | 0.65 | 33% | 106 (104–111) | 47 | 27.7 | 1.21 | 17% | 160 (159–165) | 188 | 59.8 | 0.79 | 32% | 184 (183–208) | pushed |

### deep_roots

recursion D frames deep with a root in every frame, then garbage is churned at the bottom: every collection scans D shadow-stack frames.

**heap: default** (runtime default, 16 MiB)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| depth100 | 11 | 0.1 | 0.07 | 0% | 42 (41–47) | 22 | 1.0 | 1.03 | 3% | 42 (40–44) | 91 | 0.9 | 0.90 | 2% | 42 (40–44) | ~ cheney/mine/pushed |
| depth500 | 11 | 0.1 | 0.07 | 0% | 41 (41–44) | 22 | 1.1 | 1.05 | 3% | 42 (39–44) | 91 | 1.0 | 0.91 | 2% | 43 (42–45) | ~ cheney/mine/pushed |
| depth1000 | 11 | 0.1 | 0.07 | 0% | 42 (41–44) | 22 | 1.1 | 1.04 | 3% | 43 (40–44) | 91 | 1.1 | 0.90 | 3% | 43 (42–45) | ~ cheney/pushed/mine |

**heap: 8M** (LO_HEAP_SIZE=8388608)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| depth100 | 22 | 0.1 | 0.07 | 0% | 41 (38–44) | 45 | 0.8 | 0.74 | 2% | 40 (40–44) | 183 | 0.7 | 0.60 | 2% | 40 (38–42) | ~ cheney/mine/pushed |
| depth500 | 22 | 0.1 | 0.07 | 0% | 40 (39–42) | 45 | 0.8 | 0.76 | 2% | 41 (39–44) | 183 | 0.8 | 0.62 | 2% | 40 (39–42) | ~ cheney/pushed/mine |
| depth1000 | 22 | 0.1 | 0.07 | 0% | 40 (39–61) | 45 | 0.9 | 0.76 | 2% | 43 (40–60) | 183 | 1.1 | 0.63 | 3% | 41 (39–44) | ~ cheney/pushed/mine |

## Summary

Total GC time over every cell where all runtimes completed (so an OOM in one runtime excludes that cell for all). A cell is *decided* when exactly one runtime is within max(10 %, 1.5 ms) of the best GC time; otherwise every runtime within that margin is *tied*.

| runtime | total GC ms | sum of per-cell longest pause ms | cells decided in its favour | cells tied for best | cells clearly behind |
|---|---|---|---|---|---|
| cheney | 365 | 7 | 10 | 10 | 11 |
| pushed | 462 | 32 | 1 | 16 | 14 |
| mine | 418 | 28 | 4 | 16 | 11 |

31 cells counted of 36. OOM cells: cheney:depth16@8M, cheney:window200000@8M, mine:depth16@8M, mine:window200000@8M, pushed:depth16@8M, pushed:live100000@8M, pushed:window100000@8M, pushed:window200000@8M, pushed:window200000@default.
