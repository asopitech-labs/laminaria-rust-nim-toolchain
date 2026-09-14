//! Issue #48 (G1) Checkpoint D verification-gate item 6 ("process-tree
//! observation"): direct, real evidence that no process named `nimble`
//! ever appears anywhere in the subprocess tree a real production G1
//! ingestion run spawns.
//!
//! A `#[test]` cannot safely do this itself: proving "no `nimble`
//! anywhere in this run's own subprocess tree" requires shadowing
//! `nimble` on `PATH` for this process's own children, and Rust's
//! `std::env::set_var` is unsound to call from inside `cargo test`'s
//! shared, multi-threaded test process (other tests may read the
//! environment concurrently) -- the exact hazard
//! `crate::command_runner`'s own `with_forced_offline_environment` doc
//! comment already documents for the proxy-variable case. A small,
//! single-purpose, freshly spawned process sidesteps that hazard
//! entirely: nothing else in *this* process ever reads or writes its
//! environment concurrently, so calling `set_var` once, first thing in
//! `main`, before any other code runs, is sound.
//!
//! Takes one argument, a directory already containing a shadow
//! `nimble` executable that -- if ever invoked -- creates a sentinel
//! file at `<dir>/.nimble_was_invoked`. Prepends that directory to
//! `PATH`, then runs the real production
//! [`laminaria_run::cross_ecosystem_ingest::ingest_fixture_input`]
//! entry point end-to-end through [`laminaria_run::command_runner::RealCommandRunner`]
//! (never the test-only `RecordingCommandRunner`), and reports the
//! ingestion result on stdout. The calling test then checks the
//! sentinel file itself, not anything this process claims about its
//! own behavior.

use laminaria_run::command_runner::RealCommandRunner;
use laminaria_run::cross_ecosystem_ingest::{ingest_fixture_input, FixtureLayout};

fn main() {
    let shadow_dir = std::env::args()
        .nth(1)
        .expect("usage: laminaria-nimble-shadow-probe <shadow_bin_dir>");

    // SAFETY: this is the first line of `main` in a single-purpose,
    // single-threaded-at-this-point process; nothing else in this
    // process has read or written the environment yet, so there is no
    // concurrent access for `set_var` to race with.
    unsafe {
        let existing_path = std::env::var("PATH").unwrap_or_default();
        let separator = if cfg!(windows) { ";" } else { ":" };
        std::env::set_var("PATH", format!("{shadow_dir}{separator}{existing_path}"));
    }

    let layout = FixtureLayout::discover();
    let runner = RealCommandRunner;
    match ingest_fixture_input(&runner, &layout, &["1.0.0"]) {
        Ok(input) => {
            println!(
                "INGESTION_OK demand_entry_point={}",
                input.demand_entry_point
            );
        }
        Err(e) => {
            eprintln!("INGESTION_FAILED: {e}");
            std::process::exit(1);
        }
    }
}
