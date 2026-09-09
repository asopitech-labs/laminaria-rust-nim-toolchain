//! Stage 3 of the deep-critical-path-graph fixture: bit reversal.

pub fn transform(x: u64) -> u64 {
    x.reverse_bits()
}

pub fn run(seed: u64) -> u64 {
    transform(stage_02::run(seed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_known_value() {
        assert_eq!(transform(42), 6052837899185946624);
    }
}
