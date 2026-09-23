//! The synthetic installation-inventory fixture (F02-A).
//!
//! [`synthetic_install_fixture`] builds a small authored inventory through
//! the canonical [`InstallManifest`] constructor, so tests and the future
//! `inventory` command have a real, validated input without touching the
//! owner's installation at `$CS_GAME_DIR`.
//!
//! Every row — spellings, sizes, digests, family labels — is newly authored
//! fixture data, not original content: the digests are not hashes of any
//! original file, and the fixture proves nothing about retail installations.

use std::path::Path;

use cs_types::evidence::ContentHash;
use cs_types::install::{
    FileFamily, FileRole, InstallFileRecord, InstallManifest, ManifestError, ParseState,
    RelativePath,
};

/// Builds the minimal synthetic installation inventory for `host_root`.
///
/// The fixture deliberately covers every [`FileRole`] variant — including an
/// unclassified row with a failed parse that stays in the inventory — and a
/// mix of detected and undetected families, so consumers cannot pass by
/// omitting unknown or failed rows (IDENTITY-CONTENT: collections cannot
/// exclude failed entries).
///
/// The rows are identical for every root: calling this twice with
/// differently cased host roots yields manifests with different
/// [`InstallManifest::host_root`] values but one logical identity (F02
/// AC01).
pub fn synthetic_install_fixture(host_root: &Path) -> Result<InstallManifest, ManifestError> {
    let spelling =
        |text: &str| RelativePath::new(text).expect("synthetic fixture spelling is valid");
    let family = |label: &str| {
        Some(FileFamily::new(label).expect("synthetic fixture family label is valid"))
    };
    let files = vec![
        InstallFileRecord {
            relative_spelling: spelling("PLANES.ZBD"),
            size_bytes: 64,
            sha256: ContentHash::from_bytes([0x10; 32]),
            family: family("synthetic-zbd"),
            role: FileRole::Consumed,
            parse_state: ParseState::Parsed,
        },
        InstallFileRecord {
            relative_spelling: spelling("Media/Tick.Wav"),
            size_bytes: 32,
            sha256: ContentHash::from_bytes([0x11; 32]),
            family: None,
            role: FileRole::OptionalMedia,
            parse_state: ParseState::Unparsed,
        },
        InstallFileRecord {
            relative_spelling: spelling("System/Synthetic.Dll"),
            size_bytes: 16,
            sha256: ContentHash::from_bytes([0x12; 32]),
            family: None,
            role: FileRole::PlatformSupport,
            parse_state: ParseState::Unparsed,
        },
        InstallFileRecord {
            relative_spelling: spelling("Unknown/Blob.Bin"),
            size_bytes: 8,
            sha256: ContentHash::from_bytes([0x13; 32]),
            family: None,
            role: FileRole::Unknown,
            parse_state: ParseState::Failed {
                diagnostic: "synthetic fixture row: no reader claims this file".to_owned(),
            },
        },
        InstallFileRecord {
            relative_spelling: spelling("Future/Asset.Dat"),
            size_bytes: 128,
            sha256: ContentHash::from_bytes([0x14; 32]),
            family: None,
            role: FileRole::NeededUnimplemented,
            parse_state: ParseState::Unparsed,
        },
        InstallFileRecord {
            relative_spelling: spelling("Stale/Leftover.Bak"),
            size_bytes: 4,
            sha256: ContentHash::from_bytes([0x15; 32]),
            family: None,
            role: FileRole::UnusedWithReason(
                "authored fixture leftover with no consumer".to_owned(),
            ),
            parse_state: ParseState::Unparsed,
        },
    ];
    InstallManifest::new(host_root.to_path_buf(), files)
}
