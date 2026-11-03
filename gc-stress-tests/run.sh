#!/usr/bin/env bash
# Run the Project 3 contributed GC stress tests on one target.
#
#   gc-stress-tests/run.sh native      # x86-64 Linux: needs as, cc, GNU ld
#   gc-stress-tests/run.sh wasm        # needs clang (wasm32), wasm-ld, wasmrun
#
# Environment (defaults match p3-grading-contract.sh):
#   LO_COMPILER             the compiler binary. Default: target/release/lo-compiler in this repository if it
#                           exists, else ../lo-compiler/target/release/lo-compiler (a sibling lo-compiler checkout)
#   LO_RUNTIME_NATIVE_LIB   native runtime archive. Default: <runtime>/target/release/liblo_runtime.a
#   LO_RUNTIME_WASM_LIB     wasm32 runtime archive. Default: <runtime>/target/wasm32-unknown-unknown/release/liblo_runtime.a
#                           where <runtime> is runtime/ if it exists (the lo-compiler layout), else rust/ (lo-runtime)
#   LO_TESTING              checkout of the course lo-testing repo (../lo-testing); its
#                           p3-abi-harness/ supplies lo_abi.h and run-harness.sh for the C tests
#   CLANG, WASM_LD, WASMRUN override the WASM tools
#
# Every LO test (*.lo) is compiled, linked against the runtime archive and run twice on
# native: at the default 16 MiB heap and at the `gc stress heap` named in its header
# (LO_HEAP_SIZE). On WASM a module has no environment, so it runs once at the default heap.
# Stdout and the exit status (0) must match <name>.expected.out. The C tests under abi/
# are run by the instructor's run-harness.sh. Exit status is 1 if anything failed.
set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
target="${1:?usage: run.sh native|wasm}"
# The same files work in lo-runtime (runtime in rust/, compiler in a sibling checkout) and in lo-compiler
# (runtime bundled in runtime/, compiler built in place).
if [ -d "$ROOT/runtime" ]; then RT_DIR="$ROOT/runtime"; else RT_DIR="$ROOT/rust"; fi
if [ -x "$ROOT/target/release/lo-compiler" ]; then DEFAULT_COMPILER="$ROOT/target/release/lo-compiler"
else DEFAULT_COMPILER="$ROOT/../lo-compiler/target/release/lo-compiler"; fi
LO_COMPILER="${LO_COMPILER:-$DEFAULT_COMPILER}"
NATIVE_LIB="${LO_RUNTIME_NATIVE_LIB:-$RT_DIR/target/release/liblo_runtime.a}"
WASM_LIB="${LO_RUNTIME_WASM_LIB:-$RT_DIR/target/wasm32-unknown-unknown/release/liblo_runtime.a}"
LO_TESTING="${LO_TESTING:-$ROOT/../lo-testing}"
CLANG="${CLANG:-clang}"; WASM_LD="${WASM_LD:-wasm-ld}"; WASMRUN="${WASMRUN:-wasmrun}"
work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT
fail=0

heap_of() { grep -m1 '^// gc stress heap:' "$1" | sed 's/.*: *//'; }

check() { # name, command-output-file, expected, rc, label
  if [ "$4" -eq 0 ] && cmp -s "$2" "$3"; then echo "PASS $1 ($5)"; else echo "FAIL $1 ($5, exit $4)"; fail=1; fi
}

for src in "$HERE"/*.lo; do
  name="$(basename "$src" .lo)"; exp="$HERE/$name.expected.out"
  case "$target" in
    native)
      "$LO_COMPILER" --emit-asm "$src" -o "$work/p.s" \
        && as --64 -o "$work/p.o" "$work/p.s" \
        && cc -no-pie -o "$work/p" "$work/p.o" "$NATIVE_LIB" -lpthread -ldl -lm \
        || { echo "FAIL $name (build)"; fail=1; continue; }
      env -u LO_HEAP_SIZE "$work/p" > "$work/out" 2>/dev/null; check "$name" "$work/out" "$exp" $? "default heap"
      LO_HEAP_SIZE="$(heap_of "$src")" "$work/p" > "$work/out" 2>/dev/null; check "$name" "$work/out" "$exp" $? "LO_HEAP_SIZE=$(heap_of "$src")"
      ;;
    wasm)
      "$LO_COMPILER" --emit "$src" -o "$work/p.s" \
        && "$CLANG" --target=wasm32-unknown-unknown -c "$work/p.s" -o "$work/p.o" \
        && "$WASM_LD" --no-entry --export=lo_entry --allow-undefined "$work/p.o" "$WASM_LIB" -o "$work/p.wasm" \
        || { echo "FAIL $name (build)"; fail=1; continue; }
      "$WASMRUN" "$work/p.wasm" < /dev/null > "$work/out" 2>/dev/null; check "$name" "$work/out" "$exp" $? "default heap"
      ;;
    *) echo "usage: run.sh native|wasm" >&2; exit 2 ;;
  esac
done

lib="$NATIVE_LIB"; [ "$target" = wasm ] && lib="$WASM_LIB"
if [ -x "$LO_TESTING/p3-abi-harness/run-harness.sh" ] || [ -f "$LO_TESTING/p3-abi-harness/run-harness.sh" ]; then
  CLANG="$CLANG" WASM_LD="$WASM_LD" WASMRUN="$WASMRUN" \
    bash "$LO_TESTING/p3-abi-harness/run-harness.sh" "$target" "$lib" "$HERE/abi" || fail=1
else
  echo "SKIP C tests: no lo-testing checkout at $LO_TESTING (set LO_TESTING)"; fail=1
fi
exit "$fail"
