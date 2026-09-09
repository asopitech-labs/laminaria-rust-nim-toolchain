//! `rust-heavy-workspace` fixture — binary crate tying `fixture-core` and
//! `fixture-mid` together into one buildable, runnable artifact.

use fixture_core::sum_generic;
use fixture_mid::Cluster;

fn main() {
    let cluster = Cluster::from_prime_grid(200);
    let perimeter = cluster.total_perimeter();
    let centroid = cluster.centroid();
    let xs: Vec<i64> = cluster.points.iter().map(|p| p.x).collect();
    let sum_x = sum_generic(&xs);

    println!("points={}", cluster.points.len());
    println!("perimeter={perimeter}");
    println!("centroid={centroid:?}");
    println!("sum_x={sum_x}");
}
