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
x86_64-w64-mingw32-gcc -c "$W/caller.c" -o "$W/caller.obj"
echo "=== file format ==="
x86_64-w64-mingw32-objdump -f "$W/caller.obj"
echo "=== symbols ==="
x86_64-w64-mingw32-nm "$W/caller.obj"
echo "=== disasm ==="
x86_64-w64-mingw32-objdump -d "$W/caller.obj"
echo "=== relocations ==="
x86_64-w64-mingw32-objdump -r "$W/caller.obj"
