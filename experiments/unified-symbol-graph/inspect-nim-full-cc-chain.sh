#!/usr/bin/env bash
set -euo pipefail
W=$(mktemp -d)
cat > "$W/hello.nim" << 'EOF'
echo "hi"
EOF
cd "$W"
echo "=== nim c --listCmd, full output (every gcc invocation, not just the last) ==="
nim c --listCmd -o:hello hello.nim 2>&1 | grep -E "^(Hint: gcc|CC:)"
