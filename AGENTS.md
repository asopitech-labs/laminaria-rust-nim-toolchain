# Repository development rules

## Goal-driven work instructions

The canonical instruction-authoring rules are
[docs/01-foundations/goal-driven-work-instruction-policy.md](docs/01-foundations/goal-driven-work-instruction-policy.md).

- Lead with an externally observable project goal, then define an ordered path
  of checkpoint results that causally reaches it.
- Define each checkpoint by `Result`, `Consumes`, `Must preserve`, `Evidence`,
  and `Enables`. Bind work by required outcomes and relations, not primarily by
  enumerating anticipated anti-patterns.
- The instruction author owns project, semantic, evidence, authority, and
  completion decisions. The implementer owns engineering choices between fixed
  checkpoints. If a new owner-level decision is required, stop and return it;
  do not silently reinterpret the checkpoint.
- Commands, modules, API names, and test names are prescribed only when they are
  already contracts. CI success, test counts, source-text searches, and the
  presence or absence of one implementation shape are not substitutes for a
  checkpoint result.
- Before requesting rework, audit whether the original goal and checkpoint
  chain admitted the submitted but incorrect result. Correct the instruction
  contract first.
- Optimize for the requested artifact and the whole project progression, not
  for an isolated component or metric. A local improvement is accepted only
  after its end-to-end effect, cross-lane costs, displaced work, regressions,
  and effect on later options are evaluated. Measurement depth has its own
  cost and stop condition; collecting more telemetry is not a project outcome.

## Single-source executable verification

The canonical fixture rules are [docs/01-foundations/fixture-policy.md](docs/01-foundations/fixture-policy.md).
Treat fixtures as production-consumed inputs, states, counterexamples, or workloads—not
as a second implementation of a configuration, document, or production algorithm.

- Do not encode one behavior contract redundantly as a hand-maintained YAML
  fixture, a fixture-only validator, and unit tests for that validator when
  the same agent changes all three. This merely expands the change surface and
  does not provide an independent correctness guarantee.
- Prefer direct, executable behavioral tests against the production
  implementation. Keep a separate machine-readable fixture only when it has
  an independent runtime consumer or another concrete purpose beyond checking
  its own internal consistency.
- Do not treat a fixture validator passing as evidence that the production
  implementation is correct. If a fixture is retained, test the production
  implementation by consuming it directly, rather than duplicating its rules
  in a fixture-only validator.
- Do not repeat a checked-in configuration or lock's complete names, counts,
  revisions, features, or attributes in a test. Test the production consumer's
  generic behavior with minimal constructed inputs; the declaration remains the
  sole authority for its values.
- Freeze an expected value only when it has an independent oracle: an external
  standard, a documented manual derivation, an independent reference, a semantic
  relation, or a reduced real failure. Output captured from the implementation
  under test is not an independent expected result.

## Local verification before pushing

This repo pushes straight to `origin main` with no PR gate, so a commit
is live on the trunk the instant it is pushed. Discovering a break only
from a red GitHub Actions run is a self-inflicted delay, not a normal
step of the workflow — push only what has already been verified
locally, and always push with `local-ci.sh` (or a test-declaring commit)
in the history, not GitHub Actions as the first check.

- Run `scripts/install-git-hooks.sh` once per checkout (`scripts/bootstrap.sh`
  does this automatically). This activates two tracked hooks
  (`.githooks/pre-commit`, `.githooks/commit-msg`):
  - `pre-commit` runs `cargo fmt --all -- --check` and
    `cargo clippy --workspace --all-targets -- -D warnings` on native hosts.
    On Windows it validates the source-bound receipt produced by the serialized
    WSLC owner harness instead of starting a second WSLC client.
  - `commit-msg` requires a `Tests-Run:` trailer on every commit message
    and executes what it names on native hosts. On Windows it requires a full
    owner-harness receipt for the exact source fingerprint. See
    `.githooks/commit-msg`'s own
    header comment for the exact convention
    (`Tests-Run: workspace` / `Tests-Run: <filter> [<filter> ...]` /
    `Tests-Run: none (<reason>)`).
- Run `scripts/local-ci.sh` before pushing a commit or series of commits
  with any meaningful blast radius — it mirrors the `rust` CI job's
  early, always-run gate (`cargo fmt --check`, `cargo clippy`,
  `cargo test --workspace`, the nim-planner unit tests). It is not a
  full CI mirror (no Windows job, no Docker job, no fixture-build
  steps) — that script's own header says exactly what it does and does
  not cover.
- Bypass a hook (`git commit --no-verify`) only for a specific,
  deliberate reason, never as a routine workaround for an inconvenient
  failure.

## Windows build execution

- When development is initiated from a Windows host, run every build, check,
  test, lint, format-check, and toolchain diagnostic inside the `wslc`
  container. Do not invoke host Windows `cargo`, `rustc`, `nim`, `nimble`, or
  the bootstrap script for project development.
- Treat the default per-user `wslc` session as one shared singleton across all
  processes and worktrees. Do not launch independent build/test clients in
  parallel. Run the repository-owned owner harness from the repository root:

  ```powershell
  scripts/windows-wslc-ci.ps1
  ```

- The harness holds a per-user host mutex across image build, execution, owned
  container cleanup, and receipt publication. It executes the exact image ID
  emitted by that build; the mutable `laminaria-bootstrap` tag is not an
  execution identity. Git hooks on Windows consume the matching receipt and
  must not start another `wslc` client. The canonical modes and lifecycle are
  documented in `docs/04-guides/windows-wslc-development.md`.
- Keep the image's default unprivileged `laminaria` user. Do not add
  `--user root` to normal build or test commands.
- For the Nim planning-kernel test on Windows, use the documented direct
  `nim c -r` command. Do not use `nimble test`, because dependency resolution
  can download and select a compiler other than the repository-pinned 2.2.10.
- Do not treat container measurements as native Windows performance evidence.
  The measurement policy in `docs/02-research-areas/measurement/measurement-foundation.md` still applies.
