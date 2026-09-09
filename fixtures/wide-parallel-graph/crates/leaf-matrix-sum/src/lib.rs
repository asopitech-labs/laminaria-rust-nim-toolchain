//! `wide-parallel-graph` fixture — independent leaf crate.

pub fn matrix_sum(rows: usize, cols: usize) -> u64 {
    let mut total = 0u64;
    for r in 0..rows {
        for c in 0..cols {
            total += (r * cols + c) as u64;
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_a_2x3_matrix() {
        // values: 0 1 2 / 3 4 5
        assert_eq!(matrix_sum(2, 3), 15);
    }
}
