# CC notes — P3 Phase 3 (nursery size, Rust)

**Branch:** `p3/phase-3-nursery-size` (off `p3/phase-2-large-objects`).

## What got done

The nursery shrank from 1/4 to 1/16 of the heap. Live data can fill one tenured
space, so this raises the usable heap from 37.5% to 46.875% (plain Cheney: 50%).
The split is now one constant, `NURSERY_DIVISOR` in `rust/src/alloc.rs`;
`heap_partition` and its tests are written in terms of it, so changing the
nursery fraction later is a one-line edit.

## Decisions made

- **1/16, toggleable.** Chosen with the team lead. We cannot reach 50% without
  dropping generations; the rounding remainder still goes to the nursery.
- **`tenured_spaces_hold_almost_half_the_heap`** pins the point of the change
  (tenured >= 45% of the heap at 4 KiB, 64 KiB, 1 MiB and 16 MiB). If someone
  raises the nursery fraction past that, this test fails and they must decide on
  purpose.
- **`live_data_filling_most_of_a_tenured_space_survives`** drives a rooted chain
  of 90% of a tenured space through the real nursery/promotion/major path at
  three heap sizes. It checks the mechanism holds at the ceiling; the ceiling
  itself is the partition test above.
- **New test hook `nursery_size()`** (`#[doc(hidden)]`, not ABI) so integration
  tests size allocations against the real nursery. The string test helper
  `leave_insufficient_nursery_space` used a hard-coded 1 KiB nursery and now
  derives its target from it.

## What the next phase should re-read or watch for

- With a 1/16 nursery, objects above ~6% of the heap take Phase 2's direct-tenured
  path. A 4 KiB heap has a 256-byte nursery.
- More frequent minor collections (cheap: cost scales with survivors). Under
  `gc-debug` each one runs the checker, so tiny-heap runs with the feature on
  produce a lot of log lines.
- Phase 4 (WASM heap size) should pick the stress sizes knowing a heap below ~48
  bytes falls back to the default, and that very small heaps leave a tiny nursery.

## Ambiguities for DC / SC

- Remaining gap to Cheney is ~3 points of heap (46.9% vs 50%). If the instructor's
  stress runs sit exactly at the Cheney limit, a program there could still abort
  137 here; worth asking DC what heap sizes the stress runs use.
