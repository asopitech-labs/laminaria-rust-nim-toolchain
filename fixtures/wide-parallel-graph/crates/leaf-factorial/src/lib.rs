//! `wide-parallel-graph` fixture — independent leaf crate.

pub fn factorial(n: u64) -> u64 {
    (1..=n).product::<u64>().max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factorial_5() {
        assert_eq!(factorial(5), 120);
    }

    #[test]
    fn factorial_0_is_1() {
        assert_eq!(factorial(0), 1);
    }
}
