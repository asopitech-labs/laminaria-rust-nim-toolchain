//! `leaf-matrix-sum` of the long-chain-wide-branches fixture (issue #28
//! D1-b2, M7): builds an `n`x`n` matrix where `matrix[i][j] =
//! (input + i) * (j + 1)` (both wrapping) and sums every element
//! (wrapping), spec.md 2.2. Deliberately split into 4 auxiliary
//! functions ("row generation," "cell compute," "row sum," "total sum")
//! -- the source-volume axis (axis 1) this case's own scale config
//! varies independently of the payload-volume axis (axis 2, `n` itself).

pub fn run(input: u64, n: u64) -> u64 {
    total_sum(input, n)
}

fn total_sum(input: u64, n: u64) -> u64 {
    let mut total = 0u64;
    for i in 0..n {
        let row = generate_row(input, i, n);
        total = total.wrapping_add(row_sum(&row));
    }
    total
}

fn generate_row(input: u64, i: u64, n: u64) -> Vec<u64> {
    (0..n).map(|j| compute_cell(input, i, j)).collect()
}

fn compute_cell(input: u64, i: u64, j: u64) -> u64 {
    input.wrapping_add(i).wrapping_mul(j.wrapping_add(1))
}

fn row_sum(row: &[u64]) -> u64 {
    row.iter().fold(0u64, |acc, &cell| acc.wrapping_add(cell))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_cell_matches_the_closed_formula() {
        assert_eq!(compute_cell(10, 2, 3), (10u64 + 2).wrapping_mul(3 + 1));
    }

    #[test]
    fn run_matches_a_hand_summed_2x2_matrix() {
        // input=10, n=2: cells (i,j) in {0,1}x{0,1}:
        // (10+0)*1=10, (10+0)*2=20, (10+1)*1=11, (10+1)*2=22 -> sum=63
        assert_eq!(run(10, 2), 63);
    }
}
