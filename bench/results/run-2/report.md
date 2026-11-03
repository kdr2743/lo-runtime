# GC benchmark report

Run 2026-11-03 02:15:13 on VirtualApple @ 2.50GHz (x86_64, 12 cpus); 11 repetitions per cell (plain builds: 3), scale 1.0. Repetitions are interleaved across runtimes.

| runtime | build | archive sha256 (16) |
|---|---|---|
| cheney | timed | `e95cacb28c0a1b0a` |
| cheney | plain | `560c5e65f42cc216` |
| markcompact | timed | `64f9d7ae6dfb0f0c` |
| markcompact | plain | `768c168f03b950dd` |
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
| markcompact | timed | 396 | 396 | 0 | 396 | 0 | 0 |
| markcompact | plain | 108 | 108 | 0 | 108 | 0 | 0 |
| pushed | timed | 396 | 341 | 0 | 341 | 55 | 0 |
| pushed | plain | 108 | 93 | 0 | 93 | 15 | 0 |
| mine | timed | 396 | 374 | 0 | 374 | 22 | 0 |
| mine | plain | 108 | 102 | 0 | 102 | 6 | 0 |

Runs with a wrong answer or an unexpected exit code: **none**

**V2. Collection counts are repeatable.** Cells whose count varied between repetitions: **0** of 144. (Collection counts are deterministic for a single-threaded program, so any variation means the harness is broken.)

**V3. GC time never exceeds wall time.** Violations: **0**.

**V4. The longest pause never exceeds total GC time.** Violations: **0**.

**V5. Timer overhead** (median wall, timed vs plain build, summed over cells that have both):

| runtime | plain ms | timed ms | overhead |
|---|---|---|---|
| cheney | 4676 | 4703 | +0.6% |
| markcompact | 10359 | 10575 | +2.1% |
| pushed | 4469 | 4483 | +0.3% |
| mine | 4978 | 5109 | +2.6% |

**V6. Noise.** Coefficient of variation of GC time and of wall time across repetitions (timed builds):

| runtime | metric | cells | median CV | 90th pct CV | max CV | cells with CV > 15% |
|---|---|---|---|---|---|---|
| cheney | GC time | 34 | 3.3% | 69.3% | 191.2% | 8 |
| cheney | wall time | 34 | 2.5% | 53.7% | 134.8% | 4 |
| markcompact | GC time | 36 | 4.1% | 34.8% | 100.6% | 7 |
| markcompact | wall time | 36 | 3.0% | 46.4% | 92.9% | 6 |
| pushed | GC time | 31 | 3.0% | 19.5% | 27.4% | 5 |
| pushed | wall time | 31 | 2.7% | 12.8% | 24.6% | 2 |
| mine | GC time | 34 | 4.0% | 15.0% | 39.0% | 3 |
| mine | wall time | 34 | 2.6% | 9.8% | 40.3% | 2 |

Tiny GC times (under ~1 ms) are dominated by clock resolution and run-to-run jitter; compare totals and ratios on cells with at least a few ms of GC.

## Results

Each cell: median over repetitions. **GC ms** = total time inside `collect`; **max pause** = longest single collection; **GC %** = GC ms / wall ms of the same (timed) runs; **wall** = process wall time (min–max in brackets); **OOM** = exit 137. The **best** column names the runtime with the lowest GC ms; runtimes within max(10 %, 1.5 ms) of it are shown as a tie (`~`). The first collection of a generational runtime costs about 1 ms on its own (see *max pause* on garbage-only programs), which is why differences under 1.5 ms are not ranked.

### churn

garbage only; no live data. The floor: per-collection fixed cost.

**heap: default** (runtime default, 16 MiB)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| churn | 57 | 0.1 | 0.11 | 0% | 154 (152–155) | 28 | 15.3 | 1.05 | 9% | 170 (166–173) | 114 | 1.1 | 1.10 | 1% | 156 (152–157) | 457 | 0.9 | 0.85 | 1% | 160 (155–163) | ~ cheney/mine/pushed |

**heap: 8M** (LO_HEAP_SIZE=8388608)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| churn | 114 | 0.1 | 0.11 | 0% | 146 (144–148) | 57 | 15.6 | 0.49 | 10% | 162 (161–164) | 228 | 0.9 | 0.84 | 1% | 152 (147–154) | 915 | 0.6 | 0.57 | 0% | 146 (144–149) | ~ cheney/mine/pushed |

### live_list

a linked list stays live (32 B/node) while garbage is churned. Copying collectors pay for the live set on every collection.

**heap: default** (runtime default, 16 MiB)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| live10000 | 59 | 3.0 | 0.15 | 2% | 158 (156–161) | 29 | 31.2 | 2.15 | 17% | 186 (184–189) | 114 | 1.1 | 1.10 | 1% | 155 (153–159) | 458 | 1.1 | 1.03 | 1% | 158 (157–164) | ~ mine/pushed |
| live50000 | 70 | 14.8 | 0.31 | 9% | 169 (166–171) | 31 | 97.9 | 4.58 | 39% | 252 (252–254) | 114 | 1.3 | 1.34 | 1% | 156 (152–159) | 459 | 1.3 | 1.13 | 1% | 158 (156–165) | ~ mine/pushed |
| live100000 | 92 | 38.8 | 0.55 | 20% | 190 (186–194) | 35 | 212.3 | 7.87 | 58% | 367 (364–373) | 115 | 69.8 | 1.66 | 31% | 225 (222–229) | 460 | 1.5 | 1.14 | 1% | 159 (157–163) | mine |

**heap: 8M** (LO_HEAP_SIZE=8388608)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| live10000 | 123 | 6.1 | 0.15 | 4% | 152 (151–155) | 59 | 45.6 | 1.56 | 24% | 194 (193–195) | 229 | 0.8 | 0.81 | 1% | 148 (147–151) | 916 | 0.8 | 0.73 | 1% | 147 (145–152) | ~ mine/pushed |
| live50000 | 185 | 39.9 | 0.32 | 22% | 180 (178–185) | 70 | 199.3 | 3.94 | 57% | 347 (346–350) | 229 | 69.4 | 1.05 | 32% | 218 (216–222) | 918 | 1.0 | 0.76 | 1% | 147 (145–150) | mine |
| live100000 | 482 | 221.3 | 0.62 | 63% | 354 (347–3368) | 92 | 555.1 | 9.31 | 79% | 698 (664–1293) | OOM | OOM | OOM | OOM | 18 | 921 | 1.3 | 0.79 | 1% | 151 (146–182) | mine |

### live_tree

a complete binary tree stays live while garbage is churned (two pointers per node, so more tracing work per byte).

**heap: default** (runtime default, 16 MiB)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| depth12 | 59 | 3.4 | 0.18 | 2% | 160 (157–171) | 29 | 35.9 | 2.84 | 19% | 191 (189–297) | 114 | 1.1 | 1.06 | 1% | 157 (152–209) | 458 | 1.1 | 1.04 | 1% | 158 (155–162) | ~ pushed/mine |
| depth14 | 67 | 14.1 | 0.41 | 8% | 173 (166–1212) | 31 | 109.3 | 5.77 | 40% | 275 (252–743) | 114 | 1.4 | 1.39 | 1% | 167 (156–313) | 459 | 1.4 | 1.30 | 1% | 167 (158–223) | ~ pushed/mine |
| depth16 | 152 | 122.0 | 1.19 | 45% | 270 (264–272) | 41 | 497.3 | 17.19 | 76% | 650 (634–682) | 115 | 94.4 | 1.72 | 37% | 255 (254–260) | 462 | 1.9 | 1.16 | 1% | 162 (158–167) | mine |

**heap: 8M** (LO_HEAP_SIZE=8388608)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| depth12 | 124 | 7.0 | 0.18 | 4% | 157 (152–160) | 59 | 54.7 | 2.28 | 27% | 206 (199–231) | 229 | 0.8 | 0.77 | 1% | 156 (152–161) | 916 | 0.9 | 0.79 | 1% | 151 (146–160) | ~ pushed/mine |
| depth14 | 166 | 33.4 | 0.33 | 19% | 178 (175–183) | 67 | 186.1 | 4.52 | 56% | 335 (331–350) | 229 | 46.2 | 0.90 | 23% | 197 (196–289) | 918 | 1.0 | 0.77 | 1% | 149 (146–153) | mine |
| depth16 | OOM | OOM | OOM | OOM | 17 | 152 | 1855.3 | 28.21 | 93% | 1988 (1838–7271) | OOM | OOM | OOM | OOM | 18 | OOM | OOM | OOM | OOM | 18 | markcompact |

### queue_window

a sliding window of K linked nodes: each node lives for K allocations then dies. Window below the nursery size dies young; above it, nodes are promoted and then die in tenured space, and a dead tenured tail that points at young nodes keeps them alive (nepotism).

**heap: default** (runtime default, 16 MiB)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| window1000 | 22 | 0.2 | 0.08 | 0% | 142 (139–177) | 11 | 6.4 | 1.56 | 4% | 147 (143–173) | 45 | 13.9 | 0.97 | 8% | 166 (160–173) | 183 | 33.8 | 0.99 | 19% | 178 (175–187) | cheney |
| window10000 | 23 | 1.0 | 0.13 | 1% | 142 (140–155) | 11 | 11.8 | 2.43 | 8% | 154 (149–178) | 45 | 14.7 | 1.02 | 9% | 165 (162–167) | 183 | 34.7 | 1.04 | 19% | 179 (175–203) | cheney |
| window30000 | 25 | 3.7 | 0.21 | 3% | 143 (141–145) | 12 | 28.0 | 3.69 | 17% | 166 (165–170) | 45 | 16.7 | 1.10 | 10% | 163 (162–174) | 183 | 38.1 | 1.14 | 21% | 182 (179–191) | cheney |
| window100000 | 36 | 19.9 | 1.15 | 11% | 177 (160–1056) | 13 | 109.6 | 18.66 | 42% | 263 (244–1783) | 45 | 24.5 | 1.93 | 13% | 196 (173–316) | 183 | 64.2 | 1.90 | 28% | 226 (197–593) | cheney |
| window200000 | 93 | 87.6 | 1.45 | 40% | 220 (213–233) | 17 | 288.5 | 23.32 | 67% | 433 (396–522) | OOM | OOM | OOM | OOM | 23 | 183 | 135.2 | 1.93 | 47% | 285 (278–332) | cheney |

**heap: 8M** (LO_HEAP_SIZE=8388608)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| window1000 | 46 | 0.3 | 0.07 | 0% | 134 (130–136) | 22 | 6.4 | 1.01 | 5% | 140 (135–142) | 91 | 14.5 | 0.66 | 9% | 161 (158–163) | 366 | 33.7 | 0.69 | 20% | 169 (167–174) | cheney |
| window10000 | 49 | 2.1 | 0.13 | 2% | 137 (133–142) | 23 | 16.9 | 1.71 | 11% | 151 (149–159) | 91 | 16.1 | 0.71 | 10% | 161 (160–169) | 366 | 36.1 | 0.74 | 21% | 171 (168–177) | cheney |
| window30000 | 59 | 8.7 | 0.22 | 6% | 141 (137–153) | 25 | 50.6 | 3.35 | 28% | 184 (181–195) | 91 | 19.7 | 0.81 | 12% | 165 (163–211) | 366 | 44.4 | 0.79 | 25% | 181 (177–196) | cheney |
| window100000 | 189 | 99.9 | 1.36 | 42% | 236 (217–245) | 36 | 230.8 | 9.51 | 63% | 367 (360–467) | OOM | OOM | OOM | OOM | 19 | 366 | 142.0 | 1.48 | 49% | 288 (277–304) | cheney |
| window200000 | OOM | OOM | OOM | OOM | 18 | 93 | 1530.4 | 26.09 | 93% | 1644 (1481–1790) | OOM | OOM | OOM | OOM | 20 | OOM | OOM | OOM | OOM | 19 | markcompact |

### old_to_young

M long-lived holder objects are overwritten round-robin with fresh objects: every store is old-to-young, so the write barrier and the remembered set are on the hot path.

**heap: default** (runtime default, 16 MiB)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| holders100 | 11 | 0.1 | 0.07 | 0% | 78 (73–89) | 5 | 4.4 | 1.82 | 6% | 79 (71–88) | 22 | 1.2 | 1.08 | 1% | 118 (112–126) | 91 | 1.2 | 1.04 | 1% | 130 (124–141) | ~ cheney/pushed/mine |
| holders10000 | 12 | 1.4 | 0.19 | 2% | 79 (78–85) | 5 | 10.6 | 3.59 | 13% | 82 (81–86) | 23 | 3.5 | 1.25 | 3% | 132 (127–137) | 92 | 8.8 | 1.13 | 6% | 148 (144–161) | cheney |
| holders50000 | 17 | 9.1 | 0.64 | 10% | 90 (87–291) | 6 | 45.8 | 9.57 | 36% | 127 (118–470) | 23 | 14.9 | 1.74 | 9% | 160 (153–183) | 94 | 53.4 | 1.37 | 27% | 197 (177–243) | cheney |

**heap: 8M** (LO_HEAP_SIZE=8388608)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| holders100 | 22 | 0.1 | 0.07 | 0% | 73 (69–78) | 11 | 4.3 | 1.13 | 6% | 72 (70–89) | 45 | 0.8 | 0.78 | 1% | 114 (112–132) | 183 | 1.0 | 0.74 | 1% | 129 (123–138) | ~ cheney/pushed/mine |
| holders10000 | 26 | 2.8 | 0.21 | 4% | 79 (76–94) | 12 | 18.9 | 2.80 | 21% | 90 (85–93) | 46 | 5.4 | 0.91 | 4% | 133 (129–141) | 184 | 17.2 | 0.83 | 11% | 158 (148–174) | cheney |
| holders50000 | 68 | 35.7 | 0.67 | 33% | 109 (107–114) | 17 | 113.2 | 9.64 | 61% | 187 (181–199) | 47 | 28.1 | 1.25 | 17% | 164 (161–170) | 188 | 64.0 | 0.87 | 34% | 190 (184–210) | pushed |

### deep_roots

recursion D frames deep with a root in every frame, then garbage is churned at the bottom: every collection scans D shadow-stack frames.

**heap: default** (runtime default, 16 MiB)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| depth100 | 11 | 0.1 | 0.07 | 0% | 43 (41–47) | 5 | 3.2 | 1.01 | 7% | 46 (44–49) | 22 | 1.1 | 1.05 | 3% | 41 (40–45) | 91 | 1.0 | 0.92 | 2% | 43 (41–45) | ~ cheney/mine/pushed |
| depth500 | 11 | 0.1 | 0.07 | 0% | 43 (41–47) | 5 | 3.4 | 1.04 | 7% | 46 (45–48) | 22 | 1.1 | 1.03 | 3% | 42 (41–45) | 91 | 1.0 | 0.92 | 2% | 43 (41–44) | ~ cheney/mine/pushed |
| depth1000 | 11 | 0.1 | 0.07 | 0% | 42 (41–52) | 5 | 3.4 | 1.04 | 7% | 45 (44–49) | 22 | 1.1 | 1.03 | 3% | 42 (40–45) | 91 | 1.1 | 0.89 | 3% | 42 (40–45) | ~ cheney/pushed/mine |

**heap: 8M** (LO_HEAP_SIZE=8388608)

| program | cheney colls | cheney GC ms | cheney max pause ms | cheney GC % | cheney wall ms (min–max) | markcompact colls | markcompact GC ms | markcompact max pause ms | markcompact GC % | markcompact wall ms (min–max) | pushed colls | pushed GC ms | pushed max pause ms | pushed GC % | pushed wall ms (min–max) | mine colls | mine GC ms | mine max pause ms | mine GC % | mine wall ms (min–max) | best |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| depth100 | 22 | 0.1 | 0.06 | 0% | 40 (38–40) | 11 | 3.3 | 0.43 | 8% | 43 (42–48) | 45 | 0.8 | 0.74 | 2% | 40 (39–43) | 183 | 0.7 | 0.60 | 2% | 40 (39–41) | ~ cheney/mine/pushed |
| depth500 | 22 | 0.1 | 0.07 | 0% | 41 (39–45) | 11 | 3.5 | 0.45 | 8% | 44 (43–54) | 45 | 0.8 | 0.77 | 2% | 42 (39–61) | 183 | 0.8 | 0.63 | 2% | 40 (38–52) | ~ cheney/pushed/mine |
| depth1000 | 22 | 0.1 | 0.07 | 0% | 40 (39–42) | 11 | 3.7 | 0.48 | 9% | 43 (43–44) | 45 | 0.9 | 0.74 | 2% | 41 (39–48) | 183 | 1.1 | 0.61 | 3% | 40 (40–66) | ~ cheney/pushed/mine |

## Summary

Total GC time over every cell where all runtimes completed (so an OOM in one runtime excludes that cell for all). A cell is *decided* when exactly one runtime is within max(10 %, 1.5 ms) of the best GC time; otherwise every runtime within that margin is *tied*.

| runtime | total GC ms | sum of per-cell longest pause ms | cells decided in its favour | cells tied for best | cells clearly behind |
|---|---|---|---|---|---|
| cheney | 368 | 8 | 10 | 10 | 11 |
| markcompact | 1948 | 120 | 0 | 0 | 31 |
| pushed | 468 | 33 | 1 | 16 | 14 |
| mine | 450 | 29 | 4 | 16 | 11 |

31 cells counted of 36. OOM cells: cheney:depth16@8M, cheney:window200000@8M, mine:depth16@8M, mine:window200000@8M, pushed:depth16@8M, pushed:live100000@8M, pushed:window100000@8M, pushed:window200000@8M, pushed:window200000@default.
