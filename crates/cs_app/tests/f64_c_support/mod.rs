//! Shared fixtures for the F64-C acceptance tests and their evidence harness
//! (task #256, `specs/F64-legacy-custom-aircraft-and-optional-save-import.md`
//! stage `### F64-C`).
//!
//! It is a `tests/` subdirectory module, not a test target of its own: it has
//! no `#[test]`, and cargo never discovers `tests/<dir>/mod.rs` as a binary.
//! `accept_f64_c_import_report.rs` and `evidence_report_f64_c.rs` include it
//! with `#[path = "f64_c_support/mod.rs"]`, so the acceptance suite and the
//! evidence artifact are two readings of the same boundary through **one**
//! fixture construction — a second one could disagree with the assertions.
//!
//! # What is fixture, and what is production
//!
//! Everything here is newly authored **designed** fixture content: the record
//! bytes, the catalog elements, the id bindings and the stock rule profiles
//! come from the same production `synthetic_*` constructors F64-B's tests
//! use. The layout (`synthetic_blueprint_layout`) and the field map
//! (`synthetic_blueprint_map`) are `ClaimStatus::Designed` and are only
//! reachable because the tests name
//! [`cs_content::legacy_import::LayoutAdmission::AllowDesignedFixtures`].
//! No test reads `$CS_GAME_DIR` here (the retail test does, in its own
//! file), and nothing in this module is a claim about an original stored-plane
//! format: F64-B's retail measurement established that no such file ships with
//! the installation.

#![allow(dead_code)]

use cs_app::ui::import::{ImportContext, ImportOffer};
use cs_content::catalog::Catalog;
use cs_content::construction::{
    ConstructionPolicy, ConstructionRules, PriceBook, SYNTHETIC_AIRFRAME_KEY, SYNTHETIC_ENGINE_KEY,
    SYNTHETIC_GUN_KEY, SYNTHETIC_MISSILE_KEY, WeightUnits, declared_synthetic_price_book,
    synthetic_boundary_rules, synthetic_policy,
};
use cs_content::legacy_import::{
    BlueprintFieldMap, LegacyIdBinding, LegacyIdMap, TargetProfile, synthetic_blueprint_map,
};
use cs_formats::legacy_profile::{
    ArtifactProposal, ArtifactProposalError, LegacyArtifactClass, LegacyIdClass, LegacyLayout,
    LegacyLimits, synthetic_blueprint_layout,
};
use cs_types::content::{
    CatalogElement, ConsumerKind, ContentId, ContentKind, Dependency, DependencyKind, Known,
    NormalizeState, Origin, Provenance, Readiness, Resolved, RuntimeConsumer, UnsupportedReason,
};
use cs_types::evidence::ClaimId;
use cs_types::install::ParseState;
use cs_types::profile::{ProfileId, ProfileKind};

/// A fixture content identity.
pub fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("the fixture id is valid")
}

/// Fixture provenance: designed, never measured.
pub fn designed(value: &str) -> Provenance {
    Provenance::designed(ClaimId::new(value).expect("the fixture claim id is valid"))
}

/// One ready catalog element.
pub fn ready_element(element_id: &ContentId) -> CatalogElement {
    CatalogElement {
        kind: element_id.kind(),
        id: element_id.clone(),
        display_name: Some(format!("Authored {element_id}")),
        origin: Origin::SyntheticFixture,
        dependencies: vec![Dependency {
            target: element_id.clone(),
            kind: DependencyKind::Static,
            provenance: designed("f64c.test.dependency"),
        }],
        parse_state: ParseState::Parsed,
        normalize_state: NormalizeState::Normalized,
        runtime_consumers: vec![RuntimeConsumer {
            kind: ConsumerKind::Gameplay,
            provenance: designed("f64c.test.consumer"),
        }],
        readiness: Readiness::Ready,
        unsupported_reasons: Vec::<UnsupportedReason>::new(),
        fingerprint: None,
    }
}

/// Which fixture rows the catalog holds.
///
/// A [`Catalog`] enumerates in canonical id order, so the *position* an
/// identity sits at is decided by the rows around it. [`Self::Shifted`] keeps
/// every bound identity of [`Self::Base`] but moves each of them to a
/// different position by adding two unrelated rows — exactly what a
/// reordering of catalog entries does to a consumer that reads positions. A
/// consumer that reads identities is unaffected, and that is the whole
/// scenario of sheet AC03.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogRows {
    /// The four fixture components the id table binds.
    Base,
    /// The same four, plus an extra airframe and an extra weapon that sit in
    /// front of them in canonical order.
    Shifted,
}

/// The four fixture components, in identity order.
fn base_elements() -> [ContentId; 4] {
    [
        cid(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY),
        cid(ContentKind::Engine, SYNTHETIC_ENGINE_KEY),
        cid(ContentKind::Weapon, SYNTHETIC_GUN_KEY),
        cid(ContentKind::Weapon, SYNTHETIC_MISSILE_KEY),
    ]
}

/// A catalog holding the fixture rows, all ready.
///
/// Insertion order is deliberately irrelevant: the catalog enumerates by
/// identity, so both variants are filled in sorted order and differ only in
/// which rows exist.
pub fn catalog(rows: CatalogRows) -> Catalog {
    let mut ids = base_elements().to_vec();
    if rows == CatalogRows::Shifted {
        ids.push(cid(ContentKind::Airframe, "fixture.synthetic_alpha"));
        ids.push(cid(ContentKind::Weapon, "fixture.synthetic_autocannon"));
        ids.sort();
    }
    let mut catalog = Catalog::new();
    for element_id in ids {
        catalog
            .insert(ready_element(&element_id))
            .expect("the fixture element inserts");
    }
    catalog
}

/// Where an identity sits in the catalog's canonical enumeration.
pub fn position_of(catalog: &Catalog, id: &ContentId) -> Option<usize> {
    catalog
        .sorted_ids()
        .into_iter()
        .position(|candidate| candidate == id)
}

/// The legacy ids the fixture documents carry: airframe 7, engine 8, gun 1,
/// missile (weapon class) 2 and ordnance 5.
pub fn id_map() -> LegacyIdMap {
    let mut ids = LegacyIdMap::new();
    for binding in [
        (
            LegacyIdClass::Airframe,
            7,
            ContentKind::Airframe,
            SYNTHETIC_AIRFRAME_KEY,
        ),
        (
            LegacyIdClass::Engine,
            8,
            ContentKind::Engine,
            SYNTHETIC_ENGINE_KEY,
        ),
        (
            LegacyIdClass::Weapon,
            1,
            ContentKind::Weapon,
            SYNTHETIC_GUN_KEY,
        ),
        (
            LegacyIdClass::Weapon,
            2,
            ContentKind::Weapon,
            SYNTHETIC_MISSILE_KEY,
        ),
        (
            LegacyIdClass::Ordnance,
            5,
            ContentKind::Weapon,
            SYNTHETIC_MISSILE_KEY,
        ),
    ] {
        ids.insert(
            LegacyIdBinding::new(binding.0, binding.1, cid(binding.2, binding.3))
                .expect("the binding is in its namespace"),
        )
        .expect("the binding inserts");
    }
    ids
}

/// The same table with its bindings declared in the opposite order.
///
/// The table is canonical by `(class, raw)`, so declaration order is as
/// irrelevant as catalog row order — and a consumer that read bindings by
/// position would disagree with [`id_map`].
pub fn id_map_reordered() -> LegacyIdMap {
    let mut ids = LegacyIdMap::new();
    for binding in [
        (
            LegacyIdClass::Ordnance,
            5,
            ContentKind::Weapon,
            SYNTHETIC_MISSILE_KEY,
        ),
        (
            LegacyIdClass::Weapon,
            2,
            ContentKind::Weapon,
            SYNTHETIC_MISSILE_KEY,
        ),
        (
            LegacyIdClass::Weapon,
            1,
            ContentKind::Weapon,
            SYNTHETIC_GUN_KEY,
        ),
        (
            LegacyIdClass::Engine,
            8,
            ContentKind::Engine,
            SYNTHETIC_ENGINE_KEY,
        ),
        (
            LegacyIdClass::Airframe,
            7,
            ContentKind::Airframe,
            SYNTHETIC_AIRFRAME_KEY,
        ),
    ] {
        ids.insert(
            LegacyIdBinding::new(binding.0, binding.1, cid(binding.2, binding.3))
                .expect("the binding is in its namespace"),
        )
        .expect("the binding inserts");
    }
    ids
}

/// One record of the blueprint fixture document: a name plus the airframe,
/// engine, four gun and two rocket id slots.
pub struct BlueprintRecord {
    /// The record's text field (uninterpreted).
    pub name: &'static str,
    /// `airframe_id`.
    pub airframe: u32,
    /// `engine_id`.
    pub engine: u32,
    /// `gun_1`..`gun_4`.
    pub guns: [u32; 4],
    /// `rocket_1`, `rocket_2`.
    pub rockets: [u32; 2],
}

/// The full record: every declared slot bound to a ready component.
pub fn full_record() -> BlueprintRecord {
    BlueprintRecord {
        name: "sparrow",
        airframe: 7,
        engine: 8,
        guns: [1, 1, 1, 1],
        rockets: [5, 5],
    }
}

/// A synthetic document for `synthetic_blueprint_layout`.
pub fn blueprint_document(records: &[BlueprintRecord]) -> Vec<u8> {
    let layout = synthetic_blueprint_layout();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&layout.magic());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(
        &u32::try_from(records.len())
            .expect("the fixture record count fits")
            .to_le_bytes(),
    );
    let mut label = [0u8; 8];
    label[..6].copy_from_slice(b"planes");
    bytes.extend_from_slice(&label);
    for record in records {
        let mut name = [0u8; 12];
        let taken = record.name.len().min(12);
        name[..taken].copy_from_slice(&record.name.as_bytes()[..taken]);
        bytes.extend_from_slice(&name);
        bytes.extend_from_slice(&record.airframe.to_le_bytes());
        bytes.extend_from_slice(&record.engine.to_le_bytes());
        for gun in record.guns {
            bytes.extend_from_slice(&gun.to_le_bytes());
        }
        for rocket in record.rockets {
            bytes.extend_from_slice(&rocket.to_le_bytes());
        }
    }
    bytes
}

/// A synthetic document for `synthetic_layout` — the F64-A profile fixture,
/// whose records carry an airframe id, a weapon id and a text field.
pub fn profile_document(records: &[(u32, u32, &str)]) -> Vec<u8> {
    let layout = cs_formats::legacy_profile::synthetic_layout();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&layout.magic());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(
        &u32::try_from(records.len())
            .expect("the fixture record count fits")
            .to_le_bytes(),
    );
    let mut label = [0u8; 8];
    label[..6].copy_from_slice(b"label1");
    bytes.extend_from_slice(&label);
    for (airframe, weapon, name) in records {
        bytes.extend_from_slice(&airframe.to_le_bytes());
        bytes.extend_from_slice(&weapon.to_le_bytes());
        let mut field = [0u8; 8];
        let taken = name.len().min(8);
        field[..taken].copy_from_slice(&name.as_bytes()[..taken]);
        bytes.extend_from_slice(&field);
    }
    bytes
}

/// The spellings the offers in these tests declare.
pub const FIXTURE_SPELLING: &str = "Planes/custom.pln";

/// A proposal for `bytes` that declares the digest those bytes actually have.
///
/// The digest is the production `cs_assets::install::sha256`, not a fixture
/// constant: the planner verifies the declared fingerprint against the
/// supplied bytes, so a proposal that invented a digest would be refused
/// before the document was ever read.
pub fn proposal(
    bytes: &[u8],
    class: LegacyArtifactClass,
) -> Result<ArtifactProposal, ArtifactProposalError> {
    ArtifactProposal::new(
        FIXTURE_SPELLING,
        bytes.len() as u64,
        cs_assets::install::sha256(bytes),
        Some(class),
    )
}

/// The new profile an import would land in.
pub fn target() -> TargetProfile {
    TargetProfile::new(
        ProfileId::new(9).expect("the fixture profile id is not zero"),
        ProfileKind::Production,
        "Imported legacy profile",
    )
    .expect("the target profile names itself")
}

/// The stock rules, policy and price book the blueprint stage judges with.
pub fn stock() -> (ConstructionRules, ConstructionPolicy, PriceBook) {
    (
        synthetic_boundary_rules(),
        synthetic_policy(),
        declared_synthetic_price_book(),
    )
}

/// A rule profile for the fixture airframe with a tighter weight ceiling, so
/// the full record's 5370-unit blueprint is over it (sheet AC02).
pub fn tight_mass_rules(limit: u64) -> ConstructionRules {
    ConstructionRules::try_new(
        cid(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY),
        known(4),
        known(8),
        known(WeightUnits::new(limit)),
        known(cs_content::construction::MoneyMinor::new(42_000)),
        Origin::SyntheticFixture,
        designed("f64c.rules.tight-mass"),
    )
    .expect("the rule profile is valid")
}

/// A known value with designed provenance.
pub fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, designed("f64c.test.value")))
}

/// The context a test offers through: the declared id table, the catalog, the
/// stock rules and the optional-enhancement switch.
pub fn context<'a>(
    ids: &'a LegacyIdMap,
    catalog: &'a Catalog,
    rules: &'a ConstructionRules,
    policy: &'a ConstructionPolicy,
    book: &'a PriceBook,
    legacy_save_import_enabled: bool,
) -> ImportContext<'a> {
    ImportContext {
        ids,
        catalog,
        rules,
        policy,
        book,
        admission: cs_content::legacy_import::LayoutAdmission::AllowDesignedFixtures,
        legacy_save_import_enabled,
        origin: Origin::SyntheticFixture,
        provenance: designed("f64c.test.blueprint"),
    }
}

/// One offer: the fixture source with a layout and, optionally, a field map.
pub fn offer<'a>(
    source: &'a ArtifactProposal,
    bytes: &'a [u8],
    layout: Option<&'a LegacyLayout>,
    field_map: Option<&'a BlueprintFieldMap>,
    target: &'a TargetProfile,
) -> ImportOffer<'a> {
    ImportOffer {
        source,
        bytes,
        layout,
        limits: LegacyLimits::designed(),
        field_map,
        target,
        install_identity: None,
    }
}

/// The fixture layout and field map the blueprint path reads through, as a
/// pair — the two always travel together because the field map is validated
/// against exactly this layout.
pub fn blueprint_layout_and_map() -> (LegacyLayout, BlueprintFieldMap) {
    (synthetic_blueprint_layout(), synthetic_blueprint_map())
}

/// A temp directory that removes itself, labelled for the test that made it.
pub struct TempBase(std::path::PathBuf);

impl TempBase {
    /// Creates a fresh, empty directory under the system temp directory.
    pub fn new(label: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        use std::sync::atomic::Ordering;
        let root = std::env::temp_dir().join(format!(
            "cs-f64-c-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the fixture base is created");
        Self(root)
    }

    /// The directory's path.
    pub fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempBase {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Every file below `root`, as `(relative spelling, bytes)` in sorted order.
///
/// The comparison is whole-tree, so a file created, removed or rewritten
/// anywhere under the destination shows up — not just a file a test happens
/// to know about.
pub fn tree(root: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    use std::fs;
    use std::path::PathBuf;

    let mut rows = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                stack.push(PathBuf::from(&path));
            } else {
                let spelling = path
                    .strip_prefix(root)
                    .expect("every walked path is under the root")
                    .to_string_lossy()
                    .into_owned();
                let bytes = fs::read(&path).unwrap_or_default();
                rows.push((spelling, bytes));
            }
        }
    }
    rows.sort();
    rows
}
