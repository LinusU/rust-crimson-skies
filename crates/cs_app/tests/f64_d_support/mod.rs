//! Shared readers for the F64-D acceptance tests and their evidence harness
//! (task #257, `specs/F64-legacy-custom-aircraft-and-optional-save-import.md`
//! stage `### F64-D`).
//!
//! It is a `tests/` subdirectory module, not a test target of its own: it has
//! no `#[test]`, and cargo never discovers `tests/<dir>/mod.rs` as a binary.
//! `accept_f64_d_optional_save_switch.rs`,
//! `accept_f64_d_retail_custom_aircraft_and_optional_saves.rs` and
//! `evidence_report_f64_d.rs` include it with
//! `#[path = "f64_d_support/mod.rs"]`, so the acceptance suite and the
//! evidence artifact read the same installation and the same declared switch
//! through **one** implementation — a second reader could disagree with the
//! assertions.
//!
//! # What is production here, and what is fixture
//!
//! **Production, called not re-implemented:** `cs_assets::install::discover`
//! for the manifest, `cs_assets::rof` + `cs_assets::vfs` for reading one
//! shipped member, `cs_content::legacy_import::legacy_save_import_rule` for
//! the declared switch, and `cs_app::ui::import::ImportContext` /
//! `ImportFlow` for the consumer the offers run through. The only fixture in
//! this file is the synthetic import context (designed layout admission,
//! authored content ids), which the F64-C support module already builds — the
//! synthetic offers come from there, never from a second construction.
//!
//! **Never returned, printed or committed:** original file content beyond the
//! script markers these tests name. Spellings, sizes and SHA-256 digests are
//! recorded; the bytes around a marker are not.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use cs_app::ui::import::ImportContext;
use cs_assets::install::{self, content_fingerprint, fingerprint, sha256};
use cs_assets::rof::mount_rof_into;
use cs_assets::vfs::{INSTALL_NAMESPACE, MountBuilder, SessionBuilder};
use cs_content::catalog::Catalog;
use cs_content::construction::{ConstructionPolicy, ConstructionRules, PriceBook};
use cs_content::legacy_import::{LayoutAdmission, LegacyIdMap, legacy_save_import_rule};
use cs_content::save::settings::SettingCatalog;
use cs_types::asset_id::{AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext};
use cs_types::content::Origin;
use cs_types::install::InstallManifest;

/// The retail container the construction screen lives in.
pub const BASE_CONTAINER: &str = "GOSDATA/ASSETS/crimson.rof";

/// The member the inventory's `CustomAircraft` row records as the measured
/// original content path that references custom aircraft.
pub const PLANE_CONSTRUCTION: &str = "ASSETS/SCRIPTS/PLANECONSTRUCTION.SCRIPT";

/// The saved-plane slot code shapes in [`PLANE_CONSTRUCTION`], re-derived here
/// rather than imported from another stage's tests: this stage's claim is that
/// the recorded reference still resolves to those shapes in the owner's
/// installation *today*, which is only evidence if this file measures it.
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
/// the inventory (F64-B measured this; F64-D re-derives it so the switch's
/// retail test states it from its own observation).
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

/// The installation's manifest, discovered once per process.
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

/// The installation digest every span in this stage is bound to.
pub fn install_sha256(root: &Path) -> cs_types::evidence::ContentHash {
    fingerprint(&installation(root))
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

/// Every inventoried file's relative spelling, as the production inventory
/// reports it.
pub fn inventoried_spellings(root: &Path) -> Vec<String> {
    installation(root)
        .files
        .iter()
        .map(|file| file.relative_spelling.as_str().to_owned())
        .collect()
}

/// The inventoried file whose spelling names `wanted`, compared the way a
/// case-insensitive engine filesystem compares it.
pub fn find_spelling<'a>(manifest: &'a InstallManifest, wanted: &str) -> Option<&'a str> {
    manifest
        .files
        .iter()
        .find(|file| file.relative_spelling.as_str().eq_ignore_ascii_case(wanted))
        .map(|file| file.relative_spelling.as_str())
}

/// Where a recorded original-content path lives in this installation.
///
/// F64-D measured the distinction this stage's first draft got wrong: the
/// inventory's `referenced_by` paths are **asset spellings**, and
/// `ASSETS/SCRIPTS/PLANECONSTRUCTION.SCRIPT` is a member of the base
/// container, not a file at the installation root. A reference therefore
/// resolves either as an inventoried file or through the production ROF
/// mount, and which of the two it was is part of the evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReferenceSource {
    /// A file at the installation root, by its inventoried spelling.
    InstallFile(String),
    /// A member of the base container, by its asset spelling.
    ContainerMember {
        /// The container's inventoried spelling.
        container: String,
        /// The member's asset spelling.
        spelling: String,
    },
}

impl ReferenceSource {
    /// A stable label for the evidence artifact and for assertion messages.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::InstallFile(spelling) => format!("install file {spelling}"),
            Self::ContainerMember {
                container,
                spelling,
            } => format!("{container}#{spelling}"),
        }
    }
}

/// Resolves a recorded original-content path against this installation.
///
/// An inventoried file wins when the manifest spells the path (a reference
/// could point at a root file); otherwise the path is read as a member of the
/// base container through the production ROF mount. `None` means nothing in
/// this installation resolves the reference — a stale note, which the caller
/// must fail on.
pub fn read_reference(
    root: &Path,
    manifest: &InstallManifest,
    recorded: &str,
) -> Option<(ReferenceSource, Vec<u8>)> {
    if let Some(file) = find_spelling(manifest, recorded) {
        let bytes = std::fs::read(root.join(file)).ok()?;
        return Some((ReferenceSource::InstallFile(file.to_owned()), bytes));
    }
    let container = find_spelling(manifest, BASE_CONTAINER)?.to_owned();
    let bytes = read_member(root, recorded)?;
    Some((
        ReferenceSource::ContainerMember {
            container,
            spelling: recorded.to_owned(),
        },
        bytes,
    ))
}

/// One reference's decoded length and SHA-256, for the evidence artifact.
pub fn reference_digest(
    root: &Path,
    manifest: &InstallManifest,
    recorded: &str,
) -> Option<(ReferenceSource, u64, String)> {
    let (source, bytes) = read_reference(root, manifest, recorded)?;
    Some((source, bytes.len() as u64, sha256(&bytes).to_hex()))
}

/// One member's bytes through the production ROF mount — the same reader
/// F64-B and F44-D measure original members with.
///
/// `None` is "this container does not ship that member". A container that
/// ships but cannot be mounted, or a member that resolves but cannot decode,
/// panics: those are faults in the reader, not an absent reference.
pub fn read_member(root: &Path, spelling: &str) -> Option<Vec<u8>> {
    let container = root.join(BASE_CONTAINER);
    if !container.is_file() {
        return None;
    }
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
        &container,
    )
    .expect("the base retail archive mounts");
    let session = builder.open();
    let key =
        AssetKey::from_spelling(INSTALL_NAMESPACE, spelling, "default").expect("a valid asset key");
    if session.resolve(&key).is_err() {
        return None;
    }
    Some(
        source
            .read(&key)
            .unwrap_or_else(|error| panic!("{spelling}: the member must decode: {error:?}"))
            .data,
    )
}

/// The offset of `needle` inside `haystack`.
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// The settings catalog a population must declare for the optional-save
/// switch to be a real, storable setting.
pub fn settings_catalog() -> SettingCatalog {
    SettingCatalog::new([legacy_save_import_rule()]).expect("the declared switch is a usable rule")
}

/// The import context these tests offer through, with the admission policy
/// named at the call site.
///
/// The switch is a parameter, not a constant: the whole point of F64-D is
/// that both values of it are reachable from one production
/// [`ImportContext`].
pub fn context<'a>(
    ids: &'a LegacyIdMap,
    catalog: &'a Catalog,
    rules: &'a ConstructionRules,
    policy: &'a ConstructionPolicy,
    book: &'a PriceBook,
    legacy_save_import_enabled: bool,
    admission: LayoutAdmission,
) -> ImportContext<'a> {
    ImportContext {
        ids,
        catalog,
        rules,
        policy,
        book,
        admission,
        legacy_save_import_enabled,
        origin: Origin::SyntheticFixture,
        provenance: designed("f64d.test.blueprint"),
    }
}

/// Fixture provenance: designed, never measured.
pub fn designed(value: &str) -> cs_types::content::Provenance {
    cs_types::content::Provenance::designed(
        cs_types::evidence::ClaimId::new(value).expect("the fixture claim id is valid"),
    )
}
