//! `leaf-b` of the long-chain-wide-branches fixture (issue #28 D1-b2,
//! M7): `input.to_le_bytes()`'s 8 bytes, sorted ascending by a real
//! bubble sort, reinterpreted via `u64::from_le_bytes` (spec.md 2.2).

pub fn run(input: u64) -> u64 {
    let mut bytes = input.to_le_bytes();
    bubble_sort(&mut bytes);
    u64::from_le_bytes(bytes)
}

fn bubble_sort(bytes: &mut [u8; 8]) {
    let n = bytes.len();
    for i in 0..n {
        for j in 0..n.saturating_sub(i + 1) {
            if bytes[j] > bytes[j + 1] {
                bytes.swap(j, j + 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_bytes_ascending() {
        let mut b = [5, 3, 8, 1, 9, 2, 0, 255];
        bubble_sort(&mut b);
        assert_eq!(b, [0, 1, 2, 3, 5, 8, 9, 255]);
    }

    #[test]
    fn run_reinterprets_the_sorted_bytes_as_le_u64() {
        // to_le_bytes(0x0908...) has a known, hand-checkable sorted order.
        let input = 0x0102030405060708u64;
        let expected_bytes = {
            let mut b = input.to_le_bytes();
            bubble_sort(&mut b);
            b
        };
        assert_eq!(run(input), u64::from_le_bytes(expected_bytes));
    }
}
