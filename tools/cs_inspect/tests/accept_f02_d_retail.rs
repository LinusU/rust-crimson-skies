//! F02-D retail acceptance (capability `retail`, requires CS_GAME_DIR):
//! the `audit` command runs over the owner's original installation and
//! classifies every one of its files — zero unclassified gameplay files —
//! while the full-content readiness check passes.
//!
//! The role accounting is cross-checked against this test's own
//! classification of the same tree, not taken from the implementation:
//! the test re-derives which files are zbd archives, ROF containers, TGA
//! images, MPG videos and native binaries by its own reads. The test fails
//! loudly when CS_GAME_DIR is missing; it never writes inside the
//! installation.

mod common;

use std::path::PathBuf;
use std::process::Command;

use common::TempTree;
use cs_assets::install::{FileRoleKind, discover, fingerprint};
use cs_inspect::install::{audit_report_json, full_content_readiness, install_audit};

/// The read-only original installation under test.
fn game_dir() -> PathBuf {
    let value = std::env::var_os("CS_GAME_DIR").expect(
        "CS_GAME_DIR must point at the read-only original installation (capability `retail`)",
    );
    assert!(!value.is_empty(), "CS_GAME_DIR must not be empty");
    PathBuf::from(value)
}

/// This test's own role derivation over the retail tree — deliberately not
/// shared with the production rules. Returns `(platform_support,
/// needed_unimplemented, optional_media)` counts.
fn expected_role_counts(root: &std::path::Path) -> (usize, usize, usize) {
    let mut platform = 0;
    let mut needed = 0;
    let mut optional = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).expect("a directory reads") {
            let entry = entry.expect("directory entries read");
            let file_type = entry.file_type().expect("entry types read");
            if file_type.is_dir() {
                stack.push(entry.path());
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(root)
                .expect("files sit under the root")
                .to_string_lossy()
                .replace('\\', "/")
                .to_ascii_lowercase();
            let name = relative.rsplit('/').next().unwrap_or(&relative);
            let extension = name.rsplit('.').next().unwrap_or("");
            if matches!(extension, "dll" | "exe" | "icd")
                || matches!(name, "00000409.016" | "00000409.256" | "ebusetup.sem")
                || extension == "rtf"
            {
                platform += 1;
            } else if (relative.starts_with("zbd/") && extension == "zbd")
                || (relative.starts_with("gosdata/assets/") && extension == "rof")
                || (relative.starts_with("gosdata/assets/graphics/") && extension == "tga")
                || name == "crimsonff.ifr"
            {
                needed += 1;
            } else if relative.starts_with("gosdata/assets/graphics/mpg/") && extension == "mpg" {
                optional += 1;
            } else {
                panic!("the retail file {relative} matched no role bucket in this test");
            }
        }
    }
    (platform, needed, optional)
}

#[ignore = "requires CS_GAME_DIR"]
#[test]
fn accept_f02_d_retail_audit_classifies_every_file_and_passes_readiness() {
    let root = game_dir();
    let found = discover(&root).expect("production discovery reads the original installation");
    let audit = install_audit(&found);

    // The audit covers every inventoried file, and every file is
    // classified: zero unclassified gameplay files is the task's core
    // claim, checked both ways.
    assert_eq!(audit.findings.len(), found.manifest.files.len());
    assert_eq!(
        audit.role_count(FileRoleKind::Unknown),
        0,
        "every retail file is classified — zero unclassified gameplay files"
    );
    assert!(audit.unclassified_gameplay().is_empty());
    assert!(audit.unclassified_other().is_empty());

    // The production role accounting matches this test's independent
    // derivation over the same tree.
    let (platform, needed, optional) = expected_role_counts(&root);
    assert_eq!(
        audit.role_count(FileRoleKind::PlatformSupport),
        platform,
        "platform-support count matches the test's own derivation"
    );
    assert_eq!(
        audit.role_count(FileRoleKind::NeededUnimplemented),
        needed,
        "needed-unimplemented count matches the test's own derivation"
    );
    assert_eq!(
        audit.role_count(FileRoleKind::OptionalMedia),
        optional,
        "optional-media count matches the test's own derivation"
    );
    assert!(
        needed > 0 && optional > 0 && platform > 0,
        "the retail tree really contains classified content of each kind"
    );

    // Full-content readiness passes strictly: every expected archive is
    // available and nothing is unclassified.
    let strict = full_content_readiness(&audit, true);
    assert!(
        strict.full_content_ready,
        "the retail installation passes strict full-content readiness: {:?}",
        strict.failures
    );
    assert_eq!(audit.impact.unavailable_count(), 0);

    // The library report carries the production fingerprints of the actual
    // bytes and the readiness verdict.
    let report = audit_report_json(&found, &audit, true);
    assert!(report.contains(&format!(
        "\"install_sha256\": \"{}\"",
        fingerprint(&found.manifest).to_hex()
    )));
    assert!(report.contains("\"full_content\": true"));
    assert!(report.contains("\"classes\": [\"full\"]"));
    assert!(report.contains("\"unknown\": 0"));
    assert!(!report.contains("\"kind\": \"unknown\""));

    // The real binary produces the same verdict end to end: exit zero
    // under --scope all --strict, the --out report exists and it carries
    // the retail accounting.
    let scratch = TempTree::new("retail-out");
    let out = scratch.root().join("audit.json");
    let output = Command::new(env!("CARGO_BIN_EXE_cs-inspect"))
        .arg("audit")
        .arg("--cs-path")
        .arg(&root)
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
        "the audit command exits zero on the retail installation, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cli_report = std::fs::read_to_string(&out).expect("the --out report exists");
    assert_eq!(
        cli_report.trim_end(),
        report.trim_end(),
        "the command's report is the library-rendered report"
    );
}
