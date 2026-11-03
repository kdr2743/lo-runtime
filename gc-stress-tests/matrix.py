#!/usr/bin/env python3
"""Run the contributed GC stress tests against several runtimes and record the results.

    matrix.py run    --target native|wasm --compiler LO_COMPILER --lo-testing DIR \
                     --lib LABEL=ARCHIVE [--lib ...] --out results-<target>.csv
    matrix.py report results-native.csv results-wasm.csv --out results.md

`run` builds each LO test once, links it against every archive, and runs it (native: at the
default heap and at the `gc stress heap` in its header; WASM: at the default heap, since a WASM
module has no environment). The C tests under abi/ go through the instructor's run-harness.sh.
One CSV row per (test, runtime, config). `report` turns the CSVs into a pivot table.

Native needs x86-64 Linux (as, cc). WASM needs clang (wasm32), wasm-ld and wasmrun
(override with CLANG, WASM_LD, WASMRUN). Standard library only.
"""
import argparse
import csv
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
FIELDS = ["test", "runtime", "target", "config", "exit_code", "result", "wall_ms", "note"]


def header_heap(src):
    for line in src.read_text().splitlines():
        m = re.match(r"//\s*gc stress heap:\s*(\d+)", line)
        if m:
            return int(m.group(1))
    return None


def run_one(cmd, env, stdin_null=True, timeout=300):
    t0 = time.perf_counter()
    try:
        r = subprocess.run(cmd, capture_output=True, env=env, timeout=timeout,
                           stdin=subprocess.DEVNULL if stdin_null else None)
        return r.returncode, r.stdout, (time.perf_counter() - t0) * 1000, r.stderr.decode(errors="replace")[:200]
    except subprocess.TimeoutExpired:
        return 124, b"", (time.perf_counter() - t0) * 1000, "timeout"


def cmd_run(a):
    libs = []
    for spec in a.lib:
        label, _, path = spec.partition("=")
        if not label or not Path(path).exists():
            sys.exit(f"bad --lib {spec!r}: archive not found")
        libs.append((label, path))
    tools = dict(CLANG=os.environ.get("CLANG", "clang"), WASM_LD=os.environ.get("WASM_LD", "wasm-ld"),
                 WASMRUN=os.environ.get("WASMRUN", "wasmrun"))
    work = Path(tempfile.mkdtemp(prefix="lo-stress."))
    rows = []

    def add(test, label, config, rc, expected, out, ms, note=""):
        ok = rc == 0 and out == expected
        rows.append(dict(test=test, runtime=label, target=a.target, config=config, exit_code=rc,
                         result="PASS" if ok else "FAIL", wall_ms=f"{ms:.0f}", note=note))

    for src in sorted(HERE.glob("*.lo")):
        name = src.stem
        expected = (HERE / f"{name}.expected.out").read_bytes()
        heap = header_heap(src)
        s = work / f"{name}.s"; o = work / f"{name}.o"
        if a.target == "native":
            cmds = [[a.compiler, "--emit-asm", str(src), "-o", str(s)], ["as", "--64", "-o", str(o), str(s)]]
        else:
            cmds = [[a.compiler, "--emit", str(src), "-o", str(s)],
                    [tools["CLANG"], "--target=wasm32-unknown-unknown", "-c", str(s), "-o", str(o)]]
        if any(subprocess.run(c, capture_output=True).returncode != 0 for c in cmds):
            for label, _ in libs:
                add(name, label, "build", 1, b"", b"", 0, "compile failed")
            continue
        for label, lib in libs:
            exe = work / f"{name}.{label}"
            if a.target == "native":
                link = ["cc", "-no-pie", "-o", str(exe), str(o), lib, "-lpthread", "-ldl", "-lm"]
            else:
                exe = exe.with_suffix(".wasm")
                link = [tools["WASM_LD"], "--no-entry", "--export=lo_entry", "--allow-undefined", str(o), lib, "-o", str(exe)]
            if subprocess.run(link, capture_output=True).returncode != 0:
                add(name, label, "build", 1, b"", b"", 0, "link failed")
                continue
            run = [str(exe)] if a.target == "native" else [tools["WASMRUN"], str(exe)]
            env = {k: v for k, v in os.environ.items() if k != "LO_HEAP_SIZE"}
            rc, out, ms, note = run_one(run, env)
            add(name, label, "default heap", rc, expected, out, ms, note)
            if a.target == "native" and heap:
                env["LO_HEAP_SIZE"] = str(heap)
                rc, out, ms, note = run_one(run, env)
                add(name, label, f"LO_HEAP_SIZE={heap}", rc, expected, out, ms, note)

    harness = Path(a.lo_testing) / "p3-abi-harness" / "run-harness.sh"
    for label, lib in libs:
        env = dict(os.environ, **{k: v for k, v in tools.items()})
        t0 = time.perf_counter()
        r = subprocess.run(["bash", str(harness), a.target, lib, str(HERE / "abi")], capture_output=True, text=True, env=env)
        ms = (time.perf_counter() - t0) * 1000
        seen = set()
        for line in r.stdout.splitlines():
            m = re.match(r"(PASS|FAIL) (\S+)(?: (.*))?", line)
            if m and not line.endswith("passed") and "programs" not in line:
                res, name, note = m.group(1), m.group(2), m.group(3) or ""
                seen.add(name)
                rows.append(dict(test=name, runtime=label, target=a.target, config="C harness",
                                 exit_code=0 if res == "PASS" else 1, result=res, wall_ms=f"{ms / max(1, len(seen)):.0f}", note=note))
    with open(a.out, "w", newline="") as fh:
        w = csv.DictWriter(fh, fieldnames=FIELDS)
        w.writeheader(); w.writerows(rows)
    bad = [r for r in rows if r["result"] != "PASS"]
    print(f"{len(rows)} runs, {len(rows) - len(bad)} PASS, {len(bad)} FAIL -> {a.out}")


def cmd_report(a):
    rows = []
    for p in a.csv:
        rows += list(csv.DictReader(open(p)))
    labels = list(dict.fromkeys(r["runtime"] for r in rows))
    cases = list(dict.fromkeys((r["test"], r["target"], r["config"]) for r in rows))
    cell = {(r["test"], r["target"], r["config"], r["runtime"]): r for r in rows}
    out = ["# GC stress test results\n",
           "PASS means exit status 0 and stdout equal to the expected output computed in Python. "
           "`broken-*` runtimes are deliberate bugs and are *supposed* to fail: that is the check that the tests bite.\n",
           "| test | target | config | " + " | ".join(labels) + " |",
           "|---|---|---|" + "---|" * len(labels)]
    for t, tg, cfg in cases:
        vals = []
        for l in labels:
            r = cell.get((t, tg, cfg, l))
            vals.append("—" if r is None else r["result"] + ("" if r["result"] == "PASS" else f" ({r['exit_code']})"))
        out.append(f"| {t.replace('contributed_', '')} | {tg} | {cfg} | " + " | ".join(vals) + " |")
    out.append("")
    out.append("| runtime | passed | failed |\n|---|---|---|")
    for l in labels:
        rs = [r for r in rows if r["runtime"] == l]
        out.append(f"| {l} | {sum(r['result'] == 'PASS' for r in rs)} | {sum(r['result'] != 'PASS' for r in rs)} |")
    Path(a.out).write_text("\n".join(out) + "\n")
    print(f"wrote {a.out}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("--target", choices=["native", "wasm"], required=True)
    r.add_argument("--compiler", required=True)
    r.add_argument("--lo-testing", required=True, help="checkout of the course lo-testing repo (for p3-abi-harness)")
    r.add_argument("--lib", action="append", required=True, metavar="LABEL=ARCHIVE")
    r.add_argument("--out", required=True)
    r.set_defaults(fn=cmd_run)
    rp = sub.add_parser("report")
    rp.add_argument("csv", nargs="+")
    rp.add_argument("--out", required=True)
    rp.set_defaults(fn=cmd_report)
    a = ap.parse_args()
    a.fn(a)


if __name__ == "__main__":
    main()
