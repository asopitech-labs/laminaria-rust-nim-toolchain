#!/usr/bin/env bash
set -euo pipefail
W=$(mktemp -d)
cat > "$W/hello.rs" << 'EOF'
fn main() { println!("hi"); }
EOF
cd "$W"
echo "=== rustc --print link-args (what program does rustc invoke to link?) ==="
rustc --edition 2021 -O hello.rs --print link-args -o hello 2>&1 | head -1 | tr ' ' '\n' | head -3

echo
echo "=== does rustc call cc for linking by default? (build with verbose) ==="
rustc --edition 2021 -O hello.rs -o hello --print link-args 2>&1 | tr ' ' '\n' | grep -m1 -E '^"[a-zA-Z]'

echo
echo "=== is cc itself just a wrapper? what does cc -### show for a trivial link ==="
echo 'int main(){return 0;}' > t.c
cc -### t.c -o t 2>&1 | tail -5
