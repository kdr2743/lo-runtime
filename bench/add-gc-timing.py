#!/usr/bin/env python3
"""Give an OLDER or ALTERNATIVE collector the same `gc-time` timer this tree has.

    add-gc-timing.py RUST_DIR [GC_TIME_RS]

RUST_DIR is a checkout of some other version of rust/ (for example the Cheney
reference or an earlier generational collector). The script copies this tree's
src/gc_time.rs into it, registers the module and the `gc-time` cargo feature, and
wraps `collect` in src/gc.rs so that every collection is timed exactly as it is here.
Nothing else is touched. Afterwards build with `cargo build --release --lib
--features gc-time`.

It only knows one shape: a function `unsafe fn collect(...)` in src/gc.rs that every
collection goes through (true of the Cheney reference and of both generational
versions). It stops with an error otherwise, rather than guessing.
"""
import re
import shutil
import sys
from pathlib import Path

rust = Path(sys.argv[1])
gc_time = Path(sys.argv[2]) if len(sys.argv) > 2 else Path(__file__).resolve().parent.parent / "rust/src/gc_time.rs"
if not gc_time.exists():
    sys.exit(f"{gc_time} not found")

gc = rust / "src/gc.rs"
text = gc.read_text()
m = re.search(r"^(pub\(crate\) |pub )?unsafe fn collect\(([^)]*)\) \{", text, re.M)
if not m or len(re.findall(r"unsafe fn collect\(", text)) != 1:
    sys.exit("src/gc.rs does not have exactly one `unsafe fn collect(...)`; add the timer by hand")
vis, params = m.group(1) or "", m.group(2)
names = ", ".join(p.split(":")[0].strip() for p in params.split(",") if p.strip())
wrapper = f"""{vis}unsafe fn collect({params}) {{
    #[cfg(feature = "gc-time")]
    let timer = crate::gc_time::start();
    collect_inner({names});
    #[cfg(feature = "gc-time")]
    crate::gc_time::stop(timer);
}}

unsafe fn collect_inner({params}) {{"""
gc.write_text(text[:m.start()] + wrapper + text[m.end():])

shutil.copy(gc_time, rust / "src/gc_time.rs")

lib = rust / "src/lib.rs"
t = lib.read_text()
if "mod gc_time;" not in t:
    t = t.replace("mod gc;\n", 'mod gc;\n#[cfg(feature = "gc-time")]\nmod gc_time;\n', 1)
    if "mod gc_time;" not in t:
        sys.exit("could not find `mod gc;` in src/lib.rs")
    lib.write_text(t)

cargo = rust / "Cargo.toml"
t = cargo.read_text()
if "gc-time" not in t:
    if re.search(r"^\[features\]", t, re.M):
        t = re.sub(r"^\[features\]\n", "[features]\ngc-time = []\n", t, count=1, flags=re.M)
    else:
        t += "\n[features]\ngc-time = []\n"
    cargo.write_text(t)
print(f"timer added to {rust}")
