# CC notes — P3 Phase 1 (`gc-debug` tooling, Rust)

**Branch:** `p3/phase-1-gc-debug` (off `fix/string-ops-abi-conformance`).

## What got done

Added the `gc-debug` cargo feature (off by default, works on native and wasm32):
whole-heap poison at init plus re-poisoning of freed space after every collection,
a `verify_heap` walk after every collection, and `GcStats` counters with a native
per-collection stderr log. Replaced the old `LO_GC_POISON` env var (which could
not work on WASM). Nine unit tests in `gc.rs` cover the checker's failure modes
and the stats. CI now also builds/tests/lints with `--features gc-debug`.

## Decisions made

- **Poison invariant.** Free space is always `0xEF`: nursery after every
  collection, active tenured tail, and the whole spare space (the old from-space
  is re-poisoned on each major). The allocator zeroes each slot it hands out, so
  poison never reaches live objects. Phase 2 (direct tenured allocation of large
  objects) must keep this: zero the slot, leave the rest of the tail poisoned.
- **"Known descriptor" means structurally sane.** The runtime cannot know the
  compiler's descriptors, so the checker validates alignment, outside-heap,
  `instance_size >= header`, pointer offsets inside the instance, and a
  terminating parent chain.
- **WASM logging: counters only** (agreed with the team lead before implementing).
  `host_write_stderr` is the abort channel; the print family is program output.
- **Abort code 70** for a failed check. Not an ABI code; debug builds only.
- `trigger=explicit` means `collect(true)` (`lo_gc_collect`); `alloc` means
  `collect(false)`, whichever of minor/major it chose.

## What the next phase should re-read or watch for

- Phase 2 (large objects) and Phase 3 (nursery 1/16) both change heap layout:
  run the full suite with `--features gc-debug` after each change; the checker is
  the safety net. `heap_partition` tests hard-code the 1/4 layout and must be
  rewritten around a `NURSERY_DIVISOR` constant in Phase 3.
- The checker allocates (a `HashSet` and `Vec` from Rust's heap, not the LO heap)
  and is therefore debug-only. The collector itself still allocates from Rust's
  heap too (remembered-set `HashSet`, `vec!` of regions); the policy is
  "never touches the LO heap", to be written up in the design note.
- Local toolchain gotcha: Homebrew's `cargo` shadows rustup's and lacks the wasm32
  std. Use `PATH=$HOME/.cargo/bin:$PATH`.

## Ambiguities for DC / SC

- None new from this phase. Still open from the audit: how the harness sets
  `LO_HEAP_SIZE` on WASM (Phase 4 adds a build-time fallback only).
