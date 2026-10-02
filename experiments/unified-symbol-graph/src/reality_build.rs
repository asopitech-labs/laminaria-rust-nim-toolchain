//! A minimal, executable state-retention experiment for issue #67.
//!
//! `SharedSymbolGraph` by itself is an in-memory boundary-symbol model: it
//! cannot establish whether retaining state improves a real build. This module
//! supplies the smallest bridge that can be compared with the current
//! Cargo+rustc pipeline: it retains source fingerprints and an already
//! resolved local crate DAG, selects a changed crate and its downstream
//! dependents, then invokes the real `rustc` to emit the actual libraries and
//! executable.
//!
//! This is *not* a Cargo replacement. It does not parse `Cargo.toml`, resolve
//! external packages, support features/build scripts/proc macros, or promise
//! compatibility beyond the supplied crate plan. Those omissions are exactly
//! why Cargo+rustc remains a mandatory baseline rather than an implementation
//! detail hidden by this experiment.

use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrateKind {
    Library,
    Binary,
}

#[derive(Debug, Clone)]
pub struct RustCrate {
    pub name: String,
    pub source: PathBuf,
    pub kind: CrateKind,
    /// Names of direct dependencies, in an order suitable for `--extern`.
    pub dependencies: Vec<String>,
}

impl RustCrate {
    pub fn library(name: impl Into<String>, source: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            source: source.into(),
            kind: CrateKind::Library,
            dependencies: Vec::new(),
        }
    }

    pub fn binary(
        name: impl Into<String>,
        source: impl Into<PathBuf>,
        dependencies: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            name: name.into(),
            source: source.into(),
            kind: CrateKind::Binary,
            dependencies: dependencies.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildReport {
    /// Crates for which this invocation actually ran rustc, in plan order.
    pub compiled: Vec<String>,
    pub elapsed: Duration,
    pub executable: Option<PathBuf>,
}

/// An in-process retained build state for a pre-resolved local Rust crate DAG.
pub struct PersistentRustBuild {
    rustc: PathBuf,
    output_dir: PathBuf,
    plan: Vec<RustCrate>,
    source_fingerprints: HashMap<String, u64>,
    artifacts: HashMap<String, PathBuf>,
}

impl PersistentRustBuild {
    /// `plan` must be topologically ordered: every dependency precedes its
    /// dependent. Rejecting malformed plans up front keeps incremental output
    /// from silently linking an artifact from an unrelated prior run.
    pub fn new(
        rustc: impl Into<PathBuf>,
        output_dir: impl Into<PathBuf>,
        plan: Vec<RustCrate>,
    ) -> io::Result<Self> {
        let names: HashSet<_> = plan.iter().map(|krate| krate.name.as_str()).collect();
        let mut seen = HashSet::new();
        for krate in &plan {
            if !seen.insert(krate.name.as_str()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("duplicate crate name in plan: {}", krate.name),
                ));
            }
            for dependency in &krate.dependencies {
                if !names.contains(dependency.as_str()) || !seen.contains(dependency.as_str()) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!(
                            "{} depends on {dependency}, which must appear earlier in the plan",
                            krate.name
                        ),
                    ));
                }
            }
        }

        Ok(Self {
            rustc: rustc.into(),
            output_dir: output_dir.into(),
            plan,
            source_fingerprints: HashMap::new(),
            artifacts: HashMap::new(),
        })
    }

    /// Compiles source changes and the exact transitive downstream closure.
    /// Fingerprints are committed only after a successful rustc invocation, so
    /// a failed edit cannot be mistaken for a valid cached artifact on retry.
    pub fn build(&mut self) -> io::Result<BuildReport> {
        fs::create_dir_all(&self.output_dir)?;
        let started = Instant::now();
        let fingerprints = self.current_fingerprints()?;
        let changed: HashSet<_> = self
            .plan
            .iter()
            .filter(|krate| {
                self.source_fingerprints.get(&krate.name) != fingerprints.get(&krate.name)
            })
            .map(|krate| krate.name.clone())
            .collect();
        let selected = self.downstream_closure(&changed);
        let mut compiled = Vec::new();
        let mut executable = None;

        for krate in &self.plan {
            if !selected.contains(&krate.name) {
                if krate.kind == CrateKind::Binary {
                    executable = self.artifacts.get(&krate.name).cloned();
                }
                continue;
            }

            let artifact = self.compile(krate)?;
            self.source_fingerprints.insert(
                krate.name.clone(),
                *fingerprints
                    .get(&krate.name)
                    .expect("every planned crate has a source fingerprint"),
            );
            self.artifacts.insert(krate.name.clone(), artifact.clone());
            if krate.kind == CrateKind::Binary {
                executable = Some(artifact);
            }
            compiled.push(krate.name.clone());
        }

        Ok(BuildReport {
            compiled,
            elapsed: started.elapsed(),
            executable,
        })
    }

    fn current_fingerprints(&self) -> io::Result<HashMap<String, u64>> {
        self.plan
            .iter()
            .map(|krate| {
                source_fingerprint(&krate.source)
                    .map(|fingerprint| (krate.name.clone(), fingerprint))
            })
            .collect()
    }

    fn downstream_closure(&self, changed: &HashSet<String>) -> HashSet<String> {
        let mut selected = changed.clone();
        let mut made_progress = true;
        while made_progress {
            made_progress = false;
            for krate in &self.plan {
                if !selected.contains(&krate.name)
                    && krate
                        .dependencies
                        .iter()
                        .any(|dependency| selected.contains(dependency))
                {
                    selected.insert(krate.name.clone());
                    made_progress = true;
                }
            }
        }
        selected
    }

    fn compile(&self, krate: &RustCrate) -> io::Result<PathBuf> {
        let mut command = Command::new(&self.rustc);
        command
            .arg("--crate-name")
            .arg(&krate.name)
            .arg("--edition=2021")
            .arg(&krate.source)
            .arg("--out-dir")
            .arg(&self.output_dir)
            // Match Cargo's dev profile for the comparison fixture instead
            // of letting direct rustc silently use its lower-debug default.
            .arg("-C")
            .arg("debuginfo=2")
            .arg("-C")
            .arg("split-debuginfo=unpacked")
            .arg("-C")
            .arg("embed-bitcode=no");

        match krate.kind {
            CrateKind::Library => {
                command.arg("--crate-type=lib");
            }
            CrateKind::Binary => {
                for dependency in &krate.dependencies {
                    let artifact = self.artifacts.get(dependency).ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!("missing artifact for dependency {dependency}"),
                        )
                    })?;
                    command.arg("--extern").arg(format!(
                        "{}={}",
                        dependency.replace('-', "_"),
                        artifact.display()
                    ));
                }
            }
        }

        let output = command.output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "rustc failed for {}: {}",
                krate.name,
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let artifact = match krate.kind {
            CrateKind::Library => self
                .output_dir
                .join(format!("lib{}.rlib", krate.name.replace('-', "_"))),
            CrateKind::Binary => self.output_dir.join(&krate.name),
        };
        if !artifact.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("rustc succeeded but did not emit {}", artifact.display()),
            ));
        }
        Ok(artifact)
    }
}

fn source_fingerprint(source: &Path) -> io::Result<u64> {
    let bytes = fs::read(source)?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Ok(hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    fn fixture_copy() -> PathBuf {
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/incremental-semantic-edit");
        let destination = std::env::temp_dir().join(format!(
            "unified-symbol-graph-reality-build-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        copy_dir(&source, &destination).expect("fixture must copy into isolated test directory");
        destination
    }

    fn copy_dir(source: &Path, destination: &Path) -> io::Result<()> {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let target = destination.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                copy_dir(&entry.path(), &target)?;
            } else {
                fs::copy(entry.path(), target)?;
            }
        }
        Ok(())
    }

    fn fixture_plan(root: &Path) -> Vec<RustCrate> {
        vec![
            RustCrate::library("leaf_a", root.join("crates/leaf-a/src/lib.rs")),
            RustCrate::library("leaf_b", root.join("crates/leaf-b/src/lib.rs")),
            RustCrate::library("leaf_c", root.join("crates/leaf-c/src/lib.rs")),
            RustCrate::binary(
                "aggregator",
                root.join("crates/aggregator/src/main.rs"),
                ["leaf_a", "leaf_b", "leaf_c"],
            ),
        ]
    }

    #[test]
    fn persistent_build_emits_and_incrementally_updates_the_real_fixture_binary() {
        let fixture = fixture_copy();
        let output = fixture.join("direct-rustc-output");
        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let mut build = PersistentRustBuild::new(rustc, &output, fixture_plan(&fixture)).unwrap();

        let cold = build.build().expect("cold direct-rustc build succeeds");
        assert_eq!(cold.compiled, ["leaf_a", "leaf_b", "leaf_c", "aggregator"]);
        let cold_binary = cold.executable.expect("binary artifact is emitted");
        let cold_run = Command::new(&cold_binary).output().unwrap();
        assert!(cold_run.status.success());
        assert!(String::from_utf8_lossy(&cold_run.stdout).contains("total=839875"));

        fs::copy(
            fixture.join("crates/leaf-b/src/lib.edited.rs"),
            fixture.join("crates/leaf-b/src/lib.rs"),
        )
        .unwrap();
        let incremental = build.build().expect("edited direct-rustc build succeeds");
        assert_eq!(incremental.compiled, ["leaf_b", "aggregator"]);
        let edited_binary = incremental.executable.expect("edited binary is emitted");
        let edited_run = Command::new(&edited_binary).output().unwrap();
        assert!(edited_run.status.success());
        assert!(String::from_utf8_lossy(&edited_run.stdout).contains("total=867595"));

        eprintln!(
            "[reality-build] cold={} crates in {:?}; edited={} crates in {:?}",
            cold.compiled.len(),
            cold.elapsed,
            incremental.compiled.len(),
            incremental.elapsed
        );

        fs::remove_dir_all(fixture).expect("test-owned fixture copy is removable");
    }
}
