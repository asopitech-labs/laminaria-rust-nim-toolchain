#!/usr/bin/env bash
set -euo pipefail
W=$(mktemp -d)
cc -c c/cadd/v1/cadd.c -o "$W/cadd.o" -I c/cadd/v1
echo "=== symbols ==="
nm "$W/cadd.o"
echo "=== disasm ==="
objdump -d "$W/cadd.o"
echo "=== relocations ==="
objdump -r "$W/cadd.o"
