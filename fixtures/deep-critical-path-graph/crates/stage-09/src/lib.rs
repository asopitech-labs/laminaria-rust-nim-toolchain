//! Stage 9 of the deep-critical-path-graph fixture: digital-root fold.

pub fn transform(x: u64) -> u64 {
    let mut n = x % 1_000_000;
    if n == 0 {
        return x;
    }
    while n >= 10 {
        n = n.to_string().bytes().map(|b| (b - b'0') as u64).sum();
    }
    x.wrapping_add(n)
}

pub fn run(seed: u64) -> u64 {
    transform(stage_08::run(seed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_known_value() {
        assert_eq!(transform(42), 48);
    }
}
