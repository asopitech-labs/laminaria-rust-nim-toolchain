//! Stage 2 of the deep-critical-path-graph fixture: golomb multiply and rotate.

pub fn transform(x: u64) -> u64 {
    x.wrapping_mul(0x9E3779B97F4A7C15).rotate_left(31)
}

pub fn run(seed: u64) -> u64 {
    transform(stage_01::run(seed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_known_value() {
        assert_eq!(transform(42), 8150439700282014775);
    }
}
