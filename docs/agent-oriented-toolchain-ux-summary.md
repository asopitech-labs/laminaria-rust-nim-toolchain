# Toolchain UX Objective Summary

LAMINARIA treats tool UX as a first-class engineering objective.

The internal system may accept and reason over a very broad Rust/Nim/compiler/backend/target combination space. The normal human and coding-agent UX must not expose that combinatorial space as repeated external build attempts.

The default path is:

```text
requirements + intent
  -> validated profiles / known compatibility
  -> constraint solving and pruning
  -> ranked viable plan(s)
  -> structured explanation
  -> execution
```

Coding agents should not discover compatible toolchains by repeatedly editing settings, running builds, parsing failures, and trying another combination. Known invalid states, previous qualification failures, capability gaps, and unsupported edges should be preserved as structured negative knowledge and reused across runs/sessions.

UX quality is measured by execution attempts avoided, candidates pruned before execution, time to a viable plan, validation coverage, structured-output size/context cost, and repeated rediscovery avoided—not only by CLI brevity.

The full design is in `agent-oriented-toolchain-ux.md` / `_ja.md`.
