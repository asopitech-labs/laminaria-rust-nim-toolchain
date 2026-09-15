#!/usr/bin/env bash
# issue #64 P0: measure whether aws-lc-sys's default "all-bindings" feature (vs. the feature set
# actually resolved for alopex-cli via aws-lc-rs's `default-features = false` + feature
# unification -- "prebuilt-nasm" only) explains the 45.73s standalone build cost observed in
# issue #63/#59. Builds an isolated single-dependency crate under Docker (rust:1.96-bookworm,
# matching the alopex-cli pinned toolchain) and compares (A) the resolved set and (B) full
# defaults, then compares the built static library byte-for-byte.
#
# This is a static/build-time measurement of the aws-lc-sys build unit in isolation, not a
# benchmark of alopex-cli itself, per docs/02-research-areas/measurement/measurement-foundation.md.
#
# Usage: scripts/research/measure-aws-lc-sys-feature-cost.sh
# Requires: docker
set -euo pipefail

WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT

cat > "$WORKDIR/Cargo.toml" <<'EOF'
[package]
name = "measure-issue64"
version = "0.1.0"
edition = "2021"
publish = false

[dependencies]
aws-lc-sys = "=0.42.0"

[profile.release]
opt-level = 3
EOF
mkdir -p "$WORKDIR/src"
echo 'fn main() {}' > "$WORKDIR/src/main.rs"

cat > "$WORKDIR/Dockerfile" <<'EOF'
FROM rust:1.96-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends nasm clang cmake && rm -rf /var/lib/apt/lists/*
WORKDIR /work
COPY . /work
CMD ["bash"]
EOF

docker build -q -t laminaria-issue64-measure -f "$WORKDIR/Dockerfile" "$WORKDIR" >/dev/null

docker run --rm laminaria-issue64-measure bash -c '
set -euo pipefail
cd /work
for CFG in resolved default; do
  if [ "$CFG" = "default" ]; then
    sed -i "s/^aws-lc-sys.*/aws-lc-sys = \"=0.42.0\"/" Cargo.toml
    LABEL="B_default_all-bindings"
  else
    sed -i "s/^aws-lc-sys.*/aws-lc-sys = { version = \"=0.42.0\", default-features = false, features = [\"prebuilt-nasm\"] }/" Cargo.toml
    LABEL="A_resolved_prebuilt-nasm-only"
  fi
  rm -rf target
  START=$(date +%s)
  cargo build --release -q
  END=$(date +%s)
  LIBFILE=$(find target/release/build -name "libaws_lc_*.a" 2>/dev/null | head -1)
  echo "=== $LABEL ==="
  echo "wall_time_seconds: $((END - START))"
  echo "static_lib: $LIBFILE"
  echo "static_lib_bytes: $(stat -c%s "$LIBFILE" 2>/dev/null || echo unknown)"
  echo "exported_text_symbols: $(nm "$LIBFILE" 2>/dev/null | grep -c " T " || echo unknown)"
  echo "static_lib_sha256: $(sha256sum "$LIBFILE" | cut -d" " -f1)"
done
'
