#!/usr/bin/env python3
"""Build each GC feature variant and run shared workloads against Cheney.

Emits CSV under benchmarks/results/ and a comparison table on stdout.

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


def cargo_build(feature: str) -> None:
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


def gc_bench_bin() -> Path:
    # cargo's CARGO_TARGET_DIR may redirect artifacts; prefer env override.
    target = Path(os.environ.get("CARGO_TARGET_DIR", RUST / "target"))
    return target / "release" / "gc_bench"


def run_one(
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
    binary = gc_bench_bin()
    if not binary.is_file():
        raise RuntimeError(f"missing {binary}; build with cargo first")
    cmd = [str(binary), workload]
    proc = subprocess.run(
        cmd,
        cwd=RUST,
        env=env,
        check=False,
        capture_output=True,
        text=True,
    )
    if proc.returncode == 137:
        return {
            "collector": feature.removeprefix("gc_"),
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
    fields["oom"] = "0"
    return fields


def median_or_empty(values: list[float]) -> str:
    if not values:
        return ""
    return f"{statistics.median(values):.0f}"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--trials",
        type=int,
        default=3,
        help="timed trials per collector×workload (default 3)",
    )
    parser.add_argument(
        "--heap",
        type=int,
        default=16 * 1024 * 1024,
        help="LO_HEAP_SIZE in bytes (default 16 MiB)",
    )
    parser.add_argument(
        "--workloads",
        default=",".join(DEFAULT_WORKLOADS),
        help="comma-separated workload names",
    )
    parser.add_argument(
        "--iters",
        type=int,
        default=None,
        help="override LO_GC_BENCH_ITERS for every workload",
    )
    parser.add_argument(
        "--skip-build",
        action="store_true",
        help="assume release binaries are already built for each feature",
    )
    args = parser.parse_args()
    workloads = [w.strip() for w in args.workloads.split(",") if w.strip()]
    if not workloads:
        parser.error("no workloads")

    RESULTS.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    out_path = RESULTS / f"gc-bench-{stamp}.csv"

    rows: list[dict[str, str]] = []
    for name, feature in COLLECTORS:
        if not args.skip_build:
            cargo_build(feature)
        for workload in workloads:
            print(f"== {name} / {workload} ({args.trials} trials) ==", flush=True)
            for trial in range(1, args.trials + 1):
                fields = run_one(feature, workload, args.heap, args.iters, {})
                fields["trial"] = str(trial)
                # Prefer the binary's collector= field when present.
                fields.setdefault("collector", name)
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
    # Stable "latest" copy for convenience.
    latest = RESULTS / "latest.csv"
    latest.write_text(out_path.read_text())
    print(f"Copied to {latest}")
    return 0


def print_comparison(rows: list[dict[str, str]], workloads: list[str]) -> None:
    print("\nMedian wall_ns / collections (non-OOM trials):")
    header = f"{'workload':<12} {'collector':<14} {'wall_ns':>12} {'collections':>12} {'pause_sum':>12}"
    print(header)
    print("-" * len(header))
    for workload in workloads:
        for name, _feature in COLLECTORS:
            subset = [
                r
                for r in rows
                if r.get("collector") == name
                and r.get("workload") == workload
                and r.get("oom") != "1"
                and r.get("wall_ns")
            ]
            walls = [float(r["wall_ns"]) for r in subset]
            cols = [float(r["collections"]) for r in subset if r.get("collections")]
            pauses = [
                float(r["gc_pause_ns_sum"])
                for r in subset
                if r.get("gc_pause_ns_sum")
            ]
            print(
                f"{workload:<12} {name:<14} {median_or_empty(walls):>12} "
                f"{median_or_empty(cols):>12} {median_or_empty(pauses):>12}"
            )


if __name__ == "__main__":
    raise SystemExit(main())
