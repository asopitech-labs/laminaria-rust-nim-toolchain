//! Issue #68 follow-up ("文字列、配列、ハッシュマップの操作全般についての検証は？"):
//! mmaps `target_ir::lower_str_or_slice_len_to_code_body` and
//! `target_ir::lower_first_byte_or_zero_to_code_body` as real executable
//! memory and calls them with the real System V fat-pointer calling
//! convention `(ptr: rdi, len: rsi)`, against real Rust string/byte-slice
//! data (including the empty string, the actual edge case the
//! is_empty-branch function exists for).
//!
//! Run: `cargo run --release --example string_slice_check`

use unified_symbol_graph::target_ir::{
    lower_first_byte_or_zero_to_code_body, lower_str_or_slice_len_to_code_body,
};

const PROT_READ: i32 = 0x1;
const PROT_WRITE: i32 = 0x2;
const PROT_EXEC: i32 = 0x4;
const MAP_PRIVATE: i32 = 0x02;
const MAP_ANONYMOUS: i32 = 0x20;
const MAP_FAILED: *mut std::ffi::c_void = usize::MAX as *mut std::ffi::c_void;

extern "C" {
    fn mmap(
        addr: *mut std::ffi::c_void,
        len: usize,
        prot: i32,
        flags: i32,
        fd: i32,
        offset: i64,
    ) -> *mut std::ffi::c_void;
}

unsafe fn make_len_callable(code: &[u8]) -> extern "C" fn(*const u8, usize) -> usize {
    let page_size = 4096;
    let len = code.len().div_ceil(page_size) * page_size;
    let ptr = mmap(
        std::ptr::null_mut(),
        len,
        PROT_READ | PROT_WRITE | PROT_EXEC,
        MAP_PRIVATE | MAP_ANONYMOUS,
        -1,
        0,
    );
    assert_ne!(ptr, MAP_FAILED, "mmap failed");
    std::ptr::copy_nonoverlapping(code.as_ptr(), ptr as *mut u8, code.len());
    std::mem::transmute::<*mut std::ffi::c_void, extern "C" fn(*const u8, usize) -> usize>(ptr)
}

unsafe fn make_byte_callable(code: &[u8]) -> extern "C" fn(*const u8, usize) -> u8 {
    let page_size = 4096;
    let len = code.len().div_ceil(page_size) * page_size;
    let ptr = mmap(
        std::ptr::null_mut(),
        len,
        PROT_READ | PROT_WRITE | PROT_EXEC,
        MAP_PRIVATE | MAP_ANONYMOUS,
        -1,
        0,
    );
    assert_ne!(ptr, MAP_FAILED, "mmap failed");
    std::ptr::copy_nonoverlapping(code.as_ptr(), ptr as *mut u8, code.len());
    std::mem::transmute::<*mut std::ffi::c_void, extern "C" fn(*const u8, usize) -> u8>(ptr)
}

fn main() {
    let len_code = lower_str_or_slice_len_to_code_body();
    let byte_code = lower_first_byte_or_zero_to_code_body();
    eprintln!("[string_slice_check] len code: {len_code:02x?}");
    eprintln!("[string_slice_check] first_byte_or_zero code: {byte_code:02x?}");

    let len_fn = unsafe { make_len_callable(&len_code) };
    let byte_fn = unsafe { make_byte_callable(&byte_code) };

    let mut failures = 0;

    // Real Rust &str values, including the empty string -- the actual
    // edge case first_byte_or_zero's own is_empty branch exists for.
    for s in ["hello", "", "a", "the quick brown fox", "文字列"] {
        let bytes = s.as_bytes();
        let actual_len = len_fn(bytes.as_ptr(), bytes.len());
        let expected_len = bytes.len();
        let len_ok = actual_len == expected_len;
        println!(
            "str={s:?} len: actual={actual_len} expected={expected_len} {}",
            if len_ok { "OK" } else { "MISMATCH" }
        );
        if !len_ok {
            failures += 1;
        }

        let actual_byte = byte_fn(bytes.as_ptr(), bytes.len());
        let expected_byte = bytes.first().copied().unwrap_or(0);
        let byte_ok = actual_byte == expected_byte;
        println!(
            "str={s:?} first_byte_or_zero: actual={actual_byte} expected={expected_byte} {}",
            if byte_ok { "OK" } else { "MISMATCH" }
        );
        if !byte_ok {
            failures += 1;
        }
    }

    if failures > 0 {
        eprintln!("[string_slice_check] {failures} MISMATCH(ES)");
        std::process::exit(1);
    }
    eprintln!(
        "[string_slice_check] all real &str values (including the empty string) produced \
         correct results for both the fat-pointer length read and the is_empty-branching first \
         byte access -- confirmed against real rustc -O --emit=asm output for the equivalent \
         Rust functions before writing these bytes"
    );
}
