# GC benchmark report

Run 2026-11-03 04:43:22 on VirtualApple @ 2.50GHz (x86_64, 12 cpus); 11 repetitions per cell (plain builds: 3), scale 1.0. Repetitions are interleaved across runtimes.

| runtime | build | archive sha256 (16) |
|---|---|---|
| cheney | timed | `e95cacb28c0a1b0a` |
| cheney | plain | `560c5e65f42cc216` |
| markcompact | timed | `64f9d7ae6dfb0f0c` |
| markcompact | plain | `768c168f03b950dd` |
| generational | timed | `b0c3da9b0d8d2b7a` |
| generational | plain | `7e6d4bf28bf5c002` |
| modified-generational | timed | `b9f19b890fdefc24` |
| modified-generational | plain | `bc22022320c1499c` |

## Validity checks

**V1. Output equals the independently computed expected value; exit codes.** A runtime that runs out of memory exits 137 with no output; that is reported as OOM, not as an error, because a smaller heap is part of the experiment.

| runtime | build | runs | output OK | output wrong | rc 0 | rc 137 (OOM) | other rc |
|---|---|---|---|---|---|---|---|
| cheney | timed | 396 | 374 | 0 | 374 | 22 | 0 |
| cheney | plain | 108 | 102 | 0 | 102 | 6 | 0 |
| markcompact | timed | 396 | 396 | 0 | 396 | 0 | 0 |
| markcompact | plain | 108 | 108 | 0 | 108 | 0 | 0 |
| generational | timed | 396 | 341 | 0 | 341 | 55 | 0 |
| generational | plain | 108 | 93 | 0 | 93 | 15 | 0 |
| modified-generational | timed | 396 | 374 | 0 | 374 | 22 | 0 |
| modified-generational | plain | 108 | 102 | 0 | 102 | 6 | 0 |

Runs with a wrong answer or an unexpected exit code: **none**

**V2. Collection counts are repeatable.** Cells whose count varied between repetitions: **0** of 144. (Collection counts are deterministic for a single-threaded program, so any variation means the harness is broken.)

**V3. GC time never exceeds wall time.** Violations: **0**.

**V4. The longest pause never exceeds total GC time.** Violations: **0**.

**V5. Timer overhead** (median wall, timed vs plain build, summed over cells that have both):

| runtime | plain ms | timed ms | overhead |
|---|---|---|---|
| cheney | 4550 | 4588 | +0.8% |
| markcompact | 9649 | 9639 | -0.1% |
| generational | 4345 | 4365 | +0.5% |
| modified-generational | 4850 | 4932 | +1.7% |

**V6. Noise.** Coefficient of variation of GC time and of wall time across repetitions (timed builds):

| runtime | metric | cells | median CV | 90th pct CV | max CV | cells with CV > 15% |
|---|---|---|---|---|---|---|
| cheney | GC time | 34 | 1.8% | 5.8% | 12.3% | 0 |
| cheney | wall time | 34 | 1.4% | 2.9% | 5.5% | 0 |
| markcompact | GC time | 36 | 2.1% | 4.8% | 20.9% | 1 |
| markcompact | wall time | 36 | 1.5% | 5.6% | 15.3% | 1 |
| generational | GC time | 31 | 2.4% | 5.3% | 10.4% | 0 |
| generational | wall time | 31 | 1.2% | 2.7% | 5.8% | 0 |
| modified-generational | GC time | 34 | 2.7% | 5.2% | 22.0% | 1 |
| modified-generational | wall time | 34 | 1.3% | 2.4% | 20.7% | 1 |

Tiny GC times (under ~1 ms) are dominated by clock resolution and run-to-run jitter; compare totals and ratios on cells with at least a few ms of GC.

## Results

Each cell: median over repetitions. **GC ms** = total time inside `collect`; **max pause** = longest single collection; **GC %** = GC ms / wall ms of the same (timed) runs; **wall** = process wall time (min–max in brackets); **OOM** = exit 137. The **best** column names the runtime with the lowest GC ms; runtimes within max(10 %, 1.5 ms) of it are shown as a tie (`~`). The first collection of a generational runtime costs about 1 ms on its own (see *max pause* on garbage-only programs), which is why differences under 1.5 ms are not ranked.

### churn

garbage only; no live data. The floor: per-collection fixed cost.

**heap: default** (runtime default, 16 MiB)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | generational colls | generational GC ms | generational max pause ms | generational GC % | generational wall ms (min–max) | modified-generational colls | modified-generational GC ms | modified-generational max pause ms | modified-generational GC % | modified-generational wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| churn | 57 | 0.1 | 0.11 | 0% | 154 (151–160) | 28 | 15.5 | 1.09 | 9% | 169 (166–174) | 114 | 1.1 | 1.05 | 1% | 154 (152–158) | 457 | 0.8 | 0.81 | 1% | 158 (156–161) | ~ cheney/modified-generational/generational |

**heap: 8M** (LO_HEAP_SIZE=8388608)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | generational colls | generational GC ms | generational max pause ms | generational GC % | generational wall ms (min–max) | modified-generational colls | modified-generational GC ms | modified-generational max pause ms | modified-generational GC % | modified-generational wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| churn | 114 | 0.1 | 0.12 | 0% | 147 (145–153) | 57 | 15.8 | 0.51 | 10% | 162 (159–167) | 228 | 0.8 | 0.79 | 1% | 149 (147–158) | 915 | 0.6 | 0.54 | 0% | 146 (143–152) | ~ cheney/modified-generational/generational |

### live_list

a linked list stays live (32 B/node) while garbage is churned. Copying collectors pay for the live set on every collection.

**heap: default** (runtime default, 16 MiB)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | generational colls | generational GC ms | generational max pause ms | generational GC % | generational wall ms (min–max) | modified-generational colls | modified-generational GC ms | modified-generational max pause ms | modified-generational GC % | modified-generational wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| live10000 | 59 | 3.0 | 0.15 | 2% | 157 (154–165) | 29 | 31.2 | 2.25 | 17% | 185 (183–237) | 114 | 1.1 | 1.10 | 1% | 156 (153–170) | 458 | 1.1 | 1.02 | 1% | 158 (155–163) | ~ modified-generational/generational |
| live50000 | 70 | 14.8 | 0.31 | 9% | 171 (167–174) | 31 | 98.6 | 4.79 | 39% | 256 (251–267) | 114 | 1.4 | 1.34 | 1% | 157 (152–186) | 459 | 1.2 | 1.08 | 1% | 157 (155–164) | ~ modified-generational/generational |
| live100000 | 92 | 39.1 | 0.56 | 20% | 192 (190–195) | 35 | 214.0 | 8.03 | 58% | 368 (366–381) | 115 | 69.8 | 1.67 | 31% | 226 (221–244) | 460 | 1.5 | 1.13 | 1% | 161 (158–169) | modified-generational |

**heap: 8M** (LO_HEAP_SIZE=8388608)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | generational colls | generational GC ms | generational max pause ms | generational GC % | generational wall ms (min–max) | modified-generational colls | modified-generational GC ms | modified-generational max pause ms | modified-generational GC % | modified-generational wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| live10000 | 123 | 6.1 | 0.15 | 4% | 153 (151–159) | 59 | 45.5 | 1.61 | 23% | 194 (191–197) | 229 | 0.8 | 0.79 | 1% | 151 (150–153) | 916 | 0.8 | 0.72 | 1% | 147 (143–149) | ~ modified-generational/generational |
| live50000 | 185 | 39.0 | 0.32 | 22% | 180 (178–187) | 70 | 200.9 | 4.32 | 58% | 349 (344–548) | 229 | 69.4 | 1.01 | 31% | 221 (218–229) | 918 | 1.0 | 0.75 | 1% | 147 (145–155) | modified-generational |
| live100000 | 482 | 222.3 | 0.61 | 63% | 355 (347–372) | 92 | 523.6 | 7.37 | 79% | 665 (661–758) | OOM | OOM | OOM | OOM | 16 | 921 | 1.3 | 0.76 | 1% | 148 (145–152) | modified-generational |

### live_tree

a complete binary tree stays live while garbage is churned (two pointers per node, so more tracing work per byte).

**heap: default** (runtime default, 16 MiB)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | generational colls | generational GC ms | generational max pause ms | generational GC % | generational wall ms (min–max) | modified-generational colls | modified-generational GC ms | modified-generational max pause ms | modified-generational GC % | modified-generational wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| depth12 | 59 | 3.4 | 0.17 | 2% | 157 (157–160) | 29 | 34.6 | 2.54 | 18% | 189 (186–192) | 114 | 1.1 | 1.04 | 1% | 156 (153–159) | 458 | 1.1 | 1.02 | 1% | 157 (155–162) | ~ generational/modified-generational |
| depth14 | 67 | 13.8 | 0.37 | 8% | 169 (167–171) | 31 | 95.4 | 4.70 | 38% | 250 (246–253) | 114 | 1.2 | 1.20 | 1% | 156 (155–161) | 459 | 1.2 | 1.12 | 1% | 157 (156–160) | ~ generational/modified-generational |
| depth16 | 152 | 121.9 | 1.21 | 45% | 268 (265–274) | 41 | 482.5 | 14.94 | 75% | 639 (627–657) | 115 | 94.0 | 1.70 | 37% | 252 (251–253) | 462 | 2.0 | 1.16 | 1% | 161 (158–164) | modified-generational |

**heap: 8M** (LO_HEAP_SIZE=8388608)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | generational colls | generational GC ms | generational max pause ms | generational GC % | generational wall ms (min–max) | modified-generational colls | modified-generational GC ms | modified-generational max pause ms | modified-generational GC % | modified-generational wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| depth12 | 124 | 6.8 | 0.16 | 4% | 154 (151–163) | 59 | 51.9 | 1.90 | 26% | 199 (198–203) | 229 | 0.8 | 0.75 | 1% | 149 (148–151) | 916 | 0.8 | 0.72 | 1% | 147 (146–151) | ~ generational/modified-generational |
| depth14 | 166 | 33.3 | 0.32 | 19% | 176 (174–177) | 67 | 185.4 | 4.20 | 56% | 333 (329–351) | 229 | 46.2 | 0.91 | 24% | 194 (193–198) | 918 | 1.0 | 0.76 | 1% | 146 (144–149) | modified-generational |
| depth16 | OOM | OOM | OOM | OOM | 16 | 152 | 1693.1 | 14.75 | 93% | 1823 (1808–2187) | OOM | OOM | OOM | OOM | 16 | OOM | OOM | OOM | OOM | 16 | markcompact |

### queue_window

a sliding window of K linked nodes: each node lives for K allocations then dies. Window below the nursery size dies young; above it, nodes are promoted and then die in tenured space, and a dead tenured tail that points at young nodes keeps them alive (nepotism).

**heap: default** (runtime default, 16 MiB)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | generational colls | generational GC ms | generational max pause ms | generational GC % | generational wall ms (min–max) | modified-generational colls | modified-generational GC ms | modified-generational max pause ms | modified-generational GC % | modified-generational wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| window1000 | 22 | 0.2 | 0.07 | 0% | 138 (137–144) | 11 | 6.1 | 1.49 | 4% | 142 (140–145) | 45 | 13.8 | 0.94 | 8% | 162 (160–171) | 183 | 33.3 | 0.98 | 19% | 177 (174–181) | cheney |
| window10000 | 23 | 1.0 | 0.13 | 1% | 140 (139–147) | 11 | 11.0 | 2.12 | 7% | 150 (148–152) | 45 | 14.5 | 1.00 | 9% | 162 (158–166) | 183 | 34.3 | 1.02 | 19% | 178 (174–181) | cheney |
| window30000 | 25 | 3.7 | 0.21 | 3% | 143 (139–146) | 12 | 27.6 | 3.67 | 17% | 167 (163–172) | 45 | 16.5 | 1.08 | 10% | 164 (160–173) | 183 | 37.9 | 1.15 | 21% | 182 (177–184) | cheney |
| window100000 | 36 | 17.2 | 0.56 | 11% | 154 (151–160) | 13 | 75.9 | 7.54 | 36% | 213 (211–225) | 45 | 19.9 | 1.39 | 12% | 166 (165–170) | 183 | 53.7 | 1.16 | 28% | 195 (194–210) | cheney |
| window200000 | 93 | 82.2 | 0.99 | 40% | 208 (206–213) | 17 | 202.7 | 14.11 | 60% | 338 (336–361) | OOM | OOM | OOM | OOM | 20 | 183 | 128.9 | 1.34 | 48% | 271 (267–273) | cheney |

**heap: 8M** (LO_HEAP_SIZE=8388608)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | generational colls | generational GC ms | generational max pause ms | generational GC % | generational wall ms (min–max) | modified-generational colls | modified-generational GC ms | modified-generational max pause ms | modified-generational GC % | modified-generational wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| window1000 | 46 | 0.3 | 0.07 | 0% | 133 (131–138) | 22 | 6.2 | 0.99 | 4% | 139 (136–142) | 91 | 14.1 | 0.64 | 9% | 159 (157–162) | 366 | 33.5 | 0.68 | 20% | 169 (166–299) | cheney |
| window10000 | 49 | 2.1 | 0.13 | 2% | 137 (133–143) | 23 | 16.5 | 1.55 | 11% | 149 (148–155) | 91 | 15.9 | 0.72 | 10% | 160 (159–162) | 366 | 36.0 | 0.72 | 21% | 170 (168–173) | cheney |
| window30000 | 59 | 8.6 | 0.21 | 6% | 140 (137–142) | 25 | 49.6 | 3.04 | 27% | 181 (179–187) | 91 | 19.7 | 0.80 | 12% | 165 (163–169) | 366 | 43.8 | 0.78 | 25% | 178 (176–182) | cheney |
| window100000 | 189 | 89.3 | 0.60 | 42% | 211 (207–214) | 36 | 199.1 | 7.10 | 61% | 328 (325–335) | OOM | OOM | OOM | OOM | 17 | 366 | 131.7 | 0.78 | 49% | 266 (264–269) | cheney |
| window200000 | OOM | OOM | OOM | OOM | 16 | 93 | 1095.9 | 15.79 | 91% | 1200 (1165–1275) | OOM | OOM | OOM | OOM | 17 | OOM | OOM | OOM | OOM | 17 | markcompact |

### old_to_young

M long-lived holder objects are overwritten round-robin with fresh objects: every store is old-to-young, so the write barrier and the remembered set are on the hot path.

**heap: default** (runtime default, 16 MiB)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | generational colls | generational GC ms | generational max pause ms | generational GC % | generational wall ms (min–max) | modified-generational colls | modified-generational GC ms | modified-generational max pause ms | modified-generational GC % | modified-generational wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| holders100 | 11 | 0.1 | 0.07 | 0% | 71 (70–72) | 5 | 3.8 | 1.61 | 5% | 70 (69–74) | 22 | 1.0 | 1.03 | 1% | 109 (106–115) | 91 | 1.0 | 0.95 | 1% | 123 (121–125) | ~ cheney/modified-generational/generational |
| holders10000 | 12 | 1.3 | 0.18 | 2% | 75 (74–76) | 5 | 9.7 | 3.10 | 12% | 78 (75–79) | 23 | 3.1 | 1.16 | 3% | 123 (123–124) | 92 | 8.3 | 1.05 | 6% | 139 (139–141) | cheney |
| holders50000 | 17 | 8.7 | 0.60 | 10% | 86 (82–89) | 6 | 39.7 | 8.35 | 35% | 115 (114–117) | 23 | 14.2 | 1.65 | 9% | 150 (150–156) | 94 | 38.1 | 1.17 | 23% | 168 (168–172) | cheney |

**heap: 8M** (LO_HEAP_SIZE=8388608)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | generational colls | generational GC ms | generational max pause ms | generational GC % | generational wall ms (min–max) | modified-generational colls | modified-generational GC ms | modified-generational max pause ms | modified-generational GC % | modified-generational wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| holders100 | 22 | 0.1 | 0.07 | 0% | 69 (66–70) | 11 | 4.2 | 1.08 | 6% | 69 (69–71) | 45 | 0.8 | 0.72 | 1% | 110 (108–112) | 183 | 0.8 | 0.68 | 1% | 122 (121–126) | ~ cheney/generational/modified-generational |
| holders10000 | 26 | 2.7 | 0.19 | 4% | 74 (74–75) | 12 | 17.5 | 2.44 | 21% | 84 (81–90) | 46 | 5.0 | 0.86 | 4% | 126 (125–129) | 184 | 15.2 | 0.77 | 11% | 143 (142–149) | cheney |
| holders50000 | 68 | 35.0 | 0.64 | 33% | 106 (103–115) | 17 | 102.3 | 7.64 | 59% | 175 (172–179) | 47 | 27.6 | 1.19 | 17% | 159 (156–162) | 188 | 59.2 | 0.79 | 32% | 183 (182–195) | generational |

### deep_roots

recursion D frames deep with a root in every frame, then garbage is churned at the bottom: every collection scans D shadow-stack frames.

**heap: default** (runtime default, 16 MiB)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | generational colls | generational GC ms | generational max pause ms | generational GC % | generational wall ms (min–max) | modified-generational colls | modified-generational GC ms | modified-generational max pause ms | modified-generational GC % | modified-generational wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| depth100 | 11 | 0.1 | 0.06 | 0% | 41 (40–44) | 5 | 3.1 | 1.00 | 7% | 44 (43–47) | 22 | 1.0 | 1.02 | 3% | 41 (39–42) | 91 | 0.9 | 0.87 | 2% | 42 (40–43) | ~ cheney/modified-generational/generational |
| depth500 | 11 | 0.1 | 0.07 | 0% | 41 (41–43) | 5 | 3.2 | 1.01 | 7% | 44 (44–54) | 22 | 1.0 | 1.01 | 3% | 41 (39–46) | 91 | 1.0 | 0.88 | 2% | 42 (41–45) | ~ cheney/modified-generational/generational |
| depth1000 | 11 | 0.1 | 0.07 | 0% | 42 (39–44) | 5 | 3.3 | 1.04 | 7% | 45 (44–48) | 22 | 1.1 | 1.03 | 3% | 42 (39–44) | 91 | 1.1 | 0.91 | 3% | 42 (41–43) | ~ cheney/generational/modified-generational |

**heap: 8M** (LO_HEAP_SIZE=8388608)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | generational colls | generational GC ms | generational max pause ms | generational GC % | generational wall ms (min–max) | modified-generational colls | modified-generational GC ms | modified-generational max pause ms | modified-generational GC % | modified-generational wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| depth100 | 22 | 0.1 | 0.06 | 0% | 39 (38–46) | 11 | 3.2 | 0.42 | 8% | 43 (40–45) | 45 | 0.7 | 0.72 | 2% | 40 (40–41) | 183 | 0.7 | 0.59 | 2% | 40 (37–40) | ~ cheney/modified-generational/generational |
| depth500 | 22 | 0.1 | 0.07 | 0% | 39 (38–42) | 11 | 3.5 | 0.44 | 8% | 43 (42–45) | 45 | 0.8 | 0.73 | 2% | 41 (39–43) | 183 | 0.8 | 0.61 | 2% | 39 (38–44) | ~ cheney/generational/modified-generational |
| depth1000 | 22 | 0.1 | 0.07 | 0% | 39 (39–40) | 11 | 3.7 | 0.47 | 9% | 43 (42–47) | 45 | 0.9 | 0.73 | 2% | 41 (40–42) | 183 | 1.0 | 0.59 | 3% | 40 (37–40) | ~ cheney/generational/modified-generational |

## Summary

Total GC time over every cell where all runtimes completed (so an OOM in one runtime excludes that cell for all). A cell is *decided* when exactly one runtime is within max(10 %, 1.5 ms) of the best GC time; otherwise every runtime within that margin is *tied*.

| runtime | total GC ms | sum of per-cell longest pause ms | cells decided in its favour | cells tied for best | cells clearly behind |
|---|---|---|---|---|---|
| cheney | 363 | 7 | 10 | 10 | 11 |
| markcompact | 1862 | 100 | 0 | 0 | 31 |
| generational | 459 | 32 | 1 | 16 | 14 |
| modified-generational | 414 | 27 | 4 | 16 | 11 |

31 cells counted of 36. OOM cells: cheney:depth16@8M, cheney:window200000@8M, generational:depth16@8M, generational:live100000@8M, generational:window100000@8M, generational:window200000@8M, generational:window200000@default, modified-generational:depth16@8M, modified-generational:window200000@8M.
