//! Issue #48 (G1) Checkpoint D verification-gate item 6 ("process-tree
//! observation"): direct, real evidence that no process named `nimble`
//! ever appears anywhere in the subprocess tree a real production G1
//! ingestion run spawns. An integration test (not a unit test inside
//! `src/cross_ecosystem_ingest.rs`) because `CARGO_BIN_EXE_*` -- needed
//! to locate the compiled `laminaria-nimble-shadow-probe` helper binary
//! (`src/bin/nimble_shadow_probe.rs`) -- is only defined for
//! integration test targets, not for this crate's own `src/` unit-test
//! binary.
//!
//! Unix-only, matching every other real-subprocess-spawning test in
//! this crate (`#[cfg(all(test, unix))]` in `cross_ecosystem_ingest.rs`):
//! the shadow `nimble` stand-in is a `#!/bin/sh` script.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;

#[test]
fn a_real_production_run_never_invokes_a_shadowed_nimble_binary() {
    let shadow_dir = std::env::temp_dir().join(format!(
        "laminaria-g1-checkpoint-d-nimble-shadow-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&shadow_dir).expect("must create shadow bin dir");
    let sentinel = shadow_dir.join(".nimble_was_invoked");
    let shadow_nimble = shadow_dir.join("nimble");
    std::fs::write(
        &shadow_nimble,
        format!(
            "#!/bin/sh\ntouch \"{}\"\nexit 1\n",
            sentinel.to_string_lossy()
        ),
    )
    .expect("must write shadow nimble script");
    let mut perms = std::fs::metadata(&shadow_nimble)
        .expect("must stat shadow nimble script")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&shadow_nimble, perms).expect("must chmod shadow nimble script");

    let probe_exe = env!("CARGO_BIN_EXE_laminaria-nimble-shadow-probe");
    let output = std::process::Command::new(probe_exe)
        .arg(&shadow_dir)
        .output()
        .expect("failed to spawn the shadow probe process");
    assert!(
        output.status.success(),
        "the real production ingestion run inside the probe must itself succeed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !sentinel.exists(),
        "the shadowed nimble binary was invoked during a real production ingestion run"
    );

    let _ = std::fs::remove_dir_all(&shadow_dir);
}
