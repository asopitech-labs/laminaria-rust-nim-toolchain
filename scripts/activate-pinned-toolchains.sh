#!/usr/bin/env bash
# Source this file to put the toolchains declared by toolchains.lock.toml
# ahead of ambient PATH entries. In particular, this prevents an x86_64
# Homebrew Nim/Cargo under /usr/local from winning over the arm64 tools this
# repository verified on Apple Silicon.
#
# Usage (from Bash):
#   source scripts/activate-pinned-toolchains.sh
#
# This is intentionally source-only: changing PATH in a child shell would not
# help its caller.

if [[ -z "${BASH_VERSION:-}" ]]; then
  echo "source scripts/activate-pinned-toolchains.sh from Bash (not another shell)" >&2
  return 2 2>/dev/null || exit 2
fi
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  echo "source scripts/activate-pinned-toolchains.sh; do not execute it directly" >&2
  exit 2
fi

PINNED_TOOLCHAIN_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PINNED_TOOLCHAIN_DATA="$(awk -F '"' '
  /^\[rust\.toolchains\.system_stable\]/ { section = "rust"; next }
  /^\[nim\.toolchains\./ { section = "nim"; nim_count += 1; next }
  /^\[/ { section = "" }
  section == "rust" && /^selector = / { rust = $2 }
  section == "nim" && /^selector = / { nim = $2 }
  section == "nim" && /^bin_dir = / { nim_bin = $2 }
  END {
    if (rust == "" || nim_count != 1 || nim == "" || nim_bin == "") exit 2
    print rust; print nim; print nim_bin
  }
' "$PINNED_TOOLCHAIN_ROOT/toolchains.lock.toml")" || {
  echo "could not read the required Rust and single pinned Nim entry from toolchains.lock.toml" >&2
  return 1
}
PINNED_RUST_SELECTOR="$(sed -n '1p' <<<"$PINNED_TOOLCHAIN_DATA")"
PINNED_NIM_SELECTOR="$(sed -n '2p' <<<"$PINNED_TOOLCHAIN_DATA")"
PINNED_NIM_BIN_DIR="$(sed -n '3p' <<<"$PINNED_TOOLCHAIN_DATA")"
PINNED_NIM_BIN_DIR="${PINNED_NIM_BIN_DIR/#\~/$HOME}"

if [[ ! -x "$PINNED_NIM_BIN_DIR/nim" ]]; then
  echo "pinned Nim $PINNED_NIM_SELECTOR is missing at $PINNED_NIM_BIN_DIR/nim; run scripts/bootstrap.sh --install" >&2
  return 1
fi
if ! rustup run "$PINNED_RUST_SELECTOR" rustc -vV >/dev/null 2>&1; then
  echo "pinned Rust toolchain $PINNED_RUST_SELECTOR is unavailable; run scripts/bootstrap.sh --install" >&2
  return 1
fi

PINNED_RUST_BIN_DIR="$(dirname "$(rustup which cargo --toolchain "$PINNED_RUST_SELECTOR")")"
export PATH="$PINNED_NIM_BIN_DIR:$HOME/.nimble/bin:$PINNED_RUST_BIN_DIR:$PATH"

unset PINNED_TOOLCHAIN_DATA PINNED_TOOLCHAIN_ROOT
