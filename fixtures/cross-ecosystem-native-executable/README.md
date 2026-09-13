# cross-ecosystem-native-executable (issue #48, G1)

## Category (per `docs/01-foundations/fixture-policy_ja.md` §7)

`scenario-state`: a fixed, real, minimal mixed-ecosystem project used
as an **input** to LAMINARIA's own dependency-obligation resolver
(`laminaria_plan::dependency_graph::resolve`, fed by
`crates/laminaria-run/src/cross_ecosystem_ingest.rs`'s real ingestion
functions). This directory is never itself the source of expected
values -- the resolver's own typed obligations, and the real `nm`/
`cargo metadata`/`nimble dump` facts gathered from this project, are
what the direct tests in `cross_ecosystem_ingest.rs` assert against.
No separate YAML catalog, fixture-only validator, or validator test
exists or should be added for this fixture (fixture-policy §4.8).

## What is fixed here

- **`app/`** -- one Cargo crate (binary `app`), with an explicit
  feature (`use_nim_double`, default-on) and a target condition
  (`#[cfg(unix)]`-gated foreign declarations).
- **`nimble/doubler/`** -- one real Nimble package, consumed by `app`'s
  `use_nim_double` feature.
- **`c/cadd/v1/`** -- one C library exporting `c_add`, the correct
  provider for `app`'s `extern "C" { fn c_add(...) }` requirement.
- **`c/cadd/v2/`** -- a real, *incompatible* `cadd` variant that
  genuinely exports `c_add_v2` instead of `c_add` -- the fixed
  negative case (issue #48's own "a library variant that cannot
  provide the required symbol" example), not a fabricated
  missing-symbol claim.
- **`cpp/cppmax/`** -- one C++ library whose `max_value<T>` template
  has no directly linkable symbol; `cpp_max_i32` is the explicit
  `extern "C"` adapter/instantiation unit issue #48 requires.

All four ecosystems' real outputs feed the same observable result: 
`c_add(nim_double(cpp_max_i32(3, 4)), 1)`.

## Cross-layer feedback case

`app/src/main.rs`'s real `extern "C"` declarations (discovered via
`laminaria_ir::foreign_discover::discover_foreign_function_requirements`,
a real syn-based scan, never fabricated) each carry a real
`#[link(name = "...")]` hint. The resolver looks up real, already-built
candidates for the named package and selects the one whose real `nm`
output actually provides the required symbol -- when both `cadd` v1 and
v2 are offered as candidates, the source-derived requirement for
`c_add` is what selects v1 and rejects v2, not a version-only package
choice made in advance.

## Fixed Lane C minimal information (M1-W0)

- **Exact production subject identity**: the `app` binary built from
  this fixture's own `Cargo.toml`/`src/main.rs`, linked against the
  real archives `cross_ecosystem_ingest.rs` produces from `cadd/v1`,
  `cppmax`, and `doubler` (issue #46/G2's own scope to actually link
  and hand to a Lane C harness -- not built or linked by G1's own
  tests).
- **Expected exit status**: `0`.
- **Expected stdout**: `9\n` (`cpp_max_i32(3, 4)` = `4`; `nim_double(4)`
  = `8`; `c_add(8, 1)` = `9`).
- **Expected stderr**: empty.
- **Target environment**: the host triple `rustc -vV` reports on the
  machine that ingests this fixture (no cross-compilation in this
  slice).
- **Negative-dependency expected result**: with only `cadd/v2` offered
  as a candidate for the `cadd` package, `resolve` returns a structured
  `GraphRejection` (`RejectionReason::MissingSymbol`, naming the
  `Symbol:c_add` obligation) before any G2 action request is generated
  -- never a compiler invocation, never a partial/opaque build.

## Real toolchain commands this fixture's own ingestion uses

- `cargo metadata --no-deps --format-version 1 --manifest-path app/Cargo.toml`
- `nimble dump --json` (run inside `nimble/doubler/`)
- `cc`/`c++` + `ar` (compiles `c/cadd/{v1,v2}/cadd.c` and
  `cpp/cppmax/cppmax.cpp` into real static archives)
- `nim c --app:staticlib --noMain` (compiles `nimble/doubler/src/doubler.nim`
  into a real static archive -- the same technique
  `fixtures/rust-nim-c-abi-baseline/rust-bin/build.rs` already uses)
- `nm -g --defined-only` (real exported-symbol inspection of every
  produced archive)
- `rustc -vV` (real host target triple)

None of the above ever executes `app`'s own Rust source, `cargo build`,
`cargo run`, or `nimble build`/`nimble install` -- see
`cross_ecosystem_ingest.rs`'s own
`app_own_rust_source_is_never_compiled_or_linked_by_this_ingestion_module`
regression guard.

## Non-goals (explicitly out of scope for this fixture)

Actually linking the four archives into one running `app` executable,
running it, and testing it end to end are issue #46 (G2) and issue #49
(Lane C, C1) scope, not this one. This fixture's own tests (in
`cross_ecosystem_ingest.rs`) verify the resolved obligation graph and
its required G2 action requests, never a linked binary.
