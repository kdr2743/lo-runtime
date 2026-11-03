// good test case
// ABI harness: a 300-node doubly linked ring (every node on a cycle) is built, then every third node is replaced by a fresh young node spliced in through the write barrier, with 100 garbage allocations after each replacement (so minor collections run between the forced ones) and lo_gc_collect forced after every ten replacements; after each forced collection the ring must still close in both directions with every next.prev and prev.next intact, and the final id sum must match an independently computed value
// main method return value: 0
// gc stress heap: 262144
#include "lo_abi.h"

/*
 * What this checks: forced collections at chosen points over a cyclic graph whose
 * pointers cross generations. Every replacement stores a young node into two old
 * neighbours through lo_gc_write_barrier, so a forced collection that forgot a
 * remembered pointer, failed to fix a back pointer, or broke the cycle would be
 * seen by the ring check that follows it. The garbage allocations after each
 * replacement run minor collections between the stores and the forced collections,
 * so a dropped remembered-set record is caught too, not only a forced-major bug. The ring is only reachable through one
 * rooted head, so every node moves on every collection.
 *
 * Expected output: true (ring ok after the build), true (ring ok after each of the ten
 * forced collections in the replacement phase and 100 nodes replaced), then the id sum
 * (0+1+...+299 plus 1000 for each of the 100 replaced nodes = 144850), then true.
 */

#define N 300

/* Node { Object hdr; Object* next; Object* prev; int32 value; } */
typedef struct Node {
  Object hdr;
  Object *next;
  Object *prev;
  int32_t value;
} Node;
static const uint32_t node_ptrs[2] = {offsetof(Node, next), offsetof(Node, prev)};
LO_DESC(NODE, "Node", 0, sizeof(Node), node_ptrs, 2);

/* Walk the ring from `head`. True iff it closes after exactly N steps and every
 * node's next.prev and prev.next are itself; the ids are summed into *sum_out.
 * Allocates nothing, so no pointer is read across a safepoint. */
static _Bool ring_ok(Object *head, int32_t *sum_out) {
  Object *cur = head;
  int32_t sum = 0;
  for (int i = 0; i < N; i++) {
    Node *n = (Node *)cur;
    if (n->hdr.class_descriptor != &NODE) return 0;
    if (((Node *)n->next)->prev != cur) return 0;
    if (((Node *)n->prev)->next != cur) return 0;
    sum += n->value;
    cur = n->next;
  }
  *sum_out = sum;
  return cur == head;
}

int32_t lo_entry(void) {
  lo_runtime_init();
  /* root 0 = head, 1 = tail while building / the node being replaced, 2 = new node */
  LO_FRAME(f, 3);

  Node *first = (Node *)lo_alloc(&NODE);
  first->value = 0;
  LO_ROOT(f, 0) = &first->hdr;
  LO_ROOT(f, 1) = &first->hdr;
  for (int i = 1; i < N; i++) {
    Node *n = (Node *)lo_alloc(&NODE); /* safepoint: head and tail are rooted */
    n->value = i;
    LO_ROOT(f, 2) = &n->hdr;
    lo_gc_write_barrier(LO_ROOT(f, 1), offsetof(Node, next), &n->hdr);
    lo_gc_write_barrier(&n->hdr, offsetof(Node, prev), LO_ROOT(f, 1));
    LO_ROOT(f, 1) = &n->hdr; /* tail */
    LO_ROOT(f, 2) = 0;
  }
  lo_gc_write_barrier(LO_ROOT(f, 1), offsetof(Node, next), LO_ROOT(f, 0));
  lo_gc_write_barrier(LO_ROOT(f, 0), offsetof(Node, prev), LO_ROOT(f, 1));
  LO_ROOT(f, 1) = 0;

  int32_t sum = 0;
  lo_gc_collect();
  lo_put_bool_line(ring_ok(LO_ROOT(f, 0), &sum));

  _Bool all_ok = 1;
  int replaced = 0;
  for (int pos = 1; pos < N; pos += 3) {
    /* find the node at position `pos`, rooting it before the allocation */
    Object *c = LO_ROOT(f, 0);
    for (int k = 0; k < pos; k++) c = ((Node *)c)->next;
    LO_ROOT(f, 1) = c;
    Node *fresh = (Node *)lo_alloc(&NODE); /* safepoint: c is rooted */
    c = LO_ROOT(f, 1);
    Node *old = (Node *)c;
    fresh->value = old->value + 1000;
    LO_ROOT(f, 2) = &fresh->hdr;
    Object *p = old->prev, *q = old->next;
    lo_gc_write_barrier(&fresh->hdr, offsetof(Node, next), q);
    lo_gc_write_barrier(&fresh->hdr, offsetof(Node, prev), p);
    lo_gc_write_barrier(p, offsetof(Node, next), &fresh->hdr); /* old -> young */
    lo_gc_write_barrier(q, offsetof(Node, prev), &fresh->hdr); /* old -> young */
    LO_ROOT(f, 1) = 0;
    LO_ROOT(f, 2) = 0;
    replaced++;
    for (int g = 0; g < 100; g++) (void)lo_alloc(&NODE); /* garbage: forces minors */
    if (replaced % 10 == 0) {
      lo_gc_collect();
      int32_t s;
      all_ok = all_ok && ring_ok(LO_ROOT(f, 0), &s);
    }
  }
  lo_put_bool_line(all_ok && replaced == 100);
  lo_gc_collect();
  int32_t final_sum = 0;
  _Bool closed = ring_ok(LO_ROOT(f, 0), &final_sum);
  lo_put_int_line(final_sum);
  lo_put_bool_line(closed);
  lo_pop_frame();
  lo_runtime_shutdown();
  return 0;
}
