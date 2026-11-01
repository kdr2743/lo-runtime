# CC notes — P3 Phase 2 (large objects, Rust)

**Branch:** `p3/phase-2-large-objects` (off `p3/phase-1-gc-debug`).

## What got done

Fixed a spurious out-of-memory abort: `alloc_raw` only allocated from the
nursery, so any object larger than the whole nursery aborted 137 even with plenty
of heap free (confirmed before the fix: a 5 MB string on the default 16 MiB heap
aborted). Such objects are now allocated at the tail of the active tenured space
(`alloc_large`); if they do not fit, a major collection runs first, and only then
does the allocator abort 137. Four in-crate tests and three child-process tests
cover it; all also pass under `--features gc-debug`.

## Decisions made

- **Threshold is "larger than the nursery capacity"**, read from the allocator
  statics, so it stays correct when the nursery fraction changes in Phase 3.
- **Write-barrier safety.** A large object is born zeroed, so it holds no
  old-to-young pointer. Because it lives in tenured space, every later pointer
  store into it is recorded by `lo_gc_write_barrier`. Pinned by
  `large_object_keeps_young_referent_alive_via_the_barrier`.
- **Poison invariant (Phase 1) preserved.** The slot is zeroed; the rest of the
  tenured tail stays poison.
- **Tests size objects against both layouts.** The large-object tests use a
  128 KiB heap and a 40000-byte object, which is larger than the nursery and
  smaller than a tenured space under both the current 1/4 nursery and the planned
  1/16, so Phase 3 should not need to touch them.
- The 137 child-process test asserts the abort message is the *last* stderr line,
  because `gc-debug` logs a line per collection before it.

## What the next phase should re-read or watch for

- Phase 3 rewrites `heap_partition` and its tests (still hard-coded to 1/4).
  Re-run everything with `--features gc-debug`.
- With a smaller nursery, more medium objects will take the new large path and
  fill tenured space faster; watch the "max live data" numbers.
- A large object is copied by every major collection like any other; there is no
  separate large-object space (not needed for this project's heap sizes).

## Ambiguities for DC / SC

- None new.
