//! Per-C-compiler-invocation measurement for `nim c`/`nim cpp` builds, via
//! the same wrapper-substitution idea `cargo_wrapper.rs` uses for Cargo --
//! studied from Nim's own real compiler source
//! (`.reference/Nim/compiler/extccomp.nim`) before writing this, not
//! assumed from general knowledge of how `nim c` works.
//!
//! **What was actually checked**: `extccomp.nim`'s `getCompileCFileCmd`
//! resolves the compiler executable as `getConfigVar(conf, c, ".exe")`
//! first, falling back to `getCompilerExe` (which only reads `CC`/`CXX`
//! when `--cc:env` is explicitly selected) -- so `--<ccname>.exe:<path>`
//! (e.g. `--clang.exe:<path>`) overrides just the executable while keeping
//! the normal, compiler-specific command *template* (flags, defines)
//! intact. This matters: an initial attempt using `--cc:env` (which also
//! changes the command template, not just the executable) produced a real
//! compile error (`undeclared identifier: 'atomicStoreN'`) on this
//! project's own `nim-heavy-workspace` fixture, because the generic
//! `ccEnv` template lacks clang-specific atomics detection the normal
//! `clang` profile supplies. `--<ccname>.exe:` avoids this entirely --
//! verified by actually building the fixture both ways before choosing
//! this approach.
//!
//! `getLinkCmd`'s linker-executable resolution (`getConfigVar(conf, c,
//! ".linkerexe")`, falling back to the same `.exe` value) uses the same
//! mechanism, so `--<ccname>.linkerexe:<path>` also captures the final
//! link step -- unlike the Cargo/`rustc` case, where the linker's cost
//! only rolls up into whichever `rustc` invocation spawned it. Verified
//! together: `nim c --clang.exe:<wrapper> --clang.linkerexe:<wrapper> ...`
//! on `nim-heavy-workspace` produced one wrapper invocation per generated
//! `.c` file (9, matching `extccomp.nim`'s `execProcesses`-based parallel
//! compilation) plus one separately-recorded link invocation, with the
//! resulting binary still running correctly.
//!
//! **Compiler-kind ambiguity, handled pragmatically, not silently**:
//! which named profile (`clang`, `gcc`, ...) Nim actually selects depends
//! on platform auto-detection this crate does not reproduce. Rather than
//! guess wrong, both `--clang.exe`/`--clang.linkerexe` and
//! `--gcc.exe`/`--gcc.linkerexe` are set to the same wrapper -- only
//! whichever one Nim actually selected as `conf.cCompiler` is ever read,
//! per `extccomp.nim`'s `getConfigVar`, so the unused overrides are
//! harmless. The wrapper itself is told the one real compiler to forward
//! to via `ENV_WRAPPED_CC`, resolved from `cc` on `PATH` -- correct when
//! `cc` matches whichever compiler Nim actually selects (true for this
//! project's own fixtures on both `gcc`-default Linux and `clang`-default
//! macOS), not a universal guarantee for an unusual cross-compilation
//! setup. Not silently assumed correct: if Nim selected a compiler `cc`
//! on `PATH` doesn't match, the wrapper would forward to the wrong
//! binary -- a real, named limitation, not yet hit in practice.

use std::path::PathBuf;

pub use crate::cargo_wrapper::{ENV_CLOCK_ANCHOR_UNIX_NS, ENV_EVENTS_PATH};

/// Absolute path to the real C/C++ compiler the wrapper should ultimately
/// invoke -- resolved once, from `cc` on `PATH` (see this module's doc
/// comment for the compiler-kind-ambiguity caveat that assumption carries).
pub const ENV_WRAPPED_CC: &str = "LAMINARIA_WRAPPED_CC";

/// The named Nim C-compiler profiles to override -- covers the two
/// defaults Nim auto-selects across the platforms this project targets
/// (`gcc` on Linux, `clang` on macOS). Extending this list is safe:
/// overriding an unselected profile's `.exe`/`.linkerexe` config var is a
/// no-op, per `extccomp.nim`'s `getConfigVar` only ever reading the one
/// actually-selected compiler's name.
pub const NIM_CC_PROFILES: &[&str] = &["clang", "gcc"];

/// Resolves the path to the `laminaria-cc-wrapper` binary this crate
/// builds, next to the currently-running executable -- the same
/// auxiliary-binary layout `cargo_wrapper::find_rustc_wrapper_binary`
/// uses.
pub fn find_cc_wrapper_binary() -> Option<PathBuf> {
    let current_exe = std::env::current_exe().ok()?;
    let dir = current_exe.parent()?;
    let candidate = dir.join(if cfg!(windows) {
        "laminaria-cc-wrapper.exe"
    } else {
        "laminaria-cc-wrapper"
    });
    candidate.is_file().then_some(candidate)
}

/// Extra command-line arguments to insert right after `nim c`/`nim cpp`'s
/// subcommand to activate wrapper substitution for every named profile in
/// `NIM_CC_PROFILES`. Command-line flags, not environment variables --
/// unlike Cargo's `RUSTC`, Nim has no environment-variable-only override
/// for this that also preserves the normal command template (see this
/// module's doc comment on why `--cc:env` was rejected).
pub fn wrapper_args(wrapper_bin: &std::path::Path) -> Vec<String> {
    let wrapper_str = wrapper_bin.display().to_string();
    NIM_CC_PROFILES
        .iter()
        .flat_map(|profile| {
            [
                format!("--{profile}.exe:{wrapper_str}"),
                format!("--{profile}.linkerexe:{wrapper_str}"),
            ]
        })
        .collect()
}
