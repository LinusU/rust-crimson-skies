//! F64-A acceptance tests: the read-only legacy-import plan
//! (`specs/F64-legacy-custom-aircraft-and-optional-save-import.md`, stage
//! `### F64-A`).
//!
//! The stage's minimum scenario is sheet **AC01**: a malicious or oversized old
//! profile fails **without touching the source or any new save**. That is
//! asserted on a real filesystem: a hostile payload is offered to the
//! production `cs_content::legacy_import::plan_import`, and afterwards the
//! source's bytes, its size and its modification time and the whole destination
//! profile directory are compared against the snapshot taken before the call.
//!
//! The other tests pin the contracts the later stages depend on: the
//! full/partial/unsupported split with named unresolved rows, identity-based id
//! resolution that cannot be reordered into a different element, the optional
//! save-import switch, the refusal of a designed layout under the default
//! admission, and the retained source fingerprint.
//!
//! Everything is newly authored synthetic fixture content under the system
//! temporary directory: no test reads `$CS_GAME_DIR`, touches a real user
//! profile directory or any original data, and every tree is removed again when
//! the test finishes. Nothing here is a claim about an original legacy profile,
//! save or custom-aircraft format — those layouts are still unmeasured.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_content::catalog::Catalog;
use cs_content::legacy_import::{
    ImportClass, ImportPlan, ImportRefusal, ImportRequest, LayoutAdmission, LegacyIdBinding,
    LegacyIdMap, LegacyIdMapError, TargetProfile, UnresolvedReason, plan_import,
};
use cs_formats::legacy_profile::{
    ArtifactProposal, ArtifactProposalError, LEGACY_MAGIC_BYTES, LegacyArtifactClass,
    LegacyIdClass, LegacyIdSlot, LegacyLayout, LegacyLimits, LegacyProfileErrorKind, LegacySlot,
    LegacySlotType, MAX_LEGACY_SOURCE_BYTES, TrailingPolicy, synthetic_layout,
};
use cs_types::content::{
    CatalogElement, ConsumerKind, ContentId, ContentKind, Dependency, DependencyKind,
    NormalizeState, Origin, Provenance, Readiness, RuntimeConsumer, UnsupportedReason,
};
use cs_types::evidence::{ClaimId, ClaimStatus};
use cs_types::install::ParseState;
use cs_types::profile::{ProfileId, ProfileKind};

/// A disposable directory tree, removed on drop.
struct TempBase(PathBuf);

impl TempBase {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f64-a-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the fixture base is created");
        Self(root)
    }
}

impl Drop for TempBase {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Every file below `root`, as `(relative spelling, bytes)` in sorted order.
///
/// The comparison is whole-tree, so a file created, removed or rewritten
/// anywhere under the destination shows up — not just a file whose name the
/// test happens to know.
fn tree(root: &Path) -> Vec<(String, Vec<u8>)> {
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
                stack.push(path);
            } else {
                let spelling = path
                    .strip_prefix(root)
                    .expect("every walked path is under the root")
                    .to_string_lossy()
                    .into_owned();
                rows.push((spelling, fs::read(&path).unwrap_or_default()));
            }
        }
    }
    rows.sort();
    rows
}

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("the fixture id is valid")
}

fn designed(id: &str) -> Provenance {
    Provenance::designed(ClaimId::new(id).expect("the fixture claim id is valid"))
}

fn ready_element(element_id: &ContentId) -> CatalogElement {
    CatalogElement {
        kind: element_id.kind(),
        id: element_id.clone(),
        display_name: Some(format!("Authored {element_id}")),
        origin: Origin::SyntheticFixture,
        dependencies: vec![Dependency {
            target: element_id.clone(),
            kind: DependencyKind::Static,
            provenance: designed("f64a.test.dependency"),
        }],
        parse_state: ParseState::Parsed,
        normalize_state: NormalizeState::Normalized,
        runtime_consumers: vec![RuntimeConsumer {
            kind: ConsumerKind::Gameplay,
            provenance: designed("f64a.test.consumer"),
        }],
        readiness: Readiness::Ready,
        unsupported_reasons: Vec::new(),
        fingerprint: None,
    }
}

/// A catalog holding two weapons and one airframe, all ready.
fn catalog() -> Catalog {
    let mut catalog = Catalog::new();
    for element_id in [
        cid(ContentKind::Weapon, "needle-gun"),
        cid(ContentKind::Weapon, "shrike-rocket"),
        cid(ContentKind::Airframe, "phoenix"),
    ] {
        catalog
            .insert(ready_element(&element_id))
            .expect("the fixture element inserts");
    }
    catalog
}

/// Legacy id 1 is the gun, legacy id 2 the rocket, legacy id 7 the airframe.
fn id_map() -> LegacyIdMap {
    let mut ids = LegacyIdMap::new();
    ids.insert(
        LegacyIdBinding::new(
            LegacyIdClass::Weapon,
            1,
            cid(ContentKind::Weapon, "needle-gun"),
        )
        .expect("the gun binding is in the weapon namespace"),
    )
    .expect("the gun binding inserts");
    ids.insert(
        LegacyIdBinding::new(
            LegacyIdClass::Weapon,
            2,
            cid(ContentKind::Weapon, "shrike-rocket"),
        )
        .expect("the rocket binding is in the weapon namespace"),
    )
    .expect("the rocket binding inserts");
    ids.insert(
        LegacyIdBinding::new(
            LegacyIdClass::Airframe,
            7,
            cid(ContentKind::Airframe, "phoenix"),
        )
        .expect("the airframe binding is in the airframe namespace"),
    )
    .expect("the airframe binding inserts");
    ids
}

fn target() -> TargetProfile {
    TargetProfile::new(
        ProfileId::new(9).expect("the fixture profile id is not zero"),
        ProfileKind::Production,
        "Imported legacy profile",
    )
    .expect("the target profile names itself")
}

/// A synthetic document for the designed fixture layout, with `tail` appended
/// to every record and `trailing` appended after the table.
fn document_with(records: &[(u32, u32, &str)], tail: &[u8], trailing: &[u8]) -> Vec<u8> {
    let layout = synthetic_layout();
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
        bytes.extend_from_slice(tail);
    }
    bytes.extend_from_slice(trailing);
    bytes
}

/// A document with the designed fixture layout's records and no per-record
/// tail.
fn document(records: &[(u32, u32, &str)], trailing: &[u8]) -> Vec<u8> {
    document_with(records, &[], trailing)
}

/// A proposal for `bytes` that declares the digest those bytes actually have.
///
/// The digest is the production `cs_assets::install::sha256`, not a fixture
/// constant: the planner verifies the declared fingerprint against the supplied
/// bytes, so a proposal that invented a digest would be refused before the
/// document was ever read.
fn proposal(bytes: &[u8]) -> Result<ArtifactProposal, ArtifactProposalError> {
    ArtifactProposal::new(
        "profiles/legacy/player.sav",
        bytes.len() as u64,
        cs_assets::install::sha256(bytes),
        Some(LegacyArtifactClass::CampaignSave),
    )
}

/// The fixture admission: the designed layout is read only because the test
/// says so.
fn request<'a>(
    bytes: &'a [u8],
    source: &'a ArtifactProposal,
    layout: &'a cs_formats::legacy_profile::LegacyLayout,
    ids: &'a LegacyIdMap,
    catalog: &'a Catalog,
    target: &'a TargetProfile,
) -> ImportRequest<'a> {
    ImportRequest {
        source,
        bytes,
        layout,
        limits: LegacyLimits::designed(),
        ids,
        catalog,
        target,
        admission: LayoutAdmission::AllowDesignedFixtures,
        legacy_save_import_enabled: true,
        install_identity: None,
    }
}

/// A fully resolvable document plans as `Full` and retains the source
/// fingerprint of exactly the bytes it was made from.
#[test]
fn accept_f64_a_resolvable_document_plans_as_full_with_a_retained_fingerprint() {
    let bytes = document(&[(7, 1, "phoenix")], &[]);
    let source = proposal(&bytes).expect("the fixture source is within the cap");
    let layout = synthetic_layout();
    let ids = id_map();
    let catalog = catalog();
    let target = target();

    let plan: ImportPlan = plan_import(&request(&bytes, &source, &layout, &ids, &catalog, &target))
        .expect("a resolvable document plans");

    assert_eq!(plan.class(), LegacyArtifactClass::CampaignSave);
    assert_eq!(*plan.report().class(), ImportClass::Full);
    assert!(plan.report().class().is_complete());
    assert_eq!(plan.report().records().len(), 1);
    assert_eq!(
        plan.report().records()[0].resolved_ids,
        vec![
            (
                LegacyIdClass::Airframe,
                cid(ContentKind::Airframe, "phoenix")
            ),
            (
                LegacyIdClass::Weapon,
                cid(ContentKind::Weapon, "needle-gun")
            ),
        ],
        "each declared id slot resolves to its declared identity, in layout order"
    );
    assert_eq!(
        plan.report().source().spelling(),
        "profiles/legacy/player.sav"
    );
    assert_eq!(plan.report().source().size_bytes(), bytes.len() as u64);
    assert_eq!(
        plan.report().source().sha256(),
        &cs_assets::install::sha256(&bytes),
        "the retained fingerprint must describe the bytes that were read"
    );
    assert_eq!(plan.target().id().get(), 9);
    assert!(
        plan.admitted_designed_layout(),
        "a plan made through the fixture admission says so, so fixture data can \
         never be reported as a measured import"
    );
    assert_ne!(
        plan.report().layout_evidence(),
        cs_types::evidence::ClaimStatus::VerifiedOriginal
    );
}

/// AC01 — the minimum scenario. A malicious profile (a record count of four
/// billion) and an oversized one are both refused, and neither touches the
/// source file or the destination profile directory: the source's bytes, size
/// and modification time and the whole destination tree are identical before
/// and after each attempt.
#[test]
fn accept_f64_a_malicious_or_oversized_old_profile_fails_without_touching_source_or_new_saves() {
    let base = TempBase::new("ac01");
    let source_path = base.0.join("profiles/legacy/player.sav");
    let profiles_root = base.0.join("userdata/production");
    fs::create_dir_all(source_path.parent().expect("the source has a parent"))
        .expect("the source directory is created");
    fs::create_dir_all(&profiles_root).expect("the destination root is created");
    // A pre-existing new-engine save the import must not disturb.
    let existing_save = profiles_root.join("existing.save");
    fs::write(&existing_save, b"fresh-engine-save").expect("the existing save is written");

    let layout = synthetic_layout();
    let ids = id_map();
    let catalog = catalog();
    let target = target();

    let mut bomb = document(&[(7, 1, "phoenix")], &[]);
    bomb[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
    let oversized = vec![0u8; usize::try_from(MAX_LEGACY_SOURCE_BYTES).expect("the cap fits") + 1];

    for (label, bytes) in [("malicious", &bomb), ("oversized", &oversized)] {
        fs::write(&source_path, bytes).expect("the payload is written");
        let source_metadata = fs::metadata(&source_path).expect("the source exists");
        let modified = source_metadata
            .modified()
            .expect("the platform reports a modification time");
        let source_before = fs::read(&source_path).expect("the source reads");
        let destination_before = tree(&profiles_root);
        let destination_files_before = destination_before.len();

        // The declared size is capped rather than honest, so the planner's own
        // size check — not the proposal's constructor — is what has to refuse
        // the oversized payload. The digest is the real one for both payloads:
        // the refusal under test is the size, not a stale fingerprint.
        let source = ArtifactProposal::new(
            "profiles/legacy/player.sav",
            bytes.len().min(MAX_LEGACY_SOURCE_BYTES as usize) as u64,
            cs_assets::install::sha256(bytes),
            Some(LegacyArtifactClass::CampaignSave),
        )
        .expect("the declared size is within the cap");

        let refused = plan_import(&request(bytes, &source, &layout, &ids, &catalog, &target))
            .expect_err("a hostile or oversized profile must be refused");

        match &refused {
            ImportRefusal::Unreadable(error) => {
                assert_eq!(label, "malicious", "only the bomb is read at all");
                assert_eq!(
                    error.kind,
                    LegacyProfileErrorKind::TooManyRecords,
                    "the refusal must name the hostile record count"
                );
            }
            ImportRefusal::SourceTooLarge { size, max } => {
                assert_eq!(label, "oversized");
                assert_eq!(*size, bytes.len() as u64);
                assert_eq!(*max, MAX_LEGACY_SOURCE_BYTES);
            }
            other => panic!("{label}: unexpected refusal {other}"),
        }

        // The source is byte-identical, the same size and not re-stamped.
        let after_metadata = fs::metadata(&source_path).expect("the source still exists");
        assert_eq!(
            fs::read(&source_path).expect("the source reads"),
            source_before,
            "{label}: the refused import must not change the source bytes"
        );
        assert_eq!(
            after_metadata.len(),
            source_metadata.len(),
            "{label}: the refused import must not change the source size"
        );
        assert_eq!(
            after_metadata.modified().ok(),
            Some(modified),
            "{label}: the refused import must not rewrite the source file at all"
        );

        // The new saves are untouched: no file added, removed or rewritten.
        assert_eq!(
            tree(&profiles_root),
            destination_before,
            "{label}: the refused import must not touch any new save"
        );
        assert_eq!(
            destination_before.len(),
            destination_files_before,
            "{label}: the destination must hold the same files it held"
        );
        assert_eq!(
            fs::read(&existing_save).expect("the existing save reads"),
            b"fresh-engine-save".to_vec()
        );
    }
}

/// A fingerprint that does not describe the offered bytes is refused, so a plan
/// can never describe a file that was not the one read.
#[test]
fn accept_f64_a_fingerprint_mismatch_is_refused_before_the_document_is_read() {
    let bytes = document(&[(7, 1, "phoenix")], &[]);
    let lying = ArtifactProposal::new(
        "profiles/legacy/player.sav",
        bytes.len() as u64 + 10,
        cs_assets::install::sha256(&bytes),
        Some(LegacyArtifactClass::CampaignSave),
    )
    .expect("the spelling and size are within the cap");
    let layout = synthetic_layout();
    let ids = id_map();
    let catalog = catalog();
    let target = target();

    let refused = plan_import(&request(&bytes, &lying, &layout, &ids, &catalog, &target))
        .expect_err("a fingerprint that does not describe the bytes must be refused");
    assert_eq!(
        refused,
        ImportRefusal::Source(ArtifactProposalError::FingerprintMismatch {
            declared: bytes.len() as u64 + 10,
            actual: bytes.len() as u64,
        })
    );
}

/// The digest half of the fingerprint: a source that kept its length but whose
/// bytes changed between being inventoried and being read is refused too. A
/// same-length substitution is exactly the case a length check cannot see, and
/// importing it under the inventoried identity would be the corruption
/// non-negotiable 1 forbids.
#[test]
fn accept_f64_a_same_length_source_with_a_changed_digest_is_refused() {
    let bytes = document(&[(7, 1, "phoenix")], &[]);
    let other = document(&[(7, 2, "phoenix")], &[]);
    assert_eq!(
        bytes.len(),
        other.len(),
        "the substituted source must be indistinguishable by length alone"
    );

    // The proposal describes `other`'s bytes; `bytes` is what is offered.
    let stale = ArtifactProposal::new(
        "profiles/legacy/player.sav",
        bytes.len() as u64,
        cs_assets::install::sha256(&other),
        Some(LegacyArtifactClass::CampaignSave),
    )
    .expect("the spelling and size are within the cap");
    let layout = synthetic_layout();
    let ids = id_map();
    let catalog = catalog();
    let target = target();

    let refused = plan_import(&request(&bytes, &stale, &layout, &ids, &catalog, &target))
        .expect_err("a changed source must be refused, not imported as the inventoried one");
    assert_eq!(
        refused,
        ImportRefusal::Source(ArtifactProposalError::HashMismatch {
            declared: cs_assets::install::sha256(&other),
            actual: cs_assets::install::sha256(&bytes),
        }),
        "the refusal must name the digest it saw, and no document may be read"
    );
    assert_eq!(refused.code(), "source_refused");
}

/// The default admission refuses the designed fixture layout, so no production
/// caller can import through a developer placeholder; naming the fixture
/// admission is the only way in.
#[test]
fn accept_f64_a_designed_layout_is_refused_unless_the_caller_names_the_fixture_admission() {
    let bytes = document(&[(7, 1, "phoenix")], &[]);
    let source = proposal(&bytes).expect("the fixture source is within the cap");
    let layout = synthetic_layout();
    let ids = id_map();
    let catalog = catalog();
    let target = target();

    let mut strict = request(&bytes, &source, &layout, &ids, &catalog, &target);
    strict.admission = LayoutAdmission::MeasuredOnly;
    let refused = plan_import(&strict).expect_err("a designed layout must be refused by default");
    assert_eq!(
        refused,
        ImportRefusal::LayoutEvidence {
            layout: layout.id().to_owned(),
            evidence: cs_types::evidence::ClaimStatus::Designed,
        }
    );
    assert_eq!(refused.code(), "layout_evidence");
    assert_eq!(LayoutAdmission::default(), LayoutAdmission::MeasuredOnly);
}

/// The optional save-import switch: a save is refused while the enhancement is
/// off and a custom aircraft — a class the inventory does not gate — is still
/// planned, so switching the enhancement off cannot block the rest of the
/// import surface.
#[test]
fn accept_f64_a_optional_save_import_can_be_switched_off_without_touching_other_classes() {
    let bytes = document(&[(7, 1, "phoenix")], &[]);
    let layout = synthetic_layout();
    let ids = id_map();
    let catalog = catalog();
    let target = target();

    let save = proposal(&bytes).expect("the fixture source is within the cap");
    let mut disabled = request(&bytes, &save, &layout, &ids, &catalog, &target);
    disabled.legacy_save_import_enabled = false;
    let refused = plan_import(&disabled).expect_err("the disabled enhancement must be refused");
    assert_eq!(
        refused,
        ImportRefusal::EnhancementDisabled {
            class: LegacyArtifactClass::CampaignSave,
            switch: "cs.profile.legacy_save_import",
        }
    );
    assert_eq!(refused.code(), "enhancement_disabled");

    let aircraft = ArtifactProposal::new(
        "profiles/legacy/custom.pln",
        bytes.len() as u64,
        cs_assets::install::sha256(&bytes),
        Some(LegacyArtifactClass::CustomAircraft),
    )
    .expect("the aircraft proposal is valid");
    let mut still_plannable = request(&bytes, &aircraft, &layout, &ids, &catalog, &target);
    still_plannable.legacy_save_import_enabled = false;
    let plan = plan_import(&still_plannable).expect("a custom aircraft is not behind the switch");
    assert_eq!(plan.class(), LegacyArtifactClass::CustomAircraft);
    assert!(
        !plan.requirement().is_required(),
        "no measured original path references a custom aircraft yet, so the \
         plan must not claim a triggered requirement"
    );
}

/// A record whose id is not mapped, and a record with bytes no declared slot
/// covers, both become named unresolved rows: the plan is `Partial`, never
/// `Full`, and nothing is defaulted.
#[test]
fn accept_f64_a_unmapped_ids_and_undeclared_bytes_stay_unresolved() {
    let bytes = document(&[(7, 1, "phoenix"), (7, 99, "ghost")], &[]);
    let source = proposal(&bytes).expect("the fixture source is within the cap");
    let layout = synthetic_layout();
    let ids = id_map();
    let catalog = catalog();
    let target = target();

    let plan = plan_import(&request(&bytes, &source, &layout, &ids, &catalog, &target))
        .expect("a partly resolvable document still plans");
    let ImportClass::Partial {
        resolved,
        unresolved,
    } = plan.report().class()
    else {
        panic!(
            "an unmapped id must make the plan partial, got {}",
            plan.report().class()
        );
    };
    assert_eq!(resolved, &[0u32][..], "the resolvable record is imported");
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].record_index, Some(1));
    assert_eq!(unresolved[0].field.as_deref(), Some("weapon_id"));
    assert_eq!(
        unresolved[0].reason,
        UnresolvedReason::IdNotMapped {
            class: LegacyIdClass::Weapon,
            raw: 99,
        }
    );
    assert_eq!(unresolved[0].reason.code(), "id_not_mapped");
    assert_eq!(
        plan.report().records().len(),
        1,
        "a record with an unresolved slot is never half-imported"
    );

    // A record whose stride is wider than the declared fields is unresolved
    // too: the retained bytes are reported, never interpreted.
    let wider = synthetic_layout()
        .with_record_size(20)
        .expect("the declared record fields fit inside a 20 byte stride");
    let wider_bytes = document_with(&[(7, 1, "phoenix")], &[0xaa, 0xbb, 0xcc, 0xdd], &[]);
    let wider_source = proposal(&wider_bytes).expect("the fixture source is within the cap");
    let plan = plan_import(&request(
        &wider_bytes,
        &wider_source,
        &wider,
        &ids,
        &catalog,
        &target,
    ))
    .expect("a document with undeclared record bytes still plans");
    let ImportClass::Unsupported { reason } = plan.report().class() else {
        panic!(
            "a record with undeclared bytes is never half-imported, got {}",
            plan.report().class()
        );
    };
    assert_eq!(
        *reason,
        UnresolvedReason::UndeclaredRecordBytes { bytes: 4 },
        "the retained bytes are reported, not interpreted"
    );
    assert_eq!(reason.code(), "undeclared_record_bytes");
    assert_eq!(
        plan.report().records().len(),
        0,
        "a record with undeclared bytes is not half-imported"
    );
    assert_eq!(
        wider.record_size(),
        20,
        "the declared stride is the record size"
    );
    assert!(
        synthetic_layout().with_record_size(4).is_err(),
        "a stride below the declared fields must be refused"
    );
}

/// Bytes past the record table are as unaccounted-for as bytes past a record's
/// last declared slot, so they make the plan partial rather than letting it be
/// reported `Full`. A document whose tail the layout cannot explain has not
/// imported completely, whatever its records resolved to.
#[test]
fn accept_f64_a_undeclared_trailing_bytes_stay_unresolved_instead_of_a_full_import() {
    let clean = document(&[(7, 1, "phoenix")], &[]);
    let with_tail = document(&[(7, 1, "phoenix")], &[0xde, 0xad, 0xbe, 0xef]);
    let layout = synthetic_layout();
    let ids = id_map();
    let catalog = catalog();
    let target = target();

    // The same document with no tail really is a full import, so the assertion
    // below is about the tail and not about the records.
    let clean_source = proposal(&clean).expect("the fixture source is within the cap");
    let clean_plan = plan_import(&request(
        &clean,
        &clean_source,
        &layout,
        &ids,
        &catalog,
        &target,
    ))
    .expect("the untailed document plans");
    assert_eq!(*clean_plan.report().class(), ImportClass::Full);

    let tail_source = proposal(&with_tail).expect("the fixture source is within the cap");
    let plan = plan_import(&request(
        &with_tail,
        &tail_source,
        &layout,
        &ids,
        &catalog,
        &target,
    ))
    .expect("a document with a tail still plans, as partial");
    let ImportClass::Partial {
        resolved,
        unresolved,
    } = plan.report().class()
    else {
        panic!(
            "an unexplained tail must not be a full import, got {}",
            plan.report().class()
        );
    };
    assert!(
        !plan.report().class().is_complete(),
        "a document with undeclared trailing bytes is not a complete import"
    );
    assert_eq!(resolved, &[0u32][..], "the record itself still resolves");
    assert_eq!(unresolved.len(), 1);
    assert_eq!(
        unresolved[0].record_index, None,
        "the tail is a document-level row, not a record's"
    );
    assert_eq!(
        unresolved[0].reason,
        UnresolvedReason::UndeclaredTrailingBytes { bytes: 4 },
        "the retained tail is reported, not interpreted"
    );
    assert_eq!(unresolved[0].reason.code(), "undeclared_trailing_bytes");
}

/// A legacy id slot declared wider than a `u32` is refused as out of range,
/// never clamped: a clamped value resolves against whatever is bound at the
/// clamp boundary, which would import one weapon as another.
#[test]
fn accept_f64_a_legacy_id_wider_than_the_id_space_is_unresolved_not_clamped() {
    // The same declared document, with the weapon id slot widened to 64 bits
    // and the record carrying a value that does not fit a legacy id.
    let layout = cs_formats::legacy_profile::LegacyLayout::new(
        "synthetic.wide_id/v1",
        ClaimStatus::Designed,
        *b"CSPROF01",
        vec![
            LegacySlot::new("version_major", LegacySlotType::U32, LEGACY_MAGIC_BYTES),
            LegacySlot::new("version_minor", LegacySlotType::U32, 12),
            LegacySlot::new("record_count", LegacySlotType::U32, 16),
        ],
        "version_major",
        "version_minor",
        1,
        "record_count",
        vec![
            LegacySlot::new("airframe_id", LegacySlotType::U32, 0),
            LegacySlot::new("weapon_id", LegacySlotType::U64, 4),
        ],
        vec![LegacyIdSlot::new("weapon_id", LegacyIdClass::Weapon)],
        TrailingPolicy::Retain,
    );
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&layout.magic());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&0x1_0000_0001u64.to_le_bytes());
    assert_eq!(
        bytes.len(),
        32,
        "an 8 byte magic, three u32s and a 12 byte record"
    );

    // A binding exists at the value a clamp would produce, so a clamping
    // implementation would resolve this record and report a full import.
    let mut ids = id_map();
    ids.insert(
        LegacyIdBinding::new(
            LegacyIdClass::Weapon,
            u32::MAX,
            cid(ContentKind::Weapon, "needle-gun"),
        )
        .expect("the gun binding is in the weapon namespace"),
    )
    .expect("the binding inserts");
    let catalog = catalog();
    let target = target();
    let source = proposal(&bytes).expect("the fixture source is within the cap");

    let plan = plan_import(&request(&bytes, &source, &layout, &ids, &catalog, &target))
        .expect("a document with an out-of-range id still plans");
    let ImportClass::Unsupported { reason } = plan.report().class() else {
        panic!(
            "an out-of-range id must not resolve, got {}",
            plan.report().class()
        );
    };
    assert_eq!(
        *reason,
        UnresolvedReason::IdOutOfRange {
            field: "weapon_id".to_owned(),
            class: LegacyIdClass::Weapon,
            value: 0x1_0000_0001,
        },
        "the value is reported as it was read, never clamped into range"
    );
    assert_eq!(reason.code(), "id_out_of_range");
    assert_eq!(
        plan.report().records().len(),
        0,
        "an out-of-range id is never resolved to the binding at the clamp boundary"
    );
}

/// An identity that is in the catalog but not ready never imports: it is an
/// unresolved row naming the element's own reason codes.
#[test]
fn accept_f64_a_not_ready_target_is_unresolved_with_the_element_reason() {
    let mut catalog = catalog();
    let blocked = cid(ContentKind::Weapon, "blocked-gun");
    catalog
        .insert(CatalogElement {
            readiness: Readiness::Unavailable,
            unsupported_reasons: vec![UnsupportedReason::MissingParser],
            ..ready_element(&blocked)
        })
        .expect("the unavailable element inserts");

    let mut ids = id_map();
    ids.insert(
        LegacyIdBinding::new(LegacyIdClass::Weapon, 5, blocked.clone())
            .expect("the binding is in the weapon namespace"),
    )
    .expect("the binding inserts");

    let bytes = document(&[(7, 5, "blocked")], &[]);
    let source = proposal(&bytes).expect("the fixture source is within the cap");
    let layout = synthetic_layout();
    let target = target();

    let plan = plan_import(&request(&bytes, &source, &layout, &ids, &catalog, &target))
        .expect("a document with a blocked target still plans");
    assert_eq!(
        *plan.report().class(),
        ImportClass::Unsupported {
            reason: UnresolvedReason::TargetNotReady {
                id: blocked,
                reasons: "missing_parser".to_owned(),
            }
        },
        "nothing resolved, so the plan is unsupported with the element's reason"
    );
    assert!(!plan.report().class().is_complete());
}

/// A document whose records resolve **no** identity is not a full import: the
/// plan carries nothing, so calling it `Full` would present a blank profile as a
/// successfully imported one (spec F64 non-negotiable 5). A layout that declares
/// no id slot makes every record resolve "successfully" with nothing to carry,
/// which is exactly the case this names.
#[test]
fn accept_f64_a_document_resolving_no_identity_is_never_reported_as_a_full_import() {
    // The same document, but read through a layout that declares its record's
    // fields and no id slot at all, so no record can resolve an identity.
    let layout = LegacyLayout::new(
        "synthetic.no_id_slots/v1",
        ClaimStatus::Designed,
        *b"CSPROF01",
        vec![
            LegacySlot::new("version_major", LegacySlotType::U32, LEGACY_MAGIC_BYTES),
            LegacySlot::new("version_minor", LegacySlotType::U32, 12),
            LegacySlot::new("record_count", LegacySlotType::U32, 16),
            // The same header the fixture builder writes, so the record table
            // starts where the builder put it.
            LegacySlot::new("label", LegacySlotType::Text { len: 8 }, 20),
        ],
        "version_major",
        "version_minor",
        1,
        "record_count",
        vec![
            LegacySlot::new("airframe_id", LegacySlotType::U32, 0),
            LegacySlot::new("weapon_id", LegacySlotType::U32, 4),
            LegacySlot::new("name", LegacySlotType::Text { len: 8 }, 8),
        ],
        vec![],
        TrailingPolicy::Retain,
    );
    let bytes = document(&[(7, 1, "phoenix"), (7, 2, "phoenix")], &[]);
    let source = proposal(&bytes).expect("the fixture source is within the cap");
    let ids = id_map();
    let catalog = catalog();
    let target = target();

    let plan = plan_import(&request(&bytes, &source, &layout, &ids, &catalog, &target))
        .expect("a readable document still plans");
    assert_eq!(
        *plan.report().class(),
        ImportClass::Unsupported {
            reason: UnresolvedReason::NoResolvableIdentity { records: 2 }
        },
        "a document that carries no identity must not be reported as a full import"
    );
    assert!(!plan.report().class().is_complete());
    assert_eq!(
        plan.report().records().len(),
        2,
        "the records are still listed, each carrying no identity at all"
    );
    assert!(
        plan.report()
            .records()
            .iter()
            .all(|record| record.resolved_ids.is_empty()),
        "which is why the class must not be a full import"
    );

    // The same document through the id-declaring layout still imports, so the
    // refusal above is about the missing identity and not about the document.
    let declared = synthetic_layout();
    let full = plan_import(&request(
        &bytes, &source, &declared, &ids, &catalog, &target,
    ))
    .expect("the id-declaring layout resolves the same document");
    assert_eq!(*full.report().class(), ImportClass::Full);
}

/// A document with no records is refused outright: an empty legacy profile is
/// never reported as a successful import.
#[test]
fn accept_f64_a_empty_document_is_refused_never_reported_as_a_full_import() {
    let bytes = document(&[], &[]);
    let source = proposal(&bytes).expect("the fixture source is within the cap");
    let layout = synthetic_layout();
    let ids = id_map();
    let catalog = catalog();
    let target = target();

    let refused = plan_import(&request(&bytes, &source, &layout, &ids, &catalog, &target))
        .expect_err("an empty document must be refused");
    assert_eq!(
        refused,
        ImportRefusal::NotImportable {
            class: LegacyArtifactClass::CampaignSave,
            reason: UnresolvedReason::NoRecords,
        }
    );
    assert_eq!(refused.code(), "not_importable");
}

/// Ids resolve through declared content identities, so neither the order the
/// catalog was filled in nor a different enumeration order can change what a
/// legacy id means, and a binding into the wrong namespace is refused rather
/// than coerced.
#[test]
fn accept_f64_a_ids_resolve_by_identity_so_catalog_order_changes_nothing() {
    let mut forward = Catalog::new();
    for element_id in [
        cid(ContentKind::Weapon, "needle-gun"),
        cid(ContentKind::Weapon, "shrike-rocket"),
        cid(ContentKind::Airframe, "phoenix"),
    ] {
        forward
            .insert(ready_element(&element_id))
            .expect("the fixture element inserts");
    }
    let mut reversed = Catalog::new();
    for element_id in [
        cid(ContentKind::Airframe, "phoenix"),
        cid(ContentKind::Weapon, "shrike-rocket"),
        cid(ContentKind::Weapon, "needle-gun"),
    ] {
        reversed
            .insert(ready_element(&element_id))
            .expect("the fixture element inserts");
    }
    assert_eq!(
        forward.sorted_ids(),
        reversed.sorted_ids(),
        "insertion order does not change which identities the catalog holds"
    );

    let ids = id_map();
    for row in ids.bindings() {
        let first = ids
            .resolve(row.class(), row.raw(), &forward)
            .expect("the identity resolves in the first catalog");
        let second = ids
            .resolve(row.class(), row.raw(), &reversed)
            .expect("the identity resolves in the reordered catalog");
        assert_eq!(
            first, second,
            "a legacy id must resolve to the same identity whatever order the \
             catalog was filled in"
        );
    }

    // The plan itself is identical from both catalogs, record for record.
    let bytes = document(&[(7, 2, "phoenix")], &[]);
    let source = proposal(&bytes).expect("the fixture source is within the cap");
    let layout = synthetic_layout();
    let target = target();
    let from_forward = plan_import(&request(&bytes, &source, &layout, &ids, &forward, &target))
        .expect("the document plans against the first catalog");
    let from_reversed = plan_import(&request(&bytes, &source, &layout, &ids, &reversed, &target))
        .expect("the document plans against the reordered catalog");
    assert_eq!(from_forward.report(), from_reversed.report());
    assert_eq!(
        from_forward.report().records()[0].resolved_ids[1],
        (
            LegacyIdClass::Weapon,
            cid(ContentKind::Weapon, "shrike-rocket")
        ),
        "legacy weapon id 2 is the rocket in every catalog order, never the gun"
    );

    assert_eq!(
        LegacyIdBinding::new(
            LegacyIdClass::Weapon,
            3,
            cid(ContentKind::Airframe, "phoenix")
        ),
        Err(LegacyIdMapError::KindMismatch {
            class: LegacyIdClass::Weapon,
            expected: ContentKind::Weapon,
            found: ContentKind::Airframe,
        }),
        "a binding into the wrong namespace is refused, never coerced"
    );
    assert_eq!(
        ids.resolve(LegacyIdClass::Weapon, 404, &forward),
        Err(UnresolvedReason::IdNotMapped {
            class: LegacyIdClass::Weapon,
            raw: 404,
        }),
        "an unmapped id has no fallback"
    );
}
