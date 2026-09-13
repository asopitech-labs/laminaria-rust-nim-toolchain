# Fixture Policy

## 1. Purpose and authority

This policy defines what a LAMINARIA fixture may freeze, what it must not freeze,
and what a fixture-backed test can establish. Direct executable tests against the
production implementation are authoritative for implemented behavior. A fixture
is an input, initial state, counterexample, or workload supplied to such a test;
it is not an independent implementation of the specification.

Apply this policy together with the [research program](research-program.md),
[compiler ownership contract](compiler-ownership-contract.md), and `AGENTS.md`.

## 2. Keep responsibilities separate

| Kind | Responsibility | Authority |
| --- | --- | --- |
| production implementation | Actual parser, resolver, compiler, planner, and executor behavior | Production code and direct execution |
| configuration / lock | Declared selections, versions, SHAs, features, and policy inputs | The configuration or lock file |
| documentation | Intent, contracts, procedures, and constraints | The document; not proof of behavior |
| fixture | Fixed input, initial state, counterexample, or workload for a production path | Only the input represented by the fixture |
| oracle | An independently justified property used to judge observations | A direct-test assertion or external standard |
| evidence | Raw observations, artifacts, and measurements from execution | A record bound to subject, run, and environment identity |
| reference project | A pinned external source revision used for implementation study | `reference-projects.lock.json` |

A configuration or document is not a fixture. Repeating configuration values in
a test creates a second source of truth. Mechanically copying prose into a
machine-readable expectation does not establish production correctness.

## 3. What a fixture may freeze

A fixture is allowed only when it has an independent purpose and a production
consumer directly consumes it.

### 3.1 Minimal inputs

- Source, manifests, or bytes supplied to a parser, resolver, frontend, compiler,
  linker, or CLI.
- A minimal project layout, symbol set, ABI shape, or filesystem layout that
  triggers a boundary condition.
- A reduced real user input or external-protocol input.

Freeze the input, not a hand-maintained copy of the production implementation's
internal graph or intermediate result.

### 3.2 Initial state and scenario controls

- State needed to reproduce cold, warm, no-op, edit, missing-dependency, or
  resource-limit scenarios.
- Seeds, reduced counterexamples, and operation sequences from property or fuzz failures.
- Semantically relevant clock, environment, target, and toolchain controls.

State must be reproducible from documented actions. Do not commit a cache or
generated tree merely because regenerating it is inconvenient.

### 3.3 Externally derived conformance data

- Standards-defined test vectors, wire bytes, diagnostic categories, or ABI constants.
- Published upstream compatibility corpora or inputs from real failures.

Record source, version, license, and acquisition or generation method. Output
from the same production implementation is not an independent expected result.

### 3.4 Research workloads

- Projects with deliberate wide-graph, critical-path, boundary-heavy, or
  incremental-edit structure.
- Source and workloads that provide the same semantic demand to compared methods.

Do not freeze performance results in the workload. Measurements are evidence
bound to a run and environment, not fixture data.

## 4. What a fixture must not freeze

Do not duplicate any of the following in a fixture or test as a second authority:

1. Complete copies of project names, counts, SHAs, features, or attributes already
   declared by a configuration or lock.
2. Production-generated graphs, plans, IR, or JSON recorded as golden results
   without an independent oracle.
3. The production algorithm or decision table rewritten in a separate validator.
4. A documentation checklist copied into a machine-readable catalog that is the
   only object tested.
5. Inputs consumed only by mocks and never by the real production boundary.
6. Large snapshots containing incidental paths, timestamps, unordered IDs, or
   compiler wording.
7. Generated caches or binaries committed only for convenience.
8. A fixture-only validator plus unit tests that establish only that validator's
   internal consistency.

Updating a fixture and implementation together until tests pass is not independent
assurance. A useful regression fixture must fail against the prior behavior or make
a direct production test fail under a known seeded fault.

## 5. Oracles and expected values

An expected value may be frozen only when its correctness can be justified
independently of the production implementation. Acceptable sources include:

- a standards test vector;
- a small manually derivable result with the derivation explained;
- a differential relation against an independent reference implementation;
- a property such as equivalence, zero unrequested actions, or unique symbol resolution;
- a real failure input paired with required external observable behavior.

Do not capture one current run as `expected.json`, blindly overwrite snapshots,
or let one agent update a fixture, validator, and validator test as proof. Prefer
semantic properties over whole-output snapshots. Byte-for-byte golden data is
reserved for contracts where byte identity is itself required; exclude unstable
fields and record provenance.

## 6. Manifests, schemas, and validators

A fixture manifest is permitted only when a production runtime has an independent
consumer or when scenario input must be declared. If a production parser reads
the schema, tests directly execute that parser or CLI.

- Schema validation belongs to the production parser.
- Parser tests construct minimal valid and invalid values and test generic behavior.
- Tests do not repeat every value from a checked-in configuration.
- A fixture-only validator is not an authority.
- Lints may check formatting or unsafe paths, but do not prove production correctness.

## 7. Fixture categories

Every new fixture identifies exactly one primary category.

| Category | What is frozen | Primary verdict |
| --- | --- | --- |
| `conformance-input` | Input from an external specification | Production output satisfies the specification property |
| `regression-counterexample` | Minimal prior failure input or operation | The production path does not regress |
| `scenario-state` | Cold/warm/edit/missing-dependency state | Properties of actions, results, and side effects |
| `fuzz-corpus` | Seed, corpus, or reduced case | Reproduction of crash, UB, or oracle mismatch |
| `benchmark-workload` | Work with fixed semantics and graph shape | Measurements with correctness preserved; thresholds live elsewhere |
| `artifact-subject` | A binary or object that is itself under inspection | Direct inspection with digest, producer, target, and provenance |

Reference-source pins and toolchain locks are not fixture categories. Their lock
files remain authoritative.

## 8. Add, update, and remove fixtures

A fixture-changing commit explains:

1. Its category and the production claim it can falsify.
2. The production entry point and test command that consume it.
3. The independent basis of the oracle.
4. Provenance such as source, generator, seed, toolchain, and license.
5. How it was minimized and which derived or unstable fields were not frozen.
6. Why the prior implementation or a seeded fault makes the direct test fail.

An expected-value change is a reviewed behavior-contract decision, not incidental
fixture maintenance. Fixtures with no consumer, duplicate claims, or no production
path are deletion candidates.

## 9. Review checklist

- Is this input, or a copy of an implementation-produced answer?
- Is there an independent runtime consumer?
- Does it exercise the production entry point directly?
- Is the oracle independently justified?
- Does it duplicate configuration, documentation, or another fixture?
- Is it a minimal counterexample or workload?
- Are test and production artifact identities distinct?
- Is snapshot updating subject to review?
- If the same claim can be tested directly without the fixture, is it needed?

If any answer is unclear, improve the direct test or production interface before
adding the fixture.

## 10. Application to `reference-projects.lock.json`

`reference-projects.lock.json` is a declaration of reference-source identity, not
a fixture. Project names, count, URLs, SHAs, and filesystem attributes are
authoritative only in that lock.

Tests for `scripts/reference_projects.py` use temporary local Git repositories and
temporary locks to exercise the production utility's generic safe-consumption
behavior. They must not repeat the checked-in catalog of 19 projects. Documentation
may explain why entries exist, but it is not a second executable specification.

## 11. Existing fixtures

Introducing this policy does not certify every existing item under `fixtures/`.
Audit each fixture for category, consumer, oracle, and provenance when it is changed
or used as milestone evidence. Existing nonconformance does not justify new duplication.
