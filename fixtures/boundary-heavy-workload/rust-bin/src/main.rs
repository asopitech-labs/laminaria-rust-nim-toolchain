//! `boundary-heavy-workload` fixture — Rust side.
//!
//! Crosses into Nim once per loop iteration over a large, fixed iteration
//! count, with a deliberately trivial per-call payload (two `u32`s in,
//! one `u32` out) and trivial per-call computation. The point is that the
//! *number* of boundary crossings dominates the workload's cost, not data
//! volume or per-call work — unlike `mixed-rust-nim-executable`'s handful
//! of calls over a whole array.
//!
//! Runs the identical fold step natively in pure Rust over the same
//! sequence as a same-logic comparison point: the two final accumulators
//! must match exactly, and a future Run/scenario harness (#19) can
//! compare the two loops' wall time to isolate boundary-crossing overhead
//! from the computation itself.

extern "C" {
    fn nim_fold_step(acc: u32, x: u32) -> u32;
}

const ITERATIONS: u32 = 1_000_000;
const FNV_OFFSET_BASIS: u32 = 2_166_136_261;
const EXPECTED_FINAL: u32 = 636_658_098;

/// Same FNV-1a-style fold step as `nim-lib/nimlib.nim`'s `nim_fold_step`,
/// in pure Rust — the native-only comparison point for this workload.
fn rust_fold_step(acc: u32, x: u32) -> u32 {
    let mut h = acc ^ x;
    h = h.wrapping_mul(0x0100_0193);
    h ^= h >> 15;
    h
}

/// Deterministic per-iteration input (Knuth multiplicative hash).
fn gen_x(i: u32) -> u32 {
    i.wrapping_mul(2_654_435_761)
}

fn main() {
    let mut rust_acc = FNV_OFFSET_BASIS;
    let mut nim_acc = FNV_OFFSET_BASIS;

    for i in 0..ITERATIONS {
        let x = gen_x(i);
        rust_acc = rust_fold_step(rust_acc, x);
        nim_acc = unsafe { nim_fold_step(nim_acc, x) };
    }

    println!("iterations={ITERATIONS}");
    println!("rust_acc={rust_acc}");
    println!("nim_acc={nim_acc}");

    assert_eq!(
        rust_acc, EXPECTED_FINAL,
        "pure-Rust fold drifted from the committed reference value"
    );
    assert_eq!(
        nim_acc, EXPECTED_FINAL,
        "boundary-crossing fold drifted from the pure-Rust comparison (or the reference value)"
    );
}
