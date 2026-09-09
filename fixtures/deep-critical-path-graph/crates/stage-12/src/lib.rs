//! Stage 12 of the deep-critical-path-graph fixture: prime-modulus normalize with trailing-zero mix.

pub fn transform(x: u64) -> u64 {
    (x % 1_000_000_007).wrapping_add(x.trailing_zeros() as u64)
}

pub fn run(seed: u64) -> u64 {
    transform(stage_11::run(seed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_known_value() {
        assert_eq!(transform(42), 43);
    }
}
