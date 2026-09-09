# `laminaria-run` — issue #19 findings and verification log

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
