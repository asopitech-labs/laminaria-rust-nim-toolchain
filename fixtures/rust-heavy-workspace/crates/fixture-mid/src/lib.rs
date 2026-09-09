//! `rust-heavy-workspace` fixture — middle layer, depends on `fixture-core`.

use fixture_core::{primes_up_to, Point};

pub struct Cluster {
    pub points: Vec<Point>,
}

impl Cluster {
    pub fn from_prime_grid(limit: u64) -> Self {
        let primes = primes_up_to(limit);
        let points = primes
            .iter()
            .zip(primes.iter().skip(1))
            .map(|(&a, &b)| Point::new(a as i64, b as i64))
            .collect();
        Self { points }
    }

    pub fn total_perimeter(&self) -> i64 {
        self.points
            .windows(2)
            .map(|w| w[0].manhattan_distance(&w[1]))
            .sum()
    }

    pub fn centroid(&self) -> Option<Point> {
        if self.points.is_empty() {
            return None;
        }
        let (sx, sy) = self
            .points
            .iter()
            .fold((0i64, 0i64), |(sx, sy), p| (sx + p.x, sy + p.y));
        let n = self.points.len() as i64;
        Some(Point::new(sx / n, sy / n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cluster_from_small_prime_grid() {
        let cluster = Cluster::from_prime_grid(20);
        assert!(!cluster.points.is_empty());
        assert!(cluster.total_perimeter() >= 0);
        assert!(cluster.centroid().is_some());
    }

    #[test]
    fn empty_cluster_has_no_centroid() {
        let cluster = Cluster { points: vec![] };
        assert_eq!(cluster.centroid(), None);
    }
}
