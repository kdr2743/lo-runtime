# How these results were produced

Date: 2026-11-02. Command: `gc-stress-tests/matrix.py run` once per target, then `matrix.py report`.

| runtime label | built from (lo-runtime) | commit |
|---|---|---|
| cheney | origin/main | fe7dff4 |
| markcompact | origin/feat/mark-compact-gc | d469a6d |
| generational | origin/generational | 13153f3 |
| modified-generational | origin/p3/phase-2-gc-bench | 78874ea |
| broken-barrier | modified-generational with the remembered-set insert removed (`set.insert(obj)` replaced by `false` in `lo_gc_write_barrier`) | same + patch |
| broken-major-roots | modified-generational where a major forwards only roots that point into the nursery (`if major_alloc \|\| in_nursery(*slot)` becomes `if in_nursery(*slot)`) | same + patch |

Compiler: lo-compiler at lo-compiler branch p3/gc-stress-tests (same compiler code as main).
Native: x86-64 Linux container (rust:1-bookworm) on an Apple Silicon host, release builds, default features.
WASM: wasm32-unknown-unknown release builds run under wasmrun on the host.

## Reading the results

* The four real collectors pass every case. The two `broken-*` runtimes are deliberate bugs; they are supposed to fail.
* Native runs each LO test twice: at the default 16 MiB heap and at the test's `gc stress heap`. WASM runs once, at the
  default heap, because a WASM module has no environment to carry `LO_HEAP_SIZE`.
* Some tests only catch a bug at their small stress heap: with a 16 MiB heap collections are rare, so the bug stays hidden
  (for example `gc_03_barrier_storm` and `gc_06_cycle_across_generations` pass the broken collectors at the default heap).
  That is why the WASM column, which has no small-heap run, catches the broken collectors less often.
* The C harness rows can appear twice for a failing program (default config and the `@heap=` config); the table shows one.
