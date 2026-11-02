//! Bump allocator backing `lo_alloc` (`runtime-abi.md` §3.1). The collector
//! compacts in place; this module owns the heap geometry and the bump pointer.
//!
//! The managed heap is one contiguous region `[HEAP_BASE, HEAP_LIMIT)`.
//! Allocation is a bump pointer (`FREE`) within it. When a request does not fit,
//! `lo_alloc` forces a collection (`lo_gc_collect`, see [`crate::gc`]) and
//! retries once; only a genuine live-set overflow aborts via the existing OOM
//! contract (exit 137 native / `unreachable` WASM).
//!
//! Total heap size inherits the WS-1 mechanism (decision (5c)): a 16 MiB default,
//! overridable at `lo_runtime_init` via the `LO_HEAP_SIZE` environment variable.
//! Setting `LO_HEAP_SIZE` small lets the allocation-pressure tests force a
//! collection deterministically.
//!
//! The heap pointers are `static mut` raw scalars (the runbook sanctions
//! `static mut` for the bump pointer). Every access is by value or through a raw
//! pointer — never `&`/`&mut` of the statics — which keeps this module clear of
//! the `static_mut_refs` lint. Invariant: only the functions here and the
//! collector's commit ([`set_free`]) touch these, and only on the single runtime
//! thread.

use core::alloc::Layout;
use core::ptr;

#[cfg(debug_assertions)]
use std::collections::HashSet;

use crate::descriptors::LO_STRING_CLASS;
#[cfg(debug_assertions)]
use crate::descriptors::LO_EMPTY_STRING;
use crate::object::{string_data_offset, ClassDescriptor, Object, StringObject};

/// Default total heap size (16 MiB), overridable via `LO_HEAP_SIZE` at init.
const DEFAULT_HEAP_SIZE: usize = 16 * 1024 * 1024;
/// All allocations (and the heap base) are aligned to this; covers `Object`'s
/// 8-byte pointer field with headroom.
const HEAP_ALIGN: usize = 16;

/// Base of the whole backing region (for `dealloc`); null when uninitialized.
static mut HEAP_BASE: *mut u8 = ptr::null_mut();
/// Layout the region was allocated with (for `dealloc`).
static mut HEAP_LAYOUT: Option<Layout> = None;
/// End of the contiguous heap `[HEAP_BASE, HEAP_LIMIT)`. One past the last byte.
static mut HEAP_LIMIT: *mut u8 = ptr::null_mut();
/// Bump pointer. Objects occupy `[HEAP_BASE, FREE)`.
static mut FREE: *mut u8 = ptr::null_mut();

#[inline]
const fn align_up(n: usize, align: usize) -> usize {
    (n + align - 1) & !(align - 1)
}

#[inline]
const fn align_down(n: usize, align: usize) -> usize {
    n & !(align - 1)
}

/// Allocate one contiguous heap region. Called from `lo_runtime_init`.
pub(crate) fn heap_init() {
    let total = std::env::var("LO_HEAP_SIZE")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n >= HEAP_ALIGN)
        .unwrap_or(DEFAULT_HEAP_SIZE);
    // Round down so the heap is never larger than requested and the limit stays
    // aligned. The filter above keeps `heap_space` non-zero.
    let heap_space = align_down(total, HEAP_ALIGN);
    let layout = Layout::from_size_align(heap_space, HEAP_ALIGN).expect("valid heap layout");
    // SAFETY: `layout` has non-zero size; `alloc_zeroed` returns either a valid
    // zeroed region of `heap_space` bytes or null (handled). Writes below are
    // by-value stores to the allocator statics on the single runtime thread.
    unsafe {
        let base = std::alloc::alloc_zeroed(layout);
        if base.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        HEAP_BASE = base;
        HEAP_LAYOUT = Some(layout);
        FREE = base;
        HEAP_LIMIT = base.add(heap_space);
    }
}

/// Release the heap region. Called from `lo_runtime_shutdown`.
pub(crate) fn heap_shutdown() {
    // SAFETY: by-value reads of the statics; `dealloc` is paired with the
    // `alloc_zeroed` in `heap_init` using the stored layout. Idempotent: a null
    // base (never inited, or already shut down) is a no-op.
    unsafe {
        let base = HEAP_BASE;
        let layout = HEAP_LAYOUT;
        if !base.is_null() {
            if let Some(layout) = layout {
                std::alloc::dealloc(base, layout);
            }
            HEAP_BASE = ptr::null_mut();
            HEAP_LAYOUT = None;
            FREE = ptr::null_mut();
            HEAP_LIMIT = ptr::null_mut();
        }
    }
}

/// Carve `size` zero-initialized bytes from the heap and stamp the object
/// header. On a no-fit, forces one collection and retries; aborts (exit
/// 137) only if the live set still leaves no room (runbook WS-2 §2.1, ABI §3.1).
///
/// # Safety
/// `class` must point at a valid, live `ClassDescriptor`. `size` must be at least
/// `size_of::<Object>()` so the header fits, and a multiple of 8.
unsafe fn alloc_raw(size: usize, class: *const ClassDescriptor) -> *mut Object {
    let bump = bump_if_fits(size);
    let slot = if !bump.is_null() {
        bump
    } else {
        // OOM path: collect, then retry the bump exactly once.
        crate::gc::lo_gc_collect();
        let retry = bump_if_fits(size);
        if retry.is_null() {
            crate::abort::runtime_abort("lo_alloc: out of memory", 137);
        }
        retry
    };

    ptr::write_bytes(slot, 0, size);
    let obj = slot as *mut Object;
    (*obj).class_descriptor = class;
    (*obj).gc_bits = 0;
    (*obj).flags = 0;
    obj
}

/// Try to bump `size` bytes off the heap. Returns the carved slot on success, or
/// null if it does not fit (or the heap is uninitialized).
///
/// # Safety
/// Reads/writes the allocator statics on the single runtime thread.
unsafe fn bump_if_fits(size: usize) -> *mut u8 {
    let bump = FREE;
    let limit = HEAP_LIMIT;
    // Address arithmetic for the bounds check avoids forming an out-of-bounds
    // pointer when the request would overflow the heap.
    let new_addr = (bump as usize).wrapping_add(size);
    if bump.is_null() || new_addr > limit as usize {
        return ptr::null_mut();
    }
    FREE = bump.add(size);
    bump
}

/// Allocate a zero-initialized object of `class.instance_size` bytes
/// (`runtime-abi.md` §3.1). Fills in the header. Triggers a collection and
/// retries on a full heap; aborts (exit 137) only on live-set overflow.
///
/// Codegen is responsible for finalizing String-typed fields to `LO_EMPTY_STRING`
/// immediately after this returns (see `runtime-abi.md` §3.1); `lo_alloc` is
/// deliberately type-agnostic.
///
/// # Safety
/// `class` must point at a valid `ClassDescriptor` whose `instance_size` is at
/// least `size_of::<Object>()`.
#[no_mangle]
pub unsafe extern "C" fn lo_alloc(class: *const ClassDescriptor) -> *mut Object {
    let size = align_up((*class).instance_size as usize, 8);
    alloc_raw(size, class)
}

/// Allocate a `StringObject` with room for `len` inline bytes, header stamped to
/// `LO_STRING_CLASS` and `length` set. The caller fills the `len` data bytes
/// (already zeroed) at `base + string_data_offset()`.
///
/// This is the internal variable-size string allocator used by the provided
/// `lo_read_string` and by the team-implemented `lo_string_*` ops. `lo_alloc`
/// cannot size a string itself (it only knows the class's fixed `instance_size`).
///
/// # Safety
/// Must be called after `heap_init`. Returns a valid `*mut Object`/`*mut
/// StringObject` or aborts on OOM.
pub(crate) unsafe fn bump_alloc_string(len: u32) -> *mut Object {
    let size = align_up(string_data_offset() + len as usize, 8);
    let obj = alloc_raw(size, &LO_STRING_CLASS);
    let so = obj as *mut StringObject;
    (*so).length = len;
    obj
}

// --- Collector interface ----------------------------------------------------
// The seam the collector (`crate::gc`) drives. `occupied` is the scan range;
// `set_free` commits the packed bump pointer. The collector does not re-derive
// the heap geometry.

/// Objects occupy `[base, free)`. The mark range-check and the left-to-right
/// scan both stop at `free`; `[free, HEAP_LIMIT)` is unused.
///
/// # Safety
/// By-value reads of the allocator statics on the single runtime thread.
pub(crate) unsafe fn occupied() -> (*mut u8, *mut u8) {
    (HEAP_BASE, FREE)
}

/// Install `new_free` as the bump pointer after compaction. Debug builds fill
/// the reclaimed tail `[new_free, HEAP_LIMIT)` with `0xEF`, then walk the packed
/// objects. The fill and the walk are compiled out of release builds, and they
/// do not depend on the process environment, so the WASM target runs them too.
///
/// The debug heap check verifies shape: descriptors, pointer targets, mark bits, and the
/// poisoned tail.
/// 
/// # Safety
/// `new_free` must lie in `[HEAP_BASE, HEAP_LIMIT]`. Called only by the collector
/// at the end of a cycle, on the single runtime thread.
pub(crate) unsafe fn set_free(new_free: *mut u8) {
    debug_assert!({
        let base = HEAP_BASE;
        let limit = HEAP_LIMIT;
        (new_free as usize) >= (base as usize) && (new_free as usize) <= (limit as usize)
    });
    FREE = new_free;
    #[cfg(debug_assertions)]
    {
        let base = HEAP_BASE;
        let limit = HEAP_LIMIT;
        if (limit as usize) > (new_free as usize) {
            let len = (limit as usize) - (new_free as usize);
            ptr::write_bytes(new_free, 0xEF, len);
        }
        debug_heap_check(base, new_free, limit);
    }
}

/// Walk the packed heap and the poisoned tail after a collection.
///
/// # Safety
/// `base`, `free`, and `limit` describe the heap just committed by [`set_free`]:
/// objects occupy `[base, free)` and `[free, limit)` is the `0xEF` tail. A null
/// `base` (uninitialized heap) is a no-op.
#[cfg(debug_assertions)]
unsafe fn debug_heap_check(base: *mut u8, free: *mut u8, limit: *mut u8) {
    if base.is_null() {
        return;
    }

    let mut starts = Vec::new();
    let mut cur = base;
    while cur < free {
        let obj = cur as *mut Object;
        let size = checked_object_size(obj, base, free, limit);
        starts.push(cur);
        cur = cur.add(size);
    }
    assert_eq!(cur, free, "object walk did not land on the bump pointer");

    let start_set: HashSet<*mut u8> = starts.iter().copied().collect();
    let empty = &LO_EMPTY_STRING as *const StringObject as *mut Object;
    for &start in &starts {
        let obj = start as *mut Object;
        let class = (*obj).class_descriptor;
        let count = (*class).pointer_count as usize;
        let offsets = (*class).pointer_offsets;
        for i in 0..count {
            let off = *offsets.add(i) as usize;
            let slot = start.add(off) as *const *mut Object;
            let target = *slot;
            let ok = target.is_null()
                || target == empty
                || start_set.contains(&(target as *mut u8));
            assert!(
                ok,
                "object {:p} field at +{off} points at {:p}, not a live object, null, or LO_EMPTY_STRING",
                obj, target
            );
        }
    }

    let mut tail = free;
    while tail < limit {
        assert_eq!(
            *tail, 0xEF,
            "reclaimed byte at {tail:p} is not poisoned"
        );
        tail = tail.add(1);
    }
}

/// Size of `obj`, after rejecting a header that cannot be a real packed object.
///
/// # Safety
/// `obj` lies in `[base, free)`. `base`, `free`, and `limit` are the committed
/// heap bounds.
#[cfg(debug_assertions)]
unsafe fn checked_object_size(
    obj: *mut Object,
    base: *mut u8,
    free: *mut u8,
    limit: *mut u8,
) -> usize {
    let remaining = (free as usize) - (obj as usize);
    let class = (*obj).class_descriptor;
    assert!(
        !class.is_null(),
        "heap object at {obj:p} has a null class descriptor"
    );
    let class_addr = class as usize;
    assert!(
        class_addr < base as usize || class_addr >= limit as usize,
        "class descriptor {class:p} for object {obj:p} lies inside the heap"
    );
    assert_eq!(
        (*obj).gc_bits, 0,
        "object {obj:p} still has gc_bits set after collection"
    );

    let size = if core::ptr::eq(class, &LO_STRING_CLASS) {
        assert!(
            remaining >= string_data_offset(),
            "string at {obj:p} is shorter than its header"
        );
        let len = (*(obj as *const StringObject)).length as usize;
        let raw = string_data_offset().checked_add(len).unwrap_or(usize::MAX);
        align_up(raw, 8)
    } else {
        let raw = (*class).instance_size as usize;
        assert!(
            raw >= core::mem::size_of::<Object>(),
            "object {obj:p} instance_size {raw} is smaller than the header"
        );
        align_up(raw, 8)
    };
    assert!(
        size > 0 && size <= remaining,
        "object {obj:p} size {size} does not fit in the {remaining} bytes left before the bump pointer"
    );

    let count = (*class).pointer_count as usize;
    let ptr_size = core::mem::size_of::<*mut Object>();
    let ptr_align = core::mem::align_of::<*mut Object>();
    if count == 0 {
        return size;
    }
    assert!(
        ptr_size > 0 && count <= size / ptr_size,
        "object {obj:p} pointer_count {count} does not fit in {size} bytes"
    );
    let offsets = (*class).pointer_offsets;
    assert!(
        !offsets.is_null(),
        "object {obj:p} has pointer fields but a null offsets array"
    );
    let offsets_addr = offsets as usize;
    assert!(
        offsets_addr < base as usize || offsets_addr >= limit as usize,
        "pointer offset array {offsets:p} for object {obj:p} lies inside the heap"
    );
    for i in 0..count {
        let off = *offsets.add(i) as usize;
        assert!(
            off % ptr_align == 0
                && off >= core::mem::size_of::<Object>()
                && off + ptr_size <= size,
            "object {obj:p} pointer offset {off} is outside the object or misaligned"
        );
    }
    size
}

/// Live bytes currently occupied (`FREE - HEAP_BASE`).
///
/// Test/diagnostic hook (not part of the ABI): lets the GC tests observe heap
/// occupancy drop after a collection reclaims dead objects.
#[doc(hidden)]
pub fn heap_used() -> usize {
    // SAFETY: by-value reads of the allocator statics on the single runtime
    // thread. Returns 0 before init (both null).
    unsafe {
        if HEAP_BASE.is_null() {
            0
        } else {
            (FREE as usize) - (HEAP_BASE as usize)
        }
    }
}
