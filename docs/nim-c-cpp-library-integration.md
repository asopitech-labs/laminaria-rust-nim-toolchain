# Nim C/C++ library reuse and foreign-native build integration

## Purpose

LAMINARIA must preserve a central practical property of Nim: code written in Nim can reuse the C and C++ ecosystem instead of reimplementing equivalent facilities. This requirement applies both to LAMINARIA's own implementation and to Nim projects compiled by LAMINARIA.

This document is governed by the [compiler ownership contract](compiler-ownership-contract.md). It distinguishes legitimate foreign-library compilation and linking from delegating Nim target compilation to Nim-generated C/C++.

Tracking issue: [#44](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/44).

## Upstream basis

The Nim backend documentation describes `nim c`, `nim cpp`, `importc`, `importcpp`, `compile`, `passL`, `dynlib` and header wrapping as the normal native integration surface:

- <https://nim-lang.org/docs/backends.html>
- <https://nim-lang.org/docs/manual.html>
- <https://github.com/nim-lang/c2nim>
- <https://github.com/nim-lang/Nim/blob/devel/compiler/extccomp.nim>

LAMINARIA need not reproduce Nim's generated-C implementation strategy in its owned compiler, but it must preserve the observable ability to use suitable C/C++ libraries.

## Two mandatory reuse surfaces

### LAMINARIA implementation

When an established C/C++ library covers required functionality, prefer reuse over a new Rust/Nim implementation when correctness, platform coverage, license, maintenance, security, binary/runtime cost and build reproducibility are acceptable. Record the selection and rejection reasons. A language purity goal is not sufficient reason to duplicate a suitable implementation.

### Projects compiled by LAMINARIA

A supported Nim project may declare foreign functions, types and libraries. The owned Nim frontend and semantic IR must retain these declarations; target lowering emits external references rather than requiring the foreign implementation body in LAMINARIA IR. The build graph then produces or resolves the foreign native artifact and links it with the LAMINARIA-produced target artifact.

## Ownership boundary

```text
Nim source
  -> LAMINARIA parsing and semantic analysis
  -> owned IR with foreign declarations/calls
  -> LAMINARIA target lowering
  -> LAMINARIA-produced object

declared C/C++ dependency
  -> resolve identified prebuilt artifact
     OR compile identified C/C++ source
     OR generate and compile an explicit C++ adapter/instantiation unit
  -> foreign object/archive/shared library

LAMINARIA object + foreign native artifacts + runtime obligations
  -> explicit linker action
  -> final target artifact
```

An external C/C++ compiler is permitted for the declared foreign dependency branch. It is not permitted to compile C/C++ emitted as the implementation of a Rust/Nim target unit on the owned path. Every compile, archive and link action must have declared inputs, outputs, producer identity and lineage.

## Required semantic model

The source/semantic layer must represent at least:

- `ForeignDecl`: symbol or C++ expression identity, source language and linkage;
- `ForeignType`: scalar, pointer/reference, function pointer, opaque handle, enum, union and record layout;
- calling convention, variadic use, visibility and symbol decoration;
- mutability, aliasing and pointer provenance facts available from the declaration;
- ownership, allocation/free pairing, borrowed lifetime and callback lifetime;
- exception/unwind and failure boundary;
- header/module origin and conditional-compilation requirements;
- required library and target/runtime compatibility;
- whether a direct external symbol exists or an adapter/instantiation is required.

Support for a construct means preserving its semantics through target code and link production. Merely accepting a pragma and discarding it is not support.

## Required Program/Action Graph model

The graph must distinguish:

- header sets and generated binding inputs;
- C and C++ source units;
- generated C++ adapter or template-instantiation units;
- compile definitions, include paths, language standard and feature flags;
- object, static archive, import library and shared library artifacts;
- archive construction and final link actions;
- library search paths, link order, whole-archive/dead-strip behavior and runtime search paths;
- target triple, object format, ABI, sysroot/SDK, compiler, linker and C++ standard library;
- runtime files, dynamic-library deployment and target execution requirements.

Package managers, CMake and `pkg-config` may contribute resolved facts, but may not hide target compilation in an opaque nested build. Unsupported generated-build behavior must stop with a structured diagnostic.

## C and C++ are separate capability levels

The initial C path can lower stable external function/data symbols directly and link them from an object, archive or shared library.

C++ adds overload resolution, name mangling, constructors/destructors, class layout, virtual dispatch, templates, inline/header-only APIs, exceptions and compiler/standard-library ABI coupling. When no linkable symbol exists, LAMINARIA must either:

1. generate a visible C++ adapter/instantiation unit and compile it as a foreign action;
2. use a separately maintained explicit wrapper; or
3. reject the construct with a structured reason.

Guessing a mangled name or silently treating a C++ construct as C is not acceptable.

## Identity, cache and evidence

Foreign-native identity includes all semantic inputs that can change behavior or bytes:

- header, source, generated binding and adapter content;
- resolved library bytes/version and transitive native dependencies;
- compiler/linker/archiver identity;
- C/C++ language standard, standard library and ABI mode;
- target, CPU features, sysroot/SDK and compile/link flags;
- macro definitions, include search order and package-resolution results;
- static/shared selection and runtime deployment contract.

A change invalidates only dependent foreign declarations, compile units, link actions and final artifacts. Evidence must show why each action executed, reused or was rejected.

## Initial vertical slices

### C slice

Use a real maintained C library through a Nim declaration and cover:

- an opaque handle;
- scalar and fixed-layout record calls;
- allocation/free ownership;
- a callback with explicit lifetime;
- separate foreign compilation or a prebuilt archive;
- LAMINARIA-owned Nim lowering and target object generation;
- an explicit final link and execution test;
- a negative test proving that `nim c` and generated-C fallback are unavailable.

### C++ slice

Use a real maintained C++ library and cover:

- method, constructor and destructor use;
- at least one template or header-only operation requiring instantiation;
- explicit adapter/instantiation generation where required;
- compiler and standard-library ABI identity;
- structured rejection of an unsupported exception, layout or ABI case;
- final linking with the LAMINARIA-produced object.

## Completion criteria

- LAMINARIA's implementation inventory identifies functionality that should reuse suitable C/C++ libraries and records evidence for reuse or rejection.
- The Nim frontend and IR preserve a declared C foreign dependency through an executed owned target artifact.
- The graph exposes foreign compilation, adapter generation, archive/shared-library inputs and final linking as separate work and artifacts.
- A generated-C/C++ implementation of the Nim target unit cannot masquerade as a foreign dependency.
- The C and C++ vertical slices pass with exact toolchain/artifact lineage and appropriate negative cases.
- Static, dynamic, ownership, callback and ABI obligations have explicit supported or rejected outcomes.
- Windows target production remains reproducible through the project's required `wslc` container workflow.

## Current status

The delegated `NimBuild` baseline may inherit these capabilities opaquely from the real Nim compiler, and the existing Nim wrapper observes native compiler/linker invocations. The owned Nim subset currently rejects pragmas and does not yet model foreign declarations or native dependency/link actions. Therefore this document states required work; it does not describe a completed implementation.
