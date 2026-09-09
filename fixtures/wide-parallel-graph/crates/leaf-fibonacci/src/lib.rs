//! `wide-parallel-graph` fixture — one of 8 mutually-independent leaf
//! crates. All 8 build with no dependencies on each other, only on
//! `aggregator`; this fixture's whole point is that fan-out, not any one
//! leaf's computation.

pub fn fibonacci(n: u64) -> u64 {
    let (mut a, mut b) = (0u64, 1u64);
    for _ in 0..n {
        let next = a + b;
        a = b;
        b = next;
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fibonacci_10() {
        assert_eq!(fibonacci(10), 55);
    }
}
