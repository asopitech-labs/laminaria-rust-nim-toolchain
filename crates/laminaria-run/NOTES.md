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
telemetry adapters beyond the wrapper's own basic timing/rusage (Cargo
`--timings`/JSON messages, rustc `-Z self-profile`, Nim stage
diagnostics), Level 3 platform profiler integration (`rustc-perf` itself
uses the real `perf_event` crate for this — a concrete next reference
point if this is picked up), artifact inventory (section 8), and
`PreparationRecord`/`CacheState` population (sections 9-10) — the schema
has fields for these so a later implementation doesn't need a migration,
but nothing populates them yet.
