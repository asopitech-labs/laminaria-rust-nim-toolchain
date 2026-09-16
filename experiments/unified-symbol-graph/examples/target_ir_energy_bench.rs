//! Issue #68 follow-up: mmaps `target_ir::lower_target_ir_to_code_body`'s
//! own generated x86_64 bytes as real executable memory, calls them with
//! the actual System V calling convention (param0 in `edi`, result in
//! `eax`), and times that against a real LLVM-compiled, `-O`-optimized,
//! `#[inline(never)]` Rust function performing the identical branch --
//! for the user's own requested "省エネ" (runtime-execution efficiency)
//! comparison against LLVM IR.
//!
//! Run: `cargo run --release --example target_ir_energy_bench -- <N>`
//!
//! This is not a claim that this proof of concept beats LLVM in general
//! -- see this file's own printed caveat about LLVM eliminating the
//! branch entirely (`sete`+`lea`, no `cmp`/`je` at all) for this trivial
//! function, which this crate's own `lower_target_ir_to_code_body` does
//! not attempt (it has no optimization passes; see the design doc,
//! section 6). The comparison is reported honestly either way.

use std::hint::black_box;
use unified_symbol_graph::target_ir::{lower_target_ir_to_code_body, mir_text};

// Deliberately no `libc` dependency -- this crate's own precedent
// (`disk_tiering.rs`'s "no serde" stance, confirmed elsewhere in this
// crate) is to avoid pulling in an external crate for a handful of raw
// syscalls this proof of concept can declare directly. `mmap`'s real
// Linux x86_64 signature (`man 2 mmap`), declared here rather than
// imported.
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

const REAL_NE_BRANCH_MIR: &str = "\
fn branch(_1: i32) -> i32 {
    debug param0 => _1;
    let mut _0: i32;
    let mut _2: bool;

    bb0: {
        _2 = Ne(copy _1, const 0_i32);
        switchInt(move _2) -> [0: bb2, otherwise: bb1];
    }

    bb1: {
        _0 = const 7_i32;
        goto -> bb3;
    }

    bb2: {
        _0 = const 9_i32;
        goto -> bb3;
    }

    bb3: {
        return;
    }
}
";

/// The identical function LLVM compiles for comparison, `#[inline(never)]`
/// so a real `call` boundary survives `-O` (confirmed via `objdump` in
/// this session: LLVM emits `xor eax,eax; test edi,edi; sete al; lea
/// eax,[rax*2+7]; ret` -- a branchless form, not `cmp`/`je` at all).
#[inline(never)]
fn llvm_branch(param0: i32) -> i32 {
    if param0 != 0 {
        7
    } else {
        9
    }
}

/// mmaps `code` as RWX (proof-of-concept only -- a real implementation
/// would mmap RW, write, then mprotect to RX; this benchmark keeps RWX
/// throughout for simplicity since it never treats the page as
/// untrusted input) and returns a callable function pointer with the
/// System V `i32 -> i32` calling convention this crate's own
/// `lower_target_ir_to_code_body` targets.
unsafe fn make_callable(code: &[u8]) -> extern "C" fn(i32) -> i32 {
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
    std::mem::transmute::<*mut std::ffi::c_void, extern "C" fn(i32) -> i32>(ptr)
}

fn main() {
    let n: i64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000_000);

    let ir = mir_text::parse_mir_text(REAL_NE_BRANCH_MIR).expect("real rustc MIR must parse");
    let code = lower_target_ir_to_code_body(&ir).expect("parsed IR must lower");
    eprintln!(
        "[energy_comparison] target_ir-generated machine code: {} bytes: {:02x?}",
        code.len(),
        code
    );

    let target_ir_fn = unsafe { make_callable(&code) };

    // Sanity check both functions agree on every branch outcome before
    // timing either -- a benchmark comparing two functions that disagree
    // on results would be meaningless.
    for v in [0, 1, -1, 42] {
        assert_eq!(
            target_ir_fn(v),
            llvm_branch(v),
            "target_ir-generated code and LLVM-compiled code must agree on branch({v})"
        );
    }
    eprintln!("[energy_comparison] correctness check passed: both implementations agree on all sampled inputs");

    let start = std::time::Instant::now();
    let mut acc: i64 = 0;
    for i in 0..n {
        acc = acc.wrapping_add(target_ir_fn(black_box((i % 2) as i32)) as i64);
    }
    let target_ir_elapsed = start.elapsed();
    println!(
        "target_ir (direct lowering, no optimizer): acc={acc} n={n} elapsed={target_ir_elapsed:?}"
    );

    let start = std::time::Instant::now();
    let mut acc: i64 = 0;
    for i in 0..n {
        acc = acc.wrapping_add(llvm_branch(black_box((i % 2) as i32)) as i64);
    }
    let llvm_elapsed = start.elapsed();
    println!("llvm_branch (rustc -O, #[inline(never)]): acc={acc} n={n} elapsed={llvm_elapsed:?}");

    eprintln!(
        "[energy_comparison] ratio (target_ir / llvm): {:.3}x -- LLVM's branchless codegen \
         (sete+lea, no cmp/je) is the expected reason for any LLVM speedup here, not a general \
         claim about this proof of concept's lowering mechanism itself",
        target_ir_elapsed.as_secs_f64() / llvm_elapsed.as_secs_f64()
    );
}
