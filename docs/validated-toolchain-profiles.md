# Validated Toolchain Profiles Research Direction

## Purpose

LAMINARIA should accept a broad internal combination space across Rust/Nim compiler versions, backends, targets, linkers, post-link tools, and runtimes while exposing a much smaller set of known-good paths to ordinary users.

The primary user-facing entry point is a **Validated Toolchain Profile**: an exact bundle that LAMINARIA has built, executed, measured, and qualified through reproducible evidence.

Central rule:

> Broad candidate space internally; narrow known-good paths by default.

## 1. Separate candidates from recommendations

```text
Candidate Variant Space
  Rust versions x Nim versions x backend x target x linker x optimizer x runtime ...
        -> constraints
Compatible Candidates
        -> qualification / evidence
Validated Profiles
        -> user-facing selection
Resolved Exact Toolchain Bundle
```

Constraint compatibility does not by itself make a combination recommended.

## 2. Separate upstream freshness from LAMINARIA validation

Upstream labels such as latest, stable, nightly, or LTS describe upstream release/support state. They do not describe whether a cross-toolchain combination has passed LAMINARIA qualification.

Track separately:

```text
Upstream selection/freshness:
  latest stable
  older stable
  beta/nightly/devel
  upstream LTS where actually provided

LAMINARIA qualification:
  fully validated
  validated with limitations
  smoke-tested
  unvalidated
  rejected
```

Do not promote `latest upstream` directly to `recommended`.

Rust uses stable/beta/nightly release trains; LAMINARIA must not imply an upstream Rust LTS channel where none exists. A long-lived bundle should be labeled as a **LAMINARIA-maintained long-term profile**.

An upstream Nim LTS designation, when applicable, is useful metadata, but it does not qualify the complete Rust/Nim/LLVM/linker bundle by itself.

## 3. User-facing profiles

### `recommended`

Default. The exact bundle with the strongest current evidence for general use. Priority is compatibility and low known-failure risk, then acceptable performance/resources and critical fixes, then freshness. It does not have to be the newest upstream release.

### `latest-validated`

The newest stable-oriented bundle that has passed the required LAMINARIA qualification suite. It may intentionally lag `latest upstream`.

### `long-term`

A LAMINARIA-managed bundle with lower update frequency and a documented support/retirement policy. If Rust has no upstream LTS channel, the UI must explicitly say this is a LAMINARIA long-term profile rather than an upstream guarantee.

### `preview`

May contain Rust beta/nightly, Nimony/devel, new backend routes, or other partially qualified capabilities. Validation status and known limitations are always visible.

### `custom`

Enables detailed constraints over compiler versions/revisions, Cargo/Nimble, edition/MSRV policy, backend engine, LTO, native compiler/linker, Wasm target/link/post-link/component model, runtime/memory model, and optimization/debug settings.

## 4. Progressive disclosure

### Level 1 — Profile only

```text
Recommended
Latest Validated
Long-Term
Preview
Custom
```

Most users should stop here.

### Level 2 — Intent presets

A profile can be refined by intent, for example:

```text
Default / General Development
Fast Iteration
Release / Maximum Optimization
Native
WebAssembly Core
WebAssembly Component
Compatibility-oriented
```

Intent presets add route/optimization constraints without requiring exact-version selection.

### Level 3 — Advanced overrides

Users may override individual dimensions over a profile:

```text
base = recommended
rust = <exact rustc>
lto = thin
```

The resulting combination must be re-qualified. It must not inherit the base profile's validation badge unchanged.

### Level 4 — Expert graph constraints

CLI/config may expose direct variant, artifact-boundary, and backend-route constraints. This is not the default UI path.

## 5. Profiles are moving aliases; Runs use exact identities

`recommended`, `latest-validated`, and similar names may move. Every execution resolves them to a versioned profile revision and an exact bundle.

```text
requested_profile = recommended
profile_revision = <immutable revision>
resolved_bundle = {
  rustc = exact version/revision
  cargo = exact version/build
  nim = exact version/revision
  llvm = exact build
  linker = exact build
  ...
}
```

Run/Action/Artifact/cache identities use the resolved exact identities, not the profile alias. Runs retain the requested profile and immutable profile revision for reproducibility.

## 6. Qualification evidence

Profile qualification uses the Measurement Spine (#11/#18–#21) and should include as applicable:

- exact environment/toolchain fingerprints;
- bootstrap success;
- representative Rust-only, Nim-only, and mixed Rust+Nim builds;
- native/direct/C-ABI baseline paths where relevant;
- check/build/test scenarios;
- cold/warm/no-op/incremental execution correctness;
- proof of backend/target route actually used;
- CPU/memory/I/O/performance regressions;
- artifact/code-size and runtime checks where relevant;
- known unsupported capabilities and failure signatures.

Qualification is a capability-coverage matrix, not one global boolean.

## 7. Qualification matrix

A profile can be validated differently across host, target, and capability dimensions.

```text
Profile: recommended@revision

host:
  linux-x86_64: validated
  windows-wsl2-x86_64: validated-with-limitations
  macos-arm64: validated

target:
  native: validated
  wasm32-core: validated
  wasm-component: experimental

features:
  Rust stable frontend: validated
  Nim 2 frontend: validated
  Nimony frontend: partial
  ThinLTO: validated
  DTLTO: research
```

Do not hide scope behind one undifferentiated `validated` label.

## 8. Promotion and demotion

A new upstream release first becomes a candidate.

```text
Upstream release discovered
  -> Candidate generated
  -> Bootstrap/smoke qualification
  -> Full qualification suite
  -> Performance/resource comparison
  -> Profile promotion decision
```

Possible progression:

```text
unvalidated
-> smoke-tested
-> validated-with-limitations
-> fully-validated
-> latest-validated
-> recommended (policy decision)
```

Profiles can be demoted or rolled back after regressions. Historical profile revisions remain immutable.

## 9. Long-term profile policy

LAMINARIA Long-Term is not the union of upstream LTS labels. It defines:

- exact tool versions;
- qualification scope;
- support window or retirement criteria;
- allowed security/critical patch updates;
- migration policy when compatibility breaks;
- profile revisioning policy.

If Rust has no upstream LTS channel, LAMINARIA selects and qualifies a stable Rust release for the profile's support window. Upstream Nim LTS status, when applicable, remains only one input into bundle qualification.

## 10. Failure behavior

Failure inside the documented qualification scope of a validated profile is a candidate LAMINARIA profile regression, not automatically a user error.

Preview/custom/unvalidated combinations may proceed when constraints and safety permit, but the UI must expose validation status, missing evidence, known incompatible edges, opaque regions, and fallbacks.

Unknown artifact compatibility remains fail-closed even in advanced mode.

## 11. Explainability

Candidate interfaces:

```text
laminaria toolchain profiles
laminaria toolchain profile recommended
laminaria explain-toolchain-selection
laminaria explain-profile-qualification
```

Users should be able to see why a profile is default, the exact resolved bundle, differences from upstream latest, qualification coverage, known limitations, what guarantees an override invalidates, and why a candidate was rejected.

## 12. Success criteria

1. Internal candidate space and user-facing validated profiles are separate models.
2. Default use does not require users to solve exact compiler-version combinations manually.
3. `recommended`, `latest-validated`, `long-term`, `preview`, and `custom` have distinct policies.
4. Upstream freshness and LAMINARIA qualification are independent dimensions.
5. Profiles resolve to exact versioned bundles that propagate into Run/Action/Artifact identity.
6. Qualification uses reproducible Measurement Spine evidence.
7. Overrides trigger qualification re-evaluation.
8. Expert users can constrain individual dimensions beyond presets.
9. LAMINARIA does not present its own long-term Rust selection as an upstream Rust LTS guarantee.
10. Failures on validated default paths can be detected and tracked as profile regressions.
