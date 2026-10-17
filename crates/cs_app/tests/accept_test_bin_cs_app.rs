//! Task #1543: `cs_app` links one integration-test binary.
//!
//! The crate had 249 test files, each its own Bevy-linked executable that
//! macOS security-scans on first launch. It now sets `autotests = false` with
//! five `[[test]]` targets (one binary exceeds the 16 MiB `__eh_frame` limit
//! of task #336; the owner allowed a small fixed number), and these tests ask
//! the production guard (`cs_xtask::test_layout`, task #1542) about this very
//! crate: it is migrated, it declares exactly the five targets its manifest
//! allows, and every `.rs` file under `tests/` is reached from exactly one
//! target's `mod` tree, so none is silently left unrun or run twice. Removing
//! the `autotests = false` line, adding a fourth `[[test]]` or leaving a file
//! undeclared fails them.

use std::path::Path;

use cs_xtask::test_layout;

fn workspace_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn accept_test_bin_cs_app_the_crate_has_its_declared_test_targets() {
    let layout = test_layout::check_member(&workspace_root(), "crates/cs_app")
        .expect("the guard must be able to read cs_app's manifest");
    assert!(
        layout.findings.is_empty(),
        "cs_app's test layout is wrong: {:?}",
        layout.findings
    );
    let migrated = layout
        .migrated
        .expect("cs_app must set `autotests = false` with its `[[test]]` targets");
    assert_eq!(
        migrated.mains,
        [
            "crates/cs_app/tests/suite_a.rs",
            "crates/cs_app/tests/suite_b.rs",
            "crates/cs_app/tests/suite_c.rs",
            "crates/cs_app/tests/suite_d.rs",
            "crates/cs_app/tests/suite_e.rs",
        ]
    );
    // Every former test file really is reached, not merely none reported stray.
    assert!(
        migrated.reachable.len() > 200,
        "only {} files reached from the suite roots",
        migrated.reachable.len()
    );
}

#[test]
fn accept_test_bin_cs_app_the_whole_workspace_passes_the_guard() {
    let report = test_layout::check_workspace(&workspace_root())
        .expect("the guard must run on this workspace");
    assert!(report.is_clean(), "{:?}", report.lines());
}
