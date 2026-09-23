//! Acceptance scenario F02-A at the schema level: the manifest
//! constructor's set-level rules, the logical-identity encoding (AC01), the
//! relative-spelling validation and the compatibility-profile schema.
//!
//! These tests exercise production code only: the records and validators in
//! `cs_types::install`. Removing or neutering that implementation makes them
//! fail.

use std::path::PathBuf;

use cs_types::evidence::ContentHash;
use cs_types::install::{
    CompatibilityProfile, DimensionLabel, FamilyError, FileFamily, FileRole,
    INSTALL_IDENTITY_HEADER, InstallFileRecord, InstallManifest, InstallationClass,
    InstallationEdition, LocaleLabel, MAX_DIMENSION_LABEL_LEN, MAX_FAMILY_LABEL_LEN,
    MAX_LOCALE_LABEL_LEN, ManifestError, ParseState, ProfileError, RelativePath, RelativePathError,
    RuleCompatibility,
};

/// One authored inventory row.
fn record(spelling: &str, size: u64, digest: u8) -> InstallFileRecord {
    InstallFileRecord {
        relative_spelling: RelativePath::new(spelling).expect("test spelling is valid"),
        size_bytes: size,
        sha256: ContentHash::from_bytes([digest; 32]),
        family: None,
        role: FileRole::Consumed,
        parse_state: ParseState::Unparsed,
    }
}

fn manifest(root: &str, files: Vec<InstallFileRecord>) -> Result<InstallManifest, ManifestError> {
    InstallManifest::new(PathBuf::from(root), files)
}

/// Set-level manifest rules: case-colliding rows are ambiguous, and
/// classifications that promise text must carry non-empty text.
#[test]
fn accept_f02_a_manifest_rejects_case_colliding_and_unjustified_rows() {
    assert_eq!(
        manifest(
            "/Games/CS",
            vec![record("PLANES.ZBD", 64, 1), record("planes.zbd", 64, 2)]
        ),
        Err(ManifestError::DuplicateLogicalKey {
            key: "planes.zbd".to_owned()
        }),
        "two rows differing only in case share one logical key and must be refused"
    );

    assert_eq!(
        manifest(
            "/Games/CS",
            vec![InstallFileRecord {
                role: FileRole::UnusedWithReason("  ".to_owned()),
                ..record("leftover.bak", 4, 3)
            }]
        ),
        Err(ManifestError::EmptyRoleReason),
        "an unused classification without a reason must be refused"
    );

    assert_eq!(
        manifest(
            "/Games/CS",
            vec![InstallFileRecord {
                parse_state: ParseState::Failed {
                    diagnostic: String::new()
                },
                ..record("broken.bin", 4, 4)
            }]
        ),
        Err(ManifestError::EmptyParseDiagnostic),
        "a failed parse without a diagnostic must be refused"
    );

    assert_eq!(
        manifest("", vec![]),
        Err(ManifestError::EmptyRoot),
        "an empty host root must be refused"
    );

    // A structurally valid (possibly empty) inventory is accepted: emptiness
    // is discovery's diagnosis to report (F02-B), not a schema violation.
    assert!(
        manifest("/Games/CS", vec![]).is_ok(),
        "an empty but valid inventory must be accepted"
    );
}

/// Relative spellings are validated before anything can join them into a
/// path, and their logical key is the case-insensitive comparison form
/// while the original spelling stays untouched.
#[test]
fn accept_f02_a_relative_path_spellings_validate_and_key_case_insensitively() {
    assert!(RelativePath::new("PLANES.ZBD").is_ok());
    assert!(RelativePath::new("Media\\Tick.Wav").is_ok());

    assert_eq!(RelativePath::new(""), Err(RelativePathError::Empty));
    assert_eq!(
        RelativePath::new("/etc/passwd"),
        Err(RelativePathError::Absolute)
    );
    assert_eq!(
        RelativePath::new("\\server\\share\\x.zbd"),
        Err(RelativePathError::Absolute)
    );
    assert_eq!(
        RelativePath::new("C:\\Game\\x.zbd"),
        Err(RelativePathError::Absolute)
    );
    assert_eq!(
        RelativePath::new("a/../b"),
        Err(RelativePathError::ParentComponent)
    );
    assert_eq!(
        RelativePath::new("./a"),
        Err(RelativePathError::CurrentComponent)
    );
    assert_eq!(
        RelativePath::new("a//b"),
        Err(RelativePathError::EmptyComponent)
    );
    assert_eq!(
        RelativePath::new("a/"),
        Err(RelativePathError::EmptyComponent)
    );
    assert_eq!(
        RelativePath::new("a\0b"),
        Err(RelativePathError::InteriorNul)
    );

    let windows = RelativePath::new("Worlds\\Alpha.Grp").expect("valid spelling");
    assert_eq!(
        windows.as_str(),
        "Worlds\\Alpha.Grp",
        "the original spelling is preserved byte-for-byte"
    );
    assert_eq!(
        windows.logical_key(),
        RelativePath::new("worlds/alpha.grp")
            .expect("valid spelling")
            .logical_key(),
        "the logical key folds letter case and separators"
    );
    assert_eq!(windows.logical_key(), "worlds/alpha.grp");
}

/// File-family dispatch labels are an open, bounded, lowercase vocabulary.
#[test]
fn accept_f02_a_family_labels_validate() {
    assert_eq!(
        FileFamily::new("zbd.sound").expect("valid label").as_str(),
        "zbd.sound"
    );
    assert_eq!(FileFamily::new(""), Err(FamilyError::Empty));
    assert_eq!(
        FileFamily::new("Zbd"),
        Err(FamilyError::BadFirst { ch: 'Z' })
    );
    assert_eq!(
        FileFamily::new("zbd!"),
        Err(FamilyError::BadCharacter { ch: '!' })
    );
    assert!(matches!(
        FileFamily::new(&"z".repeat(MAX_FAMILY_LABEL_LEN + 1)),
        Err(FamilyError::TooLong { .. })
    ));
}

/// The logical identity (AC01): host root, row order and letter case stay
/// out of it; content, size and every byte of data stay in it.
#[test]
fn accept_f02_a_logical_identity_tracks_data_not_host_paths() {
    let rows = vec![record("PLANES.ZBD", 64, 1), record("Media/Tick.Wav", 32, 2)];
    let host_a = manifest("/Games/CrimsonSkies", rows.clone()).expect("valid manifest");
    let host_b = manifest("/games/crimsonskies", rows.clone()).expect("valid manifest");

    assert_ne!(host_a.host_root, host_b.host_root);
    assert_eq!(
        host_a.logical_identity(),
        host_b.logical_identity(),
        "the host root must not reach the logical identity"
    );
    assert!(
        host_a
            .logical_identity()
            .as_str()
            .starts_with(INSTALL_IDENTITY_HEADER),
        "the identity carries its versioned canonical header"
    );

    let mut reversed = rows.clone();
    reversed.reverse();
    let reordered = manifest("/Games/CrimsonSkies", reversed).expect("valid manifest");
    assert_eq!(
        host_a.logical_identity(),
        reordered.logical_identity(),
        "row order must not reach the identity"
    );

    let cased = manifest(
        "/Games/CrimsonSkies",
        vec![record("planes.zbd", 64, 1), record("media/tick.wav", 32, 2)],
    )
    .expect("valid manifest");
    assert_eq!(
        host_a.logical_identity(),
        cased.logical_identity(),
        "letter case in spellings must not reach the identity"
    );

    let edited = manifest(
        "/Games/CrimsonSkies",
        vec![record("PLANES.ZBD", 64, 1), record("Media/Tick.Wav", 32, 3)],
    )
    .expect("valid manifest");
    assert_ne!(
        host_a.logical_identity(),
        edited.logical_identity(),
        "a one-byte content change must change the identity"
    );

    let resized = manifest(
        "/Games/CrimsonSkies",
        vec![record("PLANES.ZBD", 65, 1), record("Media/Tick.Wav", 32, 2)],
    )
    .expect("valid manifest");
    assert_ne!(
        host_a.logical_identity(),
        resized.logical_identity(),
        "a size change must change the identity"
    );

    let mut analyzed = manifest("/Games/CrimsonSkies", rows).expect("valid manifest");
    analyzed.files[0].role = FileRole::PlatformSupport;
    analyzed.files[0].parse_state = ParseState::Failed {
        diagnostic: "authored test failure".to_owned(),
    };
    let analyzed = InstallManifest::new(analyzed.host_root, analyzed.files)
        .expect("reclassified rows are structurally valid");
    assert_eq!(
        host_a.logical_identity(),
        analyzed.logical_identity(),
        "analysis results are not installation data and must not reach the identity"
    );
}

/// The compatibility profile keeps its four dimensions independent
/// (docs/01-ARCHITECTURE.md, "Compatibility profiles"): presentation never
/// relabels the rules, unknown stays unknown, and an edition is tied to the
/// installation's logical identity.
#[test]
fn accept_f02_a_compatibility_profile_dimensions_stay_independent() {
    let manifest_a =
        manifest("/Games/CrimsonSkies", vec![record("PLANES.ZBD", 64, 1)]).expect("valid manifest");
    let manifest_b =
        manifest("/games/crimsonskies", vec![record("PLANES.ZBD", 64, 1)]).expect("valid manifest");

    let edition_of = |manifest: &InstallManifest| {
        InstallationEdition::new(
            manifest.logical_identity(),
            [
                InstallationClass::Patched,
                InstallationClass::Full,
                InstallationClass::Patched,
            ],
            Some(LocaleLabel::new("synthetic").expect("fixture locale is valid")),
        )
    };
    let edition_a = edition_of(&manifest_a);
    let edition_b = edition_of(&manifest_b);
    assert_eq!(
        edition_a, edition_b,
        "profiles describing cased copies of one installation share their edition"
    );
    assert_eq!(
        edition_a.classes,
        [InstallationClass::Full, InstallationClass::Patched],
        "classes are canonicalized: sorted, deduplicated, never conflated into one label"
    );
    assert_eq!(InstallationClass::DemoLike.label(), "demo-like");
    assert_eq!(InstallationClass::Partial.label(), "partial");

    let profile = |presentation: Option<&str>, assists_mods: Option<&str>| CompatibilityProfile {
        installation: edition_a.clone(),
        rules: RuleCompatibility::Stock,
        presentation: presentation
            .map(DimensionLabel::new)
            .transpose()
            .expect("fixture dimension label is valid"),
        assists_mods: assists_mods
            .map(DimensionLabel::new)
            .transpose()
            .expect("fixture dimension label is valid"),
    };
    let stock = profile(Some("original-fidelity"), None);
    let enhanced = profile(Some("enhanced-shaders"), Some("assists-on"));
    assert_eq!(
        stock.rules, enhanced.rules,
        "presentation and assist dimensions must not relabel the rule dimension"
    );
    assert_eq!(stock.installation, enhanced.installation);
    assert_ne!(stock.presentation, enhanced.presentation);

    let modified = CompatibilityProfile {
        installation: edition_a.clone(),
        rules: RuleCompatibility::modified("test mission ruleset").expect("non-empty note"),
        presentation: Some(DimensionLabel::new("original-fidelity").expect("valid label")),
        assists_mods: None,
    };
    assert_eq!(modified.validate(), Ok(()));
    assert_ne!(
        modified.rules,
        RuleCompatibility::Stock,
        "a modified ruleset is structurally never the stock one"
    );

    let unknown = CompatibilityProfile {
        installation: edition_a.clone(),
        rules: RuleCompatibility::Unknown,
        presentation: None,
        assists_mods: None,
    };
    assert_eq!(unknown.validate(), Ok(()));
    assert_ne!(
        unknown.rules,
        RuleCompatibility::Stock,
        "an undetermined ruleset must not read as stock"
    );

    // Construction and admission failures, named by error.
    assert_eq!(LocaleLabel::new("  "), Err(ProfileError::EmptyLocale));
    assert_eq!(
        LocaleLabel::new(&"x".repeat(MAX_LOCALE_LABEL_LEN + 1)),
        Err(ProfileError::LocaleTooLong {
            len: MAX_LOCALE_LABEL_LEN + 1
        })
    );
    assert_eq!(
        DimensionLabel::new(""),
        Err(ProfileError::EmptyDimensionLabel)
    );
    assert!(matches!(
        DimensionLabel::new(&"x".repeat(MAX_DIMENSION_LABEL_LEN + 1)),
        Err(ProfileError::DimensionTooLong { .. })
    ));
    assert_eq!(
        RuleCompatibility::modified(" "),
        Err(ProfileError::EmptyModifiedNote)
    );

    let empty_note = CompatibilityProfile {
        installation: edition_a,
        rules: RuleCompatibility::Modified {
            note: String::new(),
        },
        presentation: None,
        assists_mods: None,
    };
    assert_eq!(
        empty_note.validate(),
        Err(ProfileError::EmptyModifiedNote),
        "a profile built through the public enum variants is still admitted only if valid"
    );
}
