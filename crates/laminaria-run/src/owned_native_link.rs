//! Explicit Darwin link boundary for LAMINARIA-produced native objects.
//!
//! Rust/Nim target source reaches this module only after it has been lowered
//! and encoded by `laminaria-ir::aarch64_darwin_target`.  This module never
//! invokes Cargo, rustc, Nim, a C compiler, or an assembler.  `/usr/bin/ld`
//! is deliberately the one declared external runtime/link action: it joins a
//! completed LAMINARIA Mach-O object with the selected Darwin system runtime.

use std::path::{Path, PathBuf};
use std::process::Command;

use laminaria_ir::aarch64_darwin_target::{generate_object, CodegenError};
use laminaria_ir::validate::ValidatedProgram;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DarwinLinkToolchain {
    /// The Darwin linker identity selected by the caller, normally
    /// `/usr/bin/ld`; it is not inferred from a source-language compiler.
    pub linker: PathBuf,
    /// The macOS SDK root that supplies the explicit `libSystem` runtime.
    pub sdk_root: PathBuf,
    /// The deployment target supplied verbatim to `ld`.
    pub minimum_macos_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedNativeArtifact {
    pub object_path: PathBuf,
    pub executable_path: PathBuf,
}

#[derive(Debug)]
pub enum OwnedNativeLinkError {
    UnsupportedHost,
    EntryHasParameters { entry: String, count: usize },
    Codegen(CodegenError),
    Io { path: PathBuf, detail: String },
    LinkerFailed { linker: PathBuf, detail: String },
}

impl std::fmt::Display for OwnedNativeLinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedHost => write!(f, "owned Darwin native link requires an aarch64 macOS host"),
            Self::EntryHasParameters { entry, count } => write!(f, "owned Darwin process entry {entry:?} has {count} parameters; this runtime boundary accepts none"),
            Self::Codegen(error) => write!(f, "owned Darwin code generation failed: {error}"),
            Self::Io { path, detail } => write!(f, "{}: {detail}", path.display()),
            Self::LinkerFailed { linker, detail } => write!(f, "owned Darwin link via {} failed: {detail}", linker.display()),
        }
    }
}

impl std::error::Error for OwnedNativeLinkError {}

/// Emits an owned source entry as Darwin process `_main`, then links it with
/// the declared runtime. The object is written before `ld` runs so its
/// producer and link input remain inspectable as separate artifacts.
pub fn compile_and_link_entry(
    program: &ValidatedProgram,
    entry: &str,
    output_dir: &Path,
    toolchain: &DarwinLinkToolchain,
) -> Result<OwnedNativeArtifact, OwnedNativeLinkError> {
    if !cfg!(all(target_arch = "aarch64", target_os = "macos")) {
        return Err(OwnedNativeLinkError::UnsupportedHost);
    }
    let function = program.program().functions.get(entry).ok_or_else(|| {
        OwnedNativeLinkError::Codegen(CodegenError::MissingEntry(entry.to_owned()))
    })?;
    if !function.params.is_empty() {
        return Err(OwnedNativeLinkError::EntryHasParameters {
            entry: entry.to_owned(),
            count: function.params.len(),
        });
    }

    let object = generate_object(program, entry).map_err(OwnedNativeLinkError::Codegen)?;
    std::fs::create_dir_all(output_dir).map_err(|error| OwnedNativeLinkError::Io {
        path: output_dir.to_path_buf(),
        detail: error.to_string(),
    })?;
    let object_path = output_dir.join("laminaria-main.o");
    let executable_path = output_dir.join("laminaria-main");
    std::fs::write(&object_path, object).map_err(|error| OwnedNativeLinkError::Io {
        path: object_path.clone(),
        detail: error.to_string(),
    })?;

    let output = Command::new(&toolchain.linker)
        .args([
            "-dynamic",
            "-arch",
            "arm64",
            "-platform_version",
            "macos",
            &toolchain.minimum_macos_version,
            &toolchain.minimum_macos_version,
            "-syslibroot",
        ])
        .arg(&toolchain.sdk_root)
        .arg("-o")
        .arg(&executable_path)
        .arg(&object_path)
        .arg("-lSystem")
        .output()
        .map_err(|error| OwnedNativeLinkError::Io {
            path: toolchain.linker.clone(),
            detail: error.to_string(),
        })?;
    if !output.status.success() {
        return Err(OwnedNativeLinkError::LinkerFailed {
            linker: toolchain.linker.clone(),
            detail: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(OwnedNativeArtifact {
        object_path,
        executable_path,
    })
}

#[cfg(all(test, target_arch = "aarch64", target_os = "macos"))]
mod tests {
    use super::*;
    use laminaria_ir::rust_frontend::lower_rust_source;
    use laminaria_ir::validate::validate_program;

    fn toolchain() -> DarwinLinkToolchain {
        let sdk = Command::new("xcrun")
            .arg("--show-sdk-path")
            .output()
            .expect("xcrun must be available on the declared macOS target");
        assert!(sdk.status.success());
        DarwinLinkToolchain {
            linker: PathBuf::from("/usr/bin/ld"),
            sdk_root: PathBuf::from(String::from_utf8(sdk.stdout).unwrap().trim()),
            minimum_macos_version: "11.0".to_owned(),
        }
    }

    #[test]
    fn owned_source_object_links_and_launches_without_rustc_or_nim() {
        let source = "fn main() -> i32 { 3i32.wrapping_add(4) }";
        let program =
            lower_rust_source(std::path::Path::new("main.rs"), source, &["main"]).unwrap();
        let root = std::env::temp_dir().join(format!(
            "laminaria-owned-native-link-action-{}",
            std::process::id()
        ));
        let artifact = compile_and_link_entry(
            &validate_program(&program).unwrap(),
            "main",
            &root,
            &toolchain(),
        )
        .unwrap();
        assert!(artifact.object_path.is_file());
        assert_eq!(
            Command::new(&artifact.executable_path)
                .status()
                .unwrap()
                .code(),
            Some(7)
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
