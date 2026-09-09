//! `wide-parallel-graph` fixture — independent leaf crate.

pub fn binary_search(sorted: &[i64], target: i64) -> Option<usize> {
    let (mut lo, mut hi) = (0isize, sorted.len() as isize - 1);
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        match sorted[mid as usize].cmp(&target) {
            std::cmp::Ordering::Equal => return Some(mid as usize),
            std::cmp::Ordering::Less => lo = mid + 1,
            std::cmp::Ordering::Greater => hi = mid - 1,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_present_element() {
        assert_eq!(binary_search(&[1, 3, 5, 7, 9], 7), Some(3));
    }

    #[test]
    fn returns_none_for_absent_element() {
        assert_eq!(binary_search(&[1, 3, 5, 7, 9], 4), None);
    }
}
