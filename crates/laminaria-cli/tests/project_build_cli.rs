//! CLI-level integration tests for `laminaria plan-build`/`build`
//! (issue #26): proves the real planner + real executor path end to end
//! through the actual CLI binary and its `--json` envelope, not just
//! argument parsing or a mocked planner. `#[cfg(unix)]` throughout,
//! matching every other real-toolchain-invoking test in this workspace --
//! the `windows` CI job deliberately never provisions Nim.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn laminaria_bin() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_laminaria"))
}

/// Builds `nim-planner/bin/laminaria-planner` via `nim c` directly, once
/// per test binary process -- same `OnceLock` pattern used throughout
/// `laminaria-run`'s own tests, for the same fresh-checkout race reason.
fn stage0_planner() -> PathBuf {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT
        .get_or_init(|| {
            let repo_root = repo_root();
            let status = std::process::Command::new("nim")
                .args([
                    "c",
                    "--path:src",
                    "-o:bin/laminaria-planner",
                    "src/laminaria_planner.nim",
                ])
                .current_dir(repo_root.join("nim-planner"))
                .status()
                .expect("failed to invoke nim -- is Nim installed?");
            assert!(
                status.success(),
                "stage0 nim c build of laminaria-planner failed"
            );
            repo_root.join("nim-planner/bin/laminaria-planner")
        })
        .clone()
}

fn tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "laminaria-cli-project-build-test-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn parse_json_stdout(output: &std::process::Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "expected valid JSON on stdout, got error {e}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn plan_build_and_build_succeed_end_to_end_for_a_rust_only_fixture() {
    let repo_root = repo_root();
    let planner = stage0_planner();
    let project_root = repo_root.join("fixtures/rust-heavy-workspace");

    let plan_output = std::process::Command::new(laminaria_bin())
        .args(["plan-build", "--project-root"])
        .arg(&project_root)
        .args(["--planner"])
        .arg(&planner)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        plan_output.status.success(),
        "plan-build failed: {plan_output:?}"
    );
    let plan_json = parse_json_stdout(&plan_output);
    assert_eq!(plan_json["ok"], true);
    assert_eq!(
        plan_json["result"]["ordered_actions"],
        serde_json::json!(["compile-rust-project"])
    );

    let generation_root = tmp_dir("rust-only-build-gen");
    let runs_root = tmp_dir("rust-only-build-runs");
    let build_output = std::process::Command::new(laminaria_bin())
        .args(["build", "--project-root"])
        .arg(&project_root)
        .args(["--generation-root"])
        .arg(&generation_root)
        .args(["--runs-root"])
        .arg(&runs_root)
        .args(["--lock"])
        .arg(repo_root.join("toolchains.lock.toml"))
        .args(["--planner"])
        .arg(&planner)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        build_output.status.success(),
        "build failed: {build_output:?}"
    );
    let build_json = parse_json_stdout(&build_output);
    assert_eq!(build_json["ok"], true);
    let actions = build_json["result"]["actions"].as_array().unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0]["succeeded"], true);
    let artifacts = actions[0]["artifacts"].as_array().unwrap();
    assert!(
        !artifacts.is_empty(),
        "expected at least one reported artifact"
    );
    for artifact in artifacts {
        let path = Path::new(artifact.as_str().unwrap());
        assert!(
            path.is_file(),
            "reported artifact {} does not exist on disk",
            path.display()
        );
    }

    let _ = std::fs::remove_dir_all(&generation_root);
    let _ = std::fs::remove_dir_all(&runs_root);
}

#[test]
fn plan_build_and_build_succeed_end_to_end_for_a_nim_only_fixture() {
    let repo_root = repo_root();
    let planner = stage0_planner();
    let project_root = repo_root.join("fixtures/nim-heavy-workspace");

    let plan_output = std::process::Command::new(laminaria_bin())
        .args(["plan-build", "--project-root"])
        .arg(&project_root)
        .args(["--planner"])
        .arg(&planner)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        plan_output.status.success(),
        "plan-build failed: {plan_output:?}"
    );

    let generation_root = tmp_dir("nim-only-build-gen");
    let runs_root = tmp_dir("nim-only-build-runs");
    let build_output = std::process::Command::new(laminaria_bin())
        .args(["build", "--project-root"])
        .arg(&project_root)
        .args(["--generation-root"])
        .arg(&generation_root)
        .args(["--runs-root"])
        .arg(&runs_root)
        .args(["--lock"])
        .arg(repo_root.join("toolchains.lock.toml"))
        .args(["--planner"])
        .arg(&planner)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        build_output.status.success(),
        "build failed: {build_output:?}"
    );
    let build_json = parse_json_stdout(&build_output);
    assert_eq!(build_json["ok"], true);
    let actions = build_json["result"]["actions"].as_array().unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0]["succeeded"], true);
    let artifacts = actions[0]["artifacts"].as_array().unwrap();
    assert_eq!(artifacts.len(), 1);
    let path = Path::new(artifacts[0].as_str().unwrap());
    assert!(
        path.is_file(),
        "reported artifact {} does not exist on disk",
        path.display()
    );

    let _ = std::fs::remove_dir_all(&generation_root);
    let _ = std::fs::remove_dir_all(&runs_root);
}

/// `--json` failures must be parseable JSON on stdout, not stderr prose --
/// the exact gap noted against `self_build_command`'s own JSON path.
#[test]
fn build_reports_an_ambiguous_project_as_a_structured_json_error_not_stderr_prose() {
    let planner = stage0_planner();
    let dir = tmp_dir("ambiguous-cli");
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("thing.nimble"), "").unwrap();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/thing.nim"), "echo \"hi\"\n").unwrap();

    let output = std::process::Command::new(laminaria_bin())
        .args(["plan-build", "--project-root"])
        .arg(&dir)
        .args(["--planner"])
        .arg(&planner)
        .arg("--json")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let json = parse_json_stdout(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["error_kind"], "ambiguous_project");
    assert!(json["detail"].as_str().unwrap().contains("--requires"));

    let _ = std::fs::remove_dir_all(&dir);
}

/// Adversarial proof that an unneeded compiler is never invoked, not just
/// "happens not to be found": a sentinel `nim`/`nimble` script (which
/// would prove itself invoked by writing a marker file) is placed first
/// on this **child process's own** `PATH` -- never this test process's
/// ambient environment, so there is no race with other parallel tests --
/// and a lock file whose Nim entry has no `bin_dir` (forcing a PATH
/// lookup if resolution were ever attempted) is used instead of the
/// repository's own. A Rust-only build must succeed without ever running
/// the sentinel.
#[test]
fn build_never_invokes_an_unneeded_nim_even_when_one_is_first_on_path() {
    let repo_root = repo_root();
    let planner = stage0_planner();
    let project_root = repo_root.join("fixtures/rust-heavy-workspace");

    let sentinel_dir = tmp_dir("sentinel-bin");
    let marker_path = tmp_dir("sentinel-marker").join("invoked");
    for name in ["nim", "nimble"] {
        let script_path = sentinel_dir.join(name);
        std::fs::write(
            &script_path,
            format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker_path.display()),
        )
        .unwrap();
        let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(&script_path, perms).unwrap();
    }

    let lock_dir = tmp_dir("no-bin-dir-lock");
    let real_lock = std::fs::read_to_string(repo_root.join("toolchains.lock.toml")).unwrap();
    // Drop nim2_pinned's bin_dir line so a regression that resolved Nim
    // unconditionally would fall through to a bare PATH lookup -- which
    // the sentinel placed first on PATH below would catch.
    let lock_without_bin_dir: String = real_lock
        .lines()
        .filter(|line| !line.trim_start().starts_with("bin_dir"))
        .collect::<Vec<_>>()
        .join("\n");
    let lock_path = lock_dir.join("toolchains.lock.toml");
    std::fs::write(&lock_path, lock_without_bin_dir).unwrap();

    let original_path = std::env::var("PATH").unwrap_or_default();
    let sandboxed_path = format!("{}:{original_path}", sentinel_dir.display());

    let generation_root = tmp_dir("sentinel-gen");
    let runs_root = tmp_dir("sentinel-runs");
    let output = std::process::Command::new(laminaria_bin())
        .env("PATH", &sandboxed_path)
        .args(["build", "--project-root"])
        .arg(&project_root)
        .args(["--generation-root"])
        .arg(&generation_root)
        .args(["--runs-root"])
        .arg(&runs_root)
        .args(["--lock"])
        .arg(&lock_path)
        .args(["--planner"])
        .arg(&planner)
        .arg("--json")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "build should succeed without touching the sentinel Nim: {output:?}"
    );
    assert!(
        !marker_path.exists(),
        "the sentinel nim/nimble was invoked even though this is a Rust-only build"
    );

    let _ = std::fs::remove_dir_all(&sentinel_dir);
    let _ = std::fs::remove_dir_all(&lock_dir);
    let _ = std::fs::remove_dir_all(&generation_root);
    let _ = std::fs::remove_dir_all(&runs_root);
    let _ = std::fs::remove_dir_all(marker_path.parent().unwrap());
}

/// The Nim-only mirror of `build_never_invokes_an_unneeded_nim_even_when_one_is_first_on_path`,
/// added per review: #26's completion criterion is symmetric ("no unused
/// compiler invocation" for *either* direction), and only the Rust-only
/// side had an adversarial proof before this. A sentinel `rustc`/`cargo`/
/// `rustup` (covering both the managed-toolchain and the plain-PATH
/// resolution mechanisms `rust_toolchain::resolve` can take) is placed
/// first on this child process's own `PATH`; a Nim-only build must
/// succeed without ever running any of them.
#[test]
fn build_never_invokes_an_unneeded_rust_toolchain_even_when_one_is_first_on_path() {
    let repo_root = repo_root();
    let planner = stage0_planner();
    let project_root = repo_root.join("fixtures/nim-heavy-workspace");

    let sentinel_dir = tmp_dir("sentinel-rust-bin");
    let marker_path = tmp_dir("sentinel-rust-marker").join("invoked");
    for name in ["rustc", "cargo", "rustup"] {
        let script_path = sentinel_dir.join(name);
        std::fs::write(
            &script_path,
            format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker_path.display()),
        )
        .unwrap();
        let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(&script_path, perms).unwrap();
    }

    let original_path = std::env::var("PATH").unwrap_or_default();
    let sandboxed_path = format!("{}:{original_path}", sentinel_dir.display());

    let generation_root = tmp_dir("sentinel-rust-gen");
    let runs_root = tmp_dir("sentinel-rust-runs");
    let output = std::process::Command::new(laminaria_bin())
        .env("PATH", &sandboxed_path)
        .args(["build", "--project-root"])
        .arg(&project_root)
        .args(["--generation-root"])
        .arg(&generation_root)
        .args(["--runs-root"])
        .arg(&runs_root)
        .args(["--lock"])
        .arg(repo_root.join("toolchains.lock.toml"))
        .args(["--planner"])
        .arg(&planner)
        .arg("--json")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "build should succeed without touching the sentinel Rust toolchain: {output:?}"
    );
    assert!(
        !marker_path.exists(),
        "the sentinel rustc/cargo/rustup was invoked even though this is a Nim-only build"
    );

    let _ = std::fs::remove_dir_all(&sentinel_dir);
    let _ = std::fs::remove_dir_all(&generation_root);
    let _ = std::fs::remove_dir_all(&runs_root);
    let _ = std::fs::remove_dir_all(marker_path.parent().unwrap());
}

/// Regression test for the exact bug an external review reproduced:
/// `plan-build` left `--project-root` relative while `build` absolutized
/// it internally, so the same logical project (invoked with a relative
/// `--project-root` from the same cwd) produced two *different*
/// `plan_id`s depending on which command computed it.
#[test]
fn plan_build_and_build_compute_the_same_plan_id_for_a_relative_project_root() {
    let repo_root = repo_root();
    let planner = stage0_planner();

    let plan_output = std::process::Command::new(laminaria_bin())
        .current_dir(&repo_root)
        .args([
            "plan-build",
            "--project-root",
            "fixtures/rust-heavy-workspace",
        ])
        .args(["--planner"])
        .arg(&planner)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        plan_output.status.success(),
        "plan-build failed: {plan_output:?}"
    );
    let plan_json = parse_json_stdout(&plan_output);
    let plan_id_from_plan_build = plan_json["result"]["plan_id"].as_str().unwrap().to_string();

    let generation_root = tmp_dir("plan-id-parity-gen");
    let runs_root = tmp_dir("plan-id-parity-runs");
    let build_output = std::process::Command::new(laminaria_bin())
        .current_dir(&repo_root)
        .args(["build", "--project-root", "fixtures/rust-heavy-workspace"])
        .args(["--generation-root"])
        .arg(&generation_root)
        .args(["--runs-root"])
        .arg(&runs_root)
        .args(["--planner"])
        .arg(&planner)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        build_output.status.success(),
        "build failed: {build_output:?}"
    );
    let build_json = parse_json_stdout(&build_output);
    let plan_id_from_build = build_json["result"]["plan_id"]
        .as_str()
        .unwrap()
        .to_string();

    assert_eq!(
        plan_id_from_plan_build, plan_id_from_build,
        "plan-build and build must compute the identical plan_id for the same logical project \
         regardless of --project-root being given as a relative path"
    );

    let _ = std::fs::remove_dir_all(&generation_root);
    let _ = std::fs::remove_dir_all(&runs_root);
}
