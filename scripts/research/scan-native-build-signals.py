#!/usr/bin/env python3
"""Issue #63 P0: hypothesis A (existing-metadata scan) reproduction on projects other than alopexDB.

Reproduces the alopexDB measurement from issue #62 P1/P2 (recorded in
docs/02-research-areas/toolchains/dependency-resolved-artifact-closure_ja.md): scan each
registry-sourced package's Cargo.toml -- already fetched during dependency resolution -- for the
`links` field and for `cc`/`cmake`/`pkg-config` build-dependencies, and treat a hit as a predicted
native-build-cost obligation. Report predicted set, ground-truth set (hand-verified against each
package's build.rs), and the resulting false-negative / false-positive rate.

Not a general tool: ground truth here is a hand-verified list per project, kept in this file,
because issue #63 P0 explicitly scopes out "implementing our own cost prediction model" -- this
script exists only to falsify or support hypothesis A's reproducibility, not to become one.

Usage:
    # one-time per project: materialize every Cargo.lock entry's manifest (cargo's own `cargo
    # fetch` only downloads compressed .crate archives; Cargo.toml is extracted to the registry
    # src/ cache lazily, only for packages that actually enter a build graph being compiled --
    # `cargo vendor` extracts every locked package deterministically without building anything)
    cd .reference/cargo && cargo vendor --versioned-dirs /path/to/vendor-cargo
    cd .reference/rust  && cargo vendor --versioned-dirs /path/to/vendor-rust

    python3 scripts/research/scan-native-build-signals.py \
        --project cargo:.reference/cargo/Cargo.lock:/path/to/vendor-cargo \
        --project rust:.reference/rust/Cargo.lock:/path/to/vendor-rust
"""
from __future__ import annotations

import argparse
import json
import re
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path

# Ground truth: registry packages whose build.rs is manually confirmed (by reading the file in
# the vendored source) to invoke a native C/C++ toolchain (cc-rs compiling vendored C sources, or
# cmake, or pkg-config probing a system library) as of the commit each project is pinned to (see
# reference-projects.lock.json). This is the oracle P0 compares the scan against.
GROUND_TRUTH: dict[str, set[str]] = {
    "cargo": {
        "curl-sys",
        "libgit2-sys",
        "libnghttp2-sys",
        "libsqlite3-sys",
        "libssh2-sys",
        "libz-sys",
        "openssl-sys",
        "ring",
        "sqlite-wasm-rs",
    },
    "rust": {
        "libz-sys",
        "libgit2-sys",
        "curl-sys",
        "lzma-sys",
        "tikv-jemalloc-sys",
        "capstone-sys",
        "openssl-sys",
        "cxx",
        "link-cplusplus",
    },
}


@dataclass
class ScanResult:
    project: str
    total_packages: int
    scanned: int
    not_vendored: list = field(default_factory=list)
    predicted_positive: set = field(default_factory=set)
    scan_seconds: float = 0.0


_PACKAGE_BLOCK_RE = re.compile(
    r"\[\[package\]\]\n"
    r'name = "(?P<name>[^"]+)"\n'
    r'version = "(?P<version>[^"]+)"\n'
    r'(?:source = "(?P<source>[^"]+)"\n)?',
)


def parse_lockfile_packages(lock_path: Path):
    text = lock_path.read_text()
    out = []
    for m in _PACKAGE_BLOCK_RE.finditer(text):
        source = m.group("source") or ""
        if source.startswith("registry+"):
            out.append((m.group("name"), m.group("version")))
    return out


def scan_project(name: str, lock_path: Path, vendor_dir: Path) -> ScanResult:
    packages = parse_lockfile_packages(lock_path)
    result = ScanResult(project=name, total_packages=len(packages), scanned=0)

    start = time.perf_counter()
    for pkg_name, version in packages:
        # cargo vendor --versioned-dirs always suffixes the directory with the exact lockfile
        # version string (including any "+" build-metadata suffix such as "+curl-8.21.0").
        crate_dir = vendor_dir / f"{pkg_name}-{version}"
        manifest = crate_dir / "Cargo.toml"
        if not manifest.is_file():
            result.not_vendored.append(f"{pkg_name}-{version}")
            continue
        result.scanned += 1
        text = manifest.read_text(errors="replace")
        # `cargo publish`/`cargo vendor` both normalize a manifest's dependency tables to one
        # `[build-dependencies.<name>]` header per dependency rather than a single
        # `[build-dependencies]` table with `<name> = "..."` entries -- matching only the inline
        # form misses every such crate. Both forms must be checked.
        hit = bool(
            re.search(r"(?m)^\s*links\s*=", text)
            or re.search(r"(?m)^\s*(cc|cmake|pkg-config)\s*=", text)
            or re.search(r"(?mi)^\[build-dependencies\.(cc|cmake|pkg-config)\]", text)
        )
        if hit:
            result.predicted_positive.add(pkg_name)
    result.scan_seconds = time.perf_counter() - start
    return result


def evaluate(result: ScanResult, ground_truth: set) -> dict:
    predicted = result.predicted_positive
    # A ground-truth entry that never got vendored can't be fairly scored either way; report it
    # separately instead of silently folding it into the false-negative count.
    missing = sorted(g for g in ground_truth if any(nv.startswith(g + "-") for nv in result.not_vendored))
    comparable_truth = ground_truth - set(missing)

    false_negatives = sorted(comparable_truth - predicted)
    false_positives = sorted(predicted - ground_truth)
    true_positives = sorted(comparable_truth & predicted)

    fn_rate = len(false_negatives) / len(comparable_truth) if comparable_truth else None
    fp_rate = len(false_positives) / len(predicted) if predicted else 0.0

    return {
        "project": result.project,
        "total_registry_packages": result.total_packages,
        "scanned_manifests": result.scanned,
        "not_vendored_count": len(result.not_vendored),
        "scan_seconds": round(result.scan_seconds, 4),
        "ground_truth_count": len(ground_truth),
        "ground_truth_missing_from_vendor": missing,
        "comparable_ground_truth_count": len(comparable_truth),
        "predicted_positive_count": len(predicted),
        "true_positives": true_positives,
        "false_negatives": false_negatives,
        "false_positives": false_positives,
        "false_negative_rate": fn_rate,
        "false_positive_rate": fp_rate,
    }


def parse_project_arg(spec: str):
    parts = spec.split(":")
    if len(parts) != 3:
        raise argparse.ArgumentTypeError(
            f"expected name:lockfile_path:vendor_dir, got {spec!r}"
        )
    name, lock_path, vendor_dir = parts
    if name not in GROUND_TRUTH:
        raise argparse.ArgumentTypeError(
            f"no ground truth registered for project {name!r}; add it to GROUND_TRUTH first"
        )
    return name, Path(lock_path), Path(vendor_dir)


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "--project",
        type=parse_project_arg,
        action="append",
        dest="projects",
        required=True,
        help="name:path/to/Cargo.lock:path/to/vendor_dir (repeatable)",
    )
    parser.add_argument("--json", action="store_true", help="emit JSON instead of a text report")
    args = parser.parse_args()

    reports = []
    for name, lock_path, vendor_dir in args.projects:
        if not lock_path.is_file():
            print(f"skip {name}: {lock_path} not found", file=sys.stderr)
            continue
        if not vendor_dir.is_dir():
            print(f"skip {name}: vendor dir {vendor_dir} not found (run `cargo vendor` first)", file=sys.stderr)
            continue
        result = scan_project(name, lock_path, vendor_dir)
        reports.append(evaluate(result, GROUND_TRUTH[name]))

    if args.json:
        print(json.dumps(reports, indent=2))
    else:
        for r in reports:
            print(f"== {r['project']} ==")
            print(f"  registry packages (Cargo.lock): {r['total_registry_packages']}")
            print(f"  scanned manifests (vendored):     {r['scanned_manifests']}")
            print(f"  not vendored (excluded from eval): {r['not_vendored_count']}")
            print(f"  scan time:                        {r['scan_seconds']}s")
            print(
                f"  ground truth (native build cost): {r['ground_truth_count']}"
                f" (comparable: {r['comparable_ground_truth_count']},"
                f" missing-from-vendor: {r['ground_truth_missing_from_vendor']})"
            )
            print(f"  predicted positive:               {r['predicted_positive_count']}")
            print(f"  true positives:                   {r['true_positives']}")
            print(f"  false negatives:                  {r['false_negatives']} (rate={r['false_negative_rate']})")
            print(f"  false positives:                  {r['false_positives']} (rate={r['false_positive_rate']})")
            print()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
