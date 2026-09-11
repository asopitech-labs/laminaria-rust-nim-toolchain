"""Unit tests for scripts/resolve_nlvm_asset.py's asset-selection logic.
Network-free: exercises select_linux_native_asset()/main() against saved
JSON fixtures, never the live GitHub API."""

import contextlib
import importlib.util
import io
from pathlib import Path
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "resolve_nlvm_asset.py"
spec = importlib.util.spec_from_file_location("resolve_nlvm_asset", SCRIPT)
resolver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(resolver)


def asset(name, digest="sha256:" + "ab" * 32, url_suffix=None):
    return {
        "name": name,
        "digest": digest,
        "browser_download_url": (
            "https://github.com/arnetheduck/nlvm/releases/download/continuous/"
            + (url_suffix or name)
        ),
    }


class SelectLinuxNativeAssetTests(unittest.TestCase):
    def test_selects_the_single_native_linux_asset_among_the_real_shape(self):
        # The exact 4-asset shape nlvm's own continuous release actually
        # produces (verified live against the GitHub API before writing
        # this test): two Linux assets (native + full), one Windows zip,
        # one bare AppImage.
        assets = [
            asset("nlvm-linux-eac5d1e-native.tar.xz"),
            asset("nlvm-linux-eac5d1e.tar.xz"),
            asset("nlvm-windows-eac5d1e.zip"),
            asset("nlvm-x86_64.AppImage", digest=""),
        ]
        chosen = resolver.select_linux_native_asset(assets)
        self.assertEqual(chosen["name"], "nlvm-linux-eac5d1e-native.tar.xz")

    def test_never_confuses_the_native_asset_with_the_full_cross_compiler_asset(self):
        assets = [asset("nlvm-linux-deadbee.tar.xz")]  # only the non-native variant present
        with self.assertRaises(resolver.AssetResolutionError):
            resolver.select_linux_native_asset(assets)

    def test_zero_candidates_is_an_error_not_a_silent_skip(self):
        assets = [asset("nlvm-windows-eac5d1e.zip"), asset("nlvm-x86_64.AppImage", digest="")]
        with self.assertRaises(resolver.AssetResolutionError) as ctx:
            resolver.select_linux_native_asset(assets)
        self.assertIn("no Linux native nlvm asset found", str(ctx.exception))

    def test_multiple_candidates_is_an_ambiguity_error_not_a_guess(self):
        # A hypothetical release accidentally carrying two native Linux
        # builds (e.g. a re-run under a different short-sha) must never
        # be resolved by picking "the first one".
        assets = [
            asset("nlvm-linux-eac5d1e-native.tar.xz"),
            asset("nlvm-linux-0123abc-native.tar.xz"),
        ]
        with self.assertRaises(resolver.AssetResolutionError) as ctx:
            resolver.select_linux_native_asset(assets)
        self.assertIn("2 candidates matched", str(ctx.exception))

    def test_a_missing_or_malformed_digest_is_rejected_even_with_a_unique_name_match(self):
        assets = [asset("nlvm-linux-eac5d1e-native.tar.xz", digest="")]
        with self.assertRaises(resolver.AssetResolutionError) as ctx:
            resolver.select_linux_native_asset(assets)
        self.assertIn("no usable sha256 digest", str(ctx.exception))

    def test_an_unexpected_download_host_is_rejected(self):
        assets = [
            {
                "name": "nlvm-linux-eac5d1e-native.tar.xz",
                "digest": "sha256:" + "ab" * 32,
                "browser_download_url": "https://evil.example/nlvm-linux-eac5d1e-native.tar.xz",
            }
        ]
        with self.assertRaises(resolver.AssetResolutionError) as ctx:
            resolver.select_linux_native_asset(assets)
        self.assertIn("browser_download_url", str(ctx.exception))


class MainTests(unittest.TestCase):
    def run_main(self, release_json_path):
        stdout = io.StringIO()
        stderr = io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            exit_code = resolver.main(["--release-json", str(release_json_path)])
        return exit_code, stdout.getvalue(), stderr.getvalue()

    def write_release_json(self, tmp_path, assets):
        import json

        path = tmp_path / "release.json"
        path.write_text(json.dumps({"assets": assets}), encoding="utf-8")
        return path

    def test_prints_key_value_lines_for_a_clean_resolution(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            assets = [
                asset("nlvm-linux-eac5d1e-native.tar.xz", digest="sha256:" + "cd" * 32),
                asset("nlvm-linux-eac5d1e.tar.xz"),
            ]
            path = self.write_release_json(tmp_path, assets)
            exit_code, out, _err = self.run_main(path)
            self.assertEqual(exit_code, 0)
            self.assertIn("NLVM_ASSET_NAME=nlvm-linux-eac5d1e-native.tar.xz", out)
            self.assertIn(
                "NLVM_ASSET_URL=https://github.com/arnetheduck/nlvm/releases/download/continuous/"
                "nlvm-linux-eac5d1e-native.tar.xz",
                out,
            )
            self.assertIn("NLVM_ASSET_SHA256=" + "cd" * 32, out)

    def test_exits_non_zero_and_prints_to_stderr_on_zero_candidates(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            path = self.write_release_json(tmp_path, [asset("nlvm-linux-eac5d1e.tar.xz")])
            exit_code, out, err = self.run_main(path)
            self.assertEqual(exit_code, 1)
            self.assertEqual(out, "")
            self.assertIn("ERROR", err)

    def test_exits_non_zero_on_multiple_candidates(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            assets = [
                asset("nlvm-linux-aaaaaaa-native.tar.xz"),
                asset("nlvm-linux-bbbbbbb-native.tar.xz"),
            ]
            path = self.write_release_json(tmp_path, assets)
            exit_code, out, err = self.run_main(path)
            self.assertEqual(exit_code, 1)
            self.assertEqual(out, "")
            self.assertIn("ERROR", err)

    def test_malformed_release_json_without_an_assets_list_is_an_error(self):
        import json
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            path = tmp_path / "release.json"
            path.write_text(json.dumps({"not_assets": []}), encoding="utf-8")
            exit_code, out, err = self.run_main(path)
            self.assertEqual(exit_code, 1)
            self.assertEqual(out, "")
            self.assertIn("ERROR", err)


if __name__ == "__main__":
    unittest.main()
