# Goal-Driven Work Instruction Policy

## 1. Purpose

LAMINARIA work instructions define a finite path from the current project state
to an observable project outcome. They do not primarily enumerate forbidden
implementations. A prohibition list is open-ended and cannot guarantee that an
unlisted shortcut still serves the goal.

The instruction author owns the goal, the semantic decisions, the acceptance
relations, and the checkpoint sequence. An implementer owns the engineering
choices used to move between those fixed checkpoints. If a new product,
research, semantic, evidence, or authority decision is required, the
implementer stops and returns the decision to the instruction author.

Apply this policy together with the [near-term research program](../near-term-research-program.md),
the [project portfolio](../03-work-items/project-portfolio.md), and the
[fixture policy](fixture-policy.md).

## 2. Instruction order

Every substantial work instruction is written in this order:

1. **Goal** — the externally meaningful state that must become true and why it
   matters to the parent project outcome.
2. **Starting state** — relevant capabilities and known gaps, without turning
   the current implementation into the specification.
3. **Checkpoint A, B, ...** — a short, ordered chain of observable intermediate
   results that necessarily leads to the goal.
4. **Verification gate** — direct observations and independent relations that
   can falsify completion.
5. **Handoff** — the exact output the next milestone consumes and who decides
   closure or a change of direction.

Non-goals and safety constraints may follow this positive path. They are
guardrails, not the organizing structure of the instruction.

## 3. Goal requirements

A goal states all of the following:

- the requested subject or artifact;
- its externally observable behavior;
- the dependency, environment, and identity boundary in which it must hold;
- the parent capability or research decision it advances;
- the whole-project outcome, cross-lane effects, and meaningful trade-offs it
  is allowed to make;
- the evidence that would prove the result wrong.

Use outcome language such as “the requested native artifact runs in a clean
environment and every retained dependency obligation has production evidence.”
Do not substitute activity language such as “add an enum,” “write seven tests,”
or “call a particular command” for the goal.

Implementation names, commands, modules, and test names are fixed only when
they are already part of a public or cross-component contract. Otherwise they
are possible means, not project outcomes.

## 4. Checkpoint contract

Each checkpoint contains exactly five fields:

| Field | Required meaning |
| --- | --- |
| Result | The observable state that exists when the checkpoint is complete |
| Consumes | Authoritative inputs and the previous checkpoint output |
| Must preserve | Semantic, identity, side-effect, and ownership invariants |
| Evidence | A direct observation or independent relation that can falsify the result |
| Enables | The next checkpoint that can now start without reconstructing or guessing state |

Checkpoint results bind the path more reliably than a list of anticipated bad
implementations. For example, “the negative workload returns a structured
rejection while the process trace and filesystem diff show that no production
action began” is a result. A growing list of disallowed compiler command
spellings is not an equivalent contract.

The sequence must be causally complete: every final acceptance claim is
produced by one checkpoint, and every checkpoint output is consumed by a later
checkpoint or the final handoff. Remove ceremonial checkpoints that do not
change what can be decided or executed next.

The instruction author must be able to state the completion relation as:

```text
Checkpoint A result
  enables Checkpoint B with its required authoritative input
Checkpoint B result
  enables the verification gate against the requested subject
Verification-gate result
  establishes the Goal inside its stated boundary
```

This relation, rather than completion of a collection of assigned activities,
is the work contract.

## 5. Decision ownership

The instruction author decides before assigning work:

- the project goal and parent outcome;
- semantic meaning and lifecycle states;
- artifact and subject identity;
- authority boundaries and permitted side effects;
- checkpoint order and handoff shape;
- acceptance oracles and the party authorized to declare completion.

The implementer decides within those boundaries:

- internal decomposition and local data structures;
- naming that is not part of a contract;
- refactoring needed to reach a checkpoint;
- efficient implementation techniques;
- additional tests that strengthen the stated verification relation.

An implementer does not silently decide that an unexpected behavior is “close
enough,” reinterpret a checkpoint, weaken an oracle, or replace a production
result with a surrogate. The correct response is a checkpoint failure report:
the observed state, evidence, affected checkpoint, and decision needed from the
instruction author.

## 6. Instruction-author preflight

Before issuing work, the instruction author performs a goal-to-checkpoint
preflight:

1. Trace the goal to the current near-term program and portfolio capability.
2. Confirm that every checkpoint advances that goal rather than merely changing
   code or making CI green.
3. Verify unstable assumptions about tools and side effects empirically. A
   command name such as `dump`, `metadata`, or `check` is not evidence that the
   operation is read-only.
4. For every checkpoint, identify a plausible wrong result that could satisfy a
   weak test, then strengthen the result or oracle so that it fails.
5. Confirm that the final handoff contains everything the next checkpoint needs;
   it must not rely on state produced outside the recorded path.
6. Check whether a checkpoint improves its local subject by transferring work,
   resource cost, complexity, risk, or maintenance to another checkpoint,
   component, lane, platform, or later milestone. Record and evaluate that
   trade-off at the parent-goal boundary.
7. Set an evidence budget and stop condition. Additional metrics are required
   only while they can distinguish live choices or falsify the goal.
8. Remove implementation prescriptions that are not necessary to preserve a
   contract.

If the instruction author cannot state a complete positive checkpoint path,
the work is not ready to delegate. Create or resolve the missing decision first.

## 7. Verification gate

Verification follows the checkpoint path, not a checklist of code shapes.

For each checkpoint, record:

- the production entry point exercised;
- the input and subject identity;
- the observed result;
- the independent relation or oracle;
- the downstream checkpoint enabled by that result.

The verification gate also records the end-to-end effect and any cost displaced
outside the checkpoint. A local metric improvement cannot establish the goal
when whole-path latency, memory, I/O, recomputation, artifact quality,
reliability, operability, maintenance, or a different lane materially regresses
without an accepted project-level trade-off.

CI success, test counts, source-text searches, enum presence, graph-node
presence, or absence of one known command are supporting observations only.
They establish completion only when they directly constitute the checkpoint's
stated result.

The reviewer tests the causal chain. Presence is not reachability; a planned
action is not executed evidence; a generated path is not an artifact; a test
variant is not the production subject; and a serialized implementation output
is not an independent oracle.

## 8. Deviation and revision protocol

When a checkpoint cannot be reached as written:

1. Stop before changing the checkpoint meaning or substituting another result.
2. Report the smallest observation that falsified the instruction assumption.
3. Identify the blocked checkpoint and the owner decision required.
4. The instruction author revises the goal path or explicitly preserves it and
   chooses a new implementation route.
5. Resume only from the last checkpoint whose result remains valid.

An issue-body revision changes the authoritative path. Progress comments and
completion reports provide evidence but do not override the goal or checkpoint
contracts.

## 9. Completion responsibility

The implementer reports checkpoint evidence and readiness. The reviewer owns
the completion decision unless the issue explicitly assigns it elsewhere.

When a submission misses the goal, review the instruction first:

- Was the goal externally observable?
- Did the checkpoint chain actually imply it?
- Could a weak surrogate satisfy the stated evidence?
- Did the instruction prescribe a faulty means instead of a required result?
- Was a decision incorrectly delegated to the implementer?

Correct instruction defects before requesting rework. A submission produced in
good faith from an ambiguous or incorrect instruction is evidence that the
instruction contract failed.

## 10. Required issue shape

An implementation or experiment issue is ready for assignment only when it has
this positive structure:

```text
Goal
  observable project result and parent outcome

Starting state
  reusable capabilities and known gap

Checkpoint A
  Result / Consumes / Must preserve / Evidence / Enables

Checkpoint B
  Result / Consumes / Must preserve / Evidence / Enables

Verification gate
  end-to-end observations and independent oracles

Handoff
  next consumer, closure authority, and stop condition
```

A list of files to edit, commands to avoid, or tests to add cannot replace this
structure.
