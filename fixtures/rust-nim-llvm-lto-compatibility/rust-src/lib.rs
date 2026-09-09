//! #11's "Rust/Nim 2/Nimony shared LLVM/LTO compatibility workload"
//! Core workload — the minimal first proof from `docs/research-program.md`
//! Track J: can Rust's own LLVM IR and Nim's own LLVM IR (via `nlvm`,
//! see `fixtures/direct-native-link/NOTES.md`) be merged into *one*
//! LLVM module with `llvm-link`, before either side reaches native
//! codegen — not merely linked as two already-native objects the way
//! every other fixture in this repo does.
//!
//! Deliberately minimal: a single function, no crate/Cargo machinery
//! (`rustc --emit=llvm-ir` on this file directly), matching the "first
//! proof should be minimal" principle from
//! `docs/rust-nim-native-linking.md`.

#[no_mangle]
pub extern "C" fn rust_add(a: i32, b: i32) -> i32 {
    a.wrapping_add(b)
}
