//! Acceptance scenarios for how large a F64-C [`ConfirmedImport`] a profile can
//! persist (task #1530, F64-C-FU1; spec
//! `specs/F64-legacy-custom-aircraft-and-optional-save-import.md` stage
//! `### F64-C`; contract `docs/contracts/STATE-TRANSACTIONS.md`).
//!
//! Task test prefix: `accept_f64_c_fu1_`.
//!
//! The bound is the save schema's: an imported record's resolved identities are
//! carried in [`ProfileDocument::import_identities`], bounded by
//! `MAX_IMPORTED_IDENTITIES` and by `MAX_IMPORTED_RECORDS` records — the same
//! per-list bound `blueprints` already has. These tests prove the bound is real,
//! that a run at it persists and round-trips every identity, and that one record
//! over it is refused by name instead of being truncated.
//!
//! The fixture is F64-C's designed content (`f64_c_support`): a synthetic
//! blueprint document read through the designed layout, admitted only under
//! `LayoutAdmission::AllowDesignedFixtures`. Nothing here reads `$CS_GAME_DIR`
//! or claims an original file format.

use cs_app::profile::{ProfileSession, SessionError};
use cs_app::ui::import::ImportFlow;
use cs_content::construction::SYNTHETIC_AIRFRAME_KEY;
use cs_content::legacy_import::{TargetProfile, legacy_save_import_rule};
use cs_content::save::library::slot_name;
use cs_content::save::settings::SettingCatalog;
use cs_formats::legacy_profile::LegacyArtifactClass;
use cs_types::content::{ContentId, ContentKind, MAX_CONTENT_KEY_LEN};
use cs_types::profile::{
    CampaignState, ExtraField, FingerprintEntry, ImportedIdentity, MAX_DISPLAY_NAME_BYTES,
    MAX_IMPORTED_IDENTITIES, MAX_IMPORTED_IDENTITIES_PER_RECORD, MAX_IMPORTED_RECORDS,
    MAX_KEY_BYTES, MAX_VALUE_BYTES, ProfileDocument, ProfileFieldError, ProfileId, ProfileKind,
    RecordEntry, Revision, SettingApply, SettingEntry,
};

use crate::f64_c_support as support;

use support::{
    BlueprintRecord, CatalogRows, TempBase, blueprint_document, blueprint_layout_and_map, catalog,
    cid, context, full_record, id_map, offer, proposal, stock,
};

/// The settings catalog: the real F64-D switch rule, so an imported profile's
/// first revision resolves a live settings state.
fn settings_catalog() -> SettingCatalog {
    SettingCatalog::new([legacy_save_import_rule()]).expect("the fixture catalog holds together")
}

fn sandbox(base: &std::path::Path) -> ProfileSession {
    ProfileSession::open_sandbox(base, &settings_catalog()).expect("the sandbox session opens")
}

/// A `TargetProfile` for the sandbox population naming the id the library is
/// about to allocate.
fn synthetic_target(id: u64, name: &str) -> TargetProfile {
    TargetProfile::new(
        ProfileId::new(id).expect("the fixture profile id is not zero"),
        ProfileKind::Synthetic,
        name,
    )
    .expect("the target profile names itself")
}

/// Runs one fixture document through the production offer → confirm chain and
/// returns the [`ConfirmedImport`] the screen would hand a persistence layer.
fn confirmed_fixture(
    records: &[BlueprintRecord],
    target: &TargetProfile,
) -> cs_app::ui::import::ConfirmedImport {
    let bytes = blueprint_document(records);
    let source = proposal(&bytes, LegacyArtifactClass::CustomAircraft)
        .expect("the fixture source is within the cap");
    let (layout, map) = blueprint_layout_and_map();
    let ids = id_map();
    let catalog = catalog(CatalogRows::Base);
    let (rules, policy, book) = stock();
    let context = context(&ids, &catalog, &rules, &policy, &book, true);
    let mut flow = ImportFlow::new();
    flow.offer(
        &context,
        &offer(&source, &bytes, Some(&layout), Some(&map), target),
    );
    flow.confirm()
        .expect("the fixture document confirms")
        .clone()
}

/// One record's `(index, slot, class:id)` identities, in the order the import
/// carries them.
fn expected_identities(confirmed: &cs_app::ui::import::ConfirmedImport) -> Vec<(u32, u32, String)> {
    let mut expected = Vec::new();
    for record in confirmed.report().records() {
        if !confirmed
            .importable_records()
            .contains(&record.record_index)
        {
            continue;
        }
        for (slot, (class, id)) in record.resolved_ids.iter().enumerate() {
            expected.push((
                record.record_index,
                u32::try_from(slot).expect("a slot fits a u32"),
                format!("{}:{id}", class.label()),
            ));
        }
    }
    expected
}

/// The `extra` value a stored document keeps under `key`.
fn extra<'d>(document: &'d ProfileDocument, key: &str) -> Option<&'d str> {
    document
        .extra
        .iter()
        .find(|field| field.key == key)
        .map(|field| field.value.as_str())
}

/// A unique key of exactly [`MAX_KEY_BYTES`] bytes: the prefix and the index
/// make it distinct, the padding makes it as long as the schema admits.
fn long_key(prefix: &str, index: usize) -> String {
    let mut key = format!("{prefix}{index}");
    while key.len() < MAX_KEY_BYTES {
        key.push('k');
    }
    key
}

/// A distinct content key of the largest length a [`ContentId`] admits.
fn long_content_key(index: usize) -> String {
    let head = format!("{index:0>4}");
    format!("{head}{}", "a".repeat(MAX_CONTENT_KEY_LEN - head.len()))
}

/// A [`MAX_VALUE_BYTES`]-long legal identity value: a long class label, a
/// colon, and a `ContentId` whose key is at its own bound.
fn maximal_identity_value() -> String {
    let id = ContentId::from_source(ContentKind::MultiplayerScenario, &long_content_key(0))
        .expect("a bounded content key is valid");
    let class = "c".repeat(MAX_VALUE_BYTES - 1 - id.as_str().len());
    format!("{class}:{id}")
}

/// The schema-wide bounds the import path is written against, checked on a
/// document rather than through the import flow: the identity list is refused
/// one entry past its bound, a record may name one identity per slot, and a
/// value must be a `class:id` pair naming a real content identity.
#[test]
fn accept_f64_c_fu1_the_identity_list_is_bounded_and_unique() {
    let airframe = cid(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY);
    let value = format!("airframe:{airframe}");

    // Exactly at the bound the document validates: the bound admits a full
    // 512-record import at the eight-slot designed layout.
    let mut document = ProfileDocument::synthetic(ProfileId::new(1).expect("nonzero"), Revision(1));
    document.import_identities = (0..MAX_IMPORTED_IDENTITIES)
        .map(|index| ImportedIdentity {
            record_index: u32::try_from(index / 8).expect("a record index fits"),
            slot: u32::try_from(index % 8).expect("a slot fits"),
            value: value.clone(),
        })
        .collect();
    assert_eq!(document.import_identities.len(), 4_096);
    document
        .validate()
        .expect("a full identity list validates at its bound");

    // One identity over the bound is refused, not dropped.
    document.import_identities.push(ImportedIdentity {
        record_index: u32::try_from(MAX_IMPORTED_RECORDS).expect("a record index fits"),
        slot: 0,
        value: value.clone(),
    });
    assert!(
        matches!(
            document.validate(),
            Err(ProfileFieldError::TooManyEntries {
                field: "import identity",
                max: MAX_IMPORTED_IDENTITIES,
            })
        ),
        "an identity list over its bound is refused by name"
    );

    // Two identities for one record's slot would make the record's composition
    // ambiguous, so the document refuses the repeat.
    let mut duplicate =
        ProfileDocument::synthetic(ProfileId::new(1).expect("nonzero"), Revision(1));
    duplicate.import_identities = vec![
        ImportedIdentity {
            record_index: 3,
            slot: 1,
            value: value.clone(),
        },
        ImportedIdentity {
            record_index: 3,
            slot: 1,
            value: value.clone(),
        },
    ];
    assert!(
        matches!(
            duplicate.validate(),
            Err(ProfileFieldError::DuplicateKey {
                field: "import identity",
                ..
            })
        ),
        "one identity per record slot is enforced by the schema"
    );

    // A value that is not a verified `class:id` identity is refused rather
    // than kept as an opaque string.
    let mut malformed =
        ProfileDocument::synthetic(ProfileId::new(1).expect("nonzero"), Revision(1));
    malformed.import_identities = vec![ImportedIdentity {
        record_index: 0,
        slot: 0,
        value: "airframe".to_owned(),
    }];
    assert!(
        matches!(
            malformed.validate(),
            Err(ProfileFieldError::Malformed {
                field: "import identity"
            })
        ),
        "a half-understood identity is refused"
    );
}

/// An import at the chosen maximum — `MAX_IMPORTED_RECORDS` records at the
/// eight-slot designed layout — persists through the production store, and
/// every one of its 4,096 identities plus the source fingerprint round-trips
/// through a close and reopen.
#[test]
fn accept_f64_c_fu1_the_chosen_maximum_persists() {
    let base = TempBase::new("capacity-max");
    let records: Vec<BlueprintRecord> = (0..MAX_IMPORTED_RECORDS).map(|_| full_record()).collect();
    let target = synthetic_target(1, "Every imported pilot");
    let confirmed = confirmed_fixture(&records, &target);
    assert_eq!(
        confirmed.importable_records().len(),
        MAX_IMPORTED_RECORDS,
        "the fixture makes the largest import a profile can hold"
    );
    let expected = expected_identities(&confirmed);
    assert_eq!(
        expected.len(),
        MAX_IMPORTED_IDENTITIES,
        "the designed layout resolves eight identities for every record"
    );

    let mut session = sandbox(base.path());
    let landed = session
        .persist_confirmed_import(&confirmed)
        .expect("the largest import a profile can hold persists");
    assert_eq!(landed, target.id());

    let check_stored = |document: &ProfileDocument| {
        assert_eq!(
            document.import_identities.len(),
            MAX_IMPORTED_IDENTITIES,
            "every resolved identity of every importable record is carried"
        );
        let stored: Vec<(u32, u32, String)> = document
            .import_identities
            .iter()
            .map(|entry| (entry.record_index, entry.slot, entry.value.clone()))
            .collect();
        assert_eq!(
            stored, expected,
            "every record's identities are stored exactly as the report resolved them"
        );
        assert_eq!(
            extra(document, "import.source.sha256"),
            Some(confirmed.source().sha256().to_hex().as_str()),
            "the source fingerprint round-trips (F64 non-negotiable 1)"
        );
        assert_eq!(extra(document, "import.verdict"), Some("full import"));
        assert_eq!(
            document.blueprints.len(),
            MAX_IMPORTED_RECORDS,
            "every importable record's conforming blueprint is owned"
        );
        assert_eq!(
            document.blueprints,
            confirmed
                .blueprints()
                .expect("the offer declared a field map")
                .records()
                .iter()
                .filter_map(|row| match row.outcome() {
                    cs_content::legacy_import::BlueprintRecordOutcome::Conforming {
                        blueprint,
                        ..
                    } => Some(blueprint.id().clone()),
                    _ => None,
                })
                .collect::<Vec<ContentId>>()
        );
        // The at-cap import is why the save schema's document bounds moved:
        // encoded, it is about 272 KB and 4,600+ lines — past the previous
        // 256 KiB / 4,096-line bounds and still inside the new ones — and it
        // decodes back above, so the wider bound is real rather than
        // unreachable slack.
        let encoded = cs_content::save::codec::encode(document).expect("the revision encodes");
        assert!(
            encoded.len() > 256 * 1024,
            "the at-cap import is {} bytes, so it cannot fit the previous document bound",
            encoded.len()
        );
        assert!(
            encoded.iter().filter(|byte| **byte == b'\n').count() > 4_096,
            "the at-cap import has more lines than the previous document bound"
        );
        assert!(encoded.len() <= cs_content::save::codec::MAX_SAVE_BYTES);
        assert!(
            encoded.iter().filter(|byte| **byte == b'\n').count()
                <= cs_content::save::codec::MAX_SAVE_LINES
        );
    };

    let document = session
        .document()
        .expect("the imported profile is selected");
    check_stored(document);
    drop(session);

    // The codec, not just the in-memory value: the largest document the schema
    // admits decodes back to the same identity set.
    let reopened = sandbox(base.path());
    let loaded = reopened
        .library()
        .load(landed)
        .expect("the imported slot reloads");
    check_stored(loaded.document.as_ref().expect("the stored revision"));
}

/// One record over the maximum is refused by name, before any allocation or
/// write: the reader admits the document (513 is far below its 4,096-record
/// limit), the profile cannot hold it, and nothing is truncated.
#[test]
fn accept_f64_c_fu1_one_record_over_the_maximum_is_refused() {
    let base = TempBase::new("capacity-over");
    let records: Vec<BlueprintRecord> = (0..=MAX_IMPORTED_RECORDS).map(|_| full_record()).collect();
    let target = synthetic_target(1, "One pilot too many");
    let confirmed = confirmed_fixture(&records, &target);
    assert_eq!(
        confirmed.importable_records().len(),
        MAX_IMPORTED_RECORDS + 1,
        "the reader admits the oversized document, so only the profile bound can refuse it"
    );

    let mut session = sandbox(base.path());
    let error = session
        .persist_confirmed_import(&confirmed)
        .expect_err("an import over the profile bound is refused");
    assert!(
        matches!(
            &error,
            SessionError::ImportTooLarge { records, max }
                if *records == MAX_IMPORTED_RECORDS + 1 && *max == MAX_IMPORTED_RECORDS
        ),
        "the refusal names the count and the bound: {error:?}"
    );
    assert_eq!(session.library().live(), &[], "no profile was created");
    assert!(
        !base
            .path()
            .join(ProfileKind::Synthetic.label())
            .join(slot_name(target.id()))
            .exists(),
        "no slot was allocated for the refused import"
    );
}

/// Every document the save schema validates fits the codec's document bounds:
/// with every list at its own maximum and every value at its own bound — 4,096
/// identities carrying maximal `class:id` values, 512 blueprints with maximal
/// content keys, and the other six bounded lists filled — the document still
/// encodes and decodes.
///
/// The at-cap import above is about 272 KB; this is the ~1.6 MiB true worst case
/// the raised document bounds are derived from, so the derivation is measured
/// rather than asserted. It fails if either bound is left at the previous
/// 256 KiB / 4,096 lines, which no list bound alone would catch.
#[test]
fn accept_f64_c_fu1_a_maximal_document_fits_the_raised_bounds() {
    use cs_content::save::codec::{MAX_SAVE_BYTES, MAX_SAVE_LINES, decode, encode};

    let identity_value = maximal_identity_value();
    assert_eq!(
        identity_value.len(),
        MAX_VALUE_BYTES,
        "the worst case uses an identity value at the value bound"
    );

    let mut document = ProfileDocument::synthetic(ProfileId::new(1).expect("nonzero"), Revision(1));
    document.display_name = "d".repeat(MAX_DISPLAY_NAME_BYTES);
    document.campaign = CampaignState {
        run_id: Some(long_key("run", 0)),
        money_minor: u64::MAX,
        applied_outcomes: (0..MAX_IMPORTED_RECORDS)
            .map(|index| long_key("outcome", index))
            .collect(),
    };
    document.blueprints = (0..MAX_IMPORTED_RECORDS)
        .map(|index| {
            ContentId::from_source(ContentKind::Blueprint, &long_content_key(index))
                .expect("a bounded content key is valid")
        })
        .collect();
    document.records = (0..MAX_IMPORTED_RECORDS)
        .map(|index| RecordEntry {
            key: long_key("record", index),
            value: u64::MAX,
        })
        .collect();
    document.settings = (0..MAX_IMPORTED_RECORDS)
        .map(|index| SettingEntry {
            key: long_key("setting", index),
            apply: SettingApply::RestartRequired,
            value: "v".repeat(MAX_VALUE_BYTES),
        })
        .collect();
    document.fingerprints = (0..MAX_IMPORTED_RECORDS)
        .map(|index| FingerprintEntry {
            name: long_key("fingerprint", index),
            hash: u64::MAX,
        })
        .collect();
    document.extra = (0..MAX_IMPORTED_RECORDS)
        .map(|index| ExtraField {
            key: long_key("extra", index),
            value: "x".repeat(MAX_VALUE_BYTES),
        })
        .collect();
    let template = identity_value.clone();
    document.import_identities = (0..MAX_IMPORTED_RECORDS)
        .flat_map(|record| {
            let value = template.clone();
            (0..MAX_IMPORTED_IDENTITIES_PER_RECORD).map(move |slot| ImportedIdentity {
                record_index: u32::try_from(record).expect("a record index fits"),
                slot: u32::try_from(slot).expect("a slot fits"),
                value: value.clone(),
            })
        })
        .collect();

    document
        .validate()
        .expect("every list at its own bound validates");

    let encoded = encode(&document).expect("the maximal document encodes");
    let lines = encoded.iter().filter(|byte| **byte == b'\n').count();
    assert!(
        encoded.len() > MAX_SAVE_BYTES / 2,
        "the worst case is {} bytes, so it exercises the bound well past the previous 256 KiB",
        encoded.len()
    );
    assert!(
        encoded.len() <= MAX_SAVE_BYTES,
        "the maximal document is {} bytes, over the {MAX_SAVE_BYTES} a save may be",
        encoded.len()
    );
    assert!(
        lines <= MAX_SAVE_LINES,
        "the maximal document has {lines} lines, over the {MAX_SAVE_LINES} a save may have"
    );
    let decoded = decode(&encoded).expect("the maximal document decodes");
    assert_eq!(
        decoded, document,
        "every list survives the codec at its own bound"
    );
}
