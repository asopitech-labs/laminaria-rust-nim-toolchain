#!/usr/bin/env python3
"""Reproduce issue #43's pinned Hike micro-Wasm reference path."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile


HIKE_REVISION = "6402155652fa61692818fe7985193f0fd97513f5"
GENERATED_FILES = (
    "HIKE-LICENSE",
    "bridge-contract.txt",
    "execution.stderr.txt",
    "execution.stdout.txt",
    "index.html",
    "main.hike",
    "main.compat.ll",
    "main.generated.ll",
    "main.wasm",
    "main.wasm.gz",
    "main.wasm.zst",
    "node_harness.js",
    "report.json",
    "runtime.js",
    "upstream-build.stderr.txt",
    "upstream-build.stdout.txt",
    "wasm-objdump.headers.txt",
    "wasm-objdump.txt",
    "wasm.wat",
)
LEGACY_FILES = (
    "app.wasm",
    "app.wasm.gz",
    "app.wasm.zst",
    "compatible.stderr.txt",
    "compatible.stdout.txt",
    "main.ll",
    "run_wasm.js",
    "run_wasm_compatible.js",
    "upstream.stderr.txt",
    "upstream.stdout.txt",
    "upstream.wasm",
)


def run(
    argv: list[str],
    *,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        argv,
        cwd=cwd,
        env=env,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if result.returncode != 0:
        rendered = " ".join(argv)
        raise RuntimeError(
            f"command failed ({result.returncode}): {rendered}\n"
            f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )
    return result


def first_line(argv: list[str]) -> str:
    result = run(argv)
    combined = result.stdout.strip() or result.stderr.strip()
    return combined.splitlines()[0]


def normalize_output(value: str) -> str:
    return re.sub(r"/tmp/hike_build_[0-9]+\.ll", "<temp>.ll", value)


def run_allow_failure(
    argv: list[str], *, cwd: Path, env: dict[str, str]
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        argv,
        cwd=cwd,
        env=env,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def extract_block(text: str, heading: str) -> list[str]:
    lines = text.splitlines()
    collected: list[str] = []
    active = False
    for line in lines:
        if line.startswith(f"{heading}["):
            active = True
            collected.append(line)
            continue
        if active and line and not line.startswith((" ", "-")):
            break
        if active:
            collected.append(line)
    return collected


def extract_llvm_definition(ir: str, symbol: str) -> str:
    lines = ir.splitlines()
    start = next(
        (index for index, line in enumerate(lines) if line.startswith("define ") and f"@{symbol}(" in line),
        None,
    )
    if start is None:
        raise RuntimeError(f"LLVM definition not found: {symbol}")
    for end in range(start + 1, len(lines)):
        if lines[end] == "}":
            return "\n".join(lines[start : end + 1]) + "\n"
    raise RuntimeError(f"unterminated LLVM definition: {symbol}")


def ensure_separate_paths(source: Path, output: Path) -> None:
    if source == output or source in output.parents or output in source.parents:
        raise RuntimeError("Hike source and evidence output must not contain one another")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--hike-source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    source = args.hike_source.resolve()
    output = args.output.resolve()
    ensure_separate_paths(source, output)
    actual_revision = run(
        ["git", "-c", f"safe.directory={source}", "-C", str(source), "rev-parse", "HEAD"]
    ).stdout.strip()
    if actual_revision != HIKE_REVISION:
        raise RuntimeError(
            f"Hike revision mismatch: expected {HIKE_REVISION}, got {actual_revision}"
        )
    origin = run(
        ["git", "-c", f"safe.directory={source}", "-C", str(source), "remote", "get-url", "origin"]
    ).stdout.strip()
    if origin != "https://github.com/kanryu/hike-lang.git":
        raise RuntimeError(f"Hike origin mismatch: {origin}")
    dirty = run(
        ["git", "-c", f"safe.directory={source}", "-C", str(source), "status", "--porcelain"]
    ).stdout
    if dirty:
        raise RuntimeError("Hike checkout must be clean")

    output.mkdir(parents=True, exist_ok=True)
    for name in GENERATED_FILES + LEGACY_FILES:
        candidate = output / name
        if candidate.exists():
            candidate.unlink()

    upstream_example = source / "examples" / "browser"
    shutil.copyfile(source / "LICENSE", output / "HIKE-LICENSE")
    shutil.copyfile(upstream_example / "main.hike", output / "main.hike")
    shutil.copyfile(upstream_example / "index.html", output / "index.html")
    shutil.copyfile(
        Path(__file__).with_name("hike_r0_node_harness.js"),
        output / "node_harness.js",
    )

    stable_env = os.environ.copy()
    stable_env.update({"LC_ALL": "C.UTF-8", "LANG": "C.UTF-8", "TZ": "UTC"})
    with tempfile.TemporaryDirectory(prefix="hike-r0-") as temp:
        hikec = Path(temp) / "hikec"
        run(
            [
                "go",
                "build",
                "-buildvcs=false",
                "-trimpath",
                "-o",
                str(hikec),
                "./cmd/hikec",
            ],
            cwd=source,
            env=stable_env,
        )
        run(
            [
                str(hikec),
                "-target",
                "wasm32",
                "-o",
                str(output / "main.generated.ll"),
                str(output / "main.hike"),
            ],
            cwd=source,
            env=stable_env,
        )

        upstream_build = run_allow_failure(
            [
                str(hikec),
                "build",
                "-target",
                "wasm32",
                "main.hike",
                "-o",
                "upstream.wasm",
            ],
            cwd=output,
            env=stable_env,
        )
        upstream_stdout = normalize_output(upstream_build.stdout)
        upstream_stderr = normalize_output(upstream_build.stderr)
        (output / "upstream-build.stdout.txt").write_text(upstream_stdout, encoding="utf-8")
        (output / "upstream-build.stderr.txt").write_text(upstream_stderr, encoding="utf-8")
        if upstream_build.returncode == 0 or "undefined value '@strlen32'" not in upstream_build.stderr:
            raise RuntimeError("pinned upstream browser build failure signature changed")

        runtime_emitter = Path(temp) / "emit_runtime.go"
        runtime_emitter.write_text(
            "package main\n"
            'import ("os"; "hikec-go/pkg/compiler")\n'
            "func main() { if err := compiler.WriteWasmJSRuntime(os.Args[1]); err != nil { panic(err) } }\n",
            encoding="utf-8",
        )
        run(
            ["go", "run", "-buildvcs=false", str(runtime_emitter), str(output / "runtime.js")],
            cwd=source,
            env=stable_env,
        )

    index_html = (output / "index.html").read_text(encoding="utf-8")
    runtime_js = (output / "runtime.js").read_text(encoding="utf-8")
    if (
        "new HikeRuntime()" not in index_html
        or "window.HikeConcurrentRuntime" not in runtime_js
        or "window.HikeRuntime" in runtime_js
    ):
        raise RuntimeError("pinned generated bridge mismatch signature changed")
    (output / "bridge-contract.txt").write_text(
        "index constructor: HikeRuntime\n"
        "generated runtime export: HikeConcurrentRuntime\n"
        "compatible: false\n"
        "Node evidence path: direct WebAssembly API harness (generated bridge bypassed)\n",
        encoding="utf-8",
    )

    generated_ir = (output / "main.generated.ll").read_text(encoding="utf-8")
    if "@strlen32(" not in generated_ir or "define internal i32 @strlen32(" in generated_ir:
        raise RuntimeError("pinned upstream strlen32 failure signature changed")
    default_runtime = (source / "pkg" / "backend" / "llvm" / "runtime" / "runtime.ll").read_text(
        encoding="utf-8"
    )
    compatibility_definition = extract_llvm_definition(default_runtime, "strlen32")
    (output / "main.compat.ll").write_text(
        generated_ir + "\n; R0 compatibility restoration from pinned runtime.ll\n" + compatibility_definition,
        encoding="utf-8",
    )
    link_command = [
        "clang",
        "--target=wasm32-unknown-unknown",
        "-O2",
        "-nostdlib",
        "-Wl,--no-entry",
        "-Wl,--export-all",
        "-Wl,--allow-undefined",
        "main.compat.ll",
        "-o",
        "main.wasm",
    ]
    run(link_command, cwd=output, env=stable_env)

    run(["wasm-validate", "main.wasm"], cwd=output, env=stable_env)
    headers = run(["wasm-objdump", "-h", "main.wasm"], cwd=output, env=stable_env)
    details = run(["wasm-objdump", "-x", "main.wasm"], cwd=output, env=stable_env)
    wat = run(["wasm2wat", "--generate-names", "main.wasm"], cwd=output, env=stable_env)
    (output / "wasm-objdump.headers.txt").write_text(headers.stdout, encoding="utf-8")
    (output / "wasm-objdump.txt").write_text(details.stdout, encoding="utf-8")
    (output / "wasm.wat").write_text(wat.stdout, encoding="utf-8")

    execution = run_allow_failure(
        ["node", "node_harness.js"], cwd=output, env=stable_env
    )
    (output / "execution.stdout.txt").write_text(execution.stdout, encoding="utf-8")
    (output / "execution.stderr.txt").write_text(execution.stderr, encoding="utf-8")
    if execution.returncode != 0:
        raise RuntimeError("Node execution failed: " + execution.stderr)

    wasm_bytes = (output / "main.wasm").read_bytes()
    (output / "main.wasm.gz").write_bytes(gzip.compress(wasm_bytes, compresslevel=9, mtime=0))
    run(
        ["zstd", "-19", "--force", "--no-progress", "main.wasm", "-o", "main.wasm.zst"],
        cwd=output,
        env=stable_env,
    )

    artifacts: dict[str, dict[str, int | str]] = {}
    for name in GENERATED_FILES:
        path = output / name
        if path.exists() and name != "report.json":
            artifacts[name] = {"bytes": path.stat().st_size, "sha256": sha256(path)}

    report = {
        "schema_version": 1,
        "reference": {
            "repository": "https://github.com/kanryu/hike-lang.git",
            "revision": actual_revision,
            "workload": "examples/browser",
        },
        "commands": {
            "compiler": "go build -buildvcs=false -trimpath -o <temp>/hikec ./cmd/hikec",
            "emit_ir": "<temp>/hikec -target wasm32 -o <output>/main.generated.ll <output>/main.hike",
            "upstream_build": "<temp>/hikec build -target wasm32 main.hike -o upstream.wasm",
            "compatibility_transform": "append pinned runtime.ll's strlen32 definition to main.generated.ll",
            "link_compatible": " ".join(link_command),
            "execute": "node node_harness.js",
        },
        "toolchain": {
            "container_base": "golang:1.22.12-bookworm@sha256:3d699e4d15d0f8f13c9195c0632a16702b8cbdece2955af1c23b37ae5d55a253",
            "debian": first_line(["sh", "-c", ". /etc/os-release && printf '%s\\n' \"$PRETTY_NAME\""]),
            "packages": sorted(run(
                [
                    "dpkg-query",
                    "-W",
                    "-f=${Package}=${Version}\\n",
                    "binaryen",
                    "clang",
                    "lld",
                    "nodejs",
                    "wabt",
                    "zstd",
                ]
            ).stdout.splitlines()),
            "git": first_line(["git", "--version"]),
            "go": first_line(["go", "version"]),
            "clang": first_line(["clang", "--version"]),
            "lld": first_line(["wasm-ld", "--version"]),
            "node": first_line(["node", "--version"]),
            "wabt": first_line(["wasm-objdump", "--version"]),
            "binaryen": first_line(["wasm-opt", "--version"]),
            "zstd": first_line(["zstd", "--version"]),
        },
        "size_comparison": {
            "article_claim": "2.56 KB (unit convention unspecified)",
            "reproduced_wasm_bytes": (output / "main.wasm").stat().st_size,
            "reproduced_runtime_js_bytes": (output / "runtime.js").stat().st_size,
            "reproduced_wasm_plus_runtime_js_bytes": (output / "main.wasm").stat().st_size
            + (output / "runtime.js").stat().st_size,
            "difference_producers": [
                "the fixed revision's examples/browser source is not byte-identical to the article excerpt",
                "the article does not publish its exact Go/Clang/LLD versions or linker artifact",
                "the fixed revision's wasm32 runtime omits strlen32, so its one-command build fails",
                "the compatibility artifact restores only strlen32 from the same revision's runtime.ll",
                "the fixed revision emits a newer unified runtime.js than the bridge shown in the article",
            ],
        },
        "bridge_contract": {
            "index_constructor": "HikeRuntime",
            "generated_runtime_export": "HikeConcurrentRuntime",
            "compatible": False,
            "node_execution_path": "direct WebAssembly API harness; generated runtime.js bypassed",
        },
        "wasm": {
            "imports": extract_block(details.stdout, "Import"),
            "exports": extract_block(details.stdout, "Export"),
            "code_symbols": extract_block(details.stdout, "Code"),
            "sections": headers.stdout.splitlines(),
        },
        "execution": {
            "exit_code": execution.returncode,
            "stdout": execution.stdout.splitlines(),
            "stderr": execution.stderr.splitlines(),
        },
        "upstream_build": {
            "exit_code": upstream_build.returncode,
            "stdout": upstream_stdout.splitlines(),
            "stderr": upstream_stderr.splitlines(),
        },
        "artifacts": artifacts,
    }
    (output / "report.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
