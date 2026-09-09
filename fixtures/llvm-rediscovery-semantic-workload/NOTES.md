# `llvm-rediscovery-semantic-workload` — issue #25's first experiment

**Status: not complete, not claimed complete.** This is the first
experiment on issue #25's own required program (`docs/
llvm-rediscovery-research.md`), covering exactly one paired workload
and one traced difference. None of issue #25's ten acceptance criteria
are checked off by this fixture alone — several explicitly require "at
least five" of something, or a prototyped candidate substrate, neither
of which exists yet. See "What this does not establish" at the end.

## Why this fixture exists, separately from `rust-nim-llvm-lto-compatibility`

That fixture (#17's) starts from already-emitted `.ll` files and asks
"can these merge." This fixture starts from **source**, on both sides
independently, and asks "what does each compiler's own stage-by-stage
lowering reveal" — per the design doc's explicit method: *"Do not begin
research from pre-existing LLVM IR."*

## The workload

`../CONTRACT.md`: given two 32-bit integers, wrapping addition, no
other observable effect. Deliberately narrower than Rust's default
`+` (whose overflow behavior depends on debug/release and
`-C overflow-checks`) specifically to isolate **compiler/target default
policy** differences from **language-semantics** differences for this
first, minimal experiment — the source of the difference found below
turned out to be exactly a default-policy question, confirming that
isolation was worth doing.

## Stage-by-stage trace

### Rust (`rust-src/add.rs`)

```text
semantic contract (../CONTRACT.md)
  -> compiler/source representation: rustc's own MIR (--emit=mir --
     explicitly labeled by rustc itself as "intended for human
     consumers only... subject to change without notice")

       fn add(_1: i32, _2: i32) -> i32 {
         bb0: { _0 = core::num::<impl i32>::wrapping_add(copy _1, copy _2) -> [return: bb1, ...]; }
         bb1: { return; }
       }

  -> lowering: --emit=llvm-ir
  -> LLVM-facing attributes/metadata:

       define noundef i32 @add(i32 noundef %a, i32 noundef %b) unnamed_addr #0 { ... }
       attributes #0 = { mustprogress nofree norecurse nosync nounwind nonlazybind
         willreturn memory(none) uwtable "probe-stack"="inline-asm" "target-cpu"="x86-64" }

  -> optimization result: opt -O2 with --pass-remarks-output -- empty
     remarks (single function, no call sites to report on; expected,
     not a mechanism failure -- see fixtures/rust-nim-llvm-lto-compatibility/
     NOTES.md's own sanity check of this same flag on a synthetic case
     with a real call site)
  -> artifact: native static library (confirmed non-empty, `ar archive`)
```

Verified locally (arm64 macOS) and in CI (`ubuntu-latest`,
`x86_64-unknown-linux-gnu`) — both produce the same shape of finding,
different only in target-specific attribute values (`"target-cpu"=
"apple-m1"` locally vs. `"target-cpu"="x86-64"` in CI).

### Nim, via `nlvm` (`nim-src/add.nim`)

```text
semantic contract (../CONTRACT.md)
  -> compiler/source representation: UNRESOLVED -- no equivalent to
     rustc's --emit=mir was found in Nim's or nlvm's own CLI. Not
     fabricated; recorded as an open question (see below).
  -> lowering: nlvm c -c --app:staticlib --noMain (LLVM IR emission,
     no C anywhere in the path -- the same C-free route
     fixtures/direct-native-link/NOTES.md already established)
  -> LLVM-facing attributes/metadata (CI, x86_64-unknown-linux-gnu):

       define hidden i32 @add(i32 %a__add_u2, i32 %b__add_u3) #0 { ... }
       attributes #0 = { "no-frame-pointer-elim"="true" }

     The module's full attribute-group list (#0 through #14, every
     group nlvm's own codegen used anywhere in this compilation, not
     just on add()) contains `noinline`, `noreturn`, `cold`,
     `nounwind`, `alloc-family`/`allockind`/`allocsize` (Nim's own GC
     allocator hints), and `no-frame-pointer-elim` -- and nothing else.
     No `probe-stack` attribute appears anywhere in the module.
  -> optimization result: opt -O2 with the same remarks flags -- real,
     non-empty remarks this time (the compiled module includes Nim's
     runtime/stdlib, with real call sites), none of them concerning
     stack probing since nlvm never emits the attribute for the
     remarks mechanism to have an opinion about.
```

## The traced difference: `"probe-stack"="inline-asm"` — present on Rust's side, categorically absent on Nim's

This is the same attribute this project first noticed in
`fixtures/rust-nim-llvm-lto-compatibility/NOTES.md`'s "Correction"
section (where it caused `rust_add` to be flatly non-inlinable,
`(cost=never): conflicting attributes`, into Nim's merged caller) --
that finding stopped at "the attribute differs." This experiment traces
it further upstream, to its actual origin, per issue #25's acceptance
criterion #4.

### Upstream trace, read from rustc's own source, not inferred

```text
observed difference: "probe-stack"="inline-asm" present on Rust's add(),
  absent (not present under any name) on Nim's/nlvm's add()
  -> producing code: rustc_codegen_llvm's own attribute-emission pass
     (.reference/rust/compiler/rustc_codegen_llvm/src/attributes.rs,
     fn probestack_attr) -- attaches the attribute to *every* function
     rustc compiles for this target, unconditionally; nothing in
     add.rs's own source requests it
  -> compiler policy: Target.stack_probes (.reference/rust/compiler/
     rustc_target/src/spec/mod.rs), a per-target default. For
     x86_64-unknown-linux-gnu specifically (.reference/rust/compiler/
     rustc_target/src/spec/targets/x86_64_unknown_linux_gnu.rs:11):
     `base.stack_probes = StackProbeType::Inline;` -- the *default*
     across all targets (mod.rs:2800) is actually `StackProbeType::None`;
     this specific target opts in explicitly
  -> semantic origin: an exploit-mitigation feature, "stack clashing
     protection" -- reading from stack pages as the stack grows so a
     page fault (not silent corruption) results if the stack collides
     with another memory region. Documented in rustc's own docs
     (.reference/rust/src/doc/rustc/src/exploit-mitigations.md):
     "The Rust compiler supports stack clashing protection via stack
     probing, and enables it by default since version 1.20.0
     (2017-08-31)."
```

This is not a per-function semantic fact either `add.rs` or `add.nim`'s
own source expresses. It is a **target-level default security policy**,
applied by rustc's codegen backend uniformly, independent of anything
the compiled function's source says.

### Confirmed absent on the Nim/nlvm side by reading nlvm's own codegen, not by omission alone

`.reference/nlvm/nlvm/llgen.nim` does set several function attributes
of its own (`grep addFuncAttribute`): `noReturn`, `noInline`,
`noOmitFP`, `cold`, `noUnwind`, and GC-allocator hints
(`alloc-family`, `allocsize`, `allockind`). None of them are
`probe-stack` or anything semantically equivalent (a stack-guard-page
mechanism). This is a genuine absence of the *mechanism*, not merely
an absence observed on one function in isolation — confirmed against
the actual attribute-emission code, and against the full 15-group
attribute list this specific compiled module produced (none mention
it), not assumed from `add()`'s own output alone.

## Candidate: what should LAMINARIA preserve

Per issue #25's own instruction, not a completed design, a candidate to
evaluate against further workloads:

1. **A target-level non-functional-guarantee fact, tracked explicitly
   per compilation unit** -- not left implicit in whichever frontend's
   own default happened to run. Stack-clash protection is one instance
   of a class (stack protector strength, CFI, sanitizers are visibly
   the same *kind* of target-attached policy in rustc's own
   `attributes.rs`, sitting in the same function alongside
   `probestack_attr`). LAMINARIA's semantic substrate would need a
   place to record "this compilation unit was produced under mitigation
   policy P," independent of and prior to backend-specific attribute
   syntax.
2. **A cross-language consistency check before merge/link**, not
   silent acceptance of whatever the linker/IR-merger tolerates.
   `llvm-link` merged Rust's and Nim's modules in `rust-nim-llvm-lto-
   compatibility` without complaint about this mismatch -- LLVM's own
   inliner is the only thing that ever objected, and only at the point
   of an actual inlining attempt, not as a whole-program property
   check. A LAMINARIA-native check could flag "this merged artifact
   contains functions with inconsistent stack-clash protection" as an
   explicit, surfaced fact, whether or not any function ever gets
   inlined across the boundary.
3. **Unresolved, deliberately not decided here**: whether Nim/`nlvm`
   *should* gain an equivalent mechanism to match Rust's security
   posture, whether Rust's mitigation should be treated as optional
   metadata LAMINARIA can choose to drop for cross-language uniformity,
   or whether asymmetric protection across a merged binary is an
   acceptable, explicitly-documented outcome. This experiment
   establishes that the asymmetry exists and where it comes from; it
   does not establish which answer is right.

## Unresolved questions and negative results (kept, not silently dropped)

- No Nim/`nlvm`-side equivalent to rustc's `--emit=mir` was found.
  `nlvm`'s own compiler frontend source (`lib/nim`, `llvm/llvm-project`
  in `.reference/nlvm/`) is itself a submodule this project's shallow
  clone did not fetch -- a real gap in what could be checked this
  round, not resolved by assuming no such flag exists.
- The exact LLVM inliner logic that turns `"probe-stack"` mismatch into
  `(cost=never): conflicting attributes` (`llvm/lib/Analysis/
  InlineCost.cpp` or wherever `functionsHaveCompatibleAttributes`
  actually lives) was located in `.reference/llvm-project/` but not
  read this round -- the *fact* of the categorical block was already
  established via `rust-nim-llvm-lto-compatibility`'s own remarks
  output; reading LLVM's own inliner source to confirm probe-stack
  specifically (rather than `nonlazybind` or `target-cpu`) is the
  attribute the check keys on remains open.
- Whether this same attribute-policy-asymmetry pattern recurs for
  other rustc target-level attributes (stack protector, CFI, `no-jump-
  tables`, sanitizers -- all visible as siblings of `probestack_attr`
  in the same `attributes.rs` file) was not checked.

## What this does not establish

Explicitly, against issue #25's own acceptance criteria: this is one
paired workload (criterion 2, partially -- traced, but only one), one
traced difference to its upstream origin (criterion 4, met for this one
case), and zero LLVM concepts re-derived from scratch (criterion 3
needs five), zero candidate LAMINARIA representations prototyped
(criterion 5), zero backend-route projections demonstrated (criterion
6). This experiment is a first data point for the research program, not
a proof of any of it.
