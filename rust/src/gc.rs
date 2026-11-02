//! GC operations (`runtime-abi.md` §3.4) — a mark-compact sliding collector.
//!
//! `lo_gc_write_barrier` is **provided** as a bare store — the correct
//! non-generational barrier (Cheney's semispace, mark-compact) out of the box,
//! and left untouched here (runbook WS-2 §2.6). `lo_gc_collect` marks from the
//! shadow stack, slides live objects to the start of the heap, and rewrites
//! roots and pointer fields. The allocator's OOM path (`crate::alloc`) drives it.
//!
//! ## Mark bit
//!
//! Bit 0 of `gc_bits` (`GC_MARK_BIT == 0b1`) is set on a live object before its
//! pointer fields are traced. The bit is collector-private — no two menu
//! collectors run on one heap — so it needs no ABI pin. The slide clears it on
//! the relocated object.

use std::collections::HashMap;

use crate::descriptors::LO_STRING_CLASS;
use crate::object::{shadow_frame_roots_offset, string_data_offset, Object, StringObject};

/// Mark bit in `gc_bits` for the mark-compact collector. Set before a marked
/// object's pointer fields are pushed, so a cycle is not traced again.
const GC_MARK_BIT: u32 = 0b1;

/// Pointer-store write barrier. Non-generational implementation: a bare store of
/// `value` into the pointer field at `obj + field_offset` (`runtime-abi.md` §3.4).
/// Codegen emits a call to this at *every* pointer store, which makes the choice
/// of collector a runtime-only decision. Correct as-is for Cheney; left untouched
/// (runbook WS-2 §2.6).
///
/// # Safety
/// `obj` must point at a valid object with a pointer-typed field at
/// `field_offset`; `value` must be a valid object pointer or null.
#[no_mangle]
pub unsafe extern "C" fn lo_gc_write_barrier(
    obj: *mut Object,
    field_offset: u32,
    value: *mut Object,
) {
    let slot = (obj as *mut u8).add(field_offset as usize) as *mut *mut Object;
    *slot = value;
}

/// Force a collection (`runtime-abi.md` §3.4): one mark-compact cycle.
/// Reachable objects slide to the start of the heap. Roots and pointer fields
/// are rewritten to the new addresses; unreachable objects are reclaimed.
#[no_mangle]
pub extern "C" fn lo_gc_collect() {
    // SAFETY: single-threaded runtime. A collect before `heap_init` walks an
    // empty root set over an empty occupied range and commits `FREE` at the
    // heap base — a no-op.
    unsafe { collect() }
}

#[inline]
const fn align8(n: usize) -> usize {
    (n + 7) & !7
}

/// True byte size of a heap object — the stride the scan advances by. Strings
/// are variable-size: their inline UTF-8 tail starts at `offset_of(StringObject,
/// data)` (= 20 on 64-bit), **not** `size_of` — the offset-of-not-size-of rule
/// (`runtime-abi.md` §2.2).
///
/// # Safety
/// `p` must point at a valid object whose `class_descriptor` is intact.
unsafe fn object_size(p: *mut Object) -> usize {
    let class = (*p).class_descriptor;
    if core::ptr::eq(class, &LO_STRING_CLASS) {
        let so = p as *const StringObject;
        align8(string_data_offset() + (*so).length as usize)
    } else {
        align8((*class).instance_size as usize)
    }
}

/// Mark `p` and every heap object reachable through its pointer fields.
///
/// The trace is an explicit stack on the system allocator, so a long chain does
/// not overflow the call stack. A pointer outside `[heap_base, free)` — null,
/// `LO_EMPTY_STRING`, or any other non-heap address — is ignored and not
/// entered. The mark bit is set before children are pushed, so a cycle is
/// skipped when it is seen again. The slot that held `p` is left unchanged;
/// repair rewrites pointers later.
///
/// # Safety
/// `heap_base` and `free` are the occupied range from [`crate::alloc::occupied`].
/// `p` is null, a pointer outside that range, or a valid object inside it whose
/// `class_descriptor` is intact. Single-threaded.
unsafe fn mark(p: *mut Object, heap_base: *mut u8, free: *mut u8) {
    let mut work = Vec::new();
    work.push(p);
    while let Some(obj) = work.pop() {
        let addr = obj as *mut u8;
        if addr < heap_base || addr >= free {
            continue;
        }
        if (*obj).gc_bits & GC_MARK_BIT != 0 {
            continue;
        }
        (*obj).gc_bits |= GC_MARK_BIT;

        let class = (*obj).class_descriptor;
        let count = (*class).pointer_count as usize;
        let offsets = (*class).pointer_offsets;
        for i in 0..count {
            let off = *offsets.add(i) as usize;
            let slot = (obj as *mut u8).add(off) as *mut *mut Object;
            work.push(*slot);
        }
    }
}

/// One mark-compact cycle. Marks from the shadow stack, slides live objects
/// toward `HEAP_BASE`, rewrites roots and pointer fields, then commits the
/// packed bump pointer.
///
/// # Safety
/// Single-threaded. The allocator statics describe a valid heap, or a
/// null/uninitialized heap, in which case the occupied range is empty and this
/// commits `FREE` at the base — a no-op.
unsafe fn collect() {
    // Mark. Reachable heap objects get GC_MARK_BIT. `mark` follows pointer
    // fields itself, so this loop only visits roots. Slots keep their pre-move
    // pointers; the repair pass rewrites them later.
    let (heap_base, heap_free) = crate::alloc::occupied();
    let mut frame = crate::shadow_stack::current_frame();
    while !frame.is_null() {
        let num = (*frame).num_roots as usize;
        let roots_base = (frame as *mut u8).add(shadow_frame_roots_offset());
        for i in 0..num {
            let slot = roots_base.add(i * core::mem::size_of::<*mut Object>()) as *mut *mut Object;
            mark(*slot, heap_base, heap_free);
        }
        frame = (*frame).parent;
    }

    // Scan cursors. `cur` walks every object in `[heap_base, heap_free)`.
    // `new_free` is the packed address of the next live object. `forwarding`
    // maps each live object's old address to that packed address.
    let mut forwarding: HashMap<*mut Object, *mut Object> = HashMap::new();
    let mut cur = heap_base;
    let mut new_free = heap_base;

    // Walk every object in `[heap_base, heap_free)`. `cur` steps by object size,
    // marked or not. A marked object is recorded and slid to `new_free` only
    // after `next` is saved, because the copy can overlap the source.
    while cur < heap_free {
        let obj = cur as *mut Object;
        let size = object_size(obj);
        let next = cur.add(size);
        if (*obj).gc_bits & GC_MARK_BIT != 0 {
            let dst = new_free as *mut Object;
            forwarding.insert(obj, dst);
            core::ptr::copy(cur, new_free, size);
            (*dst).gc_bits = 0;
            new_free = new_free.add(size);
        }
        cur = next;
    }

    // Repair roots. Each slot still holds a pre-move address. A hit in
    // `forwarding` is rewritten; null and non-heap pointers are left alone.
    frame = crate::shadow_stack::current_frame();
    while !frame.is_null() {
        let num = (*frame).num_roots as usize;
        let roots_base = (frame as *mut u8).add(shadow_frame_roots_offset());
        for i in 0..num {
            let slot = roots_base.add(i * core::mem::size_of::<*mut Object>()) as *mut *mut Object;
            if let Some(&updated) = forwarding.get(&*slot) {
                *slot = updated;
            }
        }
        frame = (*frame).parent;
    }

    // Repair pointer fields on the relocated objects. The old address may
    // already have been overwritten, so each object is read at its new address.
    // Copied fields still hold pre-move pointers; a map miss is left unchanged.
    let relocated: Vec<*mut Object> = forwarding.values().copied().collect();
    for obj in relocated {
        let class = (*obj).class_descriptor;
        let count = (*class).pointer_count as usize;
        let offsets = (*class).pointer_offsets;
        for i in 0..count {
            let off = *offsets.add(i) as usize;
            let slot = (obj as *mut u8).add(off) as *mut *mut Object;
            if let Some(&updated) = forwarding.get(&*slot) {
                *slot = updated;
            }
        }
    }

    // Commit the packed bump pointer. A debug build poisons the reclaimed tail.
    crate::alloc::set_free(new_free);
}

#[cfg(test)]
mod tests {
    //! Cheney collector tests (runbook WS-2 §3, step A.4). Run in-crate so they
    //! can reach the internal string allocator and heap-occupancy hook.
    //!
    //! The runtime is process-global single-threaded state, so these tests
    //! serialize through a mutex and bracket each body with init/shutdown. The
    //! `LO_HEAP_SIZE` override (read by `heap_init`) is set inside the lock so the
    //! allocation-pressure test cannot race another test's init.

    use std::sync::Mutex;

    use crate::alloc::{bump_alloc_string, heap_used, lo_alloc};
    use crate::descriptors::{LO_EMPTY_STRING, LO_STRING_CLASS};
    use crate::object::{string_data_offset, ClassDescriptor, Object, ShadowFrame, StringObject};
    use crate::shadow_stack::{lo_pop_frame, lo_push_frame};
    use crate::{lo_gc_collect, lo_runtime_init, lo_runtime_shutdown};

    static LOCK: Mutex<()> = Mutex::new(());

    // A test class with a single pointer field at offset 16 (right after the
    // 16-byte header): a linked-list "Node". instance_size 24, one pointer field.
    static NODE_PTR_OFFSETS: [u32; 1] = [16];
    static NODE_CLASS: ClassDescriptor = ClassDescriptor {
        name: c"Node".as_ptr() as *const u8,
        name_len: 4,
        parent: core::ptr::null(),
        instance_size: 24,
        pointer_offsets: &NODE_PTR_OFFSETS as *const u32,
        pointer_count: 1,
        vtable_size: 0,
        vtable: core::ptr::null(),
    };

    /// A shadow-stack frame with `N` inline roots, byte-compatible with the ABI's
    /// `ShadowFrame { parent, num_roots, roots: [*mut Object; N] }`.
    #[repr(C)]
    struct TestFrame<const N: usize> {
        parent: *mut ShadowFrame,
        num_roots: u32,
        roots: [*mut Object; N],
    }

    impl<const N: usize> TestFrame<N> {
        fn new() -> Self {
            TestFrame {
                parent: core::ptr::null_mut(),
                num_roots: N as u32,
                roots: [core::ptr::null_mut(); N],
            }
        }
        fn as_frame(&mut self) -> *mut ShadowFrame {
            self as *mut TestFrame<N> as *mut ShadowFrame
        }
    }

    /// Run `f` with the runtime up (optionally with a small heap to force GC),
    /// serialized and bracketed by init/shutdown.
    fn with_runtime<T>(heap_size: Option<&str>, f: impl FnOnce() -> T) -> T {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        match heap_size {
            Some(s) => std::env::set_var("LO_HEAP_SIZE", s),
            None => std::env::remove_var("LO_HEAP_SIZE"),
        }
        lo_runtime_init();
        let result = f();
        lo_runtime_shutdown();
        std::env::remove_var("LO_HEAP_SIZE");
        result
    }

    unsafe fn set_field(obj: *mut Object, off: usize, val: *mut Object) {
        *((obj as *mut u8).add(off) as *mut *mut Object) = val;
    }
    unsafe fn get_field(obj: *mut Object, off: usize) -> *mut Object {
        *((obj as *mut u8).add(off) as *mut *mut Object)
    }

    // (i) A known live graph survives. A dead node sits to the left so sliding
    // compaction actually moves the live nodes; the root and the interior
    // pointer are both rewritten, and the graph shape is preserved.
    #[test]
    fn live_graph_preserved_and_pointers_rewritten() {
        with_runtime(None, || unsafe {
            let mut frame = TestFrame::<1>::new();
            lo_push_frame(frame.as_frame());

            let _dead = lo_alloc(&NODE_CLASS); // unrooted, so a and b slide left
            let a = lo_alloc(&NODE_CLASS);
            let b = lo_alloc(&NODE_CLASS);
            set_field(a, 16, b); // a.next = b
            frame.roots[0] = a; // root = a

            lo_gc_collect();

            let a2 = frame.roots[0];
            assert_ne!(a2, a, "root must be rewritten to the moved object");
            assert_eq!((*a2).class_descriptor, &NODE_CLASS as *const _);

            let b2 = get_field(a2, 16);
            assert!(!b2.is_null(), "interior pointer must survive");
            assert_ne!(b2, b, "interior pointer must be rewritten to b's new addr");
            assert_eq!((*b2).class_descriptor, &NODE_CLASS as *const _);
            assert!(get_field(b2, 16).is_null(), "b.next was null");

            lo_pop_frame();
        });
    }

    // (ii) An unreachable object is reclaimed: heap occupancy drops by its size.
    #[test]
    fn unreachable_object_reclaimed() {
        with_runtime(None, || unsafe {
            let mut frame = TestFrame::<1>::new();
            lo_push_frame(frame.as_frame());

            let live = lo_alloc(&NODE_CLASS);
            frame.roots[0] = live;
            let _dead = lo_alloc(&NODE_CLASS); // not rooted -> unreachable

            let before = heap_used();
            assert_eq!(before, 48, "two 24-byte nodes occupy 48 bytes");

            lo_gc_collect();

            let after = heap_used();
            assert_eq!(after, 24, "only the live node survives");
            assert!(after < before);

            // The survivor's root was rewritten to its moved location and is
            // still a valid Node. (Also makes the root store observably-live to
            // the compiler, which cannot see the collector read it via the
            // registered shadow-stack pointer.)
            let live2 = frame.roots[0];
            assert_eq!((*live2).class_descriptor, &NODE_CLASS as *const _);

            lo_pop_frame();
        });
    }

    // (iii) A variable-size String survives a collection intact (bytes round-trip).
    #[test]
    fn string_survives_collection() {
        with_runtime(None, || unsafe {
            let mut frame = TestFrame::<1>::new();
            lo_push_frame(frame.as_frame());

            let bytes: &[u8] = b"hello, Cheney";
            let s = bump_alloc_string(bytes.len() as u32);
            let data = (s as *mut u8).add(string_data_offset());
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
            frame.roots[0] = s;

            lo_gc_collect();

            let s2 = frame.roots[0];
            assert_eq!((*s2).class_descriptor, &LO_STRING_CLASS as *const _);
            let so = s2 as *const StringObject;
            assert_eq!((*so).length, bytes.len() as u32);
            let data2 = (s2 as *const u8).add(string_data_offset());
            let round = core::slice::from_raw_parts(data2, bytes.len());
            assert_eq!(
                round, bytes,
                "string bytes must round-trip through the copy"
            );

            lo_pop_frame();
        });
    }

    // (iv) LO_EMPTY_STRING is immortal: a field that references it still points
    // at the same `.rodata` address after collection. The holder is the first
    // object, so sliding compaction leaves it in place.
    #[test]
    fn empty_string_not_moved() {
        with_runtime(None, || unsafe {
            let mut frame = TestFrame::<1>::new();
            lo_push_frame(frame.as_frame());

            let holder = lo_alloc(&NODE_CLASS);
            let empty = &LO_EMPTY_STRING as *const StringObject as *mut Object;
            set_field(holder, 16, empty); // holder.next = &LO_EMPTY_STRING
            frame.roots[0] = holder;

            lo_gc_collect();

            let holder2 = frame.roots[0];
            let field = get_field(holder2, 16);
            assert_eq!(
                field, empty,
                "immortal LO_EMPTY_STRING must not be relocated"
            );

            lo_pop_frame();
        });
    }

    // (v) Allocation pressure past one semispace triggers GC through lo_alloc's
    // OOM path and continues; only a genuine live-set overflow would abort.
    #[test]
    fn allocation_pressure_triggers_gc() {
        // Total 4 KiB -> 2 KiB per semispace (~85 nodes). 600 allocations of dead
        // nodes force many collections; the single rooted node survives them all.
        with_runtime(Some("4096"), || unsafe {
            let mut frame = TestFrame::<1>::new();
            lo_push_frame(frame.as_frame());

            let live = lo_alloc(&NODE_CLASS);
            frame.roots[0] = live;

            for _ in 0..600 {
                // Not rooted: each becomes garbage the next collection reclaims.
                let _dead = lo_alloc(&NODE_CLASS);
            }

            // Reaching here means GC repeatedly reclaimed and allocation continued
            // (a missing GC trigger would have aborted with exit 137, killing the
            // test process). The rooted node survived every collection.
            let live2 = frame.roots[0];
            assert_eq!((*live2).class_descriptor, &NODE_CLASS as *const _);

            lo_pop_frame();
        });
    }

    // A spine longer than the call stack must still be marked. Recursing on
    // `next` overflowed and segfaulted instead of returning to the OOM path.
    #[test]
    fn long_chain_survives_collection() {
        const N: usize = 100_000;
        with_runtime(None, || unsafe {
            let mut frame = TestFrame::<1>::new();
            lo_push_frame(frame.as_frame());

            let mut head: *mut Object = core::ptr::null_mut();
            for _ in 0..N {
                let node = lo_alloc(&NODE_CLASS);
                set_field(node, 16, head);
                head = node;
            }
            frame.roots[0] = head;

            lo_gc_collect();

            let mut cursor = frame.roots[0];
            let mut seen = 0;
            while !cursor.is_null() {
                assert_eq!((*cursor).class_descriptor, &NODE_CLASS as *const _);
                cursor = get_field(cursor, 16);
                seen += 1;
            }
            assert_eq!(seen, N);
            assert_eq!(heap_used(), N * 24);

            lo_pop_frame();
        });
    }
}
