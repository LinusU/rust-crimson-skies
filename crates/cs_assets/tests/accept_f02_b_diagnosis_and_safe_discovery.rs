//! F02-B safe discovery and diagnosis: the walk inventories exactly the
//! regular files under the root, reports everything it deliberately did not
//! follow, and finds the `ZBD/planes.zbd`, world-group and ROF candidates
//! case-insensitively while preserving their original spellings (spec F02
//! non-negotiable behaviors 2/3, IDENTITY-CONTENT "no unchecked path
//! join"). Failures are named, never silently dropped (non-negotiable 4).
//!
//! The fixture trees are newly authored bytes under the system temporary
//! directory; the original installation is never touched.

mod common;

use std::path::Path;

use common::TempTree;
use cs_assets::install::{DiscoveryError, REFERENCE_WORLD_GROUP_LEADS, SkipReason, discover};
use cs_types::install::ManifestError;

const PLANES_PAYLOAD: &[u8] = b"authored fixture planes payload";
const GAMEZ_PAYLOAD: &[u8] = b"authored fixture gamez payload";
const TEXTURE_PAYLOAD: &[u8] = b"authored fixture texture payload";
const CUSTOM_PAYLOAD: &[u8] = b"authored fixture custom-group payload";
const CRIMPTCH_PAYLOAD: &[u8] = b"authored fixture crimptch rof payload";
const NOTES_PAYLOAD: &[u8] = b"authored fixture notes rof payload";
const EXE_PAYLOAD: &[u8] = b"authored fixture executable payload";

#[test]
fn accept_f02_b_diagnosis_reports_candidates_case_insensitively() {
    let tree = TempTree::new("diagnosis");
    tree.write("ZBD/planes.zbd", PLANES_PAYLOAD);
    tree.write("ZBD/C1/gamez.zbd", GAMEZ_PAYLOAD);
    tree.write("ZBD/C3/texture.zbd", TEXTURE_PAYLOAD);
    tree.write("ZBD/CUSTOM/addon.zbd", CUSTOM_PAYLOAD);
    tree.write("GOSDATA/ASSETS/crimptch.rof", CRIMPTCH_PAYLOAD);
    tree.write("Notes.ROF", NOTES_PAYLOAD);
    tree.write("crimson.exe", EXE_PAYLOAD);

    let found = discover(tree.root()).expect("the fixture tree discovers");
    let diagnosis = &found.diagnosis;

    assert_eq!(diagnosis.file_count, 7, "every regular file is inventoried");
    assert_eq!(
        found.manifest.files.len(),
        diagnosis.file_count,
        "the diagnosis count and the manifest agree"
    );
    assert_eq!(
        diagnosis.directory_count, 6,
        "ZBD, ZBD/C1, ZBD/C3, ZBD/CUSTOM, GOSDATA, GOSDATA/ASSETS"
    );
    assert_eq!(
        diagnosis.total_bytes,
        (PLANES_PAYLOAD.len()
            + GAMEZ_PAYLOAD.len()
            + TEXTURE_PAYLOAD.len()
            + CUSTOM_PAYLOAD.len()
            + CRIMPTCH_PAYLOAD.len()
            + NOTES_PAYLOAD.len()
            + EXE_PAYLOAD.len()) as u64,
        "the diagnosed byte total is the inventoried bytes"
    );
    assert!(
        diagnosis.skipped.is_empty(),
        "this tree contains nothing that must be skipped"
    );

    // ZBD and PLANES.ZBD are enumerated case-insensitively with the
    // original spellings preserved (non-negotiable behavior 2).
    let zbd = diagnosis
        .zbd_dir
        .as_ref()
        .expect("the top-level ZBD directory is discovered");
    assert_eq!(zbd.as_str(), "ZBD", "the original spelling is preserved");
    assert_eq!(zbd.logical_key(), "zbd", "lookups key case-insensitively");
    let planes = diagnosis
        .planes_zbd
        .as_ref()
        .expect("ZBD/planes.zbd is discovered case-insensitively");
    assert_eq!(planes.as_str(), "ZBD/planes.zbd");
    assert_eq!(planes.logical_key(), "zbd/planes.zbd");

    // Every observed group under ZBD is reported — including groups beyond
    // the reference leads — while absent leads are named (non-negotiable
    // behavior 3).
    let groups: Vec<&str> = diagnosis
        .world_groups
        .iter()
        .map(|group| group.as_str())
        .collect();
    assert_eq!(
        groups,
        ["ZBD/C1", "ZBD/C3", "ZBD/CUSTOM"],
        "observed groups keep their spellings and are sorted by logical key"
    );
    assert_eq!(
        diagnosis.absent_reference_groups,
        ["c1b", "c1c", "c2", "c2b", "c4", "c5"],
        "unobserved reference leads are reported in lead order"
    );
    assert!(
        diagnosis.absent_reference_groups.len() < REFERENCE_WORLD_GROUP_LEADS.len(),
        "observed leads are not reported absent"
    );

    // ROF candidates are found anywhere, case-insensitively, originals kept.
    let rofs: Vec<&str> = diagnosis
        .rof_candidates
        .iter()
        .map(|candidate| candidate.as_str())
        .collect();
    assert_eq!(
        rofs,
        ["GOSDATA/ASSETS/crimptch.rof", "Notes.ROF"],
        "ROF candidates are sorted by logical key and keep their spellings"
    );
    assert!(
        diagnosis
            .rof_candidates
            .iter()
            .all(|candidate| candidate.logical_key().ends_with(".rof")),
        "every candidate keys to a `.rof` extension"
    );
}

/// Safe discovery: symbolic links are reported, never followed — a link
/// pointing outside the root cannot pull outside content into the
/// inventory, and a link is not silently dropped either (IDENTITY-CONTENT:
/// collections cannot exclude entries; spec F02 non-negotiable 4).
#[cfg(unix)]
#[test]
fn accept_f02_b_symbolic_links_are_reported_and_never_followed() {
    let outside = TempTree::new("outside");
    outside.write("secret.bin", b"content that must never be inventoried");

    let tree = TempTree::new("links");
    tree.write("ZBD/planes.zbd", PLANES_PAYLOAD);
    std::os::unix::fs::symlink(outside.root(), tree.root().join("ZBD/link"))
        .expect("the directory symlink is created");
    std::os::unix::fs::symlink(
        outside.root().join("secret.bin"),
        tree.root().join("link-file"),
    )
    .expect("the file symlink is created");

    let found = discover(tree.root()).expect("discovery succeeds without following links");
    let keys: Vec<String> = found
        .manifest
        .files
        .iter()
        .map(|row| row.relative_spelling.logical_key())
        .collect();
    assert_eq!(
        keys,
        [PLANES_KEY],
        "only the real regular file is inventoried"
    );
    assert!(
        !keys.iter().any(|key| key.contains("secret")),
        "the link target outside the root never enters the inventory"
    );
    assert_eq!(
        found.diagnosis.skipped.len(),
        2,
        "both links are reported instead of silently dropped"
    );
    assert!(
        found
            .diagnosis
            .skipped
            .iter()
            .all(|entry| entry.reason == SkipReason::SymbolicLink),
        "skipped entries name the symbolic-link reason"
    );
    let skipped: Vec<String> = found
        .diagnosis
        .skipped
        .iter()
        .map(|entry| {
            entry
                .host_path
                .file_name()
                .expect("skipped entries have a name")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(
        skipped,
        ["link", "link-file"],
        "the skipped paths are the links themselves, in sorted order"
    );
}

const PLANES_KEY: &str = "zbd/planes.zbd";

/// Every discovery failure names itself instead of yielding an empty or
/// partial inventory (spec F02 non-negotiable behavior 4; IDENTITY-CONTENT
/// "collections cannot exclude failed entries").
#[test]
fn accept_f02_b_discovery_failures_are_named() {
    let tree = TempTree::new("failures");
    tree.write("crimson.exe", EXE_PAYLOAD);

    let missing = tree.root().join("no-such-directory");
    assert!(
        matches!(
            discover(&missing),
            Err(DiscoveryError::RootUnavailable { .. })
        ),
        "a missing root is a named error, not an empty inventory"
    );

    let as_file = tree.root().join("crimson.exe");
    assert!(
        matches!(
            discover(&as_file),
            Err(DiscoveryError::RootUnavailable { .. })
        ),
        "a regular file passed as the root is a named error"
    );

    assert!(
        matches!(
            discover(Path::new("")),
            Err(DiscoveryError::Inventory(ManifestError::EmptyRoot))
        ),
        "an empty root is refused by name"
    );
}
