//! Issue #68 follow-up ("文字列、配列、ハッシュマップの操作全般についての検証は？"):
//! checking `HashMap::get`'s real MIR found its only genuinely new
//! structure (beyond what this crate already handles) is the `Call`
//! terminator -- a real x86_64 `call rel32` instruction. This example
//! proves `target_ir::lower_call_and_increment_to_code_body` produces a
//! real, correctly-patched, correctly-executing call by routing it
//! through this crate's own established `SharedSymbolGraph`/
//! `ElfX86_64PendingReloc`/`apply_elf_x86_64_relocations` pipeline
//! (issue #67) -- the same relocation-patching machinery that already
//! resolves cross-realm FFI calls -- rather than inventing a separate,
//! parallel mechanism just for this proof of concept.
//!
//! Run: `cargo run --release --example call_relocation_check`

use unified_symbol_graph::target_ir::lower_call_and_increment_to_code_body;
use unified_symbol_graph::{
    AddressState, CodeBody, Realm, SharedSymbolGraph, SymbolId, SymbolNode,
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

/// A real external function this crate has no hand in generating -- a
/// genuinely separate compiled function (`extern "C"`, so the real
/// System V calling convention applies) that the generated `call rel32`
/// must actually reach, not a value substituted by this test.
extern "C" fn triple(x: i32) -> i32 {
    x * 3
}

fn main() {
    // `apply_elf_x86_64_relocations` computes offsets within one
    // *conceptual, contiguous image* (lib.rs's own documented scope for
    // `assign_layout`) -- for this call-relocation shape to actually
    // execute, the caller's own patched bytes and the callee's own bytes
    // must genuinely occupy that same contiguous image, at those exact
    // relative offsets, in real memory. This example builds that: a
    // trampoline (`mov rax, <triple's real absolute address>; jmp rax`,
    // 12 bytes) stands in for the callee's own "real machine code" the
    // way `lib.rs`'s own `cpp_max_i32`/`nim_double` stand-in bodies do
    // for *their* callees -- the trampoline's job is only to jump to the
    // real, separately-compiled `triple` function, so this example never
    // needs to fabricate `triple`'s own real function body by hand.
    // Issue #68 diagnostic finding: an earlier draft of this example
    // placed the trampoline immediately after the caller with no padding,
    // which made the caller-to-callee PC-relative displacement
    // coincidentally equal exactly 0 -- `call rel32 = 0` targets the very
    // next instruction (a real no-op call-to-fallthrough), not the
    // trampoline, so the "call" silently never executed at all while
    // still returning *something* (whatever was in `eax` from a prior
    // call in the process), producing plausible-looking but wrong
    // results. A separate padding symbol placed between the caller and
    // the trampoline (below) makes this class of coincidental-zero-
    // displacement bug structurally impossible to hit by chance again,
    // and forces the relocation to do real work to reach the trampoline.
    let triple_addr = triple as *const () as usize as u64;
    let mut trampoline_code = vec![0x48, 0xB8]; // mov rax, imm64
    trampoline_code.extend_from_slice(&triple_addr.to_le_bytes());
    trampoline_code.extend_from_slice(&[0xFF, 0xE0]); // jmp rax

    let graph = SharedSymbolGraph::new();

    // A deliberate padding symbol, sorted (by SymbolId: Realm then name)
    // between the caller (Realm::Cargo) and the trampoline (Realm::C) --
    // see the diagnostic comment above for why a nonzero gap here matters.
    // `Realm::Nimble` sorts between `Cargo` and `C` (declaration order in
    // lib.rs's own `Realm` enum: Cargo, Nimble, C, Cpp).
    graph
        .declare_symbol(
            Realm::Nimble,
            SymbolNode {
                id: SymbolId {
                    realm: Realm::Nimble,
                    name: "padding".to_string(),
                },
                address: AddressState::Committed(CodeBody {
                    code: vec![0x90; 256], // NOP padding, never executed
                    relocations: vec![],
                }),
            },
        )
        .expect("declaring the padding symbol must succeed");

    let callee_id = SymbolId {
        realm: Realm::C,
        name: "triple".to_string(),
    };
    graph
        .declare_symbol(
            Realm::C,
            SymbolNode {
                id: callee_id.clone(),
                address: AddressState::Committed(CodeBody {
                    code: trampoline_code,
                    relocations: vec![],
                }),
            },
        )
        .expect("declaring the trampoline symbol must succeed");

    let caller_body = lower_call_and_increment_to_code_body(callee_id.clone());
    eprintln!(
        "[call_relocation_check] caller CodeBody: {} bytes, {} pending relocation(s): {:02x?}",
        caller_body.code.len(),
        caller_body.relocations.len(),
        caller_body.code
    );
    let caller_id = SymbolId {
        realm: Realm::Cargo,
        name: "caller".to_string(),
    };
    graph
        .declare_symbol(
            Realm::Cargo,
            SymbolNode {
                id: caller_id.clone(),
                address: AddressState::Committed(caller_body),
            },
        )
        .expect("declaring the caller symbol must succeed");

    let layout = graph.assign_layout();
    eprintln!(
        "[call_relocation_check] layout offsets: caller={}, callee={}",
        layout.addresses[&caller_id], layout.addresses[&callee_id]
    );
    let patched = graph
        .apply_elf_x86_64_relocations(&layout)
        .expect("the callee was declared, so the relocation must resolve");
    eprintln!(
        "[call_relocation_check] patched caller bytes: {:02x?}",
        patched.code[&caller_id]
    );
    eprintln!(
        "[call_relocation_check] patched callee (trampoline) bytes: {:02x?}",
        patched.code[&callee_id]
    );

    // Realize the conceptual image as a real, contiguous mmap region:
    // every symbol's patched bytes go at exactly the byte offset
    // `assign_layout` assigned it, so the relative displacement
    // `apply_elf_x86_64_relocations` computed (measured within that same
    // conceptual image) is also correct once this buffer is placed at
    // some real base address -- `call rel32`/PC-relative addressing only
    // cares about the *relative* distance, which this layout preserves
    // regardless of where the whole buffer ends up in the real address
    // space.
    let image_len = layout
        .addresses
        .iter()
        .map(|(id, &off)| off as usize + patched.code[id].len())
        .max()
        .expect("both symbols were declared, so at least one offset exists");

    let page_size = 4096;
    let mapped_len = image_len.div_ceil(page_size) * page_size;
    let base = unsafe {
        mmap(
            std::ptr::null_mut(),
            mapped_len,
            PROT_READ | PROT_WRITE | PROT_EXEC,
            MAP_PRIVATE | MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    assert_ne!(base, MAP_FAILED, "mmap failed");
    for (id, &offset) in &layout.addresses {
        let bytes = &patched.code[id];
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                (base as *mut u8).add(offset as usize),
                bytes.len(),
            );
        }
    }

    let caller_offset = layout.addresses[&caller_id] as usize;
    let caller_fn: extern "C" fn(i32) -> i32 = unsafe {
        std::mem::transmute::<*mut std::ffi::c_void, extern "C" fn(i32) -> i32>(
            (base as *mut u8).add(caller_offset) as *mut std::ffi::c_void,
        )
    };

    let mut failures = 0;
    for input in [1, 0, -1, 7, 100] {
        let actual = caller_fn(input);
        let expected = triple(input).wrapping_add(1);
        let ok = actual == expected;
        println!(
            "input={input}: actual={actual} expected={expected} {}",
            if ok { "OK" } else { "MISMATCH" }
        );
        if !ok {
            failures += 1;
        }
    }

    if failures > 0 {
        eprintln!("[call_relocation_check] {failures} MISMATCH(ES)");
        std::process::exit(1);
    }
    eprintln!(
        "[call_relocation_check] the caller's own generated `call rel32`, patched by this \
         crate's own issue #67 apply_elf_x86_64_relocations pipeline and placed in real memory \
         at the exact relative offsets assign_layout computed, genuinely reached and executed a \
         real, separately-compiled function (triple, via a jmp trampoline standing in for its \
         own real machine code) and correctly used its return value -- the Call terminator this \
         session's HashMap::get investigation identified as genuinely new is now modeled end to \
         end, real execution included"
    );
}
