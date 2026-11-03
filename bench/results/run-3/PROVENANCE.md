# How run-3 was produced

Date: 2026-11-02. Command: `bench/docker.sh -- /path/to/lo-compiler` (all four collectors, 11 repetitions, plain archives 3).

| label | built from | commit |
|---|---|---|
| cheney | origin/main | fe7dff4 |
| markcompact | origin/feat/mark-compact-gc | d469a6d |
| generational | origin/generational | 13153f3 |
| modified-generational | HEAD of p3/phase-2-gc-bench | 78874ea |

Note: `origin/generational` had moved from `4577ef1` (used in run-1 and run-2) to the commit above between runs;
its totals are within noise of the earlier runs, so the move did not change how it performs on these benchmarks.
Compiler: lo-compiler at branch p3/gc-stress-tests (compiler code identical to main). Native runs are an x86-64 container
on an Apple Silicon host. See report.md for the validity checks.
