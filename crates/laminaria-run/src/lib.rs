//! Versioned Run envelope and Level 0/1/2 process/resource/compiler
//! tracer for LAMINARIA (issue #19), following
//! `docs/measurement-foundation.md`.
//!
//! This crate implements a first, honest slice of that design, not the
//! whole thing. See each module's doc comment for exactly what it covers;
//! the summary:
//!
//! - `types`: the versioned `Run`/`ProcessRecord`/`Summary` schema
//!   (section 5).
//! - `clock`: one monotonic clock per Run (section 6).
//! - `tracer`: spawns and reaps the root command, capturing Level 0
//!   lifecycle data and Level 1 `wait4`-derived resource usage for that
//!   one process (cumulative over its reaped subtree -- see `tracer`'s
//!   doc comment for what that does and does not mean).
//! - `cargo_wrapper`/`nim_wrapper`: per-invocation granularity for
//!   Cargo/rustc and `nim c`/`cpp` builds via wrapper substitution --
//!   *not* OS-level process-tree walking, modeled on real prior art
//!   (`rustc-perf`'s `rustc-fake`) and Nim's own real compiler source,
//!   studied before implementing either (see each module's doc comment).
//! - `cargo_telemetry`: Level 2 compiler-native telemetry for Cargo
//!   builds, parsed from Cargo's own real `--message-format=json` output
//!   (studied from Cargo's actual source, not assumed) -- real artifact
//!   paths and a precise per-crate cache-freshness signal, not derived or
//!   inferred by this crate.
//! - `store`: the `runs/<run-id>/` on-disk layout (section 5) and
//!   `summary.json` regeneration from raw evidence.
//!
//! **Not yet implemented, deliberately left as explicit gaps rather than
//! silently assumed**: general (non-Cargo, non-Nim) parent/child process
//! enumeration, Level 2 telemetry for any toolchain other than Cargo
//! (rustc `-Z self-profile`, Nim stage diagnostics), Level 3 platform
//! profiler integration, full artifact inventory (section 8 -- Cargo's
//! own reported artifact paths/freshness are captured, but size/digest/
//! change-state are not), and cache-state/preparation-record population
//! (sections 9-10, beyond what Cargo's own telemetry incidentally gives).

pub mod cargo_telemetry;
pub mod cargo_wrapper;
pub mod clock;
pub mod nim_wrapper;
pub mod store;
pub mod tracer;
pub mod types;

use std::path::{Path, PathBuf};

use laminaria_fingerprint::doctor;

use crate::cargo_wrapper::{
    find_rustc_wrapper_binary, read_events, ENV_CLOCK_ANCHOR_UNIX_NS, ENV_EVENTS_PATH,
    ENV_WRAPPED_RUSTC,
};
use crate::clock::RunClock;
use crate::nim_wrapper::{find_cc_wrapper_binary, wrapper_args, ENV_WRAPPED_CC};
use crate::types::{
    CacheState, MeasurementOverhead, PreparationRecord, ProbeLevel, ProcessTrace, RootCommand, Run,
    RunResult, SCHEMA_VERSION,
};

/// Whether `root` invokes Cargo (by program name, ignoring any leading
/// directory) -- the trigger for RUSTC-wrapper substitution below. A bare
/// name check, not a resolved-path check: `cargo build`, `/usr/bin/cargo
/// test`, and `~/.cargo/bin/cargo check` should all qualify.
fn is_cargo_command(root: &RootCommand) -> bool {
    Path::new(&root.program)
        .file_stem()
        .and_then(|s| s.to_str())
        == Some("cargo")
}

/// Whether `root` invokes `nim c` or `nim cpp` -- the two Nim subcommands
/// that actually shell out to a C/C++ compiler (`compiler/extccomp.nim`,
/// studied before writing `nim_wrapper.rs`). Other subcommands (`doc`,
/// `check`, `js`, ...) never trigger CC-wrapper substitution.
fn is_nim_c_command(root: &RootCommand) -> bool {
    let is_nim = Path::new(&root.program)
        .file_stem()
        .and_then(|s| s.to_str())
        == Some("nim");
    is_nim
        && matches!(
            root.args.first().map(String::as_str),
            Some("c") | Some("cpp")
        )
}

/// Attempts to set up RUSTC-wrapper substitution (`cargo_wrapper` module
/// doc) for a Cargo root command: resolves a real `rustc` on `PATH` and
/// this crate's own `laminaria-rustc-wrapper` binary, then returns the env
/// vars to inject plus the events file they'll write to. Returns `Err`
/// (with an explanatory note) when either can't be resolved -- wrapping is
/// an enhancement over the existing root-command-only tracing, not a
/// requirement, so its absence must never fail the traced command.
fn prepare_cargo_wrapping(
    events_path: &Path,
    anchor_unix_ns: u128,
) -> Result<Vec<(String, String)>, String> {
    let real_rustc = laminaria_fingerprint::exec::which("rustc")
        .ok_or_else(|| "could not resolve a `rustc` on PATH".to_string())?;
    let wrapper_bin = find_rustc_wrapper_binary().ok_or_else(|| {
        "could not find the laminaria-rustc-wrapper binary next to the running executable \
         (expected it built alongside laminaria-cli)"
            .to_string()
    })?;

    Ok(vec![
        ("RUSTC".to_string(), wrapper_bin.display().to_string()),
        (
            ENV_WRAPPED_RUSTC.to_string(),
            real_rustc.display().to_string(),
        ),
        (
            ENV_EVENTS_PATH.to_string(),
            events_path.display().to_string(),
        ),
        (
            ENV_CLOCK_ANCHOR_UNIX_NS.to_string(),
            anchor_unix_ns.to_string(),
        ),
    ])
}

struct NimWrapping {
    extra_args: Vec<String>,
    env_vars: Vec<(String, String)>,
}

/// Attempts to set up CC-wrapper substitution (`nim_wrapper` module doc)
/// for a `nim c`/`nim cpp` root command: resolves a real `cc` on `PATH`
/// and this crate's own `laminaria-cc-wrapper` binary, then returns the
/// extra command-line arguments plus the env vars to inject. Same
/// never-fail-the-traced-command contract as `prepare_cargo_wrapping`.
fn prepare_nim_wrapping(events_path: &Path, anchor_unix_ns: u128) -> Result<NimWrapping, String> {
    let real_cc = laminaria_fingerprint::exec::which("cc")
        .ok_or_else(|| "could not resolve a `cc` on PATH".to_string())?;
    let wrapper_bin = find_cc_wrapper_binary().ok_or_else(|| {
        "could not find the laminaria-cc-wrapper binary next to the running executable \
         (expected it built alongside laminaria-cli)"
            .to_string()
    })?;

    let extra_args = wrapper_args(&wrapper_bin);
    let env_vars = vec![
        (ENV_WRAPPED_CC.to_string(), real_cc.display().to_string()),
        (
            ENV_EVENTS_PATH.to_string(),
            events_path.display().to_string(),
        ),
        (
            ENV_CLOCK_ANCHOR_UNIX_NS.to_string(),
            anchor_unix_ns.to_string(),
        ),
    ];
    Ok(NimWrapping {
        extra_args,
        env_vars,
    })
}

/// Generates a Run id from the current wall-clock time and process id.
/// Sufficiently unique for this crate's purpose (distinct `runs/<id>/`
/// directories within one repository checkout); not a claim of global
/// uniqueness across machines.
pub fn generate_run_id() -> String {
    let unix_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{unix_ns}-{}", std::process::id())
}

/// Runs `root` end-to-end: builds the `EnvironmentFingerprint`/
/// `ToolchainReport` (reusing issue #18's `laminaria-fingerprint::doctor`),
/// traces the command via `tracer::trace_root_command`, assembles a `Run`,
/// and writes it to `runs_root/<run-id>/` via `store::write_run`.
///
/// `probe_level` selects `ProbeLevel::Level1ProcessResource` (the default,
/// full `wait4`-based resource accounting plus Cargo/Nim wrapper
/// substitution where applicable) or `ProbeLevel::Level0Lifecycle` (the
/// minimal-wrapper baseline `docs/measurement-foundation.md` section 12
/// and issue #19 Experiment 6 ask every heavier probe level to be
/// measured against -- no resource accounting, no wrapper substitution,
/// portable `Child::wait` only). Any other `ProbeLevel` falls back to
/// Level 1 -- Level 2/3 are not implemented by this crate yet.
///
/// Returns the written `Run` and the directory it was written to.
#[allow(clippy::too_many_arguments)]
pub fn run_and_record(
    runs_root: &Path,
    workload_id: &str,
    scenario_id: &str,
    requested_artifact: Option<String>,
    lock_path: &Path,
    repo_root: &Path,
    root: RootCommand,
    probe_level: ProbeLevel,
) -> std::io::Result<(Run, PathBuf)> {
    let clock = RunClock::start();
    let run_id = generate_run_id();

    let doctor_run = doctor::build(lock_path, repo_root);

    let run_dir = runs_root.join(&run_id);
    std::fs::create_dir_all(&run_dir)?;
    let stdout_path = run_dir.join("stdout.log");
    let stderr_path = run_dir.join("stderr.log");
    let wrapper_events_path = run_dir.join("wrapper-invocations.jsonl");

    let is_level0 = matches!(probe_level, ProbeLevel::Level0Lifecycle);

    // Per-compiler-invocation granularity via wrapper substitution --
    // *not* OS-level process-tree walking. Cargo/rustc: Cargo's RUSTC env
    // var, modeled on rustc-perf's real `rustc-fake` (see cargo_wrapper's
    // module doc). `nim c`/`nim cpp`: Nim's `--<ccname>.exe`/
    // `--<ccname>.linkerexe` overrides, studied from Nim's own
    // compiler/extccomp.nim source (see nim_wrapper's module doc). At
    // most one applies per Run, since a root command is either Cargo or
    // Nim, never both. A resolution failure only adds a known_gaps note;
    // it never fails the traced command itself. Skipped entirely at
    // Level 0 -- wrapper substitution is itself extra instrumentation,
    // and the whole point of Level 0 is to measure a baseline with none.
    let mut effective_root = root.clone();
    let mut wrapping_note: Option<String> = None;
    let mut wrapping_kind: Option<&'static str> = None;
    let mut cargo_telemetry_requested = false;
    if is_level0 {
        // no-op: Level 0 never wraps and never requests Level 2 telemetry
        // either -- both are extra instrumentation the baseline shouldn't
        // carry (see tracer::trace_root_command_level0's doc comment).
    } else if is_cargo_command(&root) {
        match prepare_cargo_wrapping(&wrapper_events_path, clock.anchor_unix_ns()) {
            Ok(env_vars) => {
                for (key, value) in env_vars {
                    effective_root.env_overrides.insert(key, value);
                }
                wrapping_kind = Some("rustc");
            }
            Err(reason) => {
                wrapping_note = Some(format!(
                    "per-rustc-invocation tracing via RUSTC-wrapper substitution was not applied: {reason}"
                ));
            }
        }
        // Level 2 compiler telemetry (docs/measurement-foundation.md
        // section 7): Cargo's own --message-format=json, studied from
        // Cargo's real source before relying on it (see cargo_telemetry's
        // module doc). Only for the subcommands independently verified
        // not to error on the flag, and never overriding an explicit
        // --message-format the caller already specified.
        if cargo_telemetry::should_inject_message_format(&root.args) {
            effective_root
                .args
                .push("--message-format=json".to_string());
            cargo_telemetry_requested = true;
        }
    } else if is_nim_c_command(&root) {
        match prepare_nim_wrapping(&wrapper_events_path, clock.anchor_unix_ns()) {
            Ok(NimWrapping {
                extra_args,
                env_vars,
            }) => {
                // Inserted right after the "c"/"cpp" subcommand -- verified
                // this ordering compiles correctly on nim-heavy-workspace
                // before relying on it (see nim_wrapper's module doc).
                for (i, arg) in extra_args.into_iter().enumerate() {
                    effective_root.args.insert(1 + i, arg);
                }
                for (key, value) in env_vars {
                    effective_root.env_overrides.insert(key, value);
                }
                wrapping_kind = Some("cc");
            }
            Err(reason) => {
                wrapping_note = Some(format!(
                    "per-C-compiler-invocation tracing via CC-wrapper substitution was not applied: {reason}"
                ));
            }
        }
    }

    let tracer_wall_start = std::time::Instant::now();
    let process_record = if is_level0 {
        tracer::trace_root_command_level0(&clock, &effective_root, &stdout_path, &stderr_path)?
    } else {
        tracer::trace_root_command(&clock, &effective_root, &stdout_path, &stderr_path)?
    };
    let tracer_overhead_seconds = tracer_wall_start.elapsed().as_secs_f64();

    let wrapper_invocation_records = read_events(&wrapper_events_path).unwrap_or_default();
    let _ = std::fs::remove_file(&wrapper_events_path);

    let cargo_telemetry =
        cargo_telemetry_requested.then(|| cargo_telemetry::parse_cargo_json_messages(&stdout_path));

    let run_ended_at_unix_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    let exit_status = process_record
        .exit_status
        .clone()
        .expect("trace_root_command always populates exit_status on success");

    let run = Run {
        run_id: run_id.clone(),
        schema_version: SCHEMA_VERSION.to_string(),
        workload_id: workload_id.to_string(),
        scenario_id: scenario_id.to_string(),
        requested_artifact,
        environment_fingerprint: doctor_run.report.environment.clone(),
        requested_toolchain_selector: None,
        resolved_toolchain_fingerprint: Some(doctor_run.report),
        preparation_record: PreparationRecord::default(),
        cache_state: CacheState::default(),
        root_command: root,
        run_started_at_unix_ns: clock.anchor_unix_ns(),
        run_ended_at_unix_ns: Some(run_ended_at_unix_ns),
        result: Some(RunResult {
            success: exit_status.success,
            root_exit_status: exit_status,
        }),
        process_trace: {
            let wrapper_invocation_count = wrapper_invocation_records.len();
            let mut processes = vec![process_record];
            processes.extend(wrapper_invocation_records);

            let mut known_gaps = Vec::new();
            if is_level0 {
                known_gaps.push(
                    "this Run used Level 0 (lifecycle-only) tracing deliberately -- the \
                     minimal-wrapper baseline docs/measurement-foundation.md section 12 and \
                     issue #19 Experiment 6 ask for, to measure heavier probe levels' own \
                     observer overhead against; no resource_usage or wrapper substitution was \
                     even attempted, not merely unavailable"
                        .to_string(),
                );
            } else {
                match wrapping_kind {
                    Some("rustc") if wrapper_invocation_count > 0 => known_gaps.push(format!(
                        "{wrapper_invocation_count} individual rustc invocation(s) were recorded via \
                         RUSTC-wrapper substitution (see laminaria_run::cargo_wrapper); the linker is \
                         not separately recorded -- its cost rolls up into whichever rustc invocation \
                         spawned it, matching rustc-fake's own accepted scope"
                    )),
                    Some("cc") if wrapper_invocation_count > 0 => known_gaps.push(format!(
                        "{wrapper_invocation_count} individual C/C++ compiler invocation(s) (compile \
                         and link, both) were recorded via CC-wrapper substitution (see \
                         laminaria_run::nim_wrapper) for this nim c/cpp build"
                    )),
                    _ => known_gaps.push(
                        "individual descendant process enumeration (per-node pid/parent/argv/timing) \
                         is not implemented for this Run; resource_usage on the root record is \
                         cumulative over its entire reaped subtree via wait4"
                            .to_string(),
                    ),
                }
                known_gaps.push(
                    "per-invocation wrapper substitution exists only for Cargo/rustc and nim c/cpp \
                     root commands; any other command still only has the root record's cumulative \
                     resource_usage"
                        .to_string(),
                );
                if let Some(note) = wrapping_note {
                    known_gaps.push(note);
                }
            }
            match &cargo_telemetry {
                Some(telemetry) => {
                    known_gaps.push(format!(
                        "Level 2 compiler telemetry captured via Cargo's own --message-format=json \
                         ({} artifact(s) reported, {} fresh/cached); Level 3 platform profiler data \
                         is still not captured, and this is Cargo-specific -- no equivalent exists \
                         for Nim builds",
                        telemetry.artifacts.len(),
                        telemetry.artifacts.iter().filter(|a| a.fresh).count()
                    ));
                    known_gaps.push(
                        "artifact inventory (docs/measurement-foundation.md section 8) is only \
                         partially covered: Cargo's own reported artifact paths and freshness are \
                         in compiler_telemetry, but size, content digest, and create/change/delete \
                         state (the rest of section 8's required fields) are not captured"
                            .to_string(),
                    );
                }
                None => {
                    known_gaps.push(
                        "Level 2 compiler-native telemetry and Level 3 platform profiler data are \
                         not captured"
                            .to_string(),
                    );
                    known_gaps.push(
                        "artifact inventory (docs/measurement-foundation.md section 8) is not \
                         captured"
                            .to_string(),
                    );
                }
            }

            ProcessTrace {
                processes,
                known_gaps,
            }
        },
        compiler_telemetry: cargo_telemetry
            .as_ref()
            .and_then(|t| serde_json::to_value(t).ok()),
        artifact_delta: None,
        measurement_overhead: Some(MeasurementOverhead {
            probe_level,
            tracer_overhead_seconds: Some(tracer_overhead_seconds),
            notes: vec![if is_level0 {
                "tracer_overhead_seconds is this crate's own wall time around spawn/wait/\
                 record-building at Level 0 -- compare against a Level 1 Run of the identical \
                 root command (same program/args/cwd/env) to get the actual Level 1 observer \
                 overhead delta section 12 asks for; this crate does not yet automate that \
                 comparison, only makes both sides measurable"
                    .to_string()
            } else {
                "tracer_overhead_seconds is this crate's own wall time around spawn/reap/\
                 record-building at Level 1 -- compare against a Level 0 Run of the identical \
                 root command to get the actual observer overhead delta"
                    .to_string()
            }],
        }),
    };

    let dir = store::write_run(runs_root, &run)?;
    Ok((run, dir))
}
