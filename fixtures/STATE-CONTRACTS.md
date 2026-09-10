# Cold / warm / no-op state contracts

## Evidence classification correction (2026-09-10)

This file preserves historical fixture/measurement and implementation evidence, not the current research delivery order. Existing-compiler builds and driver self-builds recorded below are **reference/bootstrap/delegated-build baselines**, not proof of LAMINARIA compiler ownership or independent self-hosting. The [compiler ownership contract](../docs/compiler-ownership-contract.md) governs current issue acceptance; historical checklists do not close the revised requirements.


#11's acceptance criterion "Cold/warm/no-op have explicit reproducible
state contracts." This document defines what those three states mean
for the fixtures in this directory, precisely enough that a future Run
harness (#19-21) can put a fixture into a named state before measuring
it, and records the evidence (`cargo build -v` / `nim c` output from this
session) that each fixture actually behaves as defined here — this is a
state *contract*, not an aspiration, so every claim below was run and
its output checked before being written down.

## Definitions

- **Cold**: no build tool has any cached state for the fixture at all.
  For a Cargo-based fixture: no `target/` directory anywhere under it.
  For a Nim-based fixture: no local `nimcache/` directory under it, and
  — see the pitfall below — no entry for it under Nim's *global* cache
  either. A cold build is what every CI job in `.github/workflows/ci.yml`
  actually exercises today, because each job runs on a freshly
  provisioned runner (`actions/checkout` into an empty workspace, no
  persisted `~/.cache`/`target` between runs).
- **Warm**: the fixture was already built successfully once, nothing in
  its source changed, and it is rebuilt with the exact same source
  content still on disk (files were not touched — same mtime, same
  bytes). A build tool with a fingerprint/mtime-based cache should treat
  this as a no-op.
- **No-op**: a warm rebuild that a build tool actually recognizes as
  requiring zero re-execution. This is *not* automatically true of every
  warm rebuild — see the mtime pitfall below. `unchanged-noop-workspace`
  is the one fixture whose whole purpose is guaranteeing this state.

## Verified: cold → warm is a true no-op when files are never touched

```
$ cd fixtures/incremental-semantic-edit && cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.00s   # cold build already done earlier this session
$ cargo build --workspace -v | grep -E "Compiling|Fresh|Finished"
       Fresh leaf-b v0.1.0 (.../crates/leaf-b)
       Fresh leaf-c v0.1.0 (.../crates/leaf-c)
       Fresh leaf-a v0.1.0 (.../crates/leaf-a)
       Fresh aggregator v0.1.0 (.../crates/aggregator)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.00s
```

Same evidence pattern holds for `unchanged-noop-workspace` (its whole
point): `cargo build` immediately after `cargo test`/`cargo run` reports
`Finished ... in 0.00s` with nothing recompiled.

## Pitfall, verified: a content-identical file copy is *not* a no-op

`incremental-semantic-edit/scripts/revert-edit.sh` copies
`lib.baseline.rs` over `lib.rs` — byte-identical content to what was
there before the edit was ever applied. Cargo still recompiles it:

```
$ scripts/revert-edit.sh
reverted: crates/leaf-b is back to the 'baseline' variant
$ cargo build --workspace -v | grep -E "Compiling|Fresh|Finished"
       Fresh leaf-c v0.1.0 (.../crates/leaf-c)      # untouched — correctly cached
       Fresh leaf-a v0.1.0 (.../crates/leaf-a)      # untouched — correctly cached
   Compiling leaf-b v0.1.0 (.../crates/leaf-b)      # content-identical, but mtime changed
   Compiling aggregator v0.1.0 (.../crates/aggregator)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.10s
```

Cargo's default fingerprinting keys on mtime, and `cp` gives the
destination a fresh mtime regardless of content — so a "revert to
identical content" is a genuinely different state than "never touched":
call it **touched-unchanged**, distinct from true **no-op**. A future
Run/scenario harness that wants to test true no-op behavior after an
edit-then-revert cycle needs to either restore the original mtime (e.g.
`cp --preserve=timestamps`, or `touch -r`) or accept that
`incremental-semantic-edit`'s revert step is a touched-unchanged
measurement point, not a no-op one. `unchanged-noop-workspace` remains
the only fixture that isolates a true no-op with nothing else confounding
it.

The same run also confirms `incremental-semantic-edit/EDIT.md`'s
documented "expected invalidation set" claim with real evidence for the
first time: applying the edit recompiles exactly `leaf-b` and
`aggregator`, `leaf-a`/`leaf-c` stay `Fresh` — matching the doc, not just
asserted by it.

## Pitfall, verified: Nim's default cache is outside the project entirely

`nim c` with no `--nimcache` flag does not cache locally — it writes to
a per-invoking-user, per-project-*basename* global directory
(`~/.cache/nim/<name>_d` for a debug build on this platform; the exact
path is platform-dependent). `nim-heavy-workspace/src/fixture.nim`
compiles to the cache key `fixture` — a generic name with real collision
risk against unrelated Nim projects on the same machine, and a cache
that "cold" cannot mean just "no local `nimcache/` dir" for this
fixture, since a previous run leaves state entirely outside the
checked-out repository:

```
$ nim dump 2>&1 | grep -i cache        # (nothing — not surfaced by `dump`)
$ find ~/.cache/nim -maxdepth 1 -iname '*fixture*'
/Users/yoshinori/.cache/nim/fixture_d
```

Fixed in this session (`.github/workflows/ci.yml`): `nim-heavy-workspace`
now builds with `--nimcache:nimcache` like every other Nim-driven fixture
here (`rust-nim-c-abi-baseline`, `mixed-rust-nim-executable`,
`boundary-heavy-workload`, `direct-native-link` already pinned their
nimcache under `OUT_DIR`/a local directory via their `build.rs`/
`build.sh`). With that flag, "cold" for every Nim-touching fixture in
this directory now means the same thing: no local `nimcache/` under the
fixture, full stop — no hidden state outside the checked-out tree.

## Per-fixture cold-build commands

Every fixture below was verified this session to build successfully
from a state with no local `target/`/`nimcache/` present (CI does this
implicitly on every run; the Rust/Nim FFI fixtures were also re-verified
locally after removing `target/`):

| Fixture | Cold build |
|---|---|
| `rust-heavy-workspace` | `cargo build --workspace && cargo run -q -p fixture-bin` |
| `nim-heavy-workspace` | `nim c --nimcache:nimcache -o:fixture_out src/fixture.nim && ./fixture_out` |
| `rust-nim-c-abi-baseline` | `cd rust-bin && cargo run` |
| `wide-parallel-graph` | `cargo build --workspace && cargo test --workspace && cargo run -q -p aggregator` |
| `deep-critical-path-graph` | `cargo build --workspace && cargo test --workspace && cargo run -q -p fixture-bin` |
| `mixed-rust-nim-executable` | `cd rust-bin && cargo run` |
| `boundary-heavy-workload` | `cd rust-bin && cargo run` |
| `unchanged-noop-workspace` | `cargo test && cargo run && cargo build` |
| `incremental-semantic-edit` | `cargo build --workspace && cargo test --workspace && cargo run -q -p aggregator` |
| `direct-native-link` | `cargo test --manifest-path rust-lib/Cargo.toml && ./build.sh` |

These are exactly `.github/workflows/ci.yml`'s per-fixture steps — CI
*is* the reproducible cold-build check for all ten, on every push.
