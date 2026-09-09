//! Semantic contract, see ../CONTRACT.md. Real Rust implementation of the
//! workload the candidate representation (../substrate/) is cross-checked
//! against. Prints "a,b,use_double,result" for a fixed set of test inputs,
//! matching the format ../substrate's reference evaluator and LLVM-IR
//! projection also print, so ../trace.sh can diff them byte-for-byte.

fn double(x: i32) -> i32 {
    x.wrapping_add(x)
}

fn add_or_double(a: i32, b: i32, use_double: i32) -> i32 {
    if use_double != 0 {
        double(a)
    } else {
        a.wrapping_add(b)
    }
}

const TEST_INPUTS: &[(i32, i32, i32)] = &[
    (3, 4, 0),
    (3, 4, 1),
    (i32::MAX, 1, 0),
    (-5, 10, 1),
];

fn main() {
    for &(a, b, use_double) in TEST_INPUTS {
        println!("{a},{b},{use_double},{}", add_or_double(a, b, use_double));
    }
}
