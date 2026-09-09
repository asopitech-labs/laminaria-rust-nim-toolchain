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
//! - `nim_telemetry`: Level 2 compiler-native telemetry for `nim c`/
//!   `cpp` builds. Nim has no `--message-format=json` equivalent
//!   (checked directly against Nim's real compiler source, not assumed),
//!   so this parses Nim's own human-oriented hint/verbosity stream
//!   instead -- a materially weaker reliability claim than the Cargo
//!   adapter, stated explicitly rather than glossed over (see that
//!   module's doc comment).
//! - `artifact_inventory`: issue #20's first slice -- a no-op-safe
//!   create/modify/delete/unchanged artifact diff around the traced
//!   command, for explicitly-given `--observe` roots (no Cargo/Nim
//!   output-directory auto-detection), with enumeration and content-hashing
//!   costs measured separately (see that module's doc comment for exactly
//!   what "Modified" does and does not mean, and issue #20's remaining
//!   open acceptance criteria).
//! - `store`: the `runs/<run-id>/` on-disk layout (section 5) and
//!   `summary.json` regeneration from raw evidence.
//!
//! **Not yet implemented, deliberately left as explicit gaps rather than
//! silently assumed**: general (non-Cargo, non-Nim) parent/child process
//! enumeration, rustc `-Z self-profile`, Level 3 platform profiler
//! integration, per-artifact producing-ToolchainFingerprint/producer
//! correlation, artifact-inventory auto-detection of Cargo/Nim output
//! directories, and cache-state/preparation-record population (sections
//! 9-10, beyond what Cargo's own telemetry incidentally gives).

pub mod artifact_inventory;
pub mod cargo_telemetry;
pub mod cargo_wrapper;
pub mod clock;
pub mod nim_telemetry;
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

/// Resolves the real `rustc` Cargo-wrapper substitution should ultimately
/// invoke for `root`, respecting whatever toolchain-selection mechanism
/// `root` itself already expresses, instead of always substituting PATH's
/// default `rustc`. That blind substitution was a real correctness bug:
/// Cargo uses an explicit `RUSTC` env var as the literal compiler to
/// invoke, bypassing rustup toolchain selection entirely -- so overwriting
/// it unconditionally would silently discard a `cargo +nightly build`
/// request (or a `RUSTC`/`RUSTUP_TOOLCHAIN` the caller had already set) and
/// measure the wrong compiler instead, without any indication to the
/// caller that happened.
///
/// Checked in priority order:
/// 1. An explicit `RUSTC` already on `root.env_overrides` -- never overwrite
///    a caller's own choice.
/// 2. A leading `+toolchain` arg (`cargo +nightly build`), resolved via
///    `rustup which rustc --toolchain <toolchain>`.
/// 3. `RUSTUP_TOOLCHAIN` on `root.env_overrides`, resolved the same way.
/// 4. PATH's default `rustc` (this crate's original behavior, still correct
///    when none of the above apply).
///
/// A `rust-toolchain`/`rust-toolchain.toml` file in the invoked directory is
/// a real, separate selection mechanism this does not check -- a named,
/// open gap, not silently treated as equivalent to "no override".
fn resolve_real_rustc(root: &RootCommand) -> Result<PathBuf, String> {
    if let Some(rustc) = root.env_overrides.get("RUSTC") {
        return Ok(PathBuf::from(rustc));
    }
    if let Some(plus_arg) = root.args.first().filter(|a| a.starts_with('+')) {
        let toolchain = &plus_arg[1..];
        return resolve_rustc_via_rustup(toolchain).ok_or_else(|| {
            format!(
                "root command requests toolchain `{plus_arg}` but the real rustc for it could \
                 not be resolved via `rustup which rustc --toolchain {toolchain}`; refusing to \
                 silently substitute PATH's default rustc instead, which would measure the wrong \
                 compiler"
            )
        });
    }
    if let Some(toolchain) = root.env_overrides.get("RUSTUP_TOOLCHAIN") {
        return resolve_rustc_via_rustup(toolchain).ok_or_else(|| {
            format!(
                "root command sets RUSTUP_TOOLCHAIN={toolchain} but the real rustc for it could \
                 not be resolved via `rustup which rustc --toolchain {toolchain}`; refusing to \
                 silently substitute PATH's default rustc instead"
            )
        });
    }
    laminaria_fingerprint::exec::which("rustc")
        .ok_or_else(|| "could not resolve a `rustc` on PATH".to_string())
}

fn resolve_rustc_via_rustup(toolchain: &str) -> Option<PathBuf> {
    let rustup = laminaria_fingerprint::exec::which("rustup")?;
    let output = std::process::Command::new(rustup)
        .args(["which", "rustc", "--toolchain", toolchain])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!stdout.is_empty()).then(|| PathBuf::from(stdout))
}

/// Same reasoning as `resolve_real_rustc`, for the CC-wrapper side: never
/// overwrite a `CC` the caller already set on `root.env_overrides`. Nim
/// has no `+toolchain`-style selector syntax for its C backend, so unlike
/// the Rust side there is no equivalent second case to check.
fn resolve_real_cc(root: &RootCommand) -> Result<PathBuf, String> {
    if let Some(cc) = root.env_overrides.get("CC") {
        return Ok(PathBuf::from(cc));
    }
    laminaria_fingerprint::exec::which("cc")
        .ok_or_else(|| "could not resolve a `cc` on PATH".to_string())
}

/// Best-effort, human-readable description of which toolchain-selection
/// mechanism `root` itself expresses -- not a resolved compiler path, just
/// enough for a later reader comparing Runs across machines to tell "this
/// Run asked for nightly" from "this Run asked for whatever's default"
/// instead of always seeing `None` regardless of what was actually
/// requested. Checked in the same priority order `resolve_real_rustc`/
/// `resolve_real_cc` reason about. A `rust-toolchain(.toml)` file is a
/// real, separate mechanism this does not detect -- named as a gap, not
/// silently treated as "no selector was requested".
fn detect_requested_toolchain_selector(root: &RootCommand) -> Option<String> {
    if let Some(rustc) = root.env_overrides.get("RUSTC") {
        return Some(format!("RUSTC={rustc}"));
    }
    if let Some(plus_arg) = root.args.first().filter(|a| a.starts_with('+')) {
        return Some(format!("cargo {plus_arg}"));
    }
    if let Some(toolchain) = root.env_overrides.get("RUSTUP_TOOLCHAIN") {
        return Some(format!("RUSTUP_TOOLCHAIN={toolchain}"));
    }
    if let Some(cc) = root.env_overrides.get("CC") {
        return Some(format!("CC={cc}"));
    }
    None
}

/// Attempts to set up RUSTC-wrapper substitution (`cargo_wrapper` module
/// doc) for a Cargo root command: resolves the real `rustc` the traced
/// command actually asked for (`resolve_real_rustc`, not just whatever's on
/// PATH) and this crate's own `laminaria-rustc-wrapper` binary, then
/// returns the env vars to inject plus the events file they'll write to.
/// Returns `Err` (with an explanatory note) when either can't be resolved --
/// wrapping is an enhancement over the existing root-command-only tracing,
/// not a requirement, so its absence must never fail the traced command.
///
/// Unix-only, checked here rather than left implicit: the substituted
/// `laminaria-rustc-wrapper` binary itself measures each real `rustc`
/// invocation via `tracer::reap` (`wait4`-based), which is `Unsupported` on
/// every other platform (see tracer.rs's non-unix `reap`). Letting wrapper
/// substitution proceed there would set Cargo's `RUSTC` to a wrapper binary
/// that then fails every single compilation unit -- silently breaking the
/// actual traced build, a strictly worse outcome than simply not wrapping.
fn prepare_cargo_wrapping(
    root: &RootCommand,
    events_path: &Path,
    anchor_unix_ns: u128,
) -> Result<Vec<(String, String)>, String> {
    if !cfg!(unix) {
        return Err(
            "RUSTC-wrapper substitution is only implemented for Unix targets (the wrapper \
             binary's own per-invocation measurement is wait4-based)"
                .to_string(),
        );
    }
    let real_rustc = resolve_real_rustc(root)?;
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
fn prepare_nim_wrapping(
    root: &RootCommand,
    events_path: &Path,
    anchor_unix_ns: u128,
) -> Result<NimWrapping, String> {
    if !cfg!(unix) {
        // Same reasoning as prepare_cargo_wrapping's identical guard: the
        // substituted laminaria-cc-wrapper binary's own per-invocation
        // measurement is wait4-based and Unsupported on non-Unix targets.
        return Err(
            "CC-wrapper substitution is only implemented for Unix targets (the wrapper binary's \
             own per-invocation measurement is wait4-based)"
                .to_string(),
        );
    }
    let real_cc = resolve_real_cc(root)?;
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
/// `observation_roots` (issue #20's artifact inventory) selects which
/// directories get a before/after snapshot around the traced command,
/// producing `Run::artifact_delta` (create/modify/delete/unchanged per
/// file, with enumeration/hashing costs recorded separately -- see
/// `artifact_inventory`'s module doc for exactly what this first slice
/// does and does not cover). An empty slice means no artifact inventory is
/// captured at all, noted in `known_gaps`, not silently absent.
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
    observation_roots: &[PathBuf],
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
    let mut nim_telemetry_requested = false;
    if is_level0 {
        // no-op: Level 0 never wraps and never requests Level 2 telemetry
        // either -- both are extra instrumentation the baseline shouldn't
        // carry (see tracer::trace_root_command_level0's doc comment).
    } else if is_cargo_command(&root) {
        match prepare_cargo_wrapping(&root, &wrapper_events_path, clock.anchor_unix_ns()) {
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
        // --message-format the caller already specified. Inserted right
        // after the subcommand (`message_format_insert_index`), never
        // pushed onto the end of args -- appending unconditionally could
        // land the flag past a `--` separator and hand it to the target
        // program instead of Cargo (e.g. `cargo run -- my-program-arg`).
        if let Some(insert_at) = cargo_telemetry::message_format_insert_index(&root.args) {
            effective_root
                .args
                .insert(insert_at, "--message-format=json".to_string());
            cargo_telemetry_requested = true;
        }
    } else if is_nim_c_command(&root) {
        match prepare_nim_wrapping(&root, &wrapper_events_path, clock.anchor_unix_ns()) {
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
        // Level 2 compiler telemetry for Nim (docs/measurement-foundation.md
        // section 7): unlike Cargo, this needs no extra flag -- Nim's hint
        // stream is on by default -- so it's simply parsed afterward from
        // stderr.log (see nim_telemetry's module doc for why stderr, not
        // stdout). Still gated on Level 1 for consistency with the Cargo
        // adapter's "no Level 2 at the minimal baseline" story, even
        // though parsing already-produced output costs the traced command
        // nothing extra.
        nim_telemetry_requested = true;
    }

    // Computed from the original, unwrapped `root` -- not `effective_root`,
    // whose RUSTC/CC env vars now point at this crate's own wrapper
    // binaries, which would no longer reflect what the caller actually
    // requested.
    let requested_toolchain_selector = detect_requested_toolchain_selector(&root);

    // Timed separately from artifact_inventory's own enumeration_seconds/
    // hash_seconds via this Cell, whether or not observation_roots is
    // empty -- tracer_overhead_seconds must keep meaning exactly what
    // issue #19 Experiment 6 already measured it as (spawn/reap/
    // record-building), not grow to silently include artifact-snapshot
    // wall time too.
    let tracer_overhead_seconds_cell = std::cell::Cell::new(0.0f64);
    let do_trace = || {
        let start = std::time::Instant::now();
        let result = if is_level0 {
            tracer::trace_root_command_level0(&clock, &effective_root, &stdout_path, &stderr_path)
        } else {
            tracer::trace_root_command(&clock, &effective_root, &stdout_path, &stderr_path)
        };
        tracer_overhead_seconds_cell.set(start.elapsed().as_secs_f64());
        result
    };
    let (process_record, artifact_inventory) = if observation_roots.is_empty() {
        (do_trace()?, None)
    } else {
        let (record, inventory) = artifact_inventory::observe_around(observation_roots, do_trace)?;
        (record, Some(inventory))
    };
    let tracer_overhead_seconds = tracer_overhead_seconds_cell.get();

    // The raw wrapper-invocations.jsonl file is deliberately left on disk
    // (not deleted) -- it's the same kind of raw evidence stdout.log/
    // stderr.log already are, and a parse failure on one line must not cost
    // the well-formed lines their only backing record. See
    // EventsReadResult's own doc comment.
    let events_result = read_events(&wrapper_events_path).unwrap_or_default();
    let wrapper_invocation_records = events_result.records;

    let cargo_telemetry =
        cargo_telemetry_requested.then(|| cargo_telemetry::parse_cargo_json_messages(&stdout_path));
    let nim_telemetry =
        nim_telemetry_requested.then(|| nim_telemetry::parse_nim_hint_stream(&stderr_path));

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
        requested_toolchain_selector,
        resolved_toolchain_fingerprint: Some(doctor_run.report),
        preparation_record: PreparationRecord::default(),
        cache_state: CacheState::default(),
        // The command as actually executed (including wrapper-substitution
        // env vars and any inserted args like --message-format=json), not
        // the caller's original request -- ProcessRecord::argv already
        // reflects this, and root_command must match it for the Run to be
        // reproducible/trustworthy evidence, not silently divergent from
        // what actually ran.
        root_command: effective_root,
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
                if events_result.unparsed_line_count > 0 {
                    known_gaps.push(format!(
                        "{} line(s) of wrapper-invocations.jsonl failed to parse (a partial write \
                         from a killed process or filesystem fault) and were skipped; the \
                         well-formed lines above were kept, and the raw file itself was left at \
                         wrapper-invocations.jsonl in this Run's directory for manual recovery",
                        events_result.unparsed_line_count
                    ));
                }
            }
            match (&cargo_telemetry, &nim_telemetry) {
                (Some(telemetry), _) => {
                    known_gaps.push(format!(
                        "Level 2 compiler telemetry captured via Cargo's own --message-format=json \
                         ({} artifact(s) reported, {} fresh/cached); Level 3 platform profiler data \
                         is still not captured, and this is Cargo-specific -- no equivalent exists \
                         for Nim builds",
                        telemetry.artifacts.len(),
                        telemetry.artifacts.iter().filter(|a| a.fresh).count()
                    ));
                }
                (None, Some(telemetry)) => {
                    known_gaps.push(format!(
                        "Level 2 compiler telemetry captured by parsing Nim's own hint/verbosity \
                         stream ({} module(s) reached C-codegen, linked={}) -- unlike Cargo's \
                         --message-format=json, this has no documented stability contract from Nim \
                         itself (see laminaria_run::nim_telemetry); Level 3 platform profiler data \
                         is still not captured",
                        telemetry.processed_modules.len(),
                        telemetry.linked
                    ));
                }
                (None, None) => {
                    known_gaps.push(
                        "Level 2 compiler-native telemetry and Level 3 platform profiler data are \
                         not captured"
                            .to_string(),
                    );
                }
            }
            match &artifact_inventory {
                Some(inventory) => {
                    let created = inventory
                        .records
                        .iter()
                        .filter(|r| r.state == artifact_inventory::ArtifactState::Created)
                        .count();
                    let modified = inventory
                        .records
                        .iter()
                        .filter(|r| r.state == artifact_inventory::ArtifactState::Modified)
                        .count();
                    let deleted = inventory
                        .records
                        .iter()
                        .filter(|r| r.state == artifact_inventory::ArtifactState::Deleted)
                        .count();
                    known_gaps.push(format!(
                        "artifact inventory (issue #20) captured over {} observation root(s): \
                         {created} created, {modified} modified, {deleted} deleted, {} unchanged \
                         (of {} total); {} changed candidate(s) hashed in {:.3}s, enumeration took \
                         {:.3}s -- producer identity is always Unknown in this first slice (not yet \
                         correlated with wrapper-invocation/Cargo-message evidence), and only the \
                         explicitly given observation roots are covered, not an auto-detected \
                         Cargo/Nim output directory",
                        inventory.observation_roots.len(),
                        inventory.records.len() - created - modified - deleted,
                        inventory.records.len(),
                        inventory.changed_candidate_count,
                        inventory.hash_seconds,
                        inventory.enumeration_seconds,
                    ));
                }
                None => {
                    known_gaps.push(
                        "artifact inventory (issue #20) was not captured -- no --observe root(s) \
                         were given to this Run"
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
            .and_then(|t| serde_json::to_value(t).ok())
            .or_else(|| {
                nim_telemetry
                    .as_ref()
                    .and_then(|t| serde_json::to_value(t).ok())
            }),
        artifact_delta: artifact_inventory
            .as_ref()
            .and_then(|inv| serde_json::to_value(inv).ok()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn root(program: &str, args: &[&str]) -> RootCommand {
        RootCommand {
            program: program.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            cwd: None,
            env_overrides: BTreeMap::new(),
        }
    }

    #[test]
    fn resolve_real_rustc_respects_an_explicit_rustc_override_without_touching_path() {
        // A caller-set RUSTC must never be silently overwritten by
        // whatever's on PATH -- that was the exact bug this fixes.
        let mut r = root("cargo", &["build"]);
        r.env_overrides
            .insert("RUSTC".to_string(), "/custom/rustc".to_string());
        assert_eq!(
            resolve_real_rustc(&r).unwrap(),
            PathBuf::from("/custom/rustc")
        );
    }

    #[test]
    fn resolve_real_cc_respects_an_explicit_cc_override() {
        let mut r = root("nim", &["c", "main.nim"]);
        r.env_overrides
            .insert("CC".to_string(), "/custom/cc".to_string());
        assert_eq!(resolve_real_cc(&r).unwrap(), PathBuf::from("/custom/cc"));
    }

    #[test]
    fn detect_requested_toolchain_selector_prefers_explicit_rustc_over_toolchain_arg() {
        let mut r = root("cargo", &["+nightly", "build"]);
        r.env_overrides
            .insert("RUSTC".to_string(), "/custom/rustc".to_string());
        assert_eq!(
            detect_requested_toolchain_selector(&r),
            Some("RUSTC=/custom/rustc".to_string())
        );
    }

    #[test]
    fn detect_requested_toolchain_selector_reads_a_leading_plus_toolchain_arg() {
        let r = root("cargo", &["+nightly", "build"]);
        assert_eq!(
            detect_requested_toolchain_selector(&r),
            Some("cargo +nightly".to_string())
        );
    }

    #[test]
    fn detect_requested_toolchain_selector_is_none_when_nothing_was_requested() {
        let r = root("cargo", &["build"]);
        assert_eq!(detect_requested_toolchain_selector(&r), None);
    }

    #[test]
    fn run_and_record_stores_the_command_as_actually_executed_not_the_original_request() {
        // Regression test for the root_command/ProcessRecord divergence bug:
        // for a plain (non-Cargo, non-Nim) command nothing should be
        // rewritten, so root_command must still equal the caller's request
        // exactly -- verifying this crate didn't just start unconditionally
        // recording *some* mutated command; it records the real effective
        // one, which happens to equal the request here.
        let tmp = std::env::temp_dir().join(format!(
            "laminaria-run-lib-test-plain-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let lock = tmp.join("toolchains.lock.toml");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(&lock, "").unwrap();

        let r = root("true", &[]);
        let (run, _dir) = run_and_record(
            &tmp,
            "test-workload",
            "test-scenario",
            None,
            &lock,
            &tmp,
            r.clone(),
            ProbeLevel::Level1ProcessResource,
            &[],
        )
        .unwrap();

        assert_eq!(run.root_command.program, r.program);
        assert_eq!(run.root_command.args, r.args);
        assert!(run.artifact_delta.is_none());
    }

    #[test]
    fn run_and_record_on_a_cargo_root_records_the_actually_injected_message_format_arg() {
        // Regression test for the two bugs together: the recorded
        // root_command must reflect the actually-inserted
        // --message-format=json (finding 2), and it must land in Cargo's
        // own option region, immediately after the subcommand (finding 3) --
        // not silently absent, and not pushed past a `--` if one were
        // present.
        let tmp = std::env::temp_dir().join(format!(
            "laminaria-run-lib-test-cargo-argv-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let lock = tmp.join("toolchains.lock.toml");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(&lock, "").unwrap();

        // "cargo --version" doesn't actually build anything, so this stays
        // fast and network-free; "--version" itself is not a subcommand
        // this crate recognizes, so use a recognized-but-harmless spelling
        // that also fails fast without a real manifest present, since the
        // point here is only to inspect the *recorded* root_command, not to
        // require a successful build.
        let r = root("cargo", &["check", "--manifest-path", "does-not-exist"]);
        let (run, _dir) = run_and_record(
            &tmp,
            "test-workload",
            "test-scenario",
            None,
            &lock,
            &tmp,
            r,
            ProbeLevel::Level1ProcessResource,
            &[],
        )
        .unwrap();

        assert_eq!(
            run.root_command.args,
            vec![
                "check".to_string(),
                "--message-format=json".to_string(),
                "--manifest-path".to_string(),
                "does-not-exist".to_string(),
            ]
        );
    }

    #[test]
    fn run_and_record_with_observation_roots_populates_artifact_delta() {
        let tmp = std::env::temp_dir().join(format!(
            "laminaria-run-lib-test-observe-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let lock = tmp.join("toolchains.lock.toml");
        let observed = tmp.join("observed");
        std::fs::create_dir_all(&observed).unwrap();
        std::fs::write(&lock, "").unwrap();

        // The target path is passed as a genuine argv entry ($1), not
        // interpolated into the shell script string, and normalized to
        // forward slashes -- a Windows PathBuf's backslashes embedded
        // directly into a double-quoted sh -c script are not portable (MSYS
        // sh's own escaping rules apply to the script text, not to an argv
        // value), which is exactly what broke this test on the `windows`
        // CI job the first time this test was added.
        let created_path = observed
            .join("created.txt")
            .to_string_lossy()
            .replace('\\', "/");
        let r = root("sh", &["-c", "printf hi > \"$1\"", "sh", &created_path]);
        let (run, _dir) = run_and_record(
            &tmp,
            "test-workload",
            "test-scenario",
            None,
            &lock,
            &tmp,
            r,
            ProbeLevel::Level1ProcessResource,
            std::slice::from_ref(&observed),
        )
        .unwrap();

        let delta = run.artifact_delta.expect("artifact_delta must be Some");
        let inventory: artifact_inventory::ArtifactInventory =
            serde_json::from_value(delta).unwrap();
        assert_eq!(inventory.records.len(), 1);
        assert_eq!(
            inventory.records[0].state,
            artifact_inventory::ArtifactState::Created
        );
        assert!(run
            .process_trace
            .known_gaps
            .iter()
            .any(|g| g.contains("artifact inventory (issue #20) captured")));
    }
}
