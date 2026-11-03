#!/usr/bin/env python3
"""Build each GC feature variant, run heap-pressure workloads, emit CSV + analysis.

Example:
  python3 scripts/bench_gc.py
  python3 scripts/bench_gc.py --trials 5 --heap 4194304 --workloads churn,mixed
"""

from __future__ import annotations

import argparse
import csv
import os
import statistics
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RUST = ROOT / "rust"
RESULTS = ROOT / "benchmarks" / "results"

COLLECTORS = (
    ("cheney", "gc_cheney"),
    ("mark_compact", "gc_mark_compact"),
    ("generational", "gc_generational"),
)

DEFAULT_WORKLOADS = ("churn", "long_lived", "mixed", "explicit")

SUMMARY_PREFIX = "[lo-gc-bench] "

# What each workload is supposed to show, for the analysis write-up.
WORKLOAD_INTENT = {
    "churn": (
        "Short-lived garbage only (one live root). "
        "Expect many collections; generational should win on pause time via cheap minors."
    ),
    "long_lived": (
        "Large rooted chain + forced collections. "
        "Expect copying collectors to pay per live byte; mark-compact slides in place "
        "but its mark+repair pass is often slower here."
    ),
    "mixed": (
        "Tenured spine + young churn + write-barrier stores. "
        "Expect generational barrier_recorded > 0 and minors reclaiming young junk; "
        "Cheney/mark-compact do full collections when space fills."
    ),
    "explicit": (
        "Steady live set, garbage bursts, forced collects. "
        "Isolates pause cost with a controlled live fraction."
    ),
}


def parse_summary(line: str) -> dict[str, str]:
    if not line.startswith(SUMMARY_PREFIX):
        raise ValueError(f"expected summary line, got: {line!r}")
    fields: dict[str, str] = {}
    for part in line[len(SUMMARY_PREFIX) :].split():
        if "=" not in part:
            continue
        key, value = part.split("=", 1)
        fields[key] = value
    return fields


def target_dir() -> Path:
    return Path(os.environ.get("CARGO_TARGET_DIR", RUST / "target"))


def stamped_bin(collector: str) -> Path:
    """Per-collector copy so builds do not overwrite each other."""
    return target_dir() / "release" / f"gc_bench-{collector}"


def cargo_build(collector: str, feature: str) -> Path:
    cmd = [
        "cargo",
        "build",
        "--release",
        "--no-default-features",
        "--features",
        feature,
        "--bin",
        "gc_bench",
    ]
    print(f"+ {' '.join(cmd)}", flush=True)
    subprocess.run(cmd, cwd=RUST, check=True)
    built = target_dir() / "release" / "gc_bench"
    dest = stamped_bin(collector)
    dest.write_bytes(built.read_bytes())
    dest.chmod(built.stat().st_mode)
    print(f"  copied -> {dest}", flush=True)
    return dest


def run_one(
    collector: str,
    feature: str,
    workload: str,
    heap: int,
    iters: int | None,
    env_extra: dict[str, str],
) -> dict[str, str]:
    env = {
        **os.environ,
        "LO_HEAP_SIZE": str(heap),
        **env_extra,
    }
    if iters is not None:
        env["LO_GC_BENCH_ITERS"] = str(iters)
    binary = stamped_bin(collector)
    if not binary.is_file():
        raise RuntimeError(
            f"missing {binary}; build with cargo first (omit --skip-build)"
        )
    cmd = [str(binary), workload]
    proc = subprocess.run(
        cmd,
        cwd=RUST,
        env=env,
        check=False,
        capture_output=True,
        text=True,
    )
    empty = {
        "collector": collector,
        "workload": workload,
        "heap": str(heap),
        "oom": "1",
        "wall_ns": "",
        "gc_pause_ns_sum": "",
        "gc_pause_ns_max": "",
        "collections": "",
        "minors": "",
        "majors": "",
        "copied": "",
        "copied_bytes": "",
        "reclaimed": "",
        "peak_live": "",
        "allocs": "",
        "alloc_bytes": "",
        "barrier_calls": "",
        "barrier_recorded": "",
        "promoted": "",
    }
    if proc.returncode == 137:
        return empty
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr)
        sys.stderr.write(proc.stdout)
        raise RuntimeError(
            f"gc_bench failed (feature={feature} workload={workload} "
            f"exit={proc.returncode})"
        )
    summary = None
    for line in proc.stdout.splitlines():
        if line.startswith(SUMMARY_PREFIX):
            summary = line
    if summary is None:
        sys.stderr.write(proc.stderr)
        sys.stderr.write(proc.stdout)
        raise RuntimeError("no [lo-gc-bench] summary line in stdout")
    fields = parse_summary(summary)
    reported = fields.get("collector", "")
    if reported and reported != collector:
        raise RuntimeError(
            f"collector mismatch: ran {binary.name} for '{collector}' "
            f"but summary says collector={reported}"
        )
    fields["collector"] = collector
    fields["oom"] = "0"
    return fields


def fnum(row: dict[str, str], key: str) -> float | None:
    raw = row.get(key, "")
    if raw == "" or row.get("oom") == "1":
        return None
    try:
        return float(raw)
    except ValueError:
        return None


def median(values: list[float]) -> float | None:
    if not values:
        return None
    return float(statistics.median(values))


def fmt(n: float | None, digits: int = 0) -> str:
    if n is None:
        return "—"
    if digits == 0:
        return f"{n:.0f}"
    return f"{n:.{digits}f}"


def ns_to_ms(n: float | None) -> str:
    if n is None:
        return "—"
    ms = n / 1e6
    # Sub-millisecond pauses are real (e.g. Cheney reclaiming near-empty space).
    if ms < 0.01:
        return f"{ms:.4f}"
    return f"{ms:.2f}"


def subset_rows(
    rows: list[dict[str, str]], collector: str, workload: str
) -> list[dict[str, str]]:
    return [
        r
        for r in rows
        if r.get("collector") == collector
        and r.get("workload") == workload
        and r.get("oom") != "1"
    ]


def med_field(
    rows: list[dict[str, str]], collector: str, workload: str, key: str
) -> float | None:
    vals = []
    for r in subset_rows(rows, collector, workload):
        v = fnum(r, key)
        if v is not None:
            vals.append(v)
    return median(vals)


def write_analysis(
    rows: list[dict[str, str]],
    workloads: list[str],
    heap: int,
    trials: int,
    iters: int | None,
    out_path: Path,
) -> None:
    lines: list[str] = []
    lines.append("# GC benchmark analysis")
    lines.append("")
    lines.append(f"Generated: {datetime.now(timezone.utc).strftime('%Y-%m-%d %H:%M:%SZ')}")
    lines.append(f"Heap (`LO_HEAP_SIZE`): **{heap}** bytes")
    lines.append(f"Trials per cell: **{trials}**")
    if iters is not None:
        lines.append(
            f"`LO_GC_BENCH_ITERS` override: **{iters}** "
            "(disables heap-relative sizing — easy to get `collections=0`)"
        )
    else:
        lines.append(
            "Scale: **heap-relative defaults** in `rust/src/bin/gc_bench.rs` "
            "(churn ≈ 8× heap of dead Nodes, etc.)"
        )
    lines.append("")
    lines.append("## How we benchmarked")
    lines.append("")
    lines.append(
        "We do **not** run LO source programs. The driver builds the Rust "
        "`gc_bench` binary three times (Cheney, mark-compact, generational via "
        "cargo features) and runs the same synthetic linked-list workloads "
        "against each. Objects are 24-byte `Node`s with one pointer field; "
        "roots go through the shadow stack; pointer stores go through "
        "`lo_gc_write_barrier`."
    )
    lines.append("")
    lines.append("Usable heap differs by collector even at the same `LO_HEAP_SIZE`:")
    lines.append("")
    lines.append("| Collector | Approx. usable for new allocs |")
    lines.append("|-----------|-------------------------------|")
    lines.append("| Cheney | ~50% (one semispace) |")
    lines.append("| Mark-compact | ~100% (contiguous) |")
    lines.append("| Generational | nursery ~¼ for young allocs; tenure for survivors |")
    lines.append("")
    lines.append("### Workload intent")
    lines.append("")
    for wl in workloads:
        lines.append(f"- **`{wl}`** — {WORKLOAD_INTENT.get(wl, '')}")
    lines.append("")
    lines.append("## Results (medians over non-OOM trials)")
    lines.append("")
    lines.append(
        "| Workload | Collector | Wall ms | GC pause ms | Collections "
        "| Minors | Majors | Barrier recorded | Promoted | Notes |"
    )
    lines.append(
        "|----------|-----------|--------:|------------:|-------------:"
        "|-------:|-------:|-----------------:|---------:|-------|"
    )

    warnings: list[str] = []
    findings: list[str] = []

    for wl in workloads:
        for name, _ in COLLECTORS:
            wall = med_field(rows, name, wl, "wall_ns")
            pause = med_field(rows, name, wl, "gc_pause_ns_sum")
            cols = med_field(rows, name, wl, "collections")
            minors = med_field(rows, name, wl, "minors")
            majors = med_field(rows, name, wl, "majors")
            recorded = med_field(rows, name, wl, "barrier_recorded")
            promoted = med_field(rows, name, wl, "promoted")
            oom_n = sum(
                1
                for r in rows
                if r.get("collector") == name
                and r.get("workload") == wl
                and r.get("oom") == "1"
            )
            notes: list[str] = []
            if oom_n:
                notes.append(f"{oom_n} OOM")
            if cols is not None and cols < 1:
                notes.append("NO GC")
                warnings.append(
                    f"`{wl}` / `{name}`: median collections = 0. "
                    "Workload did not overflow the heap — results are mutator-only, "
                    "not a GC comparison. Use heap-relative defaults (omit `--iters`) "
                    "or lower `--heap`."
                )
            if wl == "mixed" and name == "generational":
                if recorded is not None and recorded < 1:
                    notes.append("no old→young")
                    warnings.append(
                        "`mixed` / `generational`: `barrier_recorded` is 0, so the "
                        "spine was never treated as tenured. The harness should "
                        "`lo_gc_collect` after building the spine before young churn."
                    )
                if minors is not None and minors < 1 and (cols or 0) > 0:
                    notes.append("majors only")
            if wl == "churn" and name == "generational" and minors is not None and minors >= 1:
                notes.append("minors OK")
            lines.append(
                f"| {wl} | {name} | {ns_to_ms(wall)} | {ns_to_ms(pause)} | "
                f"{fmt(cols)} | {fmt(minors)} | {fmt(majors)} | {fmt(recorded)} | "
                f"{fmt(promoted)} | {', '.join(notes) if notes else '—'} |"
            )

    lines.append("")
    lines.append("## What worked / what didn't (reading the numbers)")
    lines.append("")

    # Auto findings from relative medians where GC actually ran.
    for wl in workloads:
        walls = {
            name: med_field(rows, name, wl, "wall_ns") for name, _ in COLLECTORS
        }
        pauses = {
            name: med_field(rows, name, wl, "gc_pause_ns_sum") for name, _ in COLLECTORS
        }
        cols = {
            name: med_field(rows, name, wl, "collections") for name, _ in COLLECTORS
        }
        if any(c is None or c < 1 for c in cols.values()):
            findings.append(
                f"- **`{wl}`:** incomplete — at least one collector did not collect; "
                "skip ranking until scale is fixed."
            )
            continue
        # Rank by pause sum (GC cost) and wall time.
        by_pause = sorted(
            ((n, p) for n, p in pauses.items() if p is not None), key=lambda x: x[1]
        )
        by_wall = sorted(
            ((n, w) for n, w in walls.items() if w is not None), key=lambda x: x[1]
        )
        if by_pause:
            best_p, best_pv = by_pause[0]
            worst_p, worst_pv = by_pause[-1]
            findings.append(
                f"- **`{wl}` GC pause:** `{best_p}` lowest "
                f"({ns_to_ms(best_pv)} ms), `{worst_p}` highest "
                f"({ns_to_ms(worst_pv)} ms)."
            )
        if by_wall:
            best_w, best_wv = by_wall[0]
            findings.append(
                f"- **`{wl}` wall time:** `{best_w}` fastest ({ns_to_ms(best_wv)} ms)."
            )

        if wl == "churn":
            g_minors = med_field(rows, "generational", wl, "minors")
            if g_minors is not None and g_minors >= 1:
                findings.append(
                    "- **`churn` why:** generational should prefer many cheap minors "
                    "over full-heap copies; check `minors` ≫ `majors` and pause sum "
                    "vs Cheney."
                )
        if wl == "long_lived":
            findings.append(
                "- **`long_lived` why:** almost all bytes are live, so copying "
                "(Cheney / gen major) moves the whole chain every forced collect; "
                "mark-compact still walks+slides every live object and often loses "
                "on pause here despite not needing a to-space."
            )
        if wl == "mixed":
            rec = med_field(rows, "generational", wl, "barrier_recorded")
            if rec is not None and rec > 0:
                findings.append(
                    "- **`mixed` why:** `barrier_recorded` > 0 confirms old→young "
                    "stores hit the remembered set; young garbage should die in "
                    "minors without rewriting the tenured spine."
                )
        if wl == "explicit":
            findings.append(
                "- **`explicit` why:** live fraction is fixed and collects are forced, "
                "so pause differences are mostly algorithm overhead per live set, "
                "not “did we fill the heap.”"
            )

    if findings:
        lines.extend(findings)
    else:
        lines.append("- (no automatic findings — check warnings / OOM)")
    lines.append("")

    if warnings:
        lines.append("## Warnings")
        lines.append("")
        for w in warnings:
            lines.append(f"- {w}")
        lines.append("")

    out_path.write_text("\n".join(lines) + "\n")
    print(f"Wrote analysis {out_path}")


def print_comparison(rows: list[dict[str, str]], workloads: list[str]) -> None:
    print("\nMedian wall_ms / pause_ms / collections (non-OOM):")
    header = (
        f"{'workload':<12} {'collector':<14} {'wall_ms':>10} "
        f"{'pause_ms':>10} {'collections':>12}"
    )
    print(header)
    print("-" * len(header))
    for workload in workloads:
        for name, _feature in COLLECTORS:
            wall = med_field(rows, name, workload, "wall_ns")
            pause = med_field(rows, name, workload, "gc_pause_ns_sum")
            cols = med_field(rows, name, workload, "collections")
            flag = "" if (cols is not None and cols >= 1) else "  ← no GC"
            print(
                f"{workload:<12} {name:<14} {ns_to_ms(wall):>10} "
                f"{ns_to_ms(pause):>10} {fmt(cols):>12}{flag}"
            )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--trials", type=int, default=3)
    parser.add_argument(
        "--heap",
        type=int,
        default=4 * 1024 * 1024,
        help="LO_HEAP_SIZE in bytes (default 4 MiB; small enough that "
        "heap-relative workloads finish quickly but still collect)",
    )
    parser.add_argument(
        "--workloads",
        default=",".join(DEFAULT_WORKLOADS),
    )
    parser.add_argument(
        "--iters",
        type=int,
        default=None,
        help="override LO_GC_BENCH_ITERS (disables heap-relative sizing)",
    )
    parser.add_argument("--skip-build", action="store_true")
    args = parser.parse_args()
    workloads = [w.strip() for w in args.workloads.split(",") if w.strip()]
    if not workloads:
        parser.error("no workloads")

    RESULTS.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    out_path = RESULTS / f"gc-bench-{stamp}.csv"
    analysis_path = RESULTS / f"gc-bench-{stamp}-analysis.md"

    rows: list[dict[str, str]] = []
    for name, feature in COLLECTORS:
        if not args.skip_build:
            cargo_build(name, feature)
        elif not stamped_bin(name).is_file():
            raise SystemExit(
                f"missing {stamped_bin(name)}; omit --skip-build to compile"
            )
        for workload in workloads:
            print(f"== {name} / {workload} ({args.trials} trials) ==", flush=True)
            for trial in range(1, args.trials + 1):
                fields = run_one(name, feature, workload, args.heap, args.iters, {})
                fields["trial"] = str(trial)
                fields["collector"] = name
                rows.append(fields)
                wall = fields.get("wall_ns", "")
                cols = fields.get("collections", "")
                oom = fields.get("oom", "0")
                print(
                    f"  trial {trial}: wall_ns={wall} collections={cols} oom={oom}",
                    flush=True,
                )

    fieldnames = [
        "collector",
        "workload",
        "trial",
        "heap",
        "oom",
        "wall_ns",
        "gc_pause_ns_sum",
        "gc_pause_ns_max",
        "collections",
        "minors",
        "majors",
        "copied",
        "copied_bytes",
        "reclaimed",
        "peak_live",
        "allocs",
        "alloc_bytes",
        "barrier_calls",
        "barrier_recorded",
        "promoted",
    ]
    with out_path.open("w", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=fieldnames, extrasaction="ignore")
        writer.writeheader()
        for row in rows:
            writer.writerow({k: row.get(k, "") for k in fieldnames})

    print(f"\nWrote {out_path}")
    print_comparison(rows, workloads)
    write_analysis(rows, workloads, args.heap, args.trials, args.iters, analysis_path)
    latest = RESULTS / "latest.csv"
    latest.write_text(out_path.read_text())
    (RESULTS / "latest-analysis.md").write_text(analysis_path.read_text())
    print(f"Copied to {latest} and {RESULTS / 'latest-analysis.md'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
