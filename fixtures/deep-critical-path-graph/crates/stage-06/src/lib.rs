//! Stage 6 of the deep-critical-path-graph fixture: population-count squared fold.

pub fn transform(x: u64) -> u64 {
    let p = x.count_ones() as u64;
    x.wrapping_add(p * p)
}

pub fn run(seed: u64) -> u64 {
    transform(stage_05::run(seed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_known_value() {
        assert_eq!(transform(42), 51);
    }
}
