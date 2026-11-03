#!/usr/bin/env bash
# Run the benchmark suite in an x86-64 Linux container (for Apple Silicon / any non-x86 host).
#
#   bench/docker.sh [--reps N] [--scale F] [--only REGEX] [--out DIR] LABEL=GITREF ... \
#                   -- /path/to/lo-compiler-checkout
#
# For every LABEL=GITREF the runtime (rust/) at that ref is exported from this repository,
# given the gc-time timer if it does not have it (add-gc-timing.py), and built twice:
# a timed archive and a plain one (for the timer-overhead check). The LO compiler is built
# from the checkout after `--`. Results land in DIR (default bench/results): runs.csv (every run),
# summary.csv (one row per cell: medians, min/max, CV) and report.md. Needs docker; the first run pulls rust:1-bookworm.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/.." && pwd)"
reps=11; scale=1; only=""; out="$here/results"; refs=(); compiler_src=""
while [ $# -gt 0 ]; do
  case "$1" in
    --reps) reps=$2; shift 2;;
    --scale) scale=$2; shift 2;;
    --only) only=$2; shift 2;;
    --out) out=$2; shift 2;;
    --) compiler_src=$2; shift 2;;
    *=*) refs+=("$1"); shift;;
    *) echo "unknown argument: $1" >&2; exit 2;;
  esac
done
[ ${#refs[@]} -gt 0 ] && [ -n "$compiler_src" ] || { sed -n 2,12p "$0" >&2; exit 2; }

work="$(mktemp -d "${TMPDIR:-/tmp}/lo-bench.XXXXXX")"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/rt" "$work/out"
for spec in "${refs[@]}"; do
  label="${spec%%=*}"; ref="${spec#*=}"
  mkdir -p "$work/rt/$label"
  git -C "$repo" archive "$ref" rust | tar -x -C "$work/rt/$label"
  if ! grep -q 'gc-time' "$work/rt/$label/rust/Cargo.toml"; then
    python3 "$here/add-gc-timing.py" "$work/rt/$label/rust" "$repo/rust/src/gc_time.rs"
  fi
done
mkdir -p "$work/compiler"
rsync -a --exclude target --exclude .git --exclude .worktrees --exclude .idea --exclude gc-comparison "$compiler_src"/ "$work/compiler"/
cp "$here/bench.py" "$work/bench.py"

args=(--reps "$reps" --scale "$scale"); [ -n "$only" ] && args+=(--only "$only")
rts=(); for spec in "${refs[@]}"; do l="${spec%%=*}"; rts+=(--runtime "$l=/tmp/t/$l.timed.a:/tmp/t/$l.plain.a"); done
# Quote every argument for the inner shell (the --only regex may contain | or $).
rts_q="$(printf '%q ' "${rts[@]}")"; args_q="$(printf '%q ' "${args[@]}")"

docker run --rm --platform linux/amd64 -v "$work:/w" rust:1-bookworm bash -euc '
  mkdir -p /tmp/t
  for d in /w/rt/*; do l=$(basename $d)
    (cd $d/rust && CARGO_TARGET_DIR=/tmp/tgt-$l cargo build --release --lib --features gc-time >/dev/null 2>&1 && cp /tmp/tgt-$l/release/liblo_runtime.a /tmp/t/$l.timed.a)
    (cd $d/rust && CARGO_TARGET_DIR=/tmp/tgtp-$l cargo build --release --lib >/dev/null 2>&1 && cp /tmp/tgtp-$l/release/liblo_runtime.a /tmp/t/$l.plain.a)
  done
  (cd /w/compiler && CARGO_TARGET_DIR=/tmp/tgt-compiler cargo build --release >/dev/null 2>&1)
  python3 /w/bench.py run --compiler /tmp/tgt-compiler/release/lo-compiler '"$rts_q"' --out /w/out '"$args_q"'
  python3 /w/bench.py report /w/out
'
mkdir -p "$out"; cp -R "$work/out/." "$out/"
echo "results in $out (report.md)"
