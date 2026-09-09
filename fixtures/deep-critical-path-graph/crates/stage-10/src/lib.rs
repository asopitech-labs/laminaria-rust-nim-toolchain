//! Stage 10 of the deep-critical-path-graph fixture: popcount-driven right rotate.

pub fn transform(x: u64) -> u64 {
    let p = x.count_ones();
    x.rotate_right(p)
}

pub fn run(seed: u64) -> u64 {
    transform(stage_09::run(seed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_known_value() {
        assert_eq!(transform(42), 4611686018427387909);
    }
}
