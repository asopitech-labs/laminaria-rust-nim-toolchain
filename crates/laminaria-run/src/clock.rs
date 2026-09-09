//! One monotonic clock per Run (`docs/measurement-foundation.md` section 6):
//! every process/compiler event this crate records is a nanosecond offset
//! from a single `RunClock` anchor, never a wall-clock timestamp read a
//! second time from a different source -- that's what makes events from
//! different subsystems safely comparable/orderable later.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub struct RunClock {
    anchor: Instant,
    anchor_unix_ns: u128,
}

impl RunClock {
    /// Starts a new Run clock, anchored at the current instant. Also
    /// records the wall-clock time at that same instant, for
    /// human/log correlation only (`Run::run_started_at_unix_ns`) -- never
    /// used to compute any other timestamp in the Run.
    pub fn start() -> Self {
        Self {
            anchor: Instant::now(),
            anchor_unix_ns: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
        }
    }

    /// Nanoseconds elapsed since this clock's anchor.
    pub fn elapsed_ns(&self) -> u64 {
        self.anchor.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
    }

    pub fn anchor_unix_ns(&self) -> u128 {
        self.anchor_unix_ns
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_ns_is_monotonically_non_decreasing_across_calls() {
        let clock = RunClock::start();
        let mut previous = clock.elapsed_ns();
        for _ in 0..1000 {
            let current = clock.elapsed_ns();
            assert!(
                current >= previous,
                "elapsed_ns went backwards: {previous} -> {current}"
            );
            previous = current;
        }
    }

    #[test]
    fn anchor_unix_ns_is_a_plausible_recent_timestamp() {
        let clock = RunClock::start();
        // Sanity bound, not a precise check: this test suite runs after
        // 2024-01-01 and (barring a broken clock) before 2100-01-01.
        let year_2024_ns: u128 = 1_704_067_200_000_000_000;
        let year_2100_ns: u128 = 4_102_444_800_000_000_000;
        assert!(clock.anchor_unix_ns() > year_2024_ns);
        assert!(clock.anchor_unix_ns() < year_2100_ns);
    }
}
