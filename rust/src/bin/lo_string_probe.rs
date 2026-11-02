//! Subprocess probe for string-operation abort behavior. Not part of the
//! shipped runtime ABI.

fn main() {
    let count = std::env::args()
        .nth(1)
        .expect("repeat count argument")
        .parse::<i32>()
        .expect("repeat count must be an i32");

    lo_runtime::lo_runtime_init();
    unsafe {
        let string = lo_runtime::lo_string_new(b"x".as_ptr(), 1);
        let _ = lo_runtime::lo_string_repeat(string, count);
    }
    lo_runtime::lo_runtime_shutdown();
}
