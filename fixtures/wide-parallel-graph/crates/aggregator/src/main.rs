//! `wide-parallel-graph` fixture — binary tying together 8 mutually
//! independent leaf crates. Unlike `rust-heavy-workspace`'s linear
//! dependency chain (core -> mid -> bin), none of the 8 leaves here depend
//! on each other, so a scheduler with a correct build graph can compile all
//! 8 in parallel before this crate links them.

fn main() {
    let fib = leaf_fibonacci::fibonacci(10);
    let fact = leaf_factorial::factorial(5);
    let gcd = leaf_gcd::gcd(48, 18);
    let lcm = leaf_gcd::lcm(4, 6);
    let sq = leaf_sum_of_squares::sum_of_squares(10);
    let pal = leaf_palindrome::is_palindrome("racecar");

    let mut v = vec![5, 3, 8, 1, 9, 2];
    leaf_bubble_sort::bubble_sort(&mut v);

    let found = leaf_binary_search::binary_search(&v, 8);
    let matrix = leaf_matrix_sum::matrix_sum(4, 5);

    println!("fibonacci(10)={fib}");
    println!("factorial(5)={fact}");
    println!("gcd(48,18)={gcd}");
    println!("lcm(4,6)={lcm}");
    println!("sum_of_squares(10)={sq}");
    println!("is_palindrome(racecar)={pal}");
    println!("sorted={v:?}");
    println!("binary_search(8)={found:?}");
    println!("matrix_sum(4,5)={matrix}");
}
