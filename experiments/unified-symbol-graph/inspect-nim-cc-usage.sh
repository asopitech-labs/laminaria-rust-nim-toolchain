#!/usr/bin/env bash
set -euo pipefail
W=$(mktemp -d)
cat > "$W/hello.nim" << 'EOF'
echo "hi"
EOF
cd "$W"
echo "=== nim.cfg: what C compiler does this Nim toolchain target by default? ==="
nim dump --dump.format:json 2>/dev/null | head -40 || true
echo
echo "=== compile with --listCmd to see the exact cc/gcc invocation ==="
nim c --listCmd -o:hello hello.nim 2>&1 | grep -E '^(gcc|cc|clang|/usr/bin/(cc|gcc|clang))' || \
nim c --listCmd -o:hello hello.nim 2>&1 | tail -20
