//! Stage 11 of the deep-critical-path-graph fixture: checksum XOR / leading-zero mix.

pub fn transform(x: u64) -> u64 {
    (x ^ 0xA5A5_A5A5_A5A5_A5A5u64).wrapping_add(x.leading_zeros() as u64)
}

pub fn run(seed: u64) -> u64 {
    transform(stage_10::run(seed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_known_value() {
        assert_eq!(transform(42), 11936128518282651081);
    }
}
