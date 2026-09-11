#!/usr/bin/env python3
"""Resolve the exact Linux, native-only nlvm asset from the
arnetheduck/nlvm 'continuous' GitHub release (Python 3.9+, no
third-party dependencies).

nlvm is a reference/exploratory Nim-to-LLVM backend used by this repo's
own `nlvm-experiment` CI job to observe an alternative compilation route
-- it is never LAMINARIA's owned target-generation path (see
docs/compiler-ownership-contract.md). This script exists only to fetch
that external reference binary reliably; it makes no claim about, and
has no effect on, LAMINARIA's own compiler ownership.

nlvm's own release script (make-dist-linux.sh, in the nlvm repo) produces
exactly two Linux assets per continuous build:
  nlvm-linux-<short-sha>-native.tar.xz  -- native Linux target only, no
    cross-compiler runtime (no mingw/ Windows target, no wasm32-wasip1
    lib). This is the one this CI job needs: it only compiles a trivial
    *native* hello.nim, never cross-compiles.
  nlvm-linux-<short-sha>.tar.xz         -- the above PLUS the full
    cross-compiler runtime (Windows + wasm32 targets). Larger, and
    exercises capabilities this job never uses.
Both contain a working native nlvm binary, so silently picking either
would not be a correctness bug on its own -- the point of this script is
to make the choice explicit and verified, not incidental (a prior
version of the CI step hardcoded one specific release's asset name
outright, which breaks the moment nlvm cuts a new continuous build).
"""

import argparse
import json
import re
import sys
import urllib.request

RELEASE_API_URL = "https://api.github.com/repos/arnetheduck/nlvm/releases/tags/continuous"

# Matches exactly the "-native" Linux asset described above -- never the
# plain nlvm-linux-<sha>.tar.xz (with cross-compiler runtime), and never
# the Windows .zip or the architecture-only nlvm-x86_64.AppImage.
ASSET_NAME_RE = re.compile(r"^nlvm-linux-[0-9a-f]+-native\.tar\.xz$")


class AssetResolutionError(Exception):
    pass


def select_linux_native_asset(assets):
    """`assets`: the `assets` list from the GitHub release API response
    for the 'continuous' release. Returns the single matching asset dict.
    Raises AssetResolutionError if the candidate set is not exactly one,
    or if the match has no usable sha256 digest -- never proceeds on an
    ambiguous or unverifiable result."""
    candidates = [a for a in assets if ASSET_NAME_RE.fullmatch(a.get("name", ""))]
    if len(candidates) == 0:
        raise AssetResolutionError(
            "no Linux native nlvm asset found matching " + ASSET_NAME_RE.pattern
        )
    if len(candidates) > 1:
        names = ", ".join(sorted(c.get("name", "<unnamed>") for c in candidates))
        raise AssetResolutionError(
            str(len(candidates)) + " candidates matched " + ASSET_NAME_RE.pattern
            + " (expected exactly 1), refusing to guess: " + names
        )
    asset = candidates[0]
    digest = asset.get("digest", "")
    if not isinstance(digest, str) or not digest.startswith("sha256:") or len(digest) != len("sha256:") + 64:
        raise AssetResolutionError(
            asset.get("name", "<unnamed>") + ": no usable sha256 digest in the release API response "
            "(digest=" + repr(digest) + ")"
        )
    url = asset.get("browser_download_url", "")
    if not isinstance(url, str) or not url.startswith(
        "https://github.com/arnetheduck/nlvm/releases/download/"
    ):
        raise AssetResolutionError(
            asset.get("name", "<unnamed>") + ": browser_download_url is missing or unexpected: "
            + repr(url)
        )
    return asset


def fetch_release_assets(url=RELEASE_API_URL, timeout=30):
    request = urllib.request.Request(
        url,
        headers={
            "Accept": "application/vnd.github+json",
            "User-Agent": "laminaria-ci-nlvm-resolver",
        },
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:  # noqa: S310 (fixed https host)
        payload = json.load(response)
    assets = payload.get("assets")
    if not isinstance(assets, list):
        raise AssetResolutionError("release API response has no 'assets' list")
    return assets


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--release-json",
        type=argparse.FileType("r"),
        default=None,
        help="path to a saved release-API JSON response, or '-' for stdin "
        "(default: fetch the live 'continuous' release from GitHub)",
    )
    args = parser.parse_args(argv)
    try:
        if args.release_json is not None:
            payload = json.load(args.release_json)
            assets = payload.get("assets")
            if not isinstance(assets, list):
                raise AssetResolutionError("release JSON has no 'assets' list")
        else:
            assets = fetch_release_assets()
        asset = select_linux_native_asset(assets)
    except AssetResolutionError as error:
        print("ERROR: " + str(error), file=sys.stderr)
        return 1
    except (OSError, ValueError) as error:
        print("ERROR: failed to fetch/parse the release API response: " + str(error), file=sys.stderr)
        return 1

    sha256 = asset["digest"].split(":", 1)[1]
    print("NLVM_ASSET_NAME=" + asset["name"])
    print("NLVM_ASSET_URL=" + asset["browser_download_url"])
    print("NLVM_ASSET_SHA256=" + sha256)
    return 0


if __name__ == "__main__":
    sys.exit(main())
