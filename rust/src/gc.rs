//! Generational copying GC (`runtime-abi.md` §3.4) with immediate promotion.
//!
//! Minor collections copy nursery survivors into the active tenured tail,
//! tracing shadow-stack roots and a HashSet of remembered old objects. Major
//! collections trace the nursery and active tenured space into the spare tenured
//! space, then flip. Both use a Cheney scan and leave the nursery empty.
//!
//! `lo_gc_collect` forces a major collection. Allocation failure calls
//! `collect(false)`, which chooses a minor only when all used nursery bytes
//! could fit in the active tenured tail, conservatively assuming all survive.
//!
//! ## Forwarding flag
//!
//! Bit 0 of `gc_bits` (`GC_FORWARDED_BIT == 0b1`) is the **forwarding flag**: set
//! on a source object once evacuated. Its `class_descriptor` slot then holds
//! the destination address (a tombstone). Only the source is marked; the copy
//! retains its descriptor and a clear forwarding flag. This bit is private to
//! the collector.

use std::cell::RefCell;
use std::collections::HashSet;

use crate::descriptors::LO_STRING_CLASS;
use crate::object::{
    shadow_frame_roots_offset, string_data_offset, ClassDescriptor, Object, StringObject,
};

// Old objects that may contain nursery pointers. All runtime calls, including
// barrier writes and collections, must occur on the same single runtime thread.
thread_local! {
    static REMEMBERED_SET: RefCell<HashSet<*mut Object>> =
        RefCell::new(HashSet::new());
}

#[derive(Debug, Clone, Copy)]
struct Region {
    base: usize,
    limit: usize,
}

impl Region {
    fn contains(self, obj: *const Object) -> bool {
        let addr = obj as usize;
        addr >= self.base && addr < self.limit
    }
}

/// Forwarding-flag bit in `gc_bits`. See module docs.
pub(crate) const GC_FORWARDED_BIT: u32 = 0b1;

/// Record `obj` in the remembered set for an active-tenured-to-nursery write,
/// then store `value` at `obj + field_offset` (`runtime-abi.md` §3.4).
/// Codegen calls this for every heap pointer-field store. Minor collection scans
/// all pointer fields of each remembered object, not just the modified field.
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
    if crate::alloc::in_tenure(obj) && crate::alloc::in_nursery(value) {
        REMEMBERED_SET.with_borrow_mut(|set| {
            set.insert(obj);
        });
    }
    *slot = value;
}

/// Force a major collection (`runtime-abi.md` §3.4). Copy reachable nursery and
/// active tenured objects into the spare tenured space, rewriting roots and
/// interior pointers, then flip and reset the nursery.
#[no_mangle]
pub extern "C" fn lo_gc_collect() {
    // SAFETY: called on the single runtime thread with initialized heap state
    // and valid shadow-stack roots.
    unsafe { collect(true) }
}

#[inline]
const fn align8(n: usize) -> usize {
    (n + 7) & !7
}

/// True byte size of a heap object — the count `forward` copies and the stride
/// `scan` advances by. Strings are variable-size: their inline
/// UTF-8 tail starts at `offset_of(StringObject, data)` (= 20 on 64-bit), **not**
/// `size_of` — the offset-of-not-size-of rule (`runtime-abi.md` §2.2).
///
/// # Safety
/// `p` must point at a valid object whose `class_descriptor` is intact (a live
/// source object before its tombstone is installed, or a destination copy).
pub(crate) unsafe fn object_size(p: *mut Object) -> usize {
    let class = (*p).class_descriptor;
    if core::ptr::eq(class, &LO_STRING_CLASS) {
        let so = p as *const StringObject;
        align8(string_data_offset() + (*so).length as usize)
    } else {
        align8((*class).instance_size as usize)
    }
}

/// Evacuate `p` if it lies in a source region, bumping `free` after checking
/// destination capacity. Abort on overflow. Null and pointers outside the source
/// regions (including static objects and, during minors, old objects) are
/// unchanged. Already-forwarded objects return their tombstone's address.
///
/// Order matters: the size is read and the bytes copied **before** the original's
/// `class_descriptor` is overwritten, because both reads need the intact
/// descriptor. The copy carries the intact descriptor into the destination with the
/// forwarding flag still clear; only the original becomes a tombstone.
///
/// # Safety
/// `p` is null or a valid object pointer. Source regions contain valid objects;
/// `[free, destination_limit)` is writable destination storage disjoint from
/// them. Destination capacity is checked before copying. Single-threaded.
unsafe fn forward(
    p: *mut Object,
    regions: &[Region],
    free: &mut *mut u8,
    destination_limit: *mut u8,
) -> *mut Object {
    if p.is_null() || !regions.iter().any(|r| r.contains(p)) {
        return p;
    }
    let addr = p as *mut u8;

    if (*p).gc_bits & GC_FORWARDED_BIT != 0 {
        // Already moved; the class_descriptor slot holds the new address.
        return (*p).class_descriptor as *mut Object;
    }
    let n = object_size(p);
    let dst = *free;
    let remaining = (destination_limit as usize).checked_sub(dst as usize);
    if dst.is_null() || remaining.is_none_or(|remaining| n > remaining) {
        crate::abort::runtime_abort("lo_alloc: out of memory", 137);
    }
    core::ptr::copy_nonoverlapping(addr, dst, n);
    // Install the tombstone in the *original* — after the copy, so the to-space
    // copy keeps the intact descriptor and a clear forwarding flag.
    (*p).class_descriptor = dst as *const ClassDescriptor;
    (*p).gc_bits |= GC_FORWARDED_BIT;
    *free = dst.add(n);
    dst as *mut Object
}

/// Collect with immediate promotion; `force_major` bypasses minor selection.
///
/// # Safety
/// The heap and shadow-stack roots must be valid on the single runtime thread.
/// All old-to-young pointer stores must have passed through the write barrier.
pub(crate) unsafe fn collect(force_major: bool) {
    let (tenure_from_base, tenure_from_limit) = crate::alloc::tenure_from_space();
    let (nursery_base, nursery_limit) = crate::alloc::nursery_space();
    let nursery_free = crate::alloc::nursery_free();
    let tenure_free = crate::alloc::tenure_free();
    let (to_base, to_limit) = crate::alloc::to_space();

    // Conservatively reserve room for every used nursery byte, not just survivors.
    let nursery_size = nursery_free as usize - nursery_base as usize;
    let tenure_size_free = tenure_from_limit as usize - tenure_free as usize;
    let major_alloc = force_major || nursery_size > tenure_size_free;
    let destination_limit = if major_alloc {
        to_limit
    } else {
        tenure_from_limit
    };

    // Majors evacuate both generations into the spare; minors append to old.
    let (from_regions, mut free) = if major_alloc {
        (
            vec![
                Region {
                    base: tenure_from_base as usize,
                    limit: tenure_from_limit as usize,
                },
                Region {
                    base: nursery_base as usize,
                    limit: nursery_limit as usize,
                },
            ],
            to_base,
        )
    } else {
        (
            vec![Region {
                base: nursery_base as usize,
                limit: nursery_limit as usize,
            }],
            tenure_free,
        )
    };
    let mut scan = free;
    #[cfg(feature = "gc-debug")]
    let (copied_start, used_before) = (free, crate::alloc::heap_used());
    let mut frame = crate::shadow_stack::current_frame();
    while !frame.is_null() {
        let num = (*frame).num_roots as usize;
        let roots_base = (frame as *mut u8).add(shadow_frame_roots_offset());
        for i in 0..num {
            let slot = roots_base.add(i * core::mem::size_of::<*mut Object>()) as *mut *mut Object;
            if major_alloc || crate::alloc::in_nursery(*slot) {
                *slot = forward(*slot, &from_regions, &mut free, destination_limit);
            }
        }
        frame = (*frame).parent;
    }

    if !major_alloc {
        REMEMBERED_SET.with_borrow_mut(|set| {
            for obj in set.drain() {
                let class = (*obj).class_descriptor;
                for i in 0..(*class).pointer_count as usize {
                    let off = *(*class).pointer_offsets.add(i) as usize;
                    let slot = (obj as *mut u8).add(off) as *mut *mut Object;
                    *slot = forward(*slot, &from_regions, &mut free, destination_limit);
                }
            }
        })
    }

    // Cheney scan of the destination tail: forward each copied object's pointer
    // field, until `scan` catches `free`. Strings have `pointer_count == 0`, so
    // they only advance `scan` — but `object_size` still uses the variable-size
    // branch to advance correctly.
    while scan < free {
        let obj = scan as *mut Object;
        let class = (*obj).class_descriptor;
        let count = (*class).pointer_count as usize;
        let offsets = (*class).pointer_offsets;
        for i in 0..count {
            let off = *offsets.add(i) as usize;
            let slot = (obj as *mut u8).add(off) as *mut *mut Object;
            *slot = forward(*slot, &from_regions, &mut free, destination_limit);
        }
        scan = scan.add(object_size(obj));
    }

    if major_alloc {
        crate::alloc::flip(free);
    } else {
        crate::alloc::set_tenure_free(free);
    }
    crate::alloc::reset_nursery();
    REMEMBERED_SET.with_borrow_mut(|set| set.clear());

    // gc-debug: poison freed space, verify the heap, update stats and log.
    #[cfg(feature = "gc-debug")]
    crate::gc_debug::after_collection(crate::gc_debug::CollectSummary {
        major: major_alloc,
        explicit: force_major,
        used_before,
        copied_start,
        nursery: (nursery_base, nursery_limit),
        freed_tenure: major_alloc.then_some((tenure_from_base, tenure_from_limit)),
    });
}

/// Number of entries in the remembered set (gc-debug heap checker).
#[cfg(feature = "gc-debug")]
pub(crate) fn remembered_len() -> usize {
    REMEMBERED_SET.with_borrow(|set| set.len())
}

#[cfg(test)]
mod tests {
    //! Generational copying collector tests. Run in-crate so they
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

    #[test]
    fn minor_repairs_remembered_old_to_young_field() {
        with_runtime(Some("4096"), || unsafe {
            let mut frame = TestFrame::<1>::new();
            lo_push_frame(frame.as_frame());
            frame.roots[0] = lo_alloc(&NODE_CLASS);
            super::collect(false);
            let old = frame.roots[0];
            assert!(crate::alloc::in_tenure(old));

            let young = lo_alloc(&NODE_CLASS);
            super::lo_gc_write_barrier(old, 16, young);
            super::collect(false);
            assert_eq!(frame.roots[0], old);
            let promoted = get_field(old, 16);
            assert_ne!(promoted, young);
            assert!(crate::alloc::in_tenure(promoted));
            assert_eq!((*promoted).class_descriptor, &NODE_CLASS as *const _);
            assert_eq!(heap_used(), 48);
            assert_eq!(
                crate::alloc::nursery_free(),
                crate::alloc::nursery_space().0
            );
            super::REMEMBERED_SET.with_borrow(|set| assert!(set.is_empty()));
            lo_pop_frame();
        });
    }

    #[test]
    fn shared_cycle_survives_minor_and_major() {
        with_runtime(None, || unsafe {
            let mut frame = TestFrame::<2>::new();
            lo_push_frame(frame.as_frame());
            let a = lo_alloc(&NODE_CLASS);
            let b = lo_alloc(&NODE_CLASS);
            set_field(a, 16, b);
            set_field(b, 16, a);
            frame.roots = [a, a];
            for force_major in [false, true] {
                let previous = frame.roots[0];
                super::collect(force_major);
                let moved = frame.roots[0];
                assert_ne!(moved, previous);
                assert_eq!(frame.roots[1], moved);
                assert_eq!(get_field(get_field(moved, 16), 16), moved);
                assert_eq!(heap_used(), 48);
            }
            lo_pop_frame();
        });
    }

    #[test]
    fn explicit_major_reclaims_dead_tenured_objects() {
        with_runtime(Some("4096"), || unsafe {
            assert_eq!(crate::alloc::heap_size(), 4096);
            let mut frame = TestFrame::<2>::new();
            lo_push_frame(frame.as_frame());
            frame.roots[0] = lo_alloc(&NODE_CLASS);
            frame.roots[1] = lo_alloc(&NODE_CLASS);
            assert_eq!(heap_used(), 48);
            super::collect(false);
            frame.roots[1] = core::ptr::null_mut();
            let previous = frame.roots[0];
            super::collect(false);
            assert_eq!(heap_used(), 48);
            lo_gc_collect();
            assert_eq!(heap_used(), 24);
            assert_ne!(frame.roots[0], previous);
            assert_eq!((*frame.roots[0]).class_descriptor, &NODE_CLASS as *const _);
            lo_pop_frame();
        });
    }

    // (i) A known live graph survives: reachable objects are preserved with
    // updated addresses, and both the root slot and the interior pointer field
    // are rewritten to the new locations.
    #[test]
    fn live_graph_preserved_and_pointers_rewritten() {
        with_runtime(None, || unsafe {
            let mut frame = TestFrame::<1>::new();
            lo_push_frame(frame.as_frame());

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
            assert_ne!(s2, s, "string must move to to-space");
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

    // (iv) LO_EMPTY_STRING is immortal: a moved object's field that references it
    // still points at the same `.rodata` address after collection (forward's
    // range-check leaves non-from-space pointers untouched — no special case).
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
            assert_ne!(holder2, holder, "holder itself moves");
            let field = get_field(holder2, 16);
            assert_eq!(
                field, empty,
                "immortal LO_EMPTY_STRING must not be relocated"
            );

            lo_pop_frame();
        });
    }

    // (v) Filling the nursery triggers collection through lo_alloc's no-fit
    // path, allowing allocation to continue while the rooted node survives.
    #[test]
    fn allocation_pressure_triggers_gc() {
        // A 4 KiB heap has a 1 KiB nursery and two 1.5 KiB tenured spaces.
        // 600 dead-node allocations force repeated nursery collections.
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

    // --- large objects (bigger than the nursery) ------------------------------
    //
    // A 128 KiB heap has a nursery smaller than 40000 bytes (and tenured spaces
    // larger than it), whichever nursery fraction is configured.

    const LARGE_HEAP: &str = "131072";
    /// Total object size of the large test class and string: 40000 bytes.
    const LARGE_SIZE: usize = 40000;
    static LARGE_PTR_OFFSETS: [u32; 1] = [16];
    static LARGE_CLASS: ClassDescriptor = ClassDescriptor {
        name: c"Large".as_ptr() as *const u8,
        name_len: 5,
        parent: core::ptr::null(),
        instance_size: LARGE_SIZE as u32,
        pointer_offsets: &LARGE_PTR_OFFSETS as *const u32,
        pointer_count: 1,
        vtable_size: 0,
        vtable: core::ptr::null(),
    };

    /// A string whose total object size is exactly `LARGE_SIZE`, filled with a
    /// position-dependent pattern.
    unsafe fn large_string() -> *mut Object {
        let len = LARGE_SIZE - string_data_offset();
        let s = bump_alloc_string(len as u32);
        let data = core::slice::from_raw_parts_mut((s as *mut u8).add(string_data_offset()), len);
        for (i, b) in data.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        s
    }

    unsafe fn assert_large_string_intact(s: *mut Object) {
        let len = LARGE_SIZE - string_data_offset();
        assert_eq!((*s).class_descriptor, &LO_STRING_CLASS as *const _);
        assert_eq!((*(s as *const StringObject)).length as usize, len);
        let data = core::slice::from_raw_parts((s as *const u8).add(string_data_offset()), len);
        assert!(data.iter().enumerate().all(|(i, &b)| b == (i % 251) as u8));
    }

    #[test]
    fn object_larger_than_nursery_is_allocated_in_tenure() {
        with_runtime(Some(LARGE_HEAP), || unsafe {
            let (nursery_base, nursery_limit) = crate::alloc::nursery_space();
            assert!(LARGE_SIZE > nursery_limit as usize - nursery_base as usize);

            let s = large_string();
            assert!(
                crate::alloc::in_tenure(s),
                "large object goes straight to tenure"
            );
            assert_eq!(
                crate::alloc::nursery_free(),
                nursery_base,
                "nursery untouched"
            );
            assert_eq!(heap_used(), LARGE_SIZE);
            assert_large_string_intact(s);
        });
    }

    #[test]
    fn large_object_survives_minor_and_major_collections() {
        with_runtime(Some(LARGE_HEAP), || unsafe {
            let mut frame = TestFrame::<1>::new();
            lo_push_frame(frame.as_frame());
            frame.roots[0] = large_string();
            let allocated = frame.roots[0];

            super::collect(false);
            assert_eq!(
                frame.roots[0], allocated,
                "minor never moves tenured objects"
            );
            assert_large_string_intact(frame.roots[0]);

            lo_gc_collect();
            assert_ne!(frame.roots[0], allocated, "major evacuates it");
            assert_large_string_intact(frame.roots[0]);
            assert_eq!(heap_used(), LARGE_SIZE);
            lo_pop_frame();
        });
    }

    #[test]
    fn large_allocation_runs_a_major_when_tenure_is_full_of_garbage() {
        with_runtime(Some(LARGE_HEAP), || unsafe {
            let mut frame = TestFrame::<1>::new();
            lo_push_frame(frame.as_frame());
            // Dead once allocated: two of these cannot share one tenured space.
            let _garbage = large_string();
            let (tenure_base, tenure_limit) = crate::alloc::tenure_from_space();
            assert!(2 * LARGE_SIZE > tenure_limit as usize - tenure_base as usize);

            frame.roots[0] = large_string();
            assert_eq!(heap_used(), LARGE_SIZE, "the major reclaimed the garbage");
            assert_large_string_intact(frame.roots[0]);
            lo_pop_frame();
        });
    }

    #[test]
    fn large_object_keeps_young_referent_alive_via_the_barrier() {
        with_runtime(Some(LARGE_HEAP), || unsafe {
            let mut frame = TestFrame::<1>::new();
            lo_push_frame(frame.as_frame());
            let large = lo_alloc(&LARGE_CLASS);
            frame.roots[0] = large;
            assert!(crate::alloc::in_tenure(large));

            // The only path to `young` runs through the large (old) object.
            let young = lo_alloc(&NODE_CLASS);
            assert!(crate::alloc::in_nursery(young));
            super::lo_gc_write_barrier(large, 16, young);

            super::collect(false);
            let promoted = get_field(frame.roots[0], 16);
            assert_ne!(promoted, young);
            assert!(crate::alloc::in_tenure(promoted));
            assert_eq!((*promoted).class_descriptor, &NODE_CLASS as *const _);
            lo_pop_frame();
        });
    }

    // --- gc-debug: poisoning, heap checker, stats ---------------------------

    #[cfg(feature = "gc-debug")]
    mod debug {
        use super::*;
        use crate::gc_debug::{gc_debug_stats, verify_heap, POISON_BYTE};

        /// Root one old (tenured) node, run `f` with its frame, pop the frame.
        unsafe fn with_tenured_root(f: impl FnOnce(&mut TestFrame<1>, *mut Object)) {
            let mut frame = TestFrame::<1>::new();
            lo_push_frame(frame.as_frame());
            frame.roots[0] = lo_alloc(&NODE_CLASS);
            super::super::collect(false);
            let old = frame.roots[0];
            f(&mut frame, old);
            lo_pop_frame();
        }

        #[test]
        fn healthy_heap_verifies_and_free_space_is_poisoned() {
            with_runtime(Some("4096"), || unsafe {
                with_tenured_root(|_, old| {
                    assert!(crate::alloc::in_tenure(old));
                    assert_eq!(verify_heap(), Ok(()));
                    let (base, limit) = crate::alloc::nursery_space();
                    let nursery = core::slice::from_raw_parts(base, limit as usize - base as usize);
                    assert!(nursery.iter().all(|&b| b == POISON_BYTE));
                });
            });
        }

        #[test]
        fn stale_pointer_into_evacuated_nursery_reads_poison() {
            with_runtime(Some("4096"), || unsafe {
                let mut frame = TestFrame::<1>::new();
                lo_push_frame(frame.as_frame());
                let before = lo_alloc(&NODE_CLASS);
                frame.roots[0] = before;
                super::super::collect(false);
                assert_ne!(frame.roots[0], before);
                // `before` is the stale pre-move address: it must not look live.
                let stale = (*before).class_descriptor as usize;
                assert_eq!(
                    stale,
                    usize::from_ne_bytes([POISON_BYTE; size_of::<usize>()])
                );
                lo_pop_frame();
            });
        }

        #[test]
        fn checker_rejects_field_pointing_into_the_heap_but_not_at_an_object() {
            with_runtime(Some("4096"), || unsafe {
                with_tenured_root(|_, old| {
                    let bogus = crate::alloc::nursery_space().0 as *mut Object;
                    set_field(old, 16, bogus);
                    let err = verify_heap().unwrap_err();
                    assert!(err.contains("not at a live object"), "{err}");
                    set_field(old, 16, core::ptr::null_mut());
                    assert_eq!(verify_heap(), Ok(()));
                });
            });
        }

        #[test]
        fn checker_rejects_bad_root() {
            with_runtime(Some("4096"), || unsafe {
                with_tenured_root(|frame, old| {
                    // One byte into the object: inside the heap, not an object start.
                    frame.roots[0] = (old as *mut u8).add(8) as *mut Object;
                    let err = verify_heap().unwrap_err();
                    assert!(err.contains("shadow-stack root"), "{err}");
                    frame.roots[0] = old;
                    assert_eq!(verify_heap(), Ok(()));
                });
            });
        }

        #[test]
        fn checker_rejects_unpoisoned_free_space() {
            with_runtime(Some("4096"), || unsafe {
                with_tenured_root(|_, _| {
                    let tail = crate::alloc::tenure_free();
                    *tail = 0;
                    let err = verify_heap().unwrap_err();
                    assert!(err.contains("tenured tail"), "{err}");
                    *tail = POISON_BYTE;
                    assert_eq!(verify_heap(), Ok(()));
                });
            });
        }

        #[test]
        fn checker_rejects_bad_descriptor_pointer() {
            with_runtime(Some("4096"), || unsafe {
                with_tenured_root(|_, old| {
                    (*old).class_descriptor = core::ptr::null();
                    let err = verify_heap().unwrap_err();
                    assert!(err.contains("bad class descriptor"), "{err}");
                    (*old).class_descriptor = &NODE_CLASS;
                    assert_eq!(verify_heap(), Ok(()));
                });
            });
        }

        #[test]
        fn checker_rejects_leftover_remembered_set_entry() {
            with_runtime(Some("4096"), || unsafe {
                with_tenured_root(|_, old| {
                    super::super::REMEMBERED_SET.with_borrow_mut(|set| {
                        set.insert(old);
                    });
                    let err = verify_heap().unwrap_err();
                    assert!(err.contains("remembered set"), "{err}");
                    super::super::REMEMBERED_SET.with_borrow_mut(|set| set.clear());
                    assert_eq!(verify_heap(), Ok(()));
                });
            });
        }

        #[test]
        fn stats_count_collections_and_survivors() {
            with_runtime(Some("4096"), || unsafe {
                let mut frame = TestFrame::<2>::new();
                lo_push_frame(frame.as_frame());
                frame.roots[0] = lo_alloc(&NODE_CLASS);
                frame.roots[1] = lo_alloc(&NODE_CLASS);
                let _dead = lo_alloc(&NODE_CLASS);
                assert_eq!(gc_debug_stats().collections, 0);

                super::super::collect(false);
                let minor = gc_debug_stats();
                assert_eq!((minor.collections, minor.minors, minor.majors), (1, 1, 0));
                assert_eq!(minor.copied_objects, 2);
                assert_eq!(minor.reclaimed_bytes, 24);

                lo_gc_collect();
                let major = gc_debug_stats();
                assert_eq!((major.collections, major.minors, major.majors), (2, 1, 1));
                assert_eq!(major.copied_objects, 4);
                // Both roots survived (and keeps the root stores observably live).
                assert!(frame.roots.iter().all(|r| !r.is_null()));
                lo_pop_frame();
            });
        }

        #[test]
        fn cycles_strings_and_empty_string_pass_the_checker_under_pressure() {
            with_runtime(Some("8192"), || unsafe {
                let mut frame = TestFrame::<2>::new();
                lo_push_frame(frame.as_frame());
                let a = lo_alloc(&NODE_CLASS);
                let b = lo_alloc(&NODE_CLASS);
                set_field(a, 16, b);
                set_field(b, 16, a);
                frame.roots[0] = a;
                let empty = &LO_EMPTY_STRING as *const StringObject as *mut Object;
                let holder = lo_alloc(&NODE_CLASS);
                set_field(holder, 16, empty);
                frame.roots[1] = holder;
                // The checker runs inside every one of these collections.
                for i in 0..400 {
                    let _ = bump_alloc_string(i % 40);
                    let _ = lo_alloc(&NODE_CLASS);
                }
                assert!(gc_debug_stats().collections > 2);
                assert_eq!(get_field(frame.roots[1], 16), empty);
                lo_pop_frame();
            });
        }
    }
}
