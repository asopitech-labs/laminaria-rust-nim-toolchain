"""Network-free integration tests using an actual local Git repository."""

import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / "reference_projects.py"
spec = importlib.util.spec_from_file_location("reference_projects", SCRIPT)
refs = importlib.util.module_from_spec(spec)
spec.loader.exec_module(refs)


class ReferenceSetupTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="reference-setup-test-")
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.origin = self.base / "upstream"
        self.origin.mkdir()
        self.git(self.origin, "init", "--template=")
        self.git(self.origin, "config", "user.name", "Reference Test")
        self.git(self.origin, "config", "user.email", "reference-test@example.invalid")
        self.git(self.origin, "config", "core.autocrlf", "false")
        self.git(self.origin, "config", "commit.gpgsign", "false")
        (self.origin / "source.txt").write_text("pinned\n", encoding="utf-8")
        self.git(self.origin, "add", "source.txt")
        self.git(self.origin, "commit", "-m", "pinned")
        self.revision = self.git(self.origin, "rev-parse", "HEAD")
        (self.origin / "source.txt").write_text("moving upstream\n", encoding="utf-8")
        self.git(self.origin, "commit", "-am", "new tip")
        self.project = {"name": "sample", "url": self.origin.as_uri(), "revision": self.revision}
        self.lock = self.base / "lock.json"
        self.write_lock([self.project])
        self.root = self.base / "clones with spaces"

    def git(self, directory, *args):
        return refs.git(directory, *args)

    def write_lock(self, projects):
        self.lock.write_text(json.dumps({"schema_version": 1, "submodules": "none", "projects": projects}), encoding="utf-8")

    def run_cli(self, *args):
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            return refs.main([*args, "--lock", str(self.lock), "--root", str(self.root)])

    def test_exact_old_commit_detached_and_idempotent_without_network(self):
        self.assertEqual(self.run_cli("setup"), 0)
        target = self.root / "sample"
        self.assertEqual(self.git(target, "rev-parse", "HEAD"), self.revision)
        self.assertEqual((target / "source.txt").read_text(), "pinned\n")
        with self.assertRaises(refs.SetupError):
            self.git(target, "symbolic-ref", "-q", "HEAD")
        self.assertEqual(self.git(target, "rev-parse", "--is-shallow-repository"), "true")
        # An existing checkout should not need even its original remote to exist.
        self.origin.rename(self.base / "offline-upstream")
        self.assertEqual(self.run_cli("setup"), 0)
        self.assertEqual(self.run_cli("status"), 0)

    def test_dirty_tracked_and_untracked_files_are_preserved(self):
        self.assertEqual(self.run_cli("setup"), 0)
        source = self.root / "sample" / "source.txt"
        source.write_text("user edit\n")
        self.assertEqual(self.run_cli("setup"), 1)
        self.assertEqual(source.read_text(), "user edit\n")
        source.write_text("pinned\n")
        untracked = source.parent / "notes.txt"
        untracked.write_text("user notes\n")
        self.assertEqual(self.run_cli("setup"), 1)
        self.assertEqual(untracked.read_text(), "user notes\n")

    def test_mismatched_revision_and_origin_are_not_changed(self):
        self.assertEqual(self.run_cli("setup"), 0)
        self.project["revision"] = self.git(self.origin, "rev-parse", "HEAD")
        self.write_lock([self.project])
        self.assertEqual(self.run_cli("setup"), 1)
        self.assertEqual(self.git(self.root / "sample", "rev-parse", "HEAD"), self.revision)
        self.project["revision"] = self.revision
        self.project["url"] = (self.base / "different-origin").as_uri()
        self.write_lock([self.project])
        self.assertEqual(self.run_cli("setup"), 1)
        self.assertEqual(self.git(self.root / "sample", "remote", "get-url", "origin"), self.origin.as_uri())

    def test_failed_fetch_leaves_no_checkout_or_lock_and_can_retry(self):
        self.project["revision"] = "f" * 40
        self.write_lock([self.project])
        self.assertEqual(self.run_cli("setup"), 1)
        self.assertEqual(list(self.root.iterdir()), [])
        self.project["revision"] = self.revision
        self.write_lock([self.project])
        self.assertEqual(self.run_cli("setup"), 0)

    def test_existing_non_repository_is_preserved(self):
        target = self.root / "sample"
        target.mkdir(parents=True)
        note = target / "notes.txt"
        note.write_text("keep")
        self.assertEqual(self.run_cli("setup"), 1)
        self.assertEqual(note.read_text(), "keep")

    def test_symlink_is_not_followed(self):
        self.root.mkdir()
        try:
            (self.root / "sample").symlink_to(self.origin, target_is_directory=True)
        except OSError:
            self.skipTest("host does not permit unprivileged symlinks")
        self.assertEqual(self.run_cli("setup"), 1)
        self.assertEqual((self.origin / "source.txt").read_text(), "moving upstream\n")

    def test_lock_conflict_does_not_touch_existing_lock(self):
        self.root.mkdir()
        lock = self.root / ".setup-sample.lock"
        lock.write_text("other setup")
        self.assertEqual(self.run_cli("setup"), 1)
        self.assertEqual(lock.read_text(), "other setup")
        self.assertFalse((self.root / "sample").exists())

    def test_case_insensitive_volume_rejected_before_fetch(self):
        self.project["requires_case_sensitive_fs"] = True
        self.write_lock([self.project])
        with patch.object(refs, "case_sensitive_fs", return_value=False):
            self.assertEqual(self.run_cli("setup"), 1)
        self.assertEqual(list(self.root.iterdir()), [])

    def test_selection_exclusion_and_unknown_name(self):
        other = dict(self.project, name="other")
        self.write_lock([self.project, other])
        self.assertEqual(self.run_cli("setup", "--exclude", "other"), 0)
        self.assertFalse((self.root / "other").exists())
        self.assertEqual(self.run_cli("setup", "missing"), 1)
        self.assertEqual(self.run_cli("setup", "other"), 0)

    def test_status_and_list_never_create_root(self):
        self.assertEqual(self.run_cli("list"), 0)
        self.assertEqual(self.run_cli("status"), 1)
        self.assertFalse(self.root.exists())

    def test_invalid_lock_fails_before_filesystem_changes(self):
        for updates in ({"name": "../escape"}, {"revision": "main"},
                        {"url": "https://user:secret@example.org/repo.git"}):
            self.write_lock([dict(self.project, **updates)])
            self.assertEqual(self.run_cli("setup"), 1)
            self.assertFalse(self.root.exists())
        self.write_lock([self.project, dict(self.project, name="SAMPLE")])
        self.assertEqual(self.run_cli("setup"), 1)
        self.lock.write_text("[]")
        self.assertEqual(self.run_cli("setup"), 1)

    def test_checked_in_lock_contains_thirteen_pinned_projects(self):
        projects = refs.load_lock(refs.DEFAULT_LOCK)
        self.assertEqual(len(projects), 13)
        self.assertEqual([p["name"] for p in projects if p.get("requires_case_sensitive_fs")], ["linux"])


if __name__ == "__main__":
    unittest.main()
