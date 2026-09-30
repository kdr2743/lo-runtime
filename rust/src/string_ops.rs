//! String operations (`runtime-abi.md` §3.2). All **stubbed** — the team
//! implements these in P3. Each carries the exact C-ABI signature so the
//! skeleton links; calling one panics with a recognizable message.
//!
//! Implementation hints for the team live in the ABI: `lo_string_repeat` aborts
//! on negative count (exit 120); `lo_string_compare` is lexicographic UTF-8 byte
//! ordering; `lo_string_reverse` reverses codepoints, not bytes. The internal
//! variable-size allocator to build results with is `alloc::bump_alloc_string`.

use crate::alloc::bump_alloc_string;
use crate::object::Object;
use crate::object::{string_data_offset, ShadowFrame, StringObject};
use crate::shadow_stack::{lo_pop_frame, lo_push_frame};

/// Construct a string from `len` raw UTF-8 bytes (copied; caller owns the
/// source).
///
/// # Safety
/// `bytes` must point at `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn lo_string_new(bytes: *const u8, len: u32) -> *mut Object {
    // SAFETY: caller guarantees `bytes` points at `len` readable bytes. No allocation needed because by
    unsafe {
        // Guard the header-offset + alignment add (mirrors lo_string_concat)
        // so it can't overflow usize on wasm32.
        if (len as usize)
            .checked_add(string_data_offset())
            .and_then(|size| size.checked_add(7))
            .is_none()
        {
            crate::abort::runtime_abort("lo_alloc: out of memory", 137);
        }

        let new_string = bump_alloc_string(len);
        let destination = new_string.cast::<u8>().add(string_data_offset());
        core::ptr::copy_nonoverlapping(bytes, destination, len as usize);
        new_string
    }
}

/// Return a new string `a + b`.
///
/// # Safety
/// `a` and `b` must point at valid `StringObject`s.
#[no_mangle]
pub unsafe extern "C" fn lo_string_concat(a: *mut Object, b: *mut Object) -> *mut Object {
    #[repr(C)]
    struct ConcatFrame {
        parent: *mut ShadowFrame,
        num_roots: u32,
        roots: [*mut Object; 2],
    }

    // SAFETY: the caller supplies valid strings. The frame keeps both inputs
    // alive across allocation, and the collector updates its slots if they move.
    unsafe {
        let a_len = (*a.cast::<StringObject>()).length;
        let b_len = (*b.cast::<StringObject>()).length;
        let total = a_len
            .checked_add(b_len)
            .unwrap_or_else(|| crate::abort::runtime_abort("lo_alloc: out of memory", 137));
        // Include the inline header and allocator alignment without overflowing
        // usize on wasm32, before passing the length to bump_alloc_string.
        if (total as usize)
            .checked_add(string_data_offset())
            .and_then(|size| size.checked_add(7)) // bytes that could be added when aligning up to 8 bytes
            .is_none()
        {
            crate::abort::runtime_abort("lo_alloc: out of memory", 137);
        }

        let mut frame = ConcatFrame {
            parent: core::ptr::null_mut(),
            num_roots: 2,
            roots: [a, b],
        };
        lo_push_frame((&raw mut frame).cast::<ShadowFrame>());
        let new_string = bump_alloc_string(total);
        let destination = new_string.cast::<u8>().add(string_data_offset());
        core::ptr::copy_nonoverlapping(
            frame.roots[0].cast::<u8>().add(string_data_offset()),
            destination,
            a_len as usize,
        );
        core::ptr::copy_nonoverlapping(
            frame.roots[1].cast::<u8>().add(string_data_offset()),
            destination.add(a_len as usize),
            b_len as usize,
        );
        lo_pop_frame();
        new_string
    }
}

/// Return a new string: `s` repeated `n` times. Aborts (exit 120) on negative
/// `n`.
///
/// # Safety
/// `s` must point at a valid `StringObject`.
#[no_mangle]
pub unsafe extern "C" fn lo_string_repeat(s: *mut Object, n: i32) -> *mut Object {
    #[repr(C)]
    struct RepeatFrame {
        parent: *mut ShadowFrame,
        num_roots: u32,
        roots: [*mut Object; 1],
    }

    if n < 0 {
        crate::abort::runtime_abort("lo_string_repeat: negative count", 120);
    }

    // SAFETY: the caller supplies a valid string. The frame keeps the input alive across allocation,
    // and the collector updates its slot if it moves.
    unsafe {
        let str = s.cast::<crate::object::StringObject>();
        let len = (*str).length as usize;
        let new_len = len
            .checked_mul(n as usize)
            .unwrap_or_else(|| crate::abort::runtime_abort("lo_alloc: out of memory", 137));
        if new_len > u32::MAX as usize
            || new_len
                .checked_add(string_data_offset())
                .and_then(|size| size.checked_add(7))
                .is_none()
        {
            crate::abort::runtime_abort("lo_alloc: out of memory", 137);
        }
        let new_len = new_len as u32;
        let mut frame = RepeatFrame {
            parent: core::ptr::null_mut(),
            num_roots: 1,
            roots: [s],
        };
        lo_push_frame((&raw mut frame).cast::<ShadowFrame>());
        let new_str = bump_alloc_string(new_len);
        for i in 0..n {
            core::ptr::copy_nonoverlapping(
                frame.roots[0]
                    .cast::<u8>()
                    .add(crate::object::string_data_offset()),
                new_str
                    .cast::<u8>()
                    .add(crate::object::string_data_offset() + i as usize * len),
                len,
            );
        }
        lo_pop_frame();
        new_str
    }
}

/// Compare two strings, returning negative / zero / positive by lexicographic
/// UTF-8 byte ordering.
///
/// # Safety
/// `a` and `b` must point at valid `StringObject`s.
#[no_mangle]
pub unsafe extern "C" fn lo_string_compare(a: *mut Object, b: *mut Object) -> i32 {
    // SAFETY: caller guarantees `a` and `b` point at valid `StringObject`s.
    // No allocation happens here, so no GC rooting is needed.
    unsafe {
        let a_len = (*a.cast::<StringObject>()).length as usize;
        let b_len = (*b.cast::<StringObject>()).length as usize;
        let a_bytes = core::slice::from_raw_parts(a.cast::<u8>().add(string_data_offset()), a_len);
        let b_bytes = core::slice::from_raw_parts(b.cast::<u8>().add(string_data_offset()), b_len);
        match a_bytes.cmp(b_bytes) {
            core::cmp::Ordering::Less => -1,
            core::cmp::Ordering::Equal => 0,
            core::cmp::Ordering::Greater => 1,
        }
    }
}

/// Return a new string with codepoints reversed.
///
/// # Safety
/// `s` must point at a valid `StringObject`.
#[no_mangle]
pub unsafe extern "C" fn lo_string_reverse(s: *mut Object) -> *mut Object {
    #[repr(C)]
    struct ReverseFrame {
        parent: *mut ShadowFrame,
        num_roots: u32,
        roots: [*mut Object; 1],
    }

    // SAFETY: caller guarantees `s` points at a valid `StringObject`. The
    // frame keeps `s` alive and current across the allocation below.
    unsafe {
        let len = (*s.cast::<StringObject>()).length as usize;

        let mut frame = ReverseFrame {
            parent: core::ptr::null_mut(),
            num_roots: 1,
            roots: [s],
        };
        lo_push_frame((&raw mut frame).cast::<ShadowFrame>());
        let new_string = bump_alloc_string(len as u32);
        let source = frame.roots[0].cast::<u8>().add(string_data_offset());
        let destination = new_string.cast::<u8>().add(string_data_offset());

        // Walk forward once, classifying each codepoint's byte length from
        // its UTF-8 leading byte (runtime-abi.md §3.2: 1-byte 0xxxxxxx,
        // 2-byte 110xxxxx, 3-byte 1110xxxx, 4-byte 11110xxx), and copy it
        // straight to its mirrored spot in the output: total length doesn't
        // change, so the codepoint at [i, i + cp_len) belongs at
        // [len - i - cp_len, len - i) once reversed.
        let mut i = 0;
        while i < len {
            let byte = *source.add(i);
            let cp_len = if byte & 0x80 == 0x00 {
                1
            } else if byte & 0xE0 == 0xC0 {
                2
            } else if byte & 0xF0 == 0xE0 {
                3
            } else {
                4
            };
            core::ptr::copy_nonoverlapping(
                source.add(i),
                destination.add(len - i - cp_len),
                cp_len,
            );
            i += cp_len;
        }

        lo_pop_frame();
        new_string
    }
}
