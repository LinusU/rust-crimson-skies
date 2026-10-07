//! Shared retail read for the F44-D acceptance test and its evidence harness
//! (task #183, `specs/F44-aircraft-construction-budgets-loadouts-and-paint-editor.md`
//! stage `### F44-D`).
//!
//! It is a `tests/` subdirectory module, not a test target of its own: it has
//! no `#[test]`, and cargo never discovers `tests/<dir>/mod.rs` as a binary.
//! Both `accept_f44_d_retail_construction_surface.rs` and
//! `evidence_report_f44_d.rs` include it with
//! `#[path = "f44_d_support/mod.rs"]`, so the acceptance suite and the evidence
//! artifact are two readings of the same installation through **one** reader —
//! a second reader would be able to disagree with the assertions.
//!
//! # What is measured, and what deliberately never leaves this file
//!
//! **Measured (ids, counts, digests only):** the purchase screen's budget rows
//! and the engine callbacks that fill them, the resource header's purchase
//! refusal ids, the slot counts each construction screen declares (array sizes
//! and loop bounds), and the construction screen's weight/cost field ids. All
//! of them come from members of `GOSDATA/ASSETS/crimson.rof`, read through
//! `cs_assets`' production ROF mount.
//!
//! **Never returned, printed or committed:** any original *display text*. The
//! weight and cost a row shows are produced by an engine callback, so no
//! shipped file holds the number to leak; what this module can read are the
//! label ids, the callback ids and the screens' code shapes. `AGENTS.md`
//! rule 3 is satisfied by not returning file contents beyond the identifiers
//! this task commits, not by filtering them afterwards.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use cs_assets::install::{self, content_fingerprint, fingerprint, sha256};
use cs_assets::rof::mount_rof_into;
use cs_assets::vfs::{INSTALL_NAMESPACE, MountBuilder, SessionBuilder};
use cs_types::asset_id::{
    AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext, SourceSpan,
};
use cs_types::install::InstallManifest;

/// The retail container the construction screens live in.
pub const BASE_CONTAINER: &str = "GOSDATA/ASSETS/crimson.rof";

/// The members this stage reads, spelled as the installation spells them.
pub const MEMBERS: [&str; 7] = [
    "ASSETS/SCRIPTS/PURCHASE.SCRIPT",
    "ASSETS/SCRIPTS/PLANECONSTRUCTION.SCRIPT",
    "ASSETS/SCRIPTS/ARMOR.SCRIPT",
    "ASSETS/SCRIPTS/GUNS.SCRIPT",
    "ASSETS/SCRIPTS/HARDPOINTS.SCRIPT",
    "ASSETS/SCRIPTS/ORDINANCELAYOUT.SCRIPT",
    "ASSETS/SCRIPTS/RESOURCE.H",
];

/// The purchase screen: every weight-and-cost row and its fill callback.
pub const PURCHASE: &str = "ASSETS/SCRIPTS/PURCHASE.SCRIPT";
/// The construction screen: the plane slots and the weight/cost fields.
pub const PLANE_CONSTRUCTION: &str = "ASSETS/SCRIPTS/PLANECONSTRUCTION.SCRIPT";
/// The armor page: the four armor zones.
pub const ARMOR: &str = "ASSETS/SCRIPTS/ARMOR.SCRIPT";
/// The gun page: the four gun positions.
pub const GUNS: &str = "ASSETS/SCRIPTS/GUNS.SCRIPT";
/// The hardpoint page: the two hardpoint points.
pub const HARDPOINTS: &str = "ASSETS/SCRIPTS/HARDPOINTS.SCRIPT";
/// The ordnance layout: four gun slots and eight rocket slots.
pub const ORDINANCE_LAYOUT: &str = "ASSETS/SCRIPTS/ORDINANCELAYOUT.SCRIPT";
/// The engine's resource header: the refusal and field ids.
pub const RESOURCE_HEADER: &str = "ASSETS/SCRIPTS/RESOURCE.H";

/// The code shapes each count is read from, each with the member it lives in
/// and what its presence proves.
///
/// Array sizes and loop bounds are counts; a control's `@globals@AR` argument
/// is **not** — it is a control parameter whose value differs per screen
/// (`5`, `7`, `11`, `12`, `13`, `26`, `27`, `32`) and says nothing about how
/// many entries a list holds. Reading it as a count is the mistake this list
/// exists to avoid, so no entry here uses one.
pub const COUNT_BOUNDS: [(&str, &str, &str); 10] = [
    (
        "ASSETS/SCRIPTS/GUNS.SCRIPT",
        "object ES[4]",
        "the gun page's four gun-position dropdowns",
    ),
    (
        "ASSETS/SCRIPTS/GUNS.SCRIPT",
        "object DS[4]",
        "the gun page's four gun-position titles",
    ),
    (
        "ASSETS/SCRIPTS/GUNS.SCRIPT",
        "int AS[4]",
        "the gun page's four-element selection array",
    ),
    (
        "ASSETS/SCRIPTS/ARMOR.SCRIPT",
        "object X[4]",
        "the armor page's four armor-point dropdowns",
    ),
    (
        "ASSETS/SCRIPTS/HARDPOINTS.SCRIPT",
        "object DT[2]",
        "the hardpoint page's two point dropdowns",
    ),
    (
        "ASSETS/SCRIPTS/ORDINANCELAYOUT.SCRIPT",
        "object RKA[8]",
        "the ordnance layout's eight rocket dropdowns",
    ),
    (
        "ASSETS/SCRIPTS/ORDINANCELAYOUT.SCRIPT",
        "object QKA[4]",
        "the ordnance layout's four gun-ammo dropdowns",
    ),
    (
        "ASSETS/SCRIPTS/ORDINANCELAYOUT.SCRIPT",
        "object PKA[4]",
        "the ordnance layout's four gun-name rows",
    ),
    (
        "ASSETS/SCRIPTS/PLANECONSTRUCTION.SCRIPT",
        "object HMA[4]",
        "the construction screen's four saved-plane slots",
    ),
    (
        "ASSETS/SCRIPTS/ARMOR.SCRIPT",
        "for (R = 0; R < 4; R++)",
        "the loop that fills the four armor points",
    ),
];

/// The read-only installation root, or a loud failure: a retail test must fail,
/// not pass, when `CS_GAME_DIR` is absent.
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
///
/// Production discovery hashes every file of the installation, so it runs a
/// single time and is reused: every span below is bound to the *same*
/// installation digest, which is what stops a report from one installation
/// being reused for another's numbers.
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

/// Opens one retail member's bytes through the production ROF mount.
///
/// The mount answers *resolution* and the `RofSource` answers *bytes*: a ROF
/// member lives inside its container and may be compressed, so the session's
/// directory read reports `no backing` for it **by design** and the production
/// member reader is the byte path. The session is still used, to prove the
/// member resolves through the mount before a byte is read.
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

/// The [`SourceSpan`] of `needle` inside one retail member: which installation,
/// which container, which member, which bytes and what digest.
///
/// # Panics
///
/// When the member does not contain `needle` — a claim with no span is not a
/// claim this stage records.
pub fn span_of(
    install: cs_types::evidence::ContentHash,
    member: &str,
    haystack: &[u8],
    needle: &str,
) -> SourceSpan {
    let offset = find(haystack, needle.as_bytes()).unwrap_or_else(|| {
        panic!("the member must contain {needle:?} for this claim to have a span")
    });
    SourceSpan::new(
        install,
        BASE_CONTAINER,
        Some(member),
        offset as u64,
        needle.len() as u64,
        Some(sha256(haystack)),
    )
    .expect("a span inside one named member is valid")
}

/// One member's decoded length and SHA-256, for the evidence artifact.
pub fn member_digest(root: &Path, spelling: &str) -> (u64, String) {
    let bytes = read_member(root, spelling);
    (bytes.len() as u64, sha256(&bytes).to_hex())
}
