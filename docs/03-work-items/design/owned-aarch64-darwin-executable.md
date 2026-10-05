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
same-object AArch64 branches. External and cross-object calls remain diagnosed;
they are never delegated.

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
| Darwin artifact writer | Encoded AArch64 text | A relocatable `MH_OBJECT` with an owned `__TEXT,__text` section and external entry symbol | The declared linker accepts the LAMINARIA object and macOS launches the linked executable | An inspectable owned target artifact and explicit runtime/link contract |

## ABI and current boundary

Generated owned functions use the Darwin AArch64 integer convention for this
subset: the first eight `i32` arguments are in `w0` through `w7`, and the
result is in `w0`. Locals, materialized intermediate values, and incoming
parameters use compiler-owned stack slots. Generated functions preserve `x19`
as their frame base, save their link register, and marshal evaluated call
arguments into `w0` through `w7` immediately before a direct `BL` to another
symbol in the same LAMINARIA object. The current process boundary accepts a
zero-argument owned `i32` source entry; external symbols, calls requiring more
than eight arguments, and cross-object relocation remain explicitly diagnosed.
The linked process entry is the LAMINARIA-produced `_main` joined to the Darwin
`libSystem` runtime by the declared link action. `owned-native-build --entry
NAME` may select another owned source function for that `_main` symbol. This
lets a conventional Rust `fn main()` wrapper remain in the same source file for
a Cargo+rustc comparison while LAMINARIA compiles the selected owned closure;
that wrapper is deliberately outside the selected LAMINARIA closure, so it
cannot create a duplicate Darwin `_main` symbol.

The target is deliberately host-specific.  `laminaria-run` now owns the
explicit Darwin link action: it writes the LAMINARIA-produced `_main` object
and calls the declared `ld` with a caller-supplied SDK root and deployment
target to provide only `libSystem`. A real source-derived zero-argument `i32`
entry has been linked and launched this way without Cargo, rustc, Nim, a C
compiler, or an assembler. Non-AArch64-Darwin requests are
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
