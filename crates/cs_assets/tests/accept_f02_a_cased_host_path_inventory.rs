//! Acceptance scenario F02-A (AC01): identical data copied under differently
//! cased host paths is inventoried with one stable logical identity — plus
//! the failure cases around the inventory path.
//!
//! These tests exercise production code only: `cs_assets::install::
//! inventory` over the canonical records in `cs_types::install`. Removing or
//! neutering that implementation (for example by keying the identity on the
//! host path, or by stripping the root case-sensitively) makes them fail.

use std::path::{Path, PathBuf};

use cs_assets::install::{DiscoveredFile, inventory};
use cs_types::evidence::ContentHash;
use cs_types::install::{
    FileFamily, FileRole, InstallManifest, ManifestError, ParseState, RelativePathError,
};

/// The authored file set of both copies: spelling, size in bytes, digest
/// byte. Newly authored fixture data; not original content.
const FILE_SET: [(&str, u64, u8); 3] = [
    ("PLANES.ZBD", 64, 0x01),
    ("Worlds/Alpha.Grp", 128, 0x02),
    ("Media/Tick.Wav", 32, 0x03),
];

/// One discovered file under `root`, with `spelling` as the path to join.
fn file_under(root: &Path, spelling: &str, size: u64, digest: u8) -> DiscoveredFile {
    DiscoveredFile {
        host_path: root.join(spelling),
        size_bytes: size,
        sha256: ContentHash::from_bytes([digest; 32]),
        family: None,
        role: FileRole::Consumed,
        parse_state: ParseState::Unparsed,
    }
}

/// The identical file set discovered under `root`, spellings as authored.
fn file_set(root: &Path) -> Vec<DiscoveredFile> {
    FILE_SET
        .iter()
        .map(|(spelling, size, digest)| file_under(root, spelling, *size, *digest))
        .collect()
}

/// The identical file set discovered under `root`, spellings lowercased —
/// the same data reached through a case-differently-spelled tree.
fn lowercased_file_set(root: &Path) -> Vec<DiscoveredFile> {
    FILE_SET
        .iter()
        .map(|(spelling, size, digest)| file_under(root, &spelling.to_lowercase(), *size, *digest))
        .collect()
}

fn spellings(manifest: &InstallManifest) -> Vec<String> {
    manifest
        .files
        .iter()
        .map(|file| file.relative_spelling.as_str().to_owned())
        .collect()
}

/// The minimum acceptance scenario (AC01): the same data copied under
/// differently cased host paths keeps one logical identity, while each
/// copy's host root and original spellings stay intact.
#[test]
fn accept_f02_a_cased_host_paths_share_logical_identity() {
    let root_a = Path::new("/Games/CrimsonSkies");
    let root_b = Path::new("/games/crimsonskies");

    let copy_a = inventory(root_a, file_set(root_a)).expect("copy A inventories");
    let copy_b = inventory(root_b, file_set(root_b)).expect("copy B inventories");

    assert_eq!(copy_a.files.len(), FILE_SET.len(), "every file inventories");
    assert_ne!(
        copy_a.host_root, copy_b.host_root,
        "the two copies really are hosted under differently cased paths"
    );
    assert_eq!(
        copy_a.logical_identity(),
        copy_b.logical_identity(),
        "identical data under differently cased host paths must share one logical identity"
    );
    assert_eq!(
        spellings(&copy_a),
        spellings(&copy_b),
        "both copies preserve the original spellings of the data"
    );
    assert_eq!(
        spellings(&copy_a),
        ["PLANES.ZBD", "Worlds/Alpha.Grp", "Media/Tick.Wav"],
        "the discovered spelling is preserved exactly, case included"
    );
}

/// The letter case of the *relative* spellings also stays out of the
/// identity, while each manifest keeps its own originals.
#[test]
fn accept_f02_a_cased_relative_spellings_share_identity_and_keep_originals() {
    let root = Path::new("/Games/CrimsonSkies");
    let lower_root = Path::new("/games/crimsonskies");

    let upper = inventory(root, file_set(root)).expect("upper-cased tree inventories");
    let lower = inventory(lower_root, lowercased_file_set(lower_root))
        .expect("lower-cased tree inventories");

    assert_eq!(
        upper.logical_identity(),
        lower.logical_identity(),
        "letter case in relative spellings must not reach the logical identity"
    );
    assert_ne!(
        spellings(&upper),
        spellings(&lower),
        "each copy keeps its own original spellings instead of a normalized spelling"
    );
    for (upper_row, lower_row) in upper.files.iter().zip(&lower.files) {
        assert_eq!(
            upper_row.relative_spelling.logical_key(),
            lower_row.relative_spelling.logical_key(),
            "rows that differ only in case must key identically"
        );
        assert_eq!(upper_row.sha256, lower_row.sha256, "the data is identical");
    }
}

/// A host root whose letter case disagrees with the discovered paths (an
/// environment variable spelled differently from the file system) still
/// inventories: root components match case-insensitively instead of
/// demanding one exact spelling.
#[test]
fn accept_f02_a_root_case_mismatch_still_inventories_one_identity() {
    let paths_root = Path::new("/Games/CrimsonSkies");
    let declared_root = Path::new("/GAMES/CRIMSONSKIES");

    let reference = inventory(paths_root, file_set(paths_root)).expect("reference inventories");
    let mismatched = inventory(declared_root, file_set(paths_root))
        .expect("a case-mismatched root must still inventory");

    assert_eq!(mismatched.files.len(), FILE_SET.len());
    assert_eq!(
        reference.logical_identity(),
        mismatched.logical_identity(),
        "a root spelled with different case must yield the same logical identity"
    );
}

/// The identity tracks the inventoried data: content, size and discovery
/// order are handled as designed, while analysis results are not data.
#[test]
fn accept_f02_a_identity_tracks_data_not_analysis_or_order() {
    let root = Path::new("/Games/CrimsonSkies");
    let base = inventory(root, file_set(root)).expect("baseline inventories");

    // A one-byte content edit changes the digest, hence the identity: an
    // identity that ignored content fails here.
    let mut edited = file_set(root);
    edited[1].sha256 = ContentHash::from_bytes([0xff; 32]);
    let edited = inventory(root, edited).expect("edited copy inventories");
    assert_ne!(
        base.logical_identity(),
        edited.logical_identity(),
        "a one-byte content change must change the logical identity"
    );

    // A size change alone also changes the identity.
    let mut resized = file_set(root);
    resized[0].size_bytes += 1;
    let resized = inventory(root, resized).expect("resized copy inventories");
    assert_ne!(
        base.logical_identity(),
        resized.logical_identity(),
        "a size change must change the logical identity"
    );

    // Re-classifying the same bytes (role, parse state, detected family) is
    // an analysis change, not an installation change.
    let mut reclassified = file_set(root);
    reclassified[0].family =
        Some(FileFamily::new("synthetic-zbd").expect("fixture family label is valid"));
    reclassified[1].role = FileRole::Unknown;
    reclassified[2].parse_state = ParseState::Failed {
        diagnostic: "authored test reclassification".to_owned(),
    };
    let reclassified = inventory(root, reclassified).expect("reclassified copy inventories");
    assert_eq!(
        base.logical_identity(),
        reclassified.logical_identity(),
        "family, role and parse state are analysis results and must not reach the identity"
    );

    // Discovery order does not matter: rows sort by logical key.
    let mut reordered = file_set(root);
    reordered.reverse();
    let reordered = inventory(root, reordered).expect("reordered copy inventories");
    assert_eq!(
        base.logical_identity(),
        reordered.logical_identity(),
        "the discovery order of rows must not reach the identity"
    );
}

/// Nothing is ever silently omitted or merged: a file outside the root, the
/// root itself, a `..` escape and a case-colliding duplicate are each a
/// named error.
#[test]
fn accept_f02_a_invalid_discovery_is_named_not_omitted() {
    let root = Path::new("/Games/CrimsonSkies");

    let mut outside = file_set(root);
    outside.push(file_under(Path::new("/Elsewhere"), "Other.ZBD", 8, 0x09));
    assert!(
        matches!(
            inventory(root, outside),
            Err(ManifestError::NotUnderRoot { .. })
        ),
        "a file outside the root must fail the inventory, never be dropped from it"
    );

    let root_itself = DiscoveredFile {
        host_path: root.to_path_buf(),
        size_bytes: 4,
        sha256: ContentHash::from_bytes([0x04; 32]),
        family: None,
        role: FileRole::Consumed,
        parse_state: ParseState::Unparsed,
    };
    assert!(
        matches!(
            inventory(root, vec![root_itself]),
            Err(ManifestError::EmptyRelative { .. })
        ),
        "the root itself has no relative spelling and must be named, not skipped"
    );

    let mut escape = file_set(root);
    escape.push(file_under(root, "../Outside.bin", 4, 0x05));
    assert!(
        matches!(
            inventory(root, escape),
            Err(ManifestError::RelativePath {
                error: RelativePathError::ParentComponent,
                ..
            })
        ),
        "a `..` component must be refused instead of escaping the root"
    );

    let mut collision = file_set(root);
    collision.push(file_under(root, "planes.zbd", 4, 0x06));
    assert!(
        matches!(
            inventory(root, collision),
            Err(ManifestError::DuplicateLogicalKey { .. })
        ),
        "rows that differ only in letter case are one ambiguous key and must be refused"
    );

    assert!(
        matches!(
            inventory(Path::new(""), file_set(Path::new(""))),
            Err(ManifestError::EmptyRoot)
        ),
        "an empty host root is a caller error and must be named"
    );
}

/// A path component that is not valid UTF-8 fails loudly instead of being
/// lossily mangled into a spelling the engine could never re-open.
#[cfg(unix)]
#[test]
fn accept_f02_a_non_utf8_paths_fail_loudly() {
    use std::ffi::OsString;
    use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};

    let root = Path::new("/Games/CrimsonSkies");
    let mut bytes = root.as_os_str().as_bytes().to_vec();
    bytes.push(b'/');
    bytes.extend_from_slice(&[0xff, b'x']);

    let mut broken = file_set(root);
    broken.push(DiscoveredFile {
        host_path: PathBuf::from(OsString::from_vec(bytes)),
        size_bytes: 4,
        sha256: ContentHash::from_bytes([0x07; 32]),
        family: None,
        role: FileRole::Consumed,
        parse_state: ParseState::Unparsed,
    });

    assert!(
        matches!(
            inventory(root, broken),
            Err(ManifestError::NonUtf8Path { .. })
        ),
        "a non-UTF-8 path must fail the inventory by name, never be lossily converted"
    );
}
