# LAMINARIA documentation map

Start with [Project Progression and Near-Term Research Goal](near-term-research-program.md) ([日本語](near-term-research-program_ja.md)). It is the canonical entry point for the current goal, decision gates, milestone order, and task-selection rule. Do not choose work by scanning the directories below or by issue number alone.

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
4. Use the [issue plan](03-work-items/issue-plan.md) and issue-specific work item to execute or review that experiment.
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

`compiler/` covers owned semantic IR, transformations, target generation, native integration, and compiler prior art. `execution/` covers work partition and physical distribution. `measurement/` covers evidence validity. `toolchains/` covers reference/bootstrap profiles and operator UX.

These documents refine questions selected by the near-term program. They do not independently establish delivery priority.

## 03 — Work items

- [Issue plan](03-work-items/issue-plan.md) maps issues to the program.
- `design/` contains issue-specific experiment contracts, specifications, and evidence. Implemented behavior is verified by direct executable tests against production code; design fixtures and historical catalogs are not verification authorities.
- `review-contracts/` contains bounded review handoffs.

The active G1 experiment starts at [Issue #31 — first minimal cross-language owned-transformation experiment](03-work-items/design/issue-31-first-minimal-experiment.md).

## 04 — Guides

Procedures for reference-project setup, Windows development, project builds, and delegated self-build baselines live here. A working baseline is not evidence that an owned compiler milestone is complete.

## 05 — History

Dated audits and retired machine-readable research catalogs live here to preserve why contracts changed. Historical documents provide provenance and do not override the current near-term program or direct executable tests.
