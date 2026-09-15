#!/usr/bin/env bash
set -euo pipefail
rustup target add wasm32-unknown-unknown
W=$(mktemp -d)
cat > "$W/lib.rs" << 'EOF'
#[link(wasm_import_module = "cadd_module")]
extern "C" {
    fn c_add(a: i32, b: i32) -> i32;
}
#[no_mangle]
pub extern "C" fn compute() -> i32 {
    unsafe { c_add(3, 4) }
}
EOF
rustc --target wasm32-unknown-unknown --crate-type cdylib -O "$W/lib.rs" -o "$W/out.wasm"
echo "=== wasm-tools print (full module text form) ==="
wasm-tools print "$W/out.wasm"
echo
echo "=== wasm-tools dump (structural/binary layout) ==="
wasm-tools dump "$W/out.wasm" | head -60
