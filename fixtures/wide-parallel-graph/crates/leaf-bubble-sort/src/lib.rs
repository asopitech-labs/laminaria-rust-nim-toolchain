//! `wide-parallel-graph` fixture — independent leaf crate.

pub fn bubble_sort(items: &mut [i64]) {
    let n = items.len();
    for i in 0..n {
        for j in 0..n.saturating_sub(i + 1) {
            if items[j] > items[j + 1] {
                items.swap(j, j + 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_a_small_vec() {
        let mut v = vec![5, 3, 8, 1, 9, 2];
        bubble_sort(&mut v);
        assert_eq!(v, vec![1, 2, 3, 5, 8, 9]);
    }
}
