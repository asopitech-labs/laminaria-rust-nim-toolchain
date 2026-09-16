//! Issue #68 follow-up ("文字列、配列、ハッシュマップの操作全般についての検証は？"):
//! mmaps `target_ir::lower_bounds_checked_slice_index_to_code_body`'s own
//! generated x86_64 bytes as real executable memory, calls it with the
//! real System V calling convention `(ptr: rdi, len: rsi, index: rdx) ->
//! eax`, and checks both the in-bounds case (must match real element
//! values) and the out-of-bounds case (must return the sentinel
//! `i32::MIN`, per this function's own documented, deliberately
//! simplified scope -- no real panic/unwind).
//!
//! Run: `cargo run --release --example bounds_check_check`

use unified_symbol_graph::target_ir::lower_bounds_checked_slice_index_to_code_body;

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

unsafe fn make_callable(code: &[u8]) -> extern "C" fn(*const i32, usize, usize) -> i32 {
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
    std::mem::transmute::<*mut std::ffi::c_void, extern "C" fn(*const i32, usize, usize) -> i32>(
        ptr,
    )
}

fn main() {
    let code = lower_bounds_checked_slice_index_to_code_body();
    eprintln!(
        "[bounds_check_check] generated {} bytes: {:02x?}",
        code.len(),
        code
    );

    let f = unsafe { make_callable(&code) };

    let data: Vec<i32> = vec![10, 20, 30, 40, 50];
    let mut failures = 0;

    // In-bounds: every valid index must return the real element.
    for i in 0..data.len() {
        let actual = f(data.as_ptr(), data.len(), i);
        let expected = data[i];
        let ok = actual == expected;
        println!(
            "in-bounds index={i}: actual={actual} expected={expected} {}",
            if ok { "OK" } else { "MISMATCH" }
        );
        if !ok {
            failures += 1;
        }
    }

    // Out-of-bounds: index == len and index > len must both return the
    // documented sentinel (i32::MIN), never read past the array (which
    // would be real undefined behavior if it did).
    for i in [data.len(), data.len() + 1, data.len() + 100] {
        let actual = f(data.as_ptr(), data.len(), i);
        let expected = i32::MIN;
        let ok = actual == expected;
        println!(
            "out-of-bounds index={i} (len={}): actual={actual} expected={expected} {}",
            data.len(),
            if ok { "OK" } else { "MISMATCH" }
        );
        if !ok {
            failures += 1;
        }
    }

    if failures > 0 {
        eprintln!("[bounds_check_check] {failures} MISMATCH(ES)");
        std::process::exit(1);
    }
    eprintln!(
        "[bounds_check_check] all in-bounds accesses returned the real element, all \
         out-of-bounds accesses returned the sentinel without touching memory past the array \
         -- the real MIR TerminatorKind::Assert shape (index < len, captured this session from \
         `pub fn get_elem(s: &[i32], i: usize) -> i32 {{ s[i] }}`) reached and changed generated \
         x86_64 code, verified both via objdump (separately) and real execution here"
    );
}
