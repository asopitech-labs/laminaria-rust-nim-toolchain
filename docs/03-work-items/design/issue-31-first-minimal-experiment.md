# Issue #31 — first minimal cross-language owned-transformation experiment

## Decision being tested

Can LAMINARIA compose source-derived Rust and Nim semantic IR before an ABI/backend boundary, apply one owned transformation across that language boundary, and generate an executable owned WebAssembly artifact?

This is G1 of the current milestone fixed by
`docs/near-term-research-program.md` and selected under
`docs/01-foundations/research-prioritization-policy.md`. It tests the largest unresolved
LAMINARIA-specific claim before G2 can compare fused and split boundaries or
G3 can broaden the input forms. It does not ask for a finished mixed-language
compiler.

## Fixed input

Create one fixture with exactly two source units.

Rust consumer:

```rust
fn rust_entry(x: i32) -> i32 {
    cross_double(x).wrapping_add(1)
}
```

Nim producer:

```nim
proc cross_double(x: int32): int32 =
  x +% x
```

The project/fixture contract declares that `rust_entry` requires
`cross_double(i32) -> i32` and that the Nim unit provides it. Existing Rust or
Nim compilers must not compile either target unit.

## Required experiment

1. Lower each real source unit with the existing LAMINARIA frontends. Add only
   the minimum incomplete-unit/declaration representation needed to let the
   Rust unit name the declared Nim-provided function before composition.
2. Compose the two units into one `Program`. Preserve the original
   `SourceLanguage`, file and span provenance of every definition and node.
3. Resolve the declaration to the Nim definition and validate the complete
   composed program. No unresolved call may reach transformation or codegen.
4. Apply the existing owned `checked_inline` transformation with
   `caller=rust_entry`, `callee=cross_double`.
5. Confirm structurally that the transformed `rust_entry` no longer calls
   `cross_double`, while the transformed nodes still identify their Nim source
   provenance.
6. Generate WebAssembly through the existing owned `generate_wasm_module`
   route and execute `rust_entry` for `0`, `1`, `-1`, `i32::MAX` and
   `i32::MIN`.
7. Compare three results for every input: composed program before inlining via
   the owned interpreter, transformed program via the owned interpreter, and
   generated WebAssembly. Expected values are respectively `1`, `3`, `-1`,
   `-1`, and `1`.
8. Add one negative fixture where the Rust declaration expects two `i32`
   parameters but the Nim definition provides one. Composition must return a
   structured signature-mismatch diagnostic before transformation/codegen.
9. Retain a negative guard showing that this path never invokes `rustc`, the
   Nim compiler, LLVM, or another target compiler/backend.

The representation and API names are implementation choices. The semantic
boundary above, fixed source inputs, expected values, provenance requirement,
and rejection point are not.

## Evidence to report

- source-derived IR before composition;
- declaration-to-definition resolution and provenance after composition;
- transformed IR showing the cross-language call removal;
- the five three-way result comparisons;
- the structured mismatch diagnostic;
- exact changed files and tests.

No performance claim is required. Record code size or timing only if already
available without building new measurement infrastructure.

## Stop condition

Stop when the positive and negative cases establish one of these conclusions:

1. the current candidate IR can represent and transform this cross-language
   relation;
2. it cannot, and the exact missing semantic fact or representation boundary is
   identified; or
3. early ABI/backend lowering is shown necessary for this case, with evidence.

Any of the three is a valid research result. Do not extend the task to the
reverse direction, multiple files per language, general linking, runtime
capabilities, incremental reuse, resource accounting, distributed execution,
all language features, or production CLI integration.

Report the G1 conclusion and stop. Do not begin G2 or G3 from this task; the
project roadmap must be updated or reaffirmed from the G1 evidence first.
