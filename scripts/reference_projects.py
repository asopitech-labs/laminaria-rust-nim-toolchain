#!/usr/bin/env python3
"""Reproduce pinned, source-only prior-art checkouts (Python 3.11+, Git).

Existing directories are verified, never reset, cleaned, pulled or overwritten.
No upstream build/install scripts, hooks, or submodule updates are executed.
"""

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
from urllib.parse import urlsplit


REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_LOCK = REPO_ROOT / "reference-projects.lock.json"


class SetupError(Exception):
    pass


def load_lock(path):
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict) or type(data.get("schema_version")) is not int:
        raise SetupError("lock must be an object with an integer schema_version")
    if data.get("schema_version") != 1 or data.get("submodules") != "none":
        raise SetupError("unsupported lock schema or submodule policy")
    projects = data.get("projects")
    if not isinstance(projects, list) or not projects:
        raise SetupError("projects must be a nonempty array")
    seen = set()
    for project in projects:
        if not isinstance(project, dict):
            raise SetupError("each project must be an object")
        name = project.get("name", "")
        if not isinstance(name, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]*", name):
            raise SetupError("project name must be one safe directory component")
        if name.casefold() in seen:
            raise SetupError("duplicate project name (case insensitive): " + name)
        seen.add(name.casefold())
        revision = project.get("revision", "")
        if not isinstance(revision, str) or not re.fullmatch(r"[0-9a-f]{40}", revision):
            raise SetupError(name + ": revision must be a full lowercase commit SHA")
        url = project.get("url", "")
        if not isinstance(url, str):
            raise SetupError(name + ": URL must be a string")
        parsed = urlsplit(url)
        # file:// supports explicit local mirrors and network-free integration tests.
        valid = (parsed.scheme == "https" and parsed.hostname and parsed.path.endswith(".git")) or (
            parsed.scheme == "file" and not parsed.netloc and parsed.path.startswith("/")
        )
        if (not valid or parsed.username or parsed.password or parsed.query or parsed.fragment
                or any(c.isspace() for c in url)):
            raise SetupError(name + ": use a credential-free HTTPS .git URL or absolute file:// URL")
        if type(project.get("requires_case_sensitive_fs", False)) is not bool:
            raise SetupError(name + ": requires_case_sensitive_fs must be boolean")
    return projects


def git(directory, *args):
    env = os.environ.copy()
    # Avoid modifying indexes while inspecting pre-existing clones. Ignore a
    # caller's repository/worktree override; all targets are explicit -C paths.
    for key in ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR"):
        env.pop(key, None)
    env["GIT_OPTIONAL_LOCKS"] = "0"
    env["GIT_TERMINAL_PROMPT"] = "0"
    result = subprocess.run(
        ["git", "-c", "core.hooksPath=" + os.devnull, "-C", str(directory), *args],
        env=env, text=True, encoding="utf-8", errors="replace",
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )
    if result.returncode:
        raise SetupError(result.stderr.strip() or "git command failed: " + " ".join(args))
    return result.stdout.strip()


def check_checkout(directory, project):
    if directory.is_symlink():
        raise SetupError("refusing symlink checkout: " + str(directory))
    if not directory.is_dir() or not (directory / ".git").exists():
        raise SetupError("not an independent Git checkout: " + str(directory))
    top = Path(git(directory, "rev-parse", "--show-toplevel")).resolve()
    if top != directory.resolve():
        raise SetupError("Git checkout root differs from destination")
    origin = git(directory, "config", "--get", "remote.origin.url")
    if origin != project["url"]:
        raise SetupError("origin differs from lock; directory left unchanged")
    head = git(directory, "rev-parse", "HEAD")
    if head != project["revision"]:
        raise SetupError("HEAD " + head + " differs from lock; directory left unchanged")
    if git(directory, "status", "--porcelain", "--untracked-files=all"):
        raise SetupError("dirty/untracked files present; directory left unchanged")


def case_sensitive_fs(root):
    # Probe the actual destination volume, not an OS-name heuristic.
    with tempfile.TemporaryDirectory(prefix=".case-probe-", dir=root) as temp:
        probe = Path(temp) / "case-probe"
        probe.touch()
        return not (Path(temp) / "CASE-PROBE").exists()


def setup_checkout(root, project):
    name = project["name"]
    destination = root / name
    # An exclusive per-project lock makes concurrent setup invocations fail
    # safely instead of racing publication. Stale locks are never auto-deleted.
    lock = root / (".setup-" + name + ".lock")
    try:
        fd = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    except FileExistsError:
        raise SetupError("setup lock exists: " + str(lock) + " (another setup or interrupted run)")
    os.close(fd)
    staging = None
    try:
        if destination.exists() or destination.is_symlink():
            check_checkout(destination, project)
            return "already pinned (no changes)"
        if project.get("requires_case_sensitive_fs") and not case_sensitive_fs(root):
            raise SetupError("requires a case-sensitive volume; use --root on one, or --exclude " + name)
        staging = Path(tempfile.mkdtemp(prefix="." + name + "-setup-", dir=root))
        # init + fetch the exact commit avoids a transient checkout of a moving
        # default branch. Shallow history; complete top-level source tree.
        git(staging, "init", "--template=")
        git(staging, "config", "core.autocrlf", "false")
        git(staging, "config", "core.longpaths", "true")
        git(staging, "remote", "add", "origin", project["url"])
        git(staging, "-c", "fetch.recurseSubmodules=false", "fetch", "--no-tags", "--depth=1",
            "origin", project["revision"])
        actual = git(staging, "rev-parse", "FETCH_HEAD^{commit}")
        if actual != project["revision"]:
            raise SetupError("fetched commit differs from pin")
        git(staging, "-c", "submodule.recurse=false", "checkout", "--detach", actual)
        check_checkout(staging, project)
        if destination.exists() or destination.is_symlink():
            raise SetupError("destination appeared during setup; refusing to replace it")
        staging.rename(destination)
        staging = None
        return "created at " + actual
    finally:
        try:
            if staging is not None:
                # Only this invocation's unique, unpublished staging directory.
                shutil.rmtree(staging, onerror=remove_readonly)
        finally:
            lock.unlink()


def remove_readonly(function, path, error):
    """Git object files may be read-only on Windows; only used in our staging tree."""
    if not isinstance(error[1], PermissionError):
        raise error[1]
    os.chmod(path, 0o700)
    function(path)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("list", "status", "setup"))
    parser.add_argument("names", nargs="*", help="project names; default: all locked projects")
    parser.add_argument("--exclude", action="append", default=[], metavar="NAME")
    parser.add_argument("--root", type=Path, default=REPO_ROOT / ".reference")
    parser.add_argument("--lock", type=Path, default=DEFAULT_LOCK)
    args = parser.parse_args(argv)
    try:
        projects = load_lock(args.lock)
        known = {p["name"] for p in projects}
        unknown = (set(args.names) | set(args.exclude)) - known
        if unknown:
            raise SetupError("unknown project(s): " + ", ".join(sorted(unknown)))
        selected = [p for p in projects if (not args.names or p["name"] in args.names)
                    and p["name"] not in args.exclude]
        if not selected:
            raise SetupError("no projects selected")
        if args.command == "list":
            for project in selected:
                print(project["name"], project["revision"], project["url"],
                      "[case-sensitive FS]" if project.get("requires_case_sensitive_fs") else "")
            return 0
        root = args.root.expanduser().resolve()
        if args.command == "setup":
            root.mkdir(parents=True, exist_ok=True)
        failed = False
        for project in selected:
            name = project["name"]
            try:
                if args.command == "status":
                    check_checkout(root / name, project)
                    message = "pinned and clean"
                else:
                    print(name + ": checking/setup...", flush=True)
                    message = setup_checkout(root, project)
                print(name + ": " + message, flush=True)
            except (SetupError, OSError) as error:
                failed = True
                print(name + ": ERROR: " + str(error), file=sys.stderr, flush=True)
        return 1 if failed else 0
    except (SetupError, OSError, ValueError) as error:
        print("ERROR: " + str(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
