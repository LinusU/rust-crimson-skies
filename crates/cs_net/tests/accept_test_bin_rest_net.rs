//! Task #1561: `cs_net` has exactly one integration-test binary.
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
fn accept_test_bin_rest_net_manifest_declares_exactly_the_main_target() {
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
fn accept_test_bin_rest_net_every_test_file_is_a_module_of_main() {
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
    assert!(checked >= 19, "only {checked} test modules found");
}

/// The honest per-process fixes the merge into one binary required: the
/// loopback-socket tests that separate binaries serialized by `cargo test`'s
/// sequential execution now share one process-wide lock, and the F54-X10 fleet
/// driver's `--exact` child selection names `f54x10_probe` by its module path.
/// Reverting either fails this.
#[test]
fn accept_test_bin_rest_net_process_wide_fixes_hold() {
    let tests = crate_dir().join("tests");
    for file in [
        "accept_f54_b_pinned_transport.rs",
        "accept_f54_c_lifecycle.rs",
        "accept_f54_x10_loopback_loss.rs",
    ] {
        let source = fs::read_to_string(tests.join(file)).expect("the module source reads");
        assert!(
            !source.contains("static LOOPBACK"),
            "{file} still has its own LOOPBACK mutex, which cannot serialize \
             the other modules' loopback tests inside the one binary"
        );
        assert!(
            source.contains("support::loopback"),
            "{file} must take the shared loopback lock in crate::support"
        );
    }
    let fleet = fs::read_to_string(tests.join("accept_f54_x10_loopback_loss.rs"))
        .expect("the f54_x10 module reads");
    assert!(
        fleet.contains("\"accept_f54_x10_loopback_loss::f54x10_probe\""),
        "the fleet driver runs f54x10_probe in a child by --exact name; only \
         the module-qualified name selects it in the one binary"
    );
}
