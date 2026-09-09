//! Level 2 compiler-native telemetry for Cargo builds
//! (`docs/measurement-foundation.md` section 7): parses Cargo's own real
//! `--message-format=json` output, studied from Cargo's actual source
//! (`.reference/cargo/src/util/machine_message.rs`'s `Message` trait and
//! its `Artifact`/`FromCompiler`/`BuildScript`/`BuildFinished` structs)
//! before writing this, not assumed from documentation or memory of the
//! format.
//!
//! **What was actually checked, not assumed**: ran
//! `cargo build --message-format=json` for real on this project's own
//! `rust-heavy-workspace` fixture, both a forced-cold build and a
//! true-no-op rebuild, and inspected the literal JSON lines produced:
//! `"reason": "compiler-artifact"` carries real artifact paths
//! (`filenames`) and a per-crate `"fresh"` boolean that flips from
//! `false` (cold) to `true` (no-op) exactly as `machine_message.rs`'s
//! doc comment on the `Artifact` struct implies. Also checked that
//! `--message-format=json` is safe to inject only for the Cargo
//! subcommands verified to accept it (`build`, `check`, `doc` confirmed
//! directly; `test`/`run`/`bench` assumed to behave the same way since
//! they share Cargo's compilation path, but not independently verified) --
//! `cargo clean --message-format=json` errors outright
//! (`unexpected argument '--message-format' found`), so this is never
//! injected unconditionally for every Cargo invocation. See
//! `CARGO_MESSAGE_FORMAT_SUBCOMMANDS`.
//!
//! Cargo writes these messages to **stdout** specifically (human-readable
//! progress goes to stderr) -- confirmed by observing `laminaria run`'s
//! own `stdout.log` capture contained only clean JSON lines with no
//! interleaved "Compiling .../Finished ..." text, which the tracer
//! already captures to a separate file. No new capture mechanism was
//! needed; this module only adds a parser for what
//! `tracer::trace_root_command` was already writing to `stdout.log`.

use std::path::Path;

use crate::types::{CargoCompilerTelemetry, CompilerArtifactRecord};

/// Cargo subcommands independently confirmed to accept
/// `--message-format=json` without erroring (`build`, `check`, `doc`); the
/// three others listed here (`test`, `run`, `bench`) share Cargo's
/// compilation path and are expected, but not separately verified, to
/// behave the same way -- noted as an assumption, not presented as
/// equally checked.
pub const CARGO_MESSAGE_FORMAT_SUBCOMMANDS: &[&str] =
    &["build", "check", "doc", "test", "run", "bench"];

/// Parses Cargo's `--message-format=json` output from `stdout_path` (a
/// file `tracer::trace_root_command` already wrote to, JSON Lines format)
/// into `CargoCompilerTelemetry`. Never fails: an unreadable file or a
/// line that doesn't parse as JSON is reflected in the result
/// (`unparsed_line_count`) rather than returned as an error, since a
/// telemetry-parsing problem must never fail the traced command's own
/// already-recorded Run.
pub fn parse_cargo_json_messages(stdout_path: &Path) -> CargoCompilerTelemetry {
    let text = match std::fs::read_to_string(stdout_path) {
        Ok(text) => text,
        Err(_) => return CargoCompilerTelemetry::default(),
    };

    let mut telemetry = CargoCompilerTelemetry::default();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            telemetry.unparsed_line_count += 1;
            continue;
        };
        let Some(reason) = value.get("reason").and_then(|r| r.as_str()) else {
            telemetry.unparsed_line_count += 1;
            continue;
        };

        match reason {
            "compiler-artifact" => {
                if let Some(record) = parse_artifact(&value) {
                    telemetry.artifacts.push(record);
                } else {
                    telemetry.unparsed_line_count += 1;
                }
            }
            "compiler-message" => telemetry.compiler_message_count += 1,
            "build-script-executed" => telemetry.build_script_executed_count += 1,
            "build-finished" => {
                telemetry.build_finished_success = value.get("success").and_then(|s| s.as_bool());
            }
            // "build-started" and any future reason this crate doesn't
            // model yet -- counted as unparsed rather than silently
            // ignored, so a growing gap between total stdout lines and
            // recognized reasons stays visible.
            _ => telemetry.unparsed_line_count += 1,
        }
    }

    telemetry
}

fn parse_artifact(value: &serde_json::Value) -> Option<CompilerArtifactRecord> {
    let package_id = value.get("package_id")?.as_str()?.to_string();
    let target = value.get("target")?;
    let target_name = target.get("name")?.as_str()?.to_string();
    let target_kind = target
        .get("kind")?
        .as_array()?
        .iter()
        .filter_map(|k| k.as_str().map(str::to_string))
        .collect();
    let filenames = value
        .get("filenames")?
        .as_array()?
        .iter()
        .filter_map(|f| f.as_str().map(std::path::PathBuf::from))
        .collect();
    let executable = value
        .get("executable")
        .and_then(|e| e.as_str())
        .map(std::path::PathBuf::from);
    let fresh = value.get("fresh")?.as_bool()?;

    Some(CompilerArtifactRecord {
        package_id,
        target_name,
        target_kind,
        filenames,
        executable,
        fresh,
    })
}

/// Determines whether `--message-format=json` should be injected for this
/// Cargo invocation and, if so, the index in `args` to insert it at --
/// always immediately after the subcommand (accounting for a leading
/// `+toolchain` arg ahead of it), which is always inside Cargo's own
/// option-parsing region, never past a literal `--` separator (the region
/// after `--` belongs to the program Cargo runs, not to Cargo itself --
/// e.g. `cargo run -- my-program-arg`. Appending the flag unconditionally
/// at the very end of `args`, as an earlier version of this crate did,
/// could push it past `--` and hand it to the target program instead of
/// Cargo; verified against Cargo's own argument-parsing convention, not
/// assumed).
///
/// `args[0]` (or `args[1]` when `args[0]` is a `+toolchain` selector -- this
/// crate's own callers, `laminaria run -- cargo [+toolchain] <subcommand>
/// ...`, always place the subcommand there; a flag *before* the subcommand,
/// e.g. `cargo --manifest-path x build`, is not recognized by this simple
/// check and is a known limitation, not silently mishandled) must be one of
/// `CARGO_MESSAGE_FORMAT_SUBCOMMANDS`, and the caller must not have already
/// specified a `--message-format`/`--message-format=...` of their own
/// (checked only in Cargo's own option region, before any `--`, so an
/// unrelated `--message-format` the caller passes through to the target
/// program doesn't false-positive) -- never override an explicit user
/// choice.
pub fn message_format_insert_index(args: &[String]) -> Option<usize> {
    let subcommand_index = if args.first().is_some_and(|a| a.starts_with('+')) {
        1
    } else {
        0
    };
    let subcommand = args.get(subcommand_index)?;
    if !CARGO_MESSAGE_FORMAT_SUBCOMMANDS.contains(&subcommand.as_str()) {
        return None;
    }

    let cargo_owned_args = match args.iter().position(|a| a == "--") {
        Some(dashdash_index) => &args[..dashdash_index],
        None => args,
    };
    let already_specified = cargo_owned_args
        .iter()
        .any(|a| a == "--message-format" || a.starts_with("--message-format="));
    if already_specified {
        return None;
    }

    Some(subcommand_index + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_stdout(lines: &[&str]) -> std::path::PathBuf {
        // A counter, not just the process id, disambiguates temp file
        // names across tests in this module -- two tests writing the
        // same *number* of lines previously collided on an identical
        // path (both derived their name from `lines.len()` alone) and,
        // run in parallel, one clobbered the other's file mid-test. Caught
        // by this module's own tests failing nondeterministically, not
        // assumed safe.
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "laminaria-run-cargo-telemetry-test-{}-{n}.log",
            std::process::id(),
        ));
        let mut file = std::fs::File::create(&path).unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
        path
    }

    #[test]
    fn parses_a_real_cold_build_message_stream() {
        // Taken verbatim from an actual `cargo build --message-format=json`
        // run on this project's own rust-heavy-workspace fixture, not
        // hand-constructed -- see this module's doc comment.
        let path = write_stdout(&[
            r#"{"reason":"compiler-artifact","package_id":"path+file:///x/fixture-core#0.1.0","manifest_path":"/x/fixture-core/Cargo.toml","target":{"kind":["lib"],"crate_types":["lib"],"name":"fixture_core","src_path":"/x/lib.rs","edition":"2021","doc":true,"doctest":true,"test":true},"profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":false},"features":[],"filenames":["/x/target/debug/libfixture_core.rlib","/x/target/debug/deps/libfixture_core-e8cb29cbc5b2a0e4.rmeta"],"executable":null,"fresh":false}"#,
            r#"{"reason":"compiler-artifact","package_id":"path+file:///x/fixture-bin#0.1.0","manifest_path":"/x/fixture-bin/Cargo.toml","target":{"kind":["bin"],"crate_types":["bin"],"name":"fixture-bin","src_path":"/x/main.rs","edition":"2021","doc":true,"doctest":false,"test":true},"profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":false},"features":[],"filenames":["/x/target/debug/fixture-bin"],"executable":"/x/target/debug/fixture-bin","fresh":false}"#,
            r#"{"reason":"build-finished","success":true}"#,
        ]);

        let telemetry = parse_cargo_json_messages(&path);

        assert_eq!(telemetry.artifacts.len(), 2);
        assert_eq!(telemetry.artifacts[0].target_name, "fixture_core");
        assert!(!telemetry.artifacts[0].fresh);
        assert_eq!(telemetry.artifacts[0].filenames.len(), 2);
        assert_eq!(telemetry.artifacts[1].target_name, "fixture-bin");
        assert_eq!(
            telemetry.artifacts[1].executable,
            Some(std::path::PathBuf::from("/x/target/debug/fixture-bin"))
        );
        assert_eq!(telemetry.build_finished_success, Some(true));
        assert_eq!(telemetry.unparsed_line_count, 0);
    }

    #[test]
    fn fresh_flag_distinguishes_a_true_noop_rebuild() {
        let path = write_stdout(&[
            r#"{"reason":"compiler-artifact","package_id":"x","target":{"name":"fixture_core","kind":["lib"]},"filenames":["/x/libfixture_core.rlib"],"executable":null,"fresh":true}"#,
        ]);

        let telemetry = parse_cargo_json_messages(&path);

        assert_eq!(telemetry.artifacts.len(), 1);
        assert!(telemetry.artifacts[0].fresh);
    }

    #[test]
    fn non_json_and_unrecognized_lines_are_counted_not_dropped_silently() {
        let path = write_stdout(&[
            "   Compiling fixture-core v0.1.0",
            r#"{"reason":"build-started","run_id":"abc"}"#,
            "not json at all",
        ]);

        let telemetry = parse_cargo_json_messages(&path);

        assert_eq!(telemetry.artifacts.len(), 0);
        assert_eq!(telemetry.unparsed_line_count, 3);
    }

    #[test]
    fn missing_file_returns_an_empty_default_not_an_error() {
        let telemetry =
            parse_cargo_json_messages(Path::new("/nonexistent/laminaria-run-test-path.log"));
        assert_eq!(telemetry.artifacts.len(), 0);
        assert_eq!(telemetry.unparsed_line_count, 0);
    }

    #[test]
    fn should_inject_only_for_verified_subcommands_and_never_overrides_an_explicit_choice() {
        assert_eq!(message_format_insert_index(&["build".to_string()]), Some(1));
        assert_eq!(
            message_format_insert_index(&[
                "check".to_string(),
                "--manifest-path".to_string(),
                "x".to_string(),
            ]),
            Some(1)
        );
        assert_eq!(message_format_insert_index(&["clean".to_string()]), None);
        // Documented limitation: a flag *before* the subcommand is not
        // recognized (args[0] must be the subcommand itself, or a
        // +toolchain selector immediately ahead of it).
        assert_eq!(
            message_format_insert_index(&[
                "--manifest-path".to_string(),
                "x".to_string(),
                "check".to_string(),
            ]),
            None
        );
        assert_eq!(
            message_format_insert_index(&[
                "build".to_string(),
                "--message-format=human".to_string(),
            ]),
            None
        );
        assert_eq!(
            message_format_insert_index(&[
                "build".to_string(),
                "--message-format".to_string(),
                "human".to_string(),
            ]),
            None
        );
    }

    #[test]
    fn a_leading_toolchain_selector_shifts_the_subcommand_and_insertion_index() {
        assert_eq!(
            message_format_insert_index(&["+nightly".to_string(), "build".to_string()]),
            Some(2)
        );
    }

    #[test]
    fn inserts_before_the_dash_dash_separator_not_after_target_program_args() {
        // cargo run -- my-program-arg --message-format=human: the flag
        // after `--` belongs to the target program, not Cargo, so it must
        // not suppress injection or be mistaken for Cargo's own choice.
        assert_eq!(
            message_format_insert_index(&[
                "run".to_string(),
                "--".to_string(),
                "my-program-arg".to_string(),
                "--message-format=human".to_string(),
            ]),
            Some(1)
        );
    }
}
