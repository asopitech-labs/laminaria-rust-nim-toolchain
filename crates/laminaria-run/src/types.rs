//! The versioned `Run` schema, as specified in `docs/measurement-foundation.md`
//! section 5 (issue #19). This is the first permanent-code implementation of
//! the model every later Action Graph / scheduler / `explain-*` path is
//! meant to consume -- see that doc's section 14 for the responsibility
//! this places on the shape of these types.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use laminaria_fingerprint::{EnvironmentFingerprint, ToolchainReport};

/// Schema version of the `Run` records this crate emits. Bump whenever a
/// field is added, renamed, or removed so stored `runs/<run-id>/run.json`
/// files remain interpretable by a later reader
/// (`docs/measurement-foundation.md` section 5).
pub const SCHEMA_VERSION: &str = "0.1.0";

/// The layered-probe model from `docs/measurement-foundation.md` section 6.
/// Attached to every `ProcessRecord` and to `MeasurementOverhead` so a
/// reader knows exactly what capability produced a given field set, rather
/// than assuming every `Run` observed the same depth.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProbeLevel {
    /// Portable lifecycle and wall time only: pid, argv, cwd, start/end,
    /// exit status. No resource accounting.
    Level0Lifecycle,
    /// OS resource/process-tree data (this crate's `tracer` module today):
    /// `wait4`-derived CPU/RSS/I/O/fault/context-switch counters.
    Level1ProcessResource,
    /// Compiler-native telemetry (Cargo `--timings`/JSON messages, rustc
    /// `-Z self-profile`, LLVM pass timing, Nim stage diagnostics). Not yet
    /// implemented by this crate.
    Level2CompilerTelemetry,
    /// An optional platform profiler (`perf`, eBPF, or equivalent). Not yet
    /// implemented by this crate.
    Level3PlatformProfiler,
}

/// The command this Run measures. Environment variables are recorded only
/// as explicit overrides passed to the child, never as a dump of the
/// tracer's own process environment (`docs/measurement-foundation.md`
/// section 4's "must not store secrets" rule applies here too).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootCommand {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env_overrides: BTreeMap<String, String>,
}

/// Resource usage for one `ProcessRecord`. Every field is `Option` and
/// absent fields are named in `unsupported_fields` -- per
/// `docs/measurement-foundation.md` section 6's explicit acceptance
/// criterion, "missing platform fields are explicit null/unsupported
/// states, not fabricated zeros." A field being `None` without a matching
/// entry in `unsupported_fields` means the probe ran and legitimately
/// observed zero (e.g. `major_faults: Some(0)`), which is why the
/// distinction is a separate list rather than overloading `None`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceUsage {
    pub user_cpu_seconds: Option<f64>,
    pub system_cpu_seconds: Option<f64>,
    pub peak_rss_bytes: Option<u64>,
    pub block_input_ops: Option<u64>,
    pub block_output_ops: Option<u64>,
    pub minor_faults: Option<u64>,
    pub major_faults: Option<u64>,
    pub voluntary_context_switches: Option<u64>,
    pub involuntary_context_switches: Option<u64>,
    pub unsupported_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExitStatusRecord {
    pub success: bool,
    pub code: Option<i32>,
    pub signal: Option<i32>,
}

/// One process this Run observed. The first-pass tracer
/// (`crate::tracer::trace_root_command`) populates exactly one of these --
/// the root command itself -- with `resource_usage` covering that
/// process's **entire reaped descendant subtree cumulatively** (verified
/// empirically: `wait4`'s rusage on a direct child aggregates the CPU
/// time/peak-RSS-per-call of any of that child's own already-reaped
/// descendants, on both Linux and Darwin). This is *not* the same claim as
/// "each descendant process is individually recorded" -- see
/// `ProcessTrace::known_gaps`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessRecord {
    pub pid: Option<u32>,
    pub parent_pid: Option<u32>,
    pub executable: Option<PathBuf>,
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Nanoseconds since `Run`'s monotonic clock anchor
    /// (`docs/measurement-foundation.md` section 6's "one Run monotonic
    /// clock" -- never a raw wall-clock timestamp from a different source).
    pub start_elapsed_ns: u64,
    pub end_elapsed_ns: Option<u64>,
    pub exit_status: Option<ExitStatusRecord>,
    pub resource_usage: ResourceUsage,
    pub probe_level: ProbeLevel,
    /// What this record's `resource_usage` scope actually covers, in
    /// plain language (e.g. "cumulative over the root process's entire
    /// reaped descendant subtree via wait4; individual descendant
    /// processes are not separately recorded"). Required reading before
    /// comparing two `ProcessRecord`s -- see the type-level doc comment.
    pub coverage_note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProcessTrace {
    pub processes: Vec<ProcessRecord>,
    /// Explicit, human-readable list of what this Run's process tracing
    /// does *not* yet cover -- e.g. "individual descendant process
    /// enumeration (per-node pid/parent/argv) is not implemented; only the
    /// root command's own record and its aggregated subtree resource
    /// usage are captured." Required so a reader never infers full
    /// tree-walk fidelity from the mere presence of a `ProcessTrace`.
    pub known_gaps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResult {
    pub success: bool,
    pub root_exit_status: ExitStatusRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeasurementOverhead {
    pub probe_level: ProbeLevel,
    /// Wall time spent in this crate's own tracer bookkeeping (process
    /// spawn/reap/record-building), not the measured command's own
    /// runtime. Compare against a Level 0-only Run of the same scenario to
    /// get the overhead delta `docs/measurement-foundation.md` section 12
    /// requires -- this crate does not yet compute that delta itself.
    pub tracer_overhead_seconds: Option<f64>,
    pub notes: Vec<String>,
}

/// Preparation performed before the timed root command ran (toolchain
/// selection, cache clears) -- `docs/measurement-foundation.md` sections 9
/// and 10. Not yet populated by this crate's tracer; present in the schema
/// so a caller building on top of `laminaria-run` has somewhere to record
/// it without a schema migration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PreparationRecord {
    pub steps: Vec<String>,
    pub cache_clears: Vec<String>,
}

/// `docs/measurement-foundation.md` section 10. Not yet populated -- see
/// `PreparationRecord`'s doc comment for the same reasoning.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CacheState {
    pub notes: Vec<String>,
}

/// The complete, versioned Run envelope
/// (`docs/measurement-foundation.md` section 5). `environment_fingerprint`
/// and `resolved_toolchain_fingerprint` reuse `laminaria-fingerprint`'s
/// own schema (issue #18) directly rather than re-deriving environment
/// identity -- the whole point of that crate existing first.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub run_id: String,
    pub schema_version: String,
    pub workload_id: String,
    pub scenario_id: String,
    pub requested_artifact: Option<String>,
    pub environment_fingerprint: EnvironmentFingerprint,
    pub requested_toolchain_selector: Option<String>,
    pub resolved_toolchain_fingerprint: Option<ToolchainReport>,
    pub preparation_record: PreparationRecord,
    pub cache_state: CacheState,
    pub root_command: RootCommand,
    /// Unix nanoseconds at Run start -- a human/log-correlation anchor
    /// only. Every other timestamp in this Run is relative to this point
    /// on the monotonic clock (`ProcessRecord::start_elapsed_ns` etc.),
    /// never derived by re-reading the wall clock a second time.
    pub run_started_at_unix_ns: u128,
    pub run_ended_at_unix_ns: Option<u128>,
    pub result: Option<RunResult>,
    pub process_trace: ProcessTrace,
    /// Compiler-native telemetry (Level 2). Always `None` from this
    /// crate today -- see `ProbeLevel::Level2CompilerTelemetry`.
    pub compiler_telemetry: Option<serde_json::Value>,
    /// Artifact inventory deltas (`docs/measurement-foundation.md`
    /// section 8). Always `None` from this crate today.
    pub artifact_delta: Option<serde_json::Value>,
    pub measurement_overhead: Option<MeasurementOverhead>,
}

/// A derived, regeneratable view of a `Run` -- `docs/measurement-foundation.md`
/// section 5's `summary.json`. Deliberately holds nothing that isn't
/// reconstructible from `run.json` alone
/// (`crate::store::regenerate_summary`), so the "raw evidence can
/// regenerate summary.json without rerunning the workload" acceptance
/// criterion is a property of this type's construction, not a promise
/// upheld by convention.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Summary {
    pub run_id: String,
    pub schema_version: String,
    pub workload_id: String,
    pub scenario_id: String,
    pub success: Option<bool>,
    pub wall_seconds: Option<f64>,
    pub root_user_cpu_seconds: Option<f64>,
    pub root_system_cpu_seconds: Option<f64>,
    pub root_peak_rss_bytes: Option<u64>,
    pub process_record_count: usize,
    pub known_gaps: Vec<String>,
}

/// One `"reason": "compiler-artifact"` message from Cargo's real JSON
/// message stream (`--message-format=json`), reduced to the fields
/// `docs/measurement-foundation.md` section 8 (artifact inventory) and
/// section 10 (explicit cache state) actually need. Field names and shape
/// verified against Cargo's own source
/// (`.reference/cargo/src/util/machine_message.rs`'s `Artifact` struct),
/// not assumed from documentation -- see `crate::cargo_telemetry`'s module
/// doc for what was checked and how.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilerArtifactRecord {
    pub package_id: String,
    pub target_name: String,
    pub target_kind: Vec<String>,
    pub filenames: Vec<PathBuf>,
    pub executable: Option<PathBuf>,
    /// Cargo's own per-crate cache-state signal: `true` means this
    /// specific crate's artifact was already up to date and was not
    /// recompiled. Far more precise than inferring cache state from
    /// whole-build CPU time, since it's reported per compilation unit,
    /// straight from Cargo's own dependency-freshness check -- not
    /// derived or inferred by this crate at all.
    pub fresh: bool,
}

/// Level 2 compiler-native telemetry for a Cargo build
/// (`docs/measurement-foundation.md` section 7's "Rust / Cargo" adapter).
/// Populated from Cargo's real `--message-format=json` output by
/// `crate::cargo_telemetry::parse_cargo_json_messages`, stored on
/// `Run::compiler_telemetry` as a generic `serde_json::Value` (that field
/// stays adapter-agnostic in the schema; this is Cargo's own JSON shape).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CargoCompilerTelemetry {
    pub artifacts: Vec<CompilerArtifactRecord>,
    /// From the one `"reason": "build-finished"` message Cargo emits at
    /// the end of a successful parse of its own message stream. `None`
    /// when that message was never seen (e.g. the traced command crashed
    /// before Cargo could emit it, or wasn't given `--message-format=json`
    /// at all).
    pub build_finished_success: Option<bool>,
    pub compiler_message_count: usize,
    pub build_script_executed_count: usize,
    /// Lines from the traced command's stdout that were not one of
    /// Cargo's own known message `reason`s or failed to parse as JSON at
    /// all -- explicit, not silently dropped, so a reader can tell
    /// "zero artifacts" apart from "this wasn't actually a
    /// --message-format=json stream".
    pub unparsed_line_count: usize,
}

/// Level 2 compiler-native telemetry for a `nim c`/`nim cpp` build
/// (`docs/measurement-foundation.md` section 7's "Nim / Nimony" adapter).
/// A deliberately different shape from `CargoCompilerTelemetry` -- not
/// shoehorned into the same struct -- because it comes from a
/// structurally different source: Nim has no `--message-format=json`
/// equivalent for compile-stage events (only `nim dump --dump.format:json`,
/// a *separate*, static-config-only command, checked directly and found
/// unrelated to per-build telemetry -- see `crate::nim_telemetry`'s module
/// doc). This is parsed from Nim's human-oriented hint/verbosity output on
/// **stderr** (Cargo's machine messages are on stdout -- a real,
/// confirmed difference between the two toolchains' conventions, not an
/// assumption carried over from the Cargo adapter), so it carries a
/// materially weaker reliability claim: Nim makes no documented
/// backward-compatibility promise about this output's exact text, unlike
/// Cargo's `--message-format=json`, which is an explicit stable machine
/// interface.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NimCompilerTelemetry {
    /// Module names from `hintCC` ("CC: <module>") lines, in the order
    /// Nim's frontend processed them -- one per module that reached
    /// C-codegen, not one per file Nim merely parsed/imported.
    pub processed_modules: Vec<String>,
    /// Whether a `hintLinking` ("Hint: ... [Link]") line was seen.
    pub linked: bool,
    /// From the final `hintSuccessX` summary line
    /// (`compiler/lineinfos.nim`'s `hintSuccessX` format string,
    /// `$loc lines; ${sec}s; $mem; proj: $project; out: $output`).
    pub lines_compiled: Option<u64>,
    /// Nim's own self-reported wall-clock compile time, from the same
    /// summary line -- independent of this crate's own `wait4`/`Instant`
    /// measurements, useful as a cross-check but not a replacement for
    /// them (Nim's own clock, not this Run's monotonic clock).
    pub self_reported_seconds: Option<f64>,
    /// Peak memory in bytes, parsed from the summary line's
    /// human-formatted size (`strutils.formatSize`, which picks
    /// B/KiB/MiB/GiB depending on magnitude) back into a plain byte
    /// count.
    pub peak_mem_bytes: Option<u64>,
    /// Lines from the traced command's stderr that this parser recognized
    /// no hint category for -- explicit, not silently dropped, same
    /// reasoning as `CargoCompilerTelemetry::unparsed_line_count`.
    pub unrecognized_line_count: usize,
}
