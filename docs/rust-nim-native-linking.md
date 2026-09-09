# Rust–Nim Native Linking Research

## Objective

LAMINARIA investigates a Rust/Nim integration path in which both languages participate in one native artifact and link plan without requiring the cross-language boundary to be expressed first as a conventional exported C ABI.

This is a compiler/toolchain research problem, not a promise that arbitrary Rust and Nim language values can be exchanged directly. The work must distinguish native object/link compatibility from language-level semantic compatibility.

## Why this matters

The normal mixed-language route is usually:

```text
Language A
  -> C-compatible exported surface
  -> header/binding description
  -> C ABI
  -> Language B declaration
  -> native linker
```

That model is robust and portable, but it makes the C ABI the semantic bottleneck even when both compilers ultimately produce native objects for the same platform and linker.

LAMINARIA asks a different question:

```text
Rust semantic/codegen pipeline ----┐
                                  ├-> shared artifact/link model -> final native artifact
Nim semantic/codegen pipeline -----┘
```

The goal is not to eliminate all adapters. The goal is to determine where adapters are actually necessary and where the toolchain can preserve a richer direct contract.

## Research questions

1. Which Rust and Nim outputs can already participate in the same native link without an intervening C source/header contract?
2. Which symbol naming and visibility rules prevent direct reference across the two compilation pipelines?
3. Can LAMINARIA generate or control symbol identities before object generation instead of relying on a C-facing export layer?
4. Which scalar, aggregate, pointer/reference, string, sequence, closure, exception, panic, ownership, destructor and runtime concepts have compatible representations, and which require adapters?
5. What runtime initialization and teardown obligations exist when Nim-generated objects and Rust-generated objects live in the same process image?
6. Can cross-language call edges participate in LTO, whole-program optimization, dead-code elimination or linker GC under any supported backend combination?
7. How do debug information, stack unwinding and symbolization behave across direct boundaries?
8. What changes for static executables, shared libraries and WebAssembly targets?
9. Can the contract be made explicit enough for precise cache invalidation and reproducible builds?

## Experimental layers

### Layer 1 — Object/link compatibility

Start below language semantics.

For each target/toolchain pair, record:

- object format;
- architecture and target triple;
- relocation model;
- symbol visibility;
- name mangling;
- calling convention metadata;
- section layout relevant to runtime initialization;
- linker and archive behavior.

The first proof should be minimal: one Rust-produced object and one Nim-produced object in the same link, with an intentionally simple symbol relationship and no generated C header contract.

### Layer 2 — Symbol contract

Investigate whether LAMINARIA can define a stable cross-language symbol identity independently of user-facing C exports.

Evidence must include `nm`/`objdump`/platform-equivalent symbol inspection, relocation records and final linked symbols.

### Layer 3 — Type/layout contract

Build a compatibility matrix rather than assuming equivalence.

Candidate classes:

- fixed-width integers;
- floats;
- booleans;
- pointers;
- fixed-layout records/structs;
- arrays;
- slices/open arrays;
- strings;
- sequences/vectors;
- enums/tagged unions;
- closures/function values;
- opaque handles.

For every accepted class, record size, alignment, field offsets, ownership, lifetime and mutation rules. If a class requires a generated adapter, the adapter becomes an explicit Action Graph node and artifact.

**Working evidence, `fixtures/direct-native-link/NOTES.md`**: fixed-layout records/structs are safe to share **by pointer** — an independently-declared Rust `#[repr(C)] struct` and Nim `{.bycopy.} object` agree in size/alignment/field offsets, verified by both sides computing their own layout at runtime and cross-checking, on every Nim-side route tested (`nim c` and `nlvm`). Sharing the same struct **by value** (as an argument or return value) is *not* uniformly safe: it works correctly on the `nim c` route but is broken in every shape tested on `nlvm` (silently wrong as a lone argument, silently wrong as a return value, an outright crash as an argument followed by more parameters) — `nlvm`'s own compiler self-reports the return case as an incomplete TODO. By-value aggregate passing is therefore backend-route-dependent and not currently part of this compatibility matrix's accepted baseline; by-pointer is the verified-safe pattern across every route tested so far, and the practical default this project's own fixtures already converged on independently. Fixed-width integers, floats and raw pointers (Layer 1-2 scalars) show no such route-dependence.

### Layer 4 — Runtime and failure semantics

Investigate:

- Nim runtime initialization;
- allocator ownership;
- ARC/ORC or other memory-management obligations;
- Rust allocator interaction;
- panic/unwind behavior;
- Nim exception propagation;
- thread attachment and TLS;
- destructor/finalizer execution;
- process and library teardown.

Crossing a boundary with incompatible unwind/failure semantics must fail closed or use an explicit translation adapter.

### Layer 5 — Optimization

Compare at least:

1. conventional C ABI baseline;
2. direct native-object boundary with adapters only where required;
3. any backend/link mode that permits broader optimization.

Measure:

- call overhead;
- code size;
- inlining/LTO evidence when applicable;
- dead-code elimination;
- duplicate runtime/support code;
- link time;
- incremental rebuild scope.

Do not infer optimization from flags alone. Inspect the resulting artifacts and executed path.

### Layer 6 — WebAssembly

For WASM, evaluate separately:

- producing one final module from both language pipelines where toolchains permit it;
- module/component boundaries when a single link is not possible;
- generated canonical/adaptation layers;
- duplicated runtime state;
- code size and boundary cost;
- whether direct graph-level integration improves invalidation, scheduling or artifact reuse even when a runtime ABI remains necessary.

## Relationship to C ABI

C ABI remains an important baseline and compatibility mechanism. LAMINARIA should not remove it from the research model.

The comparison is:

```text
C ABI as mandatory architectural boundary
versus
C ABI/adapters as one possible boundary artifact chosen only where required
```

The project succeeds even if some data classes or targets continue to require a C-compatible adapter, provided the reasons and costs are explicit and the rest of the native link is not unnecessarily constrained by that adapter model.

## Required evidence

Every experiment must commit or reproducibly generate:

- source for both languages;
- compiler and linker versions;
- exact commands or LAMINARIA plan;
- target and backend configuration;
- object/archive/module inventory;
- symbol and relocation inspection;
- runtime output;
- failure behavior;
- size and resource measurements where relevant;
- comparison against the C-ABI baseline;
- explanation of any generated adapter or fallback path.

## Non-goals

- claiming arbitrary Rust and Nim values are layout-compatible;
- inventing a new universal ABI before measurements justify it;
- bypassing language runtime requirements;
- treating unsafe transmutation as an interoperability design;
- hiding C shims while describing the path as ABI-free;
- replacing either compiler frontend.

## Success criteria

This research track is successful when LAMINARIA can answer, with reproducible evidence, all of the following for a supported target:

1. which Rust and Nim units are linked together;
2. what cross-language symbols and artifacts connect them;
3. which parts require no C ABI adapter;
4. which parts do require an adapter and why;
5. which runtime obligations are enforced;
6. what invalidates each artifact;
7. how the direct path compares with the conventional C ABI baseline in correctness, code size, build cost, runtime cost and optimization opportunity.
