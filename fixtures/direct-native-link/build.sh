#!/usr/bin/env bash
# Builds this fixture in the direction it's designed to exercise: Rust
# produces a static library first, then Nim links directly against it
# and runs. Requires `cargo`/`rustc` and `nim` on PATH (see
# toolchains.lock.toml / scripts/bootstrap.sh).
set -euo pipefail
cd "$(dirname "$0")"

cargo build --manifest-path rust-lib/Cargo.toml --release

LIB_DIR="rust-lib/target/release"
OUT_BIN="nim-bin/direct_native_link_out"

nim c \
  --nimcache:nim-bin/nimcache \
  --passL:"-L${LIB_DIR} -lrustlib" \
  -o:"${OUT_BIN}" \
  nim-bin/main.nim

"./${OUT_BIN}"
