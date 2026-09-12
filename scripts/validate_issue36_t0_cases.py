#!/usr/bin/env python3
"""Structural validator for docs/design/issue-36-t0-cases.yaml.

This is a documentation-fixture check, not a reimplementation of the
incremental Nim planner's own runtime (that is issue #36 T1's job). It
checks exactly the properties the T0 review round required
(docs/design/issue-36-t0-incremental-contract.md's "第2版改訂" header,
fix#1/#2/#5/#7):

  1. Every Action in `action_catalog` has the required fields
     (id/kind/command_identity/inputs/outputs/compiler_work), and for the
     four real compiler-work kinds, its declared `id` is recomputed from
     `compiler_work`'s own fields via the *same* FNV-1a algorithm
     `crates/laminaria-plan/src/compiler_work.rs:251-263` implements (a
     Python reimplementation, not the real Rust code -- this only proves
     internal self-consistency of the fixture, not that the real Rust
     function produces this id; T1's own tests are the actual proof of
     that).
  2. Every `DependencyDiscovered` event's `new_actions` is non-empty
     (fix#1).
  3. Every case's `start_session` (`initial_graph` + `initial_demands`) is
     valid standing alone: every `declared` input an action lists is
     produced by some other action already in the same initial graph, and
     the initial graph contains no dependency cycle (fix#5 -- diagnosed
     invalid states must be introduced by `apply_deltas`, never by
     `start_session` itself).
  4. Every event (in `apply_deltas` or `apply_delta_orderings`) carries
     all five required envelope fields (fix#7's schema-conformance
     requirement, reused here as a general event invariant).
  5. Wherever a case expects `diagnostic: stale_generation`, the event's
     own `planning_generation` is strictly less than this validator's own
     running simulation of the current generation at that point in the
     case (a simple count of `DependencyDiscovered` events so far that
     introduced at least one action id not already known) -- confirming
     the case is genuinely stale, not merely labeled as such (fix#2).
  6. Within `apply_delta_orderings` (case4), any two events sharing the
     same `event_id` carry byte-identical payloads (a real duplicate),
     never merely a coincidentally-reused id (fix#6).
"""

import argparse
import sys
from pathlib import Path

try:
    import yaml
except ImportError:  # pragma: no cover - exercised only when PyYAML is missing
    print(
        "ERROR: PyYAML is required to validate issue-36-t0-cases.yaml "
        "(pip install pyyaml)",
        file=sys.stderr,
    )
    sys.exit(1)

MASK = (1 << 64) - 1
OFFSET_BASIS = 0xCBF29CE484222325
PRIME = 0x00000100000001B3


class ValidationError(Exception):
    pass


def fnv1a_write_str(h: int, s: str) -> int:
    for b in s.encode():
        h ^= b
        h = (h * PRIME) & MASK
    h ^= 0
    h = (h * PRIME) & MASK
    return h


def compute_artifact_id(operation: str, operation_version: str, semantic_parts):
    h = OFFSET_BASIS
    h = fnv1a_write_str(h, operation)
    h = fnv1a_write_str(h, operation_version)
    for part in semantic_parts:
        h = fnv1a_write_str(h, part)
    return format(h, "016x")


def canonical_test_inputs(rows):
    return ";".join(",".join(str(v) for v in row) for row in rows)


def recompute_action_id(action, errors, catalog_key):
    """Recomputes `action['id']` from its own `compiler_work` descriptor
    using the same shape as compiler_work.rs's own *_artifact_id
    functions, and appends a message to `errors` if it disagrees with the
    declared id. Actions with `compiler_work: null` (the abstract,
    structural-only case10 actions) are skipped -- they are explicitly
    not tied to a real compiler-work kind."""
    cw = action.get("compiler_work")
    if cw is None:
        return
    kind = action["kind"]
    op_version = cw["operation_version"]
    declared_id = action["id"]

    if kind == "LowerSource":
        joined = ",".join(cw["requested_functions"])
        recomputed = compute_artifact_id(
            "lower_source",
            op_version,
            [cw["language"], cw["source_provenance"]["source_snapshot_id"], joined, cw["contract_version"]],
        )
    elif kind == "ValidateIr":
        program_candidate_id = cw["semantic_input_artifact_ids"][0]
        recomputed = compute_artifact_id(
            "validate_ir", op_version, [program_candidate_id, cw["contract_version"]]
        )
    elif kind == "EvaluateEvidence":
        validated_artifact_id = cw["semantic_input_artifact_ids"][0]
        function_name = cw["requested_functions"][0]
        recomputed = compute_artifact_id(
            "evaluate_evidence",
            op_version,
            [validated_artifact_id, function_name, canonical_test_inputs(cw["test_inputs"]), cw["contract_version"]],
        )
    elif kind == "DiscoverSourceDependencies":
        joined = ",".join(cw["requested_functions"])
        recomputed = compute_artifact_id(
            "discover_source_dependencies",
            op_version,
            [cw["language"], cw["source_provenance"]["source_snapshot_id"], joined, cw["contract_version"]],
        )
    else:
        errors.append(f"action_catalog.{catalog_key}: unknown kind {kind!r}, cannot recompute id")
        return

    if recomputed != declared_id:
        errors.append(
            f"action_catalog.{catalog_key}: declared id {declared_id!r} does not match "
            f"recomputed id {recomputed!r} from its own compiler_work descriptor"
        )


REQUIRED_ACTION_FIELDS = ("id", "kind", "command_identity", "inputs", "outputs", "compiler_work")
REQUIRED_EVENT_FIELDS = ("event_id", "sequence_number", "planning_generation", "emitted_at_unix_ns", "kind")


def check_action_catalog(catalog, errors):
    for key, action in catalog.items():
        missing = [f for f in REQUIRED_ACTION_FIELDS if f not in action]
        if missing:
            errors.append(f"action_catalog.{key}: missing required field(s) {missing}")
            continue
        recompute_action_id(action, errors, key)


def collect_outputs(actions):
    produced = set()
    for action in actions:
        for output in action.get("outputs", []):
            if output.get("kind") == "declared":
                produced.add(output["artifact_id"])
    return produced


def check_start_session_valid(case_id, actions, errors):
    """fix#5: every case's own initial graph must be valid standing
    alone -- no declared input without a producer in the same set, and no
    cycle among the initial actions."""
    produced = collect_outputs(actions)
    by_id = {a["id"]: a for a in actions}

    for action in actions:
        for inp in action.get("inputs", []):
            if inp.get("kind") == "declared" and inp["artifact_id"] not in produced:
                errors.append(
                    f"{case_id}: start_session is not self-valid -- action {action['id']!r} "
                    f"declares input {inp['artifact_id']!r} with no producer in the same initial_graph "
                    f"(fix#5: diagnosed-invalid states must come from apply_deltas, not start_session)"
                )

    # Cycle check (simple DFS over the initial actions only).
    WHITE, GRAY, BLACK = 0, 1, 2
    color = {a["id"]: WHITE for a in actions}

    def visit(action_id, stack):
        if color[action_id] == BLACK:
            return
        if color[action_id] == GRAY:
            errors.append(f"{case_id}: start_session contains a cycle: {' -> '.join(stack + [action_id])}")
            return
        color[action_id] = GRAY
        action = by_id.get(action_id)
        if action is not None:
            for inp in action.get("inputs", []):
                if inp.get("kind") == "declared":
                    producer = next(
                        (a["id"] for a in actions if any(
                            o.get("kind") == "declared" and o["artifact_id"] == inp["artifact_id"]
                            for o in a.get("outputs", [])
                        )),
                        None,
                    )
                    if producer is not None:
                        visit(producer, stack + [action_id])
        color[action_id] = BLACK

    for action in actions:
        visit(action["id"], [])


def check_event_envelope(case_id, event, errors, where):
    missing = [f for f in REQUIRED_EVENT_FIELDS if f not in event]
    if missing:
        errors.append(f"{case_id} ({where}): event missing required field(s) {missing}: {event}")
        return
    if event["kind"] == "DependencyDiscovered":
        new_actions = event.get("new_actions", [])
        if not new_actions:
            errors.append(
                f"{case_id} ({where}): DependencyDiscovered event {event['event_id']!r} has empty "
                f"new_actions (fix#1: must always be >= 1)"
            )


def simulate_generation_and_check_staleness(case_id, apply_deltas, errors):
    """fix#2: wherever a delta's expected_response carries
    `diagnostic: stale_generation`, the event's own planning_generation
    must genuinely be less than this simulation's running current
    generation -- otherwise the case never actually exercises staleness."""
    known_ids = set()
    current_generation = 0
    for delta in apply_deltas:
        event = delta["event"]
        expected = delta.get("expected_response", {})
        if expected.get("diagnostic") == "stale_generation":
            if not (event["planning_generation"] < current_generation):
                errors.append(
                    f"{case_id}: event {event['event_id']!r} is expected to be diagnosed "
                    f"stale_generation, but its planning_generation ({event['planning_generation']}) "
                    f"is not less than the simulated current generation ({current_generation}) at "
                    f"that point (fix#2: this case must actually reach a later generation first)"
                )
        if event["kind"] == "DependencyDiscovered":
            new_ids = {a["id"] for a in event.get("new_actions", [])}
            genuinely_new = new_ids - known_ids
            known_ids |= new_ids
            if genuinely_new and expected.get("diagnostic") != "stale_generation":
                current_generation += 1


def check_ordering_duplicates(case_id, orderings, errors):
    """fix#6: within apply_delta_orderings, two events sharing the same
    event_id must carry byte-identical payloads (a real duplicate), never
    a coincidentally-reused id with different content."""
    for ordering in orderings:
        by_event_id = {}
        for event in ordering["events"]:
            eid = event["event_id"]
            stripped = {k: v for k, v in event.items() if k != "command_index"}
            if eid in by_event_id and by_event_id[eid] != stripped:
                errors.append(
                    f"{case_id} (ordering {ordering['name']!r}): two events share event_id {eid!r} "
                    f"but have different payloads -- not a real duplicate (fix#6)"
                )
            by_event_id[eid] = stripped


def validate(doc):
    errors = []
    check_action_catalog(doc["action_catalog"], errors)

    for case in doc["cases"]:
        case_id = case["id"]
        start = case.get("start_session")
        if start is not None:
            actions = start["initial_graph"]["actions"]
            check_start_session_valid(case_id, actions, errors)

        if "apply_deltas" in case:
            for delta in case["apply_deltas"]:
                check_event_envelope(case_id, delta["event"], errors, "apply_deltas")
            simulate_generation_and_check_staleness(case_id, case["apply_deltas"], errors)

        if "apply_delta_orderings" in case:
            for ordering in case["apply_delta_orderings"]:
                for event in ordering["events"]:
                    check_event_envelope(case_id, event, errors, f"ordering {ordering['name']}")
            check_ordering_duplicates(case_id, case["apply_delta_orderings"], errors)

        if "pass_criteria" not in case or "t1" not in case["pass_criteria"]:
            errors.append(f"{case_id}: missing pass_criteria.t1")
        if "pass_criteria" in case and "d1" in case["pass_criteria"]:
            errors.append(f"{case_id}: pass_criteria.d1 must not be present (use .t1)")

    return errors


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "path",
        nargs="?",
        default=str(Path(__file__).resolve().parent.parent / "docs" / "design" / "issue-36-t0-cases.yaml"),
    )
    args = parser.parse_args(argv)

    # Explicit utf-8, not the platform default: this file's own comments
    # are in Japanese, and Windows' default codepage (cp1252) cannot
    # decode them at all -- reproduced directly via the "Reference
    # project setup" workflow's windows-latest job, not assumed.
    with open(args.path, encoding="utf-8") as f:
        doc = yaml.safe_load(f)

    errors = validate(doc)
    if errors:
        print(f"FAILED: {len(errors)} issue(s) found in {args.path}", file=sys.stderr)
        for e in errors:
            print(f"  - {e}", file=sys.stderr)
        return 1

    print(f"OK: {args.path} passed all structural checks ({len(doc['cases'])} cases)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
