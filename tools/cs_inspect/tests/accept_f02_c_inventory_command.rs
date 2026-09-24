//! F02-C acceptance through the real `cs-inspect` binary: the `inventory`
//! command wires production discovery into its report consumer, `--cs-path`
//! wins over `CS_GAME_DIR`, `--out` is written atomically and its final path
//! reported, and every failure propagates as a named, nonzero exit
//! (docs/contracts/CLI-EVIDENCE.md).
//!
//! These tests run the shipped binary (`CARGO_BIN_EXE_cs-inspect`) on newly
//! authored fixture trees; they prove nothing about retail installations
//! and never touch `$CS_GAME_DIR`.

mod common;

use std::process::Command;

use common::TempTree;

const ARCHIVE_PAYLOAD: &[u8] = b"authored fixture archive payload";

/// The shipped binary under test.
fn cs_inspect() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cs-inspect"))
}

/// A small complete-enough fixture tree for the command tests.
fn install_tree(label: &str) -> TempTree {
    let tree = TempTree::new(label);
    tree.write("ZBD/planes.zbd", ARCHIVE_PAYLOAD);
    tree.write("ZBD/interp.zbd", ARCHIVE_PAYLOAD);
    tree.write("ZBD/C1/gamez.zbd", ARCHIVE_PAYLOAD);
    tree
}

/// The report is produced by real discovery: the written file carries the
/// tree's inventoried rows, and the unavailable expected archives of this
/// partial tree stay visible. Success reports the final `--out` path.
#[test]
fn accept_f02_c_inventory_command_writes_the_report() {
    let tree = install_tree("command");
    let out = tree.root().join("inventory.json");

    let output = cs_inspect()
        .arg("inventory")
        .arg("--cs-path")
        .arg(tree.root())
        .arg("--out")
        .arg(&out)
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("cs-inspect runs");

    assert_eq!(
        output.status.code(),
        Some(0),
        "inventory exits zero on success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("wrote inventory report to {}", out.display())),
        "the final --out path is reported on stderr, got: {stderr:?}"
    );
    let report = std::fs::read_to_string(&out).expect("the --out report exists");
    assert!(report.contains("\"report\": \"cs-inspect-inventory/v1\""));
    assert!(report.contains("\"files\": 3"), "the three fixture files");
    assert!(
        report.contains("\"ZBD/C1/gamez.zbd\""),
        "inventoried rows keep their preserved spelling"
    );
    // ZBD/C1 is missing cam_anim/texture/zrdr here: visible, counted.
    assert!(
        report.contains("\"available\": false"),
        "missing expected archives stay visible in the report"
    );
    assert!(
        !report.contains("\"unavailable\": 0"),
        "a partial tree cannot report zero unavailable archives"
    );
    // The atomic write leaves no temporary sibling behind.
    let leftovers: Vec<_> = std::fs::read_dir(tree.root())
        .expect("the fixture root reads")
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp-"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "the atomic write cleans up its temporary file"
    );
}

/// Without `--out` the report goes to stdout; human output stays on stderr.
#[test]
fn accept_f02_c_inventory_command_writes_json_to_stdout() {
    let tree = install_tree("stdout");
    let output = cs_inspect()
        .arg("inventory")
        .arg("--cs-path")
        .arg(tree.root())
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("cs-inspect runs");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"report\": \"cs-inspect-inventory/v1\""),
        "stdout carries the JSON report, got: {stdout:?}"
    );
}

/// `--cs-path` wins over `CS_GAME_DIR` (spec F02 "Deliverable and
/// interfaces"), and `CS_GAME_DIR` alone selects the installation when no
/// flag is given.
#[test]
fn accept_f02_c_cs_path_wins_over_cs_game_dir() {
    let flagged = TempTree::new("flagged");
    flagged.write("flagged-marker.bin", ARCHIVE_PAYLOAD);
    let environ = TempTree::new("environ");
    environ.write("environ-marker.bin", ARCHIVE_PAYLOAD);

    // The explicit flag wins: the report covers the flagged tree.
    let output = cs_inspect()
        .arg("inventory")
        .arg("--cs-path")
        .arg(flagged.root())
        .env("CS_GAME_DIR", environ.root())
        .output()
        .expect("cs-inspect runs");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("flagged-marker.bin"),
        "--cs-path selects the installation over CS_GAME_DIR"
    );
    assert!(!stdout.contains("environ-marker.bin"));

    // Without the flag the environment selects the installation.
    let output = cs_inspect()
        .arg("inventory")
        .env("CS_GAME_DIR", environ.root())
        .output()
        .expect("cs-inspect runs");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("environ-marker.bin"),
        "CS_GAME_DIR selects the installation without --cs-path"
    );
}

/// Every failure is a named, nonzero exit — never a zero after only
/// logging a failure (CLI-EVIDENCE).
#[test]
fn accept_f02_c_inventory_failures_propagate_named_exit_codes() {
    let tree = install_tree("failures");

    // No --cs-path and no CS_GAME_DIR: the retail capability is missing.
    let output = cs_inspect()
        .arg("inventory")
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("cs-inspect runs");
    assert_eq!(
        output.status.code(),
        Some(4),
        "no installation is exit 4 (missing capability)"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("cs-inspect:"));

    // Malformed input: an unknown argument and a missing flag value.
    for args in [
        vec!["inventory", "--bogus", "x"],
        vec!["inventory", "positional"],
        vec!["inventory", "--cs-path"],
    ] {
        let output = cs_inspect()
            .args(&args)
            .env_remove("CS_GAME_DIR")
            .output()
            .expect("cs-inspect runs");
        assert_eq!(
            output.status.code(),
            Some(2),
            "{args:?} is exit 2 (invalid input), stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // A missing installation root is a named discovery failure, not an
    // empty report.
    let missing = tree.root().join("no-such-installation");
    let output = cs_inspect()
        .arg("inventory")
        .arg("--cs-path")
        .arg(&missing)
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("cs-inspect runs");
    assert_eq!(
        output.status.code(),
        Some(1),
        "an unreadable root is a runtime failure"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&missing.display().to_string()),
        "the failure names the path it happened at, got: {stderr:?}"
    );

    // An --out path whose parent does not exist is an output failure, and
    // no partial report is left behind.
    let out = missing.join("report.json");
    let output = cs_inspect()
        .arg("inventory")
        .arg("--cs-path")
        .arg(tree.root())
        .arg("--out")
        .arg(&out)
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("cs-inspect runs");
    assert_eq!(output.status.code(), Some(1));
    assert!(!out.exists(), "a failed write leaves no report");
}
