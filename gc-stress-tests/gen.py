#!/usr/bin/env python3
"""Generate the five contributed P3 GC stress tests and their expected output.

Expected output is computed here, in Python, from each program's parameters (32-bit
wraparound included); it does not come from running any collector.
Usage: gen.py OUTDIR
"""
import sys
from pathlib import Path

def w32(x):
    x &= 0xFFFFFFFF
    return x - (1 << 32) if x & 0x80000000 else x

NODE = """class Node ( int id; Node next; ) [
    Node(int i, Node n) { id = i; next = n; }
] {
    int get_id() { return id; }
    Node get_next() { return next; }
    void set_next(Node n) { next = n; }
}
"""
PAYLOAD = """class Payload ( int v; ) [
    Payload(int x) { v = x; }
] {
    int get() { return v; }
}
"""
HOLDER = PAYLOAD + """
class Holder ( Payload ref; Holder next; ) [
    Holder(Payload p, Holder n) { ref = p; next = n; }
] {
    void set(Payload p) { ref = p; }
    Payload get_ref() { return ref; }
    Holder get_next() { return next; }
}
"""
BOX = """class Box ( Payload ref; ) [
    Box() { ref = null; }
] {
    void set(Payload p) { ref = p; }
    Payload get() { return ref; }
}
"""

def window_sum(k, it):
    lo, hi = it - k, it - 1
    return w32((lo + hi) * k // 2)

def header(desc, heap):
    return f"// good test case\n// GC stress: {desc}\n// main method return value: 0\n// gc stress heap: {heap}\n\n"

tests = {}

# 1. promote-then-die ---------------------------------------------------------
K, IT = 60_000, 4_000_000
tests["contributed_gc_01_promote_then_die"] = (
    header(f"a window of {K} linked nodes slides over {IT} allocations: each node outlives the nursery, is promoted, and then dies in tenured space, so tenured space fills with garbage and many major collections run while the window stays intact; the final window checksum must match an independently computed value", 8388608)
    + NODE + """
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
        while ( (i < @IT@) ) {
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
""".replace("@K@", str(K)).replace("@IT@", str(IT)),
    f"{window_sum(K, IT)}\n")

# 2. major with a large live graph ---------------------------------------------
L, K2, IT2 = 60_000, 40_000, 4_000_000
tests["contributed_gc_02_major_with_large_live_graph"] = (
    header(f"a {L}-node list stays live while a {K2}-node sliding window promotes and drops {IT2} nodes, so every major collection must copy the whole {L}-node list plus the window and rewrite every pointer; the list must still be in order and complete afterwards", 16777216)
    + NODE + """
class Main ( ) {
    int main() {
        Node list;
        Node cur;
        Node head;
        Node tail;
        Node n;
        int i;
        int count;
        int sum;
        int wsum;
        int prev;
        int ordered;
        list = null;
        i = 1;
        while ( (i < @L1@) ) {
            list = new Node(i, list);
            i = (i + 1);
        }
        head = new Node(0, null);
        tail = head;
        i = 1;
        while ( (i < @K2@) ) {
            n = new Node(i, null);
            tail.set_next(n);
            tail = n;
            i = (i + 1);
        }
        while ( (i < @IT2@) ) {
            n = new Node(i, null);
            tail.set_next(n);
            tail = n;
            head = head.get_next();
            i = (i + 1);
        }
        count = 0;
        sum = 0;
        ordered = 1;
        prev = (@L1@ + 0);
        cur = list;
        while ( (! (cur = null)) ) {
            count = (count + 1);
            sum = (sum + cur.get_id());
            if ( (cur.get_id() < prev) ) {
                ;
            } else {
                ordered = 0;
            }
            prev = cur.get_id();
            cur = cur.get_next();
        }
        wsum = 0;
        cur = head;
        while ( (! (cur = null)) ) {
            wsum = (wsum + cur.get_id());
            cur = cur.get_next();
        }
        out.print_int(count);
        out.println();
        out.print_int(sum);
        out.println();
        out.print_int(ordered);
        out.println();
        out.print_int(wsum);
        out.println();
        return 0;
    }
}
""".replace("@L1@", str(L + 1)).replace("@K2@", str(K2)).replace("@IT2@", str(IT2)),
    f"{L}\n{w32(L * (L + 1) // 2)}\n1\n{window_sum(K2, IT2)}\n")

# 3. barrier storm ---------------------------------------------------------------
M, IT3 = 12_000, 3_000_000
total = 0
for j in range(M):
    total += j + M * ((IT3 - 1 - j) // M)
tests["contributed_gc_03_barrier_storm"] = (
    header(f"{M} long-lived holder objects are overwritten round-robin with fresh objects {IT3} times, so almost every store is old-to-young and each minor collection must find thousands of remembered objects; every holder must end up pointing at its last stored value", 4194304)
    + HOLDER + """
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
        while ( (it < @IT@) ) {
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
""".replace("@M@", str(M)).replace("@IT@", str(IT3)),
    f"{w32(total)}\n")

# 4. large strings force majors while a young object hangs off an old one --------
K4, IT4, EVERY = 20_000, 3_000_000, 3_000
tests["contributed_gc_04_large_alloc_forces_major"] = (
    header(f"every {EVERY} allocations an old holder is pointed at a fresh young object and a 300 KB string, larger than the nursery, is then allocated straight into tenured space; tenured space is full of dead promoted nodes, so the string allocation forces a major collection while the young object is reachable only through the old holder, which must still read back the stored value", 4194304)
    + NODE + BOX + PAYLOAD.replace("class Payload", "class Payload") + """
class Main ( ) {
    int main() {
        Node head;
        Node tail;
        Node n;
        Node cur;
        Box box;
        String t;
        int i;
        int bad;
        int sum;
        box = new Box();
        box.set(new Payload(0));
        head = new Node(0, null);
        tail = head;
        i = 1;
        while ( (i < @K@) ) {
            n = new Node(i, null);
            tail.set_next(n);
            tail = n;
            i = (i + 1);
        }
        bad = 0;
        while ( (i < @IT@) ) {
            n = new Node(i, null);
            tail.set_next(n);
            tail = n;
            head = head.get_next();
            if ( ((i % @EVERY@) = 0) ) {
                box.set(new Payload(i));
                t = ("0123456789abcdef" * 19200);
                if ( (! ((box.get()).get() = i)) ) {
                    bad = (bad + 1);
                } else {
                    ;
                }
            } else {
                ;
            }
            i = (i + 1);
        }
        sum = 0;
        cur = head;
        while ( (! (cur = null)) ) {
            sum = (sum + cur.get_id());
            cur = cur.get_next();
        }
        out.print_int(bad);
        out.println();
        out.print_int(sum);
        out.println();
        return 0;
    }
}
""".replace("@K@", str(K4)).replace("@IT@", str(IT4)).replace("@EVERY@", str(EVERY)),
    f"0\n{window_sum(K4, IT4)}\n")

# 5. deep recursion with a root in every frame while majors run -------------------
D5, K5, IT5 = 1_000, 20_000, 2_000_000
tests["contributed_gc_05_deep_roots_during_majors"] = (
    header(f"recursion {D5} frames deep keeps a fresh object and the caller's object live in every frame, and at the bottom a {K5}-node sliding window promotes and drops {IT5} nodes, so every major collection must forward {D5} frames of roots; on the way back up every frame must still see the objects it created", 4194304)
    + NODE + """
class Probe ( int id; ) [
    Probe(int i) { id = i; }
] {
    int get_id() { return id; }
}

class Rec ( ) [
    Rec() { ; }
] {
    int window() {
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
        while ( (i < @IT@) ) {
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
    int descend(int depth, Probe carried) {
        Probe mine;
        int below;
        mine = new Probe(depth);
        if ( (depth = @D@) ) {
            below = this.window();
        } else {
            below = this.descend((depth + 1), mine);
        }
        if ( (! (mine.get_id() = depth)) ) {
            return (0 - 1000000);
        } else {
            ;
        }
        if ( (! (carried.get_id() = (depth - 1))) ) {
            return (0 - 2000000);
        } else {
            ;
        }
        return (below + depth);
    }
}

class Main ( ) {
    int main() {
        Rec r;
        r = new Rec();
        out.print_int(r.descend(1, new Probe(0)));
        out.println();
        return 0;
    }
}
""".replace("@K@", str(K5)).replace("@IT@", str(IT5)).replace("@D@", str(D5)),
    f"{window_sum(K5, IT5)}\n{D5 * (D5 + 1) // 2}\n")


# 6. doubly linked ring whose nodes are replaced one by one -------------------------
N6, IT6 = 20_000, 1_500_000   # IT6 = 75 * N6, so every position is replaced exactly 75 times
assert IT6 == 75 * N6
ring_sum = w32(75 * N6 * N6 + N6 * (N6 - 1) // 2)
tests["contributed_gc_06_cycle_across_generations"] = (
    header(f"a doubly linked ring of {N6} nodes (every node is on a cycle) is spliced {IT6} times: each splice allocates a young node and stores it into its two old neighbours, so the cycle's pointers keep crossing generations while replaced nodes die in tenured space; the construction variables are cleared first so nothing keeps the dead history alive; afterwards the ring must close in both directions, every node's next.prev and prev.next must be itself, and the ids must sum to an independently computed value", 4194304)
    + """class RNode ( int id; RNode next; RNode prev; ) [
    RNode(int i, RNode n, RNode p) { id = i; next = n; prev = p; }
] {
    int get_id() { return id; }
    RNode get_next() { return next; }
    RNode get_prev() { return prev; }
    void set_next(RNode n) { next = n; }
    void set_prev(RNode p) { prev = p; }
}

class Main ( ) {
    int main() {
        RNode first;
        RNode last;
        RNode n;
        RNode c;
        RNode p;
        RNode q;
        RNode start;
        RNode cur;
        int i;
        int sum;
        int bad;
        int steps;
        int fwd;
        int bwd;
        first = new RNode(0, null, null);
        last = first;
        i = 1;
        while ( (i < @N@) ) {
            n = new RNode(i, null, last);
            last.set_next(n);
            last = n;
            i = (i + 1);
        }
        last.set_next(first);
        first.set_prev(last);
        c = first;
        first = null;
        last = null;
        i = 0;
        while ( (i < @IT@) ) {
            p = c.get_prev();
            q = c.get_next();
            n = new RNode((@N@ + i), q, p);
            p.set_next(n);
            q.set_prev(n);
            c = q;
            i = (i + 1);
        }
        start = c;
        cur = start;
        sum = 0;
        bad = 0;
        steps = 0;
        while ( (steps < @N@) ) {
            sum = (sum + cur.get_id());
            if ( (! ((cur.get_next()).get_prev() = cur)) ) {
                bad = (bad + 1);
            } else {
                ;
            }
            if ( (! ((cur.get_prev()).get_next() = cur)) ) {
                bad = (bad + 1);
            } else {
                ;
            }
            cur = cur.get_next();
            steps = (steps + 1);
        }
        fwd = 0;
        if ( (cur = start) ) {
            fwd = 1;
        } else {
            ;
        }
        steps = 0;
        while ( (steps < @N@) ) {
            cur = cur.get_prev();
            steps = (steps + 1);
        }
        bwd = 0;
        if ( (cur = start) ) {
            bwd = 1;
        } else {
            ;
        }
        out.print_int(sum);
        out.println();
        out.print_int(bad);
        out.println();
        out.print_int(fwd);
        out.println();
        out.print_int(bwd);
        out.println();
        return 0;
    }
}
""".replace("@N@", str(N6)).replace("@IT@", str(IT6)),
    f"{ring_sum}\n0\n1\n1\n")

out = Path(sys.argv[1]); out.mkdir(parents=True, exist_ok=True)
for name, (src, exp) in tests.items():
    (out / f"{name}.lo").write_text(src)
    (out / f"{name}.expected.out").write_text(exp)
    print(name, "heap", src.split("gc stress heap: ")[1].split()[0])
