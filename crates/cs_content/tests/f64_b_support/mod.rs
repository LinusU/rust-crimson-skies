//! Shared retail read for the F64-B acceptance test and its evidence harness
//! (task #255, `specs/F64-legacy-custom-aircraft-and-optional-save-import.md`
//! stage `### F64-B`).
//!
//! It is a `tests/` subdirectory module, not a test target of its own: it has
//! no `#[test]`, and cargo never discovers `tests/<dir>/mod.rs` as a binary.
//! Both `accept_f64_b_retail_import_surface.rs` and
//! `evidence_report_f64_b.rs` include it with
//! `#[path = "f64_b_support/mod.rs"]`, so the acceptance suite and the
//! evidence artifact are two readings of the same installation through **one**
//! reader — a second reader would be able to disagree with the assertions.
//!
//! # What is measured, and what deliberately never leaves this file
//!
//! **Measured (spellings, templates, digests only):** that the installation
//! ships no legacy save or custom-plane file — the runtime-created storage
//! the engine image's own path templates name — the save and plane path
//! templates in the owner-supplied decrypted engine image
//! (`crimson.decrypted.exe`, decrypted from `crimson.icd` by the owner, as
//! `docs/findings/2026-09-29-f12-j-letter-o-colour.md` records), and the
//! saved-plane slot the construction screen fills.
//!
//! **Never returned, printed or committed:** any original file content beyond
//! the templates and identifiers this task names. A template's *offset* and
//! the executable's digest are recorded; the bytes around it are not.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use cs_assets::install::{self, content_fingerprint, fingerprint, sha256};
use cs_assets::rof::mount_rof_into;
use cs_assets::vfs::{INSTALL_NAMESPACE, MountBuilder, SessionBuilder};
use cs_types::asset_id::{AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext};
use cs_types::install::InstallManifest;

/// The retail container the construction screen lives in.
pub const BASE_CONTAINER: &str = "GOSDATA/ASSETS/crimson.rof";

/// The member this stage measures a reference in: the construction screen
/// fills the saved-plane slots that make `CustomAircraft` a referenced class.
pub const PLANE_CONSTRUCTION: &str = "ASSETS/SCRIPTS/PLANECONSTRUCTION.SCRIPT";

/// The owner-supplied decrypted engine image. Its `.text` is the decrypted
/// `crimson.icd` code — the packed original image holds these bytes — so its
/// path templates are original file content measured read-only. Its
/// provenance is the owner's decryption, which is why the tests name it
/// explicitly and never claim `crimson.exe` itself was read for them.
pub const ENGINE_IMAGE: &str = "crimson.decrypted.exe";

/// The save/plane path templates observed inside the engine image, spelled
/// exactly as the bytes spell them.
///
/// Each template proves where a *class* of runtime file lives, not what it
/// contains: `Planes\%s` is one saved plane per profile, `SavedGames\%s\...`
/// is the per-profile save directory, and the `%s`-prefixed templates hang
/// those files under a profile spelling. None of the files they name ships
/// with the installation — the tests assert that separately.
pub const STORAGE_TEMPLATES: [(&str, &str); 9] = [
    ("Planes\\*.*", "the saved-plane directory the engine lists"),
    (
        "Planes\\%s",
        "one saved custom plane per profile, opened for writing",
    ),
    ("SavedGames\\%s\\AutoSave.sav", "the per-profile autosave"),
    (
        "SavedGames\\%s\\*.sav",
        "the per-profile save directory listing",
    ),
    ("%s\\Status.dat", "a per-profile status record"),
    ("%s\\Mission.%1d%02d", "per-campaign/per-mission save files"),
    (
        "%s\\Persist.%1d%02d",
        "per-campaign/per-mission persistence files",
    ),
    ("%s\\%s.sav", "a named save inside the profile directory"),
    (
        "SOFTWARE\\Microsoft\\Microsoft Games\\Crimson Skies\\1.0",
        "the registry key the engine writes under",
    ),
];

/// The saved-plane slot code shapes in `PLANECONSTRUCTION.SCRIPT`.
///
/// `object HMA[4]` declares the four slots — the same count F44-D measured as
/// [`crate`]-external `ORIGINAL_PLANE_SLOTS` — `HMA[R].YC` names each slot's
/// plane label and `callback($$E$$, 2243, ...)` is the engine call that fills
/// slot `R` from a stored plane. This is the measured reference that switched
/// `LEGACY_LAYOUT_INVENTORY[CustomAircraft]`'s requirement on.
pub const PLANE_SLOT_MARKERS: [(&str, &str); 3] = [
    (
        "object HMA[4]",
        "the construction screen's four saved-plane slots",
    ),
    (
        "HMA[R].YC = \"px_p_plane\" conv$(R)",
        "each slot is labelled from a stored plane's name",
    ),
    (
        "callback($$E$$, 2243, (R), HMA[R].AK, 1)",
        "the engine call that fills slot R from a stored plane",
    ),
];

/// Spellings a shipped legacy artifact would take, none of which may be in
/// the inventory. The check is against *every inventoried file*, so a file
/// the list does not name cannot slip past.
pub const ABSENT_SHAPES: [&str; 6] = [
    "SavedGames",
    "Planes\\",
    ".sav",
    "Status.dat",
    "Mission.",
    "Persist.",
];

/// The read-only installation root, or a loud failure: a retail test must
/// fail, not pass, when `CS_GAME_DIR` is absent.
pub fn game_dir() -> PathBuf {
    let dir = std::env::var_os("CS_GAME_DIR").expect(
        "CS_GAME_DIR is not set: this measurement needs the original installation \
         (capability `retail`)",
    );
    let dir = PathBuf::from(dir);
    assert!(
        dir.is_dir(),
        "CS_GAME_DIR {} is not a directory",
        dir.display()
    );
    dir
}

/// The installation's manifest, discovered once.
pub fn installation(root: &Path) -> InstallManifest {
    static CACHE: OnceLock<InstallManifest> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            install::discover(root)
                .unwrap_or_else(|error| panic!("the installation must be discoverable: {error:?}"))
                .manifest
        })
        .clone()
}

/// The installation digest (`install_sha256`) and content digest
/// (`content_sha256`) the evidence record requires, both measured here.
pub fn installation_digests(root: &Path) -> (String, String) {
    let manifest = installation(root);
    (
        fingerprint(&manifest).to_hex(),
        content_fingerprint(&manifest).to_hex(),
    )
}

/// The installation digest every span in this stage is bound to.
pub fn install_sha256(root: &Path) -> cs_types::evidence::ContentHash {
    fingerprint(&installation(root))
}

/// Every inventoried file's relative spelling, as the production inventory
/// reports it.
pub fn inventoried_spellings(root: &Path) -> Vec<String> {
    installation(root)
        .files
        .iter()
        .map(|file| file.relative_spelling.as_str().to_owned())
        .collect()
}

/// The owner-supplied decrypted engine image's bytes.
pub fn engine_image(root: &Path) -> Vec<u8> {
    std::fs::read(root.join(ENGINE_IMAGE)).unwrap_or_else(|error| {
        panic!("the engine image {ENGINE_IMAGE} must be readable: {error:?}")
    })
}

/// The engine image's decoded length and SHA-256, for the evidence artifact.
pub fn engine_image_digest(root: &Path) -> (u64, String) {
    let bytes = engine_image(root);
    (bytes.len() as u64, sha256(&bytes).to_hex())
}

/// Opens one retail member's bytes through the production ROF mount — the
/// same reader `f44_d_support` uses.
pub fn read_member(root: &Path, spelling: &str) -> Vec<u8> {
    let install = install_sha256(root);
    let id: String = format!("rof-{}", BASE_CONTAINER.to_ascii_lowercase())
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '.' {
                character
            } else {
                '-'
            }
        })
        .collect();
    let mut builder = SessionBuilder::new(ResolveContext::new(install));
    let source = mount_rof_into(
        &mut builder,
        MountBuilder::new(
            MountId::new(&id).expect("a valid mount id"),
            MountNamespace::new(INSTALL_NAMESPACE).expect("a valid namespace"),
            PrecedenceClass::Shared,
            BASE_CONTAINER,
        )
        .retail(),
        &root.join(BASE_CONTAINER),
    )
    .expect("the base retail archive mounts");
    let session = builder.open();
    let key =
        AssetKey::from_spelling(INSTALL_NAMESPACE, spelling, "default").expect("a valid asset key");
    session
        .resolve(&key)
        .unwrap_or_else(|error| panic!("{spelling}: the member must resolve: {error:?}"));
    source
        .read(&key)
        .unwrap_or_else(|error| panic!("{spelling}: the member must decode: {error:?}"))
        .data
}

/// The offset of `needle` inside `haystack`.
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// One member's decoded length and SHA-256, for the evidence artifact.
pub fn member_digest(root: &Path, spelling: &str) -> (u64, String) {
    let bytes = read_member(root, spelling);
    (bytes.len() as u64, sha256(&bytes).to_hex())
}
