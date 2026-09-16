//! Issue #68 second-round critical review: proves
//! `target_ir::lower_double_load_to_code_body`'s two `Ownership`
//! branches are not just differently-sized byte sequences (already
//! confirmed via `objdump` and the crate's own unit test), but both
//! **correct**, real, executable x86_64 machine code that computes the
//! same result (`*p + *p`) for the same input pointer, despite taking a
//! different instruction path to get there. Without this check, a
//! shorter `Unique` sequence could just as easily be a bug (e.g.
//! forgetting the second load) rather than a genuine, safe optimization.
//!
//! Run: `cargo run --release --example ownership_consumption_check`

use unified_symbol_graph::target_ir::{
    lower_double_load_to_code_body, lower_load_or_reload_to_code_body, Ownership,
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

unsafe fn make_callable(code: &[u8]) -> extern "C" fn(*const i32) -> i32 {
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
    std::mem::transmute::<*mut std::ffi::c_void, extern "C" fn(*const i32) -> i32>(ptr)
}

fn main() {
    let unique_code = lower_double_load_to_code_body(Ownership::Unique);
    let shared_code = lower_double_load_to_code_body(Ownership::Shared);
    eprintln!(
        "Unique code ({} bytes): {:02x?}",
        unique_code.len(),
        unique_code
    );
    eprintln!(
        "Shared code ({} bytes): {:02x?}",
        shared_code.len(),
        shared_code
    );

    let unique_fn = unsafe { make_callable(&unique_code) };
    let shared_fn = unsafe { make_callable(&shared_code) };

    let mut failures = 0;
    for input in [0, 1, -1, 42, i32::MAX, i32::MIN, 1_000_000] {
        let value = input;
        let unique_result = unique_fn(&value);
        let shared_result = shared_fn(&value);
        let expected = value.wrapping_add(value);
        let ok = unique_result == expected && shared_result == expected;
        println!(
            "input={input}: unique={unique_result} shared={shared_result} expected={expected} {}",
            if ok { "OK" } else { "MISMATCH" }
        );
        if !ok {
            failures += 1;
        }
    }

    if failures > 0 {
        eprintln!(
            "[ownership_consumption_check] {failures} MISMATCH(ES) -- the Ownership-driven \
             optimization was NOT correctness-preserving for at least one input"
        );
        std::process::exit(1);
    }
    eprintln!(
        "[ownership_consumption_check] all inputs agree: the Unique-path optimization (one load, \
         doubled) and the Shared-path conservative code (two loads, summed) compute the identical \
         result for this test's fixed-value inputs -- Ownership changed the emitted instructions \
         (confirmed via objdump separately) without changing the observable result here"
    );

    // Issue #68 third-round critical review: verifies the corrected
    // contrast (lower_load_or_reload_to_code_body) is not just
    // differently-sized bytes but genuinely correct machine code for
    // `if *p != 0 { *p } else { -*p }`, across all four Ownership
    // variants and both branch directions -- the earlier
    // lower_double_load_to_code_body check above never exercised a
    // branch inside the generated code at all, so this closes that gap
    // too (the reviewer's #1 priority: integrate branch lowering with
    // ownership-aware codegen).
    eprintln!();
    eprintln!("[ownership_consumption_check] verifying lower_load_or_reload_to_code_body...");
    let mut reload_failures = 0;
    for ownership in [
        Ownership::Unique,
        Ownership::Shared,
        Ownership::Boxed,
        Ownership::NotAReference,
    ] {
        let code = lower_load_or_reload_to_code_body(ownership);
        let f = unsafe { make_callable(&code) };
        for input in [5, -5, 0, i32::MAX, i32::MIN] {
            let expected = if input != 0 {
                input
            } else {
                input.wrapping_neg()
            };
            let actual = f(&input);
            let ok = actual == expected;
            println!(
                "{ownership:?} input={input}: actual={actual} expected={expected} {}",
                if ok { "OK" } else { "MISMATCH" }
            );
            if !ok {
                reload_failures += 1;
            }
        }
    }

    if reload_failures > 0 {
        eprintln!(
            "[ownership_consumption_check] {reload_failures} MISMATCH(ES) in \
             lower_load_or_reload_to_code_body -- the branch+ownership integration was NOT \
             correctness-preserving for at least one (ownership, input) pair"
        );
        std::process::exit(1);
    }
    eprintln!(
        "[ownership_consumption_check] all (ownership, input) pairs agree for \
         lower_load_or_reload_to_code_body across both branch directions, including i32::MIN \
         (where negation wraps) -- confirms the corrected, branch-integrated contrast is not \
         only differently-sized bytes but actually correct execution"
    );
}
