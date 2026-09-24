# Issue #50 R0 — Rust workload and oracle lock

## Result

R0 fixes one locked, Rust-only positive workload and its eager Cargo reference
execution set. It does not claim that LAMINARIA currently implements the
cross-layer pruning or that any resource saving has been measured. Those are
R1/R2 questions.

The workload is the existing three-package workspace at
`fixtures/rust-heavy-workspace`:

```text
fixture-bin -> fixture-mid -> fixture-core
```

Identity at the R0 run:

- repository HEAD: `d87f9e8a1c45285274fdf24d5f841c1a04e91869`
- workspace manifest blob: `db0601a5604542a28b56a85e816388ee08ddd1db`
- lockfile blob: `25651b0e2588cc6b88b7b866429b4d37eefbe12e`
- fixture source and lockfile were unchanged in the working tree; the root
  repository had unrelated pre-existing edits.

The fixture has a real inter-package dependency and a reachable generic. The
binary calls `sum_generic` for `i64`; `fixture-core`'s actual unit test calls
the same generic for `i32`. This creates a useful demand distinction without
inventing a second implementation or synthetic fixture.

## Requested artifact and acceptance contract

Positive request: build and run the host-native `fixture-bin` executable from
the locked workspace. Accept only if the original unit tests pass and the
direct executable produces these four output lines, in order:

```text
points=45
perimeter=391
centroid=Some(Point { x: 89, y: 93 })
sum_x=4028
```

R0's observed Cargo oracle passed: `fixture-core` 3/3 tests,
`fixture-mid` 2/2 tests, binary/doc test targets completed, and the executable
printed the output above. This is behavioral evidence; R0 does not require
byte-for-byte executable identity.

## Eager reference and R1 demand counterexample

The eager reference command, now part of the repository-owned bootstrap image
build, is:

```sh
cd fixtures/rust-heavy-workspace
cargo build --workspace
cargo test --workspace
cargo run -q -p fixture-bin
```

Its selected target set includes all three workspace members and the unit-test
harnesses, including the `fixture-core` test that instantiates
`sum_generic<i32>`.

The artifact-rooted R1 request is specifically the `fixture-bin` executable,
not “all workspace tests.” Its required production closure includes
`fixture-bin`, `fixture-mid`, `fixture-core`, and the `sum_generic<i64>`
instantiation. The test harnesses and `sum_generic<i32>` are not required by
that binary request. Conversely, under the explicit `fixture-core` test
request, the `i32` instantiation is live and must not be pruned. This pair is
the frozen positive/negative selection relation for R1:

| Request | Required | Must not be selected as required work |
| --- | --- | --- |
| `fixture-bin` executable | three-package production dependency closure; `sum_generic<i64>` | test harnesses; test-only `sum_generic<i32>` |
| `fixture-core` unit tests | core test harness; `sum_generic<i32>` | unrelated `fixture-mid` / `fixture-bin` targets |

“Must not be selected” is demand-relative, not a claim that Cargo can avoid
all parsing or metadata inspection. R1 must report its graph-level work
boundary and reject unsupported semantics explicitly; it must not silently
fall back to Cargo/rustc as the producer for an owned Rust target.

## Environment and evidence identity

The run used the repository's serialized Windows WSLC owner harness in full
mode (`scripts/windows-wslc-ci.ps1 -Mode full`), then captured the canonical
doctor report with `scripts/windows-wslc-ci.ps1 -Mode doctor`. The doctor
image identity was `sha256:44e7679f7c598fe7f84311409edf657897ce00f7cb4c5a6eff6846432d33c62c`.
It is built from the repository-pinned Debian Bookworm-slim base image
(`sha256:3783cc01769c7b2b1b83a5c5ad96c815348e28ed7da68e2e3687004faa906251`).
The measured environment was Debian GNU/Linux 12 (bookworm), kernel
`6.18.40.1-microsoft-standard-WSL2`, x86_64, Intel Core i7-1260P (doctor
reported 1 physical / 16 logical cores), 15.4 GiB memory, `overlayfs`, and
`environment_class=container`. Rust `stable` resolved to `1.98.1`
(`48a229cea`, target `x86_64-unknown-linux-gnu`, LLVM `22.1.8`); the
`rustc` binary SHA-256 was
`859254978c0a0402c32f949f6de0d99aee73be8d15f45aac00ae1448aac51e74`.
Nim was pinned to `2.2.10` (Linux/amd64). Doctor also observed Clang/LLVM
14.0.6, LLD 22.1.8, Binaryen `wasm-opt` 108, and `wasm-tools` 1.255.0.
This is a WSLC/container correctness baseline, not native-Windows performance
data.

For future comparable measurements, preserve the canonical
`EnvironmentFingerprint` fields from
`docs/02-research-areas/measurement/measurement-foundation.md`: OS/kernel and
architecture, CPU/memory/filesystem, virtualization/container/WSL class and
resource limits, source revision/dirty state, exact resolved compiler and
target, workload/feature request, and harness/image identity. Doctor reported
`sdk_path` as unobserved; the harness did not report a separate container
resource-limit value. These remain explicitly unknown rather than inferred.
No performance samples are published.

## Verification outcome and boundary

In the same serialized full owner-harness invocation, Cargo format check,
Clippy (`--workspace --all-targets -- -D warnings`), all workspace tests, and
the Nim planning-kernel tests passed. The fixture's Cargo build/test/run oracle
also passed. The harness then failed only while publishing its verification
receipt to `.git/laminaria-wslc-verification.json` because the managed
workspace denied that write. Therefore the local checks are observed passing,
but a source-bound receipt was not produced and the Windows hook/commit gate is
not satisfied by this run.

R0 stops here. No timing, CPU, RSS, I/O, disk, energy, or work-elimination
claim is made. R1 owns implementation of the demand feedback; R2 owns matched
resource evidence and causal attribution.
