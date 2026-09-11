//! `leaf-a` of the long-chain-wide-branches fixture (issue #28 D1-b2,
//! M7): `gcd_u64` via real Euclidean algorithm (spec.md 2.2). Takes the
//! branch point's already-computed output as a plain parameter -- the
//! aggregator crate for each scale owns the actual Cargo dependency on
//! the branch-point stage and calls it once, threading the value into
//! every leaf; leaf-a itself stays a pure, scale-independent function.
//! `offset` is `97` by default (small/medium) and `97 + i` for the
//! large scale's `leaf-03..08` selection rule (spec.md 2.2).

pub fn run(input: u64, offset: u64) -> u64 {
    gcd_u64(input, offset)
}

fn gcd_u64(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gcd_of_coprime_values_is_one() {
        assert_eq!(gcd_u64(17, 97), 1);
    }

    #[test]
    fn gcd_matches_a_known_shared_factor() {
        assert_eq!(gcd_u64(48, 18), 6);
    }

    #[test]
    fn run_uses_the_given_offset() {
        assert_eq!(run(48, 18), 6);
    }
}
