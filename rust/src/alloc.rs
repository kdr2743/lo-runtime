//! Bump allocator backing `lo_alloc` (`runtime-abi.md` §3.1).
//!
//! One backing allocation contains a nursery followed by two equal tenured
//! Cheney semispaces. The nursery targets one quarter of the aligned heap;
//! rounding leftovers go to it so the tenured spaces remain equal.
//!
//! This is a transitional layout only: the nursery is reserved, not yet used.
//! Allocation bumps `TENURED_FREE` in the active tenured from-space. On a no-fit,
//! `lo_alloc` invokes the existing Cheney collector and retries once; live-set
//! overflow aborts (exit 137 native / `unreachable` WASM). Minor collection is
//! not implemented here.
//!
//! Total heap size defaults to 16 MiB and can be overridden at initialization
//! with `LO_HEAP_SIZE`. Values below three alignment units use the default;
//! trailing bytes that do not form a full alignment unit are discarded.
//!
//! The heap pointers are `static mut` raw scalars (the runbook sanctions
//! `static mut` for the bump pointer). Every access is by value or through a raw
//! pointer — never `&`/`&mut` of the statics — which keeps this module clear of
//! the `static_mut_refs` lint. Invariant: only the functions here and the
//! collector's commit ([`flip`]) touch these, and only on the single runtime
//! thread.

use core::alloc::Layout;
use core::ptr;

use crate::descriptors::LO_STRING_CLASS;
use crate::object::{string_data_offset, ClassDescriptor, Object, StringObject};

/// Default total heap size (16 MiB), overridable via `LO_HEAP_SIZE` at init.
const DEFAULT_HEAP_SIZE: usize = 16 * 1024 * 1024;
/// All allocations (and each semispace base) are aligned to this; covers
/// `Object`'s 8-byte pointer field with headroom.
const HEAP_ALIGN: usize = 16;

/// Base of the whole backing region (for `dealloc`); null when uninitialized.
static mut HEAP_BASE: *mut u8 = ptr::null_mut();
/// Layout the region was allocated with (for `dealloc`).
static mut HEAP_LAYOUT: Option<Layout> = None;

/// Reserved nursery; unused until minor collection is implemented.
static mut NURSERY_BASE: *mut u8 = ptr::null_mut();
static mut NURSERY_LIMIT: *mut u8 = ptr::null_mut();
static mut NURSERY_FREE: *mut u8 = ptr::null_mut();
/// Active tenured (from-)space: `[TENURED_FROM_BASE, TENURED_FROM_LIMIT)`, bump pointer at `TENURED_FREE`.
static mut TENURED_FROM_BASE: *mut u8 = ptr::null_mut();
static mut TENURED_FROM_LIMIT: *mut u8 = ptr::null_mut();
static mut TENURED_FREE: *mut u8 = ptr::null_mut();
/// Inactive tenured (to-)space: `[TENURED_TO_BASE, TENURED_TO_LIMIT)`. The collector copies survivors
/// here and then [`flip`]s the roles.
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
    let target_nursery_units = (total_units / 4).max(1);
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
    obj as *mut u8 >= TENURED_FROM_BASE && obj as *mut u8 <= TENURED_FROM_LIMIT
}

pub(crate) unsafe fn in_nursery(obj: *mut Object) -> bool {
    obj as *mut u8 >= NURSERY_BASE && obj as *mut u8 <= NURSERY_LIMIT
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

/// Carve `size` zero-initialized bytes from the active semispace and stamp the
/// object header. On a no-fit, forces one collection and retries; aborts (exit
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

/// Try to bump `size` bytes off the active semispace. Returns the carved slot on
/// success, or null if it does not fit (or the heap is uninitialized).
///
/// # Safety
/// Reads/writes the allocator statics on the single runtime thread.
unsafe fn bump_if_fits(size: usize) -> *mut u8 {
    let bump = TENURED_FREE;
    let limit = TENURED_FROM_LIMIT;
    // Address arithmetic for the bounds check avoids forming an out-of-bounds
    // pointer when the request would overflow the semispace.
    let new_addr = (bump as usize).wrapping_add(size);
    if bump.is_null() || new_addr > limit as usize {
        return ptr::null_mut();
    }
    TENURED_FREE = bump.add(size);
    bump
}

/// Allocate a zero-initialized object of `class.instance_size` bytes
/// (`runtime-abi.md` §3.1). Fills in the header. Triggers a collection and
/// retries on a full semispace; aborts (exit 137) only on live-set overflow.
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
// These are the seam the Cheney collector (`crate::gc`) drives. They are
// `pub(crate)` raw-pointer accessors so the collector can range-check from-space,
// find where to copy survivors, and commit the flip — without re-deriving the
// heap geometry.

/// Bounds of the active (from-)space being evacuated: `(base, limit)`.
///
/// # Safety
/// By-value reads of the allocator statics on the single runtime thread.
pub(crate) unsafe fn from_space() -> (*mut u8, *mut u8) {
    (TENURED_FROM_BASE, TENURED_FROM_LIMIT)
}

/// Start of the inactive (to-)space — where the collector copies survivors.
///
/// # Safety
/// By-value read of the allocator statics on the single runtime thread.
pub(crate) unsafe fn to_space_base() -> *mut u8 {
    TENURED_TO_BASE
}

/// Commit a collection: swap the from/to roles and install `new_free` as the
/// bump pointer in the now-active space (which the collector just filled with
/// survivors). The old from-space becomes the inactive to-space, entirely free
/// for the next cycle (runbook WS-2 §2.4 step 4).
///
/// # Safety
/// `new_free` must lie within the current to-space `[TENURED_TO_BASE, TENURED_TO_LIMIT]`. Called
/// only by the collector at the end of a cycle, on the single runtime thread.
pub(crate) unsafe fn flip(new_free: *mut u8) {
    let (old_from_base, old_from_limit) = (TENURED_FROM_BASE, TENURED_FROM_LIMIT);
    TENURED_FROM_BASE = TENURED_TO_BASE;
    TENURED_FROM_LIMIT = TENURED_TO_LIMIT;
    TENURED_TO_BASE = old_from_base;
    TENURED_TO_LIMIT = old_from_limit;
    TENURED_FREE = new_free;
}

/// Live bytes currently occupied in the active semispace (`TENURED_FREE - TENURED_FROM_BASE`).
///
/// Test/diagnostic hook (not part of the ABI): lets the GC tests observe heap
/// occupancy drop after a collection reclaims dead objects.
#[doc(hidden)]
pub fn heap_used() -> usize {
    // SAFETY: by-value reads of the allocator statics on the single runtime
    // thread. Returns 0 before init (both null).
    unsafe {
        if TENURED_FROM_BASE.is_null() {
            0
        } else {
            (TENURED_FREE as usize) - (TENURED_FROM_BASE as usize)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{heap_partition, HEAP_ALIGN};

    #[test]
    fn minimum_heap_has_three_equal_regions() {
        assert_eq!(heap_partition(3 * HEAP_ALIGN), (HEAP_ALIGN, HEAP_ALIGN));
    }

    #[test]
    fn odd_units_leave_remainder_in_nursery() {
        assert_eq!(heap_partition(7 * HEAP_ALIGN), (HEAP_ALIGN, 3 * HEAP_ALIGN));
        assert_eq!(
            heap_partition(9 * HEAP_ALIGN),
            (3 * HEAP_ALIGN, 3 * HEAP_ALIGN)
        );
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
