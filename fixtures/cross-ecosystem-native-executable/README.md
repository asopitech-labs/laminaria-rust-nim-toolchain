# cross-ecosystem-native-executable (issue #48, G1)

## Category (per `docs/01-foundations/fixture-policy_ja.md` §7)

`scenario-state`: a fixed, real, minimal mixed-ecosystem project used
as an **input** to LAMINARIA's own dependency-obligation resolver
(`laminaria_plan::dependency_graph::resolve`, fed by
`crates/laminaria-run/src/cross_ecosystem_ingest.rs`'s real ingestion
functions). This directory is never itself the source of expected
values -- the resolver's own typed obligations, and the real `cargo
metadata`/`nimble dump`/header/`exportc` facts gathered from this
project, are what the direct tests in `cross_ecosystem_ingest.rs`
assert against. No separate YAML catalog, fixture-only validator, or
validator test exists or should be added for this fixture
(fixture-policy §4.8).

## What is fixed here

- **`app/`** -- one Cargo crate (binary `app`), with an explicit
  feature (`use_nim_double`, default-on) and a target condition
  (`#[cfg(unix)]`-gated foreign declarations).
- **`nimble/doubler/`** -- one real Nimble package, consumed by `app`'s
  `use_nim_double` feature. `doubler.nim`'s own `{.exportc: "nim_double".}`
  pragma is read directly (`laminaria_ir::nim_export_discover`), never
  inferred from a compiled archive. `nimble.lock` pins the package's
  own `requires "nim >= 2.0.0"` to the exact nim `2.2.10` this repo
  already pins elsewhere (`toolchains.lock.toml`'s `nim2_pinned`) -- a
  real nimble artifact (`nimble lock`'s own output), not a G1 code
  workaround, and required so `nimble dump --json`'s own
  `nimDir`-reporting toolchain lookup resolves against this lock
  file's exact pinned revision on a fresh CI runner instead of
  crashing
  (see `crates/laminaria-run/src/command_runner.rs`'s own doc comment
  for the real CI failure this fixes).
- **`c/cadd/v1/`** -- one C library exporting `c_add`: `cadd.c` includes
  `cadd.h`, and `cadd.h`'s own prototype (`int c_add(int a, int b);`) is
  the correct provider for `app`'s `extern "C" { fn c_add(...) }`
  requirement.
- **`c/cadd/v2/`** -- a real, *incompatible* `cadd` variant: `cadd.c`
  includes `cadd.h`, and `cadd.h` genuinely declares `c_add_v2`, not
  `c_add` -- the fixed negative case (issue #48's own "a library
  variant that cannot provide the required symbol" example), not a
  fabricated missing-symbol claim.
- **`cpp/cppmax/`** -- one C++ library whose `max_value<T>` template has
  no directly linkable symbol; `cppmax.cpp` includes `cppmax.h`, whose
  `extern "C"`-wrapped prototype declares the explicit adapter/
  instantiation unit issue #48 requires, `cpp_max_i32`.

All four ecosystems' real outputs feed the same observable result:
`c_add(nim_double(cpp_max_i32(3, 4)), 1)`.

## Cross-layer feedback case

`app/src/main.rs`'s real `extern "C"` declarations (discovered via
`laminaria_ir::foreign_discover::discover_foreign_function_requirements`,
a real syn-based scan, never fabricated) each carry a real
`#[link(name = "...")]` hint. The resolver looks up real candidates for
the named package and selects the one whose real *declared* export --
a C/C++ header prototype (`laminaria_ir::c_header_discover`) or a Nim
`{.exportc.}` pragma (`laminaria_ir::nim_export_discover`), never a
compiled artifact's symbol table -- actually matches the required
symbol. When both `cadd` v1 and v2 are offered as candidates, the
source-derived requirement for `c_add` is what selects v1 and rejects
v2, not a version-only package choice made in advance.

## G1's own boundary (read, normalize, resolve, plan -- never build)

G1 (this fixture's own ingestion + resolution) never invokes a
compiler, archiver, linker, package build, package install, or build
script, in either the positive or the negative case. It reads real
source/header text and real `cargo metadata`/`nimble dump --json`
output, and emits a typed action *plan* for issue #46 (G2) to execute.
Actually compiling and linking the four archives into one running
`app` executable, running it, and testing it end to end are G2's own
scope (and issue #49/Lane C's), not this fixture's tests.

## Fixed Lane C minimal information (M1-W0)

- **Exact production subject identity**: the `app` binary that issue
  #46 (G2) will eventually build from this fixture's own
  `Cargo.toml`/`src/main.rs`, linked against the archives G2 produces
  from `cadd/v1`, `cppmax`, and `doubler` per the `RequiredAction` chain
  G1 emits -- not built or linked by G1's own tests.
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
- `nimble dump --json` (run inside `nimble/doubler/`; `nimble.lock`
  in that same directory is what makes this resolve correctly against
  real CI's nimble, not an added flag -- see this file's own note
  above)
- `rustc -vV` (real host target triple)

That is the complete list. `crates/laminaria-run/src/command_runner.rs`
enforces this exact allowlist structurally (`RealCommandRunner` refuses
anything else; the required tests inject a `RecordingCommandRunner`
that panics immediately on a forbidden command). No compiler, archiver,
linker, `nm`, `cargo build`/`cargo run`, or `nimble build`/`nimble
install` is ever invoked by G1 -- declared-export facts come from
reading `c/cadd/{v1,v2}/cadd.h`, `cpp/cppmax/cppmax.h`, and
`nimble/doubler/src/doubler.nim` as plain text.

## Non-goals (explicitly out of scope for this fixture)

Actually compiling, archiving, linking the four archives into one
running `app` executable, running it, and testing it end to end are
issue #46 (G2) and issue #49 (Lane C, C1) scope, not this one. This
fixture's own tests (in `cross_ecosystem_ingest.rs`) verify the
resolved obligation graph and its required G2 action plan, never a
linked binary.
