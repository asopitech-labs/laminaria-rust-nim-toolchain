# `laminaria-run` — issue #19 findings and verification log

## Evidence classification correction (2026-09-10)

This file preserves historical fixture/measurement and implementation evidence, not the current research delivery order. Existing-compiler builds and driver self-builds recorded below are **reference/bootstrap/delegated-build baselines**, not proof of LAMINARIA compiler ownership or independent self-hosting. The [compiler ownership contract](../../docs/compiler-ownership-contract.md) governs current issue acceptance; historical checklists do not close the revised requirements.


Implements a first, honest slice of `docs/measurement-foundation.md`'s
Run schema and process/resource tracer. This file records the empirical
checks the design leans on, and precisely what's covered vs. still open
— see `src/lib.rs`'s own doc comment for the summary, and each module's
doc comment for its own scope.

## Wrapper substitution, not process-tree walking — studied from a real reference project, not re-derived

The obvious first instinct for "parent/child process relationships are
preserved" (issue #19's acceptance criteria) is OS-level process-tree
walking: `ptrace`, `/proc` polling on Linux, `libproc` on macOS. Before
writing any of that, `docs/measurement-foundation.md` section 16's named
reference project — `rust-lang/rustc-perf` — was actually cloned and its
real collector source read (not re-derived from general knowledge of the
problem domain). It does **not** do OS-level tree walking anywhere in its
production collector (`collector/src/`, `collector/benchlib/src/`
searched directly for `ptrace`/`/proc`/`procfs` — zero matches).

What it actually does (`collector/src/bin/rustc-fake.rs`,
`collector/src/compile/execute/mod.rs`): overrides Cargo's `RUSTC`
environment variable to point at a thin wrapper binary
(`.env("RUSTC", &*FAKE_RUSTC)`, with the real compiler passed alongside
as `RUSTC_REAL`). Cargo then invokes the wrapper once per compilation
unit, believing it *is* rustc. The wrapper runs the real compiler as its
own child, measures it with `getrusage(RUSAGE_CHILDREN)` after the child
exits, then forwards the result.

**Verified this actually works on this project's own fixture before
building anything Rust-side**, with a two-line shell stand-in:

```bash
cat > /tmp/fake-rustc.sh <<'EOF'
#!/usr/bin/env bash
echo "$(date +%s%N) pid=$$ args=$*" >> /tmp/fake-rustc-invocations.log
exec "$REAL_RUSTC" "$@"
EOF
RUSTC=/tmp/fake-rustc.sh cargo build --manifest-path fixtures/rust-heavy-workspace/Cargo.toml --workspace
```

Produced one log line per real compilation unit (`fixture-core`,
`fixture-mid`, `fixture-bin`) plus Cargo's own `-vV`/probe invocations,
each with a distinct pid, in dependency order — confirming the mechanism
before investing in `laminaria-rustc-wrapper` (`src/bin/rustc_wrapper.rs`)
and `cargo_wrapper.rs`'s env-var/JSONL-event-file protocol.

**What this technique gives, precisely**: one `ProcessRecord` per real
`rustc` invocation, with its own pid, argv, and `wait4`-derived resource
usage (reusing `tracer::reap` directly — the exact same, already-verified
logic, not a second implementation). Linker cost is *not* separately
recorded, matching `rustc-fake`'s own accepted scope: the reaping-chain
aggregation `tracer.rs` already established (a process's `wait4` result
includes its own already-reaped descendants' usage) means the linker's
cost rolls up into whichever `rustc` invocation spawned it.

**What this technique does not give**: it's Cargo/rustc-specific. There
is no equivalent wrapping for Nim's compiler, arbitrary shell commands,
or any other toolchain yet — those still only get the root command's
single cumulative record, exactly as before. `Run::process_trace.known_gaps`
says this explicitly on every Run, keyed off whether any wrapper events
were actually recorded, not left as a blanket claim.

**Verified end-to-end against a real Run, not just the shell stand-in**:
tracing `cargo build --workspace` on `fixtures/rust-heavy-workspace`
(3-crate dependency chain) via `laminaria run` produces 7 `ProcessRecord`s
— 1 root + 6 wrapper-recorded `rustc` invocations (two `-vV`/probe calls,
one metadata probe, then `fixture_core`, `fixture_mid`, `fixture_bin`) —
each with a distinct pid and its own resource usage. `fixture_bin`'s
record (0.112s user CPU) costs roughly double `fixture_core`'s (0.056s)
and `fixture_mid`'s (0.057s), consistent with it being the one crate that
also triggers the final link. All records' `start_elapsed_ns`/
`end_elapsed_ns` fall correctly within the root record's own span on the
same shared `RunClock`, confirming cross-process event correlation
actually works, not just compiles. Confirmed again in CI (see the
"Trace a cold and a true-no-op rust-heavy-workspace build" step's
per-rustc-invocation assertions), on both platforms this project targets
(run `34343348319`): `ubuntu-latest` and `macos-latest` each recorded 4
total records (1 root + 3, one per `fixture-core`/`fixture-mid`/
`fixture-bin`), all with distinct pids and populated resource usage.

**A real gap this exact CI step caught on the first push, not a
theoretical concern**: the initial push (`34342837526`) failed on
`ubuntu-latest` with only 1 process record instead of the expected 4+.
Root cause, confirmed locally with a clean `target/`: `cargo test
--workspace` (which runs earlier in the CI job) compiles a
separately-hashed test-harness copy of `laminaria-rustc-wrapper` under
`target/debug/deps/`, not the plain `target/debug/laminaria-rustc-wrapper`
`find_rustc_wrapper_binary()` looks for. Wrapper substitution correctly,
silently fell back to root-only tracing -- the designed
graceful-degradation path worked exactly as intended, but the CI
assertions weren't written to tolerate it. Fixed by adding an explicit
`cargo build --workspace` step before relying on the wrapper binary.

### The cross-process clock correlation trade-off

A wrapper invocation is a separate OS process from the outer `laminaria
run` process, so it cannot share the outer `RunClock`'s `Instant`-based
anchor directly (`Instant` has no cross-process representation). The
outer Run's clock anchor is passed to the wrapper as a wall-clock
timestamp (`LAMINARIA_RUN_CLOCK_ANCHOR_UNIX_NS`, `SystemTime`-based
nanoseconds since the epoch), and the wrapper computes its own
`elapsed_ns` values as wall-clock deltas against that anchor. This is a
real, accepted trade-off, not an oversight: wall-clock deltas lack
`Instant`'s monotonicity guarantee under a clock adjustment mid-Run,
whereas the outer command's own root record stays fully `Instant`-based
and monotonic within its own process. Recorded explicitly in both
`cargo_wrapper.rs`'s doc comment and every wrapper-recorded
`ProcessRecord.coverage_note`, not left implicit.

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
This same aggregation property is also what `rustc-fake`'s
`getrusage(RUSAGE_CHILDREN)` approach implicitly relies on (see above)
— an independent confirmation that this empirical finding matches a
real, widely-used project's own design assumption.

**What this does not mean**: it is not per-node attribution on its own.
Without the wrapper-substitution technique above, if `cargo` spawns
three `rustc` invocations, this design reports their combined
CPU/RSS-peak-per-call as one number on `cargo`'s own record — it cannot
say which of the three used how much. See `ProcessTrace::known_gaps`,
populated on every `Run` this crate produces (not left as an implicit,
undocumented limitation) — now conditioned on whether wrapper events
were actually recorded for that particular Run.

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

`tracer::trace_root_command` (and now `src/bin/rustc_wrapper.rs`, which
reuses the same `tracer::reap` function) relies on exactly this: it calls
`wait4` directly via `libc`, bypassing `Child::wait()`/`try_wait()`
entirely, then lets `Child` drop normally.

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

## Concurrent-append safety for the wrapper events file

Cargo parallelizes independent `rustc` invocations across build jobs, so
multiple `laminaria-rustc-wrapper` processes append to the same
`rustc-invocations.jsonl` events file concurrently. Relied on: a
`write(2)` to an `O_APPEND`-opened file descriptor is atomic with respect
to other writers on the same local file, provided the whole line is
written in a single `write` syscall — true here, since
`cargo_wrapper::append_event` formats the complete JSON line in memory
before one `write_all` call. Exercised with concurrent writers in
`cargo_wrapper::tests::concurrent_appends_never_interleave_a_line`
(threads, not a full separate-process harness — a faithful enough stand-in
since the atomicity guarantee is a property of the file descriptor/kernel
write path, not of whether the caller is a thread or a process). Also
confirmed indirectly by the real multi-crate CI/local runs above:
`fixture-core`/`fixture-mid`/`fixture-bin` each got their own
uncorrupted, individually-parseable JSON line even though Cargo may run
some of these concurrently depending on the dependency graph and job
count.

## Level 0 (minimal wrapper) vs. Level 1 (resource tracing) overhead — measured, not asserted

Issue #19 Experiment 6 and `docs/measurement-foundation.md` section 12
both ask for observer overhead to be measured, not just implemented
around. `tracer::trace_root_command_level0` (`std::process::Child::wait`,
no `wait4`/`libc` at all) is the "minimal wrapper" baseline; `laminaria
run --probe-level {level0,level1}` selects it or the existing Level 1
tracer. At Level 0, Cargo/Nim wrapper substitution is skipped entirely
too (it's itself extra instrumentation the baseline shouldn't carry).

Measured directly, both sides of the comparison from real CLI
invocations (`measurement_overhead.tracer_overhead_seconds`, this
crate's own wall time around spawn/wait-or-reap/record-building — which
includes the traced command's real work too, so the comparison that
isolates overhead is the *delta between paired runs of the identical
command*, not either number in isolation):

Trivial command (`sh -c "echo hi"`, 5 runs each, local arm64 macOS):

```
level0: 0.00319, 0.00321, 0.00329, 0.00319, 0.00327  (mean ~0.00323s)
level1: 0.00327, 0.00326, 0.00342, 0.00327, 0.00338  (mean ~0.00332s)
```

Real fixture (`cargo build --workspace` on `rust-heavy-workspace`, true
no-op state, 3 runs each — first run of each set excluded as a cold-cache
outlier, ~2.4s/0.76s respectively, clearly not steady-state):

```
level0 (steady state): 0.034684, 0.034570  (mean ~0.03463s)
level1 (steady state): 0.034736, 0.035079  (mean ~0.03491s)
```

**Result**: Level 1's `wait4`-based resource accounting adds roughly
1-3% wall-time overhead over Level 0's lifecycle-only tracing, on both a
trivial command and a real no-op Cargo rebuild, on this platform. Not
zero, but small relative to a single extra syscall's noise floor at this
sample size — a genuine measurement, not a claim that the overhead is
negligible by design. `docs/measurement-foundation.md` section 11's own
guidance (store every raw sample, don't trust one wall-clock number)
applies here too: this is 3-5 samples on one machine, not a rigorous
statistical claim — sufficient to demonstrate the comparison is now
*possible and produces a real number*, not sufficient as a final,
citable overhead figure.

**CI confirms the comparison runs cleanly on both platforms (run
`34347389399`), and also confirms why a single sample isn't trustworthy**:
a single paired level0/level1 run of the same `rust-heavy-workspace` true
no-op rebuild gave `ubuntu-latest: level0=0.0491s level1=0.0751s`
(~53% higher) and `macos-latest: level0=0.0919s level1=0.1105s` (~20%
higher) — both a much larger relative gap than this dev machine's
steady-state 3-5-sample average (~1-3%). Recorded as-is rather than
discarded as inconvenient: shared CI runners carry more scheduling
noise than a quiet dev machine, and one sample per platform cannot
distinguish real overhead from that noise. This is itself evidence for,
not against, section 11's repeated-sampling requirement — the CI step
demonstrates the comparison *mechanism* works on both platforms; it does
not by itself produce a trustworthy overhead figure, and isn't claimed to.

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

## Level 2 compiler telemetry for Nim — no JSON stream exists, so Nim's own hint output is parsed instead

`docs/measurement-foundation.md` section 7's "Nim / Nimony" adapter
explicitly anticipates this case: *"do not assume Nim 2 and Nimony
expose the same adapter/capability set"* as Cargo. Searched Nim's real
compiler source (`.reference/Nim/compiler/`) for a `--message-format=
json`-equivalent stream before writing anything, the same way as the
Cargo adapter. There isn't one.

`nim dump --dump.format:json` exists (`compiler/main.nim`'s `cmdDump`
handling) but is a **separate command** dumping static configuration
(version, search paths, defined symbols, enabled hints/warnings) —
confirmed by reading its actual field list, not assumed from the name.
It has nothing to do with per-build compile-stage telemetry.

What Nim does expose natively, unconditionally, is its human-oriented
hint stream (`compiler/lineinfos.nim`'s hint category table:
`hintCC = "CC"` → `"CC: $1"` per module reaching C-codegen, `hintLinking`,
and the final `hintSuccessX` summary line, format string
`"$build\n$loc lines; ${sec}s; $mem; proj: $project; out: $output"`).
This is exactly the *"use ... wrappers when native telemetry is
insufficient"* fallback the design doc names — `nim_telemetry.rs`
parses this stream instead of a structured JSON one.

### A confirmed difference from the Cargo adapter — stdout vs. stderr

Captured `nim c`'s stdout and stderr to separate files and found stdout
**empty** — Nim's entire hint stream, including the final summary line,
goes to stderr. `cargo_telemetry::parse_cargo_json_messages` reads
`stdout.log`; `nim_telemetry::parse_nim_hint_stream` reads `stderr.log`.
Not assumed to match Cargo's convention; checked directly.

### A materially weaker reliability claim, stated explicitly rather than glossed over

`--message-format=json` is Cargo's documented, stable machine interface.
Nim's hint text carries no such contract — nothing prevents a future Nim
release from rewording `"CC: $1"` or the `hintSuccessX` format string.
`types::NimCompilerTelemetry`'s own doc comment states this difference
explicitly, and every `Run`'s `known_gaps` repeats it when Nim telemetry
was captured, rather than presenting Cargo- and Nim-sourced telemetry as
equally trustworthy.

### Peak-memory byte conversion — reversing Nim's own formatting function, checked against its doctests

The summary line reports memory as e.g. `"38.184MiB peakmem"` —
human-formatted by `strutils.formatSize` (binary, 1024-based IEC
prefixes: B/KiB/MiB/GiB). `nim_telemetry::extract_peak_mem_bytes`
reverses this back to a plain byte count. The unit set and base (1024,
not 1000) were confirmed against `formatSize`'s own doctests in
`.reference/Nim/lib/pure/strutils.nim`
(`doAssert formatSize((1'i64 shl 31) + (300'i64 shl 20)) == "2.293GiB"`),
not assumed from the field name alone.

### Verified end-to-end on the real fixture, then in CI on both platforms

```
nim-heavy-workspace cold build: 8 modules reaching C-codegen, linked=true,
  29436 lines compiled, ~40MB peak memory (local, arm64 macOS)
```

Confirmed in CI (run `34350673357`) on both platforms, with genuinely
different numbers reflecting each platform's own compile path — not
identical copies:

```
ubuntu-latest: 8 modules, linked=true, 26773 lines, 33271316 peak_mem_bytes
macos-latest:  8 modules, linked=true, 29436 lines, 40009465 peak_mem_bytes
```

The differing `lines_compiled` between platforms (26773 vs 29436) is
itself informative, not a bug: Nim's stdlib module set compiled in
differs slightly by platform (different `system`/`os`-conditional code
paths), so a genuinely different line count is the *correct*, expected
result — not something to normalize away.

## Level 2 compiler telemetry for Cargo — Cargo's own JSON messages, studied from Cargo's real source

`docs/measurement-foundation.md` section 7 names "Cargo JSON messages may
assist artifact/process relationships" as the concrete Level 2 adapter
for Rust/Cargo. Studied from Cargo's actual source
(`.reference/cargo/src/util/machine_message.rs`) before implementing
anything: the `Message` trait and its `Artifact`/`FromCompiler`/
`BuildScript`/`BuildFinished` structs are the real, canonical schema
`--message-format=json` emits — not reconstructed from documentation.

Two fields turned out to matter well beyond "assist artifact/process
relationships":

- `Artifact.filenames`: the real, Cargo-reported paths of every artifact
  a compilation unit produced. Directly relevant to section 8's artifact
  inventory — though only partially: no size, content digest, or
  create/change/delete state, since this crate doesn't diff filesystem
  state before/after the Run. Recorded as a partial contribution, not
  conflated with a completed artifact inventory (`known_gaps` says so
  explicitly on every Run with telemetry).
- `Artifact.fresh`: a **per-crate** boolean, straight from Cargo's own
  dependency-freshness check — not inferred or derived by this crate at
  all. This is a precise section-10 cache-state signal, strictly better
  than the whole-build CPU-time heuristic used elsewhere in this
  project (the cold-vs-no-op CPU comparison above): it answers "was
  *this specific crate* rebuilt," not "did the aggregate CPU cost look
  small."

### `--message-format=json` is not safe to inject unconditionally — checked, not assumed

`cargo clean --message-format=json` genuinely errors:
`unexpected argument '--message-format' found`. Checked directly (along
with `build`, `check`, `doc`, all of which accept the flag cleanly)
before deciding on an allowlist (`cargo_telemetry::CARGO_MESSAGE_FORMAT_SUBCOMMANDS`)
rather than injecting the flag for every Cargo invocation. `test`/`run`/
`bench` are included in the allowlist on the reasoning that they share
Cargo's compilation path with `build`/`check`, but were not independently
run and checked the same way — noted as an assumption, not presented as
equally verified.

### Cargo writes these messages to stdout specifically — confirmed by observation, not by reading a spec

No new capture mechanism was needed: `tracer::trace_root_command` already
writes the traced command's stdout to `stdout.log`, and inspecting that
file after enabling `--message-format=json` showed clean JSON lines with
no interleaved human-readable "Compiling .../Finished ..." text (that
goes to stderr, captured separately, unparsed by this crate). Confirmed
by direct observation of the actual file contents, not by trusting that
Cargo's stdout/stderr split matches every other CLI tool's convention.

### Two real bugs caught by this module's own tests, not shipped silently

- `should_inject_message_format`'s first implementation looked for "the
  first argument not starting with `-`" as the subcommand, which matched
  a *flag's value* (e.g. the `x` in `--manifest-path x check`) before the
  real subcommand. Caught by a test exercising exactly that argument
  order, which failed on first run. Fixed by simplifying to `args[0]`
  (matches this crate's own actual call pattern, `laminaria run --
  cargo <subcommand> ...`, subcommand always first) and documenting the
  narrower, less general behavior explicitly rather than fixing it to be
  fully general.
- The test helper generating temp file paths derived the filename from
  the number of lines written; two different tests that happened to write
  the same number of lines got the same path, and running in parallel
  (the default), one clobbered the other's file mid-test — a nondeterministic
  failure, not a consistent one, caught by the test suite failing on one
  run's `cargo test` invocation. Fixed with an atomic counter alongside the
  process id, not just accepting one test file per `#[test]` "seems
  probably fine."

### Verified end-to-end on the real fixture, both cold and no-op, then in CI on both platforms

```
cold build:      3 artifacts reported, 0 fresh, build_finished_success=true
true-noop rebuild: 3 artifacts reported (same 3 crates), 3 fresh
```

Confirmed identically in CI (run `34348892499`) on both `ubuntu-latest`
and `macos-latest`: `Level 2 compiler telemetry: cold 0/3 fresh,
true-noop 3/3 fresh` on both.

## Nim's own analog to Cargo's RUSTC — studied from Nim's real compiler source, not assumed

Extending per-invocation tracing to `nim c`/`nim cpp` builds meant first
answering: does Nim have anything like Cargo's `RUSTC` env var? Checked
against Nim's actual compiler source
(`.reference/Nim/compiler/extccomp.nim`, cloned locally for exactly this
kind of question — see that directory's own `README.md`), not assumed
from general knowledge of `nim c`.

Found the real mechanism: `getCompileCFileCmd` resolves the compiler
executable as `getConfigVar(conf, c, ".exe")` first — a command-line/
config-file override keyed on the *selected compiler's name*
(`--<ccname>.exe:<path>`) — falling back to `getCompilerExe`'s `CC`/`CXX`
env-var reading only when `--cc:env` is explicitly selected as the
compiler itself. `getLinkCmd`'s linker resolution uses the identical
mechanism with a `.linkerexe` suffix.

**A real dead end, hit and diagnosed before the working approach, not
skipped past**: `--cc:env` (the more obvious "just override CC" route)
was tried first and produced a genuine compile error on this project's
own `nim-heavy-workspace` fixture — `undeclared identifier:
'atomicStoreN'` — because `ccEnv`'s command template is generic and
lacks the clang-specific atomics-detection flags the normal `clang`
profile supplies. `--clang.exe:<path>` (keeping the `clang` profile
selected, only redirecting its executable) avoided this entirely and
compiled/linked/ran correctly.

`nim_wrapper.rs` sets both `--clang.exe`/`--clang.linkerexe` and
`--gcc.exe`/`--gcc.linkerexe` unconditionally — only whichever Nim
actually selected as `conf.cCompiler` is ever read (confirmed by reading
`getConfigVar`'s implementation, which looks up only the currently
selected compiler's name), so setting the unused profile's override is a
harmless no-op, not a silent risk.

**Better coverage than the Cargo/rustc case, for a structural reason, not
extra effort**: because Nim's `.exe`/`.linkerexe` config vars separately
cover compiling and linking, `laminaria-cc-wrapper` records the link step
as its own `ProcessRecord` too — something the RUSTC-wrapper approach
cannot do, since Cargo has no equivalent per-link-step hook and the
linker's cost there only reaches `laminaria-run` by rolling up into
whichever `rustc` invocation spawned it.

Verified end-to-end, both locally and in CI (run `34345606184`, both
platforms): tracing `nim c` on `nim-heavy-workspace` produces 10-11
`ProcessRecord`s (platform-dependent: 11 on `ubuntu-latest` (gcc), 10 on
`macos-latest` (clang) — the stdlib module set each platform's default
compiler profile pulls in differs slightly), each with a distinct pid and
populated resource usage, and the produced binary still runs correctly.

**Compiler-real-path resolution, a named limitation**: the wrapper is
told which real compiler to forward to via a single `cc`-on-`PATH`
resolution, done once by `laminaria run` itself — correct exactly when
`cc` matches whichever compiler Nim actually auto-selected (true for
`gcc`-default Linux and `clang`-default macOS, this project's own two CI
platforms), not a universal guarantee for an unusual cross-compilation
setup where Nim might select a compiler `cc` on `PATH` doesn't match.
Not silently assumed safe: if that mismatch ever happens, the wrapper
would forward to the wrong binary — recorded here as a real, specific,
not-yet-hit limitation rather than glossed over.

## What issue #19's acceptance criteria this crate satisfies today, and what it doesn't

Checked against the issue's actual acceptance-criteria list, not a
paraphrase of it:

- [x] Run/process schemas are versioned (`types::SCHEMA_VERSION`,
  carried in every `Run`).
- [~] **Parent/child process relationships are preserved** — met for both
  toolchains this project actually targets: Cargo/`rustc` builds via
  RUSTC-wrapper substitution (one `ProcessRecord` per real `rustc`
  invocation; linker cost rolls up into whichever `rustc` spawned it,
  matching `rustc-fake`'s own accepted scope), and `nim c`/`nim cpp`
  builds via CC-wrapper substitution (one `ProcessRecord` per C/C++
  compiler invocation *and* the link step separately — better coverage
  than the Cargo case, a structural consequence of Nim's `.exe`/
  `.linkerexe` config vars covering compile and link independently, not
  extra implementation effort). **Not** met generally: any other root
  command (plain shell commands, other build tools) still only gets the
  root command's own record, with cumulative (not per-node) resource
  usage — no OS-level process-tree walking exists as a fallback for the
  fully general case.
- [x] Major wall/CPU/memory/I/O fields are captured where the host
  exposes them (per-invocation under Cargo/rustc and Nim c/cpp;
  root-command-cumulative otherwise).
- [x] Missing platform fields are explicit null/unsupported states, not
  fabricated zeros (`ResourceUsage::unsupported_fields`).
- [x] All events can be correlated to one monotonic Run clock
  (`clock::RunClock`; the outer command's own timestamps are `Instant`-based
  and strictly monotonic; wrapper-recorded events are wall-clock deltas
  against a shared anchor -- see "the cross-process clock correlation
  trade-off" above for why that's not the same guarantee, stated
  explicitly rather than glossed over).
- [x] Failure/cancellation does not discard partial evidence (CI
  Experiment 5: exit code, resource usage, stdout/stderr all present
  for a deliberately failing command).
- [x] **Minimal and resource-traced modes can be compared for observer
  overhead** — `laminaria run --probe-level {level0,level1}` selects
  between `tracer::trace_root_command_level0` (portable, no `wait4`, no
  Cargo/Nim wrapper substitution) and the existing Level 1 tracer. Real
  overhead measured, not just made theoretically comparable: ~1-3% wall
  time for Level 1 over Level 0, on both a trivial command and a real
  no-op Cargo rebuild (see "Level 0 (minimal wrapper) vs. Level 1
  (resource tracing) overhead" above) — a small sample on one machine,
  not yet a rigorous statistical claim, but the comparison itself now
  produces a real, reproducible number rather than being unimplemented.
- [x] Raw evidence can regenerate `summary.json` without rerunning the
  workload (`store::regenerate_summary_from_disk`, exercised as a real
  disk round trip both in unit tests and in CI against genuine
  CI-produced Runs).
- [ ] **The implementation can later accept dynamic child actions from
  #15** — not evaluated; #15 is not yet implemented, so this is
  unverifiable either way today.

Also not implemented, beyond the checklist: Level 2 compiler-native
telemetry adapters beyond the wrapper's own basic timing/rusage (Cargo
`--timings`/JSON messages, rustc `-Z self-profile`, Nim stage
diagnostics), Level 3 platform profiler integration (`rustc-perf` itself
uses the real `perf_event` crate for this — a concrete next reference
point if this is picked up), artifact inventory (section 8), and
`PreparationRecord`/`CacheState` population (sections 9-10) — the schema
has fields for these so a later implementation doesn't need a migration,
but nothing populates them yet.

## Correctness/evidence-integrity review, and fixes

An external review of this crate (implementation code only, not
documentation) found ten real defects, six of them P1 -- evidence-integrity
or correctness bugs, not documentation gaps. All ten were verified against
the actual source (not taken on faith) and fixed:

1. **Windows always failed at the CLI's default probe level.**
   `tracer::reap`'s `#[cfg(not(unix))]` arm unconditionally returned
   `Unsupported`, and `trace_root_command` (Level 1, the CLI default) had
   no fallback -- `laminaria run` without `--probe-level level0` failed
   outright on Windows. Fixed: a `#[cfg(not(unix))]` `trace_root_command`
   now falls back to the same portable `Child::wait`-based tracing Level 0
   uses, with its own `NON_UNIX_LEVEL1_FALLBACK_NOTE` distinguishing "Level
   1 requested but unavailable, fell back automatically" from Level 0's own
   deliberate scope -- not silently relabeled as real Level 1 data. Closed
   the "no CI ever exercises this" gap too: a new lean `windows` CI job
   (deliberately not folded into the existing bash/jq-heavy `rust` matrix)
   runs the workspace test suite plus a `laminaria run` smoke test on
   `windows-latest`.
2. **The recorded `root_command` didn't match what actually ran.**
   `run_and_record` mutated a cloned `effective_root` (wrapper env vars,
   inserted args) but stored the original, unmutated `root` on the `Run` --
   so `ProcessRecord::argv` (the true, executed argv) and `Run::root_command`
   disagreed. Fixed: `root_command` now stores `effective_root`. Verified
   live against a real `cargo build` through `laminaria run`: `root_command`
   now shows the actual `RUSTC`-wrapper env vars and the inserted
   `--message-format=json`, not the bare original command.
3. **`--message-format=json` was appended at the end of `args`
   unconditionally**, which for `cargo run -- my-program-arg` would land the
   flag *after* the `--` separator and hand it to the target program instead
   of Cargo. Fixed: `cargo_telemetry::message_format_insert_index` computes
   the correct index (immediately after the subcommand, before any `--`),
   also accounting for a leading `+toolchain` arg and restricting the
   "already specified" check to Cargo's own option region.
4. **`requested_toolchain_selector` was hardcoded to `None`.** Fixed:
   `detect_requested_toolchain_selector` records a best-effort selector
   (explicit `RUSTC`/`CC` override, `cargo +toolchain`, or
   `RUSTUP_TOOLCHAIN`) from the *original* root command. A
   `rust-toolchain(.toml)` file is a real, separate mechanism this still
   does not detect -- named as an open gap, not silently treated as "no
   selector requested".
5. **A single corrupted line in `wrapper-invocations.jsonl` discarded every
   well-formed line, and the raw file was then deleted.** `read_events`
   used `.collect::<Result<Vec<_>, _>>()`, so one bad line turned the whole
   read into `Err`, silently swallowed by `run_and_record`'s
   `.unwrap_or_default()` -- and the raw file was deleted immediately after
   regardless. Fixed: `read_events` now returns `EventsReadResult{records,
   unparsed_line_count}`, keeping every line that *did* parse; the raw file
   is no longer deleted (kept as evidence, like `stdout.log`/`stderr.log`
   already are); `known_gaps` now surfaces a note when any line failed to
   parse.
6. **Wrapper substitution silently forced whichever `rustc`/`cc` happened
   to be on `PATH`**, discarding a `cargo +nightly` request or a caller-set
   `RUSTC`/`CC` -- a real risk, since Cargo's own `RUSTC` env var bypasses
   rustup toolchain selection entirely. Fixed: `resolve_real_rustc`/
   `resolve_real_cc` respect an explicit `RUSTC`/`CC` the caller already
   set, and `resolve_real_rustc` resolves a `+toolchain` arg or
   `RUSTUP_TOOLCHAIN` via `rustup which rustc --toolchain <toolchain>`
   rather than silently substituting PATH's default. A
   `rust-toolchain(.toml)` file remains unhandled -- same named gap as #4.
7. **`run_id` path traversal.** Neither `store::run_dir` nor its callers
   validated `run_id`, so `laminaria regenerate-summary --runs-root runs
   "../../etc/passwd"` could read/write outside `runs_root`. Fixed:
   `store::validate_run_id` rejects anything but a single safe path
   component, called from `write_run`, `read_run`, and (transitively)
   `regenerate_summary_from_disk` -- the CLI-exposed entry point. Verified
   live: `regenerate-summary ... "../../../etc/passwd"` now fails cleanly
   with `InvalidInput` instead of touching the filesystem outside
   `runs_root`.
8. **`laminaria_fingerprint::exec::run` treated a failed command's non-empty
   stdout as a successful read** -- contradicting its own doc comment
   ("`None` if ... exits non-zero"). Fixed: any non-zero exit now returns
   `None` unconditionally, matching the documented contract.
9. **The two wrapper binaries (`rustc_wrapper`, `cc_wrapper`) truncated the
   real compiler's exit code to `u8`** via `ExitCode::from(code as u8)` --
   `ExitCode::from` only accepts a `u8` on every platform, not just Windows,
   so any exit code above 255 was silently wrong everywhere, worse on
   Windows where exit codes routinely exceed that range. Fixed: both now
   call `std::process::exit(code)` directly, which passes the real `i32`
   through to the OS.
10. **No Windows CI coverage at all** for any of the above. Fixed: see
    item 1's new `windows` job.

All ten fixes are covered by tests (new unit tests for the pure helpers,
two new `run_and_record`-level regression tests for the root_command/
message-format fix, `store`-level path-traversal rejection tests,
`exec::run`'s corrected contract, and a live smoke run against the real
`rust-heavy-workspace` fixture confirming the recorded `root_command` now
matches the actually-executed command). `cargo test --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo fmt --all -- --check` all pass after the fixes.

### The new `windows` job immediately caught a real regression in item 1's own fix

Not a hypothetical benefit -- this happened on the very first push. The new
`windows` job (item 10 above) failed to even compile:

- `tracer.rs`'s `traces_a_signal_killed_command` test referenced
  `libc::SIGTERM` unconditionally, but `libc` is a `[target.'cfg(unix)'.
  dependencies]` -- doesn't exist in the dependency graph on Windows at
  all. Fixed: gated that test `#[cfg(unix)]` (its assertions about
  `ExitStatusRecord::signal` are inherently Unix-specific: the non-Unix
  fallback path always returns `signal: None`).
- The non-Unix `reap` in `tracer.rs` was private (`fn reap`, not `pub fn
  reap`), but `src/bin/rustc_wrapper.rs`/`cc_wrapper.rs` import it
  unconditionally on every target laminaria-run itself builds for --
  `E0603: function 'reap' is private` on Windows. Fixed: made it `pub`,
  matching the `#[cfg(unix)]` definition.

Fixing just those two would have been enough to make the `windows` job
*compile* -- but not enough to make it *correct*. Making `reap` merely
compile on Windows would leave it doing exactly what its own doc comment
says: returning `Unsupported` unconditionally. `prepare_cargo_wrapping`/
`prepare_nim_wrapping` had no platform gate of their own, so on Windows
they would still set Cargo's `RUSTC` (or Nim's C-compiler override) to the
substituted wrapper binary -- which would then fail *every single
compilation unit* Cargo/Nim tries to run through it, since the wrapper
binary's own measurement calls this same `Unsupported`-returning `reap`.
That is a strictly worse outcome than the original bug (item 1): the
original bug failed the whole Run immediately, before touching Cargo/Nim
at all; this one would have let the traced command start and then broken
every real compiler invocation partway through, for any Cargo/Nim root
command specifically -- exactly the kind of thing "fix the reported bug in
isolation" would have missed, and the reason this crate's own tracer.rs
fix was checked against a *running* CI job, not just a passing local unit
test suite. Fixed: `prepare_cargo_wrapping`/`prepare_nim_wrapping` now
return `Err` immediately on non-Unix, with an explanatory note, so wrapper
substitution is skipped (root-command-only tracing still applies, via the
portable fallback from item 1) rather than silently breaking the build.

## Issue #20's first slice: no-op-safe artifact inventory (`artifact_inventory.rs`)

**Status: one independently-reviewable slice of issue #20, not the whole
issue.** None of #20's thirteen acceptance criteria are claimed complete
by this alone -- several explicitly require multi-toolchain experiments
(#20's "at least two exact Rust toolchains"), LLVM pass timing, Nim/Nimony
comparison, none of which this slice touches. See "What this does not
establish" below.

### What it does

`laminaria run --observe <path>` (repeatable) snapshots each given
directory immediately before the traced command spawns and again
immediately after it exits, then diffs the two snapshots into
`Run::artifact_delta` and `runs/<run-id>/artifacts.jsonl`. Per issue #20's
explicit phase separation: metadata enumeration (`stat()`-only) is timed
separately from content hashing, and hashing only ever runs on changed
candidates (created, or size/mtime differs from the pre-snapshot) --
never on files metadata already proved unchanged. Producer identity is
always `Unknown` (not wired to wrapper-invocation/Cargo-message evidence
in this slice -- see the module's own doc comment).

### Verified against the real `rust-heavy-workspace` fixture, not just synthetic tests

Ran `laminaria run --observe fixtures/rust-heavy-workspace/target -- cargo
build ...` three times in sequence: a cold build (after `rm -rf target`),
an immediate true no-op rebuild, and a rebuild after `touch`-ing one
source file (`fixture-core/src/lib.rs`). Real, non-fabricated numbers:

```text
cold:  147 total, 147 created,   0 modified,   0 unchanged, 147 hashed, hash_seconds=0.357s
noop:  147 total,   0 created,   2 modified, 145 unchanged,   2 hashed, hash_seconds=0.0001s
edit:  263 total, 116 created,  23 modified, 124 unchanged, 139 hashed, hash_seconds=0.353s
```

Enumeration cost stayed ~0.001-0.003s across all three (proportional to
tree size, not to how much changed) -- the no-op-safe property issue #20
asks for: the no-op rebuild's hashing cost (0.0001s, 2 files) is nowhere
near the cold build's (0.357s, 147 files), even though both walked
comparably-sized trees.

### A real, kept-not-smoothed-over finding: the true no-op rebuild still shows 2 "Modified" records

`target/debug/fixture-bin.d` and `target/debug/libfixture_mid.d` (Cargo's
own dep-info files) are flagged `Modified` on every single invocation,
including the genuinely no-op one. Checked, not assumed: their
`digest_sha256` is byte-identical to what the prior (cold) Run recorded
for the same path -- Cargo rewrites these files' content identically but
bumps their mtime on every invocation, confirmed by comparing the actual
recorded digests across the two Runs, not inferred from Cargo's docs.
This means `ArtifactState::Modified` in this crate's current design means
"metadata (size/mtime) changed", not "content changed" -- documented
directly on the enum variant now, not left implicit. A CPU-time-only
no-op heuristic (this project's earlier telemetry) would never have
surfaced this; Cargo's own `--message-format=json` `fresh` flag also
reports `.d` files as part of the crate's freshness, not separately.
Fixing this precisely (comparing pre- and post- content hashes, not just
metadata) is real follow-up work, deliberately not done here -- would
require hashing the pre-state too, which this slice's design intentionally
avoids for the common case (see the module's phase-separation doc comment).

### What this does not establish

Against issue #20's own acceptance criteria: artifact schema is versioned
(`ARTIFACT_SCHEMA_VERSION`) -- met. Detection costs separately measurable,
no-op measurement doesn't hide full-tree hashing behind a cache-hit stat,
producer identity proven-or-unknown (not filename-inferred) -- met, for
this one slice. **Not** met: no ToolchainFingerprint attached to
individual artifact records yet (only to the whole Run); no multi-toolchain
experiment; no rustc self-profile/LLVM pass-timing/Nim-Nimony telemetry
attached to artifacts; no auto-detection of Cargo/Nim output directories
(the caller must name `--observe` roots explicitly); `Modified` conflates
metadata-change with content-change, as described above.

## Issue #21's first slice: scenario repetition and noise-floor-aware comparison (`scenario.rs`)

**Status: one independently-reviewable slice of issue #21, not the whole
issue.** None of #21's ten acceptance criteria are claimed fully closed.

### What it does

`laminaria scenario-run --workload {rust-heavy-workspace,nim-heavy-workspace}
--kind {cold,noop,edit} --repeat N` runs one of the four scenarios issue
#21 names to start with, `N` times, each repetition a full
`run_and_record` (preparation excluded from the timed interval, then
recorded into `PreparationRecord`/`CacheState` -- both previously always
`::default()`, now genuinely populated for the first time). Every raw
sample is retained (`ScenarioReport::wall_seconds.samples`); aggregated
stats (min/p50/p90/mean/stddev) are also computed, but the raw samples
stay in the report, not discarded after aggregation.

`laminaria scenario-regenerate` rebuilds the same report purely from
already-written `run.json` files -- verified as a real disk round trip:
regenerated JSON matched the original byte-for-byte in manual testing.

`laminaria scenario-compare --baseline <report.json> --candidate
<report.json>` compares two reports: a noise-floor-aware wall-time
verdict (`above_noise`/`within_noise`/`below_noise`, using the *baseline
report's own* measured stddev × 2 as the floor -- not one universal
percentage threshold across environments, per issue #21's explicit
warning), plus two independent structural checks -- process count and
artifact create/modify/delete profile -- that are flagged in
`confounding_notes` regardless of what the wall-time verdict says. This is
the literal "a result is not an improvement merely because wall time
fell" requirement: `scenario-compare`'s own exit code is non-zero
whenever `confounding_notes` is non-empty, so a caller can't accidentally
treat a structurally-different comparison as a clean pass.

### Verified against the real `rust-heavy-workspace` fixture

`scenario-run --kind cold --repeat 2`: both repetitions independently
`rm -rf target` then rebuild from scratch, producing 147 created
artifacts and 7 process records each time (root + 6 wrapper-recorded
`rustc` invocations) -- consistent across repetitions, as a cold build
should be.

`scenario-run --kind noop --repeat 3`: 1 process record each time (Cargo
determined everything was fresh without spawning any `rustc` at all --
confirmed, not assumed, from the actual recorded process count), wall
time ~0.035s consistently, and artifact records mostly `Unchanged` (one
of Cargo's own `.d` dep-info files still shows as changed on every
repetition -- the same real finding `artifact_inventory`'s own NOTES.md
section documents).

`scenario-compare` on the cold vs. noop reports above: wall-time verdict
`above_noise` (52x), **and** both `process_count_changed`
(1 vs. 7) and `artifact_profile_changed` are flagged in
`confounding_notes` -- exactly the intended behavior: a caller reading
only the wall-time verdict would correctly conclude "much slower," but
the confounding notes correctly explain *why* (a completely different set
of actions ran), rather than presenting it as a clean apples-to-apples
timing comparison.

### What this does not establish

Against issue #21's own acceptance criteria: scenario schema versioned
(`SCENARIO_SCHEMA_VERSION`) -- met. Cold/no-op have explicit reproducible
preconditions, edits mutate exactly the intended source file, cache state
is explicit (if only a 3-value label), raw samples retained and reports
regenerable, observed differences compared against a measured noise
floor rather than a fixed threshold, work/resource/path changes surfaced
alongside wall time -- all met *for this one slice's four scenarios*, not
validated more broadly. **Not** met: cross-environment comparison
rejection (`compare_reports` does not check `EnvironmentFingerprint`
compatibility at all yet); the cache-state contract is a bare 3-value
label, not the full per-subsystem contract (Cargo target dir vs. compiler
incremental state vs. sccache vs. ThinLTO cache vs. filesystem/page-cache
policy) issue #21 actually asks for; the observer-overhead-by-layer
matrix (issue #19's Level 0/1 comparison already exists, but Level 2/3
and artifact-hashing overhead are not broken out per layer here);
backend/config-only, link-only, worktree-relocation, and every
ThinLTO/Wasm scenario remain unimplemented; the noise-floor comparison
itself is a fixed stddev multiplier, not a rigorous statistical test (no
confidence interval, no small-sample correction).

## Issues #7/#12's first slice: minimal reuse-decision core (`reuse.rs`)

**Status: one independently-reviewable slice of two large [Research]
issues, not either issue as a whole.** #7 and #12 combine for 23
acceptance criteria spanning CAS storage, cross-toolchain-version policy,
heterogeneous hosts, ThinLTO/Wasm reuse, and persistence tiers -- none of
that is attempted here. This slice answers exactly one question: **given
two Runs of the same logical scenario, can LAMINARIA decide "this
artifact is reusable" vs. "this must rebuild," with the decision
explained from identity differences.**

### What it does

`ArtifactIdentityKey`: source content digest (every file under given
source roots, content-hashed -- not mtime-based), the resolved
toolchain's own executable digest (`ExecutableIdentity::digest_sha256`,
reused directly from issue #18/#19's already-captured
`ToolchainReport`, not re-derived), a caller-supplied logical command
identity, and target/host identity. `decide_reuse` compares two keys and
returns `Reusable` or `MustRebuild { reasons }` -- every difference is
named, never a bare yes/no, and an *unresolved* toolchain digest
(`None`) is always treated as a difference, even against another `None`
-- issue #7's own "fail closed when unknown" rule, applied literally
rather than only documented.

### Verified against a real (copied, never the tracked) `rust-heavy-workspace` fixture

Ran cold build -> true no-op -> a `touch`-only edit -> a real content
edit, computing an `ArtifactIdentityKey` after each:

- Cold vs. true no-op: `Reusable` (identical source/toolchain/command/
  target).
- Cold vs. `touch`-only edit (mtime bumped, content byte-identical):
  **also `Reusable`** by LAMINARIA's content-based decision -- verified
  as a genuine divergence, not a testing artifact, by confirming via the
  Run's own `artifact_delta` (issue #20's module) that the touch *did*
  trigger real Cargo rebuild output (a Created/Modified artifact under
  `target/`). This is the concrete evidence issue #12 asks for: Cargo's
  own mtime-based invalidation did unnecessary work here that a
  content-based reuse decision would have skipped.
- Cold vs. a real content edit: `MustRebuild`, with `reasons` explicitly
  naming "source content digest differs."

### A real test-authoring finding, worth keeping

The first version of this same test asserted the touch triggered a
rebuild by checking `process_trace.processes.len() > 1` -- and failed,
showing exactly 1 process record for a build that (confirmed separately
via the CLI) really did recompile a crate. Cause: `cargo_wrapper.rs`'s
`find_rustc_wrapper_binary` looks for `laminaria-rustc-wrapper` next to
`std::env::current_exe()`; inside `cargo test`, that's the test binary
itself, not `laminaria-cli`, so RUSTC-wrapper substitution silently never
engages there (the same "cargo test doesn't put the wrapper binary in
target/debug/" gotcha this crate's own history already names -- but this
time it made a *test's own assertion* misleading, not just a real Run).
Fixed by checking the artifact delta instead, which doesn't depend on
wrapper substitution at all.

### What this does not establish

Against issues #7/#12's combined acceptance criteria: identity schema
explicit and versioned, hit/miss explained from identity differences,
reuse rejected when compatibility can't be proven (fail-closed on an
unresolved toolchain), work elimination distinguished from a cache hit
(the touch-edit case above) -- met, for this one slice. **Not** met: no
CAS storage or artifact retrieval (this only decides and explains, never
stores/fetches bytes); no cross-machine, cross-worktree-path, or
cross-toolchain-version reuse; no actual "skip the compiler" behavior --
`run_and_record` still always runs the traced command regardless of what
`decide_reuse` would say; no ThinLTO/Wasm identity, no host/target
distinction beyond one triple/OS-CPU pair, no persistence-tier modeling;
no `laminaria-cli` subcommand exposing this yet (verified via a direct
`cargo test`, not the CLI).

## Second review pass: 5 more correctness bugs, all fixed and verified

An external review of commits `a75c314..ab60dfa` (the full #19-review-fix
→ #20 → #25 → #21 → #7/#12 sequence above) found five more P1 bugs
touching measurement/reuse correctness directly, plus one P2 the same
code region made cheap to fix at the same time. All six verified against
real reproductions, not just patched blind.

1. **`resolve_real_rustc`/`resolve_real_cc` ignored the caller's own
   ambient environment.** Only `root.env_overrides` was checked, never
   `std::env::var` -- but `RootCommand.env_overrides` is only populated by
   an explicit mechanism this CLI doesn't even expose, so a caller who set
   `RUSTC=/usr/bin/false` in their own shell had that choice silently
   discarded: `tracer::build_command` inherits the parent's full
   environment by default, and this crate's own unconditional wrapper
   substitution then overwrote it. Reproduced directly: `RUSTC=/usr/bin/
   false cargo check` fails as expected; the same environment through
   `laminaria run` silently substituted PATH's default rustc and
   succeeded. Fixed with a shared `effective_env_var` helper (checks
   `env_overrides` first, then ambient `std::env::var`), used by
   `resolve_real_rustc`/`resolve_real_cc`/`detect_requested_toolchain_selector`
   alike. Re-verified live: `requested_toolchain_selector` now correctly
   reads `"RUSTC=/usr/bin/false"`, and the traced build now genuinely
   fails, matching direct Cargo behavior.
2. **`toolchain_identity_from_run` used `.first()` instead of the
   actually-invoked toolchain.** `toolchains.lock.toml` can declare more
   than one Rust/Nim toolchain, and `ToolchainReport` resolves *all* of
   them regardless of which one a given Run actually used -- so changing
   a requested selector from toolchain A to B still compared as
   `Reusable`, because both keys carried A's digest (declared first).
   Fixed by matching the Run's actually-used compiler path (the wrapper's
   own `LAMINARIA_WRAPPED_RUSTC` record for Rust, the root process
   record's resolved `executable` for Nim) against the report's declared
   toolchains by path, returning `None` (fail-closed) when it matches
   none. Caught a second, genuine environment fact in the process: this
   session's own dev machine has a non-rustup `rustc` shadowing `PATH`
   ahead of the rustup-managed one (`EnvironmentFingerprint::
   path_toolchain_shadow` already named this), so the real-fixture test
   now pins `RUSTC` explicitly to the lock-resolved path rather than
   depending on `PATH` happening to agree with it.
3. **A failed scenario repetition was aggregated as a normal, fast wall-
   time sample.** `build_report` fed every repetition's wall time into
   `Stats::from_samples` regardless of `Run::result.success` -- an early
   failure (exiting fast) looked like a speedup. Fixed: `wall_seconds` is
   now computed from successful repetitions only; `run_ids`/`success`/
   `process_counts`/`artifact_*` still cover every repetition (failure
   evidence is kept, not discarded, and each failed run's own `run.json`
   is untouched); a scenario where *every* repetition failed now returns
   an error rather than fabricating an empty or fictitious `Stats`.
4. **`compare_reports` had no workload/environment/toolchain eligibility
   check at all.** Two reports from genuinely different workloads (or
   with no comparability identity recorded on either side, prior to this
   fix) produced an ordinary `below_noise` verdict with zero
   `confounding_notes`. Fixed: `ScenarioReport` now carries
   `environment_fingerprint` and `toolchain_digest_sha256` (from the
   first repetition), and `compare_reports` checks `workload_id` equality,
   environment comparability (`laminaria_fingerprint::comparability::
   environments_comparable`, reused directly, not reinvented), and
   toolchain digest equality -- rejecting the comparison outright
   (`Result::Err`, before any numeric verdict is computed) on a mismatch
   or an unresolved digest on either side. `scenario-compare`'s CLI exit
   code reflects this too.
5. **`hash_source_roots` swallowed I/O errors as an empty source set.** A
   nonexistent root, an unreadable directory, or a single *file* passed as
   a root (`read_dir` on a file simply errors) were all silently treated
   as contributing zero entries -- reproduced directly: editing a
   single-file root's content never changed its digest at all, since the
   walk silently saw nothing regardless. Fixed: each of those cases now
   returns a real `io::Error` instead of an empty-set digest.
6. **[P2, fixed alongside #4] `compare_reports`'s process-count/artifact-
   profile comparison used only each report's minimum**, so a baseline of
   `[1]` and a candidate of `[1, 50, 50]` shared the same minimum and
   compared as unchanged, hiding that two of three candidate repetitions
   did far more work. Fixed: both now compare `(min, max)` ranges instead
   of a bare minimum.

All six are covered by new, targeted tests (fast synthetic unit tests for
the identity/comparison logic, plus the existing real-fixture integration
test extended to actually exercise RUSTC-wrapper substitution through the
real CLI binary -- required after fix #2, since running scenarios
in-process via `cargo test` can never engage wrapper substitution at all,
a pre-existing crate limitation this fix's own test had to route around).
`cargo test --workspace`: 68 passed (up from 56). Clippy and fmt clean.

Two of the six new tests were themselves environment-dependent and needed
a follow-up fix: `detect_requested_toolchain_selector_is_none_when_nothing_was_requested`
asserted `None` unconditionally, but GitHub's rustup-managed CI runners
(confirmed on both macOS and Windows) set `RUSTUP_TOOLCHAIN` in the
ambient environment globally -- `effective_env_var` (fix #1) correctly
picks this up, so the test now self-skips when an ambient var is already
present rather than asserting something false. The real-fixture test
itself passes on Linux/macOS but, as expected, cannot exercise wrapper
substitution on Windows at all (`prepare_cargo_wrapping`'s own long-
standing `#[cfg(unix)]`-equivalent guard) -- gated `#[cfg(unix)]`
accordingly, along with its now-otherwise-unused helper functions.

## Third review pass: 5 more bugs (2 P1-adjacent, 3 P2), plus a P2 in the #25 substrate prototype

Continuing the same external review, five more real bugs across
`scenario.rs`/`artifact_inventory.rs` and the issue #25 substrate
prototype fixture, all fixed and verified:

6. **`TrueNoop`'s empty `prepare` had no precondition check**, and the
   touch-only "edit" scenario was misleadingly named. Reproduced: running
   a `TrueNoop` scenario against a workload whose output had never been
   built triggered a real, full build (Cargo itself correctly reported
   `fresh=false`), yet the `Run` was still recorded `cache_state=TrueNoop`.
   Fixed with `verify_true_noop_precondition`, checked before any
   preparation or the timed command runs, erroring when an observation
   root doesn't already exist. Also renamed the `Warm`/touch-only
   scenario's own id from `"rust-implementation-edit"`/`
   "nim-implementation-edit"` to `"rust-mtime-touch-edit"`/`
   "nim-mtime-touch-edit"` -- it's an mtime bump, not a content change (see
   `reuse.rs`'s own dependence on exactly that property), and issue #21's
   actual "implementation-only edit" scenario remains a separate, still-
   open gap.
7. **The artifact-inventory walker couldn't handle a single-file
   observation root.** The Nim scenario preset passes `out_path` (the
   final linked binary, one file) as an observation root, and the walker
   called `read_dir` on every root unconditionally -- which errors on a
   file, silently collapsed into the same "zero entries" outcome the
   module deliberately uses for a root that doesn't exist yet. Reproduced:
   rewriting a single-file root's content left the inventory with 0
   records. Fixed with `walk_root`, which checks whether a root is a file
   (tracked directly) or a directory (walked as before) before falling
   back to "doesn't exist, zero entries."
8. **Two observation roots sharing a final path component collided into
   one logical path.** `to_logical_path` used only `root.file_name()` as
   the logical prefix, so `a/target/output.o` and `b/target/output.o`
   (both named `target`) both reported as `target/output.o` -- two
   distinct files, different digests, one colliding identity. Fixed by
   prefixing with the root's own index in the `roots` slice (`root0-`,
   `root1-`, ...) instead of just its bare name.
9. *(Already covered above alongside #4: the `(min, max)`-range fix for
   `compare_reports`'s process-count/artifact-profile comparison.)*
10. **[Issue #25 substrate prototype] The inliner could duplicate a
    side-effecting argument expression.** `substitute_params` copies the
    argument expression verbatim into *every* occurrence of a parameter
    in the callee's body -- but `inline_call` only ever checked the
    *callee's* `has_side_effects` fact, never whether the actual argument
    itself contained a call that inlining would then duplicate.
    Reproduced directly: `double(effect(x))` (`double`'s own body, `x +%
    x`, references its parameter twice; `effect` registered
    `has_side_effects=true`) was permitted, silently invoking `effect`
    twice. Fixed: `inline_call` now also refuses whenever a
    multiply-referenced parameter's actual argument contains any `Call`
    -- fails closed on "cannot prove this argument is safe to duplicate,"
    not "assume it's pure." A singly-referenced parameter receiving a
    call argument is still permitted (nothing to duplicate), verified by
    a dedicated test so the fix isn't an overbroad "never inline a call
    argument" rule.

All five verified by new tests; `cargo test --workspace`: 74 passed (up
from 68). The substrate prototype's own `cargo test`/`cargo clippy`
(run separately, it's outside the main workspace) and `trace.sh`'s full
cross-check (real Rust/Nim binaries, reference evaluator, LLVM-IR
projection, and the allowed/rejected inlining demo) all still pass.

## Fourth review pass: 6 more bugs (3 P1, 3 P2), all in comparison/reuse/inlining correctness

A third round of external review, checking the same review's own earlier
fixes (all reconfirmed still passing via independent reproduction) and
finding six more real bugs, all fixed and verified:

1. **[P1] `compare_reports` never checked `success`.** `build_report`
   already excludes a failed repetition's wall time from `wall_seconds`,
   but the comparator itself never checked whether *any* repetition in
   either report had failed at all. Reproduced: a `[success, failure]`
   report compared against an all-success one produced an ordinary
   `-90%`/`BelowNoise` verdict with zero confounding notes. Fixed with
   `reject_if_any_repetition_failed`, called on both baseline and
   candidate before any other eligibility check -- excluding a failed
   sample's *time* from statistics is not the same claim as "this report
   is a valid baseline/candidate."
2. **[P1] `build_report` only validated the first Run's identity.**
   `environment_fingerprint`/`toolchain_digest_sha256` were read from
   `runs[0]` alone, with nothing checking whether the rest of the
   repetition set actually agreed. Reproduced: mixing Runs of different
   `workload_id`/architecture/toolchain still built one report carrying
   only the first Run's identity, and a comparison against it passed
   cleanly. Fixed with a validation loop over every Run in the set,
   checking `workload_id` equality, `environments_comparable`, and
   toolchain digest equality against the first Run, erroring with the
   specific mismatching Run's id on any disagreement.
3. **[P1] `reuse.rs`'s source walker skipped symlinks inside the tree.**
   The prior round's root-level checks (missing/unreadable/non-directory
   root) didn't cover a symlink *inside* a source root -- `walk_files`
   silently contributed nothing for a symlink entry, so a source file that
   happened to be a symlink was invisible to the digest, and editing its
   link target's content never changed the digest. Fixed to error on any
   symlink encountered while walking (`InvalidInput`) rather than skip it
   -- following it safely needs cycle detection this walker doesn't
   implement, so it's explicitly unsupported input until then, not a
   silently-empty one. (`artifact_inventory.rs`'s own, separate symlink-
   skipping walker was deliberately left as-is -- it tracks build
   *artifacts*, not source identity, a different concern the reviewer
   didn't flag.)
4. **[P2] `TrueNoop`'s precondition only checked `root.exists()`.** An
   *empty* `target/` directory passes `exists()` but isn't evidence of a
   prior successful build; running a scenario against one still performed
   a real, full build (Cargo correctly reported `fresh=false`) yet was
   still recorded `TrueNoop`. Fixed two ways: (a) `verify_true_noop_
   precondition` now requires at least one entry under each observation
   root, not just existence; (b) a new postcondition check,
   `true_noop_postcondition_violation`, runs *after* the traced command,
   counting Created/Modified records in the run's own `artifact_delta` --
   more than `TRUE_NOOP_CHANGED_ARTIFACT_TOLERANCE` (5, matching the
   already-established CI tolerance for Cargo's own benign `.d`-file
   rewrites, see the "Scenario repetition..." CI step) means real
   compile/link work happened despite the `TrueNoop` label. The Run is
   still written to disk either way (the work that happened is real
   evidence, not something to discard because the label turned out
   wrong), with the violation appended to `cache_state.notes`, and
   `run_scenario_once` returns an `Err` so callers are alerted rather than
   silently trusting a mislabeled result. This is the same mislabeling
   `reuse.rs`'s own real-fixture integration test had been relying on for
   its deliberate "edit content, then run scenario kind `noop`" step --
   that step now correctly uses kind `edit` (`CacheStateLabel::Warm`)
   instead, matching what actually happens to the source.
5. **[P2, issue #25 substrate prototype] The inliner silently dropped an
   unused call argument's evaluation.** The prior round's fix refused
   inlining when a parameter's occurrence count was `> 1` (duplication),
   but permitted `0` (an unused parameter) -- substituting only the
   referenced parameters silently drops any unreferenced argument
   expression, never evaluating it. Reproduced: for `pick(x, y) = x`,
   inlining `pick(x, effect(x))` transformed to just `x`, and the call to
   `effect` disappeared entirely. Fixed by changing the guard from `> 1`
   to `!= 1`, covering both hazards (duplication and silent drop) with
   distinct error wording for each. The still-open evaluation-*order*
   hazard the reviewer also named (a singly-evaluated argument whose
   *position* relative to another argument's own effects changes) remains
   explicitly documented as unfixed in `inline_call`'s own doc comment --
   fixing it needs a single-evaluation, order-preserving binding form this
   small experiment doesn't implement.
6. **[P2] `environments_comparable` didn't check resource capacity.**
   CPU core count and installed memory weren't load-bearing fields at
   all, so two fingerprints differing only in `cpu_physical_cores`/
   `cpu_logical_cores`/`memory_bytes` (e.g. 8 cores/32 GiB vs. 192
   cores/1 TiB) compared as one performance baseline. Fixed by adding all
   three to the field list `push_if_differs` already checks. A caller
   deliberately comparing across different resource envelopes (a real,
   separate research question, see `docs/horizontal-distribution-
   research.md`) is not served by this function at all -- it exists
   specifically to reject that by default for an ordinary regression
   comparison.

Every finding was verified against a live reproduction before fixing (the
reviewer's own additional reproduction code, cross-checked directly
rather than patched blind), and every fix has a dedicated test.
`cargo test --workspace`: 119 passed (up from 110). Clippy and fmt clean,
both for the main workspace and the substrate prototype fixture
separately.

## Fifth review pass: self-build correctness (issues #8/#6/#4) plus the two named carryovers

A review of the newly-added self-build pipeline (`self_build.rs`,
`laminaria-plan`) found three more bugs (2 P1, 1 P2), all fault-injection-
reproduced before and after each fix, and asked for the two still-open
carryovers this same file already named (item 4 above's `TrueNoop`
count-based tolerance, and item 5's evaluation-order gap) to actually be
closed rather than left documented as open indefinitely:

1. **[P1] Cross-generation cache sharing let a second generation "succeed"
   with zero real compilation.** `run_generation` built into `repo_root`'s
   own shared `target/` and `nim-planner/bin/`; a second call into a
   *different* `--generation-root` still silently inherited the first
   call's already-fresh Cargo/Nim outputs. Reproduced: 57/57 Cargo
   artifacts `fresh=true`, 0 recompiled, still reported success. Fixed by
   giving every generation its own isolated `<generation_root>/.build/`
   staging area (a fresh Cargo `--target-dir` and Nim `--nimcache`, wiped
   at the start of every `run_generation` call). Manually re-verified:
   two different generation roots each now show 0/57 fresh.
2. **[P1] An internally-consistent-but-empty `ExecutionPlan` validated and
   "succeeded."** `laminaria_plan::validate` checked only the returned
   plan's own internal ordering consistency, never its correspondence to
   the original `PlanningInput` -- reproduced with a stub planner
   returning `{"actions": {}, "ordered_actions": []}`: exit 0, no
   generation root, "success," because the empty set trivially equals
   itself. `validate` now also takes the original `PlanningInput` and
   rejects a plan whose action set doesn't match exactly, whose
   per-action shape was altered, or that doesn't produce every demanded
   artifact. Also closed a related gap found alongside it: duplicate-
   output-producer detection in Rust's own `validate` was silently
   overwritten by `BTreeMap::insert` instead of rejected outright.
3. **[P2] Execution proceeded on a missing/unreadable toolchain lock,
   silently using whatever `cargo`/`nim` were on `PATH`.** Reproduced: a
   nonexistent `--lock` still "succeeded," with an empty resolved-
   toolchain list in the evidence. `run_generation` now resolves and
   verifies a real Rust and Nim toolchain from the lock file before
   executing any action, and invokes those resolved executable paths
   directly (never bare `"cargo"`/`"nim"` names) -- manually re-verified:
   a nonexistent `--lock` now fails closed with a structured
   `ToolchainUnresolved` error before touching the generation root at
   all.
4. **[P2, carryover from the fourth pass] `TrueNoop`'s postcondition
   tolerated up to 5 changed artifacts regardless of what they were.**
   Reproduced: a genuine C compile that newly created exactly one
   executable still validated as `TrueNoop`, since one file is fewer than
   the tolerance of five. Replaced the count-based tolerance with a
   kind-based judgment: `Created` is *never* tolerated at any count (a
   genuine no-op must never bring a new file into existence), and
   `Modified` is tolerated only for the one specific, independently-
   verified benign pattern -- Cargo's own `.d` dep-info files, judged by
   their literal `.d` suffix, not by how many changed. Re-verified against
   the real `rust-heavy-workspace` fixture's own true no-op rebuild
   (`reuse_decision_matches_real_fixture_behavior_across_cold_noop_and_edits`
   still passes, confirming the real `.d`-file paths this fix tolerates
   match what Cargo actually rewrites).
5. **[P2, carryover from the fourth pass, issue #25 substrate prototype]
   The inliner could still reorder two side-effecting call arguments.**
   The fourth pass's occurrence-count fix (`!= 1`) only proved a
   parameter was referenced *once*, not that it was evaluated at the same
   *position* the caller's own argument list implied. Reproduced directly
   before this fix: `reverse(x, y) = y +% x` (each parameter referenced
   exactly once) inlining `reverse(effect1(), effect2())` returned `Ok`,
   producing `effect2() +% effect1()` -- silently reordering two
   side-effecting calls. Fixed with `param_evaluation_order`, which walks
   the callee's body in the exact order `eval_expr` actually evaluates it
   and requires, for every pair of parameters that both receive an
   effectful argument, that the earlier-indexed one's occurrence precede
   the later-indexed one's -- refusing inlining otherwise. A companion
   test (`combine(x, y) = x +% y`, order-preserving) confirms the fix is
   scoped to actual reordering, not a blanket "never inline two effectful
   arguments" rule. The representation still has no single-evaluation,
   order-preserving binding form that could prove a genuinely-reordering
   case safe instead of merely refusing it -- named as before, but no
   longer gating an unfixed hazard.

Every finding was verified against a live reproduction before fixing, and
every fix has a dedicated test. `cargo test --workspace`: 141 passed (up
from 119); the substrate prototype fixture: 10 passed (up from 8).
Clippy and fmt clean throughout, both for the main workspace and the
substrate prototype fixture separately.

## Sixth review pass: self-build path/toolchain/CI alignment (issues #8/#6)

A third external review of the self-build pipeline, run after the fifth
pass's fixes actually shipped and immediately broke CI (the fifth pass's
own local testing never exercised a from-scratch CI runner, only this
already-bootstrapped dev machine -- see below). Four P1/P2 findings, all
fault-injection-reproduced, plus the CI break itself:

1. **[P1] A relative `--generation-root` resolved three different,
   mutually inconsistent ways.** `nim_build_root` spawns `nim` with
   `cwd = repo_root/nim-planner`; `cargo_build_root` spawns `cargo` with
   `cwd = repo_root`; `run_integrate_action` never spawns a subprocess at
   all (plain `std::fs` calls resolve against *this process's* cwd).
   Reproduced directly with `--generation-root gen-relative`: Nim wrote
   under `nim-planner/gen-relative/...`, Cargo under `gen-relative/...`
   relative to `repo_root`, and `integrate` found neither. Fixed with
   `absolute_path`, resolving `generation_root` to an absolute path
   *before* anything else derives a path from it -- closing the whole
   class at the source rather than fixing each call site's own relative
   resolution separately.
2. **[P1] A "verified" toolchain didn't have to match the version the
   lock actually requested, and pinning `cargo` didn't pin `rustc`.**
   `resolve_verified_toolchain` previously only checked that *a* path
   resolved, never that its version matched the lock's own `selector`.
   Reproduced: setting the Nim selector to a nonexistent `"999.0.0"`
   while `bin_dir` still pointed at the real, already-installed
   `2.2.10` resolved (and was accepted) with no mismatch reported at
   all. Fixed with `selector_matches_resolved` (a numeric selector must
   prefix-match the resolved version; a channel name like `"stable"` has
   no fixed version to compare against and always matches), applied to
   both Rust and Nim. Separately: resolving `cargo`'s own path is not
   the same as pinning which `rustc` it invokes internally -- `cargo`
   (or a `rustup` shim) resolves `rustc` through its own independent
   mechanism unless told otherwise. Fixed by having `cargo_build_root`
   set an explicit `RUSTC` override to the verified toolchain's own
   resolved `rustc` path, which also feeds `lib.rs`'s existing
   `resolve_real_rustc` (checked first, ahead of `PATH`/`+toolchain`
   detection) so the per-invocation tracer wraps the same, correct
   binary.
3. **[P1] CI never actually provisioned the Nim version the lock file
   pins, so real toolchain verification (once it existed) had no chance
   of passing there.** `toolchains.lock.toml`'s `nim2_pinned` entry
   declares `bin_dir = "~/.choosenim/toolchains/nim-2.2.10/bin"`; CI's
   "Install Nim" step only ever ran plain `apt-get install nim` / `brew
   install nim`, which never populates that path. This was invisible
   before the fifth pass's toolchain verification existed (nothing
   previously treated an unresolved toolchain as a hard failure) and
   surfaced immediately once it did: both of the ubuntu-latest self-build
   tests failed with `ToolchainUnresolved("Nim toolchain 'nim2_pinned'
   has no resolved nim executable")` on the very next push. Fixed by
   adding a CI step that installs the exact pinned version via
   `choosenim` (the same mechanism `scripts/bootstrap.sh` already
   documents), rather than loosening the lock file's own pinning to fit
   what CI happened to have -- keeping the plain apt/brew install
   alongside it for the later fixture-building steps that only need
   *some* nim runtime on `PATH`, not the pinned identity.
4. **[P2] Windows `clippy -D warnings` failed on dead code / an unused
   import that only exist to support Unix-only tests.** `#[cfg(unix)]`
   correctly excludes `laminaria-run`'s `copy_workspace_sources`/
   `copy_dir_filtered` test helpers and `laminaria-plan`'s
   `real_planner_binary`-only `use std::path::PathBuf` from compiling on
   Windows at all -- but their own definitions weren't gated the same
   way, so on Windows they became genuinely dead/unused code under
   `-D warnings`. Fixed by gating the helper functions (and the import)
   `#[cfg(unix)]` too, matching their only callers.

A structural lesson from how (3) reached CI at all: this session's own
local testing runs on a dev machine that a *previous* session already
fully bootstrapped via `scripts/bootstrap.sh --install`, so
`nim2_pinned`'s `bin_dir` silently resolved correctly here despite CI
never being able to reproduce that. Verifying "does the lock file's own
pinning mechanism actually work" needs a genuinely fresh environment,
not just this dev machine's already-provisioned one -- worth remembering
before claiming a toolchain-resolution fix is verified based on local
success alone.

Every finding was verified against a live reproduction before fixing
(including the CLI's own `--generation-root gen-relative` end-to-end,
not just the library-level test), and every fix has a dedicated test.
`cargo test --workspace`: 145 passed (up from 141). Clippy and fmt clean.
CI's actual green/red status for this pass is confirmed by the next
push's own workflow run, not by local reproduction alone -- see this
file's revision history / the corresponding commit for whether it held.

That push's own CI run confirmed the sixth pass's own real fixes worked
(choosenim install succeeded, clippy passed, stage0 built) but surfaced
two more bugs on a genuinely fresh checkout, neither visible on this
already-warm dev machine:

- **A real race**: `laminaria-plan`'s two real-planner-binary tests, and
  `laminaria-run`'s three real-stage0-binary tests, each independently
  used a bare `if !bin.is_file() { build it }` guard. On a fresh
  checkout with no binary yet, multiple tests on separate threads (Rust
  test binaries default to one thread per core) raced to `nim c` the
  *same* output path simultaneously; one test's `spawn()` of the
  half-written result hit `PermissionDenied`. Fixed in both places with
  `std::sync::OnceLock`, which guarantees the build closure runs exactly
  once regardless of how many threads call it concurrently -- the same
  "build once, cache the result" intent the old guard had, just actually
  race-free.
- **Two test bugs, not production bugs, caught only on the `windows`
  runner**: `absolute_path_leaves_an_already_absolute_path_untouched`
  hardcoded a Unix-style `/tmp/...` literal as "already absolute," but
  `Path::is_absolute()` on Windows requires a drive-letter/UNC prefix --
  the test silently exercised the *relative* branch there instead of the
  one it named. Fixed by using `std::env::current_dir()` itself (always
  genuinely absolute on every platform) instead of a platform-specific
  literal. Separately, `resolve_verified_toolchain_rejects_a_version_
  that_does_not_match_the_requested_selector` needs a real, resolvable
  Nim toolchain to reach the version-mismatch code path it actually
  tests -- the `windows` CI job never installs Nim at all (kept
  deliberately lean), so it hit the *earlier* "no resolved nim
  executable" rejection instead and failed on the wrong message. Fixed
  by `#[cfg(unix)]`-gating it, matching every other real-toolchain test
  in this module.

`cargo test --workspace`: still 145 passed locally (no new tests, two
fixed and one re-scoped). Verified from a genuinely clean state this
time (`rm -rf nim-planner/bin` first, `-- --test-threads=8`), not just
re-run on an already-warm binary -- confirmed via each fix's own build
log showing exactly one `nim c`/`SuccessX` line despite multiple
concurrent callers, where a race would have shown more than one racing
attempt or a mid-build failure.

## Seventh review pass: version-selector matching and a relative runs-root (issues #8/#6)

A fourth external review, run after the sixth pass's CI run finally went
fully green. Two P2s, both fault-injection-reproduced:

1. **A version selector match was a string-prefix check, not a real
   version comparison.** `selector_matches_resolved`'s prior
   `resolved.starts_with(selector)` treats `"2.2.1"` as matching
   `"2.2.10"`, because it really is a character-for-character prefix of
   it as plain text -- reproduced directly: a lock pinning the exact
   patch version `"2.2.1"` still accepted the real, already-installed
   `"2.2.10"`, a different release, with self-build reporting success.
   Fixed by splitting both the selector and the resolved version on `.`
   and comparing components exactly (for as many components as the
   selector specifies) instead of raw string prefix matching -- a
   shorter selector like `"2.2"` still matches any resolved version
   sharing that prefix, but a full `"2.2.1"` now only matches an
   actually-resolved `"2.2.1"`.
2. **A relative `--runs-root` silently dropped every per-compiler-
   invocation record.** `run_and_record`'s `wrapper_events_path`
   (derived from `runs_root`) is handed to the spawned wrapper binary
   via an environment variable, which the wrapper resolves against *its
   own* `cwd` -- inherited from the traced root command's `cwd`, which
   self-build's own actions deliberately set to `repo_root` or
   `repo_root/nim-planner`, not the calling process's cwd. Reproduced
   directly: with the CLI's own default relative `--runs-root` ("runs"),
   both the Nim and Cargo builds succeeded, but every per-rustc/per-cc
   wrapper event failed to write (`No such file or directory` from the
   wrapper's own perspective) and was silently dropped -- only the root
   process's own Run record survived, with zero real compiler-invocation
   evidence. Fixed the same way as the sixth pass's `generation_root`
   fix: `run_and_record` now resolves `runs_root` to an absolute path
   immediately, before `run_dir`/`wrapper_events_path` are derived from
   it. The shared `absolute_path` helper moved from `self_build.rs` to
   `lib.rs` (`pub(crate)`) so both call sites use the same
   implementation instead of two copies.

The second finding's own test
(`a_relative_runs_root_still_captures_per_compiler_invocation_records`)
had to go through the real `laminaria` CLI binary as a subprocess, not
an in-process call to `run_generation` -- confirmed directly (see
`laminaria_cli_binary`'s own doc comment, mirroring `reuse.rs`'s
existing one): `find_rustc_wrapper_binary` looks for
`laminaria-rustc-wrapper` next to `std::env::current_exe()`, which
inside `cargo test` is the test binary itself under `target/debug/deps/`
-- a directory the wrapper binaries are never copied into. An in-process
test call to `run_generation` therefore never engages wrapper
substitution at all, regardless of this fix; this was true of every
`self_build.rs` test *before* this pass too, just never previously
exercised or noticed because none of them asserted anything about
per-invocation wrapper record counts.

`cargo test --workspace`: 154 passed (up from 145). Clippy and fmt
clean. Both fixes were manually re-verified through the real CLI binary
in addition to their dedicated tests, matching this file's own repeated
lesson about not trusting an in-process/already-warm-environment
reproduction alone.

## Eighth review pass: channel-selector verification (issues #8/#6)

A fifth external review, filed after the seventh pass's own CI run went
green end to end. One P2, an existing gap the review's own framing was
careful to note as pre-existing rather than a regression from this
session's other fixes:

- **A non-numeric selector was accepted unconditionally, regardless of
  what actually resolved.** `selector_matches_resolved`'s "not purely
  numeric" branch returned `true` outright, on the (unstated) assumption
  that a channel name like `"stable"`/`"nightly"` has no fixed version
  to compare against and therefore can't be usefully checked. Reproduced
  directly without `rustup` on `PATH`: `rust_toolchain::resolve`'s own
  fallback path resolves whatever `rustc`/`cargo` happen to be active on
  `PATH`, completely ignoring the requested selector -- so requesting
  `"nightly"` while only a `"stable"` toolchain was actually on `PATH`
  still "verified" successfully and self-build proceeded to compile with
  it. Fixed by checking a recognized channel name (`"stable"`/`"beta"`/
  `"nightly"`, exactly) against the resolved `channel` field
  (`RustToolchainFingerprint`'s own, already-derived from
  `resolved_version` by issue #18's `rust_toolchain.rs`) instead of
  accepting it unconditionally, and by rejecting any *other* non-numeric
  selector outright (a dated nightly like `"nightly-2024-01-15"`, a
  custom toolchain name, ...) as unverifiable rather than silently
  treating "not purely numeric" as "no constraint" -- the review's own
  explicit ask: fail closed on what can't actually be checked. Nim has
  no channel concept at all (`NimToolchainFingerprint` carries no
  `channel` field), so `selector_matches_resolved` is called with
  `resolved_channel: None` for it, meaning a non-numeric Nim selector can
  never match either -- correct, since `toolchains.lock.toml` never
  actually declares one today.

Six new unit tests cover the full decision table directly (channel
match, the exact "nightly requested, stable resolved" repro, an
unverifiable dated selector, no-channel-for-Nim, empty selector,
numeric-with-no-resolved-version) -- an end-to-end
`resolve_verified_toolchain`-level reproduction was deliberately not
attempted for the channel-mismatch case specifically, since it would
depend on whether a `nightly` rustup toolchain happens to already be
installed on whatever machine runs the test, making it environment-
fragile in a way the deterministic unit tests aren't.

`cargo test --workspace`: 157 passed (up from 154). Clippy and fmt
clean. The real, already-resolved `"stable"` toolchain on this dev
machine (and every self-build integration test that depends on it) was
re-verified to still pass after this change -- confirmed directly via
`laminaria doctor --json` that its `channel` field really is
`"stable"`, matching its own `"stable"` selector, before trusting that
the fix doesn't newly break the one toolchain configuration this project
actually uses.

## Issue #26: single-language and minimally-declared mixed project builds

`self-build` always needs both a Rust and a Nim toolchain, which is
correct for LAMINARIA building itself but wrong for a general "build a
user's target project" entry point -- issue #26 asked for a separate one
that resolves and invokes only the toolchain(s) a target project's own
demanded artifacts actually need. A first plan-mode design pass was
reviewed and rejected with five concrete defects before any code was
written, all fixed in what actually landed:

1. **Toolchain resolution being skipped during planning doesn't matter if
   execution/recording re-probes both anyway.** `lib.rs`'s own
   `run_and_record` called `doctor::build(lock_path, repo_root)`
   internally -- unconditionally, regardless of what any caller had
   already resolved selectively. Split into `run_and_record_with_doctor`
   (takes an already-resolved `DoctorRun`) plus a thin `run_and_record`
   wrapper that calls plain `doctor::build` for callers (`self_build.rs`)
   that always want both. `project_build.rs` passes the *same*
   selectively-resolved `DoctorRun` it already built for toolchain
   verification into `run_and_record_with_doctor`, so the unneeded family
   is never probed a second time at record time either.
2. **"A `Cargo.toml` and a Nim entry coexisting ⇒ reject as unsupported"
   is wrong.** File coexistence isn't dependency (a Rust app can sit next
   to an unrelated Nim helper tool), and a real dependency can exist
   *without* file coexistence at all (a `build.rs` shelling out to
   `nim`, no separate Nim source file anywhere). Fixed by making
   `--requires rust|nim|rust,nim` always authoritative over file
   inference when given, inferring only when exactly one language's
   files are present, and treating coexistence with no explicit
   `--requires` as an *ambiguous project* request for designation, not a
   blanket rejection. When both are genuinely requested, the resulting
   plan depends honestly on whether a real Nim producer entry point
   exists: two independent producer actions (no fabricated `Integrate`
   step) when one does, or a single `CargoBuild` action with the Nim
   toolchain's `bin` directory prepended to that action's own `PATH`
   when one doesn't (the "single build process needs the other toolchain
   on hand" shape).
3. **Missing the resolved-compiler-pinning step.** The design pass had
   initially omitted the `RUSTC` env-override fix `self_build.rs`'s own
   `cargo_build_root` already needed once (see the fifth review pass
   above) -- `cargo_project_build_root` carries it over explicitly rather
   than silently regressing a bug already fixed once in this crate.
4. **The verification/fingerprint root defaulted to the caller's own
   cwd (LAMINARIA-shaped) instead of the target project.** Unlike
   `self_build.rs`, `project_build.rs` has no separate `repo_root`
   concept at all -- every `doctor`/environment call fingerprints
   `project_root` itself, so a recorded `Run`'s environment/dirty-state
   describes the actual target being built.
5. **A fixed `.build/cargo-target/release` guessed output path doesn't
   hold under `--target <triple>` or a custom profile.** Fixed by
   reporting a `CargoBuild` action's real artifact paths from Cargo's own
   `--message-format=json` telemetry (`cargo_telemetry`, already parsed
   unconditionally for every Cargo root command) instead of guessing a
   directory layout. A `NimBuild` action's artifact is the exact `-o:`
   path this code itself chose -- fully known upfront, no discovery
   needed.

New: `laminaria_fingerprint::doctor::build_selective` (skips resolving a
toolchain family's `resolve()` calls entirely, not just its result, when
unneeded -- `doctor::build` is now a thin `build_selective(.., true,
true)` wrapper); `laminaria-run`'s `toolchain_resolve.rs` (shared
verification logic factored out of `self_build.rs`'s
`resolve_verified_toolchain`, used by both it and the new
`project_build.rs`); `project_build.rs` itself; two new `laminaria`
subcommands, `plan-build`/`build`, with a `--json` failure envelope
(`{"ok", "error_kind", "detail", "result"}`, always on stdout) that
`self_build_command`'s own JSON path does not have (a separately-noted
gap, not retroactively fixed there in this pass).

Verified against the real fixtures: `fixtures/rust-heavy-workspace`
(pure Rust) and `fixtures/nim-heavy-workspace` (pure Nim, `fixture.nimble`
+ `src/fixture.nim` matching the standard `nimble init` convention this
code's own inference relies on) each build end to end through the real
Nim planner and real `cargo`/`nim`, with the *other* toolchain family
provably never resolved -- checked structurally (`doctor::build_selective`
never calls the unneeded family's `resolve()` at all, proven by two
`laminaria-fingerprint` unit tests) and adversarially (a CLI-level test
places a sentinel `nim`/`nimble` script first on the spawned build's own
`PATH`, using a lock file with `bin_dir` stripped to force a PATH lookup
if resolution were ever attempted, and asserts the sentinel is never
invoked while the build still succeeds).

`cargo test --workspace`: 179 passed (up from 157: +19 in
`laminaria-run`, +3 in `laminaria-fingerprint`, +4 new integration tests
in `laminaria-cli`, which had none before). Clippy and fmt clean.
Manually re-verified via the real CLI binary too (`laminaria plan-build`/
`build --json` against both fixtures, with `--requires rust`/`rust,nim`
combinations), confirming the reported plans/artifacts match this
document's own description. This dev machine's `nim` and `cargo`/`rustc`
share the same `/usr/local/bin` directory, so a genuine PATH-removal
check (as opposed to the sentinel-poisoning test, or renaming an
installed system compiler, which was deliberately not done here as a
needlessly risky action against shared local tooling) was not performed
manually -- the structural guarantee (`build_selective` never calls the
unneeded family's `resolve()` at all, regardless of what is or isn't on
`PATH`) and the CLI-level sentinel-poisoning test together are the actual
evidence for "no unused compiler invocation," not a manual PATH edit.

## Issue #26, second pass: four real bugs found against the real CLI

A review of the first pass's actual behavior (not just its design) found
four concrete bugs by running the real CLI against constructed repro
cases, not just reading the code:

1. **P1: success was reported even when the demanded artifact did not
   exist.** A target Nim project's own `nim.cfg` can set
   `--compileOnly:on` (entirely outside this crate's control) --
   confirmed directly: `nim c -o:<path> src/thing.nim` with that config
   prints `[SuccessX]` and exits 0, and `<path>` is never created.
   `run_project_traced_action` previously only checked the root exit
   status; it now additionally requires every candidate artifact path to
   actually exist on disk (`produced_expected_artifacts`), and there must
   be at least one -- exit status is necessary but not sufficient. When
   this check fails on an otherwise-zero exit, the persisted `Run`'s own
   `result.success` is corrected to `false` too (with a `known_gaps` note
   explaining why), so the raw evidence file on disk doesn't keep
   claiming success once this function's own verdict disagrees with it.
2. **P1: a mixed build could invoke an unverified Nim instead of the
   verified one.** `--requires rust,nim` with a *real* Nim entry point
   also present (the two-independent-producers shape) previously never
   put the verified Nim toolchain on the Cargo action's own `PATH` at
   all -- gated on `req.nim_entry.is_none()`, i.e. "only when there's no
   separate Nim producer." A Rust `build.rs` that itself shells out to
   `nim` in that shape therefore found whatever `nim` happened to be
   first on this process's own ambient `PATH` instead of the one this
   crate had just resolved and verified. Whether a Nim *artifact* exists
   and whether the Cargo *action* needs Nim on hand are independent
   facts; fixed by making the PATH injection depend purely on `req.rust
   && req.nim` (both requested at all), never on `nim_entry`.
3. **P2: an explicit but nonexistent `--nim-entry` was silently
   downgraded to "no Nim entry."** `resolve_nim_entry`'s override branch
   returned `None` (via `Option::then`) exactly the same way "no entry
   found by inference" did, so `--nim-entry src/missing.nim` against a
   project that also had a `Cargo.toml` quietly built Rust-only and
   reported success, discarding the caller's actual (broken) request
   without a word. `resolve_nim_entry` now returns
   `Result<Option<PathBuf>, ProjectBuildError>`: an override that doesn't
   exist is `Err(NimEntryNotFound)`, a genuinely new variant -- distinct
   from "not given at all," which still resolves to `Ok(None)` when
   inference finds nothing.
4. **P2: `plan-build` and `build` computed different `plan_id`s for the
   same project.** `run_project_generation` absolutizes `project_root`
   before building the `PlanningInput` (so `ArtifactRef::source`'s path
   is stable regardless of the caller's cwd); `plan_build_command` never
   did, since it calls `project_planning_input` directly without going
   through `run_project_generation` at all. The same relative
   `--project-root` therefore produced two different `plan_id`s
   (confirmed independently by the review: `ceb707e7f866c23e` vs.
   `500339001d2a53b3` for the same logical project) depending on which
   command computed it. Fixed by exposing
   `project_build::absolute_project_root` (a thin wrapper over the same
   `absolute_path` helper) and calling it from `plan_build_command`
   before doing anything else -- confirmed manually afterward: both
   commands now report `d4d9703df41d5b39` for the same relative
   `fixtures/rust-heavy-workspace` project root.

The review also asked for the existing Rust-side "unneeded Nim never
invoked" adversarial test to get a Nim-side mirror, since #26's actual
completion criterion is symmetric. Added
`build_never_invokes_an_unneeded_rust_toolchain_even_when_one_is_first_on_path`,
placing sentinel `rustc`/`cargo`/`rustup` (covering both the
managed-toolchain and plain-PATH resolution paths `rust_toolchain::resolve`
can take) first on a Nim-only build's own `PATH` and asserting none of
them are ever invoked.

Six new regression tests total (four in `laminaria-run::project_build`,
two in `laminaria-cli`'s integration suite): the `--compileOnly:on` repro
built as a real throwaway Nim project (not simulated); a direct check on
`cargo_project_build_root`'s constructed `RootCommand` proving the
verified Nim bin dir is on `PATH` even with a real Nim entry present;
both the inference-path and explicit-`--requires`-path variants of the
nonexistent-`--nim-entry` case; the Nim-side sentinel test; and a
`plan-build`/`build` plan_id-parity test run from the same cwd with a
relative `--project-root`.

`cargo test --workspace`: 185 passed (up from 179). Clippy and fmt clean.
Each of the four bugs was independently reproduced against the real CLI
binary before writing its fix, not just inferred from reading the review
comment -- the `--compileOnly:on` repro in particular was verified
directly (`nim c` really does exit 0 and skip linking) before assuming
the review's description was exactly right.

## Issue #26, third pass: the artifact-existence check itself regressed a real macOS build

A review of the second pass's own fix (the artifact-existence check added
for the `--compileOnly:on` bug) found one new P1: a genuinely successful
macOS Cargo build under `[profile.release] debug = true` +
`split-debuginfo = "packed"` was reported as `action_failed`. Reproduced
directly before trusting the review's description: `cargo build --release
--message-format=json` with that profile really does emit a
`"compiler-artifact"` message whose `filenames` includes both the real
executable and a `<name>.dSYM` path, and `<name>.dSYM` really is a
directory (a macOS debug-info bundle), not a regular file --
`produced_expected_artifacts`'s `p.is_file()` check (added in the second
pass specifically to catch the *absence* of an artifact) rejected that
directory outright and turned a real success into a false failure.

The fix is a one-line change with the correct scope: `is_file()` ->
`exists()`. Cargo's own `--message-format=json` telemetry is authoritative
about what it actually produced (it is real evidence, not a guess this
crate is making); this check's job is only to confirm those specific
paths are real, not to additionally assert they're regular files rather
than directories -- `--compileOnly:on`'s failure mode (nothing at the
path at all) is still caught correctly by `exists()`, since a
never-created path doesn't exist under either check. Two existing test
assertions (`is_file()` on every returned artifact, in both
`laminaria-run`'s and `laminaria-cli`'s test suites) were loosened to
`exists()` to match, and two new regression tests were added -- one at
the `run_project_generation` level, one through the real CLI binary --
both reproducing the exact `debug = true` / `split-debuginfo = "packed"`
profile and asserting a `.dSYM` directory is accepted among the reported
artifacts alongside the real executable. Both new tests are
`#[cfg(target_os = "macos")]`: `.dSYM` bundles are a Darwin-specific
format, and this is a real regression only reproducible there -- not
gated for portability's sake, gated because the bug itself doesn't exist
on other platforms. Confirmed the new library-level test actually catches
the regression by temporarily reverting the fix and re-running it before
restoring the fix (it failed with the exact `ActionFailed` the review
described, then passed again once restored).

`cargo test --workspace`: 187 passed (up from 185). Clippy and fmt clean.

## Issue #27 stage C, first slice: a real in-process compiler-work executor

New `compiler_work_executor.rs`: the first actual connection of
`laminaria-ir`'s own frontends/transforms/validator/interpreter to a
production-Nim-planner-produced, Rust-validated `ExecutionPlan` --
`LowerSource -> ValidateIr -> TransformFunction -> ValidateIr ->
EvaluateEvidence`, dispatched sequentially in `ordered_actions` order.
Deliberately a *first slice*: CPU-budget admission control, real
concurrent execution, memory accounting, and cancellation are issue
#27's own next, separate PR ("終了試験PR"), not attempted here -- this
slice's own acceptance is the vertical path itself actually running end
to end, judged only after issue #27's boundary-contract fixes
(`laminaria_plan::compiler_work::validate_compiler_work_action`,
`laminaria_ir::validate`) landed.

- **`ArtifactStore`**: three separate maps (candidate `Program`,
  `ValidatedProgram`, evaluation evidence), each keyed by its producing
  action's own content-derived id -- issue #27 B's own "IR payloadは
  初期sliceではRust側のin-memory storeで管理," never inspected by the
  Nim planner.
- **No external compiler fallback anywhere**: every dispatch arm calls
  directly into `laminaria_ir`'s own owned logic
  (`rust_frontend`/`nim_frontend`/`transform::{anf_insert,
  checked_inline}`/`validate::validate_program`/`interpreter::
  eval_function`); a failure is a hard `Err`, never a silent substitute
  -- a genuinely different role from this crate's existing
  `self_build`/`project_build`, which legitimately shell out to real
  `cargo`/`nim` for *delegated*-build actions.
- **`ValidatedProgram` required at the executor's own boundary, actually
  enforced**: `TransformFunction`/`EvaluateEvidence` dispatch read their
  input exclusively from the *validated* map -- confirmed with a
  dedicated test that a same-id entry present only in the *candidate*
  map (never promoted to validated) is still rejected with
  `MissingValidatedInput`, not silently substituted. Issue #27 A2's own
  "未検証IRをexecutorが黙って実行してはならない," enforced by a real
  executor now, not merely documented as an intention.
- **The full vertical path runs end to end for real**: a genuine Nim
  source file on disk, lowered through the real `nim_frontend`,
  ANF-inlined through the real `transform::anf_insert`, validated at
  both ends, planned and ordered by the *real* `laminaria-planner`
  binary, checked by `laminaria_plan::validate::validate`, and evaluated
  through the real interpreter -- confirmed both by the correct wrapping-
  i32 result values (`g(i32::MAX) = i32::MIN`) and by inspecting the
  post-transform validated `Program` to confirm `add` was actually
  inlined away, not a no-op copy.
- Extended `CompilerWorkDescriptor` with `test_inputs: Vec<Vec<i64>>`
  (the actual finite test-input tuples `EvaluateEvidence` runs, kept
  separate from and not hashed into `test_inputs_digest`, which alone
  identifies *which* inputs for a stable artifact id) -- a real
  descriptor gap this slice's own dispatch surfaced (nothing could
  actually *run* `EvaluateEvidence` without the literal values
  somewhere). `EvaluateEvidence` also reuses `requested_functions`
  (documented elsewhere as "LowerSource-only") to name which function to
  evaluate -- a deliberate, named minimal reuse rather than a new field,
  recorded here rather than silently assumed; a future contract version
  may give it a dedicated field instead. `nim-planner/src/contract.nim`
  mirrors `test_inputs` by hand, plus a dedicated round-trip test.

4 new tests (the full pipeline, the validated-vs-candidate enforcement,
a legacy delegated-build kind rejected not silently run, a
compiler-work-kinded action with no descriptor rejected).

`cargo test --workspace`: 315 passed (was 311). Clippy/fmt clean, Nim
suite green.

### What this slice deliberately does not do

- No concurrency: `run_compiler_work_plan` dispatches
  `ordered_actions` strictly sequentially. CPU-budget-1-vs-many result
  equivalence and real concurrent execution are issue #27's own next PR.
- No resource accounting or admission control: `ResourceRequest` is
  carried on the wire but nothing here reads or enforces it.
- No cancellation.
- No demand-based pruning: `run_compiler_work_plan` dispatches every
  action `ordered_actions` names, not only those a `demanded_artifacts`
  closure would actually require.
- **`source_snapshot_id` is not verified against the file on disk at
  dispatch time** -- a real, open gap: a source file edited between
  planning and dispatch would silently lower the *new* text under the
  *old* snapshot id, named directly in `dispatch_lower_source`'s own
  doc comment rather than glossed over.
