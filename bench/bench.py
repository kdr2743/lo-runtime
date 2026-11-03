#!/usr/bin/env python3
"""GC benchmark suite for the LO runtime.

    bench.py list
    bench.py run    --compiler LO_COMPILER --runtime LABEL=TIMED[:PLAIN] ... --out DIR
    bench.py report DIR

`run` generates the benchmark programs, compiles each to x86-64 assembly with the
LO compiler, links every program against every runtime archive, and runs them
interleaved (rep 1 of every runtime, then rep 2, ...) so that machine drift hits all
runtimes alike. `report` turns DIR/runs.csv into DIR/report.md.

What is measured
  * GC time: the runtime built with `--features gc-time` times every collection
    itself and prints `[lo-gc-time] collections= total_ns= max_ns=` at exit. This is
    the number to compare collectors on. Whole-process wall time is also recorded,
    but it contains the program's own work and process start-up.
  * Correctness: every program's stdout is compared with an expected value computed
    here, in Python, from the program's parameters. Nothing is checked against another
    collector's output, so a bug shared by all collectors would still be caught.

Linking needs x86-64 Linux (`cc`, GNU ld). On Apple Silicon use bench/docker.sh.
Standard library only.
"""
import argparse
import csv
import hashlib
import json
import math
import os
import platform
import re
import statistics
import subprocess
import sys
import time
from pathlib import Path

MIB = 1 << 20


def w32(x):
    """Wrap to a signed 32-bit int, as LO's `int` does."""
    x &= 0xFFFFFFFF
    return x - (1 << 32) if x & 0x80000000 else x


# --------------------------------------------------------------------------- #
# LO source building blocks
# --------------------------------------------------------------------------- #

PAIR_CHURN = """\
class Pair ( int a; int b; ) [
    Pair(int x, int y) { a = x; b = y; }
] {
    int sum() { return (a + b); }
}

class Churn ( ) [
    Churn() { ; }
] {
    int run(int n) {
        int i;
        Pair p;
        i = 0;
        while ( (i < n) ) {
            p = new Pair(i, i);
            i = (i + 1);
        }
        return i;
    }
}
"""

NODE = """\
class Node ( int id; Node next; ) [
    Node(int i, Node n) { id = i; next = n; }
] {
    int get_id() { return id; }
    Node get_next() { return next; }
    void set_next(Node n) { next = n; }
}
"""


def src_churn(p):
    return PAIR_CHURN + """
class Main ( ) {
    int main() {
        Churn c;
        int i;
        c = new Churn();
        i = c.run(@N@);
        out.print_int(i);
        out.println();
        return 0;
    }
}
""".replace("@N@", str(p["n"]))


def exp_churn(p):
    return f"{p['n']}\n"


def src_live_list(p):
    return NODE + PAIR_CHURN + """
class Main ( ) {
    int main() {
        Node head;
        Node cur;
        Churn c;
        int i;
        int sum;
        int count;
        head = null;
        i = 1;
        while ( (i < @L1@) ) {
            head = new Node(i, head);
            i = (i + 1);
        }
        c = new Churn();
        i = c.run(@N@);
        sum = 0;
        count = 0;
        cur = head;
        while ( (! (cur = null)) ) {
            sum = (sum + cur.get_id());
            count = (count + 1);
            cur = cur.get_next();
        }
        out.print_int(count);
        out.println();
        out.print_int(sum);
        out.println();
        return 0;
    }
}
""".replace("@L1@", str(p["live"] + 1)).replace("@N@", str(p["n"]))


def exp_live_list(p):
    n = p["live"]
    return f"{n}\n{w32(n * (n + 1) // 2)}\n"


TREE = """\
class Tree ( Tree left; Tree right; int v; ) [
    Tree(Tree l, Tree r, int x) { left = l; right = r; v = x; }
] {
    int total() {
        int s;
        s = v;
        if ( (! (left = null)) ) {
            s = (s + left.total());
        } else {
            ;
        }
        if ( (! (right = null)) ) {
            s = (s + right.total());
        } else {
            ;
        }
        return s;
    }
}

class Builder ( ) [
    Builder() { ; }
] {
    Tree build(int depth, int base) {
        Tree t;
        if ( (depth = 0) ) {
            t = new Tree(null, null, base);
        } else {
            t = new Tree(this.build((depth - 1), (base * 2)), this.build((depth - 1), ((base * 2) + 1)), base);
        }
        return t;
    }
}
"""


def src_live_tree(p):
    return TREE + PAIR_CHURN + """
class Main ( ) {
    int main() {
        Builder b;
        Tree root;
        Churn c;
        int i;
        b = new Builder();
        root = b.build(@D@, 1);
        c = new Churn();
        i = c.run(@N@);
        out.print_int(root.total());
        out.println();
        return 0;
    }
}
""".replace("@D@", str(p["depth"])).replace("@N@", str(p["n"]))


def exp_live_tree(p):
    n = (1 << (p["depth"] + 1)) - 1  # heap-numbered nodes 1..n
    return f"{w32(n * (n + 1) // 2)}\n"


def src_queue_window(p):
    return NODE + """
class Main ( ) {
    int main() {
        Node head;
        Node tail;
        Node n;
        Node cur;
        int i;
        int sum;
        head = new Node(0, null);
        tail = head;
        i = 1;
        while ( (i < @K@) ) {
            n = new Node(i, null);
            tail.set_next(n);
            tail = n;
            i = (i + 1);
        }
        while ( (i < @ITERS@) ) {
            n = new Node(i, null);
            tail.set_next(n);
            tail = n;
            head = head.get_next();
            i = (i + 1);
        }
        sum = 0;
        cur = head;
        while ( (! (cur = null)) ) {
            sum = (sum + cur.get_id());
            cur = cur.get_next();
        }
        out.print_int(sum);
        out.println();
        return 0;
    }
}
""".replace("@K@", str(p["window"])).replace("@ITERS@", str(p["iters"]))


def exp_queue_window(p):
    k, it = p["window"], p["iters"]
    lo, hi = it - k, it - 1  # the window holds ids it-k .. it-1
    return f"{w32((lo + hi) * k // 2)}\n"


HOLDER = """\
class Payload ( int v; ) [
    Payload(int x) { v = x; }
] {
    int get() { return v; }
}

class Holder ( Payload ref; Holder next; ) [
    Holder(Payload p, Holder n) { ref = p; next = n; }
] {
    void set(Payload p) { ref = p; }
    Payload get_ref() { return ref; }
    Holder get_next() { return next; }
}
"""


def src_old_to_young(p):
    return HOLDER + """
class Main ( ) {
    int main() {
        Holder first;
        Holder cur;
        int i;
        int it;
        int sum;
        first = null;
        i = 0;
        while ( (i < @M@) ) {
            first = new Holder(new Payload(0), first);
            i = (i + 1);
        }
        cur = first;
        it = 0;
        while ( (it < @ITERS@) ) {
            cur.set(new Payload(it));
            cur = cur.get_next();
            if ( (cur = null) ) {
                cur = first;
            } else {
                ;
            }
            it = (it + 1);
        }
        sum = 0;
        cur = first;
        while ( (! (cur = null)) ) {
            sum = (sum + (cur.get_ref()).get());
            cur = cur.get_next();
        }
        out.print_int(sum);
        out.println();
        return 0;
    }
}
""".replace("@M@", str(p["holders"])).replace("@ITERS@", str(p["iters"]))


def exp_old_to_young(p):
    m, it = p["holders"], p["iters"]
    total = 0
    for j in range(m):  # holder j is stored to at it = j, j+m, ... < iters
        if j < it:
            total += j + m * ((it - 1 - j) // m)
    return f"{w32(total)}\n"


def src_deep_roots(p):
    return """\
class Node ( int id; ) [
    Node(int i) { id = i; }
] {
    int get_id() { return id; }
}

""" + PAIR_CHURN + """
class Rec ( ) [
    Rec() { ; }
] {
    int descend(int depth, Node carried) {
        Node mine;
        Churn c;
        int below;
        int junk;
        mine = new Node(depth);
        if ( (depth = @D@) ) {
            c = new Churn();
            junk = c.run(@N@);
            below = 0;
        } else {
            below = this.descend((depth + 1), mine);
        }
        return (below + depth);
    }
}

class Main ( ) {
    int main() {
        Rec r;
        r = new Rec();
        out.print_int(r.descend(1, new Node(0)));
        out.println();
        return 0;
    }
}
""".replace("@D@", str(p["depth"])).replace("@N@", str(p["n"]))


def exp_deep_roots(p):
    d = p["depth"]
    return f"{w32(d * (d + 1) // 2)}\n"


# --------------------------------------------------------------------------- #
# The matrix
# --------------------------------------------------------------------------- #
# `iters`-like fields are multiplied by --scale; live-set sizes are not, because
# they are what the benchmark is about.

FAMILIES = {
    "churn": dict(
        what="garbage only; no live data. The floor: per-collection fixed cost.",
        src=src_churn, exp=exp_churn, scaled=("n",),
        points=[dict(n=20_000_000)],
    ),
    "live_list": dict(
        what="a linked list stays live (32 B/node) while garbage is churned. "
             "Copying collectors pay for the live set on every collection.",
        src=src_live_list, exp=exp_live_list, scaled=("n",),
        points=[dict(live=l, n=20_000_000) for l in (10_000, 50_000, 100_000)],
    ),
    "live_tree": dict(
        what="a complete binary tree stays live while garbage is churned "
             "(two pointers per node, so more tracing work per byte).",
        src=src_live_tree, exp=exp_live_tree, scaled=("n",),
        points=[dict(depth=d, n=20_000_000) for d in (12, 14, 16)],
    ),
    "queue_window": dict(
        what="a sliding window of K linked nodes: each node lives for K "
             "allocations then dies. Window below the nursery size dies young; "
             "above it, nodes are promoted and then die in tenured space, and a "
             "dead tenured tail that points at young nodes keeps them alive "
             "(nepotism).",
        src=src_queue_window, exp=exp_queue_window, scaled=("iters",),
        points=[dict(window=k, iters=6_000_000) for k in (1_000, 10_000, 30_000, 100_000, 200_000)],
    ),
    "old_to_young": dict(
        what="M long-lived holder objects are overwritten round-robin with fresh "
             "objects: every store is old-to-young, so the write barrier and the "
             "remembered set are on the hot path.",
        src=src_old_to_young, exp=exp_old_to_young, scaled=("iters",),
        points=[dict(holders=m, iters=4_000_000) for m in (100, 10_000, 50_000)],
    ),
    "deep_roots": dict(
        what="recursion D frames deep with a root in every frame, then garbage "
             "is churned at the bottom: every collection scans D shadow-stack frames.",
        src=src_deep_roots, exp=exp_deep_roots, scaled=("n",),
        points=[dict(depth=d, n=4_000_000) for d in (100, 500, 1_000)],
    ),
}

HEAPS = {"default": None, "8M": 8 * MIB}  # LO_HEAP_SIZE; None = runtime default (16 MiB)


def point_name(family, p, scale):
    key = "_".join(f"{k}{v}" for k, v in p.items() if k not in FAMILIES[family]["scaled"])
    return f"{family}__{key}" if key else family


def scaled(family, p, scale):
    q = dict(p)
    for k in FAMILIES[family]["scaled"]:
        q[k] = max(1, int(q[k] * scale))
    return q


def all_programs(scale, only):
    out = []
    for fam, spec in FAMILIES.items():
        for p in spec["points"]:
            name = point_name(fam, p, scale)
            if only and not re.search(only, name):
                continue
            q = scaled(fam, p, scale)
            out.append(dict(name=name, family=fam, params=q, src=spec["src"](q), expected=spec["exp"](q)))
    return out


# --------------------------------------------------------------------------- #
# run
# --------------------------------------------------------------------------- #

def sh(cmd, **kw):
    r = subprocess.run(cmd, capture_output=True, text=True, **kw)
    if r.returncode != 0:
        sys.exit(f"command failed: {' '.join(map(str, cmd))}\n{r.stderr}")
    return r


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()[:16]


def parse_runtime(spec):
    label, _, rest = spec.partition("=")
    timed, _, plain = rest.partition(":")
    if not label or not timed:
        sys.exit(f"bad --runtime {spec!r}; want LABEL=TIMED_ARCHIVE[:PLAIN_ARCHIVE]")
    return label, timed, plain or None


def cmd_run(a):
    if platform.system() != "Linux" or platform.machine() not in ("x86_64", "AMD64"):
        sys.exit("run needs x86-64 Linux to link the generated assembly "
                 f"(this is {platform.system()}/{platform.machine()}); use bench/docker.sh")
    out = Path(a.out)
    for d in ("programs", "asm", "bin"):
        (out / d).mkdir(parents=True, exist_ok=True)
    runtimes = [parse_runtime(s) for s in a.runtime]
    labels = [r[0] for r in runtimes]
    if len(set(labels)) != len(labels):
        sys.exit("duplicate runtime labels")
    progs = all_programs(a.scale, a.only)
    if not progs:
        sys.exit("no benchmark matches --only")

    variants = []  # (label, build, archive)
    for label, timed, plain in runtimes:
        variants.append((label, "timed", timed))
        if plain and a.plain_reps > 0:
            variants.append((label, "plain", plain))

    meta = dict(
        date=time.strftime("%Y-%m-%d %H:%M:%S"), machine=platform.machine(),
        cpu=next((l.split(":", 1)[1].strip() for l in open("/proc/cpuinfo") if l.startswith("model name")), "?"),
        cpus=os.cpu_count(), reps=a.reps, plain_reps=a.plain_reps, scale=a.scale,
        archives={f"{l}/{b}": [arc, sha(arc)] for l, b, arc in variants},
    )
    (out / "meta.json").write_text(json.dumps(meta, indent=2))

    for pr in progs:
        src = out / "programs" / f"{pr['name']}.lo"
        asm = out / "asm" / f"{pr['name']}.s"
        src.write_text(pr["src"])
        sh([a.compiler, "--emit-asm", str(src), "-o", str(asm)])
        for label, build, arc in variants:
            sh(["cc", "-no-pie", "-o", str(out / "bin" / f"{pr['name']}.{label}.{build}"),
                str(asm), arc, "-lpthread", "-ldl", "-lm"])
        (out / "programs" / f"{pr['name']}.expected").write_text(pr["expected"])

    rows = []
    total = len(progs) * len(HEAPS)
    done = 0
    for pr in progs:
        for heap, heap_bytes in HEAPS.items():
            env = dict(os.environ)
            env.pop("LO_HEAP_SIZE", None)
            if heap_bytes:
                env["LO_HEAP_SIZE"] = str(heap_bytes)
            for rep in range(1, a.reps + 1):
                for label, build, arc in variants:
                    if build == "plain" and rep > a.plain_reps:
                        continue
                    exe = out / "bin" / f"{pr['name']}.{label}.{build}"
                    t0 = time.perf_counter()
                    r = subprocess.run([str(exe)], capture_output=True, env=env, timeout=a.timeout)
                    wall_ms = (time.perf_counter() - t0) * 1000
                    err = r.stderr.decode(errors="replace")
                    m = re.findall(r"\[lo-gc-time\] collections=(\d+) total_ns=(\d+) max_ns=(\d+)", err)
                    n, tot, mx = (m[-1] if m else ("", "", ""))
                    rows.append(dict(
                        program=pr["name"], family=pr["family"], heap=heap, runtime=label,
                        build=build, rep=rep, rc=r.returncode,
                        ok="OK" if r.stdout.decode(errors="replace") == pr["expected"] else "DIFF",
                        wall_ms=f"{wall_ms:.3f}", collections=n,
                        gc_total_ms=f"{int(tot) / 1e6:.4f}" if tot else "",
                        gc_max_ms=f"{int(mx) / 1e6:.4f}" if mx else ""))
            done += 1
            print(f"[{done}/{total}] {pr['name']} heap={heap}", flush=True)

    with open(out / "runs.csv", "w", newline="") as fh:
        w = csv.DictWriter(fh, fieldnames=list(rows[0]))
        w.writeheader()
        w.writerows(rows)
    print(f"wrote {out / 'runs.csv'} ({len(rows)} runs); now: bench.py report {out}")


# --------------------------------------------------------------------------- #
# report
# --------------------------------------------------------------------------- #

TIE_REL, TIE_ABS_MS = 1.10, 1.5


def near_best(g, best):
    """Within noise of the best GC time: 10 % or 1.5 ms, whichever is larger. Sub-millisecond
    differences are clock jitter and one-time warm-up, not collector quality."""
    return g <= max(best * TIE_REL, best + TIE_ABS_MS)


def med(xs):
    return statistics.median(xs) if xs else float("nan")


def cv(xs):
    return 100 * statistics.pstdev(xs) / statistics.mean(xs) if len(xs) > 1 and statistics.mean(xs) else 0.0


def fmt(x, d=1):
    return "—" if x is None or (isinstance(x, float) and math.isnan(x)) else f"{x:.{d}f}"


def cmd_report(a):
    d = Path(a.dir)
    meta = json.loads((d / "meta.json").read_text())
    rows = list(csv.DictReader(open(d / "runs.csv")))
    labels = list(dict.fromkeys(r["runtime"] for r in rows))
    progs = list(dict.fromkeys(r["program"] for r in rows))
    heaps = list(dict.fromkeys(r["heap"] for r in rows))
    cell = {}
    for r in rows:
        cell.setdefault((r["program"], r["heap"], r["runtime"], r["build"]), []).append(r)
    out = []
    P = out.append

    P("# GC benchmark report\n")
    P(f"Run {meta['date']} on {meta['cpu']} ({meta['machine']}, {meta['cpus']} cpus); "
      f"{meta['reps']} repetitions per cell (plain builds: {meta['plain_reps']}), scale {meta['scale']}. "
      "Repetitions are interleaved across runtimes.\n")
    P("| runtime | build | archive sha256 (16) |\n|---|---|---|")
    for k, (arc, h) in meta["archives"].items():
        P(f"| {k.split('/')[0]} | {k.split('/')[1]} | `{h}` |")

    # ---------------- validity ----------------
    P("\n## Validity checks\n")
    P("**V1. Output equals the independently computed expected value; exit codes.** "
      "A runtime that runs out of memory exits 137 with no output; that is reported as OOM, "
      "not as an error, because a smaller heap is part of the experiment.\n")
    P("| runtime | build | runs | output OK | output wrong | rc 0 | rc 137 (OOM) | other rc |\n|---|---|---|---|---|---|---|---|")
    problems = []
    for label in labels:
        for build in ("timed", "plain"):
            rs = [r for r in rows if r["runtime"] == label and r["build"] == build]
            if not rs:
                continue
            ok_runs = [r for r in rs if r["rc"] == "0"]
            wrong = [r for r in ok_runs if r["ok"] != "OK"]
            other = [r for r in rs if r["rc"] not in ("0", "137")]
            for r in wrong + other:
                problems.append((r["program"], r["heap"], label, build, r["rc"], r["ok"]))
            P(f"| {label} | {build} | {len(rs)} | {sum(r['ok'] == 'OK' for r in rs)} | {len(wrong)} | "
              f"{len(ok_runs)} | {sum(r['rc'] == '137' for r in rs)} | {len(other)} |")
    P("\nRuns with a wrong answer or an unexpected exit code: "
      + (", ".join(sorted({"/".join(map(str, p)) for p in problems})) if problems else "**none**") + "\n")

    nd = [k for k, rs in cell.items() if k[3] == "timed" and len({r["collections"] for r in rs}) > 1]
    P(f"**V2. Collection counts are repeatable.** Cells whose count varied between repetitions: "
      f"**{len(nd)}** of {sum(1 for k in cell if k[3] == 'timed')}. "
      "(Collection counts are deterministic for a single-threaded program, so any variation means the harness is broken.)\n")

    viol = [k for k, rs in cell.items() if k[3] == "timed" and all(r["gc_total_ms"] for r in rs)
            and med([float(r["gc_total_ms"]) for r in rs]) > med([float(r["wall_ms"]) for r in rs])]
    P(f"**V3. GC time never exceeds wall time.** Violations: **{len(viol)}**.\n")

    maxgt = [k for k, rs in cell.items() if k[3] == "timed" and all(r["gc_total_ms"] for r in rs)
             and any(float(r["gc_max_ms"]) > float(r["gc_total_ms"]) + 1e-6 for r in rs)]
    P(f"**V4. The longest pause never exceeds total GC time.** Violations: **{len(maxgt)}**.\n")

    pl = [(l, sum(med([float(r["wall_ms"]) for r in cell[(p, h, l, "plain")]]) for p in progs for h in heaps if (p, h, l, "plain") in cell),
           sum(med([float(r["wall_ms"]) for r in cell[(p, h, l, "timed")]]) for p in progs for h in heaps if (p, h, l, "plain") in cell))
          for l in labels]
    pl = [x for x in pl if x[1] > 0]
    if pl:
        P("**V5. Timer overhead** (median wall, timed vs plain build, summed over cells that have both):\n")
        P("| runtime | plain ms | timed ms | overhead |\n|---|---|---|---|")
        for l, tp, tt in pl:
            P(f"| {l} | {tp:.0f} | {tt:.0f} | {100 * (tt / tp - 1):+.1f}% |")
        P("")
    else:
        P("**V5. Timer overhead:** not measured (no plain archives given; run with `LABEL=TIMED:PLAIN`).\n")

    P("**V6. Noise.** Coefficient of variation of GC time and of wall time across repetitions (timed builds):\n")
    P("| runtime | metric | cells | median CV | 90th pct CV | max CV | cells with CV > 15% |\n|---|---|---|---|---|---|---|")
    for l in labels:
        for metric, key in (("GC time", "gc_total_ms"), ("wall time", "wall_ms")):
            cvs = sorted(cv([float(r[key]) for r in rs]) for k, rs in cell.items()
                         if k[2] == l and k[3] == "timed" and all(r[key] for r in rs) and rs[0]["rc"] == "0")
            if cvs:
                P(f"| {l} | {metric} | {len(cvs)} | {med(cvs):.1f}% | {cvs[int(len(cvs) * .9)]:.1f}% | {cvs[-1]:.1f}% | {sum(x > 15 for x in cvs)} |")
    P("\nTiny GC times (under ~1 ms) are dominated by clock resolution and run-to-run jitter; "
      "compare totals and ratios on cells with at least a few ms of GC.\n")

    # ---------------- results ----------------
    P("## Results\n")
    P("Each cell: median over repetitions. **GC ms** = total time inside `collect`; **max pause** = longest single "
      "collection; **GC %** = GC ms / wall ms of the same (timed) runs; **wall** = process wall time (min–max in "
      "brackets); **OOM** = exit 137. The **best** column names the runtime with the lowest GC ms; runtimes within "
      "max(10 %, 1.5 ms) of it are shown as a tie (`~`). The first collection of a generational runtime costs about 1 ms "
      "on its own (see *max pause* on garbage-only programs), which is why differences under 1.5 ms are not ranked.\n")
    for fam, spec in FAMILIES.items():
        fprogs = [p for p in progs if p.split("__")[0] == fam]
        if not fprogs:
            continue
        P(f"### {fam}\n\n{spec['what']}\n")
        for heap in heaps:
            P(f"**heap: {heap}** ({'runtime default, 16 MiB' if heap == 'default' else 'LO_HEAP_SIZE=' + str(HEAPS[heap])})\n")
            hdr = "| program | " + " | ".join(f"{l} colls | {l} GC ms | {l} max pause ms | {l} GC % | {l} wall ms (min–max)" for l in labels) + " | best |"
            P(hdr)
            P("|" + "---|" * (2 + 5 * len(labels)))
            for p in fprogs:
                cols, gcs = [], {}
                for l in labels:
                    rs = cell.get((p, heap, l, "timed"), [])
                    if not rs:
                        cols += ["—"] * 5
                        continue
                    oom = rs[0]["rc"] == "137"
                    wall = [float(r["wall_ms"]) for r in rs]
                    if oom or not all(r["gc_total_ms"] for r in rs):
                        cols += ["OOM", "OOM", "OOM", "OOM", f"{med(wall):.0f}"]
                        continue
                    g = [float(r["gc_total_ms"]) for r in rs]
                    mx = [float(r["gc_max_ms"]) for r in rs]
                    gcs[l] = med(g)
                    cols += [rs[0]["collections"], fmt(med(g)), fmt(med(mx), 2), f"{100 * med(g) / med(wall):.0f}%",
                             f"{med(wall):.0f} ({min(wall):.0f}–{max(wall):.0f})"]
                best = "—"
                if gcs:
                    order = sorted(gcs, key=gcs.get)
                    best = order[0]
                    ties = [l for l in order[1:] if near_best(gcs[l], gcs[order[0]])]
                    if ties:
                        best = "~ " + "/".join([order[0]] + ties)
                P(f"| {p.split('__', 1)[1] if '__' in p else p} | " + " | ".join(cols) + f" | {best} |")
            P("")

    # ---------------- summary ----------------
    P("## Summary\n")
    P("Total GC time over every cell where all runtimes completed (so an OOM in one runtime excludes that cell for all). "
      "A cell is *decided* when exactly one runtime is within max(10 %, 1.5 ms) of the best GC time; otherwise every runtime "
      "within that margin is *tied*.\n")
    both = [(p, h) for p in progs for h in heaps
            if all((p, h, l, "timed") in cell and cell[(p, h, l, "timed")][0]["rc"] == "0"
                   and all(r["gc_total_ms"] for r in cell[(p, h, l, "timed")]) for l in labels)]
    P("| runtime | total GC ms | sum of per-cell longest pause ms | cells decided in its favour | cells tied for best | cells clearly behind |\n|---|---|---|---|---|---|")
    tot = {l: 0.0 for l in labels}; mxs = {l: 0.0 for l in labels}
    won = {l: 0 for l in labels}; tied = {l: 0 for l in labels}; behind = {l: 0 for l in labels}
    for p, h in both:
        g = {l: med([float(r["gc_total_ms"]) for r in cell[(p, h, l, "timed")]]) for l in labels}
        best = min(g.values())
        close = [l for l in labels if near_best(g[l], best)]
        for l in labels:
            tot[l] += g[l]
            mxs[l] += med([float(r["gc_max_ms"]) for r in cell[(p, h, l, "timed")]])
            if l not in close:
                behind[l] += 1
            elif len(close) == 1:
                won[l] += 1
            else:
                tied[l] += 1
    for l in labels:
        P(f"| {l} | {tot[l]:.0f} | {mxs[l]:.0f} | {won[l]} | {tied[l]} | {behind[l]} |")
    P(f"\n{len(both)} cells counted of {len(progs) * len(heaps)}. "
      "OOM cells: " + (", ".join(sorted({f'{l}:{p.split("__", 1)[-1]}@{h}' for (p, h, l, b), rs in cell.items() if b == 'timed' and rs[0]['rc'] == '137'})) or "none") + ".\n")
    # One row per (program, heap, runtime, build): the aggregate of the repetitions.
    with open(d / "summary.csv", "w", newline="") as fh:
        w = csv.writer(fh)
        w.writerow(["program", "family", "heap", "runtime", "build", "reps", "exit_code", "output_ok",
                    "collections", "gc_ms_median", "gc_ms_min", "gc_ms_max", "gc_ms_cv_pct",
                    "max_pause_ms_median", "gc_pct_of_wall", "wall_ms_median", "wall_ms_min", "wall_ms_max",
                    "wall_ms_cv_pct"])
        for (p_, h, l, b), rs in cell.items():
            wall = [float(r["wall_ms"]) for r in rs]
            have = all(r["gc_total_ms"] for r in rs)
            g = [float(r["gc_total_ms"]) for r in rs] if have else []
            mx = [float(r["gc_max_ms"]) for r in rs] if have else []
            w.writerow([p_, rs[0]["family"], h, l, b, len(rs), rs[0]["rc"],
                        "OK" if all(r["ok"] == "OK" for r in rs) else ("OOM" if rs[0]["rc"] == "137" else "WRONG"),
                        rs[0]["collections"] if have else "",
                        f"{med(g):.4f}" if g else "", f"{min(g):.4f}" if g else "", f"{max(g):.4f}" if g else "",
                        f"{cv(g):.2f}" if g else "", f"{med(mx):.4f}" if mx else "",
                        f"{100 * med(g) / med(wall):.2f}" if g else "",
                        f"{med(wall):.3f}", f"{min(wall):.3f}", f"{max(wall):.3f}", f"{cv(wall):.2f}"])
    (d / "report.md").write_text("\n".join(out))
    print(f"wrote {d / 'report.md'} and {d / 'summary.csv'}")


def cmd_list(a):
    for p in all_programs(a.scale, a.only):
        print(f"{p['name']:40s} {p['params']}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    l = sub.add_parser("list"); l.add_argument("--scale", type=float, default=1.0); l.add_argument("--only")
    l.set_defaults(fn=cmd_list)
    r = sub.add_parser("run")
    r.add_argument("--compiler", required=True, help="path to the lo-compiler binary (supports --emit-asm)")
    r.add_argument("--runtime", action="append", required=True, metavar="LABEL=TIMED[:PLAIN]",
                   help="runtime archive built with --features gc-time, optionally with a plain archive for overhead check")
    r.add_argument("--out", required=True)
    r.add_argument("--reps", type=int, default=11)
    r.add_argument("--plain-reps", type=int, default=3)
    r.add_argument("--scale", type=float, default=1.0, help="multiply iteration counts (not live-set sizes)")
    r.add_argument("--only", help="regex on benchmark names")
    r.add_argument("--timeout", type=int, default=300)
    r.set_defaults(fn=cmd_run)
    rp = sub.add_parser("report"); rp.add_argument("dir"); rp.set_defaults(fn=cmd_report)
    a = ap.parse_args()
    a.fn(a)


if __name__ == "__main__":
    main()
