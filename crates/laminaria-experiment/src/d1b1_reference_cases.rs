//! Issue #28 D1-b1: re-runs the *existing* fixtures/tests named by
//! `docs/design/issue-35-d0-cases.yaml`'s M4/M5 cases and records the
//! raw evidence, the exact reproduction command, and the
//! `execution_role` classification already fixed at D0 -- this module
//! never re-selects a case's role, source layout, or expected value; it
//! only reproduces and records.
//!
//! Every reproduction command below is copied verbatim from
//! `.github/workflows/ci.yml`'s own already-passing steps for these
//! fixtures (not re-derived), so a passing run here reproduces exactly
//! what CI already proves on every push, with the raw output additionally
//! kept as case-registration evidence under `runs/d1b1/<case-id>/`.

use std::path::PathBuf;
use std::process::Command;

use crate::planner_binary::repo_root;

#[derive(Debug, Clone)]
pub struct CaseEvidence {
    pub case_id: String,
    pub execution_role: String,
    pub reproduction_commands: Vec<String>,
    pub raw_log_path: PathBuf,
    pub pass: bool,
    pub summary: String,
}

fn runs_dir() -> PathBuf {
    repo_root().join("runs").join("d1b1")
}

/// Runs `bash -c script` from `cwd` (relative to the repo root), writes a
/// combined command+stdout+stderr+exit-status transcript to
/// `runs/d1b1/<case_id>.log`, and returns `(script's own exit code == 0,
/// full transcript)`. Reused as-is for every case below -- the specific
/// pass/fail semantics (e.g. "exit 134 is the expected outcome") are
/// encoded *inside* each case's own script, exactly as
/// `.github/workflows/ci.yml` already does, not reinterpreted here.
fn run_and_log(case_id: &str, cwd: &str, script: &str) -> Result<(bool, String), String> {
    let root = repo_root();
    let full_cwd = root.join(cwd);
    let output = Command::new("bash")
        .arg("-c")
        .arg(script)
        .current_dir(&full_cwd)
        .output()
        .map_err(|e| format!("failed to spawn bash for {case_id}: {e}"))?;

    let transcript = format!(
        "case_id={case_id}\ncwd={}\nscript:\n{script}\n--- stdout ---\n{}\n--- stderr \
         ---\n{}\nexit_code={:?}\n",
        full_cwd.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        output.status.code()
    );

    let dir = runs_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
    let log_path = dir.join(format!("{case_id}.log"));
    std::fs::write(&log_path, &transcript)
        .map_err(|e| format!("failed to write {}: {e}", log_path.display()))?;

    Ok((output.status.success(), transcript))
}

/// M4-rust-nim-c-abi-scalar: `fixtures/rust-nim-c-abi-baseline`. No
/// internal assertion exists in this fixture's `main.rs` (confirmed by
/// direct read) -- D0's own `expected.value_or_diagnostic` is
/// deliberately "the existing main.rs's own output," not a pinned
/// number, so this case's pass criterion is a clean exit reproducing
/// that existing, unmodified program -- the raw output is kept as
/// registration evidence, not compared against an independently
/// invented number.
pub fn run_m4_scalar() -> Result<CaseEvidence, String> {
    let case_id = "M4-rust-nim-c-abi-scalar";
    let script = "cargo run";
    let (pass, log) = run_and_log(case_id, "fixtures/rust-nim-c-abi-baseline/rust-bin", script)?;
    Ok(CaseEvidence {
        case_id: case_id.to_string(),
        execution_role: "reference".to_string(),
        reproduction_commands: vec![format!(
            "cd fixtures/rust-nim-c-abi-baseline/rust-bin && {script}"
        )],
        raw_log_path: runs_dir().join(format!("{case_id}.log")),
        pass,
        summary: if pass {
            log.lines()
                .filter(|l| {
                    l.starts_with("points=")
                        || l.starts_with("perimeter=")
                        || l.starts_with("centroid=")
                        || l.starts_with("is_17_prime=")
                })
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            "cargo run exited non-zero".to_string()
        },
    })
}

/// M4-rust-nim-c-abi-buffer: `fixtures/mixed-rust-nim-executable`.
/// `main.rs:28-33` pins `EXPECTED_CHECKSUM_BEFORE=11146`,
/// `EXPECTED_SUM=15788`, `EXPECTED_CHECKSUM_AFTER=50566` (plus min/max/
/// mean) and asserts them itself (`main.rs:86-98`) -- a clean exit *is*
/// the cross-check against D0's pinned expected value.
pub fn run_m4_buffer() -> Result<CaseEvidence, String> {
    let case_id = "M4-rust-nim-c-abi-buffer";
    let script = "cargo run";
    let (pass, _log) = run_and_log(
        case_id,
        "fixtures/mixed-rust-nim-executable/rust-bin",
        script,
    )?;
    Ok(CaseEvidence {
        case_id: case_id.to_string(),
        execution_role: "reference".to_string(),
        reproduction_commands: vec![format!(
            "cd fixtures/mixed-rust-nim-executable/rust-bin && {script}"
        )],
        raw_log_path: runs_dir().join(format!("{case_id}.log")),
        pass,
        summary: if pass {
            "internal assert_eq! against EXPECTED_CHECKSUM_BEFORE=11146/EXPECTED_SUM=15788/\
             EXPECTED_CHECKSUM_AFTER=50566 (main.rs:28-33) passed"
                .to_string()
        } else {
            "cargo run exited non-zero (an internal assert_eq! failed or the binary crashed)"
                .to_string()
        },
    })
}

/// M4-rust-nim-c-abi-callcount: `fixtures/boundary-heavy-workload`.
/// `main.rs:22` pins `EXPECTED_FINAL=636_658_098` and asserts it itself
/// (`main.rs:52-58`) after 1,000,000 calls -- a clean exit *is* the
/// cross-check. The D0 case's own `measurement` fixes `warmup_runs: 1,
/// repetitions: 5` (reference timing only, `noise_floor:
/// "not-applicable"` per the case -- no owned-baseline comparison is
/// made, the wall-time is recorded as evidence, not judged).
pub fn run_m4_callcount() -> Result<CaseEvidence, String> {
    let case_id = "M4-rust-nim-c-abi-callcount";
    let cwd = "fixtures/boundary-heavy-workload/rust-bin";
    let script = "cargo run --release";

    // Warmup: a failure here is fatal (build/environment problem), not
    // recorded as a measured repetition.
    let (warmup_pass, warmup_log) = run_and_log(&format!("{case_id}--warmup"), cwd, script)?;
    if !warmup_pass {
        return Ok(CaseEvidence {
            case_id: case_id.to_string(),
            execution_role: "reference".to_string(),
            reproduction_commands: vec![format!("cd {cwd} && {script}")],
            raw_log_path: runs_dir().join(format!("{case_id}--warmup.log")),
            pass: false,
            summary: format!("warmup run failed before any measured repetition: {warmup_log}"),
        });
    }

    let mut all_pass = true;
    let mut combined_log = String::new();
    let mut wall_seconds = Vec::new();
    for i in 0..5 {
        let start = std::time::Instant::now();
        let (pass, log) = run_and_log(&format!("{case_id}--rep{i}"), cwd, script)?;
        wall_seconds.push(start.elapsed().as_secs_f64());
        combined_log.push_str(&log);
        combined_log.push_str("\n====\n");
        if !pass {
            all_pass = false;
        }
    }

    let dir = runs_dir();
    let log_path = dir.join(format!("{case_id}.log"));
    std::fs::write(&log_path, &combined_log)
        .map_err(|e| format!("failed to write {}: {e}", log_path.display()))?;

    let mean = wall_seconds.iter().sum::<f64>() / wall_seconds.len() as f64;

    Ok(CaseEvidence {
        case_id: case_id.to_string(),
        execution_role: "reference".to_string(),
        reproduction_commands: vec![format!(
            "cd {cwd} && {script}  # 1 warmup + 5 measured repetitions per the D0 case's own \
             measurement.warmup_runs/repetitions"
        )],
        raw_log_path: log_path,
        pass: all_pass,
        summary: if all_pass {
            format!(
                "internal assert_eq! against EXPECTED_FINAL=636658098 (main.rs:22,52-58) after \
                 1,000,000 calls passed in all 5 measured repetitions (1 warmup discarded); \
                 reference wall_seconds mean={mean:.4} samples={wall_seconds:?}"
            )
        } else {
            "at least one measured repetition exited non-zero (an internal assert_eq! failed or \
             the binary crashed)"
                .to_string()
        },
    })
}

/// M5-nim-entry-rust-lib: `fixtures/direct-native-link`. Bundles the
/// same 4 sub-experiments `.github/workflows/ci.yml` already runs on
/// every push (nim-c route only -- the nlvm route is a separately
/// tracked, known-unrelated-failure job, out of scope here): the
/// top-level `build.sh` (expects `result=43` and matching
/// `size=8 align=4` layout on both sides), `c-abi-baseline/build.sh`
/// (needs `cbindgen` on PATH), `panic-experiment` (expects the process
/// to abort with SIGABRT, exit code 134), and `callback-experiment`
/// (expects a clean exit code 1, an unhandled-exception report, not a
/// crash). `thread-experiment` is also run (exit 0 expected) as
/// additional D0-registered evidence even though the D0 case's own
/// `expected` field does not name a specific value for it.
pub fn run_m5_direct_native_link() -> Result<CaseEvidence, String> {
    let case_id = "M5-nim-entry-rust-lib";
    let mut all_pass = true;
    let mut summaries = Vec::new();
    let mut commands = Vec::new();
    let mut combined_log = String::new();

    let sub_cases: &[(&str, &str, &str)] = &[
        (
            "direct-route (result=43, layout)",
            "fixtures/direct-native-link",
            "cargo test --manifest-path rust-lib/Cargo.toml && ./build.sh",
        ),
        (
            "c-abi-baseline comparison",
            "fixtures/direct-native-link/c-abi-baseline",
            "./build.sh",
        ),
        (
            "panic-boundary (expect SIGABRT/134)",
            "fixtures/direct-native-link/panic-experiment",
            "cargo build --release --manifest-path ../rust-lib/Cargo.toml && \
             nim c --nimcache:nimcache --passL:\"-L../rust-lib/target/release -lrustlib\" \
             -o:main main.nim && \
             set +e; ./main; STATUS=$?; set -e; \
             if [ \"$STATUS\" -eq 134 ]; then echo 'PASS: SIGABRT (134) as expected'; \
             else echo \"FAIL: expected 134, got $STATUS\"; exit 1; fi",
        ),
        (
            "thread-boundary (expect clean exit)",
            "fixtures/direct-native-link/thread-experiment",
            "cargo build --release --manifest-path ../rust-lib/Cargo.toml && \
             nim c --threads:on --nimcache:nimcache \
             --passL:\"-L../rust-lib/target/release -lrustlib\" -o:main main.nim && ./main",
        ),
        (
            "callback-boundary (expect clean exit 1)",
            "fixtures/direct-native-link/callback-experiment",
            "cargo build --release --manifest-path ../rust-lib/Cargo.toml && \
             nim c --nimcache:nimcache --passL:\"-L../rust-lib/target/release -lrustlib\" \
             -o:main main.nim && \
             set +e; ./main; STATUS=$?; set -e; \
             if [ \"$STATUS\" -eq 1 ]; then echo 'PASS: clean exit 1 as expected'; \
             else echo \"FAIL: expected 1, got $STATUS\"; exit 1; fi",
        ),
    ];

    for (label, cwd, script) in sub_cases {
        let sub_case_id = format!(
            "{case_id}--{}",
            label.split_whitespace().next().unwrap_or(label)
        );
        let (pass, log) = run_and_log(&sub_case_id, cwd, script)?;
        combined_log.push_str(&log);
        combined_log.push_str("\n====\n");
        commands.push(format!("cd {cwd} && {script}"));
        if !pass {
            all_pass = false;
        }
        summaries.push(format!("{label}: {}", if pass { "PASS" } else { "FAIL" }));
    }

    // result=43 / layout evidence must actually be present in the
    // direct-route sub-case's own stdout, not merely a zero exit code --
    // grep it back out of the combined log rather than trusting exit
    // status alone (an unrelated echo could exit 0 without the real
    // number ever appearing).
    let has_result_43 = combined_log.contains("result=43");
    let has_layout_nim = combined_log.contains("layout: nim size=8 align=4");
    let has_layout_rust = combined_log.contains("layout: rust size=8 align=4");
    if !(has_result_43 && has_layout_nim && has_layout_rust) {
        all_pass = false;
        summaries.push(format!(
            "D0-pinned evidence missing from raw output: result=43={has_result_43} \
             layout_nim={has_layout_nim} layout_rust={has_layout_rust}"
        ));
    }

    let dir = runs_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
    let log_path = dir.join(format!("{case_id}.log"));
    std::fs::write(&log_path, &combined_log)
        .map_err(|e| format!("failed to write {}: {e}", log_path.display()))?;

    Ok(CaseEvidence {
        case_id: case_id.to_string(),
        execution_role: "reference".to_string(),
        reproduction_commands: commands,
        raw_log_path: log_path,
        pass: all_pass,
        summary: summaries.join(" | "),
    })
}

pub fn run_all() -> Result<Vec<CaseEvidence>, String> {
    Ok(vec![
        run_m4_scalar()?,
        run_m4_buffer()?,
        run_m4_callcount()?,
        run_m5_direct_native_link()?,
    ])
}
