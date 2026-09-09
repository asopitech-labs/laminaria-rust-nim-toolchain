//! `mixed-rust-nim-executable` fixture — Rust side.
//!
//! Unlike `rust-nim-c-abi-baseline` (deliberately minimal: Rust calls one
//! scalar-only Nim helper), this fixture has both languages contribute
//! real algorithmic work to the one final binary over a shared array
//! buffer: Rust generates the data and computes its own checksum, Nim
//! reads the same buffer for statistics and then mutates it in place, and
//! Rust re-checksums the mutated buffer — a realistic mixed-language
//! workload rather than a single one-shot cross-boundary call.

extern "C" {
    fn NimMain();
    fn nim_array_stats(
        data: *const i32,
        len: i32,
        out_sum: *mut i64,
        out_min: *mut i32,
        out_max: *mut i32,
        out_mean_x1000: *mut i64,
    );
    fn nim_array_scale_evens(data: *mut i32, len: i32, factor: i32);
}

const SEED: u32 = 20260909;
const COUNT: usize = 32;
const SCALE_FACTOR: i32 = 3;

const EXPECTED_CHECKSUM_BEFORE: i64 = 11146;
const EXPECTED_SUM: i64 = 15788;
const EXPECTED_MIN: i32 = 2;
const EXPECTED_MAX: i32 = 924;
const EXPECTED_MEAN_X1000: i64 = 493375;
const EXPECTED_CHECKSUM_AFTER: i64 = 50566;

/// Deterministic LCG data generator (pure Rust logic).
fn generate_data(n: usize, seed: u32) -> Vec<i32> {
    let mut state = seed;
    (0..n)
        .map(|_| {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            ((state >> 8) % 1000) as i32
        })
        .collect()
}

/// Position-weighted XOR fold (pure Rust logic).
fn rust_checksum(data: &[i32]) -> i64 {
    data.iter().enumerate().fold(0i64, |acc, (i, &v)| {
        acc ^ ((v as i64).wrapping_mul(i as i64 + 1))
    })
}

fn main() {
    // Required once before calling any Nim-compiled function: initializes
    // Nim's runtime globals (GC/exception bookkeeping).
    unsafe { NimMain() };

    let mut data = generate_data(COUNT, SEED);
    let checksum_before = rust_checksum(&data);

    let (sum, min, max, mean_x1000) = unsafe {
        let mut sum = 0i64;
        let mut min = 0i32;
        let mut max = 0i32;
        let mut mean_x1000 = 0i64;
        nim_array_stats(
            data.as_ptr(),
            data.len() as i32,
            &mut sum,
            &mut min,
            &mut max,
            &mut mean_x1000,
        );
        (sum, min, max, mean_x1000)
    };

    unsafe {
        nim_array_scale_evens(data.as_mut_ptr(), data.len() as i32, SCALE_FACTOR);
    }
    let checksum_after = rust_checksum(&data);

    println!("checksum_before={checksum_before}");
    println!("sum={sum} min={min} max={max} mean_x1000={mean_x1000}");
    println!("checksum_after={checksum_after}");

    assert_eq!(
        checksum_before, EXPECTED_CHECKSUM_BEFORE,
        "rust-side checksum (pre-mutation) drifted"
    );
    assert_eq!(sum, EXPECTED_SUM, "nim array sum drifted");
    assert_eq!(min, EXPECTED_MIN, "nim array min drifted");
    assert_eq!(max, EXPECTED_MAX, "nim array max drifted");
    assert_eq!(
        mean_x1000, EXPECTED_MEAN_X1000,
        "nim array mean*1000 drifted"
    );
    assert_eq!(
        checksum_after, EXPECTED_CHECKSUM_AFTER,
        "rust-side checksum (post nim mutation) drifted"
    );
}
