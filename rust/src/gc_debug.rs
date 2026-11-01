//! Debug-only GC instrumentation, compiled under the `gc-debug` cargo feature.
//!
//! Three tools, all driven from the end of [`crate::gc::collect`]:
//!
//! - **Poisoning.** The whole heap is filled with [`POISON_BYTE`] at init, and
//!   every region a collection frees is re-filled: the nursery after every
//!   collection, and the old active tenured space after a major. Free space is
//!   therefore always poison, so a stale pointer reads `0xEF…` and fails at once.
//!   (The allocator zeroes each slot it hands out, so poison never leaks into
//!   live objects.)
//! - **Heap checker.** [`verify_heap`] walks the active tenured space and checks
//!   the invariants listed on it. A failure aborts with a message naming the
//!   violated invariant. The runtime cannot know the compiler's class
//!   descriptors, so "valid descriptor" means structurally sane, not registered.
//! - **Stats and logging.** [`GcStats`] counters are kept on every target and
//!   readable through [`gc_debug_stats`]. Native builds also log one line per
//!   collection to stderr. WASM keeps counters only: `host_write_stderr` is the
//!   abort channel and the print family is the program's own output, so neither
//!   is safe for diagnostics.
//!
//! None of this is part of the ABI. The checker aborts with exit status
//! [`CHECK_FAILED_EXIT`], which is deliberately not one of the ABI's codes.

use std::cell::Cell;
use std::collections::HashSet;

use crate::gc::object_size;
use crate::object::{shadow_frame_roots_offset, ClassDescriptor, Object};

/// Fill byte for free space.
pub(crate) const POISON_BYTE: u8 = 0xEF;
/// Exit status for a failed heap check (native). Not an ABI code.
const CHECK_FAILED_EXIT: i32 = 70;
/// Longest parent chain the checker follows before calling it cyclic.
const MAX_PARENT_DEPTH: usize = 1024;

/// Cumulative collection counters since `lo_runtime_init`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GcStats {
    pub collections: u64,
    pub minors: u64,
    pub majors: u64,
    /// Bytes reclaimed, summed over collections.
    pub reclaimed_bytes: u64,
    /// Survivor objects copied, summed over collections.
    pub copied_objects: u64,
}

thread_local! {
    static STATS: Cell<GcStats> = const {
        Cell::new(GcStats {
            collections: 0,
            minors: 0,
            majors: 0,
            reclaimed_bytes: 0,
            copied_objects: 0,
        })
    };
}

/// Counters since the last `lo_runtime_init`. Test/diagnostic hook, not ABI.
pub fn gc_debug_stats() -> GcStats {
    STATS.with(Cell::get)
}

pub(crate) fn reset_stats() {
    STATS.with(|s| s.set(GcStats::default()));
}

/// What `collect` tells the hook about the collection that just finished.
pub(crate) struct CollectSummary {
    pub major: bool,
    /// True when the collection was forced rather than triggered by allocation.
    /// Only the native log line reads it.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub explicit: bool,
    /// `heap_used()` before the collection.
    pub used_before: usize,
    /// Start of the bytes this collection copied into the active tenured space.
    pub copied_start: *mut u8,
    /// Nursery bounds, evacuated by every collection.
    pub nursery: (*mut u8, *mut u8),
    /// Old active tenured bounds, evacuated (and now spare) after a major.
    pub freed_tenure: Option<(*mut u8, *mut u8)>,
}

/// Fill `[base, limit)` with poison.
unsafe fn poison(base: *mut u8, limit: *mut u8) {
    if !base.is_null() {
        core::ptr::write_bytes(base, POISON_BYTE, limit as usize - base as usize);
    }
}

/// Run after every collection, once the allocator state is committed: poison
/// freed space, verify the heap, then update stats and log.
///
/// # Safety
/// Must be called at the end of `collect`, on the runtime thread, with the
/// allocator's bump pointers already updated.
pub(crate) unsafe fn after_collection(s: CollectSummary) {
    poison(s.nursery.0, s.nursery.1);
    if let Some((base, limit)) = s.freed_tenure {
        poison(base, limit);
    }

    let walk = match walk_and_check(s.copied_start as usize) {
        Ok(walk) => walk,
        Err(msg) => crate::abort::runtime_abort(
            &format!("lo_gc_debug: heap check failed: {msg}"),
            CHECK_FAILED_EXIT,
        ),
    };

    let used_after = crate::alloc::heap_used();
    let reclaimed = s.used_before.saturating_sub(used_after);
    let mut stats = gc_debug_stats();
    stats.collections += 1;
    if s.major {
        stats.majors += 1;
    } else {
        stats.minors += 1;
    }
    stats.reclaimed_bytes += reclaimed as u64;
    stats.copied_objects += walk.copied as u64;
    STATS.with(|c| c.set(stats));

    #[cfg(not(target_arch = "wasm32"))]
    eprintln!(
        "[lo-gc] #{} {} trigger={} reclaimed={} objects={} used={}",
        stats.collections,
        if s.major { "major" } else { "minor" },
        if s.explicit { "explicit" } else { "alloc" },
        reclaimed,
        walk.copied,
        used_after,
    );
}

struct Walk {
    /// Objects in the active tenured space at or above the caller's
    /// `copied_from` address.
    copied: usize,
}

/// Check every heap invariant that must hold between collections.
///
/// - The nursery is empty and fully poisoned; the active tenured tail and the
///   whole spare space are fully poisoned.
/// - The remembered set is empty.
/// - Every object in the active tenured space has a structurally valid
///   descriptor (aligned, outside the heap, sane size, pointer offsets inside
///   the instance, terminating parent chain) and no forwarding bit.
/// - Every pointer field and every shadow-stack root is null, the start of a
///   live tenured object, or outside the heap (a static such as
///   `LO_EMPTY_STRING`). Nothing may point into the nursery, the tail, the
///   spare space, or the middle of an object.
#[cfg(test)]
pub(crate) fn verify_heap() -> Result<(), String> {
    // SAFETY: called on the runtime thread with an initialized heap.
    unsafe { walk_and_check(usize::MAX).map(|_| ()) }
}

unsafe fn check_poisoned(name: &str, base: *mut u8, limit: *mut u8) -> Result<(), String> {
    let len = (limit as usize).saturating_sub(base as usize);
    let bytes = core::slice::from_raw_parts(base, len);
    match bytes.iter().position(|&b| b != POISON_BYTE) {
        None => Ok(()),
        Some(i) => Err(format!("{name} not poisoned at offset {i} ({base:p}+{i})")),
    }
}

unsafe fn walk_and_check(copied_from: usize) -> Result<Walk, String> {
    let (nursery_base, nursery_limit) = crate::alloc::nursery_space();
    let (from_base, from_limit) = crate::alloc::tenure_from_space();
    let (to_base, to_limit) = crate::alloc::to_space();
    let free = crate::alloc::tenure_free();
    if nursery_base.is_null() {
        return Err("heap is not initialized".into());
    }
    let heap_lo = nursery_base as usize;
    let heap_hi = (from_limit as usize).max(to_limit as usize);

    // Free space: poison, and the structural facts that make walking safe.
    if crate::alloc::nursery_free() != nursery_base {
        return Err("nursery not empty after collection".into());
    }
    if free < from_base || free > from_limit {
        return Err("tenured bump pointer outside the active space".into());
    }
    check_poisoned("nursery", nursery_base, nursery_limit)?;
    check_poisoned("tenured tail", free, from_limit)?;
    check_poisoned("spare space", to_base, to_limit)?;
    let remembered = crate::gc::remembered_len();
    if remembered != 0 {
        return Err(format!("remembered set holds {remembered} entries"));
    }

    // Pass 1: walk the active tenured space object by object.
    let mut starts: HashSet<usize> = HashSet::new();
    let mut objects: Vec<*mut Object> = Vec::new();
    let mut cursor = from_base;
    while cursor < free {
        let obj = cursor as *mut Object;
        if !(cursor as usize).is_multiple_of(8) {
            return Err(format!("object {obj:p} is not 8-byte aligned"));
        }
        check_descriptor((*obj).class_descriptor, heap_lo, heap_hi)
            .map_err(|e| format!("object {obj:p}: {e}"))?;
        if (*obj).gc_bits & crate::gc::GC_FORWARDED_BIT != 0 {
            return Err(format!("object {obj:p} still carries the forwarding bit"));
        }
        let size = object_size(obj);
        if size < core::mem::size_of::<Object>() || size > free as usize - cursor as usize {
            return Err(format!("object {obj:p} has impossible size {size}"));
        }
        starts.insert(cursor as usize);
        objects.push(obj);
        cursor = cursor.add(size);
    }

    // Pass 2: every pointer field.
    let mut copied = 0;
    for &obj in &objects {
        if obj as usize >= copied_from {
            copied += 1;
        }
        let class = (*obj).class_descriptor;
        for i in 0..(*class).pointer_count as usize {
            let off = *(*class).pointer_offsets.add(i) as usize;
            let value = *((obj as *mut u8).add(off) as *const *mut Object);
            check_ref(value, &starts, heap_lo, heap_hi)
                .map_err(|e| format!("object {obj:p} field at +{off}: {e}"))?;
        }
    }

    // Shadow-stack roots.
    let mut frame = crate::shadow_stack::current_frame();
    while !frame.is_null() {
        let roots = (frame as *mut u8).add(shadow_frame_roots_offset()) as *const *mut Object;
        for i in 0..(*frame).num_roots as usize {
            check_ref(*roots.add(i), &starts, heap_lo, heap_hi)
                .map_err(|e| format!("shadow-stack root {i}: {e}"))?;
        }
        frame = (*frame).parent;
    }

    Ok(Walk { copied })
}

/// A reference is valid if null, outside the heap, or the start of a live object.
fn check_ref(
    value: *mut Object,
    starts: &HashSet<usize>,
    heap_lo: usize,
    heap_hi: usize,
) -> Result<(), String> {
    let addr = value as usize;
    if value.is_null() || addr < heap_lo || addr >= heap_hi || starts.contains(&addr) {
        return Ok(());
    }
    if addr == usize::from_ne_bytes([POISON_BYTE; core::mem::size_of::<usize>()]) {
        return Err(format!("poison pointer {value:p}"));
    }
    Err(format!(
        "{value:p} is inside the heap but not at a live object"
    ))
}

/// Structural check of a descriptor and its parent chain, without trusting it
/// until each pointer has been checked.
unsafe fn check_descriptor(
    class: *const ClassDescriptor,
    heap_lo: usize,
    heap_hi: usize,
) -> Result<(), String> {
    let poison_addr = usize::from_ne_bytes([POISON_BYTE; core::mem::size_of::<usize>()]);
    let plausible = |c: *const ClassDescriptor| {
        let a = c as usize;
        !c.is_null()
            && a != poison_addr
            && a.is_multiple_of(core::mem::align_of::<ClassDescriptor>())
            && !(heap_lo..heap_hi).contains(&a)
    };
    if !plausible(class) {
        return Err(format!("bad class descriptor pointer {class:p}"));
    }

    let header = core::mem::size_of::<Object>();
    let ptr_size = core::mem::size_of::<*mut Object>();
    let instance_size = (*class).instance_size as usize;
    if instance_size < header {
        return Err(format!("instance_size {instance_size} smaller than header"));
    }
    let count = (*class).pointer_count as usize;
    if count > 0 && (*class).pointer_offsets.is_null() {
        return Err("pointer_count > 0 with null pointer_offsets".into());
    }
    for i in 0..count {
        let off = *(*class).pointer_offsets.add(i) as usize;
        if off < header || !off.is_multiple_of(ptr_size) || off + ptr_size > instance_size {
            return Err(format!(
                "pointer offset {off} invalid for instance_size {instance_size}"
            ));
        }
    }

    let mut parent = (*class).parent;
    for _ in 0..MAX_PARENT_DEPTH {
        if parent.is_null() {
            return Ok(());
        }
        if !plausible(parent) {
            return Err(format!("bad parent descriptor pointer {parent:p}"));
        }
        parent = (*parent).parent;
    }
    Err("parent chain is cyclic or too deep".into())
}
