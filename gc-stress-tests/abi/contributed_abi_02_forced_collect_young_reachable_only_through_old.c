// good test case
// ABI harness: a holder promoted to the old generation by a forced collection is the only path to each freshly allocated young payload stored into it through the write barrier; allocation pressure runs minor collections and every fifth round a forced collection runs a major, and after each the payload must still be readable through the holder with its value intact
// main method return value: 0
// gc stress heap: 65536
#include "lo_abi.h"

/*
 * What this checks: the missed-barrier-record failure (handout section 3), made
 * deliberate. After the first lo_gc_collect() the holder is old. Each round then
 * allocates a young payload, stores it into the holder through
 * lo_gc_write_barrier, and drops every other reference, so the only path to the
 * payload runs through an old object. 2000 further allocations force minor
 * collections at a 64 KiB heap: a collector that did not record the old-to-young
 * store reclaims the payload while it is live and the read-back fails. Every
 * fifth round also forces a collection and checks again, so both the minor and
 * the forced paths must keep the payload.
 *
 * Expected output: 0 mismatches, the last payload value (199), then true.
 */

/* Holder { Object hdr; Object* ref; } and Payload { Object hdr; int32 v; } */
typedef struct Holder {
  Object hdr;
  Object *ref;
} Holder;
typedef struct Payload {
  Object hdr;
  int32_t v;
} Payload;
static const uint32_t holder_ptrs[1] = {offsetof(Holder, ref)};
LO_DESC(HOLDER, "Holder", 0, sizeof(Holder), holder_ptrs, 1);
LO_DESC(PAYLOAD, "Payload", 0, sizeof(Payload), 0, 0);

static _Bool payload_ok(Object *holder, int32_t want) {
  Object *r = ((Holder *)holder)->ref;
  return r != 0 && r->class_descriptor == &PAYLOAD && ((Payload *)r)->v == want;
}

int32_t lo_entry(void) {
  lo_runtime_init();
  LO_FRAME(f, 1);
  LO_ROOT(f, 0) = lo_alloc(&HOLDER);
  lo_gc_collect(); /* the holder is now in the old generation */

  int32_t bad = 0;
  for (int32_t round = 0; round < 200; round++) {
    Payload *p = (Payload *)lo_alloc(&PAYLOAD); /* young */
    p->v = round;
    lo_gc_write_barrier(LO_ROOT(f, 0), offsetof(Holder, ref), &p->hdr);
    p = 0; /* the holder is now the only path to the payload */
    for (int g = 0; g < 2000; g++) (void)lo_alloc(&PAYLOAD);
    if (!payload_ok(LO_ROOT(f, 0), round)) bad++;
    if (round % 5 == 4) {
      lo_gc_collect();
      if (!payload_ok(LO_ROOT(f, 0), round)) bad++;
    }
  }
  lo_put_int_line(bad);
  lo_put_int_line(((Payload *)((Holder *)LO_ROOT(f, 0))->ref)->v);
  lo_put_bool_line(bad == 0);
  lo_pop_frame();
  lo_runtime_shutdown();
  return 0;
}
