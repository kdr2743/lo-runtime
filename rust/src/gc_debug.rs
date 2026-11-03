//! Post-collection structural checks. Metadata and shadow-frame storage must
//! still be readable: these checks are diagnostics, not a memory-safety sandbox.
use crate::descriptors::{LO_EMPTY_STRING, LO_STRING_CLASS};
use crate::object::{
    shadow_frame_roots_offset, string_data_offset, ClassDescriptor, Object, StringObject,
};
use std::collections::HashSet;
use std::mem::{align_of, size_of};

/// Validate the committed heap. Both generations' survivors are now tenured.
/// Poison ranges are precisely the regions filled by this collection.
pub(super) unsafe fn check(poisoned: &[(*mut u8, *mut u8)]) {
    let nursery = crate::alloc::nursery_space();
    let active = crate::alloc::tenure_from_space();
    let spare = crate::alloc::to_space();
    let free = crate::alloc::tenure_free() as usize;
    let regions = [nursery, active, spare];
    for &(base, limit) in &regions {
        assert!(
            !base.is_null() && (base as usize) < limit as usize,
            "invalid heap region"
        );
        assert_eq!(base as usize % 16, 0, "misaligned region");
        assert_eq!(limit as usize % 16, 0, "misaligned region limit");
    }
    for i in 0..regions.len() {
        for j in i + 1..regions.len() {
            assert!(
                regions[i].1 <= regions[j].0 || regions[j].1 <= regions[i].0,
                "overlapping heap regions"
            );
        }
    }
    assert_eq!(
        active.1 as usize - active.0 as usize,
        spare.1 as usize - spare.0 as usize
    );
    assert!(
        free >= active.0 as usize && free <= active.1 as usize,
        "invalid tenured boundary"
    );
    assert_eq!(crate::alloc::nursery_free(), nursery.0, "nursery not empty");

    let mut starts = HashSet::new();
    let mut cur = active.0 as usize;
    while cur < free {
        assert_eq!(cur % 8, 0, "misaligned object");
        assert!(free - cur >= size_of::<Object>(), "truncated object header");
        let obj = cur as *const Object;
        let class = (*obj).class_descriptor;
        check_external(class as usize, align_of::<ClassDescriptor>(), &regions);
        assert_eq!((*obj).gc_bits, 0, "destination object has forwarding bits");
        let raw = if core::ptr::eq(class, &LO_STRING_CLASS) {
            assert!(
                free - cur >= string_data_offset(),
                "truncated string header"
            );
            string_data_offset()
                .checked_add((*(obj as *const StringObject)).length as usize)
                .expect("string size overflow")
        } else {
            let size = (*class).instance_size as usize;
            assert!(size >= size_of::<Object>(), "instance smaller than header");
            size
        };
        let size = raw.checked_add(7).expect("alignment overflow") & !7;
        assert!(
            size <= free - cur,
            "object extends beyond allocation boundary"
        );
        let count = (*class).pointer_count as usize;
        assert!(
            count <= size / size_of::<*mut Object>(),
            "invalid pointer count"
        );
        if count != 0 {
            check_external(
                (*class).pointer_offsets as usize,
                align_of::<u32>(),
                &regions,
            );
        }
        for i in 0..count {
            let off = *(*class).pointer_offsets.add(i) as usize;
            assert!(
                off >= size_of::<Object>()
                    && off.is_multiple_of(align_of::<*mut Object>())
                    && off
                        .checked_add(size_of::<*mut Object>())
                        .is_some_and(|end| end <= raw),
                "pointer field outside object or misaligned"
            );
        }
        starts.insert(cur);
        cur += size;
    }
    assert_eq!(cur, free, "object walk misses allocation boundary");
    let valid = |p: *mut Object| {
        p.is_null()
            || core::ptr::eq(p, (&LO_EMPTY_STRING as *const StringObject).cast())
            || starts.contains(&(p as usize))
    };
    for &start in &starts {
        let class = (*(start as *const Object)).class_descriptor;
        for i in 0..(*class).pointer_count as usize {
            let off = *(*class).pointer_offsets.add(i) as usize;
            let target = *((start + off) as *const *mut Object);
            assert!(
                valid(target),
                "object {start:#x} field +{off} targets non-object {target:p}"
            );
        }
    }
    let mut frames = HashSet::new();
    let mut frame = crate::shadow_stack::current_frame();
    while !frame.is_null() {
        assert!(
            frames.insert(frame as usize),
            "cycle in shadow-stack frames"
        );
        let roots = (frame as *const u8)
            .add(shadow_frame_roots_offset())
            .cast::<*mut Object>();
        for i in 0..(*frame).num_roots as usize {
            let target = *roots.add(i);
            assert!(
                valid(target),
                "shadow root {i} targets non-object {target:p}"
            );
        }
        frame = (*frame).parent;
    }
    for &(base, limit) in poisoned {
        // Check machine words to keep full-region validation affordable.
        let len = limit as usize - base as usize;
        assert_eq!(len % 8, 0);
        let words = core::slice::from_raw_parts(base.cast::<u64>(), len / 8);
        assert!(
            words.iter().all(|&word| word == 0xefefefefefefefef),
            "reclaimed region is not poisoned"
        );
    }
}

fn check_external(addr: usize, alignment: usize, regions: &[(*mut u8, *mut u8)]) {
    assert!(
        addr != 0 && addr.is_multiple_of(alignment),
        "null or misaligned metadata"
    );
    assert!(
        regions
            .iter()
            .all(|&(base, limit)| addr < base as usize || addr >= limit as usize),
        "metadata points inside managed heap"
    );
}
