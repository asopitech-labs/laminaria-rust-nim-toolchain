//! Source-derived owned IR for a declared Rust/Nim subset (issues #25/#3),
//! implemented jointly with the #6/#8 planner/scheduler contract shape per
//! `docs/compiler-ownership-contract.md`'s corrected research order.
//!
//! ## Scope of this task
//!
//! A small, declared subset: `i32`/`int32` parameters and locals, integer
//! literals, wrapping arithmetic (`wrapping_add/sub/mul`, `+%`/`-%`/`*%`),
//! `!=`/`==` against a literal `0`, `if`/`else` at a block's tail position,
//! `let` bindings, and calls between functions in the same lowering
//! request. Both `rust_frontend`/`nim_frontend` reject anything outside
//! this subset with a [`diagnostics::Diagnostic`] naming the real
//! construct and source span -- never a panic, never a partial IR, never a
//! fallback to invoking `rustc`/`nim` (`docs/compiler-ownership-contract.md`'s
//! "Separate roles" table).
//!
//! ## What this closes from the existing fixture
//!
//! `fixtures/laminaria-semantic-substrate-prototype/substrate/src/repr.rs`'s
//! own doc comment admits its `Expr`/`Stmt` grammar was "transcribed by
//! hand ... not derived mechanically" from `rust-src/add_or_double.rs`/
//! `nim-src/add_or_double.nim`, carries no source positions, and models
//! effects as a bare `has_side_effects: bool` with no way to verify what
//! the effect actually was or when it happened. This crate's
//! [`types::Program`] is parsed from real source with [`types::Provenance`]
//! on every node, and [`interpreter::EvalOutcome`] carries a real,
//! executable effect trace (the actual sequence of function calls) instead
//! of a declared boolean.
//!
//! ## Task 2 seam (not implemented here)
//!
//! Issues #6/#8 (the production Nim planner / Rust scheduler) are meant to
//! consume this crate's output as LAMINARIA-owned compiler work, not just
//! whole external-compiler processes. The two functions that seam is built
//! around:
//!
//! - [`rust_frontend::lower_rust_source`] / [`nim_frontend::lower_nim_source`]
//!   : `(source_file, source_text, requested_functions) -> Result<Program,
//!   Vec<Diagnostic>>` -- pure, over serializable inputs/outputs.
//! - [`transform`]'s two candidate transformations: `(&Program, fn_name) ->
//!   Result<Program, Vec<Diagnostic>>`, same shape.
//!
//! Wiring a new `laminaria_plan::ActionKind` variant around these later is
//! meant to be a small, well-defined addition, not a redesign -- but this
//! crate does not itself touch `laminaria-plan`, `nim-planner`,
//! `laminaria-run`, or `laminaria-cli`; that is Task 2's own scope.

pub mod diagnostics;
pub mod discover;
pub mod interpreter;
pub mod nim_frontend;
pub mod rust_frontend;
pub mod transform;
pub mod types;
pub mod validate;
pub mod wasm_target;

// `#[cfg(all(test, unix))]` on the whole module, not `#[cfg(unix)]` on each
// test function individually -- a CI failure caught the difference
// directly: gating only the functions left this module's own `use`
// imports and helpers (`repo_root`, `TEST_INPUTS`) unconditionally
// compiled on every platform, so the `windows` job (which never
// provisions Nim, and doesn't need this module's real-toolchain-invoking
// tests at all) failed on `-D warnings` (`unused_imports`/`dead_code`) for
// items nothing on that platform could reference. Gating the module
// itself removes the entire module's contents together, matching every
// other real-toolchain-invoking test in this workspace (the `windows` job
// is kept lean).
// This module is the *only* place in this whole crate that invokes an
// external `rustc`/`nim` process (grep confirms it) -- every other test
// is a pure, self-contained independent-IR test. Verified directly, not
// merely structurally argued: building this crate's test binary
// normally, then running the *already-compiled* binary with `rustc`/
// `nim` invisible on `PATH` (`PATH=/usr/bin:/bin ... --skip
// fixture_parity_tests`) passes every one of the remaining tests --
// issue #27 A4's own requirement that independent-IR tests run "from a
// distributed test runner rustc/nim are invisible from" ("配布済みtest
// runnerからrustc/nimが見えない環境でも実行できる"). This is a fact about
// this workspace's `laminaria-ir` crate specifically -- not a claim about
// any other crate in it.
#[cfg(all(test, unix))]
mod fixture_parity_tests {
    //! The actual proof this crate is source-derived, not hand-transcribed
    //! the way `fixtures/laminaria-semantic-substrate-prototype/substrate/
    //! src/repr.rs` is: parses the **existing** `rust-src/add_or_double.rs`
    //! for real through `rust_frontend`, and diffs its interpreter output
    //! against a genuinely `rustc`-compiled binary of that exact file, for
    //! the same `TEST_INPUTS` the file itself already declares.

    use crate::interpreter::eval_function;
    use crate::rust_frontend::lower_rust_source;
    use std::path::PathBuf;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap()
    }

    const TEST_INPUTS: &[(i32, i32, i32)] = &[(3, 4, 0), (3, 4, 1), (i32::MAX, 1, 0), (-5, 10, 1)];

    #[test]
    #[cfg(unix)]
    fn rust_frontend_output_matches_a_real_rustc_compiled_binary() {
        let repo_root = repo_root();
        let source_file = repo_root
            .join("fixtures/laminaria-semantic-substrate-prototype/rust-src/add_or_double.rs");
        let source_text = std::fs::read_to_string(&source_file).unwrap();

        let program =
            lower_rust_source(&source_file, &source_text, &["double", "add_or_double"]).unwrap();

        let tmp = std::env::temp_dir().join(format!(
            "laminaria-ir-rustc-parity-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let binary = tmp.join("add_or_double");
        let status = std::process::Command::new("rustc")
            .args(["-O", "-o"])
            .arg(&binary)
            .arg(&source_file)
            .status()
            .expect("failed to invoke rustc -- is Rust installed?");
        assert!(
            status.success(),
            "rustc failed to compile the real fixture source"
        );

        let output = std::process::Command::new(&binary).output().unwrap();
        let real_stdout = String::from_utf8(output.stdout).unwrap();

        let mut derived_stdout = String::new();
        for &(a, b, use_double) in TEST_INPUTS {
            let outcome = eval_function(
                &program,
                "add_or_double",
                &[a as i64, b as i64, use_double as i64],
            )
            .unwrap();
            derived_stdout.push_str(&format!("{a},{b},{use_double},{}\n", outcome.value as i32));
        }

        assert_eq!(
            real_stdout, derived_stdout,
            "this crate's source-derived IR + interpreter must match the real compiled binary's \
             own output byte for byte"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    #[cfg(unix)]
    fn nim_frontend_output_matches_a_real_nim_compiled_binary() {
        use crate::nim_frontend::lower_nim_source;

        let repo_root = repo_root();
        let source_file = repo_root
            .join("fixtures/laminaria-semantic-substrate-prototype/nim-src/add_or_double.nim");
        let source_text = std::fs::read_to_string(&source_file).unwrap();

        let program =
            lower_nim_source(&source_file, &source_text, &["double", "addOrDouble"]).unwrap();

        let tmp = std::env::temp_dir().join(format!(
            "laminaria-ir-nim-parity-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let binary = tmp.join("add_or_double");
        let status = std::process::Command::new("nim")
            .args(["c", "--hints:off"])
            .arg(format!("--nimcache:{}", tmp.join("nimcache").display()))
            .arg(format!("-o:{}", binary.display()))
            .arg(&source_file)
            .status()
            .expect("failed to invoke nim -- is Nim installed?");
        assert!(
            status.success(),
            "nim failed to compile the real fixture source"
        );

        let output = std::process::Command::new(&binary).output().unwrap();
        let real_stdout = String::from_utf8(output.stdout).unwrap();

        let mut derived_stdout = String::new();
        for &(a, b, use_double) in TEST_INPUTS {
            let outcome = eval_function(
                &program,
                "addOrDouble",
                &[a as i64, b as i64, use_double as i64],
            )
            .unwrap();
            derived_stdout.push_str(&format!("{a},{b},{use_double},{}\n", outcome.value as i32));
        }

        assert_eq!(
            real_stdout, derived_stdout,
            "this crate's source-derived IR + interpreter must match the real Nim-compiled \
             binary's own output byte for byte"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
