//! Stage 5 of the deep-critical-path-graph fixture: Collatz step-count fold.

pub fn transform(x: u64) -> u64 {
    let mut n = (x % 100_000).max(1);
    let mut steps: u64 = 0;
    while n != 1 {
        if n.is_multiple_of(2) {
            n /= 2;
        } else {
            n = 3 * n + 1;
        }
        steps += 1;
    }
    x.wrapping_add(steps)
}

pub fn run(seed: u64) -> u64 {
    transform(stage_04::run(seed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_known_value() {
        assert_eq!(transform(42), 50);
    }
}
