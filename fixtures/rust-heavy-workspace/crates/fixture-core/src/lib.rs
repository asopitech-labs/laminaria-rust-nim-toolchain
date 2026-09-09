//! `rust-heavy-workspace` fixture — lowest layer.
//!
//! This crate exists to be *built*, not to demonstrate design. It is one of
//! #11's "Core workloads" reference fixtures consumed by the LAMINARIA
//! measurement spine (issues #18-21): a small but real multi-crate Rust
//! dependency chain a Run can be captured against.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: i64,
    pub y: i64,
}

impl Point {
    pub fn new(x: i64, y: i64) -> Self {
        Self { x, y }
    }

    pub fn manhattan_distance(&self, other: &Point) -> i64 {
        (self.x - other.x).abs() + (self.y - other.y).abs()
    }
}

pub fn is_prime(n: u64) -> bool {
    if n < 2 {
        return false;
    }
    let mut i = 2u64;
    while i * i <= n {
        if n % i == 0 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn primes_up_to(limit: u64) -> Vec<u64> {
    (2..=limit).filter(|&n| is_prime(n)).collect()
}

pub fn sum_generic<T>(items: &[T]) -> T
where
    T: Copy + std::ops::Add<Output = T> + Default,
{
    items.iter().fold(T::default(), |acc, &x| acc + x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manhattan_distance_is_symmetric() {
        let a = Point::new(0, 0);
        let b = Point::new(3, 4);
        assert_eq!(a.manhattan_distance(&b), 7);
        assert_eq!(b.manhattan_distance(&a), 7);
    }

    #[test]
    fn primes_up_to_20() {
        assert_eq!(primes_up_to(20), vec![2, 3, 5, 7, 11, 13, 17, 19]);
    }

    #[test]
    fn sum_generic_over_ints() {
        assert_eq!(sum_generic(&[1, 2, 3, 4]), 10);
    }
}
