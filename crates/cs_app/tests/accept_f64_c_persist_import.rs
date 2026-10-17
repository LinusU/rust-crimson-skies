//! Acceptance scenarios for persisting a F64-C [`ConfirmedImport`] into a new
//! profile, atomically, through the session's profile store (task #756, a
//! follow-up of `specs/F64-legacy-custom-aircraft-and-optional-save-import.md`
//! stage `### F64-C`; contract `docs/contracts/STATE-TRANSACTIONS.md`).
//!
//! Task test prefix: `accept_f64_c_persist_`.
//!
//! The import fixture is the same designed content F64-C's report tests use
//! (`f64_c_support`): a synthetic blueprint document read through the designed
//! layout, which only `LayoutAdmission::AllowDesignedFixtures` admits, so the
//! imported profile lands in the **synthetic** population a sandbox session
//! owns. Nothing here reads `$CS_GAME_DIR` or claims an original file format.
//!
//! What these tests prove about the write itself:
//!
//! * the new profile carries the retained report — the source's spelling,
//!   byte size, full SHA-256 and the verdict — and exactly the records
//!   [`ConfirmedImport::importable_records`] names;
//! * the source file's bytes and mtime never change, and an existing
//!   profile's slot files never change;
//! * naming a profile id that is not the next allocation is refused, never
//!   merged or overwritten;
//! * a write torn between temp-write and rename recovers the previous
//!   revision.

use std::fs;

use cs_app::profile::{ProfileSession, SessionError};
use cs_app::ui::import::ImportFlow;
use cs_content::legacy_import::{
    BlueprintRecordOutcome, LegacyIdBinding, TargetProfile, legacy_save_import_rule,
};
use cs_content::save::library::{LibraryError, slot_name};
use cs_content::save::settings::SettingCatalog;
use cs_formats::legacy_profile::{LegacyArtifactClass, LegacyIdClass};
use cs_types::content::{ContentId, ContentKind};
use cs_types::profile::{ProfileDocument, ProfileId, ProfileKind, Revision};

use crate::f64_c_support as support;

use support::{
    CatalogRows, TempBase, blueprint_document, blueprint_layout_and_map, catalog, cid, context,
    full_record, id_map, offer, profile_document, proposal, stock,
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
    records: &[support::BlueprintRecord],
    target: &TargetProfile,
) -> (cs_app::ui::import::ConfirmedImport, Vec<u8>) {
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
    let confirmed = flow
        .confirm()
        .expect("the fixture document confirms")
        .clone();
    (confirmed, bytes)
}

/// The `extra` value a stored document keeps under `key`.
fn extra<'d>(document: &'d ProfileDocument, key: &str) -> Option<&'d str> {
    document
        .extra
        .iter()
        .find(|field| field.key == key)
        .map(|field| field.value.as_str())
}

/// The distinct `import.record.<index>` prefixes a stored document carries,
/// in order — the carried record set.
fn record_indices(document: &ProfileDocument) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    document
        .extra
        .iter()
        .filter(|field| field.key.starts_with("import.record."))
        .filter_map(|field| {
            field
                .key
                .strip_prefix("import.record.")
                .and_then(|rest| rest.split('.').next())
                .filter(|index| seen.insert(index.to_owned()))
                .map(str::to_owned)
        })
        .collect()
}

/// The blueprint ids the confirmed import's assessment judged conforming and
/// [`cs_app::ui::import::ConfirmedImport::importable_records`] admits.
fn expected_blueprints(confirmed: &cs_app::ui::import::ConfirmedImport) -> Vec<ContentId> {
    confirmed
        .blueprints()
        .map(|assessment| {
            assessment
                .records()
                .iter()
                .filter(|row| confirmed.importable_records().contains(&row.record_index()))
                .filter_map(|row| match row.outcome() {
                    BlueprintRecordOutcome::Conforming { blueprint, .. } => {
                        Some(blueprint.id().clone())
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A confirmed import lands in a newly allocated profile through the store's
/// atomic path, the retained report round-trips through a close and reopen,
/// and nothing about the source file or an existing profile changes.
#[test]
fn accept_f64_c_persist_a_confirmed_import_lands_in_a_new_profile() {
    let base = TempBase::new("persist-new");

    // The source as a real file: a confirmed import is built from bytes, but
    // its fingerprint describes *this* file — the one the player pointed at.
    // Persisting must never write over it.
    let source_dir = base.path().join("sources");
    fs::create_dir_all(&source_dir).expect("the source directory is created");
    let source_path = source_dir.join("custom.pln");

    let mut session = sandbox(base.path());
    let existing = session.create("Existing pilot").expect("create");
    session
        .document_mut()
        .expect("a document")
        .campaign
        .money_minor = 7_000;
    session.commit().expect("a second committed revision");
    let existing_files_before = support::tree(&session.library().slot_dir(existing));

    // The next allocation is id 2: the offer's target names it, which is the
    // whole point of allocating the target while the owner reads the report.
    let target = synthetic_target(2, "Imported pilot");
    let (confirmed, bytes) = confirmed_fixture(&[full_record()], &target);
    fs::write(&source_path, &bytes).expect("the source file is written");
    let source_bytes_before = fs::read(&source_path).expect("read the source");
    let source_mtime_before = fs::metadata(&source_path)
        .and_then(|meta| meta.modified())
        .expect("the source has an mtime");

    let landed = session
        .persist_confirmed_import(&confirmed)
        .expect("the confirmed import persists");
    assert_eq!(
        landed,
        target.id(),
        "the import lands in the profile the offer allocated"
    );
    assert_eq!(session.selected(), Some(landed));
    assert_eq!(session.library().active(), Some(landed));

    // The source is a read through its fingerprint only: identical bytes and
    // an identical mtime afterwards.
    assert_eq!(
        fs::read(&source_path).expect("read the source after"),
        source_bytes_before,
        "the source's bytes are untouched"
    );
    assert_eq!(
        fs::metadata(&source_path)
            .and_then(|meta| meta.modified())
            .expect("the source still has an mtime"),
        source_mtime_before,
        "the source's mtime is untouched"
    );

    // The existing profile's slot is byte-identical: the import wrote a new
    // profile, not a revision of one that was there.
    assert_eq!(
        support::tree(&session.library().slot_dir(existing)),
        existing_files_before,
        "the existing profile's slot files are untouched"
    );

    let check_stored = |document: &ProfileDocument| {
        assert_eq!(document.profile_id, landed);
        assert_eq!(document.kind, ProfileKind::Synthetic);
        assert_eq!(document.display_name, "Imported pilot");
        assert_eq!(document.revision, Revision(1));
        // The retained report round-trips (spec F64 non-negotiable 1).
        assert_eq!(
            extra(document, "import.source.spelling"),
            Some(support::FIXTURE_SPELLING)
        );
        assert_eq!(
            extra(document, "import.source.size"),
            Some(bytes.len().to_string().as_str())
        );
        assert_eq!(
            extra(document, "import.source.sha256"),
            Some(confirmed.source().sha256().to_hex().as_str())
        );
        assert_eq!(extra(document, "import.verdict"), Some("full import"));
        assert_eq!(extra(document, "import.fixture_admitted"), Some("yes"));
        // A made profile's first revision records its resolved defaults,
        // exactly like one `create` wrote.
        assert!(
            document
                .settings
                .iter()
                .any(|entry| entry.key == cs_formats::legacy_profile::LEGACY_SAVE_IMPORT_SWITCH),
            "the catalog seed is materialized in the first revision"
        );
        // Exactly the importable set is carried: one record field per index,
        // naming the resolved identities, and the conforming blueprint ids.
        assert_eq!(confirmed.importable_records(), &[0]);
        assert_eq!(record_indices(document), vec!["0".to_owned()]);
        for (slot, (class, id)) in confirmed.report().records()[0]
            .resolved_ids
            .iter()
            .enumerate()
        {
            assert_eq!(
                extra(document, &format!("import.record.0.{slot}")),
                Some(format!("{}:{id}", class.label())).as_deref(),
                "the stored record names {id} under class {class}"
            );
        }
        assert_eq!(document.blueprints, expected_blueprints(&confirmed));
        assert_eq!(document.blueprints.len(), 1);
    };

    // The in-memory view and the re-read view agree: the codec round-trip is
    // what makes the retained report *persisted*, not just written.
    let document = session
        .document()
        .expect("the imported profile is selected");
    check_stored(document);
    drop(session);
    let reopened = sandbox(base.path());
    let loaded = reopened
        .library()
        .load(landed)
        .expect("the imported slot reloads");
    check_stored(loaded.document.as_ref().expect("the stored revision"));
    assert_eq!(
        reopened.selected(),
        Some(landed),
        "the imported profile is the persisted active pointer"
    );
}

/// A save-type import has no blueprint report at all: the carried records
/// still land as resolved-identity fields and `blueprints` stays empty —
/// nothing fabricates an aircraft the source did not describe.
#[test]
fn accept_f64_c_persist_a_save_import_carries_records_without_blueprints() {
    let base = TempBase::new("persist-save");
    let mut session = sandbox(base.path());

    let bytes = profile_document(&[(7, 1, "phoenix"), (7, 2, "kestrel")]);
    let source = proposal(&bytes, LegacyArtifactClass::CampaignSave)
        .expect("the fixture source is within the cap");
    let layout = cs_formats::legacy_profile::synthetic_layout();
    let ids = id_map();
    let catalog = catalog(CatalogRows::Base);
    let (rules, policy, book) = stock();
    let context = context(&ids, &catalog, &rules, &policy, &book, true);
    let target = synthetic_target(1, "Imported campaign");

    let mut flow = ImportFlow::new();
    flow.offer(
        &context,
        // No field map: a campaign save is judged for records alone.
        &offer(&source, &bytes, Some(&layout), None, &target),
    );
    let confirmed = flow
        .confirm()
        .expect("resolved save records confirm")
        .clone();
    assert!(confirmed.blueprints().is_none());
    assert_eq!(confirmed.importable_records(), &[0, 1]);

    let landed = session
        .persist_confirmed_import(&confirmed)
        .expect("the save import persists");
    let loaded = session
        .library()
        .load(landed)
        .expect("the imported slot loads");
    let document = loaded.document.as_ref().expect("the stored revision");

    assert_eq!(extra(document, "import.verdict"), Some("full import"));
    assert_eq!(
        record_indices(document),
        vec!["0".to_owned(), "1".to_owned()],
        "both resolved records are carried"
    );
    assert_eq!(
        extra(document, "import.record.0.0"),
        Some(
            format!(
                "airframe:{}",
                cs_types::content::ContentId::from_source(
                    cs_types::content::ContentKind::Airframe,
                    cs_content::construction::SYNTHETIC_AIRFRAME_KEY,
                )
                .expect("the fixture id is valid")
            )
            .as_str()
        )
    );
    assert!(
        document.blueprints.is_empty(),
        "a save import mints no blueprint"
    );
}

/// A partial import carries only the records `importable_records()` names:
/// the record the id table could not resolve names no field and mints no
/// blueprint id, while the report still says `partial import`.
#[test]
fn accept_f64_c_persist_carries_only_the_importable_records() {
    let base = TempBase::new("persist-partial");
    let mut session = sandbox(base.path());

    let mut second = full_record();
    second.rockets = [5, 9]; // 9 is not bound by the id table.
    let target = synthetic_target(1, "Imported pilot");
    let (confirmed, _bytes) = confirmed_fixture(&[full_record(), second], &target);
    assert_eq!(
        confirmed.importable_records(),
        &[0],
        "the unresolvable record is planned out"
    );

    let landed = session
        .persist_confirmed_import(&confirmed)
        .expect("the partial import persists");
    let loaded = session
        .library()
        .load(landed)
        .expect("the imported slot loads");
    let document = loaded.document.as_ref().expect("the stored revision");

    assert_eq!(extra(document, "import.verdict"), Some("partial import"));
    assert_eq!(
        record_indices(document),
        vec!["0".to_owned()],
        "only the importable record is carried — index 1 names no field"
    );
    assert!(
        extra(document, "import.record.1.0").is_none(),
        "the unresolved record is not carried"
    );
    assert_eq!(
        document.blueprints,
        expected_blueprints(&confirmed),
        "only the importable record's conforming blueprint is owned"
    );
    assert_eq!(document.blueprints.len(), 1);
}

/// A record the plan resolved but the stock rules do not accept is not
/// carried: `importable_records()` is planned **and** stock-conforming, so a
/// non-conforming record names no field and mints no blueprint even though the
/// migration report lists it as a resolved record.
///
/// This is the case the `importable_records()` filter exists for: the plan's
/// `records()` alone would carry it. The fixture's second airframe is parsed
/// and ready, so every id slot of the second record resolves, but the fixture
/// rules and price book describe the first airframe only, so the assembled
/// blueprint is refused by the stock stage while the first record conforms.
#[test]
fn accept_f64_c_persist_drops_a_record_the_stock_rules_refuse() {
    let base = TempBase::new("persist-nonconforming");
    let mut session = sandbox(base.path());

    // Bind a second airframe the catalog holds but the fixture rules do not
    // describe; the plan resolves it, the blueprint stage cannot judge it.
    let mut ids = id_map();
    ids.insert(
        LegacyIdBinding::new(
            LegacyIdClass::Airframe,
            9,
            cid(ContentKind::Airframe, "fixture.synthetic_alpha"),
        )
        .expect("the binding is in its namespace"),
    )
    .expect("the binding inserts");

    let mut nonconforming = full_record();
    nonconforming.name = "unsold";
    nonconforming.airframe = 9;

    let bytes = blueprint_document(&[full_record(), nonconforming]);
    let source = proposal(&bytes, LegacyArtifactClass::CustomAircraft)
        .expect("the fixture source is within the cap");
    let (layout, map) = blueprint_layout_and_map();
    let catalog = catalog(CatalogRows::Shifted);
    let (rules, policy, book) = stock();
    let context = context(&ids, &catalog, &rules, &policy, &book, true);
    let target = synthetic_target(1, "Imported pilot");
    let mut flow = ImportFlow::new();
    flow.offer(
        &context,
        &offer(&source, &bytes, Some(&layout), Some(&map), &target),
    );
    let confirmed = flow
        .confirm()
        .expect("the conforming record confirms")
        .clone();

    // Both records resolved, so the plan and the report list both; only the
    // first is both planned and stock-conforming.
    assert_eq!(
        confirmed.report().records().len(),
        2,
        "the plan resolved both records, so the report lists both"
    );
    assert_eq!(confirmed.importable_records(), &[0]);
    let assessment = confirmed
        .blueprints()
        .expect("the offer declared a field map");
    assert!(
        !matches!(
            assessment.records()[1].outcome(),
            BlueprintRecordOutcome::Conforming { .. }
        ),
        "the second record is not stock-conforming"
    );

    let landed = session
        .persist_confirmed_import(&confirmed)
        .expect("the conforming record persists");
    let loaded = session
        .library()
        .load(landed)
        .expect("the imported slot loads");
    let document = loaded.document.as_ref().expect("the stored revision");

    assert_eq!(
        record_indices(document),
        vec!["0".to_owned()],
        "the non-conforming record names no field"
    );
    assert!(
        extra(document, "import.record.1.0").is_none(),
        "a resolved but non-conforming record is not carried"
    );
    assert_eq!(
        document.blueprints,
        expected_blueprints(&confirmed),
        "only the conforming record's blueprint is owned"
    );
    assert_eq!(document.blueprints.len(), 1);
}

/// A deleted profile's id is retired, never reissued: an offer naming it is
/// refused rather than resurrecting a slot the player deleted.
#[test]
fn accept_f64_c_persist_refuses_a_retired_profile_id() {
    let base = TempBase::new("persist-retired");
    let mut session = sandbox(base.path());
    let deleted = session.create("Deleted pilot").expect("create");
    session.delete(deleted).expect("delete");
    assert_eq!(session.library().live(), &[]);

    let target = synthetic_target(deleted.get(), "Resurrected pilot");
    let (confirmed, _) = confirmed_fixture(&[full_record()], &target);
    let error = session
        .persist_confirmed_import(&confirmed)
        .expect_err("a retired id is not a new profile");
    assert!(
        matches!(
            &error,
            SessionError::ImportTargetMismatch {
                offered,
                expected_next: 2,
            } if *offered == target.id()
        ),
        "the refusal names the retired id and the real next allocation: {error:?}"
    );
    assert_eq!(
        session.library().live(),
        &[],
        "nothing was resurrected or written"
    );
}

/// Naming a profile id the population would not allocate next — an issued id,
/// one the high-water mark has not reached, or a retired id — is refused,
/// never merged or overwritten; a target of another population's kind is
/// refused by the population rule.
#[test]
fn accept_f64_c_persist_refuses_to_write_over_an_existing_profile() {
    let base = TempBase::new("persist-refuse");
    let mut session = sandbox(base.path());
    let existing = session.create("Existing pilot").expect("create");
    let existing_files_before = support::tree(&session.library().slot_dir(existing));

    // The offer's target is the already-live id 1 while the mark would hand
    // out 2: persisting it would be writing over a profile.
    let taken = synthetic_target(1, "A different pilot");
    let (confirmed, _) = confirmed_fixture(&[full_record()], &taken);
    let error = session
        .persist_confirmed_import(&confirmed)
        .expect_err("an issued id is not a new profile");
    assert!(
        matches!(
            &error,
            SessionError::ImportTargetMismatch {
                offered,
                expected_next: 2,
            } if *offered == taken.id()
        ),
        "the refusal names the offered id and the real next allocation: {error:?}"
    );

    // An id ahead of the mark is stale in the other direction: the offer was
    // made for a population state that no longer exists.
    let future = synthetic_target(7, "A pilot from a stale offer");
    let (confirmed, _) = confirmed_fixture(&[full_record()], &future);
    let error = session
        .persist_confirmed_import(&confirmed)
        .expect_err("a stale allocation is refused too");
    assert!(
        matches!(
            &error,
            SessionError::ImportTargetMismatch {
                offered,
                expected_next: 2,
            } if *offered == future.id()
        ),
        "the refusal names what the mark would allocate: {error:?}"
    );

    // A target of another population's kind never reaches the id check.
    let foreign = TargetProfile::new(
        ProfileId::new(2).expect("nonzero"),
        ProfileKind::Modded,
        "A modded pilot",
    )
    .expect("the target names itself");
    let (confirmed, _) = confirmed_fixture(&[full_record()], &foreign);
    let error = session
        .persist_confirmed_import(&confirmed)
        .expect_err("another population's target is refused");
    assert!(
        matches!(
            &error,
            SessionError::Library(LibraryError::ForeignDocument { .. })
        ),
        "the population rule names the foreign document: {error:?}"
    );

    // Nothing was written anywhere: the live set is still just the first
    // profile, and its slot is byte-identical.
    assert_eq!(session.library().live(), &[existing]);
    assert_eq!(
        support::tree(&session.library().slot_dir(existing)),
        existing_files_before,
        "the existing profile's slot files are untouched"
    );
}

/// A write torn between temp-write and rename — a leftover `profile.tmp`
/// that never finished — recovers the previous committed revision: the
/// imported profile survives a mid-write crash intact.
#[test]
fn accept_f64_c_persist_recovers_the_previous_profile_after_a_mid_write_crash() {
    let base = TempBase::new("persist-crash");
    let target = synthetic_target(1, "Imported pilot");
    let (confirmed, _bytes) = confirmed_fixture(&[full_record()], &target);

    let landed;
    let stored_before;
    {
        let mut session = sandbox(base.path());
        landed = session
            .persist_confirmed_import(&confirmed)
            .expect("the confirmed import persists");
        stored_before = fs::read(session.library().slot_dir(landed).join("profile.sav"))
            .expect("the committed revision is on disk");
    }
    // The crash: a commit wrote its temp and died before the rename. What it
    // leaves is a `profile.tmp` that does not decode — the torn half-write.
    let slot = base
        .path()
        .join(ProfileKind::Synthetic.label())
        .join(slot_name(landed));
    fs::write(slot.join("profile.tmp"), b"a torn half-write").expect("the crash debris is planted");

    // Recovery keeps the previous committed revision and says the temp was
    // ignored, instead of installing the debris.
    let reopened = sandbox(base.path());
    let loaded = reopened
        .library()
        .load(landed)
        .expect("the imported slot recovers");
    let document = loaded.document.as_ref().expect("a whole revision survives");
    assert_eq!(document.revision, Revision(1));
    assert_eq!(document.profile_id, landed);
    assert_eq!(
        extra(document, "import.source.sha256"),
        Some(confirmed.source().sha256().to_hex().as_str()),
        "the retained fingerprint is the committed revision's"
    );
    assert!(
        loaded
            .warnings
            .iter()
            .any(|warning| warning.contains("profile.tmp did not decode")),
        "the torn temp is reported, not installed: {:?}",
        loaded.warnings
    );
    assert_eq!(
        fs::read(slot.join("profile.sav")).expect("the current file remains"),
        stored_before,
        "the committed bytes are still the installed ones"
    );
}
