#!/usr/bin/env python3
"""Plot a benchmark run: SVG charts and an index.html, standard library only.

    plot.py RUN_DIR [--out DIR]

RUN_DIR is a directory written by `bench.py report` (it needs summary.csv). Output goes to
RUN_DIR/plots/ unless --out is given:

    <family>.svg   one chart per benchmark family: GC time and longest pause against the
                   family's size parameter, one line per collector, at each heap size
    summary.svg    total GC time and summed longest pause per collector
    index.html     all of the above on one page

How to read a family chart: each column of panels is a heap size (left: default 16 MiB,
right: 8 MiB). The top row is total GC time (dot = median of the repetitions, whisker = min
to max); the bottom row is the longest single pause. Both axes use a log scale because the
values span four orders of magnitude. A "x" at the top of a panel means that collector ran
out of memory (exit 137) at that point, so there is no number for it.
"""
import argparse
import csv
import html
import math
import re
from pathlib import Path

ORDER = ["cheney", "markcompact", "generational", "diddy", "pushed", "mine"]
COLORS = {  # colour-blind-safe, distinguishable in grey
    "cheney": "#2b6cb0",
    "markcompact": "#8e5bb5",
    "generational": "#d9822b",
    "diddy": "#1f9d55",
    "pushed": "#d9822b",
    "mine": "#1f9d55",
}
FALLBACK = ["#c0392b", "#16a0a0", "#7f8c8d", "#b8860b"]
FONT = "font-family='-apple-system,Segoe UI,Helvetica,Arial,sans-serif'"
INK, MUTED, GRID = "#1a202c", "#5f6b7a", "#e3e8ee"


def esc(s):
    return html.escape(str(s), quote=True)


def color_for(runtime, seen):
    if runtime in COLORS:
        return COLORS[runtime]
    return FALLBACK[(len(seen) - 1) % len(FALLBACK)]


def fmt_param(v):
    if v >= 1000 and v % 1000 == 0:
        return f"{v // 1000}k"
    return str(v)


def fmt_tick(v):
    if v >= 1000:
        return f"{v:g}"
    if v >= 1:
        return f"{v:g}"
    return f"{v:.3g}"


def load(run_dir):
    rows = []
    with open(Path(run_dir) / "summary.csv") as fh:
        for r in csv.DictReader(fh):
            if r["build"] != "timed":
                continue
            ok = r["output_ok"]
            r["oom"] = ok == "OOM"
            r["valid"] = ok == "OK"
            for k in ("gc_ms_median", "gc_ms_min", "gc_ms_max", "max_pause_ms_median"):
                r[k] = float(r[k]) if r[k] not in ("", None) else None
            rows.append(r)
    return rows


def split_program(name):
    """('queue_window__window1000') -> ('queue_window', 1000); ('churn') -> ('churn', None)."""
    fam, _, rest = name.partition("__")
    m = re.search(r"(\d+)$", rest)
    return fam, int(m.group(1)) if m else None


def runtimes_in(rows):
    seen = []
    for r in rows:
        if r["runtime"] not in seen:
            seen.append(r["runtime"])
    seen.sort(key=lambda x: ORDER.index(x) if x in ORDER else len(ORDER))
    return seen


class Panel:
    """One log-y line chart with categorical x positions."""

    def __init__(self, x, y, w, h, title, xlabels, ylabel):
        self.x, self.y, self.w, self.h = x, y, w, h
        self.title, self.xlabels, self.ylabel = title, xlabels, ylabel
        self.items = []  # (runtime, xi, med, lo, hi, oom)

    def add(self, runtime, xi, med, lo=None, hi=None, oom=False):
        self.items.append((runtime, xi, med, lo, hi, oom))

    def render(self, colors, order):
        vals = [v for it in self.items for v in (it[2], it[3], it[4]) if v is not None and v > 0]
        lo_v = min(vals) if vals else 0.1
        hi_v = max(vals) if vals else 1.0
        ymin = 10 ** math.floor(math.log10(max(lo_v, 0.01)) - 0.0001)
        ymax = 10 ** math.ceil(math.log10(max(hi_v, ymin * 10)) + 0.0001)
        ymin = max(ymin, 0.01)
        lymin, lymax = math.log10(ymin), math.log10(ymax)
        left, right, top, bottom = 56, 12, 26, 38
        px, py = self.x + left, self.y + top
        pw, ph = self.w - left - right, self.h - top - bottom

        def X(i):
            n = max(len(self.xlabels), 1)
            return px + pw * (i + 0.5) / n

        def Y(v):
            v = max(v, ymin)
            return py + ph * (1 - (math.log10(v) - lymin) / (lymax - lymin))

        o = [f"<g>", f"<text x='{self.x + left}' y='{self.y + 16}' {FONT} font-size='12' font-weight='600' fill='{INK}'>{esc(self.title)}</text>"]
        d = lymin
        while d <= lymax + 1e-9:
            yy = py + ph * (1 - (d - lymin) / (lymax - lymin))
            o.append(f"<line x1='{px}' y1='{yy:.1f}' x2='{px + pw}' y2='{yy:.1f}' stroke='{GRID}'/>")
            o.append(f"<text x='{px - 6}' y='{yy + 4:.1f}' text-anchor='end' {FONT} font-size='10' fill='{MUTED}'>{fmt_tick(10 ** d)}</text>")
            d += 1
        o.append(f"<rect x='{px}' y='{py}' width='{pw}' height='{ph}' fill='none' stroke='{MUTED}' stroke-opacity='0.5'/>")
        for i, lab in enumerate(self.xlabels):
            o.append(f"<text x='{X(i):.1f}' y='{py + ph + 15}' text-anchor='middle' {FONT} font-size='10' fill='{MUTED}'>{esc(lab)}</text>")
        o.append(f"<text x='{self.x + 12}' y='{py + ph / 2:.1f}' transform='rotate(-90 {self.x + 12} {py + ph / 2:.1f})' text-anchor='middle' {FONT} font-size='10' fill='{MUTED}'>{esc(self.ylabel)}</text>")

        n_rt = max(len(order), 1)
        for ri, rt in enumerate(order):
            pts = sorted((it for it in self.items if it[0] == rt and not it[5] and it[2] is not None), key=lambda it: it[1])
            dx = (ri - (n_rt - 1) / 2) * 3.5  # small horizontal jitter so overlapping points stay visible
            c = colors[rt]
            if len(pts) > 1:
                path = " ".join(f"{X(it[1]) + dx:.1f},{Y(it[2]):.1f}" for it in pts)
                o.append(f"<polyline points='{path}' fill='none' stroke='{c}' stroke-width='1.6' stroke-opacity='0.85'/>")
            for it in pts:
                if it[3] is not None and it[4] is not None and it[4] > it[3]:
                    o.append(f"<line x1='{X(it[1]) + dx:.1f}' y1='{Y(it[3]):.1f}' x2='{X(it[1]) + dx:.1f}' y2='{Y(it[4]):.1f}' stroke='{c}' stroke-width='1' stroke-opacity='0.7'/>")
                o.append(f"<circle cx='{X(it[1]) + dx:.1f}' cy='{Y(it[2]):.1f}' r='3.2' fill='{c}'><title>{esc(rt)}: {it[2]:.3g} ms</title></circle>")
            for it in (i for i in self.items if i[0] == rt and i[5]):
                cx, cy = X(it[1]) + dx, py + 8 + ri * 0.0
                o.append(f"<g stroke='{c}' stroke-width='2'><line x1='{cx - 4:.1f}' y1='{cy - 4:.1f}' x2='{cx + 4:.1f}' y2='{cy + 4:.1f}'/><line x1='{cx - 4:.1f}' y1='{cy + 4:.1f}' x2='{cx + 4:.1f}' y2='{cy - 4:.1f}'/><title>{esc(rt)}: out of memory</title></g>")
        o.append("</g>")
        return "\n".join(o)


def legend(runtimes, colors, x, y):
    o = []
    cx = x
    for rt in runtimes:
        o.append(f"<circle cx='{cx + 5}' cy='{y}' r='4' fill='{colors[rt]}'/>")
        o.append(f"<text x='{cx + 14}' y='{y + 4}' {FONT} font-size='12' fill='{INK}'>{esc(rt)}</text>")
        cx += 22 + 7.2 * len(rt)
    o.append(f"<g stroke='{MUTED}' stroke-width='2'><line x1='{cx + 4}' y1='{y - 4}' x2='{cx + 12}' y2='{y + 4}'/><line x1='{cx + 4}' y1='{y + 4}' x2='{cx + 12}' y2='{y - 4}'/></g>")
    o.append(f"<text x='{cx + 18}' y='{y + 4}' {FONT} font-size='12' fill='{MUTED}'>out of memory</text>")
    return "\n".join(o)


def svg_doc(w, h, body):
    return (f"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 {w} {h}' width='{w}' height='{h}'>"
            f"<rect width='100%' height='100%' fill='#ffffff'/>{body}</svg>")


def family_chart(fam, rows, runtimes, colors):
    progs = []
    for r in rows:
        if split_program(r["program"])[0] == fam and r["program"] not in progs:
            progs.append(r["program"])
    progs.sort(key=lambda p: (split_program(p)[1] is None, split_program(p)[1] or 0))
    xlabels = [fmt_param(split_program(p)[1]) if split_program(p)[1] is not None else "–" for p in progs]
    heaps = []
    for r in rows:
        if r["heap"] not in heaps:
            heaps.append(r["heap"])
    heaps.sort(key=lambda h: (h != "default", h))
    heap_name = {"default": "16 MiB heap (default)"}
    pw, ph = 520, 300
    W, H = 20 + len(heaps) * pw, 70 + 2 * ph
    body = [f"<text x='20' y='26' {FONT} font-size='16' font-weight='700' fill='{INK}'>{esc(fam)}</text>",
            legend(runtimes, colors, 20, 48)]
    pname = {"live_list": "live nodes", "live_tree": "tree depth", "queue_window": "window size (nodes)",
             "old_to_young": "holders", "deep_roots": "recursion depth", "churn": ""}.get(fam, "")
    for hi, heap in enumerate(heaps):
        for ri, (key, ylabel, what) in enumerate((("gc", "ms", "GC time"), ("pause", "ms", "longest pause"))):
            panel = Panel(10 + hi * pw, 60 + ri * ph, pw, ph,
                          f"{what} - {heap_name.get(heap, heap + ' heap')}"
                          + (f" - x: {pname}" if pname and ri == 1 else ""), xlabels, ylabel)
            for xi, p in enumerate(progs):
                for r in rows:
                    if r["program"] != p or r["heap"] != heap:
                        continue
                    if r["oom"]:
                        panel.add(r["runtime"], xi, None, oom=True)
                    elif r["valid"] and r["gc_ms_median"] is not None:
                        if key == "gc":
                            panel.add(r["runtime"], xi, r["gc_ms_median"], r["gc_ms_min"], r["gc_ms_max"])
                        elif r["max_pause_ms_median"] is not None:
                            panel.add(r["runtime"], xi, r["max_pause_ms_median"])
            body.append(panel.render(colors, runtimes))
    return svg_doc(W, H, "\n".join(body))


def bars(x, y, w, h, title, data, colors, runtimes, unit):
    top, bottom, left = 34, 30, 20
    vmax = max(data.values()) or 1
    # round the axis maximum up to a "nice" number
    mag = 10 ** math.floor(math.log10(vmax))
    nice = next(m * mag for m in (1, 1.5, 2, 3, 4, 5, 7.5, 10) if m * mag >= vmax)
    n = len(runtimes)
    bw = (w - left - 10) / n * 0.6
    o = [f"<text x='{x + left}' y='{y + 18}' {FONT} font-size='13' font-weight='600' fill='{INK}'>{esc(title)}</text>"]
    ph = h - top - bottom
    for i, rt in enumerate(runtimes):
        v = data[rt]
        cx = x + left + (w - left - 10) * (i + 0.5) / n
        bh = ph * v / nice
        o.append(f"<rect x='{cx - bw / 2:.1f}' y='{y + top + ph - bh:.1f}' width='{bw:.1f}' height='{bh:.1f}' rx='2' fill='{colors[rt]}'><title>{esc(rt)}: {v:.0f} {unit}</title></rect>")
        o.append(f"<text x='{cx:.1f}' y='{y + top + ph - bh - 5:.1f}' text-anchor='middle' {FONT} font-size='12' font-weight='600' fill='{INK}'>{v:,.0f}</text>")
        o.append(f"<text x='{cx:.1f}' y='{y + top + ph + 16}' text-anchor='middle' {FONT} font-size='12' fill='{MUTED}'>{esc(rt)}</text>")
    o.append(f"<line x1='{x + left}' y1='{y + top + ph}' x2='{x + w - 10}' y2='{y + top + ph}' stroke='{MUTED}' stroke-opacity='0.5'/>")
    return "\n".join(o)


def summary_chart(rows, runtimes, colors):
    cells = {}
    for r in rows:
        cells.setdefault((r["program"], r["heap"]), {})[r["runtime"]] = r
    tot = {rt: 0.0 for rt in runtimes}
    pause = {rt: 0.0 for rt in runtimes}
    counted = 0
    for c in cells.values():
        if all(rt in c and c[rt]["valid"] and c[rt]["gc_ms_median"] is not None for rt in runtimes):
            counted += 1
            for rt in runtimes:
                tot[rt] += c[rt]["gc_ms_median"]
                pause[rt] += c[rt]["max_pause_ms_median"] or 0.0
    W, H = 900, 380
    body = [f"<text x='20' y='26' {FONT} font-size='16' font-weight='700' fill='{INK}'>Summary over {counted} of {len(cells)} cells</text>",
            f"<text x='20' y='46' {FONT} font-size='12' fill='{MUTED}'>Only cells where every collector finished are counted, so an out-of-memory in one collector excludes that cell for all.</text>",
            bars(10, 56, 440, 310, "Total GC time (ms)", tot, colors, runtimes, "ms"),
            bars(450, 56, 440, 310, "Sum of per-cell longest pause (ms)", pause, colors, runtimes, "ms")]
    return svg_doc(W, H, "\n".join(body))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("run_dir")
    ap.add_argument("--out")
    a = ap.parse_args()
    run = Path(a.run_dir)
    out = Path(a.out) if a.out else run / "plots"
    out.mkdir(parents=True, exist_ok=True)
    rows = load(run)
    runtimes = runtimes_in(rows)
    seen = []
    colors = {}
    for rt in runtimes:
        seen.append(rt)
        colors[rt] = color_for(rt, seen)
    fams = []
    for r in rows:
        f = split_program(r["program"])[0]
        if f not in fams:
            fams.append(f)
    written = []
    (out / "summary.svg").write_text(summary_chart(rows, runtimes, colors))
    written.append("summary.svg")
    for fam in fams:
        (out / f"{fam}.svg").write_text(family_chart(fam, rows, runtimes, colors))
        written.append(f"{fam}.svg")
    page = ["<!doctype html><meta charset='utf-8'><meta name='viewport' content='width=device-width,initial-scale=1'>",
            f"<title>GC benchmark plots - {esc(run.name)}</title>",
            "<style>body{font:15px/1.5 -apple-system,Segoe UI,Helvetica,Arial,sans-serif;margin:24px auto;max-width:1100px;padding:0 16px;color:#1a202c;background:#fff}"
            "img{max-width:100%;height:auto;border:1px solid #e3e8ee;border-radius:6px;margin:8px 0 28px}h1{font-size:22px}p{color:#5f6b7a}</style>",
            f"<h1>GC benchmark plots: {esc(run.name)}</h1>",
            "<p>Each family chart: columns are heap sizes; top row total GC time, bottom row longest single pause; "
            "log scales; dot = median, whisker = min to max over repetitions; x = out of memory.</p>"]
    for name in written:
        page.append(f"<h2>{esc(name[:-4])}</h2><img src='{esc(name)}' alt='{esc(name[:-4])} chart'>")
    (out / "index.html").write_text("\n".join(page))
    print(f"wrote {len(written)} charts and index.html to {out}")


if __name__ == "__main__":
    main()
