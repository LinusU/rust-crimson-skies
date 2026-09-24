//! F02-C acceptance through the dependency-impact report (spec F02 AC03):
//! a missing mission archive remains visible as unavailable, not omitted
//! from the count.
//!
//! These tests exercise production code only: `cs_assets::install::discover`
//! builds the inventory and `cs_inspect::install::{dependency_impact,
//! inventory_report_json}` renders the report. If unavailable expected
//! archives were dropped — or never listed — every count here fails.
//!
//! The fixture trees are newly authored bytes under the system temporary
//! directory; the original installation is never touched.

mod common;

use common::TempTree;
use cs_assets::install::discover;
use cs_inspect::install::{ArchiveKind, dependency_impact, inventory_report_json};

const ARCHIVE_PAYLOAD: &[u8] = b"authored fixture archive payload";

/// A partial installation: group `c3` is present but carries only one of
/// its expected archives, mission `c2/m01` lost its `mis_anim.zbd`, mission
/// `c1/empty` carries nothing at all, and five reference-lead groups are
/// absent entirely.
fn partial_tree() -> TempTree {
    let tree = TempTree::new("partial");
    tree.write("ZBD/planes.zbd", ARCHIVE_PAYLOAD);
    tree.write("ZBD/interp.zbd", ARCHIVE_PAYLOAD);
    // c1 is complete; TEXTURE.ZBD exercises case-insensitive availability.
    tree.write("ZBD/C1/cam_anim.zbd", ARCHIVE_PAYLOAD);
    tree.write("ZBD/C1/gamez.zbd", ARCHIVE_PAYLOAD);
    tree.write("ZBD/C1/TEXTURE.ZBD", ARCHIVE_PAYLOAD);
    tree.write("ZBD/C1/zrdr.zbd", ARCHIVE_PAYLOAD);
    tree.write("ZBD/C1/M01/mis_anim.zbd", ARCHIVE_PAYLOAD);
    tree.write("ZBD/C1/M01/zrdr.zbd", ARCHIVE_PAYLOAD);
    tree.mkdir("ZBD/C1/EMPTY");
    tree.write("ZBD/C3/texture.zbd", ARCHIVE_PAYLOAD);
    tree.write("ZBD/C2/M01/zrdr.zbd", ARCHIVE_PAYLOAD);
    tree
}

/// The minimum acceptance scenario: expected archives that are missing stay
/// visible as unavailable rows and are counted — at every level (zbd,
/// group, mission) — instead of being omitted.
#[test]
fn accept_f02_c_missing_mission_archive_stays_visible_and_counted() {
    let tree = partial_tree();
    let found = discover(tree.root()).expect("the fixture tree discovers");
    let impact = dependency_impact(&found);

    // The expected set is the reference/observed layout, not the files that
    // happen to be present: 2 zbd archives + 8 groups × 4 + 3 mission
    // directories × 2 = 40 rows, of which 30 are missing here.
    assert_eq!(impact.expected_count(), 40, "the expected set is fixed");
    assert_eq!(impact.available_count(), 10);
    assert_eq!(
        impact.unavailable_count(),
        30,
        "every missing expected archive is counted, not omitted"
    );

    let archive = |key: &str| {
        impact
            .archives
            .iter()
            .find(|archive| archive.logical_key == key)
            .unwrap_or_else(|| panic!("{key} stays a row even when missing"))
    };

    // A group archive that is absent from an observed group is an
    // unavailable row naming its dependent — the AC03 missing mission
    // archive, not a missing count.
    let gamez = archive("zbd/c3/gamez.zbd");
    assert!(
        !gamez.available(),
        "the missing group archive is unavailable"
    );
    assert_eq!(gamez.kind, ArchiveKind::Group);
    assert_eq!(gamez.dependent, "zbd/c3");
    assert_eq!(gamez.observed, None);

    // A mission directory that lost one archive reports exactly that
    // archive, and a directory that carries nothing still expects both.
    let mis_anim = archive("zbd/c2/m01/mis_anim.zbd");
    assert!(!mis_anim.available());
    assert_eq!(mis_anim.kind, ArchiveKind::Mission);
    assert_eq!(mis_anim.dependent, "zbd/c2/m01");
    assert!(!archive("zbd/c1/empty/mis_anim.zbd").available());
    assert!(!archive("zbd/c1/empty/zrdr.zbd").available());

    // Absent reference-lead groups report all four expected archives.
    for lead in ["c1b", "c1c", "c2b", "c4", "c5"] {
        for name in ["cam_anim.zbd", "gamez.zbd", "texture.zbd", "zrdr.zbd"] {
            let missing = archive(&format!("zbd/{lead}/{name}"));
            assert!(
                !missing.available(),
                "absent group {lead} keeps {name} visible as unavailable"
            );
            assert_eq!(missing.dependent, format!("zbd/{lead}"));
        }
    }

    // Available rows carry the preserved on-disk spelling, resolved
    // case-insensitively (non-negotiable behavior 2).
    let texture = archive("zbd/c1/texture.zbd");
    assert!(texture.available());
    assert_eq!(
        texture.observed.as_ref().map(|path| path.as_str()),
        Some("ZBD/C1/TEXTURE.ZBD"),
        "the available row preserves the original spelling"
    );
    assert_eq!(
        archive("zbd/planes.zbd")
            .observed
            .as_ref()
            .map(|p| p.as_str()),
        Some("ZBD/planes.zbd")
    );

    // The impacted dependents name every scope that loses an archive.
    assert_eq!(
        impact.impacted_dependents(),
        [
            "zbd/c1/empty",
            "zbd/c1b",
            "zbd/c1c",
            "zbd/c2",
            "zbd/c2/m01",
            "zbd/c2b",
            "zbd/c3",
            "zbd/c4",
            "zbd/c5",
        ],
        "every dependent scope with a missing archive is impacted"
    );
    for dependent in impact
        .archives
        .iter()
        .filter(|archive| !archive.available())
        .map(|archive| &archive.dependent)
    {
        assert!(
            impact.impacted_dependents().contains(dependent),
            "dependent {dependent} is impacted"
        );
    }
}

/// The rendered report keeps the same accounting: the JSON carries the
/// unavailable row and the totals, so a consumer sees the missing archive
/// instead of a shortened list.
#[test]
fn accept_f02_c_inventory_report_counts_unavailable_archives() {
    let tree = partial_tree();
    let found = discover(tree.root()).expect("the fixture tree discovers");
    let report = inventory_report_json(&found);

    assert!(report.contains("\"report\": \"cs-inspect-inventory/v1\""));
    assert!(
        report.contains(
            "{\"expected\": \"zbd/c3/gamez.zbd\", \"kind\": \"group-archive\", \
             \"dependent\": \"zbd/c3\", \"available\": false, \"observed\": null}"
        ),
        "the missing mission archive is a visible unavailable row:\n{report}"
    );
    assert!(
        report.contains(
            "\"summary\": {\"expected\": 40, \"available\": 10, \"unavailable\": 30, \
             \"impacted_dependents\": 9}"
        ),
        "the summary counts unavailable archives:\n{report}"
    );
    assert!(report.contains("\"zbd/c1/empty\""), "impacted dependents");
    // One inventory row per discovered file — 10 in this fixture — and the
    // fingerprints describe the actual bytes.
    assert!(report.contains("\"files\": 10"), "counts.files");
    assert!(report.contains("\"install_sha256\": \""));
    assert!(report.contains("\"content_sha256\": \""));
}

/// A complete layout reports zero unavailable archives and no impacted
/// dependents: the report distinguishes a partial installation from a
/// complete one instead of always flagging something.
#[test]
fn accept_f02_c_complete_layout_reports_nothing_unavailable() {
    let tree = TempTree::new("complete");
    tree.write("ZBD/planes.zbd", ARCHIVE_PAYLOAD);
    tree.write("ZBD/interp.zbd", ARCHIVE_PAYLOAD);
    for group in ["C1", "C1B", "C1C", "C2", "C2B", "C3", "C4", "C5"] {
        for name in ["cam_anim.zbd", "gamez.zbd", "texture.zbd", "zrdr.zbd"] {
            tree.write(&format!("ZBD/{group}/{name}"), ARCHIVE_PAYLOAD);
        }
    }
    tree.write("ZBD/C1/M02/mis_anim.zbd", ARCHIVE_PAYLOAD);
    tree.write("ZBD/C1/M02/zrdr.zbd", ARCHIVE_PAYLOAD);

    let found = discover(tree.root()).expect("the fixture tree discovers");
    let impact = dependency_impact(&found);
    assert_eq!(impact.expected_count(), 2 + 8 * 4 + 2);
    assert_eq!(impact.unavailable_count(), 0);
    assert!(
        impact.impacted_dependents().is_empty(),
        "a complete layout impacts no dependent"
    );

    // An extra group beyond the reference leads is still expected to carry
    // its archives (non-negotiable behavior 3: leads are not authoritative).
    tree.write("ZBD/CUSTOM/cam_anim.zbd", ARCHIVE_PAYLOAD);
    let found = discover(tree.root()).expect("the extended tree discovers");
    let impact = dependency_impact(&found);
    let custom = |name: &str| {
        impact
            .archives
            .iter()
            .find(|archive| archive.logical_key == format!("zbd/custom/{name}"))
            .unwrap_or_else(|| panic!("zbd/custom/{name} is an expected row"))
    };
    assert!(custom("cam_anim.zbd").available());
    assert!(!custom("gamez.zbd").available());
    assert_eq!(impact.unavailable_count(), 3);
}
