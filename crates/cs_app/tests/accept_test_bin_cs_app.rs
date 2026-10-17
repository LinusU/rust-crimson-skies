//! Task #1543: `cs_app` links one integration-test binary.
//!
//! The crate had 249 test files, each its own Bevy-linked executable that
//! macOS security-scans on first launch. It now sets `autotests = false` with a
//! single `[[test]]` target, and these tests ask the production guard
//! (`cs_xtask::test_layout`, task #1542) about this very crate: it is migrated,
//! it declares exactly one target, and every `.rs` file under `tests/` is
//! reached from that target's `mod` tree, so none is silently left unrun.
//! Removing the `autotests = false` line, adding a second `[[test]]` or leaving
//! a file undeclared fails them.

use std::path::Path;

use cs_xtask::test_layout;

fn workspace_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn accept_test_bin_cs_app_the_crate_has_a_single_test_target() {
    let layout = test_layout::check_member(&workspace_root(), "crates/cs_app")
        .expect("the guard must be able to read cs_app's manifest");
    assert!(
        layout.findings.is_empty(),
        "cs_app's test layout is wrong: {:?}",
        layout.findings
    );
    let migrated = layout
        .migrated
        .expect("cs_app must set `autotests = false` with one `[[test]]` target");
    assert_eq!(migrated.main, "crates/cs_app/tests/main.rs");
    // Every former test file really is reached, not merely none reported stray.
    assert!(
        migrated.reachable.len() > 200,
        "only {} files reached from tests/main.rs",
        migrated.reachable.len()
    );
}

#[test]
fn accept_test_bin_cs_app_the_whole_workspace_passes_the_guard() {
    let report = test_layout::check_workspace(&workspace_root())
        .expect("the guard must run on this workspace");
    assert!(report.is_clean(), "{:?}", report.lines());
}
