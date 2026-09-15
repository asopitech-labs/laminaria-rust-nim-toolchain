#!/usr/bin/env bash
set -euo pipefail
W=$(mktemp -d)
cat > "$W/caller.c" << 'EOF'
extern int c_add(int a, int b);
extern int cpp_max_i32(int a, int b);
int compute(void) {
    return c_add(cpp_max_i32(3, 4), 1);
}
EOF
cc -c "$W/caller.c" -o "$W/caller.o"
echo "=== symbols ==="
nm "$W/caller.o"
echo "=== disasm ==="
objdump -d "$W/caller.o"
echo "=== relocations (all sections) ==="
objdump -r "$W/caller.o"
