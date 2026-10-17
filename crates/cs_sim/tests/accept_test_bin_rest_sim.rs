//! Task #1557: `cs_sim`'s integration suite compiles as one binary.
//!
//! The crate sets `autotests = false` and declares exactly one `[[test]]`
//! target (`cs_sim_tests` at `tests/main.rs`); every former `tests/*.rs`
//! target — 64 files — and the former `tests/ai/main.rs` directory target is
//! a `mod` of that one binary, so cargo links one executable instead of 65
//! and the owner's laptop pays one ~50-70 s macOS XProtect scan instead of
//! one per binary (task #1542's finding).
//!
//! This test asserts the layout through the guard's own library function,
//! `cs_xtask::test_layout::check_member` — the same check `cs_xtask
//! test-layout` runs from the command line and `accept_test_bin_guard_` runs
//! against this workspace inside CI's `cargo test --workspace`:
//!
//! * the member's single target is `tests/main.rs`, so removing the
//!   `autotests = false` / `[[test]]` pair from the manifest fails here;
//! * every `.rs` file under `tests/` — this file included — is reached by a
//!   `mod` declaration, so dropping a module from `tests/main.rs` fails here;
//! * the reachable set is exactly the sources on disk, so the pass is not
//!   vacuous.

use std::fs;
use std::path::{Path, PathBuf};

use cs_xtask::test_layout;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every `.rs` file under `dir`, recursively — the same walk the guard's
/// stray-file check performs, counted independently so the assertion below
/// does not simply restate the code under test.
fn count_rs_sources(dir: &Path) -> usize {
    let mut count = 0;
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(&current)
            .unwrap_or_else(|error| panic!("{current:?} must be readable: {error}"))
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension() == Some("rs".as_ref()) {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn accept_test_bin_rest_sim_cs_sim_links_one_test_binary_that_reaches_every_source() {
    let layout = test_layout::check_member(&workspace_root(), "crates/cs_sim")
        .expect("the guard must be able to read this workspace member");

    assert!(
        layout.findings.is_empty(),
        "cs_sim's single-binary test layout must be clean, got {:?}",
        layout.findings
    );
    let migrated = layout.migrated.expect(
        "cs_sim must be a migrated crate (autotests = false plus one [[test]] target), \
         or this test proves nothing",
    );
    assert_eq!(
        migrated.main, "crates/cs_sim/tests/main.rs",
        "the crate's single test binary is its declared [[test]] target"
    );

    let sources = count_rs_sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests"));
    assert!(
        sources > 1,
        "the crate must have test sources for this check to mean anything"
    );
    assert_eq!(
        migrated.reachable.len(),
        sources,
        "every `.rs` file under crates/cs_sim/tests/ must be reached from tests/main.rs, \
         got {:?}",
        migrated.reachable
    );
}
