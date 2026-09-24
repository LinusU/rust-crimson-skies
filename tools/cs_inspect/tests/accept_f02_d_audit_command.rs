//! F02-D acceptance through the real `cs-inspect` binary: the `audit`
//! command wires production discovery, classification and the full-content
//! readiness check into its report consumer — exit 0 when the installation
//! passes, exit 3 when it does not, and every failure propagates as a
//! named, nonzero exit (docs/contracts/CLI-EVIDENCE.md).
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

/// A complete, fully classified fixture tree (the same layout the
/// readiness tests prove passes).
fn complete_tree(label: &str) -> TempTree {
    let tree = TempTree::new(label);
    tree.write("ZBD/planes.zbd", ARCHIVE_PAYLOAD);
    tree.write("ZBD/interp.zbd", ARCHIVE_PAYLOAD);
    for group in ["C1", "C1B", "C1C", "C2", "C2B", "C3", "C4", "C5"] {
        for name in ["cam_anim.zbd", "gamez.zbd", "texture.zbd", "zrdr.zbd"] {
            tree.write(&format!("ZBD/{group}/{name}"), ARCHIVE_PAYLOAD);
        }
        for name in ["mis_anim.zbd", "zrdr.zbd"] {
            tree.write(&format!("ZBD/{group}/M01/{name}"), ARCHIVE_PAYLOAD);
        }
    }
    tree.write("crimson.exe", b"authored fixture executable");
    tree.write("GOSDATA/ASSETS/crimson.rof", ARCHIVE_PAYLOAD);
    tree.write(
        "GOSDATA/ASSETS/GRAPHICS/MPG/chap0.mpg",
        b"authored fixture video",
    );
    tree
}

/// A ready installation exits 0, reports the final `--out` path on stderr
/// and writes the audit report atomically.
#[test]
fn accept_f02_d_audit_command_exits_zero_when_ready() {
    let tree = complete_tree("command-ready");
    let out = tree.root().join("audit.json");

    let output = cs_inspect()
        .arg("audit")
        .arg("--cs-path")
        .arg(tree.root())
        .arg("--scope")
        .arg("all")
        .arg("--strict")
        .arg("--out")
        .arg(&out)
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("cs-inspect runs");

    assert_eq!(
        output.status.code(),
        Some(0),
        "audit exits zero on a ready installation, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("wrote audit report to {}", out.display())),
        "the final --out path is reported on stderr, got: {stderr:?}"
    );
    let report = std::fs::read_to_string(&out).expect("the --out report exists");
    assert!(report.contains("\"report\": \"cs-inspect-audit/v1\""));
    assert!(report.contains("\"scope\": \"all\""));
    assert!(report.contains("\"strict\": true"));
    assert!(report.contains("\"full_content\": true"));
    assert!(report.contains("\"classes\": [\"full\"]"));
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

/// A partial installation exits 3 (failed validation): the report is still
/// produced and carries the exact failures — a nonzero exit is never a
/// logged success (CLI-EVIDENCE).
#[test]
fn accept_f02_d_audit_command_exits_three_when_not_ready() {
    let tree = TempTree::new("command-partial");
    tree.write("ZBD/planes.zbd", ARCHIVE_PAYLOAD);
    tree.write("ZBD/interp.zbd", ARCHIVE_PAYLOAD);
    tree.write("ZBD/C1/odd.dat", b"authored fixture blob");
    let out = tree.root().join("audit.json");

    let output = cs_inspect()
        .arg("audit")
        .arg("--cs-path")
        .arg(tree.root())
        .arg("--out")
        .arg(&out)
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("cs-inspect runs");

    assert_eq!(
        output.status.code(),
        Some(3),
        "a partial installation is exit 3 (failed validation), stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = std::fs::read_to_string(&out).expect("the report is still written");
    assert!(report.contains("\"full_content\": false"));
    assert!(report.contains("\"classes\": [\"partial\"]"));
    assert!(
        report.contains("expected archives unavailable"),
        "the unavailable failure is listed"
    );
    assert!(
        report.contains("unclassified gameplay files"),
        "the unclassified-gameplay failure is listed"
    );
    assert!(
        report.contains("\"zbd/c1/odd.dat\""),
        "the unclassified gameplay file is named"
    );
}

/// `--cs-path` wins over `CS_GAME_DIR` for the audit too, and `CS_GAME_DIR`
/// alone selects the installation when no flag is given.
#[test]
fn accept_f02_d_audit_cs_path_wins_over_cs_game_dir() {
    let flagged = TempTree::new("flagged");
    flagged.write("flagged-marker.dll", b"authored fixture library");
    let environ = TempTree::new("environ");
    environ.write("environ-marker.dll", b"authored fixture library");

    let output = cs_inspect()
        .arg("audit")
        .arg("--cs-path")
        .arg(flagged.root())
        .env("CS_GAME_DIR", environ.root())
        .output()
        .expect("cs-inspect runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("flagged-marker.dll"),
        "--cs-path selects the installation over CS_GAME_DIR"
    );
    assert!(!stdout.contains("environ-marker.dll"));

    let output = cs_inspect()
        .arg("audit")
        .env("CS_GAME_DIR", environ.root())
        .output()
        .expect("cs-inspect runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("environ-marker.dll"),
        "CS_GAME_DIR selects the installation without --cs-path"
    );
}

/// Every failure is a named, nonzero exit — never a zero after only
/// logging a failure (CLI-EVIDENCE).
#[test]
fn accept_f02_d_audit_failures_propagate_named_exit_codes() {
    let tree = complete_tree("command-failures");

    // No --cs-path and no CS_GAME_DIR: the retail capability is missing.
    let output = cs_inspect()
        .arg("audit")
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("cs-inspect runs");
    assert_eq!(
        output.status.code(),
        Some(4),
        "no installation is exit 4 (missing capability)"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("cs-inspect:"));

    // Malformed input: an unknown argument, a missing flag value and an
    // unsupported scope.
    for args in [
        vec!["audit", "--bogus", "x"],
        vec!["audit", "positional"],
        vec!["audit", "--cs-path"],
        vec!["audit", "--scope"],
        vec!["audit", "--scope", "zbd"],
        vec!["audit", "--strict", "yes"],
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

    // A missing installation root is a named discovery failure.
    let missing = tree.root().join("no-such-installation");
    let output = cs_inspect()
        .arg("audit")
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
        .arg("audit")
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
