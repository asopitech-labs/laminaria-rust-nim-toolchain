# Owned AArch64 Darwin executable: first native product path

## Result

For the initial native core of the already supported pure `i32` Rust/Nim subset,
LAMINARIA accepts a
validated owned IR program and emits an `aarch64-apple-darwin` Mach-O object
file.  The object contains a selected owned-IR entry function under the
Darwin AArch64 integer ABI.  The
initial core is parameters, literals, and wrapping add/subtract/multiply;
lexical `let` bindings and `if EXPR != 0 { ... } else { ... }` are lowered
as owned stack operations and in-function AArch64 control flow. Calls among
functions discovered from the selected source entry are lowered to direct
same-object AArch64 branches. A conventional argument-free Rust `fn main()`
may terminate through the closed `std::process::exit(i32)` surface: LAMINARIA
represents that separately from ordinary value calls and writes a real Mach-O
external branch relocation to Darwin C `exit` in `libSystem`. Other external
and cross-object calls remain diagnosed; they are never delegated.

This is a product compiler path, not a comparison experiment: the source
frontends, validation, lowering, instruction selection, AArch64 encoding, and
Mach-O writer are all LAMINARIA code. It neither invokes nor embeds Cargo,
`rustc`, Nim, a C/C++ compiler, or assembler. A completed LAMINARIA object
is subsequently joined to the Darwin system runtime by one explicit,
caller-declared `ld` action; that action does not compile target source.

## Checkpoints

| Checkpoint | Consumes | Must preserve | Evidence | Enables |
| --- | --- | --- | --- | --- |
| Native executable model | `ValidatedProgram`, selected entry and `i32` arguments | The existing source provenance, validation boundary, wrapping-i32 semantics and lexical `let` scope | The backend rejects an absent entry or wrong arity before writing bytes | A source-derived program can become an owned native artifact |
| AArch64 lowering | `Expr`/`Stmt`/`FnFact` | Source-order evaluation, two's-complement wrapping operations, lexical local shadowing, parameter values across nested calls, and the selected conditional arm | Generated machine code executes `let`, both `if` branches, and transitive source calls with the same entry result as the interpreter for the supported subset | Native execution is a semantic consumer, not a hand-written duplicate fixture |
| Darwin artifact writer | Encoded AArch64 text and closed external targets | A relocatable `MH_OBJECT` with owned `__TEXT,__text`, internal direct branches, and `ARM64_RELOC_BRANCH26` records only where a declared external target needs the linker | The object has an `N_UNDF|N_EXT` `_exit` reference, no `LC_DYSYMTAB`, and the declared linker launches the conventional source `main` | An inspectable owned target artifact and explicit runtime/link contract |

## ABI and current boundary

Generated owned functions use the Darwin AArch64 integer convention for this
subset: the first eight `i32` arguments are in `w0` through `w7`, and the
result is in `w0`. Locals, materialized intermediate values, and incoming
parameters use compiler-owned stack slots. Generated functions preserve `x19`
as their frame base, save their link register, and marshal evaluated call
arguments into `w0` through `w7` immediately before a direct `BL` to another
symbol in the same LAMINARIA object. The ordinary process boundary accepts a
zero-argument Rust `fn main()` with unit result and the exact final statement
`std::process::exit(EXPR);`. It evaluates `EXPR` as owned i32 code, puts the
result in `w0`, and emits a `BL` with an external
`ARM64_RELOC_BRANCH26` record to `_exit` (Darwin's object-file spelling for C
`exit`, not the separate POSIX `_exit` API). The linked process entry is the
LAMINARIA-produced `_main` joined to `libSystem` by the declared link action.
The explicit `--entry NAME` value path remains compatible for a selected owned
i32 closure, but it is not the regular comparison route.

The target is deliberately host-specific.  `laminaria-run` now owns the
explicit Darwin link action: it writes the LAMINARIA-produced `_main` object
and calls the declared `ld` with a caller-supplied SDK root and deployment
target to provide only `libSystem`. A real source-derived conventional `main`
has been linked and launched this way without Cargo, rustc, Nim, a C compiler,
or an assembler. This small process boundary maps to C `exit`; it does not yet
reproduce Rust standard-library cleanup/handler behavior beyond the pure-i32,
single-threaded subset. Non-AArch64-Darwin requests are
diagnosed rather than delegated to a host compiler.  C/C++ foreign components
remain separate declared actions and do not participate in lowering Rust or
Nim target source.

## Comparison discipline

Cargo+rustc remains the practical baseline for the same workload.  It is a
comparison target for correctness, startup, artifact size, and rebuild work;
it is never invoked to create LAMINARIA's target artifact and is not a
precondition for extending this owned path.

`scripts/measure-owned-native-darwin.sh` fixes the initial comparison
procedure. It consumes `fixtures/owned-native-call-compare/src/main.rs` both
as an independent Cargo package and as input to `owned-native-build`, builds
each route clean, then repeats the same one-line semantic edit and records the
ordinary rebuild. Every observed command is wrapped in `laminaria run` at
Level 1, so the saved Run envelopes contain the qualified environment,
toolchain identities, process/resource trace, output artifact inventory, and
the exact command. The compiler bootstrap is recorded but outside the timed
commands: it cannot be silently charged to LAMINARIA while Cargo's compiler is
treated as pre-existing. The script also launches both products after each
build, checking their independent, manually derived exit values (75 then 86).
Its emitted samples are evidence for a Pareto comparison, not a fabricated
single-score declaration that either compiler has won in general.

### Measured result: conventional `main` versus the current Cargo+rustc path

On 2026-10-06, the fixed procedure ran five clean and five same-edit rebuild
repetitions per route from clean commit `797c03f3b2a2f29e09420fc2cd5c6e2462b12d5a`
on native arm64 macOS (18 logical cores, 48 GiB memory). The saved raw Run
envelopes and Level 1 traces are under
`/private/tmp/laminaria-owned-native-compare-797c03f`; every envelope records
that same clean commit and `dirty=false`. Both routes consumed the exact same
source file, including a conventional `main` whose body is
`std::process::exit(pack(combine(3), increment(4)));`. All ten Cargo products
and all ten LAMINARIA products launched successfully: the unchanged source
exited 75 and the identical `wrapping_add(1)` → `wrapping_add(2)` edit exited
86.

| Scenario and measured boundary | Cargo+rustc median | LAMINARIA median | Current result for this workload |
| --- | ---: | ---: | --- |
| Clean target build wall time | 159.204 ms | 21.408 ms | LAMINARIA is 7.44× shorter |
| Clean root-process peak RSS | 83.594 MiB | 35.438 MiB | LAMINARIA is 2.36× lower |
| Clean observed output footprint | 13 files / 865,104 B scanned / 430,976 B executable | 2 files / 50,791 B scanned / 50,080 B executable | LAMINARIA writes less; executable is 8.61× smaller |
| Rebuild after the same semantic edit, wall time | 94.949 ms | 21.290 ms | LAMINARIA is 4.46× shorter |
| Rebuild root-process peak RSS | 83.594 MiB | 35.578 MiB | LAMINARIA is 2.35× lower |
| Rebuild observed output footprint | 7 files / 862,753 B scanned | 2 files / 50,791 B scanned | LAMINARIA writes less |

For this currently implemented, pure-i32 transitive-call `main`, LAMINARIA is
better than the current Cargo+rustc route on every measured dimension above,
without bypassing the source's process entry. This is deliberately not a claim
that it is already the better general Rust compiler: Cargo provides Rust's full
runtime contract, while LAMINARIA's closed process boundary maps the exact
accepted exit call to C `exit` in `libSystem` and does not yet model all Rust
cleanup or handler semantics. Future feature work must rerun this procedure
rather than treating this narrow result as a standing performance exemption.
