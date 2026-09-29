//! F14-E acceptance: `cs-inspect campaign` exposes the retail campaign
//! directory layout as a read-only inspection report (task #373).
//!
//! The synthetic tests run the shipped binary (`CARGO_BIN_EXE_cs-inspect`)
//! over newly authored fixture trees: they prove the production walk and
//! report, never anything about a retail installation, and never touch
//! `$CS_GAME_DIR`. The retail test is `#[ignore = "requires CS_GAME_DIR"]`
//! and is run by the implementing and reviewing agents with
//! `--include-ignored`; it asserts the installation's declared 24-mission
//! `ZBD/C<chapter><variant>/M<nn>` layout (5/5/5/5/4 across chapters 1-5),
//! that every declared program archive is present, and that each digest is
//! the digest of the archive it names — cross-checked against the committed
//! M01 binding record.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use cs_assets::install::sha256;
use cs_content::campaign_bindings::{CampaignLayoutEntry, campaign_layout};

use common::TempTree;

const MISSION_A: &[u8] = b"authored campaign fixture program archive A";
const MISSION_B: &[u8] = b"authored campaign fixture program archive B, different bytes";

/// The shipped binary under test.
fn cs_inspect() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cs-inspect"))
}

/// The original installation, as the environment declares it. Panics when
/// `CS_GAME_DIR` is unset, so an `--include-ignored` run without the retail
/// capability fails loudly instead of passing vacuously.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: F14-E's retail test needs the retail capability; run this \
             suite with `--include-ignored` and CS_GAME_DIR pointing at the read-only installation"
        )
    }))
}

/// The workspace root, for the committed inventory and binding records.
fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tools/")
        .parent()
        .expect("workspace root")
        .join(relative)
}

/// A three-mission fixture: chapter 1 stores M01 and M02 in two world-group
/// directories, chapter 2 stores M01 without a program archive.
fn campaign_tree(label: &str) -> TempTree {
    let tree = TempTree::new(label);
    tree.write("ZBD/C1/M01/zrdr.zbd", MISSION_A);
    tree.write("ZBD/C1B/M02/zrdr.zbd", MISSION_B);
    tree.mkdir("ZBD/C2/M01");
    tree
}

fn read_report(path: &Path) -> String {
    std::fs::read_to_string(path).expect("the --out report exists")
}

/// The JSON field for one mission, located by its `program_asset` spelling.
fn mission_object<'a>(report: &'a str, asset: &str) -> &'a str {
    let marker = format!("\"program_asset\":\"{asset}\"");
    let index = report
        .find(&marker)
        .unwrap_or_else(|| panic!("no mission row for {asset} in report: {report}"));
    let start = report[..index]
        .rfind('{')
        .expect("a row starts before the marker");
    let end = report[index..]
        .find('}')
        .expect("a row ends after the marker")
        + index;
    &report[start..=end]
}

// ------------------------------------------------------------- synthetic ---

/// The report lists every mission directory with its chapter, number, world
/// group, program path and presence, and each present archive's digest.
#[test]
fn accept_f14_e_campaign_report_lists_every_mission_directory() {
    let tree = campaign_tree("report");
    let out = tree.root().join("campaign.json");

    let output = cs_inspect()
        .arg("campaign")
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
        "campaign exits zero on success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains(&format!("wrote campaign report to {}", out.display())),
        "the final --out path is reported on stderr"
    );

    let report = read_report(&out);
    assert!(
        report.contains("\"schema\":\"cs-inspect-campaign/v1\""),
        "the report names its schema, got: {report}"
    );
    assert!(
        report.contains("\"mission_count\":3"),
        "all three mission directories are listed, got: {report}"
    );
    // Chapter summary: chapter 1 holds two missions across two world groups,
    // chapter 2 holds one.
    assert!(
        report.contains("\"chapter\":1,\"mission_count\":2,\"world_groups\":[\"c1\",\"c1b\"]"),
        "chapter 1 keeps both world groups, got: {report}"
    );
    assert!(
        report.contains("\"chapter\":2,\"mission_count\":1,\"world_groups\":[\"c2\"]"),
        "chapter 2 keeps its single world group, got: {report}"
    );

    // Chapter 1 mission 1: world group lowercased for identity, the archive
    // path as spelled on disk, present, with the digest of exactly those
    // bytes.
    let m1 = mission_object(&report, "ZBD/C1/M01/zrdr.zbd");
    assert!(m1.contains("\"chapter\":1"), "row: {m1}");
    assert!(m1.contains("\"mission_number\":1"), "row: {m1}");
    assert!(m1.contains("\"world_group\":\"c1\""), "row: {m1}");
    assert!(m1.contains("\"program_present\":true"), "row: {m1}");
    assert!(
        m1.contains(&format!(
            "\"program_sha256\":\"{}\"",
            sha256(MISSION_A).to_hex()
        )),
        "the digest is the digest of the archive's bytes, row: {m1}"
    );

    // A different archive has a different digest.
    let m2 = mission_object(&report, "ZBD/C1B/M02/zrdr.zbd");
    assert!(m2.contains("\"world_group\":\"c1b\""), "row: {m2}");
    assert!(
        m2.contains(&format!(
            "\"program_sha256\":\"{}\"",
            sha256(MISSION_B).to_hex()
        )),
        "row: {m2}"
    );
    assert_ne!(sha256(MISSION_A).to_hex(), sha256(MISSION_B).to_hex());

    // An absent program archive stays visible with no digest.
    let missing = mission_object(&report, "ZBD/C2/M01/zrdr.zbd");
    assert!(
        missing.contains("\"program_present\":false"),
        "row: {missing}"
    );
    assert!(
        missing.contains("\"program_sha256\":null"),
        "row: {missing}"
    );
}

/// Mutating the archive bytes changes the reported digest, so a constant or
/// stubbed digest cannot pass.
#[test]
fn accept_f14_e_the_digest_follows_the_archive_bytes() {
    let first = TempTree::new("digest-one");
    first.write("ZBD/C1/M01/zrdr.zbd", MISSION_A);
    let second = TempTree::new("digest-two");
    second.write("ZBD/C1/M01/zrdr.zbd", MISSION_B);

    let digest = |tree: &TempTree| -> String {
        let out = tree.root().join("campaign.json");
        let output = cs_inspect()
            .arg("campaign")
            .arg("--cs-path")
            .arg(tree.root())
            .arg("--out")
            .arg(&out)
            .env_remove("CS_GAME_DIR")
            .output()
            .expect("cs-inspect runs");
        assert_eq!(output.status.code(), Some(0));
        let report = read_report(&out);
        let row = mission_object(&report, "ZBD/C1/M01/zrdr.zbd");
        row.split("\"program_sha256\":")
            .nth(1)
            .and_then(|tail| tail.split('"').nth(1))
            .expect("the row carries a digest")
            .to_owned()
    };

    assert_eq!(digest(&first), sha256(MISSION_A).to_hex());
    assert_eq!(digest(&second), sha256(MISSION_B).to_hex());
    assert_ne!(digest(&first), digest(&second));
}

/// `--cs-path` wins over `CS_GAME_DIR`, and without `--out` the report goes
/// to stdout.
#[test]
fn accept_f14_e_cs_path_wins_over_cs_game_dir_and_stdout_carries_json() {
    let flagged = TempTree::new("flagged");
    flagged.write("ZBD/C1/M01/zrdr.zbd", MISSION_A);
    let environ = TempTree::new("environ");
    environ.write("ZBD/C3/M07/zrdr.zbd", MISSION_B);

    let output = cs_inspect()
        .arg("campaign")
        .arg("--cs-path")
        .arg(flagged.root())
        .env("CS_GAME_DIR", environ.root())
        .output()
        .expect("cs-inspect runs");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"program_asset\":\"ZBD/C1/M01/zrdr.zbd\""),
        "--cs-path selects the installation, got: {stdout}"
    );
    assert!(
        !stdout.contains("ZBD/C3/M07/zrdr.zbd"),
        "CS_GAME_DIR must not contribute when --cs-path is given"
    );

    // Without --cs-path the environment selects the installation.
    let output = cs_inspect()
        .arg("campaign")
        .env("CS_GAME_DIR", environ.root())
        .output()
        .expect("cs-inspect runs");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"program_asset\":\"ZBD/C3/M07/zrdr.zbd\""),
        "CS_GAME_DIR selects the installation, got: {stdout}"
    );
}

/// Every failure is a named, nonzero exit — never a zero after only logging
/// a failure.
#[test]
fn accept_f14_e_campaign_failures_propagate_named_exit_codes() {
    let tree = campaign_tree("failures");

    // No --cs-path and no CS_GAME_DIR: the retail capability is missing.
    let output = cs_inspect()
        .arg("campaign")
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("cs-inspect runs");
    assert_eq!(
        output.status.code(),
        Some(4),
        "no installation is exit 4 (missing capability), stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("cs-inspect:"));

    // Malformed input: an unknown argument and a missing flag value.
    for args in [
        vec!["campaign", "--bogus", "x"],
        vec!["campaign", "positional"],
        vec!["campaign", "--cs-path"],
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

    // A tree with no campaign mission directory is a failed validation, not
    // an empty success.
    let empty = TempTree::new("empty");
    empty.mkdir("ZBD/C1");
    let output = cs_inspect()
        .arg("campaign")
        .arg("--cs-path")
        .arg(empty.root())
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("cs-inspect runs");
    assert_eq!(
        output.status.code(),
        Some(3),
        "an installation with no campaign is exit 3, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // A tree with no ZBD directory at all is a runtime failure that names
    // the path.
    let no_zbd = TempTree::new("no-zbd");
    no_zbd.write("GOSDATA/readme.txt", b"x");
    let output = cs_inspect()
        .arg("campaign")
        .arg("--cs-path")
        .arg(no_zbd.root())
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("cs-inspect runs");
    assert_eq!(
        output.status.code(),
        Some(1),
        "a missing ZBD is a runtime failure"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&no_zbd.root().join("ZBD").display().to_string()),
        "the failure names the path it happened at, got: {stderr:?}"
    );

    // An --out path whose parent does not exist is an output failure, and no
    // partial report is left behind.
    let out = tree.root().join("no-such-dir").join("campaign.json");
    let output = cs_inspect()
        .arg("campaign")
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

// ---------------------------------------------------------------- retail ---

/// The installation declares 24 campaign missions in
/// `ZBD/C<chapter><variant>/M<nn>` directories (5/5/5/5/4 across chapters
/// 1-5); every declared program archive is present; every digest is the
/// digest of the archive it names, cross-checked against the committed M01
/// binding record.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f14_e_retail_campaign_layout_declares_the_24_mission_baseline() {
    let install = game_dir();
    let layout = campaign_layout(&install).expect("the installation yields a campaign layout");

    assert_eq!(
        layout.len(),
        24,
        "the retail campaign declares 24 missions, got {}",
        layout.len()
    );
    let mut per_chapter = std::collections::BTreeMap::new();
    for entry in &layout {
        *per_chapter.entry(entry.mission.chapter).or_insert(0u32) += 1;
        assert!(
            entry.mission.mission_number >= 1,
            "a mission number is 1-based, got {}",
            entry.mission.mission_number
        );
        assert!(
            !entry.mission.world_group.is_empty()
                && entry
                    .mission
                    .world_group
                    .chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit()),
            "the world group is lowercased for identity, got {:?}",
            entry.mission.world_group
        );
        assert!(
            entry.mission.program_asset.starts_with("ZBD/"),
            "the program path is inside the ZBD container, got {:?}",
            entry.mission.program_asset
        );
        assert!(
            entry.mission.program_present,
            "the retail installation declares every mission program archive, missing {}",
            entry.mission.program_asset
        );
        let bytes =
            std::fs::read(install.join(&entry.mission.program_asset)).unwrap_or_else(|error| {
                panic!("cannot re-read {}: {error}", entry.mission.program_asset)
            });
        let digest = entry
            .program_sha256
            .as_ref()
            .unwrap_or_else(|| panic!("no digest for {}", entry.mission.program_asset));
        assert_eq!(
            digest,
            &sha256(&bytes).to_hex(),
            "the recorded digest of {} is stale",
            entry.mission.program_asset
        );
    }
    assert_eq!(
        per_chapter,
        [(1, 5), (2, 5), (3, 5), (4, 5), (5, 4)]
            .into_iter()
            .collect(),
        "the layout is 5/5/5/5/4 across chapters 1-5"
    );

    // The report is the same derivation the committed M01 binding record was
    // built from: chapter 1 mission 1 is `ZBD/C1C/M01/zrdr.zbd` with the
    // digest the record cites.
    let m01 = layout
        .iter()
        .find(|entry| entry.mission.chapter == 1 && entry.mission.mission_number == 1)
        .expect("chapter 1 declares M01");
    assert_eq!(m01.mission.program_asset, "ZBD/C1C/M01/zrdr.zbd");
    assert_eq!(m01.mission.world_group, "c1c");
    let record = std::fs::read_to_string(repo_path("missions/bindings/M01.json"))
        .expect("the committed M01 binding record reads");
    let digest = m01
        .program_sha256
        .as_ref()
        .expect("M01 has a program digest");
    assert!(
        record.contains(digest),
        "the M01 binding record does not cite the layout's digest {digest}"
    );

    // The shipped consumer produces the same report over the installation.
    let out = std::env::temp_dir().join(format!(
        "cs-f14-e-retail-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos()
    ));
    let output = cs_inspect()
        .arg("campaign")
        .arg("--cs-path")
        .arg(&install)
        .arg("--out")
        .arg(&out)
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("cs-inspect runs");
    assert_eq!(
        output.status.code(),
        Some(0),
        "the campaign command runs over the installation, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = read_report(&out);
    let _ = std::fs::remove_file(&out);
    assert!(
        report.contains("\"mission_count\":24"),
        "the consumer report declares 24 missions, got: {report}"
    );
    assert!(
        report.contains("\"chapter\":5,\"mission_count\":4"),
        "the consumer report keeps chapter 5's four missions, got: {report}"
    );
}

/// `CampaignLayoutEntry` is the shared production record the command renders:
/// a small guard that its public fields stay reachable to the consumers.
#[test]
fn accept_f14_e_layout_entry_exposes_the_mission_and_digest() {
    let entry: CampaignLayoutEntry = CampaignLayoutEntry {
        mission: cs_content::campaign_bindings::CampaignMission {
            chapter: 4,
            mission_number: 2,
            world_group: "c4b".to_owned(),
            program_asset: "ZBD/C4B/M02/zrdr.zbd".to_owned(),
            program_present: true,
        },
        program_sha256: Some("f".repeat(64)),
    };
    assert_eq!(entry.mission.chapter, 4);
    assert_eq!(entry.mission.mission_number, 2);
    assert_eq!(entry.mission.world_group, "c4b");
    assert!(entry.program_sha256.is_some());
}
