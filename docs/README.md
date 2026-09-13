# LAMINARIA documentation map

Start with [Project Progression and Near-Term Research Goal](near-term-research-program.md) ([日本語](near-term-research-program_ja.md)). It is the canonical entry point for the current goal: resolve the Cargo/Nimble/C/C++ dependency closure and produce a runnable native binary efficiently. WebAssembly is an optional target, not the current milestone.

The numbered directories are downstream layers, not parallel roadmaps:

```text
near-term-research-program.md     current goal and progression (start here)
├── 01-foundations/              durable purpose, ownership, policy, architecture
├── 02-research-areas/           subject-specific hypotheses and supporting tracks
├── 03-work-items/               issue map, experiment specs, evidence, reviews
├── 04-guides/                   procedures for the implemented/bootstrap baseline
└── 05-history/                  dated audits and superseded context
```

## Reading order

1. Read the [near-term program](near-term-research-program.md) to identify the active goal and stopping condition.
2. Use [compiler ownership](01-foundations/compiler-ownership-contract.md) and [research prioritization](01-foundations/research-prioritization-policy.md) to constrain what counts as evidence and how the next experiment is selected.
3. Open only the relevant research-area document for the active decision.
4. Use the [project work portfolio](03-work-items/project-portfolio.md) to see outcomes, capabilities, unissued gaps, and horizons; then use the [issue plan](03-work-items/issue-plan.md) only for the GitHub-tracked projection.
5. Consult guides for commands and history for provenance; neither overrides the current program.

Japanese entry points are [当面の研究ゴール](near-term-research-program_ja.md), [独自コンパイラ責務契約](01-foundations/compiler-ownership-contract_ja.md), and [研究優先順位ポリシー](01-foundations/research-prioritization-policy_ja.md).

## 01 — Foundations

- [Compiler ownership contract](01-foundations/compiler-ownership-contract.md) / [日本語](01-foundations/compiler-ownership-contract_ja.md)
- [Research prioritization policy](01-foundations/research-prioritization-policy.md) / [日本語](01-foundations/research-prioritization-policy_ja.md)
- [Research foundations](01-foundations/research-foundations.md) / [日本語](01-foundations/research-foundations_ja.md)
- [Research program and evidence policy](01-foundations/research-program.md) / [日本語](01-foundations/research-program_ja.md)
- [Project proposal](01-foundations/project-proposal.md) / [日本語](01-foundations/project-proposal_ja.md)
- [Metrics-first policy](01-foundations/metrics-policy.md)

## 02 — Research areas

`compiler/` covers owned semantic IR, transformations, target generation, native integration, and compiler prior art. `execution/` covers work partition and physical distribution. `measurement/` covers evidence validity. `toolchains/` covers the current [cross-ecosystem dependency-graph research](02-research-areas/toolchains/cross-ecosystem-dependency-graph.md) ([日本語](02-research-areas/toolchains/cross-ecosystem-dependency-graph_ja.md)), reference/bootstrap profiles, and operator UX.

The current Rust/C/C++ baseline is documented in [Rust/CargoにおけるC/C++ native依存のbuild model](02-research-areas/toolchains/rust-c-cpp-native-build-model_ja.md). The primary prior-art survey is [複数ecosystem依存とcompiler IRを結合して解く先行研究調査](02-research-areas/toolchains/cross-ecosystem-dependency-and-ir-resolution-landscape_ja.md). [Native executableをrootとするcross-layer枝刈り](02-research-areas/toolchains/cross-layer-reachability-pruning_ja.md) defines how demand pruning, compiler DCE, and linker garbage collection form one evidence-backed reachability contract.

The three lane foundation reports turn that landscape into explicit baselines, hypotheses, counterexamples, and M0/M1 experiments: [Lane A — Semantic and Artifact Closure](02-research-areas/toolchains/lane-a-semantic-artifact-closure-foundations_ja.md), [Lane B — Efficient Compiler Computation](02-research-areas/execution/lane-b-efficient-compiler-computation-foundations_ja.md), and [Lane C — Executable Verification and Testability](02-research-areas/toolchains/lane-c-executable-verification-foundations_ja.md). They share one typed graph and must not be read as independent architecture proposals.

[Dependency-discharge artifact contract](02-research-areas/toolchains/dependency-resolved-artifact-closure_ja.md) defines the user-facing result: package, source-semantic, language/intermediate-IR, ABI, symbol, and link obligations are transformed and discharged into a native artifact, while the original graph remains provenance and unavoidable runtime requirements remain explicit contracts. Pruning is a subordinate optimization of that process.

[Testable Native Artifactと第一級Test Harness](02-research-areas/toolchains/testable-native-artifact-harness_ja.md) makes testability a cross-cutting artifact property: exact production subjects, test artifacts, controls, observations, target environments, test-only dependencies, and raw evidence are represented in the same production graph rather than left to an external fixture validator.

These documents refine questions selected by the near-term program. They do not independently establish delivery priority.

## 03 — Work items

- [Project work portfolio](03-work-items/project-portfolio.md) manages the three coupled research lanes, shared milestone gates, outcomes, unissued gaps, implementation, verification, release, and later expansion.
- [Issue plan](03-work-items/issue-plan.md) maps the bounded GitHub-tracked projection to that portfolio.
- `design/` contains issue-specific experiment contracts, specifications, and evidence. Implemented behavior is verified by direct executable tests against production code; design fixtures and historical catalogs are not verification authorities.
- `review-contracts/` contains bounded review handoffs.

The active experiment starts at the [first cross-ecosystem dependency-graph experiment](03-work-items/design/cross-ecosystem-dependency-graph-first-experiment.md). Issue #31 remains a bounded semantic/WASM experiment, not the current project gate.

## 04 — Guides

Procedures for reference-project setup, Windows development, project builds, and delegated self-build baselines live here. A working baseline is not evidence that an owned compiler milestone is complete.

## 05 — History

Dated audits and retired machine-readable research catalogs live here to preserve why contracts changed. Historical documents provide provenance and do not override the current near-term program or direct executable tests.
