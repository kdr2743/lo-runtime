# Project 3 GC stress tests

Tests we wrote to hunt collector bugs beyond the provided suites. Six are LO programs in the
stress-test header-contract format (`<name>.lo` plus `<name>.expected.out`, with a
`// gc stress heap:` line); two are C programs in the ABI-harness format under `abi/`, because only C can
call `lo_gc_collect` directly. Expected output is computed in Python (`gen.py`), never taken from a collector.

| Test | What it forces |
|---|---|
| `contributed_gc_01_promote_then_die` | a 60k-node window outlives the nursery and dies in tenured space: many majors |
| `contributed_gc_02_major_with_large_live_graph` | every major copies a 3.2 MB live list plus a window |
| `contributed_gc_03_barrier_storm` | 12k old holders overwritten round-robin: a large remembered set |
| `contributed_gc_04_large_alloc_forces_major` | a string larger than the nursery forces a major while a young object is reachable only through an old holder |
| `contributed_gc_05_deep_roots_during_majors` | 1000 frames of roots while majors run |
| `contributed_gc_06_cycle_across_generations` | a doubly linked ring whose nodes are replaced one by one, so the cycle's pointers keep crossing generations |
| `abi/contributed_abi_01_forced_collections_on_a_cyclic_ring` | forced `lo_gc_collect` over a cyclic ring, with minors between the forced majors |
| `abi/contributed_abi_02_forced_collect_young_reachable_only_through_old` | the only path to a young object runs through an old one, across minors and forced majors |

## Running them

Build the runtime for both targets and the compiler, then run the script once per target from the repository root.

In **lo-runtime** (the compiler lives in a sibling `lo-compiler` checkout):

```sh
( cd rust && cargo build --release --lib &&
  cargo build --release --target wasm32-unknown-unknown --lib )
( cd ../lo-compiler && cargo build --release )

gc-stress-tests/run.sh native      # x86-64 Linux: needs as, cc, GNU ld
gc-stress-tests/run.sh wasm        # needs clang (wasm32), wasm-ld, wasmrun
```

In **lo-compiler** the runtime is bundled in `runtime/` and the compiler is built in place; the commands are the
same with `runtime` for `rust` and `cargo build --release` at the root.

`run.sh` takes its paths from `LO_COMPILER`, `LO_RUNTIME_NATIVE_LIB`, `LO_RUNTIME_WASM_LIB` (defaults chosen by
layout, see the top of the script) and `LO_TESTING`, a checkout of the course `lo-testing` repo (default `../lo-testing`),
whose `p3-abi-harness/` supplies `lo_abi.h` and `run-harness.sh` for the C tests. On native each LO test runs
twice, at the default 16 MiB heap and at the `gc stress heap` in its header; on WASM it runs once at the default
heap, because a WASM module has no environment to carry `LO_HEAP_SIZE`. The script prints PASS or FAIL per
run and exits 1 if anything failed. On Apple Silicon, run the native half in an `x86_64` Linux container.

## Running them against several runtimes, and checking they bite

`matrix.py` runs every test against any number of runtime archives and writes one CSV row per (test, runtime,
config); `report` turns the CSVs into a table:

```sh
gc-stress-tests/matrix.py run --target native --compiler ../lo-compiler/target/release/lo-compiler \
    --lo-testing ../lo-testing --lib mine=path/to/liblo_runtime.a --lib other=... --out results-native.csv
gc-stress-tests/matrix.py report results-native.csv results-wasm.csv --out results.md
```

`results/run-1/` holds our run: Cheney, mark-compact, the earlier generational collector and the modified
generational collector (all pass every case), plus two deliberately broken copies of the modified generational
collector (a dropped remembered-set insert; majors that skip roots already in tenured space), which are
supposed to fail. `PROVENANCE.md` there records the commits and the exact patches. Natively both broken
collectors are caught by all eight tests; on WASM, which only runs the default heap, by 2 and 5 of the 8.

## Small heaps and debug builds

* Any program can be run at a tiny heap by hand: `LO_HEAP_SIZE=65536 ./prog` (native only).
* The runtime's `gc-debug` feature poisons the heap and freed space and runs a heap checker after every
  collection (exit 70 on corruption): `cd runtime && cargo test --features gc-debug`, or
  `cargo build --release --lib --features gc-debug` for an archive that checks every collection of a compiled
  program. It builds for both targets.
* To regenerate the LO tests and their expected output: `python3 gc-stress-tests/gen.py <outdir>`.
