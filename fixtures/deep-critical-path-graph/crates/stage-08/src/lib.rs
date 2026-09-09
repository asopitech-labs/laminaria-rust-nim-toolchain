//! Stage 8 of the deep-critical-path-graph fixture: modular exponentiation fold (x^3 mod 1_000_003).

pub fn transform(x: u64) -> u64 {
    let base = (x % 97) + 1;
    let modulus: u64 = 1_000_003;
    let mut result: u64 = 1;
    let mut b = base % modulus;
    let mut e: u64 = 3;
    while e > 0 {
        if e & 1 == 1 {
            result = (result * b) % modulus;
        }
        b = (b * b) % modulus;
        e >>= 1;
    }
    x.wrapping_add(result)
}

pub fn run(seed: u64) -> u64 {
    transform(stage_07::run(seed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_known_value() {
        assert_eq!(transform(42), 79549);
    }
}
