//! F64-C acceptance tests: import validation and the user-facing migration
//! report (`specs/F64-legacy-custom-aircraft-and-optional-save-import.md`,
//! stage `### F64-C`).
//!
//! The stage's minimum scenario is sheet **AC03**: reordering catalog entries
//! does not remap an imported weapon to another type. That is pinned end to
//! end through the stage's own consumer — the same bytes are offered to
//! [`ImportFlow`] against two catalogs whose *row order* differs, and the
//! report both attempts render must be identical, with the weapon-class slot
//! presenting the identity the id table declared rather than whatever sits at
//! that position in the catalog.
//!
//! The parent sheet's other three criteria are preserved here at the same
//! boundary: **AC01** (a hostile or oversized old profile fails without
//! touching the source or any new save), **AC02** (a legacy blueprint over a
//! stock limit is rejected with the specific fields) and **AC04** (the
//! optional old-save migration can be switched off while new profiles and the
//! required class keep working). F64-B proved AC02 inside the producer; here
//! it is proved where the player meets it — the report carries the exact
//! `limit`/`total` pair and the confirm action refuses the whole import.
//!
//! Everything synthetic below is newly authored fixture content read through
//! the production `read_legacy_profile`, resolved by the production
//! `LegacyIdMap`/`Catalog` pair and judged by the production
//! `ConstructionRules::validate`, under the fixture admission the tests name
//! explicitly. The last test is the `retail` one: it needs `$CS_GAME_DIR`
//! (marked `#[ignore = "requires CS_GAME_DIR"]`, run with
//! `--include-ignored`) and measures what this installation can actually
//! offer the import path today.

#[path = "f64_c_support/mod.rs"]
mod support;

use std::fs;

use cs_app::ui::import::{
    ConfirmError, FlowRefusal, ImportContext, ImportFlow, MigrationVerdict, ReportLine,
};
use cs_assets::install::sha256;
use cs_content::legacy_import::{
    ImportClass, ImportRefusal, LayoutAdmission, LegacyIdMap, TargetProfile, UnresolvedReason,
};
use cs_formats::legacy_profile::{
    ArtifactProposal, LegacyArtifactClass, LegacyIdClass, LegacyLimits, MAX_LEGACY_SOURCE_BYTES,
    synthetic_blueprint_layout, synthetic_layout,
};
use cs_types::content::ContentKind;
use cs_types::evidence::ClaimStatus;
use cs_types::profile::{ProfileId, ProfileKind};
use support::{
    BlueprintRecord, CatalogRows, TempBase, blueprint_document, blueprint_layout_and_map, catalog,
    cid, context, designed, full_record, id_map, id_map_reordered, no_id_layout, offer,
    position_of, profile_document, proposal, stock, target, tight_mass_rules, tree,
};

/// The identities the fixture document's one record must present, in layout
/// declaration order — whatever order the catalog was filled in.
fn expected_identities() -> Vec<(LegacyIdClass, cs_types::content::ContentId)> {
    let airframe = (
        LegacyIdClass::Airframe,
        cid(
            ContentKind::Airframe,
            cs_content::construction::SYNTHETIC_AIRFRAME_KEY,
        ),
    );
    let engine = (
        LegacyIdClass::Engine,
        cid(
            ContentKind::Engine,
            cs_content::construction::SYNTHETIC_ENGINE_KEY,
        ),
    );
    let gun = (
        LegacyIdClass::Weapon,
        cid(
            ContentKind::Weapon,
            cs_content::construction::SYNTHETIC_GUN_KEY,
        ),
    );
    let rocket = (
        LegacyIdClass::Ordnance,
        cid(
            ContentKind::Weapon,
            cs_content::construction::SYNTHETIC_MISSILE_KEY,
        ),
    );
    vec![
        airframe,
        engine,
        gun.clone(),
        gun.clone(),
        gun.clone(),
        gun,
        rocket.clone(),
        rocket,
    ]
}

/// The record line of a rendered report, with its identities.
fn record_identities(lines: &[ReportLine]) -> Vec<(LegacyIdClass, cs_types::content::ContentId)> {
    let mut found = Vec::new();
    for line in lines {
        if let ReportLine::Record { identities, .. } = line {
            found.extend(identities.iter().cloned());
        }
    }
    assert!(
        !found.is_empty(),
        "the report must render a record line; it rendered {lines:?}"
    );
    found
}

/// **AC03** (the stage's minimum scenario): reordering catalog entries does
/// not remap an imported weapon to another type.
///
/// A [`cs_content::catalog::Catalog`] enumerates in canonical id order, so
/// what a reordering of entries does to a consumer is move every identity to
/// a different **position**. The two runs below therefore differ twice: the
/// second catalog adds an unrelated airframe and an unrelated weapon in front
/// of the bound ones, and the second id table declares the very same bindings
/// in the opposite order. The test asserts those positions really differ
/// before it asserts anything else, so a consumer that read a position rather
/// than an identity cannot pass by accident: every line of both reports must
/// come out identical, naming the identities the table declared.
#[test]
fn accept_f64_c_reordered_catalog_never_remaps_an_imported_weapon() {
    let bytes = blueprint_document(&[full_record()]);
    let source = proposal(&bytes, LegacyArtifactClass::CustomAircraft)
        .expect("the fixture source is within the cap");
    let (layout, map) = blueprint_layout_and_map();
    let base_ids = id_map();
    let shifted_ids = id_map_reordered();
    let (rules, policy, book) = stock();
    let target = target();

    let base = catalog(CatalogRows::Base);
    let shifted = catalog(CatalogRows::Shifted);

    // Every identity the record binds sits at a different position in the
    // shifted catalog — the reorder is real, or the scenario is vacuous.
    let bound = [
        cid(
            ContentKind::Airframe,
            cs_content::construction::SYNTHETIC_AIRFRAME_KEY,
        ),
        cid(
            ContentKind::Engine,
            cs_content::construction::SYNTHETIC_ENGINE_KEY,
        ),
        cid(
            ContentKind::Weapon,
            cs_content::construction::SYNTHETIC_GUN_KEY,
        ),
        cid(
            ContentKind::Weapon,
            cs_content::construction::SYNTHETIC_MISSILE_KEY,
        ),
    ];
    for id in &bound {
        let base_at = position_of(&base, id);
        let shifted_at = position_of(&shifted, id);
        assert!(
            base_at.is_some() && shifted_at.is_some(),
            "{id} must be present in both catalogs: {base_at:?} {shifted_at:?}"
        );
        assert_ne!(
            base_at, shifted_at,
            "{id} must sit at a different position, or nothing was reordered"
        );
    }

    let mut rendered = Vec::new();
    for (ids, catalog) in [(&base_ids, &base), (&shifted_ids, &shifted)] {
        let context = context(ids, catalog, &rules, &policy, &book, true);
        let mut flow = ImportFlow::new();
        let lines = flow
            .offer(
                &context,
                &offer(&source, &bytes, Some(&layout), Some(&map), &target),
            )
            .view()
            .expect("the fixture document reports")
            .lines()
            .to_vec();
        rendered.push(lines);
    }

    assert_eq!(
        rendered[0], rendered[1],
        "moving an identity to another position must not change a single line \
         of the migration report"
    );
    for lines in &rendered {
        assert_eq!(
            record_identities(lines),
            expected_identities(),
            "legacy weapon id 1 is the gun and ordnance id 5 is the missile in \
             every catalog order, never the row at that position"
        );
        assert!(
            !lines
                .iter()
                .any(|line| matches!(line, ReportLine::Unresolved { .. })),
            "nothing in this document is unresolved: {lines:?}"
        );
        assert!(
            lines.iter().any(|line| matches!(
                line,
                ReportLine::Verdict {
                    verdict: MigrationVerdict::Full
                }
            )),
            "a fully resolvable document reports a full import"
        );
    }
}

/// **AC01**: a malicious or oversized old profile fails without touching the
/// source or any new save — now proven through the consumer that the screen
/// actually drives, on a real filesystem, with the refusal's own code.
#[test]
fn accept_f64_c_hostile_or_oversized_offer_touches_neither_source_nor_destination() {
    let base = TempBase::new("ac01");
    let source_path = base.path().join("Planes/custom.pln");
    let profiles_root = base.path().join("userdata/production");
    fs::create_dir_all(source_path.parent().expect("the source has a parent"))
        .expect("the source directory is created");
    fs::create_dir_all(&profiles_root).expect("the destination root is created");
    // A pre-existing new-engine save the import must not disturb.
    let existing_save = profiles_root.join("existing.save");
    fs::write(&existing_save, b"fresh-engine-save").expect("the existing save is written");

    let (layout, map) = blueprint_layout_and_map();
    let ids = id_map();
    let (rules, policy, book) = stock();
    let catalog = catalog(CatalogRows::Base);
    let context = context(&ids, &catalog, &rules, &policy, &book, true);
    let target = target();

    let mut bomb = blueprint_document(&[full_record()]);
    // The declared record count, at the offset the fixture layout declares it
    // at: a hostile table the reader must refuse rather than allocate.
    bomb[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
    let oversized = vec![0u8; usize::try_from(MAX_LEGACY_SOURCE_BYTES).expect("the cap fits") + 1];

    for (label, bytes, expected_code) in [
        ("malicious", &bomb, "unreadable"),
        ("oversized", &oversized, "source_too_large"),
    ] {
        fs::write(&source_path, bytes).expect("the payload is written");
        let before = fs::metadata(&source_path).expect("the source exists");
        let modified = before.modified().expect("the platform reports a time");
        let source_bytes = fs::read(&source_path).expect("the source reads");
        let destination_before = tree(&profiles_root);
        assert_eq!(
            destination_before,
            vec![("existing.save".to_owned(), b"fresh-engine-save".to_vec())],
            "the destination holds exactly the pre-existing new-engine save"
        );

        // The declared size is capped rather than honest, so the planner's own
        // size check — not the proposal's constructor — refuses the oversized
        // payload. The digest is the real one for both payloads.
        let source = ArtifactProposal::new(
            support::FIXTURE_SPELLING,
            bytes.len().min(MAX_LEGACY_SOURCE_BYTES as usize) as u64,
            sha256(bytes),
            Some(LegacyArtifactClass::CustomAircraft),
        )
        .expect("the declared size is within the cap");

        let mut flow = ImportFlow::new();
        let refused = flow
            .offer(
                &context,
                &offer(&source, bytes, Some(&layout), Some(&map), &target),
            )
            .refusal()
            .unwrap_or_else(|| panic!("{label}: a hostile or oversized profile must be refused"));

        assert_eq!(
            refused.code(),
            expected_code,
            "{label}: the producer's own refusal code must reach the screen"
        );
        assert_eq!(
            refused.lines()[0].code(),
            expected_code,
            "{label}: the first line carries the refusal code verbatim"
        );
        assert_eq!(
            refused.lines()[0].to_string(),
            refused.refusal().to_string(),
            "{label}: the line carries the producer's message, not a paraphrase"
        );
        assert_eq!(
            refused.lines()[1].code(),
            "nothing_written",
            "{label}: the screen must be told nothing was written"
        );
        assert!(
            flow.view().is_none(),
            "{label}: a refused attempt has no report to read"
        );

        // The source is byte-identical, the same size and not re-stamped.
        let after = fs::metadata(&source_path).expect("the source still exists");
        assert_eq!(
            fs::read(&source_path).expect("the source reads"),
            source_bytes,
            "{label}: the refused import must not change the source bytes"
        );
        assert_eq!(
            after.len(),
            before.len(),
            "{label}: the refused import must not change the source size"
        );
        assert_eq!(
            after.modified().ok(),
            Some(modified),
            "{label}: the refused import must not rewrite the source at all"
        );

        // The new saves are untouched: no file added, removed or rewritten.
        assert_eq!(
            tree(&profiles_root),
            destination_before,
            "{label}: the refused import must not touch any new save"
        );

        // The explicit owner action is refused while a refusal is on screen —
        // with the refusal's own code — and it leaves everything exactly as
        // it was: the refusal still shown, nothing confirmed.
        assert_eq!(
            flow.confirm()
                .expect_err("a refused attempt has no report to confirm"),
            ConfirmError::Refused {
                code: expected_code
            },
            "{label}: the owner action names the refusal it ran into"
        );
        assert!(
            flow.refusal_view().is_some(),
            "{label}: a refused confirmation leaves the refusal on screen"
        );
        assert!(flow.confirmed().is_none(), "{label}: and confirms nothing");

        // Teardown leaves nothing of the failed attempt behind.
        flow.dismiss();
        assert!(flow.is_idle(), "{label}: dismiss returns the flow to idle");
        assert!(flow.outcome().is_none());
        assert!(flow.confirmed().is_none());
    }
}

/// **AC02** at the boundary the player meets: a legacy blueprint over the
/// stock weight ceiling is rejected with the specific fields, the report says
/// so, and the explicit owner action refuses the import rather than carrying
/// a blueprint the stock rules reject.
#[test]
fn accept_f64_c_rejected_blueprint_is_reported_with_breach_fields_and_cannot_be_confirmed() {
    let bytes = blueprint_document(&[full_record()]);
    let source = proposal(&bytes, LegacyArtifactClass::CustomAircraft)
        .expect("the fixture source is within the cap");
    let (layout, map) = blueprint_layout_and_map();
    let ids = id_map();
    let catalog = catalog(CatalogRows::Base);
    let rules = tight_mass_rules(5_000);
    let policy = cs_content::construction::synthetic_policy();
    let book = cs_content::construction::declared_synthetic_price_book();
    let context = context(&ids, &catalog, &rules, &policy, &book, true);
    let target = target();

    let mut flow = ImportFlow::new();
    let view = flow
        .offer(
            &context,
            &offer(&source, &bytes, Some(&layout), Some(&map), &target),
        )
        .view()
        .expect("the document reads and is described");

    // The plan-level verdict is full: every id slot resolved. The blueprint
    // verdict is a separate, more specific judgment.
    assert_eq!(view.verdict(), MigrationVerdict::Full);
    let blueprints = view.blueprints().expect("the offer declared a field map");
    assert_eq!(blueprints.rejected_count(), 1);
    assert_eq!(blueprints.conforming_count(), 0);

    let expected_breach = cs_content::construction::LimitBreach::Mass {
        limit: cs_content::construction::WeightUnits::new(5_000),
        total: cs_content::construction::WeightUnits::new(5_370),
    };
    assert!(
        view.lines()
            .iter()
            .any(|line| matches!(line, ReportLine::BlueprintRejected { index: 0 })),
        "the report must say record 0 was rejected: {:?}",
        view.lines()
    );
    assert!(
        view.lines().iter().any(|line| matches!(
            line,
            ReportLine::Breach { index: 0, breach } if *breach == expected_breach
        )),
        "the breach must be the exact limit/total pair, not a prose message: {:?}",
        view.lines()
    );

    // Nothing would be carried, so the owner action is refused — twice, by
    // both entry points, with the report left on screen to explain why.
    assert_eq!(view.importable_records(), &[] as &[u32]);
    assert_eq!(
        view.confirmable().expect_err("no record conforms"),
        ConfirmError::NoConformingRecord
    );
    assert_eq!(
        flow.confirm().expect_err("no record conforms"),
        ConfirmError::NoConformingRecord
    );
    assert!(flow.confirmed().is_none());
    assert!(
        flow.view().is_some(),
        "a refused confirmation leaves the report on screen"
    );
    assert!(
        flow.view()
            .expect("still there")
            .lines()
            .iter()
            .any(|line| matches!(line, ReportLine::Breach { .. })),
        "and the breach the owner must see is still rendered"
    );
}

/// **AC04**: the optional old-save migration can be disabled while new
/// campaigns still work.
///
/// With `cs.profile.legacy_save_import` off, an optional save class is
/// refused **by name** with the switch that did it — while a fresh profile is
/// still creatable, an idle flow still accepts a new campaign with no import
/// at all, and the required custom-aircraft class imports as usual.
#[test]
fn accept_f64_c_optional_save_import_can_be_disabled_while_new_profiles_still_work() {
    let bytes = blueprint_document(&[full_record()]);
    let save_source = proposal(&bytes, LegacyArtifactClass::CampaignSave)
        .expect("the fixture source is within the cap");
    let plane_source = proposal(&bytes, LegacyArtifactClass::CustomAircraft)
        .expect("the fixture source is within the cap");
    let (layout, map) = blueprint_layout_and_map();
    let ids = id_map();
    let catalog = catalog(CatalogRows::Base);
    let (rules, policy, book) = stock();
    // The switch is off for this dialog.
    let context = context(&ids, &catalog, &rules, &policy, &book, false);
    let target = target();

    let mut flow = ImportFlow::new();
    let refused = flow
        .offer(
            &context,
            &offer(&save_source, &bytes, Some(&layout), Some(&map), &target),
        )
        .refusal()
        .expect("an optional save class is refused while its switch is off");
    assert_eq!(refused.code(), "enhancement_disabled");
    assert!(
        refused
            .refusal()
            .to_string()
            .contains("cs.profile.legacy_save_import"),
        "the refusal names the switch that controls it: {}",
        refused.refusal()
    );
    assert!(
        matches!(
            refused.refusal(),
            FlowRefusal::Plan(ImportRefusal::EnhancementDisabled {
                switch: "cs.profile.legacy_save_import",
                ..
            })
        ),
        "and it keeps the producer's structured error whole"
    );

    // A new campaign needs no import: a fresh flow is idle, and confirming
    // nothing is an error rather than an empty success.
    let mut fresh = ImportFlow::new();
    assert!(fresh.is_idle());
    assert!(fresh.view().is_none());
    assert_eq!(
        fresh.confirm().expect_err("there is nothing to confirm"),
        ConfirmError::NothingToConfirm
    );
    let new_profile = TargetProfile::new(
        ProfileId::new(12).expect("the fixture profile id is not zero"),
        ProfileKind::Production,
        "New campaign",
    )
    .expect("a fresh profile is still nameable with the enhancement off");

    // And the required class is not behind the switch: the same dialog, with
    // the switch still off, imports a custom aircraft.
    let view = fresh
        .offer(
            &context,
            &offer(
                &plane_source,
                &bytes,
                Some(&layout),
                Some(&map),
                &new_profile,
            ),
        )
        .view()
        .expect("the required class is not an optional enhancement");
    assert_eq!(view.verdict(), MigrationVerdict::Full);
    assert_eq!(view.plan().target(), &new_profile);
    assert!(
        view.lines().iter().any(|line| matches!(
            line,
            ReportLine::Class {
                class: LegacyArtifactClass::CustomAircraft,
                ..
            }
        )),
        "the report says which class was imported"
    );
    assert!(
        view.lines().iter().any(|line| matches!(
            line,
            ReportLine::InventoryEvidence {
                class: LegacyArtifactClass::CustomAircraft,
                evidence: ClaimStatus::Unknown,
            }
        )),
        "and that its inventory row is still unmeasured, so a fixture report \
         never reads as a measured one"
    );
}

/// Teardown and retry: a refusal does not survive the attempt that produced
/// it, and the retry runs the whole pipeline again from a clean stage while
/// the attempt counter keeps the history straight.
#[test]
fn accept_f64_c_a_refused_attempt_is_torn_down_and_the_retry_runs_clean() {
    let good = blueprint_document(&[full_record()]);
    let mut hostile =
        vec![0u8; usize::try_from(MAX_LEGACY_SOURCE_BYTES).expect("the cap fits") + 1];
    hostile[0..8].copy_from_slice(&synthetic_blueprint_layout().magic());
    let (layout, map) = blueprint_layout_and_map();
    let ids = id_map();
    let catalog = catalog(CatalogRows::Base);
    let (rules, policy, book) = stock();
    let context = context(&ids, &catalog, &rules, &policy, &book, true);
    let target = target();

    let hostile_source = ArtifactProposal::new(
        support::FIXTURE_SPELLING,
        MAX_LEGACY_SOURCE_BYTES,
        sha256(&hostile),
        Some(LegacyArtifactClass::CustomAircraft),
    )
    .expect("the declared size is exactly the cap");
    let good_source = proposal(&good, LegacyArtifactClass::CustomAircraft)
        .expect("the fixture source is within the cap");

    let mut flow = ImportFlow::new();
    let refused = flow
        .offer(
            &context,
            &offer(
                &hostile_source,
                &hostile,
                Some(&layout),
                Some(&map),
                &target,
            ),
        )
        .refusal()
        .expect("the oversized source is refused");
    assert_eq!(refused.attempt(), 1);
    assert_eq!(refused.code(), "source_too_large");
    assert!(
        refused.lines()[0].to_string().contains("byte cap"),
        "the propagated message is the producer's: {}",
        refused.lines()[0]
    );

    // The retry: same flow, clean stage, a good source.
    let view = flow
        .retry(
            &context,
            &offer(&good_source, &good, Some(&layout), Some(&map), &target),
        )
        .view()
        .expect("the good source reports")
        .clone();
    assert_eq!(
        view.attempt(),
        2,
        "each attempt is counted, including failures"
    );
    assert!(
        flow.refusal_view().is_none(),
        "the refusal does not survive the retry"
    );
    assert!(
        !view
            .lines()
            .iter()
            .any(|line| matches!(line, ReportLine::Refusal { .. })),
        "the retried report carries no line from the failed attempt: {:?}",
        view.lines()
    );
    assert_eq!(
        view.source().spelling(),
        support::FIXTURE_SPELLING,
        "the report describes the source the retry was given"
    );
    assert_eq!(
        view.source().sha256(),
        &sha256(&good),
        "and its fingerprint is the verified digest of those bytes"
    );

    // Teardown: the report is gone, the attempt history is not.
    flow.dismiss();
    assert!(flow.is_idle());
    assert!(flow.outcome().is_none());
    assert!(flow.view().is_none());
    assert!(flow.refusal_view().is_none());
    assert!(flow.confirmed().is_none());
    assert_eq!(flow.attempt(), 2, "teardown keeps the attempt counter");
}

/// The explicit owner action produces the outcome transaction: a confirmed
/// import that retains the verified source fingerprint, the rendered report
/// and exactly the records a persistence layer may carry.
#[test]
fn accept_f64_c_confirm_retains_the_fingerprint_and_the_importable_records() {
    let bytes = blueprint_document(&[full_record(), second_record()]);
    let source = proposal(&bytes, LegacyArtifactClass::CustomAircraft)
        .expect("the fixture source is within the cap");
    let (layout, map) = blueprint_layout_and_map();
    let ids = id_map();
    let catalog = catalog(CatalogRows::Base);
    let (rules, policy, book) = stock();
    let context = context(&ids, &catalog, &rules, &policy, &book, true);
    let target = target();

    let mut flow = ImportFlow::new();
    let lines_before = flow
        .offer(
            &context,
            &offer(&source, &bytes, Some(&layout), Some(&map), &target),
        )
        .view()
        .expect("the fixture document reports")
        .lines()
        .to_vec();

    let confirmed = flow
        .confirm()
        .expect("an owner who has read the report may confirm it");
    assert_eq!(confirmed.attempt(), 1);
    assert_eq!(
        confirmed.target(),
        &target,
        "the import lands in the new profile"
    );
    assert_eq!(
        confirmed.source().sha256(),
        &sha256(&bytes),
        "the retained fingerprint is the verified digest of the source"
    );
    assert_eq!(confirmed.source().spelling(), support::FIXTURE_SPELLING);
    assert_eq!(
        confirmed.lines(),
        lines_before,
        "the confirmed import retains exactly what the owner was shown"
    );
    assert!(
        confirmed.fixture_admitted(),
        "a fixture-produced report must carry its fixture marker into the import"
    );
    assert_eq!(
        confirmed.importable_records(),
        &[0, 1],
        "both records resolved and both conform, so both may be carried"
    );
    assert_eq!(
        confirmed.report().class(),
        &ImportClass::Full,
        "the migration report itself is retained for the profile it lands in"
    );
    let blueprints = confirmed
        .blueprints()
        .expect("the offer declared a field map");
    assert_eq!(blueprints.conforming_count(), 2);

    // Confirming is one action: the report is consumed into it, and asking
    // again is not a second import.
    assert!(flow.view().is_none());
    assert!(flow.outcome().is_none());
    assert!(flow.confirmed().is_some());
    assert_eq!(
        flow.confirm().expect_err("the action was already taken"),
        ConfirmError::NothingToConfirm
    );
}

/// A second record whose ordnance slot names an id the table does not bind:
/// the plan reports it as unresolved, so the import is **partial** and the
/// report names the row instead of silently carrying the other record.
#[test]
fn accept_f64_c_a_partial_import_names_what_it_could_not_carry() {
    let mut second = full_record();
    second.rockets = [5, 9]; // 9 is not bound by the id table.
    let bytes = blueprint_document(&[full_record(), second]);
    let source = proposal(&bytes, LegacyArtifactClass::CustomAircraft)
        .expect("the fixture source is within the cap");
    let (layout, map) = blueprint_layout_and_map();
    let ids = id_map();
    let catalog = catalog(CatalogRows::Base);
    let (rules, policy, book) = stock();
    let context = context(&ids, &catalog, &rules, &policy, &book, true);
    let target = target();

    let mut flow = ImportFlow::new();
    let view = flow
        .offer(
            &context,
            &offer(&source, &bytes, Some(&layout), Some(&map), &target),
        )
        .view()
        .expect("the document reads");

    assert_eq!(view.verdict(), MigrationVerdict::Partial);
    let ImportClass::Partial {
        resolved,
        unresolved,
    } = view.report().class()
    else {
        panic!("a document with one unresolved record is a partial import");
    };
    assert_eq!(resolved, &[0], "the conforming record is carried");
    assert_eq!(unresolved.len(), 1, "the unbound ordnance id is named");
    assert_eq!(unresolved[0].reason.code(), "id_not_mapped");
    assert_eq!(unresolved[0].record_index, Some(1));
    assert!(
        view.lines().iter().any(|line| matches!(
            line,
            ReportLine::Unresolved {
                code: "id_not_mapped",
                ..
            }
        )),
        "the screen shows the unresolved row by code: {:?}",
        view.lines()
    );
    // The one record that may be carried is exactly the one that resolved.
    assert_eq!(view.importable_records(), &[0]);
    flow.confirm()
        .expect("a partial import with one conforming record may be confirmed");
    assert_eq!(
        flow.confirmed()
            .expect("just confirmed")
            .importable_records(),
        &[0]
    );
}

/// An empty identity table: the plan carries nothing, so the report says
/// `unsupported` and the owner action refuses — never a blank profile called
/// imported (spec F64 non-negotiable 5).
#[test]
fn accept_f64_c_a_document_resolving_nothing_is_never_confirmed_as_a_blank_profile() {
    let bytes = blueprint_document(&[full_record()]);
    let source = proposal(&bytes, LegacyArtifactClass::CustomAircraft)
        .expect("the fixture source is within the cap");
    let (layout, _map) = blueprint_layout_and_map();
    let ids = LegacyIdMap::new();
    let catalog = catalog(CatalogRows::Base);
    let (rules, policy, book) = stock();
    let context = context(&ids, &catalog, &rules, &policy, &book, true);
    let target = target();

    let mut flow = ImportFlow::new();
    let view = flow
        .offer(
            &context,
            // No field map: this is about what the *plan* would carry.
            &offer(&source, &bytes, Some(&layout), None, &target),
        )
        .view()
        .expect("the document reads; it just resolves nothing")
        .clone();
    assert_eq!(view.verdict(), MigrationVerdict::Unsupported);
    assert!(view.importable_records().is_empty());
    let err = view.confirmable().expect_err("nothing would be carried");
    assert!(
        matches!(&err, ConfirmError::Unsupported { reason } if reason.code() == "id_not_mapped"),
        "the refusal names the producer's own unresolved reason: {err:?}"
    );
    assert_eq!(flow.confirm().unwrap_err(), err);
    assert!(flow.confirmed().is_none());
    assert!(
        view.lines().iter().any(|line| matches!(
            line,
            ReportLine::Verdict {
                verdict: MigrationVerdict::Unsupported
            }
        )),
        "the screen says unsupported rather than full"
    );
    assert!(
        view.lines().iter().any(|line| matches!(
            line,
            ReportLine::Unresolved {
                code: "id_not_mapped",
                ..
            }
        )),
        "and the screen shows *why* nothing could be carried, by the \
         producer's own reason code: {:?}",
        view.lines()
    );
}

/// A layout that declares no id slot: every record reads cleanly and resolves
/// with **nothing to carry**, so the plan is `unsupported` even though its
/// record list is full. The report must name the reason, offer no record to
/// persist and refuse the owner action — the three ways a screen could
/// otherwise present this document as an import of a blank profile (spec F64
/// non-negotiable 5).
#[test]
fn accept_f64_c_an_unsupported_document_names_its_reason_and_offers_no_record() {
    let bytes = profile_document(&[(7, 1, "phoenix"), (7, 2, "kestrel")]);
    let source = proposal(&bytes, LegacyArtifactClass::CustomAircraft)
        .expect("the fixture source is within the cap");
    let layout = no_id_layout();
    let ids = id_map();
    let catalog = catalog(CatalogRows::Base);
    let (rules, policy, book) = stock();
    let context = context(&ids, &catalog, &rules, &policy, &book, true);
    let target = target();

    let mut flow = ImportFlow::new();
    let view = flow
        .offer(
            &context,
            // No blueprint roles: this layout declares no id slot at all.
            &offer(&source, &bytes, Some(&layout), None, &target),
        )
        .view()
        .expect("the document reads; it just carries nothing");

    assert_eq!(view.verdict(), MigrationVerdict::Unsupported);
    assert_eq!(
        view.report().records().len(),
        2,
        "both records read cleanly — the verdict is about what they carry, \
         not whether they parsed"
    );

    // The verdict and the record list must not contradict each other: an
    // unsupported plan carries nothing, so nothing may be offered for it.
    assert!(
        view.importable_records().is_empty(),
        "an unsupported report offers no record to persist: {:?}",
        view.importable_records()
    );

    // The reason the verdict rests on reaches the screen with its own code.
    assert!(
        view.lines().iter().any(|line| matches!(
            line,
            ReportLine::Unresolved {
                code: "no_resolvable_identity",
                detail
            } if detail.contains("2 declared")
        )),
        "the screen says why nothing would be carried: {:?}",
        view.lines()
    );

    // Every record line reads as a sentence: a record that carries nothing
    // must say so rather than ending on "carries".
    for line in view.lines() {
        let text = line.to_string();
        assert!(
            !text.ends_with("carries "),
            "the report line must not end mid-sentence: {text:?}"
        );
    }

    // And the owner action refuses it, with the producer's reason.
    let error = flow
        .confirm()
        .expect_err("a plan that carries nothing cannot be confirmed");
    assert!(
        matches!(
            &error,
            ConfirmError::Unsupported { reason }
                if matches!(
                    reason,
                    UnresolvedReason::NoResolvableIdentity { records: 2 }
                )
        ),
        "the refusal carries the producer's own reason: {error:?}"
    );
    assert!(flow.confirmed().is_none());
    assert!(
        flow.view().is_some(),
        "the refused confirmation leaves the report on screen"
    );
}

/// Error propagation from the blueprint stage: a field map that cannot
/// describe the layout is refused **after** the plan succeeded, and the
/// producer's own code reaches the screen instead of a generic failure.
#[test]
fn accept_f64_c_a_field_map_that_cannot_describe_the_layout_propagates_its_code() {
    let bytes = profile_document(&[(7, 1, "phoenix")]);
    let source = proposal(&bytes, LegacyArtifactClass::CustomAircraft)
        .expect("the fixture source is within the cap");
    let map = cs_content::legacy_import::synthetic_blueprint_map();
    let ids = id_map();
    let catalog = catalog(CatalogRows::Base);
    let (rules, policy, book) = stock();
    let context = context(&ids, &catalog, &rules, &policy, &book, true);
    let target = target();

    let mut flow = ImportFlow::new();
    let refused = flow
        .offer(
            &context,
            // The profile fixture layout reads this document; the blueprint
            // field map is for the *other* fixture layout, so it cannot
            // describe these records.
            &offer(
                &source,
                &bytes,
                Some(&synthetic_layout()),
                Some(&map),
                &target,
            ),
        )
        .refusal()
        .expect("a field map for another layout must refuse, not half-apply");
    assert_eq!(refused.code(), "map_invalid");
    assert!(
        matches!(
            refused.refusal(),
            FlowRefusal::Blueprint(cs_content::legacy_import::BlueprintImportRefusal::Map(_))
        ),
        "the whole blueprint-stage refusal is retained: {:?}",
        refused.refusal()
    );
    assert!(flow.view().is_none());
}

/// The production admission refuses fixture data by its own code: nothing the
/// player could reach through the default policy is a designed layout.
#[test]
fn accept_f64_c_production_admission_refuses_the_fixture_layout_by_code() {
    let bytes = blueprint_document(&[full_record()]);
    let source = proposal(&bytes, LegacyArtifactClass::CustomAircraft)
        .expect("the fixture source is within the cap");
    let (layout, map) = blueprint_layout_and_map();
    let ids = id_map();
    let catalog = catalog(CatalogRows::Base);
    let (rules, policy, book) = stock();
    // The production default: measured evidence only.
    let context = ImportContext {
        ids: &ids,
        catalog: &catalog,
        rules: &rules,
        policy: &policy,
        book: &book,
        admission: LayoutAdmission::MeasuredOnly,
        legacy_save_import_enabled: true,
        origin: cs_types::content::Origin::SyntheticFixture,
        provenance: designed("f64c.test.blueprint"),
    };
    let target = target();

    let mut flow = ImportFlow::new();
    let refused = flow
        .offer(
            &context,
            &offer(&source, &bytes, Some(&layout), Some(&map), &target),
        )
        .refusal()
        .expect("a designed layout is refused by the measured-only admission");
    assert_eq!(refused.code(), "layout_evidence");
    assert!(
        matches!(
            refused.refusal(),
            FlowRefusal::Plan(ImportRefusal::LayoutEvidence {
                evidence: ClaimStatus::Designed,
                ..
            })
        ),
        "the producer's structured refusal is kept: {:?}",
        refused.refusal()
    );

    // And with no layout to be found at all — the honest state of this build,
    // whose inventory rows are all Unknown — the offer is declined by name
    // before a byte is judged.
    let none_flow = flow.offer(
        &context,
        &cs_app::ui::import::ImportOffer {
            source: &source,
            bytes: &bytes,
            layout: None,
            limits: LegacyLimits::designed(),
            field_map: None,
            target: &target,
            install_identity: None,
        },
    );
    let refused = none_flow.refusal().expect("no layout, no import");
    assert_eq!(refused.code(), "no_measured_layout");
    assert_eq!(
        refused.refusal(),
        &FlowRefusal::NoMeasuredLayout {
            class: LegacyArtifactClass::CustomAircraft,
            inventory_evidence: ClaimStatus::Unknown,
        },
        "the refusal names the class and what its inventory row still lacks"
    );
    assert!(
        refused.lines()[0]
            .to_string()
            .contains("legacy.custom_aircraft"),
        "the screen says which class has no measured layout: {}",
        refused.lines()[0]
    );
}

/// A second conforming record, so the confirm path has more than one record
/// to reason about.
fn second_record() -> BlueprintRecord {
    BlueprintRecord {
        name: "kestrel",
        airframe: 7,
        engine: 8,
        guns: [1, 1, 1, 1],
        rockets: [5, 5],
    }
}

/// **Retail**: every file this installation ships is offered to the
/// production consumer and refused by name, with nothing written anywhere.
///
/// Needs `$CS_GAME_DIR` (capability `retail`); the test fails loudly when it
/// is unset. What it measures is the honest state F64-B established and F64-C
/// must not paper over: the install ships no legacy artifact, and this build
/// has measured no byte layout for any class, so *no* original file can reach
/// a migration report — each offer is declined with `no_measured_layout`
/// before a byte is judged, and the source bytes and a probe destination are
/// unchanged afterwards.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f64_c_retail_every_shipped_file_is_refused_by_name_and_nothing_is_written() {
    let root = std::path::PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must point at the original installation"),
    );
    let found =
        cs_assets::install::discover(&root).expect("production discovery reads the installation");
    let identity = found.manifest.logical_identity();

    let base = TempBase::new("retail");
    let destination = base.path().join("userdata/production");
    fs::create_dir_all(&destination).expect("the destination root is created");
    let probe = destination.join("existing.save");
    fs::write(&probe, b"fresh-engine-save").expect("the probe save is written");
    let destination_before = tree(&destination);

    let (layout, map) = blueprint_layout_and_map();
    let ids = id_map();
    let catalog = catalog(CatalogRows::Base);
    let (rules, policy, book) = stock();
    // The production default: measured evidence only, switch on.
    let context = ImportContext {
        ids: &ids,
        catalog: &catalog,
        rules: &rules,
        policy: &policy,
        book: &book,
        admission: LayoutAdmission::MeasuredOnly,
        legacy_save_import_enabled: true,
        origin: cs_types::content::Origin::SyntheticFixture,
        provenance: designed("f64c.test.blueprint"),
    };
    let target = target();
    let mut flow = ImportFlow::new();

    let mut offered = 0usize;
    let mut over_cap = 0usize;
    for file in found.manifest.files.iter() {
        let size = file.size_bytes;
        if size > MAX_LEGACY_SOURCE_BYTES {
            // The proposal constructor refuses these before any consumer sees
            // them: the import cap is a designed bound, and a bigger file is
            // not a candidate at all.
            over_cap += 1;
            assert!(
                ArtifactProposal::new(
                    file.relative_spelling.as_str(),
                    size,
                    file.sha256,
                    Some(LegacyArtifactClass::CustomAircraft),
                )
                .is_err(),
                "{} is over the import cap and must not be proposable",
                file.relative_spelling
            );
            continue;
        }
        let bytes = fs::read(root.join(file.relative_spelling.as_str()))
            .unwrap_or_else(|error| panic!("{} must be readable: {error}", file.relative_spelling));
        let source = ArtifactProposal::new(
            file.relative_spelling.as_str(),
            bytes.len() as u64,
            sha256(&bytes),
            Some(LegacyArtifactClass::CustomAircraft),
        )
        .unwrap_or_else(|error| panic!("{} must be proposable: {error}", file.relative_spelling));

        // No layout: this build has measured none for any class.
        let refused = flow
            .offer(
                &context,
                &cs_app::ui::import::ImportOffer {
                    source: &source,
                    bytes: &bytes,
                    layout: None,
                    limits: LegacyLimits::designed(),
                    field_map: None,
                    target: &target,
                    install_identity: Some(identity.clone()),
                },
            )
            .refusal()
            .unwrap_or_else(|| {
                panic!(
                    "{} must not reach a migration report: no byte layout for \
                     this class has been measured",
                    file.relative_spelling
                )
            });
        assert_eq!(
            refused.code(),
            "no_measured_layout",
            "{}: {}",
            file.relative_spelling,
            refused.refusal()
        );
        assert_eq!(
            fs::read(root.join(file.relative_spelling.as_str())).expect("re-read"),
            bytes,
            "{}: the offer must not change the installation's bytes",
            file.relative_spelling
        );
        offered += 1;
    }

    assert!(
        offered > 0,
        "the installation must offer at least one file within the import cap"
    );
    assert_eq!(
        offered + over_cap,
        found.manifest.files.len(),
        "every inventoried file is either offered within the cap or over it"
    );
    assert_eq!(
        flow.attempt() as usize,
        offered,
        "every offer is one attempt of the same dialog"
    );
    assert_eq!(
        tree(&destination),
        destination_before,
        "no offer may write into the profile destination"
    );

    // Even with a layout in hand, the production admission refuses fixture
    // evidence: an original file never reaches a report through the default
    // policy either.
    let first = found
        .manifest
        .files
        .iter()
        .find(|file| file.size_bytes <= MAX_LEGACY_SOURCE_BYTES)
        .expect("at least one file is within the cap");
    let bytes = fs::read(root.join(first.relative_spelling.as_str())).expect("the file reads");
    let source = ArtifactProposal::new(
        first.relative_spelling.as_str(),
        bytes.len() as u64,
        sha256(&bytes),
        Some(LegacyArtifactClass::CustomAircraft),
    )
    .expect("the file is proposable");
    let refused = flow
        .offer(
            &context,
            &cs_app::ui::import::ImportOffer {
                source: &source,
                bytes: &bytes,
                layout: Some(&layout),
                limits: LegacyLimits::designed(),
                field_map: Some(&map),
                target: &target,
                install_identity: Some(identity.clone()),
            },
        )
        .refusal()
        .expect("the fixture layout is refused by the measured-only admission");
    assert_eq!(refused.code(), "layout_evidence");
    assert!(
        !destination.join("imported.save").exists(),
        "no import landed anywhere"
    );
}
