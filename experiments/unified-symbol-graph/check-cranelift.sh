#!/usr/bin/env bash
set -euo pipefail
echo "=== rustc version (need nightly for cranelift backend flag) ==="
rustc --version
echo "=== is rustc_codegen_cranelift installable via rustup component? ==="
rustup component list 2>&1 | grep -i cranelift || echo "not found as a rustup component"
echo "=== check if nightly is available ==="
rustup toolchain list 2>&1
