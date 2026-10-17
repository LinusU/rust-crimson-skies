//! Task #1558: `cs_assets` is one integration-test binary.
//!
//! Runs the production `cs_xtask::test_layout` guard on the real workspace:
//! the crate must have migrated (`autotests = false`, one `[[test]]`), every
//! `.rs` file under `tests/` must be reached from `tests/main.rs`, and the
//! module tree must really contain this crate's former test files.

use std::path::Path;

use cs_xtask::test_layout;

fn workspace_root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

#[test]
fn accept_test_bin_rest_assets_crate_is_one_binary_with_no_stray_file() {
    let layout = test_layout::check_member(workspace_root(), "crates/cs_assets")
        .expect("the guard reads cs_assets' manifest");
    assert!(
        layout.findings.is_empty(),
        "cs_assets test layout findings: {:?}",
        layout.findings
    );
    let migrated = layout
        .migrated
        .expect("cs_assets sets autotests = false with one [[test]] target");
    assert!(
        migrated.main.ends_with("crates/cs_assets/tests/main.rs"),
        "{}",
        migrated.main
    );
    for needed in [
        "crates/cs_assets/tests/common/mod.rs",
        "crates/cs_assets/tests/accept_f02_b_sha256_hashing.rs",
        "crates/cs_assets/tests/accept_f15_b_atomic_cache.rs",
        "crates/cs_assets/tests/evidence_report_t693.rs",
    ] {
        assert!(
            migrated.reachable.iter().any(|f| f.ends_with(needed)),
            "{needed} is not reached from tests/main.rs: {:?}",
            migrated.reachable
        );
    }
}
