//! Type operations (`runtime-abi.md` §3.5).
//!
//! `lo_cast_check` returns `obj` if its class is `target` or a descendant, else
//! aborts (exit 101) after writing `lo_cast_check: cannot cast <from> to <to>`;
//! a null `obj` short-circuits to null. `lo_instanceof` returns a bool and never
//! aborts; a null receiver yields `false`. Both walk `ClassDescriptor.parent` up
//! the single-inheritance chain.

use crate::object::{ClassDescriptor, Object};

/// True if `class` is `target` or a descendant, walking `.parent` until null.
///
/// # Safety
/// `class`, if non-null, must point at a valid `ClassDescriptor` chain.
unsafe fn is_subtype(mut class: *const ClassDescriptor, target: *const ClassDescriptor) -> bool {
    while !class.is_null() {
        if core::ptr::eq(class, target) {
            return true;
        }
        class = (*class).parent;
    }
    false
}

/// Read `name` / `name_len` from a descriptor (length excludes the trailing NUL).
///
/// # Safety
/// `class` must point at a valid `ClassDescriptor` whose `name` covers `name_len`
/// readable UTF-8 bytes.
unsafe fn class_name(class: *const ClassDescriptor) -> &'static str {
    let bytes = core::slice::from_raw_parts((*class).name, (*class).name_len as usize);
    core::str::from_utf8(bytes).unwrap_or("<invalid>")
}

/// Checked downcast: return `obj` if its class is `target` or a descendant; abort
/// (exit 101) otherwise. Null `obj` returns null.
///
/// # Safety
/// `obj`, if non-null, must point at a valid object; `target` must point at a
/// valid `ClassDescriptor`.
#[no_mangle]
pub unsafe extern "C" fn lo_cast_check(
    obj: *mut Object,
    target: *const ClassDescriptor,
) -> *mut Object {
    if obj.is_null() {
        return obj;
    }
    let runtime_class = (*obj).class_descriptor;
    if is_subtype(runtime_class, target) {
        return obj;
    }
    crate::abort::runtime_abort(
        &format!(
            "lo_cast_check: cannot cast {} to {}",
            class_name(runtime_class),
            class_name(target),
        ),
        101,
    );
}

/// Return true iff `obj`'s class is `target` or a descendant. Null `obj` yields
/// false; never aborts.
///
/// # Safety
/// `obj`, if non-null, must point at a valid object; `target` must point at a
/// valid `ClassDescriptor`.
#[no_mangle]
pub unsafe extern "C" fn lo_instanceof(obj: *mut Object, target: *const ClassDescriptor) -> bool {
    if obj.is_null() {
        return false;
    }
    is_subtype((*obj).class_descriptor, target)
}
