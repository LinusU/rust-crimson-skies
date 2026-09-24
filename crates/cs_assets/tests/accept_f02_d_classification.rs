//! F02-D acceptance through the audit classification (spec F02
//! non-negotiable behavior 4): every file is classified as consumed,
//! needed-unimplemented, optional-media, unused-with-reason,
//! platform-support or unknown — and unknown gameplay dependencies fail
//! completeness, never pass silently.
//!
//! These tests exercise production code only: `cs_assets::install::{classify,
//! in_gameplay_scope}` assigns the roles the audit reports. If the rule set
//! were removed or replaced by `Unknown`-for-everything, every classification
//! assertion here fails; if it claimed unobserved files, the unknown-shape
//! test fails.
//!
//! All keys are newly authored logical keys; the original installation is
//! never touched.

use cs_assets::install::{FileRoleKind, classify, in_gameplay_scope};
use cs_types::install::FileRole;

/// Every file shape observed in the owner's retail installation carries a
/// concrete (non-unknown) role bound to the rule that explains it.
#[test]
fn accept_f02_d_every_retail_file_shape_is_classified() {
    let cases: &[(&str, FileRoleKind)] = &[
        // The zbd content tree: zbd-level, group-level, rtexture and
        // mission-directory archives are all game archives awaiting F06.
        ("zbd/interp.zbd", FileRoleKind::NeededUnimplemented),
        ("zbd/planes.zbd", FileRoleKind::NeededUnimplemented),
        ("zbd/rimage.zbd", FileRoleKind::NeededUnimplemented),
        ("zbd/soundsh.zbd", FileRoleKind::NeededUnimplemented),
        ("zbd/soundsl.zbd", FileRoleKind::NeededUnimplemented),
        ("zbd/zrdr.zbd", FileRoleKind::NeededUnimplemented),
        ("zbd/c1/gamez.zbd", FileRoleKind::NeededUnimplemented),
        ("zbd/c1/cam_anim.zbd", FileRoleKind::NeededUnimplemented),
        ("zbd/c1/texture.zbd", FileRoleKind::NeededUnimplemented),
        ("zbd/c1/zrdr.zbd", FileRoleKind::NeededUnimplemented),
        ("zbd/c1/rtexture15.zbd", FileRoleKind::NeededUnimplemented),
        ("zbd/c2b/rtexture9.zbd", FileRoleKind::NeededUnimplemented),
        ("zbd/c1/m02/mis_anim.zbd", FileRoleKind::NeededUnimplemented),
        ("zbd/c1/m02/zrdr.zbd", FileRoleKind::NeededUnimplemented),
        ("zbd/c1/ia1/zrdr.zbd", FileRoleKind::NeededUnimplemented),
        // The gosdata asset tree.
        (
            "gosdata/assets/crimson.rof",
            FileRoleKind::NeededUnimplemented,
        ),
        (
            "gosdata/assets/crimptch.rof",
            FileRoleKind::NeededUnimplemented,
        ),
        (
            "gosdata/assets/graphics/font.tga",
            FileRoleKind::NeededUnimplemented,
        ),
        (
            "gosdata/assets/graphics/arial8.tga",
            FileRoleKind::NeededUnimplemented,
        ),
        (
            "gosdata/assets/graphics/mpg/chap0.mpg",
            FileRoleKind::OptionalMedia,
        ),
        (
            "gosdata/assets/graphics/mpg/final.mpg",
            FileRoleKind::OptionalMedia,
        ),
        // Native binaries and installer/setup support at every level.
        ("crimson.exe", FileRoleKind::PlatformSupport),
        ("crimson.icd", FileRoleKind::PlatformSupport),
        ("clokspl.exe", FileRoleKind::PlatformSupport),
        ("cszoneregister.exe", FileRoleKind::PlatformSupport),
        ("uninstal.exe", FileRoleKind::PlatformSupport),
        ("mfc42.dll", FileRoleKind::PlatformSupport),
        ("setupenu.dll", FileRoleKind::PlatformSupport),
        (
            "gosdata/assets/binaries/ijl10.dll",
            FileRoleKind::PlatformSupport,
        ),
        (
            "gosdata/assets/binaries/roffile.dll",
            FileRoleKind::PlatformSupport,
        ),
        ("00000409.016", FileRoleKind::PlatformSupport),
        ("00000409.256", FileRoleKind::PlatformSupport),
        ("ebusetup.sem", FileRoleKind::PlatformSupport),
        ("eula.rtf", FileRoleKind::PlatformSupport),
        ("readme.rtf", FileRoleKind::PlatformSupport),
        // The force-feedback effect resource.
        ("crimsonff.ifr", FileRoleKind::NeededUnimplemented),
    ];
    for (key, expected) in cases {
        let classification = classify(key);
        assert_eq!(
            classification.role, *expected,
            "{key} must classify as {expected:?}"
        );
        assert!(
            !classification.basis.is_empty(),
            "{key} carries a basis for its role"
        );
        // The materialized FileRole matches the kind (and an unused
        // classification would carry a non-empty reason by construction).
        let role = classification.role.to_role();
        match (classification.role, role) {
            (FileRoleKind::NeededUnimplemented, FileRole::NeededUnimplemented)
            | (FileRoleKind::OptionalMedia, FileRole::OptionalMedia)
            | (FileRoleKind::PlatformSupport, FileRole::PlatformSupport) => {}
            (kind, role) => panic!("{key}: kind {kind:?} materialized as {role:?}"),
        }
    }
}

/// File shapes never observed in the retail installation stay `Unknown` —
/// the classification reports them instead of guessing a role.
#[test]
fn accept_f02_d_unobserved_shapes_stay_unknown() {
    for key in [
        // A non-zbd file inside the zbd tree: unobserved, unclassified.
        "zbd/c1/notes.txt",
        "zbd/planes.dat",
        // Unobserved content in the gosdata tree.
        "gosdata/assets/mystery.bin",
        "gosdata/assets/graphics/readme.png",
        "gosdata/assets/graphics/mpg/notes.txt",
        // A zbd archive outside the zbd tree and a rof outside gosdata.
        "data/loose.zbd",
        "archives/crimson.rof",
        // An unrecognized root-level file.
        "savegame.sav",
        "settings.ini",
    ] {
        let classification = classify(key);
        assert_eq!(
            classification.role,
            FileRoleKind::Unknown,
            "{key} stays unclassified rather than being guessed"
        );
        assert_eq!(
            classification.role.to_role(),
            FileRole::Unknown,
            "{key} materializes as the explicit unknown role"
        );
    }
}

/// `in_gameplay_scope` discriminates the unclassified files: an unknown
/// inside `zbd/` or `gosdata/` is a gameplay dependency that fails
/// completeness; an unknown outside is reported but is not evidence of
/// missing gameplay content on its own.
#[test]
fn accept_f02_d_gameplay_scope_marks_unclassified_dependencies() {
    for key in [
        "zbd/c1/notes.txt",
        "zbd/odd.bin",
        "gosdata/assets/mystery.bin",
        "gosdata/other/file.dat",
    ] {
        assert!(
            in_gameplay_scope(key),
            "{key} sits inside a gameplay content root"
        );
    }
    for key in [
        "savegame.sav",
        "data/loose.zbd",
        "crimsonff.ifr",
        "readme.rtf",
        "gosdatafile.bin",
        "zbdk/file.zbd",
    ] {
        assert!(
            !in_gameplay_scope(key),
            "{key} sits outside the gameplay content roots"
        );
    }
}
