//! `wide-parallel-graph` fixture — independent leaf crate.

pub fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

pub fn lcm(a: u64, b: u64) -> u64 {
    a / gcd(a, b) * b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gcd_of_48_and_18() {
        assert_eq!(gcd(48, 18), 6);
    }

    #[test]
    fn lcm_of_4_and_6() {
        assert_eq!(lcm(4, 6), 12);
    }
}
