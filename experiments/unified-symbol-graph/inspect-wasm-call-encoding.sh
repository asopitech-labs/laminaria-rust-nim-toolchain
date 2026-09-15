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
echo "=== module structural section listing ==="
wasm-tools dump "$W/out.wasm" 2>/dev/null | grep -E "^\s*0x[0-9a-f]+ \|.*(section|import|export|func|code)" -i | head -30
echo
echo "=== import section raw bytes ==="
wasm-tools dump "$W/out.wasm" 2>/dev/null | awk '/import section/{p=1} /export section/{p=0} p' | head -20
echo
echo "=== code section raw bytes (the actual call instruction encoding) ==="
wasm-tools dump "$W/out.wasm" 2>/dev/null | awk '/code section/{p=1} p' | head -30
