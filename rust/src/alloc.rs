//! Bump allocator backing `lo_alloc` (`runtime-abi.md` §3.1).
//!
//! One backing allocation contains a nursery followed by two equal tenured
//! Cheney semispaces. The nursery targets `1 / NURSERY_DIVISOR` of the aligned
//! heap; rounding leftovers go to it so the tenured spaces remain equal. Live
//! data can fill one tenured space, so a smaller nursery leaves more of the heap
//! usable (about `(1 - 1/NURSERY_DIVISOR) / 2`, versus 1/2 for plain Cheney).
//!
//! Allocation bumps through the nursery. On a no-fit, `lo_alloc` collects and
//! retries once; failure to fit aborts (exit 137 native / `unreachable` WASM).
//! An object larger than the whole nursery is allocated directly at the tail of
//! the active tenured space; if that does not fit, a major collection runs
//! first, so the abort happens only when a full collection cannot make room.
//! Minor collections immediately promote survivors into the active tenured tail;
//! major collections copy nursery and active tenured survivors into the spare.
//!
//! Total heap size defaults to 16 MiB and can be overridden at initialization
//! with `LO_HEAP_SIZE`. Values below three alignment units use the default;
//! trailing bytes that do not form a full alignment unit are discarded.
//!
//! Heap state is accessed only on the single runtime thread, by value or raw
//! pointer rather than references to the mutable statics. The collector updates
//! allocation boundaries through this module's accessors.

use core::alloc::Layout;
use core::ptr;

use crate::descriptors::LO_STRING_CLASS;
use crate::object::{string_data_offset, ClassDescriptor, Object, StringObject};

/// Default total heap size (16 MiB), overridable via `LO_HEAP_SIZE` at init.
const DEFAULT_HEAP_SIZE: usize = 16 * 1024 * 1024;
/// All allocations (and each semispace base) are aligned to this; covers
/// `Object`'s 8-byte pointer field with headroom.
const HEAP_ALIGN: usize = 16;
/// The nursery targets `1 / NURSERY_DIVISOR` of the heap (at least one alignment
/// unit). The only knob for the nursery/tenured split: `heap_partition` and its
/// tests are written in terms of it, so changing it needs no other edit.
const NURSERY_DIVISOR: usize = 16;

/// Base of the whole backing region (for `dealloc`); null when uninitialized.
static mut HEAP_BASE: *mut u8 = ptr::null_mut();
/// Layout the region was allocated with (for `dealloc`).
static mut HEAP_LAYOUT: Option<Layout> = None;

/// Nursery allocation region; emptied after every collection.
static mut NURSERY_BASE: *mut u8 = ptr::null_mut();
static mut NURSERY_LIMIT: *mut u8 = ptr::null_mut();
static mut NURSERY_FREE: *mut u8 = ptr::null_mut();
/// Active tenured (from-)space: `[TENURED_FROM_BASE, TENURED_FROM_LIMIT)`, bump pointer at `TENURED_FREE`.
static mut TENURED_FROM_BASE: *mut u8 = ptr::null_mut();
static mut TENURED_FROM_LIMIT: *mut u8 = ptr::null_mut();
static mut TENURED_FREE: *mut u8 = ptr::null_mut();
/// Inactive tenured (to-)space: `[TENURED_TO_BASE, TENURED_TO_LIMIT)`. The collector copies survivors
/// here during a major collection and then [`flip`]s the roles.
static mut TENURED_TO_BASE: *mut u8 = ptr::null_mut();
static mut TENURED_TO_LIMIT: *mut u8 = ptr::null_mut();

#[inline]
const fn align_up(n: usize, align: usize) -> usize {
    (n + align - 1) & !(align - 1)
}

/// Return aligned nursery and per-tenured-space byte sizes.
fn heap_partition(total: usize) -> (usize, usize) {
    let total_units = total / HEAP_ALIGN;
    assert!(
        total_units >= 3,
        "heap requires at least three alignment units"
    );
    let target_nursery_units = (total_units / NURSERY_DIVISOR).max(1);
    let tenured_units = (total_units - target_nursery_units) / 2;
    let nursery_units = total_units - 2 * tenured_units;
    (nursery_units * HEAP_ALIGN, tenured_units * HEAP_ALIGN)
}

/// Allocate one nursery and two equal tenured semispaces. Called from
/// `lo_runtime_init`.
pub(crate) fn heap_init() {
    let total = std::env::var("LO_HEAP_SIZE")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n >= 3 * HEAP_ALIGN)
        .unwrap_or(DEFAULT_HEAP_SIZE);
    let (nursery, tenured) = heap_partition(total);
    let region = nursery + 2 * tenured;
    let layout = Layout::from_size_align(region, HEAP_ALIGN).expect("valid heap layout");
    // SAFETY: `layout` has non-zero size; `alloc_zeroed` returns either a valid
    // zeroed region of `region` bytes or null (handled). Writes below are
    // by-value stores to the allocator statics on the single runtime thread.
    unsafe {
        let base = std::alloc::alloc_zeroed(layout);
        if base.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        // gc-debug: all free space starts poisoned (see `gc_debug`).
        #[cfg(feature = "gc-debug")]
        {
            ptr::write_bytes(base, crate::gc_debug::POISON_BYTE, region);
            crate::gc_debug::reset_stats();
        }
        HEAP_BASE = base;
        HEAP_LAYOUT = Some(layout);
        NURSERY_BASE = base;
        NURSERY_LIMIT = base.add(nursery);
        NURSERY_FREE = base;
        TENURED_FROM_BASE = base.add(nursery);
        TENURED_FROM_LIMIT = base.add(nursery + tenured);
        TENURED_FREE = TENURED_FROM_BASE;
        TENURED_TO_BASE = TENURED_FROM_LIMIT;
        TENURED_TO_LIMIT = base.add(region);
    }
}

pub(crate) unsafe fn in_tenure(obj: *mut Object) -> bool {
    (obj as *mut u8) >= TENURED_FROM_BASE && (obj as *mut u8) < TENURED_FROM_LIMIT
}

pub(crate) unsafe fn in_nursery(obj: *mut Object) -> bool {
    (obj as *mut u8) >= NURSERY_BASE && (obj as *mut u8) < NURSERY_LIMIT
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
            NURSERY_BASE = ptr::null_mut();
            NURSERY_LIMIT = ptr::null_mut();
            NURSERY_FREE = ptr::null_mut();
            TENURED_FROM_BASE = ptr::null_mut();
            TENURED_FROM_LIMIT = ptr::null_mut();
            TENURED_FREE = ptr::null_mut();
            TENURED_TO_BASE = ptr::null_mut();
            TENURED_TO_LIMIT = ptr::null_mut();
        }
    }
}

/// Carve `size` zero-initialized bytes and stamp the header: from the nursery,
/// or straight from tenured space for an object larger than the nursery (see
/// [`alloc_slot`]). On a no-fit, collect and retry once; abort (exit 137
/// native) if the request still does not fit.
///
/// # Safety
/// `class` must point at a valid, live `ClassDescriptor`. `size` must be at least
/// `size_of::<Object>()` so the header fits, and a multiple of 8.
unsafe fn alloc_raw(size: usize, class: *const ClassDescriptor) -> *mut Object {
    let slot = alloc_slot(size);
    ptr::write_bytes(slot, 0, size);
    let obj = slot as *mut Object;
    (*obj).class_descriptor = class;
    (*obj).gc_bits = 0;
    (*obj).flags = 0;
    obj
}

/// Reserve `size` bytes for a new object, collecting if necessary; abort if
/// the request cannot be satisfied even after a full collection.
///
/// An object larger than the whole nursery could never fit there, so it is
/// allocated directly in the active tenured space ([`alloc_large`]). Such an
/// object is born zeroed, so it holds no old-to-young pointer yet; every later
/// pointer store into it goes through the write barrier, which records it
/// because it lives in tenured space.
unsafe fn alloc_slot(size: usize) -> *mut u8 {
    if size > NURSERY_LIMIT as usize - NURSERY_BASE as usize {
        return alloc_large(size);
    }
    let bump = bump_if_fits(size);
    if !bump.is_null() {
        return bump;
    }
    // OOM path: collect, then retry the bump exactly once. A collection
    // empties the nursery and `size` fits in an empty one, so the retry only
    // fails if `collect` itself aborted.
    crate::gc::collect(false);
    let retry = bump_if_fits(size);
    if retry.is_null() {
        crate::abort::runtime_abort("lo_alloc: out of memory", 137);
    }
    retry
}

/// Reserve `size` bytes at the tail of the active tenured space. On a no-fit,
/// run a major collection (reclaiming dead tenured objects) and retry once;
/// abort 137 if it still does not fit, i.e. when a full collection cannot make
/// room.
///
/// # Safety
/// Reads/writes the allocator statics on the single runtime thread.
unsafe fn alloc_large(size: usize) -> *mut u8 {
    let slot = bump_tenured_if_fits(size);
    if !slot.is_null() {
        return slot;
    }
    crate::gc::collect(true);
    let retry = bump_tenured_if_fits(size);
    if retry.is_null() {
        crate::abort::runtime_abort("lo_alloc: out of memory", 137);
    }
    retry
}

/// Try to bump `size` bytes from the active tenured space. Null if it does not
/// fit.
///
/// # Safety
/// Reads/writes the allocator statics on the single runtime thread.
unsafe fn bump_tenured_if_fits(size: usize) -> *mut u8 {
    let free = TENURED_FREE;
    debug_assert!(!free.is_null(), "heap must be initialized");
    let Some(new_addr) = (free as usize).checked_add(size) else {
        return ptr::null_mut();
    };
    if new_addr > TENURED_FROM_LIMIT as usize {
        return ptr::null_mut();
    }
    TENURED_FREE = free.add(size);
    free
}

/// Try to bump `size` bytes from the nursery. Return the carved slot on
/// success, or null if it does not fit. The heap must be initialized.
///
/// # Safety
/// Reads/writes the allocator statics on the single runtime thread.
unsafe fn bump_if_fits(size: usize) -> *mut u8 {
    let bump = NURSERY_FREE;
    let n_limit = NURSERY_LIMIT;
    debug_assert!(!bump.is_null(), "heap must be initialized");
    // Address arithmetic for the bounds check avoids forming an out-of-bounds
    // pointer when the request would overflow the nursery.
    let Some(new_addr) = (bump as usize).checked_add(size) else {
        return ptr::null_mut();
    };

    if new_addr > n_limit as usize {
        return ptr::null_mut();
    }
    NURSERY_FREE = bump.add(size);
    bump
}

/// Allocate a zero-initialized object of `class.instance_size` bytes
/// (`runtime-abi.md` §3.1). Fills in the header. Triggers a collection and
/// retries on a full nursery; aborts (exit 137 native) if copying overflows the
/// destination or the request still does not fit after collection.
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
// The collector uses these accessors to select source regions and copy
// destinations, reset the nursery, and commit tenured promotion or a major flip.

/// Bounds of the active tenured space, evacuated only by a major collection.
///
/// # Safety
/// By-value reads of the allocator statics on the single runtime thread.
pub(crate) unsafe fn tenure_from_space() -> (*mut u8, *mut u8) {
    (TENURED_FROM_BASE, TENURED_FROM_LIMIT)
}

/// Nursery space bounds: `(base, limit)`.
///
/// # Safety
/// By-value reads of the allocator statics on the single runtime thread.
pub(crate) unsafe fn nursery_space() -> (*mut u8, *mut u8) {
    (NURSERY_BASE, NURSERY_LIMIT)
}

pub(crate) unsafe fn nursery_free() -> *mut u8 {
    NURSERY_FREE
}

pub(crate) unsafe fn tenure_free() -> *mut u8 {
    TENURED_FREE
}

pub(crate) unsafe fn set_tenure_free(new_free: *mut u8) {
    TENURED_FREE = new_free;
}

pub(crate) unsafe fn reset_nursery() {
    NURSERY_FREE = NURSERY_BASE;
}

/// Bounds of the spare tenured space, the major collection destination.
///
/// # Safety
/// By-value read of the allocator statics on the single runtime thread.
pub(crate) unsafe fn to_space() -> (*mut u8, *mut u8) {
    (TENURED_TO_BASE, TENURED_TO_LIMIT)
}

/// Commit a major collection: swap the from/to roles and install `new_free` as the
/// bump pointer in the now-active space (which the collector just filled with
/// survivors). The old from-space becomes the inactive to-space, entirely free
/// for the next major collection.
///
/// # Safety
/// `new_free` must lie within the current to-space `[TENURED_TO_BASE, TENURED_TO_LIMIT]`. Called
/// only by the collector at the end of a major collection, on the runtime thread.
pub(crate) unsafe fn flip(new_free: *mut u8) {
    let (old_from_base, old_from_limit) = (TENURED_FROM_BASE, TENURED_FROM_LIMIT);
    TENURED_FROM_BASE = TENURED_TO_BASE;
    TENURED_FROM_LIMIT = TENURED_TO_LIMIT;
    TENURED_TO_BASE = old_from_base;
    TENURED_TO_LIMIT = old_from_limit;
    TENURED_FREE = new_free;
}

/// Allocated bytes in the nursery plus active tenured space, including garbage
/// not yet reclaimed. Excludes the spare tenured space.
///
/// Test/diagnostic hook (not part of the ABI): lets the GC tests observe heap
/// occupancy drop after a collection reclaims dead objects.
#[doc(hidden)]
pub fn heap_used() -> usize {
    // SAFETY: by-value reads of the allocator statics on the single runtime
    // thread. Returns 0 before init or after shutdown.
    unsafe {
        if TENURED_FROM_BASE.is_null() {
            0
        } else {
            (TENURED_FREE as usize) - (TENURED_FROM_BASE as usize) + (NURSERY_FREE as usize)
                - (NURSERY_BASE as usize)
        }
    }
}

/// Nursery capacity in bytes. Zero when the heap is uninitialized. Not part of
/// the ABI; lets tests size their allocations against the real nursery.
#[doc(hidden)]
pub fn nursery_size() -> usize {
    // SAFETY: by-value reads on the single runtime thread.
    unsafe { NURSERY_LIMIT as usize - NURSERY_BASE as usize }
}

/// Total backing heap capacity, including the nursery and both tenured semispaces.
/// Returns zero when the heap is uninitialized. Not part of the ABI.
#[doc(hidden)]
pub fn heap_size() -> usize {
    // SAFETY: by-value read on the single runtime thread.
    unsafe { HEAP_LAYOUT.map_or(0, |layout| layout.size()) }
}

#[cfg(test)]
mod tests {
    use super::{heap_partition, HEAP_ALIGN, NURSERY_DIVISOR};

    #[test]
    fn minimum_heap_has_three_equal_regions() {
        assert_eq!(heap_partition(3 * HEAP_ALIGN), (HEAP_ALIGN, HEAP_ALIGN));
    }

    #[test]
    fn nursery_is_one_divisor_th_of_the_heap_and_remainder_goes_to_it() {
        // Exact multiple: target nursery units = total / divisor.
        let total_units = 4 * NURSERY_DIVISOR;
        let (nursery, tenured) = heap_partition(total_units * HEAP_ALIGN);
        assert_eq!(nursery, 4 * HEAP_ALIGN);
        assert_eq!(tenured, (total_units - 4) / 2 * HEAP_ALIGN);

        // An odd leftover unit cannot split across two tenured spaces, so the
        // nursery absorbs it.
        let total_units = 4 * NURSERY_DIVISOR + 1;
        let (nursery, tenured) = heap_partition(total_units * HEAP_ALIGN);
        let tenured_units = (total_units - 4) / 2;
        assert_eq!(tenured, tenured_units * HEAP_ALIGN);
        assert_eq!(nursery, (total_units - 2 * tenured_units) * HEAP_ALIGN);
        assert!(nursery > 4 * HEAP_ALIGN, "the odd unit went to the nursery");
    }

    #[test]
    fn tenured_spaces_hold_almost_half_the_heap() {
        // The reason the nursery is small: live data can fill one tenured space.
        // Cheney gets 50%; the generational layout must stay close to it.
        for total in [4096, 65536, 1 << 20, 16 << 20] {
            let (_, tenured) = heap_partition(total);
            assert!(tenured * 100 >= total * 45, "{total}: tenured {tenured}");
        }
    }

    #[test]
    fn partitions_preserve_alignment_and_equal_tenured_spaces() {
        for total in 3 * HEAP_ALIGN..256 * HEAP_ALIGN {
            let (nursery, tenured) = heap_partition(total);
            let boundaries = [0, nursery, nursery + tenured, nursery + 2 * tenured];
            assert!(nursery >= HEAP_ALIGN);
            assert!(tenured >= HEAP_ALIGN);
            assert!(boundaries.iter().all(|offset| offset % HEAP_ALIGN == 0));
            assert_eq!(boundaries[2] - boundaries[1], boundaries[3] - boundaries[2]);
            assert_eq!(boundaries[3], total / HEAP_ALIGN * HEAP_ALIGN);
        }
    }
}
