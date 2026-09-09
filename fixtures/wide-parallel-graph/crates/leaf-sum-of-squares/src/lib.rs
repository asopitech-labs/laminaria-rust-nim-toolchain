//! `wide-parallel-graph` fixture — independent leaf crate.

pub fn sum_of_squares(n: u64) -> u64 {
    (1..=n).map(|x| x * x).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sum_of_squares_up_to_3() {
        assert_eq!(sum_of_squares(3), 1 + 4 + 9);
    }
}
