# Toolchain UX Research Contract

## Purpose

LAMINARIA treats the usability of a broad compiler/toolchain space by humans and coding agents as a research result in its own right, alongside compiler-pipeline work, caching, scheduling, backend white-boxing, and performance.

The internal model may accept a broad combination space across Rust/Nim compiler versions, backends, targets, linkers, LTO, WebAssembly composition, runtimes, and artifact boundaries. Exposing that freedom directly and requiring users or agents to discover valid combinations through repeated failing builds is considered a design failure.

This document turns `agent-oriented-toolchain-ux.md` and `validated-toolchain-profiles.md` into a falsifiable research contract with explicit hypotheses, metrics, stopping conditions, and failure semantics.

Central rules:

> Resolve before retrying. Explain before experimenting.

> Broad internal freedom must not become external trial-and-error cost.

## 1. UX as a research subject

LAMINARIA UX is not defined only by short commands or a small number of flags.

The research surface has four layers:

1. **Selection UX** — choosing an appropriate toolchain/profile/route.
2. **Configuration UX** — progressively exposing Simple / Guided / Advanced / Expert control.
3. **Failure UX** — explaining incompatibility, unvalidated paths, fallbacks, opaque regions, and next choices.
4. **Agent Search UX** — preventing coding agents from solving combination explosion through repeated failed builds.

A correct final artifact is therefore not sufficient evidence of good UX if a human or agent had to trial many candidate configurations before reaching it.

## 2. Separate internal freedom from external search

The internal model retains a broad candidate space:

```text
Rust versions
x Nim versions / Nim 2 / Nimony
x backend engine
x target
x linker
x LTO
x post-link optimizer
x Wasm composition
x runtime
x feature/profile
x artifact boundary
...
```

Normal UX does not expose this Cartesian space directly.

```text
Project Requirements
+ Host / Target
+ Requested Intent
+ Package Constraints
+ Toolchain Capability Matrix
+ Validated Profile Evidence
+ Known Compatibility / Incompatibility
+ Negative Knowledge
        -> Constraint Resolution / Pruning
        -> Ranked Viable Plans
        -> Recommended Plan + Explanation
        -> Execution
```

**Execution is not the primary search mechanism.** Broad candidate execution belongs to explicit Research Mode.

## 3. Research hypotheses

### HUX-1 — Validated Profile Reduction

Using qualified profiles and their evidence before execution can materially reduce external compiler/build attempts relative to blind trial-and-error for the same requested artifact.

### HUX-2 — Constraint-first Pruning

Evaluating `rust-version`, edition, compiler capability, target/backend support, artifact compatibility, and known incompatibilities before execution can eliminate most invalid candidates before compiler processes start.

### HUX-3 — Negative Knowledge Reuse

Persisting proven incompatibilities, unsupported paths, and qualification failures as identity-scoped structured evidence can reduce repeated rediscovery by later Runs, sessions, and coding agents.

### HUX-4 — Progressive Disclosure

A progression of validated profile -> intent preset -> advanced override -> expert constraints can keep ordinary configuration simple while preserving access to the full variant space for expert and research use.

### HUX-5 — Structured Explanation / Context Economy

Machine-readable selection, rejection, and ranking summaries can reduce the amount of compiler-log history and trial history that coding agents must retain in context.

### HUX-6 — Bounded Exploration

Explicit exploration budgets and stop conditions can prevent unresolved or unvalidated requests from entering indefinite retries and instead return unresolved constraints and ranked alternatives.

### HUX-7 — Recommendation Without Capability Loss

Narrow validated defaults can coexist with Custom, Expert, and Research modes without removing the ability to investigate novel compiler/backend/target combinations.

## 4. Explicit exploration budgets

Normal coding-agent operation uses policy-controlled exploration budgets instead of “try until something works.”

Budget dimensions should include at least:

```text
max external build attempts
max expensive capability probes
max unvalidated candidate executions
max fallback transitions
max planner wall time
max candidate expansion count
```

The goal is not one universal fixed threshold. Policies may vary by profile qualification, intent, candidate cost, and environment class.

### Stop conditions

Stop additional builds and return an explanation when:

- a fully validated candidate already satisfies the request;
- a candidate is known incompatible;
- an equivalence class of candidates can be rejected for the same reason;
- all remaining candidates are unvalidated and outside normal-mode policy;
- the exploration budget is exhausted;
- artifact/ABI compatibility is unknown under a fail-closed policy;
- no available toolchain exposes the required capability.

The result must contain more than `failed`, including:

```text
unresolved constraints
best validated alternative
ranked next candidates
validation gaps
required user/agent decision
research-mode option
```

## 5. Negative Knowledge Contract

Negative knowledge is reusable decision evidence, not merely retained logs.

Record at least:

```text
reason_code
reason_class
input/toolchain/profile/host/target identity scope
evidence reference
first observed / last confirmed
confidence / evidence class
transient-or-semantic classification
invalidated-by conditions
```

Example reason classes:

```text
semantic_incompatible
capability_missing
target_unsupported
backend_linker_incompatible
runtime_contract_unresolved
artifact_compatibility_unknown
qualification_failed
transient_environment_failure
tool_failure_unknown
```

### Avoid both fail-open and over-pruning

- semantic incompatibility and unknown ABI/artifact compatibility remain fail-closed;
- transient host/process/network failures must not become permanent incompatibility rules;
- evidence whose assumptions changed after toolchain/profile updates must be re-evaluable.

## 6. Profiles are evidence contracts, not search order hints

`recommended` does not mean “the first candidate to try.”

A profile revision should carry at least:

```text
exact ToolchainFingerprint bundle
qualification scope
host/target coverage
known limitations
known failures
performance/resource evidence
supported backend/target routes
promotion/demotion history
```

If a fully validated profile satisfies the request, normal mode should select it directly instead of executing more uncertain alternatives first.

Overrides must trigger qualification re-evaluation rather than inheriting the base profile's validation state unchanged.

## 7. Human and agent UX share one resolver

Do not maintain different compatibility logic for human and coding-agent frontends.

The same resolver/planner produces decision evidence; presentation differs.

### Human-facing

```text
Recommended
Latest Validated
Long-Term
Preview
Custom
```

Exact versions, reasons, and limitations expand on demand.

### Agent-facing

```text
laminaria toolchain resolve --json
laminaria plan --json
laminaria explain-toolchain-selection --json
laminaria explain-candidate-rejection --json
```

The same decision graph supplies structured evidence.

## 8. UX metrics

UX is continuously measurable.

### Exploration cost

- external build/process attempts before first viable plan;
- failed toolchain attempts;
- expensive probe count;
- unvalidated execution count;
- fallback transitions;
- time-to-first-viable-plan;
- request-to-successful-artifact wall time.

### Search reduction

- theoretical candidate count;
- explored candidate count;
- statically pruned count;
- negative-evidence-pruned count;
- candidates selected/eliminated via qualification;
- merged equivalent states;
- candidates reaching real compiler execution;
- pruning ratio.

### Agent context economy

- structured output bytes/tokens returned to the agent;
- raw compiler-log volume referenced;
- log volume produced per rejected candidate;
- repeated rediscovery of the same incompatibility across sessions.

### Recommendation quality

- success rate from `recommended`;
- success rate from `latest-validated`;
- unexpected failure rate inside validated scope;
- profile rollback/demotion count;
- fallback rate from recommended paths;
- retries after validation-gap explanation.

## 9. Baseline comparison

Compare the same fixtures against at least:

### Baseline A — Manual / Blind Agent

An agent without combination knowledge reads build errors and repeatedly edits configuration and rebuilds.

### Baseline B — Static Constraint Only

Package metadata and toolchain capabilities prune candidates, but no profile qualification or reusable negative knowledge is used.

### LAMINARIA

Validated Profiles + constraints + capability matrix + negative knowledge + bounded exploration.

Compare external attempts, wall time, CPU/I/O, context/log volume, and explored candidates.

## 10. Failure semantics

The following are UX research failures:

- the build eventually succeeds only after many external candidate builds;
- the internal planner is lazy but delegates unresolved variants to an agent for external trial-and-error;
- `recommended` is only a first candidate and does not suppress unnecessary exploration after failure;
- a known incompatibility is rebuilt under the same relevant identities;
- a validation gap exists but the validated badge is retained;
- automatic retries continue after the exploration budget is exhausted;
- the human UI is simple but the agent API lacks structured selection/rejection reasons;
- the agent API exists but internally relies on parsing human-readable compiler failures as its primary resolver;
- simplifying normal configuration removes Custom/Research capabilities.

## 11. Acceptance workloads

Maintain versioned fixtures that demonstrate at least:

1. a mixed Rust/Nim native build resolves using only `recommended`;
2. when latest upstream is unqualified, the resolver selects `latest-validated` without blind execution;
3. an incompatible `rust-version` candidate is rejected before process start;
4. backend/target/linker incompatibility is rejected before process start;
5. a synthetic space of 50+ candidates is reduced to a small viable set;
6. a previously proven incompatibility is not re-executed in a later Run/session;
7. transient failures remain retryable while semantic incompatibilities do not;
8. an override explains the loss/change of profile qualification;
9. if no fully validated candidate exists, normal mode stops within budget and returns validation gaps plus ranked alternatives;
10. Research Mode can explicitly widen exploration over the same candidate space.

## 12. Related research tracks

This work does not create a separate solver stack.

- #8 Variant Explosion Control — candidate expansion, pruning, and merging.
- #10 Explainability — selection/rejection/ranking reason schema.
- #11 / #18–#21 Measurement Spine — qualification evidence and UX metric collection.
- #22 Multi-version Toolchains — broad internal candidate and compatibility model.
- #23 Validated Toolchain Profiles — user-facing known-good bundle policy.
- #24 Agent-Oriented Planning UX — bounded exploration and coding-agent acceptance.

All of these use the same Nim Planning Kernel and Rust Runtime architecture.

## 13. Completion criteria

This research is established only when reproducible evidence shows that:

1. ordinary users can reach artifacts without manually searching exact toolchain combinations;
2. coding agents do not solve combination explosion through repeated external build failures;
3. most of a large candidate space can be pruned/merged before execution;
4. known negative knowledge is reused across Runs/sessions;
5. normal exploration is bounded and stops with explanations/alternatives when unresolved;
6. selection/rejection/ranking is machine-readable and evidence-backed;
7. simple profile/intent UX and detailed expert/research control coexist on the same resolver;
8. compared with blind trial-and-error, LAMINARIA shows measurable improvement in at least some of external attempts, wall time, resource consumption, or agent context volume;
9. UX regressions are treated as quality-gate failures that require design changes, just like material performance regressions.
