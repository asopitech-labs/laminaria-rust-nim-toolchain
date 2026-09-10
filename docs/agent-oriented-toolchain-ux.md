# Agent-Oriented Toolchain UX Research Direction

## Ownership correction (2026-09-10)

The [compiler ownership contract](compiler-ownership-contract.md) governs research objectives and acceptance.

Normal target profiles select a LAMINARIA compiler revision, supported Rust/Nim language contracts, IR/transform revisions, target/runtime requirements and scheduler/resource policy. The existing-compiler version matrices and external build attempts below belong to explicitly separate reference/bootstrap profiles only. Success there must never promote a target-compilation profile. Missing LAMINARIA support stops with a diagnostic; it does not select rustc/Nim as fallback.

## Main-path compiler profiles and acceptance

The main candidate space is `LAMINARIA revision × Rust/Nim language contracts × IR/transform revision × target/runtime × resource policy`. Cargo/Nimble resolver identity is a separate dependency input. Main-path selection does not branch into existing compilers.

Qualify Rust-only, Nim-only and mixed source through the same owned compiler. Reject unsupported syntax, targets and dependencies with structured diagnostics before external compilation. Test that reference/bootstrap qualification cannot promote a target profile and that profile/IR revision changes invalidate correctly. Full qualification matrices must not delay the small #25/#3/#6/#8 implementation.

## Scope of the existing-tool matrix below: reference/bootstrap profiles

The rustc/Nim/LLVM version, release and compatibility matrices, configuration examples, external build attempts and their success criteria below apply **only to reference/bootstrap profiles**. Reuse pruning, explanation and exploration-budget principles for owned-compiler profiles without promoting existing compilers to their execution engines. Examples are not a list of implemented or qualified profiles.

## Purpose

LAMINARIA treats toolchain-selection and configuration UX as a research and product objective, alongside compiler performance, caching, scheduling, and backend white-boxing.

The internal combination space across Rust/Nim compiler versions, backends, targets, linkers, LTO modes, WebAssembly composition routes, and runtimes may be broad. LAMINARIA must not expose that space directly and require humans or coding agents to discover a viable combination by repeatedly executing failing builds.

Central rule:

> Resolve before retrying. Explain before experimenting.

LAMINARIA should use known constraints, validated profiles, compatibility evidence, and reusable negative knowledge to shrink the search space before execution and return a small number of viable plans with structured explanations.

The falsifiable hypotheses, exploration budgets, UX metrics, failure semantics, and baselines for this direction are defined in `toolchain-ux-research-contract.md`.

## 1. UX is an independent success criterion

A technically correct toolchain is still inadequate if users must manually discover version/backend/linker combinations through trial and error.

LAMINARIA should ensure that:

- ordinary users do not manually solve exact compiler/backend/linker versions;
- coding agents do not need many failed builds to discover a valid plan;
- invalid combinations are rejected or pruned before execution whenever possible;
- recommended paths have structured evidence;
- advanced users can progressively descend into lower-level dimensions;
- custom mode still rejects known incompatibilities early and explains why.

## 2. Agent trial-and-error explosion is a first-class problem

A coding agent may otherwise perform a search like:

```text
Rust A + Nim X + LLVM P -> fail
Rust A + Nim X + LLVM Q -> fail
Rust A + Nim Y + LLVM P -> fail
Rust B + Nim X + LLVM P -> fail
...
```

This consumes build time, process resources, log/context budget, and repeated reasoning effort.

LAMINARIA should instead plan from:

```text
Project Requirements
  + Host / Target
  + Requested Intent
  + Package Constraints
  + Toolchain Capability Matrix
  + Known Compatibility / Incompatibility
  + Validated Profile Evidence
        ↓
Constraint Resolution / Pruning
        ↓
Ranked Viable Plans
        ↓
Recommended Plan + Explanation
        ↓
Execution
```

Execution is primarily for running or validating an already selected plan, not the default search mechanism.

## 3. Three user surfaces

### Simple

Profiles such as `recommended`, `latest-validated`, `long-term`, and `preview` are sufficient for ordinary humans and coding agents.

### Guided

Users specify intent without exact toolchain versions, for example:

```text
profile = recommended
target = wasm-component
priority = fast-iteration
```

LAMINARIA resolves the remaining dimensions from validated candidates.

### Expert

Exact compiler versions, backend routes, LTO, linkers, and artifact boundaries may be constrained directly.

Expert mode does not disable known incompatibility pruning or turn the system into blind trial-and-error execution.

## 4. Profiles are evidence-backed constraints, not merely search priors

A validated profile provides more than a candidate to try first. It provides an exact resolved bundle, qualification scope, known limitations, compatible routes, measured resource/performance evidence, known failures, and promotion/demotion history.

Coding agents should consume this evidence before inventing new combinations.

## 5. Preserve negative knowledge

Store and reuse structured rejection evidence, for example:

```text
candidate rejected because:
  rustc capability missing
  target unsupported
  linker/object model incompatible
  Nim runtime obligation unresolved
  profile qualification failed on this host/target
  artifact compatibility unknown
```

Equivalent future candidates should be pruned without repeating the same expensive experiment.

Temporary or environment-specific failures must remain distinguishable from semantic incompatibility so transient failures are not promoted into permanent rejection rules.

## 6. Static resolution -> cheap probe -> execution

Exploration is staged by cost.

### Stage 1 — Static/recorded resolution

Resolve package/version constraints, compiler capabilities, known target/backend support, profile qualification, artifact compatibility, required components, and known incompatible edges without starting expensive builds.

### Stage 2 — Cheap capability probes

Only when needed, query versions, target lists, linker support, or equivalent low-cost capabilities.

### Stage 3 — Qualification lookup

Reuse Measurement Spine evidence when the combination has already been exercised.

### Stage 4 — Execution

Run only the sufficiently narrowed plan.

Research mode may intentionally execute many candidates, but that mode is distinct from normal user/agent UX.

## 7. Return ranked plans

The resolver may return a small ordered set rather than only success/failure:

```text
1. recommended / fully validated
2. latest-validated / fully validated, newer Rust
3. preview / validated-with-limitations, Nimony
```

Ranking inputs may include qualification level, constraint satisfaction, failure risk, host/target coverage, performance/resource evidence, freshness, migration cost, and requested intent. Ranking must be explainable.

## 8. Agent-facing structured interfaces

Candidate interfaces include:

```text
laminaria toolchain resolve --json
laminaria toolchain profiles --json
laminaria explain-toolchain-selection --json
laminaria explain-profile-qualification --json
laminaria explain-candidate-rejection --json
laminaria plan --json
```

Machine-readable output should include requested constraints, selected profile/revision, exact toolchains, backend/target route, qualification scope, explored/pruned/rejected counts, reason codes, known limitations, fallbacks, evidence class, and whether any additional probe/execution is required.

## 9. Bounded exploration policy

Normal agent-oriented operation has an exploration budget.

- if a fully validated candidate exists, do not automatically execute unvalidated alternatives;
- do not execute known-incompatible candidates;
- merge/prune equivalent candidates that share the same rejection reason;
- use explicit fallback policy;
- when the budget is exhausted, return unresolved constraints and ranked next options rather than continuing retries indefinitely.

Research mode can explicitly enable broad exploration.

## 10. Agent context economy

Logs and context are resources. LAMINARIA should provide compact structured summaries rather than forcing an agent to retain dozens of compiler logs.

Example:

```text
selection_failed:
  reason = target_backend_incompatible
  rejected = 47 equivalent variants
  nearest_validated = profile:recommended@rev
  required_change = target -> wasm32-core
```

Raw logs remain available on demand.

## 11. UX metrics

Candidate metrics include:

- build/process attempts before the first viable plan;
- failed toolchain attempt count;
- candidates pruned statically and pruning ratio;
- retries avoided by known negative evidence;
- time from profile request to successful artifact;
- resolver/planner latency;
- explored/pruned/merged candidate counts;
- structured/log output volume returned to an agent;
- fallback count;
- success rate for validated profiles;
- failure-classification coverage for custom/preview paths;
- repeated rediscovery of the same issue across sessions.

The objective is not only a fast solver; it is reducing unnecessary external compilation attempts.

## 12. Coding-agent acceptance scenarios

At minimum test:

1. a mixed Rust/Nim native build succeeds from `recommended` without manual version solving;
2. when latest upstream is unvalidated, an agent selects `latest-validated` instead of blind retry;
3. an unsatisfied `rust-version` constraint rejects a candidate before build;
4. unsupported backend/target combinations are pruned before build;
5. an override explains the loss/change of profile qualification;
6. a synthetic space of 50+ candidates is pruned to a small executable set;
7. a known incompatible combination is not retried in a later run/session;
8. when no fully validated candidate exists, the agent receives validation gaps and ranked alternatives instead of infinite retries;
9. broad exploration remains available explicitly in research mode.

## 13. Success criteria

LAMINARIA's UX research is not merely about short CLI syntax.

It succeeds when:

1. ordinary users avoid manual toolchain-combination search;
2. coding agents do not solve combination explosion through repeated external build failures;
3. validated profiles, constraints, and negative knowledge prune most invalid candidates before execution;
4. selection and rejection reasons are machine-readable;
5. expert users can still constrain every required dimension;
6. narrowing normal UX does not prohibit novel research combinations;
7. failed attempts, time-to-plan, pruned variants, and agent log/context volume are measured continuously.
