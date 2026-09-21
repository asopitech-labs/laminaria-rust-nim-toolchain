//! Issue #49's independent consumer for an already-published production
//! artifact. This module never calls G2 production or its preflight helpers.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use laminaria_plan::dependency_graph::{ObligationKind, ObligationState, RequiredActionKind};
use laminaria_plan::obligation_lifecycle::{
    ArtifactManifest, RuntimeContractEvidence, OBLIGATION_CONTRACT_VERSION,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestContract {
    pub contract_id: String,
    pub subject_digest: String,
    pub harness_identity: String,
    pub environment_identity: String,
    pub graph_demand: String,
    pub retained_roots: Vec<String>,
    pub pruned_roots: Vec<String>,
    pub expected_exit: i32,
    pub expected_stdout: String,
    pub expected_stderr: String,
    pub required_symbols: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessEventKind {
    PrepareEnvironment,
    InspectArtifact,
    ExecuteSubject,
    ObserveResult,
    Cleanup,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessEvent {
    pub kind: HarnessEventKind,
    pub succeeded: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum TestVerdict {
    Pass,
    DependencyMissing { contract_id: String, path: String },
    Fail { detail: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestResult {
    pub contract: TestContract,
    pub runtime_contract: RuntimeContractEvidence,
    pub exact_subject_digest: String,
    pub events: Vec<HarnessEvent>,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub verdict: TestVerdict,
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

fn run_inspector(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("spawn {program}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{program} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn under_runtime_root(runtime_root: &Path, published_path: &str) -> PathBuf {
    runtime_root.join(published_path.trim_start_matches('/'))
}

fn dependency_missing(
    contract: TestContract,
    runtime_contract: RuntimeContractEvidence,
    digest: String,
    events: Vec<HarnessEvent>,
    path: &Path,
) -> TestResult {
    TestResult {
        contract,
        exact_subject_digest: digest,
        verdict: TestVerdict::DependencyMissing {
            contract_id: runtime_contract.contract_id.clone(),
            path: path.display().to_string(),
        },
        runtime_contract,
        events,
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
    }
}

/// Qualifies a published artifact solely from the artifact, manifest, and a
/// consumer-selected runtime root. A missing provider under that root fails
/// before execution and is never recovered from the host filesystem.
#[cfg(target_os = "linux")]
pub fn qualify_published_artifact(
    subject_path: &Path,
    manifest_path: &Path,
    runtime_root: &Path,
    work_dir: &Path,
) -> Result<TestResult, String> {
    let manifest: ArtifactManifest = serde_json::from_slice(
        &fs::read(manifest_path)
            .map_err(|error| format!("{}: {error}", manifest_path.display()))?,
    )
    .map_err(|error| format!("manifest: {error}"))?;
    if manifest.contract_version != OBLIGATION_CONTRACT_VERSION {
        return Err(format!(
            "unsupported manifest contract {}",
            manifest.contract_version
        ));
    }
    let digest = sha256_file(subject_path)?;
    if digest != manifest.requested_artifact.digest {
        return Err("subject digest differs from the published manifest".to_owned());
    }
    let runtime_decision = manifest
        .decisions
        .iter()
        .find(|decision| decision.kind == ObligationKind::Runtime)
        .ok_or_else(|| "manifest has no runtime decision".to_owned())?;
    if runtime_decision.state != ObligationState::Externalized {
        return Err("runtime obligation is not Externalized".to_owned());
    }
    let runtime_contract = manifest
        .runtime_contracts
        .first()
        .cloned()
        .ok_or_else(|| "manifest publishes no runtime contract".to_owned())?;
    if manifest.runtime_contracts.len() != 1
        || !manifest.operations.iter().any(|operation| {
            operation.operation_id == runtime_contract.operation_id
                && operation.action_kind == RequiredActionKind::PreflightRuntimeContract
        })
    {
        return Err("runtime contract has no unique causal preflight operation".to_owned());
    }

    let contract = TestContract {
        contract_id: "m1-exact-artifact-cross-language".to_owned(),
        subject_digest: digest.clone(),
        harness_identity: format!(
            "laminaria-run/exact-artifact-harness@{}",
            env!("CARGO_PKG_VERSION")
        ),
        environment_identity: runtime_root.display().to_string(),
        graph_demand: "qualify:executable:app".to_owned(),
        retained_roots: vec!["exact-subject".to_owned(), "runtime-contract".to_owned()],
        pruned_roots: vec![
            "framework-test-binary".to_owned(),
            "instrumented-binary".to_owned(),
        ],
        expected_exit: 0,
        expected_stdout: "9\n".to_owned(),
        expected_stderr: String::new(),
        required_symbols: vec![
            "c_add".to_owned(),
            "nim_double".to_owned(),
            "cpp_max_i32".to_owned(),
        ],
    };
    let mut events = vec![HarnessEvent {
        kind: HarnessEventKind::PrepareEnvironment,
        succeeded: true,
        detail: format!(
            "runtime root {} selected by consumer",
            runtime_root.display()
        ),
    }];

    let loader = under_runtime_root(runtime_root, &runtime_contract.loader_requirement);
    if !loader.is_file() {
        events.push(HarnessEvent {
            kind: HarnessEventKind::InspectArtifact,
            succeeded: false,
            detail: format!("missing loader {}", loader.display()),
        });
        return Ok(dependency_missing(
            contract,
            runtime_contract,
            digest,
            events,
            &loader,
        ));
    }
    for provider in &runtime_contract.required_providers {
        if let Some(path) = &provider.path {
            let candidate = under_runtime_root(runtime_root, path);
            if !candidate.is_file() {
                events.push(HarnessEvent {
                    kind: HarnessEventKind::InspectArtifact,
                    succeeded: false,
                    detail: format!("missing provider {}", candidate.display()),
                });
                return Ok(dependency_missing(
                    contract,
                    runtime_contract,
                    digest,
                    events,
                    &candidate,
                ));
            }
        }
    }

    let headers = run_inspector(
        "readelf",
        &["--program-headers", &subject_path.to_string_lossy()],
    )?;
    if !headers.contains(&runtime_contract.loader_requirement) {
        return Err("artifact interpreter differs from published runtime contract".to_owned());
    }
    let symbols = run_inspector("nm", &[&subject_path.to_string_lossy()])?;
    for symbol in &contract.required_symbols {
        if !symbols
            .lines()
            .any(|line| line.split_whitespace().last() == Some(symbol.as_str()))
        {
            return Err(format!(
                "exact artifact does not contain required symbol {symbol}"
            ));
        }
    }
    events.push(HarnessEvent {
        kind: HarnessEventKind::InspectArtifact,
        succeeded: true,
        detail: "digest, interpreter, providers, and Rust/Nim/C/C++ symbols observed independently"
            .to_owned(),
    });

    fs::create_dir_all(work_dir).map_err(|error| format!("{}: {error}", work_dir.display()))?;
    let isolated_subject = work_dir.join("subject");
    fs::copy(subject_path, &isolated_subject)
        .map_err(|error| format!("copy exact subject: {error}"))?;
    if sha256_file(&isolated_subject)? != digest {
        return Err("isolated subject digest changed".to_owned());
    }
    let output = Command::new(&isolated_subject)
        .env_clear()
        .current_dir(work_dir)
        .output()
        .map_err(|error| format!("execute exact subject: {error}"))?;
    let exit_code = output.status.code();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    events.push(HarnessEvent {
        kind: HarnessEventKind::ExecuteSubject,
        succeeded: true,
        detail: "exact subject executed with empty environment".to_owned(),
    });
    let passed = exit_code == Some(contract.expected_exit)
        && stdout == contract.expected_stdout
        && stderr == contract.expected_stderr;
    events.push(HarnessEvent {
        kind: HarnessEventKind::ObserveResult,
        succeeded: passed,
        detail: format!("exit={exit_code:?} stdout={stdout:?} stderr={stderr:?}"),
    });
    events.push(HarnessEvent {
        kind: HarnessEventKind::Cleanup,
        succeeded: true,
        detail: "caller-owned evidence directory retained for raw inspection".to_owned(),
    });
    Ok(TestResult {
        contract,
        runtime_contract,
        exact_subject_digest: digest,
        events,
        exit_code,
        stdout,
        stderr,
        verdict: if passed {
            TestVerdict::Pass
        } else {
            TestVerdict::Fail {
                detail: "observable result differs from contract".to_owned(),
            }
        },
    })
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use crate::g2_execute::execute_fixed_m1_graph;

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap()
    }

    #[test]
    fn independent_consumer_accepts_exact_digest_and_rejects_missing_runtime() {
        let root = std::env::temp_dir().join(format!("laminaria-issue-49-{}", std::process::id()));
        let published = root.join("published");
        let publication = execute_fixed_m1_graph(&repo_root(), &published).unwrap();

        let positive = qualify_published_artifact(
            &publication.executable.executable_path,
            &publication.manifest_path,
            Path::new("/"),
            &root.join("positive-consumer"),
        )
        .unwrap();
        assert_eq!(positive.verdict, TestVerdict::Pass);
        assert_eq!(
            positive.exact_subject_digest,
            publication.executable.executable_sha256
        );
        assert_eq!(
            positive
                .events
                .iter()
                .filter(|event| event.kind == HarnessEventKind::ExecuteSubject)
                .count(),
            1
        );

        let empty_runtime = root.join("missing-runtime-root");
        fs::create_dir_all(&empty_runtime).unwrap();
        let negative = qualify_published_artifact(
            &publication.executable.executable_path,
            &publication.manifest_path,
            &empty_runtime,
            &root.join("negative-consumer"),
        )
        .unwrap();
        assert!(matches!(
            negative.verdict,
            TestVerdict::DependencyMissing { .. }
        ));
        assert!(negative
            .events
            .iter()
            .all(|event| event.kind != HarnessEventKind::ExecuteSubject));
        assert!(!root.join("negative-consumer/subject").exists());
        let _ = fs::remove_dir_all(root);
    }
}
