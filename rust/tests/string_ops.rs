//! Direct conformance tests for the five string entry points in
//! `runtime-abi.md` §3.2.

use lo_runtime::{
    heap_used, lo_pop_frame, lo_push_frame, lo_runtime_init, lo_runtime_shutdown,
    lo_string_compare, lo_string_concat, lo_string_new, lo_string_repeat, lo_string_reverse,
    nursery_size, string_data_offset, Object, ShadowFrame, StringObject, LO_STRING_CLASS,
};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

static RUNTIME_LOCK: Mutex<()> = Mutex::new(());

struct RuntimeGuard {
    _lock: MutexGuard<'static, ()>,
}

impl RuntimeGuard {
    fn start(heap_size: Option<&str>) -> Self {
        let lock = RUNTIME_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match heap_size {
            Some(size) => std::env::set_var("LO_HEAP_SIZE", size),
            None => std::env::remove_var("LO_HEAP_SIZE"),
        }
        lo_runtime_init();
        Self { _lock: lock }
    }
}

impl Drop for RuntimeGuard {
    fn drop(&mut self) {
        lo_runtime_shutdown();
        std::env::remove_var("LO_HEAP_SIZE");
    }
}

#[repr(C)]
struct TestFrame<const N: usize> {
    parent: *mut ShadowFrame,
    num_roots: u32,
    roots: [*mut Object; N],
}

impl<const N: usize> TestFrame<N> {
    fn new() -> Self {
        Self {
            parent: core::ptr::null_mut(),
            num_roots: N as u32,
            roots: [core::ptr::null_mut(); N],
        }
    }

    fn as_shadow_frame(&mut self) -> *mut ShadowFrame {
        (self as *mut Self).cast::<ShadowFrame>()
    }
}

unsafe fn new_string(bytes: &[u8]) -> *mut Object {
    lo_string_new(bytes.as_ptr(), bytes.len() as u32)
}

unsafe fn string_bytes(string: *mut Object) -> Vec<u8> {
    assert!(!string.is_null());
    assert_eq!((*string).class_descriptor, &LO_STRING_CLASS as *const _);
    let length = (*string.cast::<StringObject>()).length as usize;
    core::slice::from_raw_parts(string.cast::<u8>().add(string_data_offset()), length).to_vec()
}

unsafe fn assert_string(string: *mut Object, expected: &[u8]) {
    assert_eq!(string_bytes(string), expected);
}

/// Fill the nursery to 16 bytes short of full, leaving less room than the
/// smallest string allocation (24 bytes). The next string result must therefore
/// collect before it can be allocated.
unsafe fn leave_insufficient_nursery_space() {
    let target_used = nursery_size() - 16;
    let used = heap_used();
    assert!(used < target_used);
    let allocation_size = target_used - used;
    assert_eq!(allocation_size % 8, 0);
    let filler_length = allocation_size
        .checked_sub(string_data_offset())
        .expect("room for a StringObject header");
    let filler = vec![b'x'; filler_length];
    let _dead = new_string(&filler);
    assert_eq!(heap_used(), target_used);
}

#[test]
fn string_new_copies_bytes_and_handles_empty_utf8() {
    let _runtime = RuntimeGuard::start(None);
    let mut source = "hé😀".as_bytes().to_vec();
    let expected = source.clone();

    unsafe {
        let string = new_string(&source);
        source.fill(b'x');
        assert_string(string, &expected);

        let empty = new_string(b"");
        assert_string(empty, b"");
    }
}

#[test]
fn string_concat_preserves_operand_order_and_utf8() {
    let _runtime = RuntimeGuard::start(None);

    unsafe {
        let left = new_string("hé".as_bytes());
        let right = new_string("😀!".as_bytes());
        assert_string(lo_string_concat(left, right), "hé😀!".as_bytes());

        let empty = new_string(b"");
        assert_string(lo_string_concat(empty, right), "😀!".as_bytes());
        assert_string(lo_string_concat(left, empty), "hé".as_bytes());
    }
}

#[test]
fn string_repeat_handles_zero_one_and_multibyte_counts() {
    let _runtime = RuntimeGuard::start(None);

    unsafe {
        let string = new_string("é😀".as_bytes());
        assert_string(lo_string_repeat(string, 0), b"");
        assert_string(lo_string_repeat(string, 1), "é😀".as_bytes());
        assert_string(lo_string_repeat(string, 3), "é😀é😀é😀".as_bytes());
    }
}

#[test]
fn string_compare_uses_lexicographic_utf8_byte_order() {
    let _runtime = RuntimeGuard::start(None);

    unsafe {
        let equal_a = new_string("café".as_bytes());
        let equal_b = new_string("café".as_bytes());
        let prefix = new_string("caf".as_bytes());
        let greater = new_string("caff".as_bytes());

        assert_eq!(lo_string_compare(equal_a, equal_b), 0);
        assert!(lo_string_compare(prefix, equal_a) < 0);
        assert!(lo_string_compare(equal_a, prefix) > 0);
        assert!(lo_string_compare(equal_a, greater) > 0);
        assert!(lo_string_compare(greater, equal_a) < 0);
    }
}

#[test]
fn string_reverse_reverses_complete_utf8_codepoints() {
    let _runtime = RuntimeGuard::start(None);

    unsafe {
        let mixed = new_string("Aé€😀".as_bytes());
        assert_string(lo_string_reverse(mixed), "😀€éA".as_bytes());

        let one = new_string("😀".as_bytes());
        assert_string(lo_string_reverse(one), "😀".as_bytes());

        let empty = new_string(b"");
        assert_string(lo_string_reverse(empty), b"");
    }
}

#[test]
fn concat_roots_operands_when_result_allocation_collects() {
    let _runtime = RuntimeGuard::start(Some("4096"));
    let mut frame = TestFrame::<2>::new();

    unsafe {
        lo_push_frame(frame.as_shadow_frame());
        frame.roots[0] = new_string(b"ab");
        frame.roots[1] = new_string("é".as_bytes());
        let old_left = frame.roots[0];
        let old_right = frame.roots[1];
        leave_insufficient_nursery_space();

        let result = lo_string_concat(frame.roots[0], frame.roots[1]);
        assert_ne!(frame.roots[0], old_left);
        assert_ne!(frame.roots[1], old_right);
        assert_string(result, "abé".as_bytes());
        lo_pop_frame();
    }
}

#[test]
fn repeat_roots_operand_when_result_allocation_collects() {
    let _runtime = RuntimeGuard::start(Some("4096"));
    let mut frame = TestFrame::<1>::new();

    unsafe {
        lo_push_frame(frame.as_shadow_frame());
        frame.roots[0] = new_string("é".as_bytes());
        let old_string = frame.roots[0];
        leave_insufficient_nursery_space();

        let result = lo_string_repeat(frame.roots[0], 4);
        assert_ne!(frame.roots[0], old_string);
        assert_string(result, "éééé".as_bytes());
        lo_pop_frame();
    }
}

#[test]
fn reverse_roots_operand_when_result_allocation_collects() {
    let _runtime = RuntimeGuard::start(Some("4096"));
    let mut frame = TestFrame::<1>::new();

    unsafe {
        lo_push_frame(frame.as_shadow_frame());
        frame.roots[0] = new_string("Aé€😀".as_bytes());
        let old_string = frame.roots[0];
        leave_insufficient_nursery_space();

        let result = lo_string_reverse(frame.roots[0]);
        assert_ne!(frame.roots[0], old_string);
        assert_string(result, "😀€éA".as_bytes());
        lo_pop_frame();
    }
}

#[test]
fn negative_repeat_reports_count_and_exits_120() {
    let output = Command::new(env!("CARGO_BIN_EXE_lo_string_probe"))
        .arg("-7")
        .output()
        .expect("run lo_string_probe");

    assert_eq!(output.status.code(), Some(120));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "lo_string_repeat: negative count -7\n"
    );
}

/// Run the probe (`repeat("x", count)`) under `heap_size`, or the default heap.
fn run_repeat_probe(count: &str, heap_size: Option<&str>) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lo_string_probe"));
    command.arg(count);
    match heap_size {
        Some(size) => command.env("LO_HEAP_SIZE", size),
        None => command.env_remove("LO_HEAP_SIZE"),
    };
    command.output().expect("run lo_string_probe")
}

#[test]
fn string_larger_than_nursery_allocates_on_the_default_heap() {
    // 5 MB exceeds the 16 MiB heap's nursery; it must still be allocatable.
    let output = run_repeat_probe("5000000", None);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
}

#[test]
fn string_larger_than_nursery_allocates_on_a_small_heap() {
    let output = run_repeat_probe("20000", Some("65536"));
    assert_eq!(output.status.code(), Some(0), "{output:?}");
}

#[test]
fn string_larger_than_the_whole_heap_exits_137() {
    let output = run_repeat_probe("1000000", Some("65536"));
    assert_eq!(output.status.code(), Some(137));
    // Under `gc-debug` the failed major collection logs lines first; the abort
    // message is always the last line.
    assert!(
        String::from_utf8_lossy(&output.stderr).ends_with("lo_alloc: out of memory\n"),
        "{output:?}"
    );
}
