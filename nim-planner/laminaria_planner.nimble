# Package

version       = "0.1.0"
author        = "LAMINARIA"
description   = "LAMINARIA production Nim Planning Kernel: plan(PlanningInput) -> ExecutionPlan (issue #8)."
license       = "MIT OR Apache-2.0"
srcDir        = "src"
bin           = @["laminaria_planner", "laminaria_incremental_planner"]
binDir        = "bin"

# `laminaria-plan::nim_planner_client` looks for a binary literally named
# `laminaria-planner` (hyphenated, matching `laminaria-rustc-wrapper`/
# `laminaria-cc-wrapper`'s own naming convention in laminaria-run) next to
# the running laminaria-cli executable; Nim source/module names cannot
# contain hyphens, so the produced binary is renamed via `namedBin` rather
# than naming the source file itself with a hyphen.
namedBin["laminaria_planner"] = "laminaria-planner"
# Issue #36 T1's new session-scoped binary -- same hyphenation convention.
namedBin["laminaria_incremental_planner"] = "laminaria-incremental-planner"

# Dependencies

requires "nim >= 2.0.0"

task test, "Run the Planning Kernel test suite":
  exec "nim c -r --path:src --hints:off -o:bin/test_planning_kernel tests/test_planning_kernel.nim"
