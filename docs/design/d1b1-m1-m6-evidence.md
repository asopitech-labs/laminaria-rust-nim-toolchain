# D1-b1: M1 / M6 evidence (isolated `c812d70` snapshot)

Issue #28 D1-b1. Covers the 4 cases in `docs/design/issue-35-d0-cases.yaml`
that measure LAMINARIA's own whole-workspace compile graph
(`M1-fingerprint-cold`, `M1-fingerprint-noop`, `M1-fingerprint-leaf-edit`,
`M6-diamond-fingerprint-plan`). Run against an isolated `git worktree`
checked out at commit `c812d70` (the D0-accepted base commit), never the
current `main` checkout -- the current workspace has since grown
`crates/laminaria-experiment` (and other post-c812d70 additions), which
would change the exact crate set these cases' `expected` values were
pinned against. Never re-selects source layout, role, or expected value
-- all D0-fixed.

Not wired into an automated bin (unlike M2/M3-topology/M4/M5): this
procedure requires `git worktree add --detach <dir> c812d70` plus a
temporary source edit *inside that isolated worktree only*, reverted
before the worktree is removed -- a fundamentally different execution
shape from "run this crate's existing test/binary," so it is recorded
here as a reproducible procedure + result, the same evidence style this
project already uses for reference/bootstrap-role findings reported to
the issue tracker (`runs/` itself is gitignored project-wide; raw logs
are not committed, exactly as the existing M3/M8 owned-baseline JSON
output under `runs/d1a/` is not).

## Procedure

```bash
git worktree add --detach /tmp/laminaria-d1b1-m1-m6-snapshot c812d70
cd /tmp/laminaria-d1b1-m1-m6-snapshot
# confirm exactly 5 workspace members (laminaria-fingerprint, -run,
# -plan, -cli, -ir) -- the "5crate+2bin" shape M1's own source_layout
# names, with no crates/laminaria-experiment present.
```

### M1-fingerprint-cold (`execution_role: reference`, repetitions: 3)

```bash
for i in 1 2 3; do
  rm -rf target
  cargo build --workspace -v > cold-rep$i.log 2>&1
  grep -c "Compiling laminaria-fingerprint " cold-rep$i.log
done
```

**Result**: all 3 repetitions exit 0, `Compiling laminaria-fingerprint`
appears exactly once each. Matches `pass_criteria.d1`.

### M1-fingerprint-noop (`execution_role: reference`, repetitions: 3)

Continuing from the same `target/` `M1-fingerprint-cold`'s last
repetition left behind (no `rm -rf target` between these three runs, per
the case's own `source_layout`: "coldビルド完了後の状態から開始する"):

```bash
for i in 1 2 3; do
  cargo build --workspace -v > noop-rep$i.log 2>&1
  grep -c "Compiling " noop-rep$i.log
done
```

**Result**: all 3 repetitions exit 0, zero `Compiling` lines each time
(a genuine, immediate `Finished` with nothing rebuilt). Matches
`pass_criteria.d1` ("forbidden_workがゼロ回、3回とも再現").

### M1-fingerprint-leaf-edit (`execution_role: reference`, repetitions: 1)

The exact declared diff -- `crates/laminaria-fingerprint/src/env.rs`'s
`ENV_ALLOWLIST` array, appending `"NIM_CONFIG_DIR"` immediately after
`"WSL_INTEROP"` (the array's existing final element):

```bash
sed -i 's/    "WSL_INTEROP",/    "WSL_INTEROP",\n    "NIM_CONFIG_DIR",/' \
  crates/laminaria-fingerprint/src/env.rs
cargo build --workspace -v > leaf-edit.log 2>&1
grep -oE "Compiling [a-zA-Z0-9_-]+" leaf-edit.log | sort | uniq -c
grep "laminaria-ir" leaf-edit.log
git checkout -- crates/laminaria-fingerprint/src/env.rs   # revert
```

**Result**: exit 0.
`Compiling laminaria-fingerprint` / `laminaria-plan` / `laminaria-run` /
`laminaria-cli` each appear exactly once; `laminaria-ir` is reported
`Fresh` (never recompiled -- it does not depend on
`laminaria-fingerprint`). Matches `pass_criteria.d1` ("再ビルド集合が
期待通り、余分な再ビルドがゼロ、fingerprint自体の重複コンパイルなし").
Edit reverted immediately after; `git status --porcelain` confirmed
empty before proceeding.

### M6-diamond-fingerprint-plan (`execution_role: reference`, repetitions: 1)

Per the case's own text, reuses the identical `ENV_ALLOWLIST` diff above
(no new struct field, per the case's own P1 correction), requesting only
`laminaria-cli`'s own artifact (`target/debug/laminaria`) to exercise its
specific 4-path convergence onto `laminaria-fingerprint` and 2-path
convergence onto `laminaria-plan`:

```bash
sed -i 's/    "WSL_INTEROP",/    "WSL_INTEROP",\n    "NIM_CONFIG_DIR",/' \
  crates/laminaria-fingerprint/src/env.rs
cargo build -p laminaria-cli -v > m6-diamond.log 2>&1
grep -oE "Compiling [a-zA-Z0-9_-]+" m6-diamond.log | sort | uniq -c
git checkout -- crates/laminaria-fingerprint/src/env.rs   # revert
```

**Result**: exit 0. `Compiling laminaria-fingerprint` /
`laminaria-plan` / `laminaria-run` / `laminaria-cli` each appear exactly
once -- despite `laminaria-cli` reaching `laminaria-fingerprint` via 4
distinct dependency paths and `laminaria-plan` via 2 (per the case's own
`dependency_edges`), neither is compiled more than once. Matches
`pass_criteria.d1` ("重複コンパイルがゼロ、伝播集合が期待通り"). Edit
reverted immediately after; `git status --porcelain` confirmed empty
before the worktree was removed.

## Cleanup

```bash
git worktree remove --force /tmp/laminaria-d1b1-m1-m6-snapshot
```

The main checkout was never touched by any of the above -- every build
and edit happened inside the isolated worktree only.

## Summary

| case | reps | result |
|---|---|---|
| M1-fingerprint-cold | 3/3 | PASS |
| M1-fingerprint-noop | 3/3 | PASS |
| M1-fingerprint-leaf-edit | 1/1 | PASS |
| M6-diamond-fingerprint-plan | 1/1 | PASS |

All 4 cases reproduce their D0-pinned `pass_criteria.d1` exactly, against
the isolated `c812d70` snapshot, with the current workspace's own
`crates/laminaria-experiment` addition excluded from the measured
dependency graph as required.
