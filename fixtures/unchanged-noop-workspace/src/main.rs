//! `unchanged-noop-workspace` fixture.
//!
//! #11's "unchanged/no-op workspace" Core workload. Deliberately the
//! smallest, simplest fixture in this directory: a single crate with one
//! real, unit-tested computation and no cross-crate or cross-language
//! structure to measure. Its purpose is not the computation itself but
//! the *scenario* built on top of it once a Run harness exists (#19,
//! #21): build once, then rebuild with the source completely unchanged,
//! and characterize the metadata/hash/I/O/process-launch overhead a
//! correct build system still pays on a full no-op — the "no-op runs
//! report metadata/hash/I/O/process-launch overhead" acceptance
//! criterion on this issue. Every other fixture in this directory has
//! real internal structure worth measuring; this one deliberately does
//! not, so a no-op rebuild's cost isn't confounded by graph shape.

const SEED: &str = "laminaria-unchanged-noop-workspace";

/// A small, real, deterministic checksum — enough to be genuine work, not
/// so much that its own cost competes with the no-op-rebuild cost this
/// fixture exists to isolate.
fn checksum(input: &str) -> u32 {
    input.bytes().fold(2_166_136_261u32, |acc, b| {
        (acc ^ b as u32).wrapping_mul(0x0100_0193)
    })
}

fn main() {
    let result = checksum(SEED);
    println!("checksum={result}");
    assert_eq!(
        result, EXPECTED,
        "checksum drifted from the committed reference value"
    );
}

const EXPECTED: u32 = 0x81D0_3D1E;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_known_value() {
        assert_eq!(checksum(SEED), EXPECTED);
    }

    #[test]
    fn checksum_is_deterministic() {
        assert_eq!(checksum(SEED), checksum(SEED));
    }
}
