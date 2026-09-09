//! Compiles `../nim-lib/nimlib.nim` into a static library with `nim c
//! --app:staticlib --noMain` and links it in — same build strategy as
//! `rust-nim-c-abi-baseline`. Requires `nim` on `PATH` (see
//! `toolchains.lock.toml` / `scripts/bootstrap.sh`).

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let nim_src = manifest_dir.join("..").join("nim-lib").join("nimlib.nim");
    let lib_path = out_dir.join("libnimlib.a");
    let nimcache_dir = out_dir.join("nimcache");

    let status = Command::new("nim")
        .arg("c")
        .arg("--app:staticlib")
        .arg("--noMain")
        .arg(format!("--nimcache:{}", nimcache_dir.display()))
        .arg(format!("-o:{}", lib_path.display()))
        .arg(&nim_src)
        .status()
        .unwrap_or_else(|e| {
            panic!("failed to run `nim` (is it on PATH? see toolchains.lock.toml): {e}")
        });
    assert!(
        status.success(),
        "nim compilation of {} failed",
        nim_src.display()
    );

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=nimlib");
    println!("cargo:rerun-if-changed={}", nim_src.display());
}
