# Issue #50 R1 progress: generic demand feedback slice

## Result

The R0-locked `rust-heavy-workspace` now has a source-fact-to-planner path for
one generic function instance. For a `fixture-bin` executable request, the
planner retains `sum_generic<i64>` and omits the `#[cfg(test)]`-only
`sum_generic<i32>` from its demand-relative generic work set. For a
`fixture-core` test request, that selection reverses: `i32` remains and `i64`
is omitted. An artifact feedback plan now composes that specialization demand
with the package selection from the same artifact request and rejects generic
providers that are not candidates or are not selected. The selection now
drives source-derived lowering of the selected generic instances into a narrow
owned semantic IR and target-specific native objects. This does not yet produce
the fixture's native executable or prove that later compiler stages were skipped.

## Checkpoint contract

- **Consumes:** The checked-in Rust source for `fixture-core` and `fixture-bin`
  in `fixtures/rust-heavy-workspace`, the generic-function declarations found
  in the provider source, and an explicitly selected production or test demand
  root.
- **Must preserve:** The existing package-level `rust_cross_layer` planner and
  its `many-unrequested-targets` integration test; R0's Cargo behavior oracle;
  and the rule that discovery uncertainty must not silently remove required
  work.
- **Evidence:** `laminaria-ir::rust_generic_demand` discovers the fixture's
  `sum_generic<i64>` call and test-only `sum_generic<i32>` call. Package
  candidates and direct dependencies come from the existing
  `cargo_metadata 0.18.1` crate; the source collector resolves the fixture's
  imported type/function roots and avoids interpreting `Cluster::...` as a
  package. The artifact feedback planner requires requested instances to be
  in the eager inventory, verifies canonical Cargo package identities across
  package/generic layers, and reports eager, feedback, and pruned
  monomorphize/codegen work identities. The integration test checks both the
  executable and core-test request directions. Unsupported generic inference
  and unknown macros are structured discovery errors.
- **Enables:** The selected specialization work set can drive owned semantic
  lowering and native object generation. Final linking and direct execution of
  the fixed executable remain necessary for R1 completion.

## Current boundary and remaining R1 result

The source collector is a narrow `syn`-based recognizer, not Rust type
checking. It currently handles the locked fixture's direct generic call,
annotated `Vec<T>` argument, unsuffixed integer array literal, and its known
expression macros. It accepts functions with exactly one type parameter and
fails closed for other generic signatures, unrecognized macros, and unsupported
argument inference. Its `include_test_cfg` behavior is validated against the
fixture's inline `#[cfg(test)]` module; it is not a general implementation of
Rust cfg evaluation.

The planner creates demand-relative semantic-analysis, IR-lowering,
monomorphization, codegen, and symbol/liveness identities for each selected
generic instance. The selected IR-lowering identities now invoke an owned
source-derived semantic recognizer for `&[T]::iter().fold` with scalar addition.
The selected Codegen identities now produce owned AArch64 Mach-O or Linux
x86_64 ELF objects from those IR instances. The other identities remain
planning identities, not evidence that those stages ran. This narrow recognizer
is not a general Rust type checker, is not yet lowered to the main Program
representation, and is not connected to final linking of `fixture-bin` or
process/action provenance.

The artifact feedback plan exposes a combined eager/feedback/pruned execution
identity set containing both package-stage and generic-instance work. The
executor bridge now maps its typed `RustExecutionWork` values to the concrete
action IDs supplied by the action builder; it never guesses from an ID prefix
or from ordering. The selected IDs are closure-checked before dispatch, and
the executor records only successfully completed IDs. The focused regression
therefore proves a real six-action eager run, a three-action feedback run, and
structured rejection when a producer is omitted.

This still does not establish the full requested-artifact → unit →
semantic/IR → symbol/liveness → unit feedback loop, a positive compiler
artifact from the owned path, or raw resource/performance attribution. Cargo
remains the R0 reference oracle. R2 remains responsible for matched raw
resource evidence and causal attribution.

R1's current result is the first executable package feedback slice: typed
artifact demand selects the action set, dependency closure is enforced, and
dispatch evidence is observable. The remaining R1 work is to connect the same
selection contract to the real artifact-producing path and its independent
consumer evidence.

The executor now publishes a typed `CompilerWorkExecutionReceipt` from the
actual `ArtifactStore` evidence, including selected actions, successful
dispatch identities, and produced evidence artifact identities. A separate
integration-test consumer validates the receipt against the immutable plan and
requested evidence artifact, and rejects unknown or tampered action identities.
This consumer accepts both eager and feedback-produced positive evidence; it
does not rely on the mutable `laminaria-bootstrap` tag or on producer-side
assertions alone.

## Checkpoint failure report — 2026-10-08

The R0-locked positive subject is the actual `fixture-bin` executable, with
the four output lines recorded in `issue-50-r0-oracle-lock.md`. The existing
feedback executor's positive evidence is an interpreted owned-IR result, not
that executable. Its direct test
`the_r0_generic_fixture_is_rejected_before_owned_program_publication` confirms
that the current owned frontend rejects the real `sum_generic` declaration
before publishing even a candidate program. The locked binary also uses
`Vec`, iterators, structs, `Option`, and formatting outside the current owned
IR. `laminaria-ir` has an owned WASM generator but no owned native object
generator; G2's fixed native fixture currently calls external `rustc` and Nim
and cannot serve as the owned Rust producer for this R1 subject.

Thus the current action-selection and receipt checkpoints remain valid, but
they do not enable R1's positive native-artifact checkpoint. Completing that
checkpoint while preserving the locked R0 subject entails a substantially
larger source-semantics, runtime, and native-generation path. Narrowing the
subject would change R0's acceptance contract. The existing instruction keeps
the locked subject, so implementation continues along the owned-compiler
path. Neither a delegated Cargo binary nor an interpreted evidence artifact is
an equivalent completion.

## Continue against the locked R0 oracle

The [issue #50 close comment](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/50#issuecomment-5843413138)
calls the `ArtifactStore` evidence and six-versus-three action dispatch an R1
close. Those observations establish the executor-selection checkpoint above,
but the stored evidence is not the R0-locked `fixture-bin` native executable.
The [issue's R1 contract](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/50)
requires a positive native artifact and direct executable behavior. The close
comment is therefore insufficient evidence for that contract. Continue from
the validated action-selection and receipt checkpoints while preserving R0's
subject and oracle. The next result must be an owned semantic/IR representation
of the three fixture crates, including the generic `i64` instance and their
reachable `Vec`, iterator, `Option`, struct, arithmetic, and formatting
behavior. That result must enable owned native target production and a final
link action, followed by direct execution of `fixture-bin` and the same
feedback comparison. The `i32` test instance remains the negative demand
relation. This compiler and runtime path overlaps G2/#46 and #5's backend
route decision; shared work should advance both rather than creating a
fixture-only compiler.

The last established R1 result is typed feedback selection with independently
checked owned-IR execution evidence. No direct native executable result or R2
resource claim follows from it yet.

## Owned native scalar checkpoint — 2026-10-08

- **Result:** The validated owned scalar IR can produce a relocatable AArch64
  Mach-O object. A platform linker connects that object with an independent C
  caller, and the resulting native process returns the four previously locked
  `add_or_double` values. Rust target code is generated by LAMINARIA, not by
  the C compiler used for the caller.
- **Consumes:** `laminaria-ir`'s existing validated `i32` Program and the
  separately verified `add_or_double` source/oracle.
- **Must preserve:** The source-derived Rust/Nim frontend and interpreter
  semantics, the WASM target, and the no-external-Rust-compiler ownership
  boundary.
- **Evidence:** The native object is linked and executed on macOS arm64;
  outputs are `7`, `6`, `-2147483648`, and `-10`. The `laminaria-ir` tests and
  workspace verification exercise the new producer.
- **Enables:** A native target action can consume validated owned IR. R1 still
  needs the fixed fixture's wider Rust semantics, runtime operations, and
  target generation on the verification host before `fixture-bin` can be claimed.

The owned compiler-work graph now includes that native target action. The real
Nim planner orders source lowering, IR validation, and AArch64 object generation
by declared artifact dependencies; the Rust executor reads only the validated
Program and publishes object bytes under the target-specific work identity.
The focused integration test observes all three successful dispatch identities
and the produced Mach-O object. This connects the scalar producer to the
planner/executor boundary, but no native final-link action or R0 fixture
semantics are claimed yet.

## Source-derived generic semantic checkpoint — 2026-10-08

- **Result:** The fixed fixture's `sum_generic<i64>` provider function lowers
  from its actual Rust source into a narrow owned semantic IR, and that IR
  evaluates the selected scalar fold. The `fixture-bin` artifact feedback plan
  executes this lowering and AArch64 Mach-O object generation for `i64`; an
  eager comparison additionally processes the test-only `i32` instance.
- **Consumes:** The checked-in `fixture-core` provider source, the discovered
  concrete generic demand, the artifact feedback plan, and an explicit target
  overflow policy.
- **Must preserve:** The fixed `fixture-bin` R0 acceptance subject, source
  provenance, fail-closed rejection of unsupported Rust syntax or changed fold
  semantics, and the distinction between selected IR work and later native
  artifact production.
- **Evidence:** Focused integration tests obtain the plan from Cargo workspace
  metadata and fixture source, then execute the same source-derived lowering
  for eager and feedback selections. Eager produces two IR instances;
  feedback produces only `i64` IR and its native object. The resulting `i64`
  IR evaluates `[12, 30]` to `42`; the eager-only `i32` IR evaluates
  `[1, 2, 3, 4]` to `10`. Both native objects link with an independent C caller
  on macOS arm64 and return those values. Altering the fold body to subtraction
  is rejected.
- **Enables:** The selected generic IR can be integrated with reachable
  fixture semantics and the remaining native target producer. No `fixture-bin`
  executable, final-link result, or complete R1 feedback loop follows from this
  checkpoint. Checked overflow currently traps in the native subset because
  the owned runtime has no Rust panic/unwind support.

## Linux x86_64 portability checkpoint — 2026-10-08

- **Result:** The same source-derived `sum_generic<i64>` IR now produces an
  owned Linux x86_64 ELF relocatable object. The `fixture-bin` feedback plan
  selects only that concrete instance when this optional target is requested.
- **Consumes:** The existing generic fold IR, its explicit checked overflow
  policy, and the target-independent generic Codegen work identity.
- **Must preserve:** The selected-source semantics, stable symbol identity
  across targets, fail-closed IR/codegen correspondence, and the distinction
  between an object file and the complete `fixture-bin` executable.
- **Evidence:** On macOS, the ELF parser recognizes the output as x86_64 and
  finds its defined function symbol; the function's instruction bytes agree
  with an independently assembled x86_64 loop. A Linux x86_64 target-only test
  links the owned object with a C caller and checks a two-element and an empty
  slice; this host cannot execute that test locally.
- **Enables:** Later owned code generation for the fixture's other reachable
  functions can share this optional target. Linux execution is not a gate for
  the current macOS verification result.

## macOS arm64 verification checkpoint — 2026-10-08

R0 records the environment of its original Linux reference run; its acceptance
contract requests a **host-native** executable. The current verification host
is macOS arm64. GitHub Actions is intentionally `workflow_dispatch` only during
this research phase, so an automatic Linux CI run cannot serve as the local
verification gate.

- **Result:** The fixed fixture's actual `Cluster::from_prime_grid(200)`
  supplies the `xs` slice to the source-derived, feedback-selected owned
  `sum_generic<i64>` Mach-O object. A directly linked macOS process computes
  `4028`, matching the fixture's `sum_x` oracle.
- **Consumes:** The unchanged three-crate fixture source, its Cargo reference
  executable as a test oracle, the artifact feedback plan, and the selected
  owned generic object. The independent C caller passes reference data to the
  owned function; it does not implement the Rust target function.
- **Must preserve:** The four frozen R0 output lines, the test-only `i32`
  pruning relation, and the distinction between an owned generic function
  object and a complete owned `fixture-bin` executable.
- **Evidence:** The Mac integration test runs the original `fixture-bin` via
  Cargo and checks all four lines, includes the original core/mid source as
  reference-only test modules, links the owned generic object with a C caller,
  and checks its native `4028` result on the real `xs`. The original fixture's
  five unit tests also pass when run in its own Cargo workspace.
- **Enables:** The Mac verification loop is complete for this selected generic
  function. The wider fixture semantics and final owned `fixture-bin` link are
  still required before the full R1 executable claim can be made.
