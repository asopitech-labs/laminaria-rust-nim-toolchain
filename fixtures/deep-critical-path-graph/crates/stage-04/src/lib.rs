//! Stage 4 of the deep-critical-path-graph fixture: decimal digit-sum fold.

pub fn transform(x: u64) -> u64 {
    let digit_sum: u64 = x.to_string().bytes().map(|b| (b - b'0') as u64).sum();
    x.wrapping_add(digit_sum)
}

pub fn run(seed: u64) -> u64 {
    transform(stage_03::run(seed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_known_value() {
        assert_eq!(transform(42), 48);
    }
}
