//! Task #1544: the seven crates after `cs_types` each link one integration-test binary.
//!
//! `cs_content`, `cs_sim`, `cs_assets`, `cs_script`, `cs_formats`, `cs_net` and
//! `cs_xtask` were migrated by #1556-#1562. This is the integration check: every one of them is
//! a migrated crate whose single test target is its `tests/main.rs`, and the guard finds no
//! unreached `.rs` file anywhere in the workspace. It goes through the guard's own library
//! functions, so un-migrating a crate or orphaning a test file fails here.

use std::path::{Path, PathBuf};

use cs_xtask::test_layout;

const MIGRATED: [&str; 7] = [
    "crates/cs_content",
    "crates/cs_sim",
    "crates/cs_assets",
    "crates/cs_script",
    "crates/cs_formats",
    "crates/cs_net",
    "tools/cs_xtask",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn accept_test_bin_rest_each_remaining_crate_links_one_test_binary() {
    let _tree = crate::accept_test_bin_guard::real_tree_lock();
    for member in MIGRATED {
        let layout = test_layout::check_member(&workspace_root(), member)
            .unwrap_or_else(|error| panic!("{member} must be readable by the guard: {error:?}"));
        assert!(
            layout.findings.is_empty(),
            "{member}'s single-binary test layout must be clean, got {:?}",
            layout.findings
        );
        let migrated = layout
            .migrated
            .unwrap_or_else(|| panic!("{member} must be migrated (autotests = false)"));
        assert_eq!(
            migrated.main,
            format!("{member}/tests/main.rs"),
            "{member}'s single test binary is its declared [[test]] target"
        );
        assert!(
            migrated.reachable.len() > 1,
            "{member} must have test modules reached from tests/main.rs"
        );
    }
}

#[test]
fn accept_test_bin_rest_workspace_guard_reports_no_stray_test_file() {
    let _tree = crate::accept_test_bin_guard::real_tree_lock();
    let report = test_layout::check_workspace(&workspace_root())
        .expect("the guard must be able to read this workspace");
    assert!(report.is_clean(), "guard findings: {:?}", report.lines());
    for member in MIGRATED {
        assert!(
            report.migrated.iter().any(|m| m.member == member),
            "{member} must be among the migrated members"
        );
    }
}
