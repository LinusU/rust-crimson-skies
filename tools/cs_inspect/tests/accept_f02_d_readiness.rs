//! F02-D acceptance through the full-content readiness check (spec F02
//! AC04): a partial installation never passes it.
//!
//! These tests exercise production code only: `cs_assets::install::discover`
//! builds the inventory, `cs_inspect::install::{install_audit,
//! full_content_readiness}` classifies every file and evaluates readiness.
//! If readiness ignored unavailable archives or unclassified gameplay
//! files, every negative assertion here fails.
//!
//! The fixture trees are newly authored bytes under the system temporary
//! directory; the original installation is never touched.

mod common;

use common::TempTree;
use cs_assets::install::{FileRoleKind, discover};
use cs_inspect::install::{
    audit_report_json, dependency_impact, full_content_readiness, install_audit,
};

const ARCHIVE_PAYLOAD: &[u8] = b"authored fixture archive payload";

/// A complete installation: both zbd-level archives, all eight reference
/// groups with their four archives each, one mission directory per group
/// carrying its two archives, plus the platform/media/gosdata files the
/// classifier covers.
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
    tree.write("crimsonff.ifr", b"authored fixture resource");
    tree.write("Readme.rtf", b"authored fixture document");
    tree.write("GOSDATA/ASSETS/crimson.rof", ARCHIVE_PAYLOAD);
    tree.write(
        "GOSDATA/ASSETS/GRAPHICS/font.tga",
        b"authored fixture image",
    );
    tree.write(
        "GOSDATA/ASSETS/GRAPHICS/MPG/chap0.mpg",
        b"authored fixture video",
    );
    tree.write(
        "GOSDATA/ASSETS/BINARIES/ijl10.dll",
        b"authored fixture library",
    );
    tree
}

/// A complete, fully classified installation passes full-content readiness
/// in both modes, and the report shows the `full` class with no failures.
#[test]
fn accept_f02_d_complete_installation_passes_full_content_readiness() {
    let tree = complete_tree("complete");
    let found = discover(tree.root()).expect("the fixture tree discovers");
    let audit = install_audit(&found);

    // Every fixture file is classified: zero unknowns anywhere.
    assert_eq!(
        audit.role_count(FileRoleKind::Unknown),
        0,
        "the complete fixture classifies every file"
    );
    assert!(audit.unclassified_gameplay().is_empty());
    assert!(audit.unclassified_other().is_empty());

    for strict in [false, true] {
        let check = full_content_readiness(&audit, strict);
        assert!(
            check.full_content_ready,
            "strict={strict}: a complete installation is ready: {:?}",
            check.failures
        );
        assert!(check.failures.is_empty(), "a pass carries no failures");
    }

    let report = audit_report_json(&found, &audit, true);
    assert!(report.contains("\"report\": \"cs-inspect-audit/v1\""));
    assert!(report.contains("\"full_content\": true"));
    assert!(report.contains("\"classes\": [\"full\"]"));
    assert!(report.contains("\"unknown\": 0"));
    assert!(report.contains("\"unavailable\": 0"));
}

/// The minimum acceptance scenario (AC04): a partial installation never
/// passes the full-content readiness check — missing mission archives,
/// missing group archives and absent reference groups each fail it, in
/// both modes, with the impacted scopes named.
#[test]
fn accept_f02_d_partial_installation_never_passes_full_content_readiness() {
    let tree = TempTree::new("partial");
    tree.write("ZBD/planes.zbd", ARCHIVE_PAYLOAD);
    // interp.zbd missing at the zbd level.
    tree.write("ZBD/C1/cam_anim.zbd", ARCHIVE_PAYLOAD);
    tree.write("ZBD/C1/gamez.zbd", ARCHIVE_PAYLOAD);
    tree.write("ZBD/C1/texture.zbd", ARCHIVE_PAYLOAD);
    tree.write("ZBD/C1/zrdr.zbd", ARCHIVE_PAYLOAD);
    tree.write("ZBD/C1/M01/mis_anim.zbd", ARCHIVE_PAYLOAD);
    // ZBD/C1/M01/zrdr.zbd missing (AC03's missing mission archive).
    tree.mkdir("ZBD/C2/EMPTY");
    tree.write("ZBD/C3/texture.zbd", ARCHIVE_PAYLOAD);
    tree.write("crimson.exe", b"authored fixture executable");

    let found = discover(tree.root()).expect("the fixture tree discovers");
    let audit = install_audit(&found);
    assert!(
        dependency_impact(&found).unavailable_count() > 0,
        "the fixture is partial by construction"
    );

    for strict in [false, true] {
        let check = full_content_readiness(&audit, strict);
        assert!(
            !check.full_content_ready,
            "strict={strict}: a partial installation never passes"
        );
        assert!(
            check
                .failures
                .iter()
                .any(|failure| failure.contains("expected archives unavailable")),
            "strict={strict}: the failure names unavailable archives: {:?}",
            check.failures
        );
        // The exact impacted scopes are reported, not just a count.
        assert!(
            check
                .failures
                .iter()
                .any(|failure| failure.contains("zbd/c1/m01")),
            "strict={strict}: the mission directory losing its archive is named: {:?}",
            check.failures
        );
    }

    let report = audit_report_json(&found, &audit, false);
    assert!(report.contains("\"full_content\": false"));
    assert!(report.contains("\"classes\": [\"partial\"]"));
    assert!(
        report.contains("\"zbd/c1/m01/zrdr.zbd\""),
        "the missing archive stays a named unavailable row:\n{report}"
    );
}

/// An unclassified file inside the gameplay scope is an unknown gameplay
/// dependency: it fails completeness even when every expected archive is
/// present (spec F02 non-negotiable behavior 4).
#[test]
fn accept_f02_d_unclassified_gameplay_file_fails_readiness() {
    let tree = complete_tree("unclassified-gameplay");
    tree.write("ZBD/C1/odd.dat", b"authored fixture blob");
    tree.write("GOSDATA/ASSETS/mystery.bin", b"authored fixture blob");

    let found = discover(tree.root()).expect("the fixture tree discovers");
    let audit = install_audit(&found);
    assert_eq!(
        audit.unclassified_gameplay().len(),
        2,
        "both planted unknowns are gameplay-scope unclassified"
    );

    for strict in [false, true] {
        let check = full_content_readiness(&audit, strict);
        assert!(
            !check.full_content_ready,
            "strict={strict}: unclassified gameplay files fail completeness"
        );
        let failure = check
            .failures
            .iter()
            .find(|failure| failure.contains("unclassified gameplay files"))
            .expect("the gameplay failure is reported");
        assert!(failure.contains("zbd/c1/odd.dat"));
        assert!(failure.contains("gosdata/assets/mystery.bin"));
    }
}

/// An unclassified file outside the gameplay scope does not prove missing
/// gameplay content: base readiness still passes, but `--strict` fails it —
/// a strict audit classifies every file.
#[test]
fn accept_f02_d_strict_fails_unclassified_files_outside_gameplay_scope() {
    let tree = complete_tree("unclassified-other");
    tree.write("stray-leftover.sav", b"authored fixture blob");

    let found = discover(tree.root()).expect("the fixture tree discovers");
    let audit = install_audit(&found);
    assert!(audit.unclassified_gameplay().is_empty());
    assert_eq!(audit.unclassified_other().len(), 1);

    let base = full_content_readiness(&audit, false);
    assert!(
        base.full_content_ready,
        "a non-gameplay unknown does not fail base readiness: {:?}",
        base.failures
    );
    let strict = full_content_readiness(&audit, true);
    assert!(
        !strict.full_content_ready,
        "strict fails the unclassified file"
    );
    assert!(
        strict
            .failures
            .iter()
            .any(|failure| failure.contains("stray-leftover.sav")),
        "the strict failure names the file: {:?}",
        strict.failures
    );
}

/// The audit findings cover every inventoried file one-for-one — classified
/// and unclassified rows alike — so the audit can never pass by omitting a
/// file it failed to classify (IDENTITY-CONTENT: collections cannot
/// exclude failed entries).
#[test]
fn accept_f02_d_audit_finds_every_file_and_names_each_role() {
    let tree = TempTree::new("findings");
    tree.write("ZBD/planes.zbd", ARCHIVE_PAYLOAD);
    tree.write("crimson.exe", b"authored fixture executable");
    tree.write(
        "GOSDATA/ASSETS/GRAPHICS/MPG/chap0.mpg",
        b"authored fixture video",
    );
    tree.write("odd.blob", b"authored fixture blob");

    let found = discover(tree.root()).expect("the fixture tree discovers");
    let audit = install_audit(&found);
    assert_eq!(
        audit.findings.len(),
        found.manifest.files.len(),
        "one finding per inventoried file, none dropped"
    );

    let finding = |key: &str| {
        audit
            .findings
            .iter()
            .find(|finding| finding.logical_key == key)
            .unwrap_or_else(|| panic!("{key} is a finding"))
    };
    assert_eq!(
        finding("zbd/planes.zbd").kind,
        FileRoleKind::NeededUnimplemented
    );
    assert_eq!(finding("crimson.exe").kind, FileRoleKind::PlatformSupport);
    assert_eq!(
        finding("gosdata/assets/graphics/mpg/chap0.mpg").kind,
        FileRoleKind::OptionalMedia
    );
    let odd = finding("odd.blob");
    assert_eq!(odd.kind, FileRoleKind::Unknown);
    assert_eq!(odd.role, cs_types::install::FileRole::Unknown);
    assert!(!odd.basis.is_empty(), "every finding names its rule basis");
    assert!(
        !odd.gameplay_scope,
        "a root-level unknown is outside the gameplay scope"
    );
}
