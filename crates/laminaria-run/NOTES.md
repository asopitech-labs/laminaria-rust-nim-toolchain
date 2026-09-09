# `laminaria-run` — issue #19 findings and verification log

Implements a first, honest slice of `docs/measurement-foundation.md`'s
Run schema and process/resource tracer. This file records the empirical
checks the design leans on, and precisely what's covered vs. still open
— see `src/lib.rs`'s own doc comment for the summary, and each module's
doc comment for its own scope.

## `wait4`'s rusage on a direct child aggregates that child's own reaped descendants

Load-bearing for the whole Level 1 design: if this weren't true, tracing
only the root command (`cargo`, `nim`) would say nothing about the real
work done by its descendants (`rustc`, the linker, the system C
compiler nim's `nim c` route spawns).

Checked with a standalone C probe before writing any Rust code
(`fork()` a child that itself `fork()`s a grandchild, burns ~0.3s of
CPU in the grandchild, `wait()`s on it, then exits; the parent
`wait4()`s the child and reports the rusage it gets back):

```
child ru_utime=0.336745s ru_stime=0.001952s maxrss=999424KB
```

The ~0.3s shows up on the *child's* rusage even though the child itself
did essentially no work — confirming `wait4` aggregates through the
reaping chain, not just the directly-terminated process's own usage.
Re-verified separately through the real Rust path this crate ships,
not just the throwaway C probe: `tracer::resource_usage_aggregates_a_grandchild_process_cumulatively`
spawns `sh -c "sh -c '<busy loop>'"` through `trace_root_command` and
asserts non-trivial CPU shows up on the single returned `ProcessRecord`.

**What this does not mean**: it is not per-node attribution. If `cargo`
spawns three `rustc` invocations, this design reports their combined
CPU/RSS-peak-per-call as one number on `cargo`'s own record — it cannot
say which of the three used how much. See `ProcessTrace::known_gaps`,
populated on every `Run` this crate produces (not left as an implicit,
undocumented limitation).

## Manually reaping via `wait4`, then dropping `std::process::Child`, is safe

Rust's `Child` does not itself call `waitpid` on drop (unlike some other
languages' process-handle types) — but this was verified rather than
relied on from memory, since getting it wrong would mean either a
double-reap race or a leaked zombie:

```
wait4 returned 70588, pid=70588
WIFEXITED=true WEXITSTATUS=7
rusage.ru_utime=0.002360
child dropped cleanly after manual wait4 reap
```

`tracer::trace_root_command` relies on exactly this: it calls `wait4`
directly via `libc`, bypassing `Child::wait()`/`try_wait()` entirely,
then lets `Child` drop normally.

## `ru_maxrss`'s unit is platform-specific — Linux: kilobytes, Darwin: bytes

Not assumed from documentation (which is easy to misremember or find
conflicting claims about) — checked against this repo's own values. A
trivial test binary's `ru_maxrss` read as `999424` on this dev machine
(Darwin/arm64). Treated as bytes, that's ~976KB — plausible for a
minimal C binary's default stack/heap footprint. Treated as kilobytes,
that's ~976MB — implausible for the same binary. `tracer.rs` converts
per-platform accordingly (`#[cfg(target_os = "linux")]` multiplies by
1024; `#[cfg(target_os = "macos")]` does not), and any other Unix target
explicitly marks `peak_rss_bytes` as unsupported rather than guessing.

## Cold vs. true-no-op CPU attribution — real, checked in CI, not just plumbing

The point of Level 1 tracing is to actually *distinguish* scenarios, not
merely run without crashing. Checked directly by tracing the same
`rust-heavy-workspace` build twice — once forced cold (`rm -rf target`
first), once immediately after (a true no-op) — and asserting the cold
run's cumulative user CPU clearly exceeds the no-op's. Confirmed on both
CI platforms (run `34336163081`):

```
ubuntu-latest: cold build user_cpu_seconds=0.225346, true-noop user_cpu_seconds=0.009147
macos-latest:  cold build user_cpu_seconds=0.474587, true-noop user_cpu_seconds=0.013473
```

A ~25-35x difference on both platforms — the tracer is genuinely
attributing `rustc`/linker descendant work to the traced root command,
not just measuring `cargo`'s own dispatch overhead (which is what the
no-op number alone represents).

## What issue #19's acceptance criteria this crate satisfies today, and what it doesn't

Checked against the issue's actual acceptance-criteria list, not a
paraphrase of it:

- [x] Run/process schemas are versioned (`types::SCHEMA_VERSION`,
  carried in every `Run`).
- [ ] **Parent/child process relationships are preserved** — NOT met by
  this first pass. Only the root command's own `ProcessRecord` exists;
  individual descendant pid/parent/argv/timing is not enumerated. This
  is the single largest remaining gap and the natural next slice of
  work (would need `/proc` polling on Linux, `libproc`/`proc_listchildpids`
  on macOS, or ptrace-based tracing for real-time attach).
- [x] Major wall/CPU/memory/I/O fields are captured where the host
  exposes them (root-command-cumulative, not per-node — see above).
- [x] Missing platform fields are explicit null/unsupported states, not
  fabricated zeros (`ResourceUsage::unsupported_fields`).
- [x] All events can be correlated to one monotonic Run clock
  (`clock::RunClock`; `elapsed_ns_is_monotonically_non_decreasing_across_calls`
  test).
- [x] Failure/cancellation does not discard partial evidence (CI
  Experiment 5: exit code, resource usage, stdout/stderr all present
  for a deliberately failing command).
- [ ] **Minimal and resource-traced modes can be compared for observer
  overhead** — NOT implemented. There is only one tracer path
  (Level 1); no Level-0-only mode exists to diff against, so
  `MeasurementOverhead.tracer_overhead_seconds` records this crate's
  own bookkeeping cost but not a cross-level delta.
- [x] Raw evidence can regenerate `summary.json` without rerunning the
  workload (`store::regenerate_summary_from_disk`, exercised as a real
  disk round trip both in unit tests and in CI against genuine
  CI-produced Runs).
- [ ] **The implementation can later accept dynamic child actions from
  #15** — not evaluated; #15 is not yet implemented, so this is
  unverifiable either way today.

Also not implemented, beyond the checklist: Level 2 compiler-native
telemetry adapters (Cargo `--timings`/JSON messages, rustc
`-Z self-profile`, Nim stage diagnostics), Level 3 platform profiler
integration, artifact inventory (section 8), and
`PreparationRecord`/`CacheState` population (sections 9-10) — the
schema has fields for these so a later implementation doesn't need a
migration, but nothing populates them yet.
