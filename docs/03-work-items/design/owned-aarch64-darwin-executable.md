# Owned AArch64 Darwin executable: first native product path

## Result

For the initial native core of the already supported pure `i32` Rust/Nim subset,
LAMINARIA accepts a
validated owned IR program and emits an `aarch64-apple-darwin` Mach-O object
file.  The object contains a selected owned-IR entry function under the
Darwin AArch64 integer ABI.  The
initial core is parameters, literals, and wrapping add/subtract/multiply;
calls, lets, and conditionals are diagnosed until their ABI/control-flow
lowering is added.  They are never delegated.

This is a product compiler path, not a comparison experiment: the source
frontends, validation, lowering, instruction selection, AArch64 encoding, and
Mach-O writer are all LAMINARIA code.  It neither invokes nor embeds Cargo,
`rustc`, Nim, a C/C++ compiler, assembler, or linker.

## Checkpoints

| Checkpoint | Consumes | Must preserve | Evidence | Enables |
| --- | --- | --- | --- | --- |
| Native executable model | `ValidatedProgram`, selected entry and `i32` arguments | The existing source provenance, validation boundary, wrapping-i32 semantics and lexical `let` scope | The backend rejects an absent entry or wrong arity before writing bytes | A source-derived program can become an owned native artifact |
| AArch64 lowering | `Expr`/`Stmt`/`FnFact` | Source-order argument evaluation, two's-complement wrapping operations, calls and nested local shadowing | Generated machine code executes the same entry result as the interpreter for the supported subset | Native execution is a semantic consumer, not a hand-written duplicate fixture |
| Darwin artifact writer | Encoded AArch64 text | A `MH_EXECUTE` header, one executable `__TEXT,__text` section, and an `LC_MAIN` entry offset | macOS executes the emitted file directly; no intermediate object or external linker exists | The first native target artifact and later explicit runtime/link contracts |

## ABI and current boundary

Generated owned functions use the Darwin AArch64 integer convention for this
subset: the first eight `i32` arguments are in `w0` through `w7`; the result
is in `w0`; calls preserve the link register around nested calls.  Locals use
compiler-owned stack slots.  The generated process entry materializes the
declared test/application arguments, calls the selected function, and invokes
the Darwin `exit` system call directly.  There is no libc runtime contract in
this first slice.

The target is deliberately host-specific.  `laminaria-run` now owns the
explicit Darwin link action: it writes the LAMINARIA-produced `_main` object
and calls the declared `ld` with a caller-supplied SDK root and deployment
target to provide only `libSystem`.  A real source-derived `fn main() -> i32`
has been linked and launched this way without Cargo, rustc, Nim, a C compiler,
or an assembler. Non-AArch64-Darwin requests are
diagnosed rather than delegated to a host compiler.  C/C++ foreign components
remain separate declared actions and do not participate in lowering Rust or
Nim target source.

## Comparison discipline

Cargo+rustc remains the practical baseline for the same workload.  It is a
comparison target for correctness, startup, artifact size, and rebuild work;
it is never invoked to create LAMINARIA's target artifact and is not a
precondition for extending this owned path.
