"""Unit tests for scripts/validate_issue36_t0_cases.py.

Exercises `validate()` directly against small, in-memory doc fixtures
(never re-parses the real docs/design/issue-36-t0-cases.yaml for these
unit tests) plus one end-to-end pass against the real file, to catch a
validator that would otherwise rubber-stamp anything."""

import importlib.util
from pathlib import Path
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "validate_issue36_t0_cases.py"
spec = importlib.util.spec_from_file_location("validate_issue36_t0_cases", SCRIPT)
validator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(validator)


def minimal_action(action_id, kind="LowerSource", inputs=None, outputs=None, compiler_work="auto"):
    if compiler_work == "auto":
        compiler_work = {
            "descriptor_schema_version": "0.1.0",
            "operation_version": "0.1.0",
            "semantic_input_artifact_ids": [],
            "requested_functions": ["f"],
            "language": "rust",
            "contract_version": "0.1.0",
            "transform": None,
            "source_provenance": {"source_file": "f.rs", "source_snapshot_id": "deadbeef"},
            "test_inputs": [],
            "resource_request": {"cpu_slots": 1, "transient_memory_bytes_estimate": 0},
            "budget_token": "budget-1",
        }
        if action_id is None:
            # Make the id genuinely consistent with this descriptor by
            # construction, only when the caller didn't ask for a
            # specific (possibly deliberately wrong) id.
            action_id = validator.compute_artifact_id(
                "lower_source", "0.1.0", ["rust", "deadbeef", "f", "0.1.0"]
            )
    return {
        "id": action_id,
        "kind": kind,
        "command_identity": "lower_source",
        "inputs": inputs or [],
        "outputs": outputs if outputs is not None else [{"kind": "declared", "artifact_id": action_id}],
        "compiler_work": compiler_work,
    }


def minimal_event(event_id, kind="ProducerCompleted", planning_generation=0, seq=1, **extra):
    event = {
        "event_id": event_id,
        "sequence_number": seq,
        "planning_generation": planning_generation,
        "emitted_at_unix_ns": 1,
        "kind": kind,
    }
    event.update(extra)
    return event


def minimal_doc(cases):
    return {"action_catalog": {}, "cases": cases}


class ActionCatalogChecksTests(unittest.TestCase):
    def test_flags_a_declared_id_that_does_not_match_its_own_descriptor(self):
        action = minimal_action("this-id-is-wrong")
        errors = []
        validator.recompute_action_id(action, errors, "K")
        self.assertTrue(errors, "a mismatched id must be flagged")

    def test_accepts_a_correctly_recomputed_id(self):
        action = minimal_action(None)  # id derived from its own descriptor
        errors = []
        validator.recompute_action_id(action, errors, "K")
        self.assertEqual(errors, [])

    def test_flags_a_missing_required_field(self):
        action = minimal_action(None)
        del action["command_identity"]
        errors = []
        validator.check_action_catalog({"K": action}, errors)
        self.assertTrue(any("missing required field" in e for e in errors))


class StartSessionValidityTests(unittest.TestCase):
    def test_flags_a_dangling_declared_input_in_the_initial_graph(self):
        consumer = minimal_action(
            "consumer", inputs=[{"kind": "declared", "artifact_id": "never-produced"}], outputs=[]
        )
        errors = []
        validator.check_start_session_valid("case-x", [consumer], errors)
        self.assertTrue(any("no producer in the same initial_graph" in e for e in errors))

    def test_accepts_a_self_contained_producer_consumer_pair(self):
        producer = minimal_action("p", outputs=[{"kind": "declared", "artifact_id": "p-out"}])
        consumer = minimal_action(
            "c", inputs=[{"kind": "declared", "artifact_id": "p-out"}], outputs=[]
        )
        errors = []
        validator.check_start_session_valid("case-x", [producer, consumer], errors)
        self.assertEqual(errors, [])

    def test_flags_a_two_action_cycle(self):
        a = minimal_action("a", inputs=[{"kind": "declared", "artifact_id": "b-out"}],
                            outputs=[{"kind": "declared", "artifact_id": "a-out"}])
        b = minimal_action("b", inputs=[{"kind": "declared", "artifact_id": "a-out"}],
                            outputs=[{"kind": "declared", "artifact_id": "b-out"}])
        errors = []
        validator.check_start_session_valid("case-x", [a, b], errors)
        self.assertTrue(any("cycle" in e for e in errors))


class EventEnvelopeAndDependencyDiscoveredTests(unittest.TestCase):
    def test_flags_a_missing_envelope_field(self):
        event = minimal_event("e1")
        del event["sequence_number"]
        errors = []
        validator.check_event_envelope("case-x", event, errors, "apply_deltas")
        self.assertTrue(any("missing required field" in e for e in errors))

    def test_flags_an_empty_new_actions_list(self):
        event = minimal_event("e1", kind="DependencyDiscovered", new_actions=[])
        errors = []
        validator.check_event_envelope("case-x", event, errors, "apply_deltas")
        self.assertTrue(any("empty new_actions" in e for e in errors))


class StalenessSimulationTests(unittest.TestCase):
    def test_flags_a_stale_diagnostic_when_generation_never_actually_advanced(self):
        # Reproduces the exact defect the review caught in the first
        # revision's case6: current generation and event generation both 0.
        deltas = [
            {
                "event": minimal_event("e1", kind="DependencyDiscovered", planning_generation=0,
                                        new_actions=[minimal_action(None)]),
                "expected_response": {"diagnostic": "stale_generation"},
            }
        ]
        errors = []
        validator.simulate_generation_and_check_staleness("case-x", deltas, errors)
        self.assertTrue(errors, "a same-generation 'stale' claim must be flagged")

    def test_accepts_a_genuinely_stale_event_after_a_real_advance(self):
        real_action = minimal_action(None)
        deltas = [
            {
                "event": minimal_event("e1", kind="DependencyDiscovered", planning_generation=0,
                                        new_actions=[real_action]),
                "expected_response": {"diagnostic": None},
            },
            {
                "event": minimal_event("e2", kind="DependencyDiscovered", planning_generation=0,
                                        new_actions=[real_action]),
                "expected_response": {"diagnostic": "stale_generation"},
            },
        ]
        errors = []
        validator.simulate_generation_and_check_staleness("case-x", deltas, errors)
        self.assertEqual(errors, [])


class OrderingDuplicateTests(unittest.TestCase):
    def test_flags_a_reused_event_id_with_different_payload(self):
        orderings = [
            {
                "name": "o1",
                "events": [
                    minimal_event("dup", artifact_id="a"),
                    minimal_event("dup", artifact_id="b"),
                ],
            }
        ]
        errors = []
        validator.check_ordering_duplicates("case-x", orderings, errors)
        self.assertTrue(any("not a real duplicate" in e for e in errors))

    def test_accepts_a_byte_identical_resend(self):
        orderings = [
            {
                "name": "o1",
                "events": [
                    minimal_event("dup", artifact_id="a", seq=1),
                    minimal_event("dup", artifact_id="a", seq=1),
                ],
            }
        ]
        errors = []
        validator.check_ordering_duplicates("case-x", orderings, errors)
        self.assertEqual(errors, [])


class RealFileEndToEndTest(unittest.TestCase):
    def test_the_real_cases_yaml_passes_every_check(self):
        import yaml

        real_path = Path(__file__).resolve().parents[2] / "docs" / "design" / "issue-36-t0-cases.yaml"
        with open(real_path, encoding="utf-8") as f:
            doc = yaml.safe_load(f)
        errors = validator.validate(doc)
        self.assertEqual(errors, [], "\n".join(errors))


if __name__ == "__main__":
    unittest.main()
