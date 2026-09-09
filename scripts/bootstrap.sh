#!/usr/bin/env bash
# LAMINARIA environment bootstrap/readiness check (issue #18).
#
# By default this only reports what is missing against toolchains.lock.toml
# and prints the command that would install it — it does not touch the
# system. Pass --install to actually run those commands (macOS/Homebrew and
# rustup only for now).
#
# Usage:
#   scripts/bootstrap.sh            # report only
#   scripts/bootstrap.sh --install  # report, then install what's missing

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL=0
if [[ "${1:-}" == "--install" ]]; then
  INSTALL=1
fi

missing=0
note() { echo "  - $*"; }
ok() { echo "  ok $*"; }

check() {
  local name="$1" cmd="$2" install_hint="$3"
  if command -v "$cmd" >/dev/null 2>&1; then
    ok "$name ($cmd found: $(command -v "$cmd"))"
  else
    note "$name missing: $cmd not found on PATH. Install with: $install_hint"
    missing=1
    if [[ "$INSTALL" == "1" ]]; then
      echo "    installing: $install_hint"
      eval "$install_hint"
    fi
  fi
}

echo "LAMINARIA bootstrap check (repo: $REPO_ROOT)"
echo

echo "Rust toolchain manager"
check "rustup" rustup "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
if command -v rustup >/dev/null 2>&1; then
  while IFS= read -r selector; do
    [[ -z "$selector" ]] && continue
    if rustup run "$selector" rustc -vV >/dev/null 2>&1; then
      ok "rust toolchain '$selector' resolvable via rustup"
    else
      note "rust toolchain '$selector' not installed. Install with: rustup toolchain install $selector"
      missing=1
      if [[ "$INSTALL" == "1" ]]; then
        rustup toolchain install "$selector"
      fi
    fi
  done < <(python3 - "$REPO_ROOT/toolchains.lock.toml" <<'PY'
import sys, tomllib
with open(sys.argv[1], "rb") as f:
    data = tomllib.load(f)
for entry in data.get("rust", {}).get("toolchains", {}).values():
    print(entry["selector"])
PY
)
fi
echo

echo "Nim toolchain manager"
CHOOSENIM_CMD="$(command -v choosenim || true)"
if [[ -z "$CHOOSENIM_CMD" && -x "$HOME/.nimble/bin/choosenim" ]]; then
  CHOOSENIM_CMD="$HOME/.nimble/bin/choosenim"
fi
if [[ -n "$CHOOSENIM_CMD" ]]; then
  ok "choosenim ($CHOOSENIM_CMD found)"
else
  note "choosenim missing. Install with: curl https://nim-lang.org/choosenim/init.sh -sSf | sh -s -- -y"
  missing=1
  if [[ "$INSTALL" == "1" ]]; then
    curl https://nim-lang.org/choosenim/init.sh -sSf | sh -s -- -y
    CHOOSENIM_CMD="$HOME/.nimble/bin/choosenim"
  fi
fi

# Every [nim.toolchains.*] entry whose bin_dir points into a choosenim
# toolchain directory is installed/verified through choosenim, the same way
# rustup pins exact Rust toolchains above — this is what lets bootstrap
# reproduce an exact Nim version instead of depending on whatever a system
# package manager happens to have installed. Reported even when choosenim
# itself is still missing, so the gap is visible in report-only mode too.
while IFS=$'\t' read -r selector bin_dir; do
  [[ -z "$selector" || "$bin_dir" != *".choosenim/toolchains/"* ]] && continue
  expanded_bin_dir="${bin_dir/#\~/$HOME}"
  if [[ -x "$expanded_bin_dir/nim" ]]; then
    ok "nim toolchain '$selector' installed at $expanded_bin_dir"
  else
    note "nim toolchain '$selector' not installed at $expanded_bin_dir. Install with: choosenim --yes $selector"
    missing=1
    if [[ "$INSTALL" == "1" && -n "$CHOOSENIM_CMD" && -x "$CHOOSENIM_CMD" ]]; then
      "$CHOOSENIM_CMD" --yes "$selector"
    fi
  fi
done < <(python3 - "$REPO_ROOT/toolchains.lock.toml" <<'PY'
import sys, tomllib
with open(sys.argv[1], "rb") as f:
    data = tomllib.load(f)
for entry in data.get("nim", {}).get("toolchains", {}).values():
    print(f"{entry['selector']}\t{entry.get('bin_dir', '')}")
PY
)

# Any Nim toolchain entry without a choosenim bin_dir still falls back to
# whatever nim/nimble are active on PATH (see nim_toolchain.rs).
check "nim (PATH fallback)" nim "brew install nim"
check "nimble (PATH fallback)" nimble "brew install nim"
echo

echo "Backend / target tools"
check "clang" clang "xcode-select --install"
check "llvm-config" llvm-config "brew install llvm"
check "ld.lld" ld.lld "brew install llvm"
check "wasm-ld" wasm-ld "brew install llvm"
check "wasm-opt" wasm-opt "brew install binaryen"
check "wasm-tools" wasm-tools "brew install wasm-tools"
echo

if [[ "$missing" == "1" && "$INSTALL" == "0" ]]; then
  echo "Some tools are missing. Re-run with --install to install them, or run the hints above yourself."
  exit 1
fi

echo "Run 'cargo run -p laminaria-cli -- doctor' for the full machine-readable environment/toolchain fingerprint."
