//! Stage 1 of the deep-critical-path-graph fixture: 64-bit avalanche mix (splitmix64 finalizer step).

pub fn transform(x: u64) -> u64 {
    let mut z = x;
    z ^= z >> 33;
    z = z.wrapping_mul(0xff51afd7ed558ccd);
    z
}

pub fn run(seed: u64) -> u64 {
    transform(seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_known_value() {
        assert_eq!(transform(42), 16386023356140951970);
    }
}
