//! F64-A acceptance tests: the legacy-import surface inventory and the
//! declared-layout reader (stage A, `specs/F64-legacy-custom-aircraft-and-
//! optional-save-import.md`).
//!
//! Every test calls the production `cs_formats::legacy_profile` API — the same
//! `plan_import` path and the same `read_legacy_profile` entry point F64-B
//! will use with a measured layout — so deleting the inventory, the refusal
//! taxonomy or the reader's bounds makes these tests fail rather than pass
//! beside them.
//!
//! All data is newly authored synthetic fixture content. No original file was
//! opened, and nothing here is a claim about an original legacy profile, save
//! or custom-aircraft format: every inventory row's evidence is `Unknown` and
//! the one shipped layout is `Designed`.

use cs_formats::legacy_profile::{
    ArtifactProposal, ArtifactProposalError, ImportRequirement, LEGACY_LAYOUT_INVENTORY,
    LEGACY_MAGIC_BYTES, LegacyArtifactClass, LegacyIdClass, LegacyIdSlot, LegacyLayout,
    LegacyLayoutError, LegacyLimits, LegacyProfileErrorKind, LegacySlot, LegacySlotType,
    LegacyValue, MAX_LEGACY_SOURCE_BYTES, TrailingPolicy, layout_record, read_legacy_profile,
    synthetic_layout,
};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};

fn hash(byte: u8) -> ContentHash {
    let mut bytes = [0u8; 32];
    bytes[0] = byte;
    ContentHash::from_bytes(bytes)
}

fn proposal(class: Option<LegacyArtifactClass>, size: u64) -> ArtifactProposal {
    ArtifactProposal::new("profiles/legacy/player.sav", size, hash(7), class)
        .expect("the fixture proposal is valid")
}

/// A synthetic document for the designed fixture layout: header, then
/// `records` fixed-size records of `(airframe_id, weapon_id, name)`.
fn document(records: &[(u32, u32, &str)], trailing: &[u8]) -> Vec<u8> {
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
    }
    bytes.extend_from_slice(trailing);
    bytes
}

/// A layout with a different magic, so a document read through one layout is
/// refused by the other.
fn foreign_layout() -> LegacyLayout {
    LegacyLayout::new(
        "synthetic.foreign/v1",
        ClaimStatus::Designed,
        *b"OTHERPRF",
        vec![
            LegacySlot::new("version_major", LegacySlotType::U32, LEGACY_MAGIC_BYTES),
            LegacySlot::new("version_minor", LegacySlotType::U32, 12),
            LegacySlot::new("record_count", LegacySlotType::U32, 16),
        ],
        "version_major",
        "version_minor",
        1,
        "record_count",
        vec![LegacySlot::new("name", LegacySlotType::Text { len: 8 }, 0)],
        vec![LegacyIdSlot::new("name", LegacyIdClass::Mission)],
        TrailingPolicy::Retain,
    )
}

/// The inventory enumerates every class exactly once, and no row claims
/// measured evidence.
#[test]
fn accept_f64_a_inventory_covers_every_class_with_unknown_evidence() {
    let mut seen: Vec<LegacyArtifactClass> = LEGACY_LAYOUT_INVENTORY
        .iter()
        .map(|record| record.class)
        .collect();
    seen.sort();
    seen.dedup();
    assert_eq!(
        seen,
        LegacyArtifactClass::ALL.to_vec(),
        "every artifact class needs exactly one inventory row"
    );
    for record in &LEGACY_LAYOUT_INVENTORY {
        assert_eq!(
            record.evidence,
            ClaimStatus::Unknown,
            "{} has no measured layout at this stage, so its evidence must stay \
             unknown rather than being asserted",
            record.class.label()
        );
        assert!(
            !record.unknowns.is_empty(),
            "{} must record what has to be measured before it leaves unknown",
            record.class.label()
        );
        assert_eq!(
            layout_record(record.class).requirement,
            record.requirement,
            "the lookup must return the same row the inventory holds"
        );
    }
}

/// Custom-aircraft import is required only when a measured original content
/// path references it, and no such path has been measured — so the requirement
/// is *not triggered* rather than assumed. The save rows are the separately
/// labeled optional enhancement with a named disable switch.
#[test]
fn accept_f64_a_custom_aircraft_import_is_not_required_until_a_path_references_it() {
    let aircraft = layout_record(LegacyArtifactClass::CustomAircraft).requirement;
    assert!(
        !aircraft.is_required(),
        "no original content path has been measured to reference a custom \
         aircraft, so import must not be treated as required yet"
    );
    assert!(
        aircraft.referenced_by().is_empty(),
        "an unreferenced row must carry no referencing path"
    );

    for class in [
        LegacyArtifactClass::CampaignSave,
        LegacyArtifactClass::SettingsBlob,
        LegacyArtifactClass::ProfilePointer,
    ] {
        let requirement = layout_record(class).requirement;
        assert!(
            requirement.is_optional_enhancement(),
            "{class} must be a separately labeled optional enhancement"
        );
        assert!(!requirement.is_required());
        let ImportRequirement::OptionalEnhancement {
            label,
            disable_switch,
        } = requirement
        else {
            panic!("{class} must carry the optional-enhancement form");
        };
        assert_eq!(
            label, "legacy-save-import",
            "the label is part of the contract"
        );
        assert!(
            !disable_switch.is_empty(),
            "an optional enhancement needs a named disable switch"
        );
    }
}

/// A candidate source is classified only by declared evidence: a name that
/// looks like a save never becomes one, and a source with no declared class is
/// unclassified rather than guessed from its extension.
#[test]
fn accept_f64_a_source_class_comes_from_declared_evidence_not_from_its_name() {
    let named_like_a_save = ArtifactProposal::new(
        "weapon.cfg",
        10,
        hash(1),
        Some(LegacyArtifactClass::CampaignSave),
    )
    .expect("the spelling is valid");
    assert_eq!(
        named_like_a_save.class(),
        Some(LegacyArtifactClass::CampaignSave),
        "the class is what the caller declared, whatever the name says"
    );

    let undeclared = proposal(None, 10);
    assert_eq!(
        undeclared.class(),
        None,
        "an undeclared source has no class; a name is never evidence"
    );
    assert_eq!(
        undeclared.require_class(),
        Err(ArtifactProposalError::Unclassified)
    );
}

/// A source over the cap, and a spelling that could escape the source root,
/// are refused before a byte is read.
#[test]
fn accept_f64_a_oversized_or_escaping_source_is_refused_at_the_door() {
    assert_eq!(
        ArtifactProposal::new(
            "profiles/legacy/big.sav",
            MAX_LEGACY_SOURCE_BYTES + 1,
            hash(2),
            Some(LegacyArtifactClass::CampaignSave)
        ),
        Err(ArtifactProposalError::SourceTooLarge {
            size: MAX_LEGACY_SOURCE_BYTES + 1,
            max: MAX_LEGACY_SOURCE_BYTES,
        })
    );
    assert_eq!(
        ArtifactProposal::new(
            "../outside.sav",
            10,
            hash(2),
            Some(LegacyArtifactClass::CampaignSave)
        )
        .err()
        .map(|error| matches!(error, ArtifactProposalError::Spelling(_))),
        Some(true),
        "a parent component must never be accepted as an in-installation path"
    );
    assert!(
        ArtifactProposal::new(
            "profiles/legacy/exact.sav",
            MAX_LEGACY_SOURCE_BYTES,
            hash(2),
            Some(LegacyArtifactClass::CampaignSave)
        )
        .is_ok()
    );
}

/// A fingerprint that does not describe the bytes supplied is refused, so a
/// plan can never be made for a file that was not the one read.
#[test]
fn accept_f64_a_fingerprint_must_describe_the_bytes_that_were_supplied() {
    let declared = proposal(Some(LegacyArtifactClass::CampaignSave), 40);
    assert_eq!(
        declared.validate_against(39),
        Err(ArtifactProposalError::FingerprintMismatch {
            declared: 40,
            actual: 39,
        })
    );
    assert!(declared.validate_against(40).is_ok());
}

/// The reader's record count is checked against the limits *and* against the
/// document length before any record is reserved: a bomb that claims four
/// billion records is refused as `too_many_records`, and the same claim inside
/// the limit but past the end of the file is refused as
/// `record_table_out_of_range`.
#[test]
fn accept_f64_a_hostile_record_count_is_refused_before_any_table_is_reserved() {
    let layout = synthetic_layout();
    let limits = LegacyLimits::designed();

    let mut bomb = document(&[], &[]);
    // The record-count field sits at offset 16 of the designed header.
    bomb[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
    let refused = read_legacy_profile(&bomb, &layout, &limits)
        .expect_err("a four-billion-record claim must be refused");
    assert_eq!(refused.kind, LegacyProfileErrorKind::TooManyRecords);
    assert_eq!(refused.field, "record_count");
    assert!(
        refused.observed.contains(&u32::MAX.to_string()),
        "the refusal must report the count it saw, got {}",
        refused.observed
    );

    // Inside the limit but past the end of the document: the table extent is
    // checked against the bytes, so no partial table is ever read.
    let mut truncated = document(&[(1, 2, "a")], &[]);
    truncated[16..20].copy_from_slice(&3u32.to_le_bytes());
    let refused = read_legacy_profile(&truncated, &layout, &limits)
        .expect_err("a record count past the end of the file must be refused");
    assert_eq!(refused.kind, LegacyProfileErrorKind::RecordTableOutOfRange);
}

/// A document larger than the limit is refused before its first byte is
/// interpreted, and a layout that declares a field wider than the limits is
/// refused before the document is looked at at all.
#[test]
fn accept_f64_a_oversized_document_and_overwide_field_are_refused_before_reading() {
    let layout = synthetic_layout();
    let limits = LegacyLimits {
        max_bytes: 32,
        ..LegacyLimits::designed()
    };
    let big = document(&[(1, 2, "a"), (3, 4, "b")], &[]);
    assert!(big.len() as u64 > limits.max_bytes);
    let refused = read_legacy_profile(&big, &layout, &limits)
        .expect_err("a document over the limit must be refused");
    assert_eq!(refused.kind, LegacyProfileErrorKind::DocumentTooLarge);
    assert_eq!(refused.offset, 0, "the refusal is about the whole document");

    let overwide = LegacyLimits {
        max_text_bytes: 4,
        ..LegacyLimits::designed()
    };
    let refused = read_legacy_profile(&document(&[(1, 2, "a")], &[]), &layout, &overwide)
        .expect_err("a layout declaring a field over the limit must be refused");
    assert_eq!(refused.kind, LegacyProfileErrorKind::FieldTooWide);
    assert_eq!(refused.field, "label", "the named field is reported");
}

/// Bytes past the last declared slot and past the record table are retained,
/// never dropped: they are what makes a record unresolved later instead of
/// silently short.
#[test]
fn accept_f64_a_undeclared_bytes_are_retained_and_visible() {
    let layout = synthetic_layout();
    let limits = LegacyLimits::designed();
    let bytes = document(&[(7, 9, "ship")], &[0xde, 0xad]);
    let read = read_legacy_profile(&bytes, &layout, &limits).expect("the fixture document reads");
    assert_eq!(read.records().len(), 1);
    assert!(
        read.records()[0].undeclared.is_empty(),
        "the designed record declares every byte it declares"
    );
    assert_eq!(
        read.trailing(),
        &[0xde, 0xad],
        "bytes past the record table are kept, not discarded"
    );
    assert_eq!(
        read.header_value("label"),
        Some(&LegacyValue::Text("label1\u{0}\u{0}".to_owned())),
        "the declared text field is read whole, padding included"
    );
    assert_eq!(read.version_major(), 1);
    assert_eq!(read.version_minor(), 0);
    assert_eq!(read.record(0).expect("record 0").index, 0);
    assert_eq!(read.layout_evidence(), ClaimStatus::Designed);
}

/// A document whose magic or version major is not the layout's is refused by
/// name, and the refusal carries no payload bytes.
#[test]
fn accept_f64_a_foreign_magic_and_unsupported_version_are_named_refusals() {
    let layout = synthetic_layout();
    let limits = LegacyLimits::designed();
    let bytes = document(&[(1, 2, "a")], &[]);

    let refused = read_legacy_profile(&bytes, &foreign_layout(), &limits)
        .expect_err("a foreign magic must be refused");
    assert_eq!(refused.kind, LegacyProfileErrorKind::MagicMismatch);
    assert_eq!(refused.offset, 0);
    assert!(
        !refused.observed.contains("CSPROF"),
        "the refusal must not copy source bytes into a diagnostic: {}",
        refused.observed
    );

    let mut future = bytes.clone();
    future[LEGACY_MAGIC_BYTES..LEGACY_MAGIC_BYTES + 4].copy_from_slice(&2u32.to_le_bytes());
    let refused = read_legacy_profile(&future, &layout, &limits)
        .expect_err("a different version major must be refused");
    assert_eq!(refused.kind, LegacyProfileErrorKind::UnsupportedVersion);
    assert_eq!(refused.observed, "version major 2");
    assert!(refused.expected.contains('1'));
}

/// A version field declared wider than a `u32` is narrowed with a checked
/// conversion, so a document carrying `0x1_0000_0001` is refused as a version
/// this build cannot read instead of truncating onto the supported major. A
/// truncating reader would import a document it cannot actually version-check.
#[test]
fn accept_f64_a_wide_version_field_is_refused_never_truncated_onto_a_supported_major() {
    let layout = LegacyLayout::new(
        "synthetic.wide_version/v1",
        ClaimStatus::Designed,
        *b"CSPROF01",
        vec![
            LegacySlot::new("version_major", LegacySlotType::U64, LEGACY_MAGIC_BYTES),
            LegacySlot::new("version_minor", LegacySlotType::U32, 16),
            LegacySlot::new("record_count", LegacySlotType::U32, 20),
        ],
        "version_major",
        "version_minor",
        1,
        "record_count",
        vec![LegacySlot::new("airframe_id", LegacySlotType::U32, 0)],
        vec![],
        TrailingPolicy::Retain,
    );
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&layout.magic());
    bytes.extend_from_slice(&0x1_0000_0001u64.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&7u32.to_le_bytes());

    let refused = read_legacy_profile(&bytes, &layout, &LegacyLimits::designed())
        .expect_err("a version that does not fit 32 bits must be refused");
    assert_eq!(refused.kind, LegacyProfileErrorKind::UnsupportedVersion);
    assert_eq!(refused.field, "version_major");
    assert!(
        refused.observed.contains("4294967297"),
        "the refusal reports the value it read, got {}",
        refused.observed
    );

    // The same layout reading a major that does fit is accepted, so the refusal
    // above is about the width and not about the layout being unusable.
    let mut fitting = bytes.clone();
    fitting[LEGACY_MAGIC_BYTES..LEGACY_MAGIC_BYTES + 8].copy_from_slice(&1u64.to_le_bytes());
    let read = read_legacy_profile(&fitting, &layout, &LegacyLimits::designed())
        .expect("a version that fits 32 bits reads normally");
    assert_eq!(read.version_major(), 1);
    assert_eq!(read.version_minor(), 0);
}

/// A layout declaration that could not describe a document is refused, so a
/// measured layout cannot be built with an id slot naming no field.
#[test]
fn accept_f64_a_invalid_layout_declarations_are_refused_by_name() {
    let layout = synthetic_layout();
    assert!(layout.validate().is_ok());

    let dangling = LegacyLayout::new(
        "synthetic.dangling/v1",
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
        vec![LegacySlot::new("name", LegacySlotType::Text { len: 8 }, 0)],
        vec![LegacyIdSlot::new("absent_id", LegacyIdClass::Weapon)],
        TrailingPolicy::Retain,
    );
    assert_eq!(
        dangling.validate(),
        Err(LegacyLayoutError::IdSlotNotDeclared {
            field: "absent_id".to_owned()
        })
    );

    let overlapping = LegacyLayout::new(
        "synthetic.overlap/v1",
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
            LegacySlot::new("first", LegacySlotType::U32, 0),
            LegacySlot::new("second", LegacySlotType::U32, 2),
        ],
        vec![],
        TrailingPolicy::Retain,
    );
    assert!(matches!(
        overlapping.validate(),
        Err(LegacyLayoutError::OverlappingSlots { .. })
    ));

    let no_count = LegacyLayout::new(
        "synthetic.nocount/v1",
        ClaimStatus::Designed,
        *b"CSPROF01",
        vec![LegacySlot::new(
            "version_major",
            LegacySlotType::U32,
            LEGACY_MAGIC_BYTES,
        )],
        "version_major",
        "version_major",
        1,
        "record_count",
        vec![LegacySlot::new("name", LegacySlotType::Text { len: 4 }, 0)],
        vec![],
        TrailingPolicy::Retain,
    );
    assert!(matches!(
        no_count.validate(),
        Err(LegacyLayoutError::RecordCountNotDeclared { .. })
    ));

    // One record field carries one legacy id of one class: declaring it as an id
    // slot twice would resolve one value twice, and into two namespaces when the
    // classes differ — a second reading of the same bytes.
    let twice = LegacyLayout::new(
        "synthetic.double_id_slot/v1",
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
        vec![LegacySlot::new("airframe_id", LegacySlotType::U32, 0)],
        vec![
            LegacyIdSlot::new("airframe_id", LegacyIdClass::Airframe),
            LegacyIdSlot::new("airframe_id", LegacyIdClass::Weapon),
        ],
        TrailingPolicy::Retain,
    );
    assert_eq!(
        twice.validate(),
        Err(LegacyLayoutError::DuplicateIdSlot {
            field: "airframe_id".to_owned()
        })
    );
}

/// A text slot that is not valid UTF-8 is refused rather than lossily
/// converted, and the refusal names the byte where it failed.
#[test]
fn accept_f64_a_text_slot_that_is_not_utf8_is_refused_not_converted() {
    let layout = synthetic_layout();
    let limits = LegacyLimits::designed();
    let mut bytes = document(&[(1, 2, "ok")], &[]);
    // The record's 8-byte name field starts at header end + 8.
    let name_at = layout.header_bytes() + 8;
    bytes[name_at] = 0xff;
    let refused =
        read_legacy_profile(&bytes, &layout, &limits).expect_err("invalid UTF-8 must be refused");
    assert_eq!(refused.kind, LegacyProfileErrorKind::TextNotUtf8);
    assert_eq!(refused.field, "name");
    assert_eq!(refused.offset as usize, name_at);
    assert!(!refused.observed.contains('\u{fffd}'));
}

/// The entry point the evidence ledger names exists and reads the designed
/// fixture layout, and the fixture layout is `Designed`, never
/// `verified_original`.
#[test]
fn accept_f64_a_named_entrypoint_reads_only_a_designed_fixture_layout() {
    assert_eq!(
        cs_formats::legacy_profile::LEGACY_PROFILE_ENTRYPOINT,
        "cs_formats::legacy_profile::read_legacy_profile"
    );
    let layout = synthetic_layout();
    assert_eq!(layout.evidence(), ClaimStatus::Designed);
    assert_ne!(layout.evidence(), ClaimStatus::VerifiedOriginal);
    let read = read_legacy_profile(
        &document(&[(1, 2, "a")], &[]),
        &layout,
        &LegacyLimits::designed(),
    )
    .expect("the fixture document reads through the named entry point");
    assert_eq!(read.layout_id(), layout.id());
    assert_eq!(
        read.record(0).expect("record 0").integer("airframe_id"),
        Some(1)
    );
    let _ = ClaimId::new("f64-a.test.claim").expect("the fixture claim id is valid");
}
