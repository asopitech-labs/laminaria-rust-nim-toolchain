# `incremental-semantic-edit` fixture

## Evidence classification correction (2026-09-10)

This file preserves historical fixture/measurement and implementation evidence, not the current research delivery order. Existing-compiler builds and driver self-builds recorded below are **reference/bootstrap/delegated-build baselines**, not proof of LAMINARIA compiler ownership or independent self-hosting. The [compiler ownership contract](../../docs/compiler-ownership-contract.md) governs current issue acceptance; historical checklists do not close the revised requirements.


#11's "incremental semantic edit" Core workload: a workspace with one
*designated* single-line semantic edit, a fixed expected pre/post value
for it, and an explicit expected invalidation set — the shape a future
Run/scenario harness (#19-21) needs to check this issue's acceptance
criterion that "controlled incremental workloads can fail when
unexpected extra actions execute even if the final artifact is
correct."

## Topology

Three independent leaf crates (`leaf-a`, `leaf-b`, `leaf-c`), each with
its own real, unit-tested computation, and one `aggregator` binary that
depends on all three and sums their results.

## The designated edit

`crates/leaf-b/src/lib.rs`'s `value()` changes from the product of the
first 5 primes to the product of the first 6 primes (adds a `* 13`
factor). `crates/leaf-b/src/lib.edited.rs` is the committed post-edit
source; `crates/leaf-b/src/lib.baseline.rs` is a pristine backup of the
pre-edit source. Neither file is part of the build by itself — Cargo
only compiles `src/lib.rs` — so applying/reverting the edit means
swapping which content sits at that path:

```bash
scripts/apply-edit.sh    # crates/leaf-b/src/lib.rs <- lib.edited.rs
scripts/revert-edit.sh   # crates/leaf-b/src/lib.rs <- lib.baseline.rs
```

## Expected values

| State    | `leaf-b::value()` | `aggregator` total |
|----------|-------------------|---------------------|
| baseline | 2310               | 839875              |
| edited   | 30030              | 867595              |

`leaf-a::value()` (5525) and `leaf-c::value()` (832040) never change.
`aggregator/src/main.rs` reads `leaf_b::VARIANT` at runtime and asserts
the total that matches whichever variant is actually compiled in, so
`cargo run -p aggregator` is correct in both states without external
bookkeeping.

## Expected invalidation set

Applying the edit and rebuilding should recompile exactly:

- `leaf-b` (the crate whose source changed);
- `aggregator` (depends on `leaf-b`).

It should **not** recompile `leaf-a` or `leaf-c` — neither their source
nor anything they depend on changed. A build system that recompiles
either of them after `scripts/apply-edit.sh` is exhibiting exactly the
"unexpected extra actions" failure mode this issue's acceptance
criteria call out, even if `aggregator`'s printed total is still
correct.

## Usage

```bash
cargo build --workspace && cargo test --workspace
cargo run -q -p aggregator        # baseline: total=839875

scripts/apply-edit.sh
cargo run -q -p aggregator        # edited: total=867595

scripts/revert-edit.sh
cargo run -q -p aggregator        # back to baseline: total=839875
```

CI exercises exactly this apply → run → revert → run cycle so the
fixture's own build/test/edit/revert path stays reproducible, even
though today's CI has no Run/scenario harness (#19-21) yet to record
which actions actually re-executed.
