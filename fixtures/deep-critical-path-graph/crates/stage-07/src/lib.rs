//! Stage 7 of the deep-critical-path-graph fixture: byte-order XOR fold.

pub fn transform(x: u64) -> u64 {
    x ^ x.swap_bytes()
}

pub fn run(seed: u64) -> u64 {
    transform(stage_06::run(seed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_known_value() {
        assert_eq!(transform(42), 3026418949592973354);
    }
}
