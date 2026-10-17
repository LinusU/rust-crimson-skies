//! Task #1556: `cs_content` has exactly one integration-test binary.
//!
//! `autotests = false` plus a single `[[test]]` target means every former
//! `tests/*.rs` file is a module of `tests/main.rs`; a file nobody declares
//! would silently run no test. This checks the manifest and the module
//! declarations directly, so reverting the migration fails it.

use std::fs;
use std::path::PathBuf;

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn accept_test_bin_rest_content_manifest_declares_exactly_the_main_target() {
    let manifest = fs::read_to_string(crate_dir().join("Cargo.toml")).expect("manifest reads");
    assert!(
        manifest.lines().any(|l| l.trim() == "autotests = false"),
        "autotests must be off so tests/*.rs are not separate binaries"
    );
    let targets = manifest.lines().filter(|l| l.trim() == "[[test]]").count();
    assert_eq!(targets, 1);
    assert!(manifest.contains("path = \"tests/main.rs\""));
}

#[test]
fn accept_test_bin_rest_content_every_test_file_is_a_module_of_main() {
    let tests = crate_dir().join("tests");
    let main = fs::read_to_string(tests.join("main.rs")).expect("tests/main.rs reads");
    let mut checked = 0;
    for entry in fs::read_dir(&tests).expect("tests/ lists") {
        let path = entry.expect("entry").path();
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("utf-8 name");
        if stem == "main" {
            continue;
        }
        let declared = main.lines().any(|l| l.trim() == format!("mod {stem};"));
        assert!(
            declared,
            "{} is not declared in tests/main.rs",
            path.display()
        );
        checked += 1;
    }
    assert!(checked > 100, "only {checked} test modules found");
}
