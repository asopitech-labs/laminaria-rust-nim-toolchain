# Project build: single-language and minimally-declared mixed targets (issue #26)

This document describes `laminaria build`/`plan-build`: the entry point for
planning and building a **user's target project**, as opposed to
`laminaria self-build` (`docs/self-build.md`), which plans and builds
**LAMINARIA itself**. The distinction issue #26 asks to keep visible:

1. **LAMINARIA's own implementation** is Rust + Nim. A Rust-only target
   build must still use the real, production Nim Planning Kernel to plan
   it — this is not permission to substitute a Rust-computed plan.
2. **Building LAMINARIA itself** (`self-build`) legitimately requires both
   the Rust and Nim toolchains used to rebuild both of LAMINARIA's own
   components.
3. **Building a target project** requires only the toolchain(s) that
   project's own demanded artifacts and dependency graph actually need —
   never LAMINARIA's own implementation languages leaked onto an unrelated
   target.

`build`/`plan-build` reuse the exact same production Nim planner
(`laminaria_plan::call_planner`) and Rust executor
(`laminaria_run::run_and_record_with_doctor`) `self-build` uses — not a
separate or duplicated code path. Investigation before implementing this
confirmed the `PlanningInput -> ExecutionPlan` contract and the Nim kernel
itself (`nim-planner/src/planning_kernel.nim`) never branch on how many
`CargoBuild` vs `NimBuild` actions a plan contains, so no contract or
Nim-kernel change was needed — everything language-specific lived entirely
in `self_build.rs`'s hard-coded three-action shape and its unconditional
both-toolchains resolution, neither of which `project_build.rs` inherits.

## Determining what a target project needs

Never inferred from file coexistence alone, and coexistence is never
silently rejected as "unsupported" either — see
`laminaria_run::project_build::determine_project_requirements`:

1. **`--requires rust|nim|rust,nim`, when given, is always authoritative.**
   File presence is never consulted to override it. This is what makes two
   real cases representable that pure file-based inference cannot:
   - A Rust application living alongside an unrelated Nim helper tool in
     the same directory, where only the Rust side should be built
     (`--requires rust` — the Nim file is never even inspected).
   - A Cargo project whose `build.rs` genuinely shells out to `nim`, with
     no separate Nim source file to compile at all (`--requires rust,nim`
     with no resolvable Nim entry point — see below).
2. **Otherwise, inferred only when unambiguous**: exactly one of a
   `Cargo.toml` or a resolvable Nim entry point (the standard `nimble
   init` convention — a single `<name>.nimble` alongside `src/<name>.nim`
   or `<name>.nim`, or an explicit `--nim-entry`) is present at
   `--project-root`. No configuration file is required for this case.
3. **Both present, no explicit `--requires`**: this is an *ambiguous
   project* error naming both candidates, asking for an explicit
   designation — not a blanket "mixed projects are unsupported" verdict,
   and not a silent guess either way.
4. **Neither present**: a "no buildable sources" error.

## The two-toolchain shapes, honestly

When both Rust and Nim are requested (`--requires rust,nim`, or inferred
implicitly not applicable — this always requires an explicit `--requires`
per rule 3 above), what gets planned depends on whether a real Nim
producer entry point actually exists:

- **A real Nim entry point exists** (a resolvable `.nimble` + source, or
  an explicit `--nim-entry`): two **independent** producer actions are
  planned — one `CargoBuild`, one `NimBuild` — both in
  `demanded_artifacts`, with **no declared dependency edge between them**.
  This is the honest plan for "both are wanted, no known relationship
  between them": inventing an `Integrate` step here would be fabricating
  an action the demand/dependency graph never asked for.
  `self-build`'s own `Integrate` action assembles *LAMINARIA's own*
  generation-root layout (four specific, named sibling binaries) — it has
  no generic equivalent for an arbitrary target project, and this code
  does not pretend otherwise.
- **No Nim entry point exists**: a single `CargoBuild` action is planned.

In **both** cases, whenever Rust and Nim are both requested, the verified
Nim toolchain's resolved `bin` directory is prepended to the `CargoBuild`
action's own `PATH` — so a `build.rs` that shells out to `nim` can find
it. This is governed purely by "was Nim requested at all"
(`req.rust && req.nim`), never by whether a separate Nim entry point also
happens to exist: whether a Nim *artifact* is produced and whether the
Cargo *action* needs Nim on hand are independent facts. An earlier version
of this code conflated them — gating the `PATH` injection on `req.nim_entry
.is_none()` — so a `build.rs` that called `nim` in the
two-independent-producers shape silently found whatever unpinned `nim`
happened to be first on this process's own ambient `PATH` instead of the
one this crate had just resolved and verified. Fixed, and covered by a
dedicated test constructing the `RootCommand` directly and asserting the
verified bin dir is on `PATH` even with a real Nim entry present.

**What this does not attempt**: automatically *inferring* a real
cross-language dependency from source or manifest inspection (e.g.
detecting that a `build.rs` calls `nim`, or resolving a genuine
linked-artifact relationship between two producers) is the job of the
variant/compatibility model (issue #22), not this code. `project_build.rs`
only ever acts on what the caller explicitly declares via
`--requires`/`--nim-entry`.

## Resolving only the toolchain(s) actually needed

`laminaria_fingerprint::doctor::build_selective(lock_path, root, need_rust,
need_nim)` skips resolving a toolchain family **entirely** — no `rustup`
invocation, no `nim`/`nimble` PATH or `bin_dir` lookup — when the caller
doesn't need it, rather than resolving it and discarding the result. This
is what makes "no unused compiler invocation is attempted" true by
construction for a target project that only needs one language, not merely
true because the unused tool happened to be absent. `doctor::build` (used
by `self-build`, which always needs both) is now a thin wrapper over
`build_selective(.., true, true)` — a pure refactor, no behavior change for
any existing caller.

A second, easy-to-miss place the same leak could reappear: `laminaria-run`'s
own `run_and_record` used to call `doctor::build` a *second* time
internally when recording a traced `Run`, which would have silently
re-probed both toolchain families at record time even after
`project_build.rs` resolved only the needed one up front. `run_and_record`
is now a thin wrapper over `run_and_record_with_doctor`, which takes an
already-resolved `DoctorRun` — `project_build.rs` passes the *same* one it
built during toolchain resolution, so the unneeded family is never probed
a second time either.

The exact, resolved, lock-verified `cargo`/`rustc`/`nim` executables are
used as each `RootCommand`'s own `program` (never a bare `"cargo"`/`"nim"`
resolved implicitly on `PATH` at spawn time), and `RUSTC` is set as an
explicit environment override on every `CargoBuild` action — the same
compiler-pinning fix `self-build`'s own `cargo_build_root` already applies,
carried over here rather than silently dropped in the new code path.

Every doctor/environment call in `project_build.rs` fingerprints
`--project-root` itself — there is no separate "LAMINARIA's own repo root"
concept in this module at all, unlike `self-build`. `--lock`'s path is
resolved relative to the invoking process's own working directory,
independent of `--project-root` — a target project does not carry its own
`toolchains.lock.toml`.

## Reporting real artifact paths, not a guessed layout

A `CargoBuild` action's `artifacts` are taken from the actually-executed
Cargo's own `--message-format=json` telemetry (`filenames`/`executable`,
already parsed unconditionally by `run_and_record` for every Cargo root
command) — correct under `--target <triple>`, custom profiles, or multiple
bin targets, none of which a fixed guessed directory like
`.build/cargo-target/release` would survive. A `NimBuild` action's
`artifacts` is exactly the `-o:` path this code itself passed — fully known
upfront, since Nim has no equivalent variable output-layout concern.

A zero exit status alone is never treated as proof that the demanded
artifact actually exists: a target project's own `nim.cfg` can set
`--compileOnly:on`, which makes `nim c` exit 0 without ever linking the
requested `-o:` output (confirmed directly before relying on it). Every
candidate artifact path is checked to actually exist on disk, and there
must be at least one, for an action to be reported as succeeded — an
otherwise-zero exit that fails this check is reported as a failure, and
the persisted `Run`'s own `result.success` is corrected to `false` to
match, so the raw evidence on disk never disagrees with the reported
outcome.

## Usage

```
# Infer capability from project files (no config needed):
laminaria plan-build --project-root path/to/rust-only-project --json
laminaria build --project-root path/to/rust-only-project \
  --generation-root /tmp/gen --json

laminaria plan-build --project-root path/to/nim-only-project --json

# Explicit designation when project files are ambiguous or a real
# dependency exists that file presence alone cannot express:
laminaria build --project-root path/to/mixed-project --requires rust,nim \
  --generation-root /tmp/gen --json

laminaria build --project-root path/to/rust-project-with-nim-build-rs \
  --requires rust,nim --generation-root /tmp/gen --json
```

`--json` failures are always a parseable `{"ok": false, "error_kind": ...,
"detail": ..., "result": null}` envelope on **stdout** — never stderr
prose in that mode — so a scripted caller always has something to parse on
either outcome.

## Verified negative behavior

All of the following are automated tests (`crates/laminaria-run/src/
project_build.rs`, `crates/laminaria-cli/tests/project_build_cli.rs`), not
just described behavior:

- A pure Rust fixture and a pure Nim fixture each infer the correct
  single capability with zero configuration.
- Two files coexisting with no `--requires` is rejected as an ambiguous
  project, naming both candidates.
- An explicit `--requires rust` against a directory that also contains a
  resolvable Nim entry point builds Rust only; the Nim family is never
  resolved (checked directly on the `Run`'s own
  `resolved_toolchain_fingerprint`, not just the pre-execution intent).
- An explicit `--requires rust,nim` against the same directory plans two
  independent producer actions with no `Integrate` step between them.
- An explicit `--requires rust,nim` against a Cargo-only directory (no
  Nim file at all) plans a single `CargoBuild` action.
- `--requires nim` against a directory with no resolvable Nim entry point
  is rejected rather than silently producing zero actions.
- An explicit `--nim-entry` naming a file that does not exist is rejected
  outright — both via pure inference and via explicit `--requires
  rust,nim` — never silently downgraded to "no Nim entry, build Rust
  only."
- A real Rust-only build succeeds end to end (real planner, real `cargo`)
  with a sentinel `nim`/`nimble` placed first on the spawned build's own
  `PATH` and a lock file forcing a PATH-based lookup if resolution were
  ever attempted — the sentinel is never invoked. A Nim-only build has the
  symmetric test: a sentinel `rustc`/`cargo`/`rustup` first on `PATH` is
  never invoked either.
- `--requires rust,nim` with a real Nim entry point also present still
  puts the verified Nim toolchain on the Cargo action's own `PATH` — not
  just when no separate Nim entry exists.
- A Nim `nim.cfg` setting `--compileOnly:on` (which exits 0 without
  linking the requested output — confirmed directly against real `nim c`)
  is reported as a build failure, not a false success naming a
  nonexistent artifact; the persisted `Run`'s own `result.success` is
  corrected to match.
- `plan-build` and `build`, invoked with the same relative
  `--project-root` from the same working directory, compute the identical
  `plan_id`.
- A genuinely broken source aborts the build as a reported failure, never
  a false success.
- Mixed-project `self-build` (stage0 → stage1) is untouched by any of the
  above — it does not go through `project_build.rs` at all.
