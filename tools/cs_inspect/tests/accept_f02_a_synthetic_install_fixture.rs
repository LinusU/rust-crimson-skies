//! Acceptance scenario F02-A through the inspector's synthetic fixture: the
//! authored installation inventory validates through the canonical
//! constructor, keeps every row — including unknown and failed ones — and
//! shares one logical identity across differently cased host roots (AC01).
//!
//! These tests exercise production code only: `cs_inspect::install::
//! synthetic_install_fixture` over `cs_types::install::InstallManifest`.
//! Removing or neutering that implementation makes them fail.

use std::path::Path;

use cs_inspect::install::synthetic_install_fixture;
use cs_types::evidence::ContentHash;
use cs_types::install::{FileRole, InstallManifest, ParseState};

/// The fixture must be a complete, classified inventory: every role variant
/// present, unknown/failed rows kept, original spellings preserved.
#[test]
fn accept_f02_a_fixture_inventory_keeps_every_row_and_classification() {
    let manifest =
        synthetic_install_fixture(Path::new("/Games/CrimsonSkies")).expect("fixture inventories");

    assert_eq!(
        manifest.files.len(),
        6,
        "the fixture carries one row per role variant"
    );
    let has_role =
        |matcher: fn(&FileRole) -> bool| manifest.files.iter().any(|file| matcher(&file.role));
    assert!(has_role(|role| matches!(role, FileRole::Consumed)));
    assert!(has_role(|role| matches!(
        role,
        FileRole::NeededUnimplemented
    )));
    assert!(has_role(|role| matches!(role, FileRole::OptionalMedia)));
    assert!(
        has_role(
            |role| matches!(role, FileRole::UnusedWithReason(reason) if !reason.trim().is_empty())
        ),
        "the unused row carries its mandatory reason"
    );
    assert!(has_role(|role| matches!(role, FileRole::PlatformSupport)));
    assert!(
        has_role(|role| matches!(role, FileRole::Unknown)),
        "the unclassified row stays in the inventory; unknown never disappears"
    );

    let failed = manifest
        .files
        .iter()
        .find(|file| matches!(file.parse_state, ParseState::Failed { .. }))
        .expect("the fixture keeps a failed-parse row visible");
    assert!(
        matches!(&failed.parse_state, ParseState::Failed { diagnostic } if !diagnostic.trim().is_empty()),
        "the failed row carries its diagnostic"
    );
    assert_eq!(failed.role, FileRole::Unknown);

    let planes = manifest
        .files
        .iter()
        .find(|file| file.relative_spelling.as_str() == "PLANES.ZBD")
        .expect("the fixture preserves the original spelling of its archive row");
    assert_eq!(
        planes.relative_spelling.logical_key(),
        "planes.zbd",
        "the case-insensitive key folds the preserved spelling"
    );
    assert_eq!(
        planes.family.as_ref().map(|family| family.as_str()),
        Some("synthetic-zbd"),
        "the detected family travels on the row"
    );
    assert_eq!(
        manifest
            .files
            .iter()
            .filter(|file| file.family.is_some())
            .count(),
        1,
        "undetected families stay None instead of being guessed"
    );
}

/// AC01 through the fixture: the same authored data under differently cased
/// host roots yields one logical identity, and the identity still tracks
/// the data (a digest edit changes it).
#[test]
fn accept_f02_a_fixture_identity_is_stable_across_cased_roots() {
    let copy_a =
        synthetic_install_fixture(Path::new("/Games/CrimsonSkies")).expect("fixture inventories");
    let copy_b =
        synthetic_install_fixture(Path::new("/games/crimsonskies")).expect("fixture inventories");

    assert_ne!(
        copy_a.host_root, copy_b.host_root,
        "the fixture roots really are cased differently"
    );
    assert_eq!(
        copy_a.logical_identity(),
        copy_b.logical_identity(),
        "identical fixture data under differently cased host paths shares one logical identity"
    );

    let mut edited = copy_a.clone();
    edited.files[0].sha256 = ContentHash::from_bytes([0xee; 32]);
    let edited = InstallManifest::new(edited.host_root, edited.files)
        .expect("edited fixture rows are structurally valid");
    assert_ne!(
        edited.logical_identity(),
        copy_a.logical_identity(),
        "the fixture exercises the real identity encoding: content changes it"
    );
}
