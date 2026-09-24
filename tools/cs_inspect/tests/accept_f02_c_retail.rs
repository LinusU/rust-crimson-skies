//! F02-C retail acceptance (capability `retail`, requires CS_GAME_DIR):
//! the `inventory` command and the dependency-impact report run over the
//! owner's original installation. The report is cross-checked against
//! independent direct reads of the same tree: the expected world-group and
//! mission-directory counts are re-derived by this test, not taken from the
//! implementation.
//!
//! The test fails loudly when CS_GAME_DIR is missing; it never writes
//! inside the installation.

mod common;

use std::path::PathBuf;
use std::process::Command;

use common::TempTree;
use cs_assets::install::{REFERENCE_WORLD_GROUP_LEADS, discover, fingerprint};
use cs_inspect::install::{dependency_impact, inventory_report_json};

/// The read-only original installation under test.
fn game_dir() -> PathBuf {
    let value = std::env::var_os("CS_GAME_DIR").expect(
        "CS_GAME_DIR must point at the read-only original installation (capability `retail`)",
    );
    assert!(!value.is_empty(), "CS_GAME_DIR must not be empty");
    PathBuf::from(value)
}

/// The mission directories of one world group, counted by this test's own
/// directory reads — every directory directly under the group directory.
fn observed_mission_dirs(zbd: &std::path::Path, group: &str) -> usize {
    std::fs::read_dir(zbd.join(group))
        .expect("the world group reads")
        .filter(|entry| {
            entry
                .as_ref()
                .expect("directory entries read")
                .file_type()
                .expect("entry types read")
                .is_dir()
        })
        .count()
}

#[ignore = "requires CS_GAME_DIR"]
#[test]
fn accept_f02_c_retail_inventory_reports_every_expected_archive() {
    let root = game_dir();
    let found = discover(&root).expect("production discovery reads the original installation");
    let diagnosis = &found.diagnosis;

    // Independently re-derive the layout this test expects: every
    // reference lead is a real group, and every group/mission directory is
    // counted by direct reads.
    assert!(
        diagnosis.absent_reference_groups.is_empty(),
        "the retail installation carries every reference world group"
    );
    let zbd = diagnosis
        .zbd_dir
        .as_ref()
        .expect("the installation carries a ZBD directory");
    let zbd_path = root.join(zbd.as_str());
    let observed_groups: Vec<String> = std::fs::read_dir(&zbd_path)
        .expect("the ZBD directory reads")
        .filter_map(|entry| {
            let entry = entry.expect("directory entries read");
            entry
                .file_type()
                .expect("entry types read")
                .is_dir()
                .then(|| entry.file_name().to_string_lossy().to_ascii_lowercase())
        })
        .collect();
    assert_eq!(
        observed_groups.len(),
        diagnosis.world_groups.len(),
        "the diagnosed groups are exactly the directories under ZBD"
    );
    for lead in REFERENCE_WORLD_GROUP_LEADS {
        assert!(
            observed_groups.contains(&lead.to_owned()),
            "reference lead {lead} is an observed group"
        );
    }
    let mission_dirs: usize = observed_groups
        .iter()
        .map(|group| observed_mission_dirs(&zbd_path, group))
        .sum();
    assert!(
        mission_dirs > 0,
        "the installation carries mission directories"
    );

    // The dependency-impact report over the retail installation: every
    // expected archive is available and nothing is impacted. The expected
    // count is this test's own derivation, not the implementation's.
    let impact = dependency_impact(&found);
    let expected = 2 + 4 * observed_groups.len() + 2 * mission_dirs;
    assert_eq!(
        impact.expected_count(),
        expected,
        "2 zbd archives + 4 per group + 2 per mission directory"
    );
    assert_eq!(impact.available_count(), expected);
    assert_eq!(
        impact.unavailable_count(),
        0,
        "the retail installation misses no expected archive"
    );
    assert!(
        impact.impacted_dependents().is_empty(),
        "no dependent is impacted"
    );
    for archive in &impact.archives {
        assert!(
            archive.available(),
            "expected archive {} is present",
            archive.logical_key
        );
    }

    // The rendered report's fingerprints are the production fingerprints
    // of the actual installation bytes.
    let report = inventory_report_json(&found);
    assert!(report.contains(&format!(
        "\"install_sha256\": \"{}\"",
        fingerprint(&found.manifest).to_hex()
    )));
    assert!(report.contains("\"unavailable\": 0"));

    // The real binary produces the same report end to end: exit zero, the
    // --out file exists, and it carries the retail counts.
    let scratch = TempTree::new("retail-out");
    let out = scratch.root().join("inventory.json");
    let output = Command::new(env!("CARGO_BIN_EXE_cs-inspect"))
        .arg("inventory")
        .arg("--cs-path")
        .arg(&root)
        .arg("--out")
        .arg(&out)
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("cs-inspect runs");
    assert_eq!(
        output.status.code(),
        Some(0),
        "the inventory command exits zero on the retail installation, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cli_report = std::fs::read_to_string(&out).expect("the --out report exists");
    assert!(cli_report.contains("\"unavailable\": 0"));
    assert!(cli_report.contains(&format!("\"files\": {}", diagnosis.file_count)));
    assert_eq!(
        cli_report.trim_end(),
        report.trim_end(),
        "the command's report is the library-rendered report"
    );
}
