# GC stress test results

PASS means exit status 0 and stdout equal to the expected output computed in Python. `broken-*` runtimes are deliberate bugs and are *supposed* to fail: that is the check that the tests bite.

| test | target | config | cheney | markcompact | generational | modified-generational | broken-barrier | broken-major-roots |
|---|---|---|---|---|---|---|---|---|
| gc_01_promote_then_die | native | default heap | PASS | PASS | PASS | PASS | FAIL (0) | FAIL (0) |
| gc_01_promote_then_die | native | LO_HEAP_SIZE=8388608 | PASS | PASS | PASS | PASS | FAIL (0) | FAIL (0) |
| gc_02_major_with_large_live_graph | native | default heap | PASS | PASS | PASS | PASS | FAIL (0) | FAIL (0) |
| gc_02_major_with_large_live_graph | native | LO_HEAP_SIZE=16777216 | PASS | PASS | PASS | PASS | FAIL (0) | FAIL (0) |
| gc_03_barrier_storm | native | default heap | PASS | PASS | PASS | PASS | PASS | PASS |
| gc_03_barrier_storm | native | LO_HEAP_SIZE=4194304 | PASS | PASS | PASS | PASS | FAIL (0) | FAIL (0) |
| gc_04_large_alloc_forces_major | native | default heap | PASS | PASS | PASS | PASS | FAIL (-11) | FAIL (0) |
| gc_04_large_alloc_forces_major | native | LO_HEAP_SIZE=4194304 | PASS | PASS | PASS | PASS | FAIL (0) | FAIL (0) |
| gc_05_deep_roots_during_majors | native | default heap | PASS | PASS | PASS | PASS | PASS | FAIL (-11) |
| gc_05_deep_roots_during_majors | native | LO_HEAP_SIZE=4194304 | PASS | PASS | PASS | PASS | FAIL (0) | FAIL (-11) |
| gc_06_cycle_across_generations | native | default heap | PASS | PASS | PASS | PASS | PASS | PASS |
| gc_06_cycle_across_generations | native | LO_HEAP_SIZE=4194304 | PASS | PASS | PASS | PASS | FAIL (0) | FAIL (-11) |
| abi_01_forced_collections_on_a_cyclic_ring | native | C harness | PASS | PASS | PASS | PASS | FAIL (1) | FAIL (1) |
| abi_02_forced_collect_young_reachable_only_through_old | native | C harness | PASS | PASS | PASS | PASS | FAIL (1) | FAIL (1) |
| gc_01_promote_then_die | wasm | default heap | PASS | PASS | PASS | PASS | FAIL (0) | FAIL (0) |
| gc_02_major_with_large_live_graph | wasm | default heap | PASS | PASS | PASS | PASS | PASS | FAIL (134) |
| gc_03_barrier_storm | wasm | default heap | PASS | PASS | PASS | PASS | PASS | PASS |
| gc_04_large_alloc_forces_major | wasm | default heap | PASS | PASS | PASS | PASS | FAIL (0) | FAIL (134) |
| gc_05_deep_roots_during_majors | wasm | default heap | PASS | PASS | PASS | PASS | PASS | FAIL (134) |
| gc_06_cycle_across_generations | wasm | default heap | PASS | PASS | PASS | PASS | PASS | PASS |
| abi_01_forced_collections_on_a_cyclic_ring | wasm | C harness | PASS | PASS | PASS | PASS | PASS | FAIL (1) |
| abi_02_forced_collect_young_reachable_only_through_old | wasm | C harness | PASS | PASS | PASS | PASS | PASS | PASS |

| runtime | passed | failed |
|---|---|---|
| cheney | 22 | 0 |
| markcompact | 22 | 0 |
| generational | 22 | 0 |
| modified-generational | 22 | 0 |
| broken-barrier | 9 | 13 |
| broken-major-roots | 5 | 18 |
