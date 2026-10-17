//! Acceptance tests for the front-end's legacy-import dialog (task #757,
//! `specs/F64-legacy-custom-aircraft-and-optional-save-import.md` `### F64-C`,
//! presented through `specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`
//! `### F45-A`).
//!
//! F64-C built **the boundary** ([`cs_app::ui::import`]) that produces the
//! migration report; nothing drew it. These tests drive **the screen** that
//! does — [`cs_app::ui::front_end::ImportDialog`] on
//! [`cs_app::ui::front_end::Screen::ImportDialog`] — through the production
//! state machine, so what is proven is what a player's button does:
//!
//! * a refusal reaches the screen with the producer's **own code**, and the
//!   report the refusal replaced is gone;
//! * a report reaches the screen with its verdict, its record lines and — when
//!   the producer marked it so — its fixture marker, and the screen draws the
//!   producer's own [`ReportLine`]s rather than a second rendering of them;
//! * `ConfirmImport` asks the domain for
//!   [`cs_app::ui::front_end::Request::ConfirmImport`] **only** when
//!   `MigrationView::confirmable()` is `Ok`, and is refused by the producer's
//!   own reason otherwise, with the report left on screen;
//! * `Cancel` is the teardown: the attempt, its lines and its confirmation are
//!   gone afterwards;
//! * `Retry` re-runs the whole production pipeline from a clean stage;
//! * no path through the screen writes anything: the source bytes and a
//!   destination tree are byte-for-byte unchanged after every action,
//!   including a confirmed one.
//!
//! Everything here is designed fixture content read through the production
//! `read_legacy_profile`, resolved by the production `LegacyIdMap`/`Catalog`
//! pair and judged by the production `ConstructionRules::validate`, under the
//! fixture admission the tests name explicitly. Nothing is `verified_original`,
//! and the fixture source is the same one F64-C's suite offers
//! (`tests/f64_c_support/`), so the two suites describe one source rather than
//! two.

use crate::f64_c_support as support;

use std::fs;

use cs_app::campaign::lower_campaign;
use cs_app::ui::front_end::{
    Action, Effect, FlowError, FlowSetup, FrontEnd, FrontEndFlow, ImportDialog, Refusal, Request,
    Screen, ScreenDeck, ScreenSession,
};
use cs_app::ui::import::{ConfirmError, ImportFlow, ImportOffer, MigrationVerdict, ReportLine};
use cs_content::campaign::declared_synthetic_campaign;
use cs_content::construction::ConstructionRules;
use cs_formats::legacy_profile::LegacyArtifactClass;
use cs_sim::campaign::{CampaignRunId, DifficultyId};
use cs_types::content::ContentKind;
use cs_types::evidence::ClaimStatus;
use support::{
    BlueprintRecord, CatalogRows, TempBase, blueprint_document, blueprint_layout_and_map, catalog,
    cid, context, full_record, id_map, offer, proposal, stock, target, tight_mass_rules, tree,
};

/// A front end standing on the import dialog screen, the way a player reaches
/// it: install verified, a new profile asked for, the dialog opened.
fn front_end_on_the_import_dialog() -> FrontEnd {
    let mut front_end = FrontEnd::new();
    front_end
        .apply(Action::InstallVerified)
        .expect("the installation verifies");
    front_end
        .apply(Action::NewProfile)
        .expect("a new profile is asked for");
    front_end
        .apply(Action::OpenImportDialog)
        .expect("the profile screen offers the import dialog");
    assert_eq!(front_end.screen(), Screen::ImportDialog);
    front_end
}

/// The dialog a screen is opened on: `bytes` offered through the production
/// blueprint layout and field map, judged by `rules`.
fn opened_dialog(bytes: &[u8], rules: &ConstructionRules) -> ImportDialog {
    let (layout, map) = blueprint_layout_and_map();
    let ids = id_map();
    let catalog = catalog(CatalogRows::Base);
    let policy = cs_content::construction::synthetic_policy();
    let book = cs_content::construction::declared_synthetic_price_book();
    let context = context(&ids, &catalog, rules, &policy, &book, true);
    let source = proposal(bytes, LegacyArtifactClass::CustomAircraft)
        .expect("the fixture source is within the import cap");
    let target = target();
    ImportDialog::open(
        &context,
        &offer(&source, bytes, Some(&layout), Some(&map), &target),
    )
}

/// The requests an outcome asked the domain for.
fn requests(effects: &[Effect]) -> Vec<&Request> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Request(request) => Some(request),
            _ => None,
        })
        .collect()
}

/// The identities a report's record lines carry, in order.
fn record_identities(
    lines: &[ReportLine],
) -> Vec<(
    cs_formats::legacy_profile::LegacyIdClass,
    cs_types::content::ContentId,
)> {
    let mut found = Vec::new();
    for line in lines {
        if let ReportLine::Record { identities, .. } = line {
            found.extend(identities.iter().cloned());
        }
    }
    found
}

/// A refusal reaches the screen with the producer's own code, the
/// nothing-was-written notice, and no verdict at all — and the explicit owner
/// action is refused by that same code rather than confirming a report that
/// does not exist.
#[test]
fn accept_f64_c_import_dialog_a_refusal_renders_the_producers_own_code() {
    // A hostile table the production reader must refuse rather than
    // allocate: the declared record count at the offset the fixture layout
    // declares it at.
    let mut hostile = blueprint_document(&[full_record()]);
    hostile[16..20].copy_from_slice(&u32::MAX.to_le_bytes());

    let mut front_end = front_end_on_the_import_dialog();
    front_end
        .open_import_dialog(opened_dialog(&hostile, &support::stock().0))
        .expect("the dialog screen takes the offered source");
    let dialog = front_end.import_dialog().expect("the dialog is on screen");

    assert_eq!(dialog.refusal_code(), Some("unreadable"));
    assert!(
        dialog.verdict().is_none(),
        "a refused attempt has no verdict, never a verdict of `refused`"
    );
    assert_eq!(
        dialog.lines()[0].code(),
        "unreadable",
        "the first row carries the producer's code: {:?}",
        dialog.lines()
    );
    assert_eq!(
        dialog.lines()[1].code(),
        "nothing_written",
        "the screen is told nothing was written: {:?}",
        dialog.lines()
    );
    assert!(
        !dialog.fixture_admitted(),
        "a refusal is not a fixture-admitted report"
    );

    // The explicit owner action is refused with the refusal's own code, the
    // refusal stays on screen and nothing is confirmed or asked for.
    let error = front_end
        .apply(Action::ConfirmImport)
        .expect_err("a refused attempt has no report to confirm");
    assert_eq!(
        error,
        Refusal::Import(ConfirmError::Refused { code: "unreadable" }),
        "the owner action names the refusal it ran into"
    );
    let dialog = front_end
        .import_dialog()
        .expect("the refused confirmation leaves the dialog on screen");
    assert_eq!(dialog.refusal_code(), Some("unreadable"));
    assert!(dialog.confirmed().is_none(), "and confirms nothing");
}

/// A report reaches the screen with its verdict and its record lines, marked
/// as fixture data by the producer's own notice, and the identities the record
/// lines carry are the ones the declared id table bound — never whatever sits
/// at that position in a catalog.
#[test]
fn accept_f64_c_import_dialog_a_report_renders_its_verdict_records_and_fixture_marker() {
    let bytes = blueprint_document(&[full_record()]);
    let (rules, _, _) = stock();
    let mut front_end = front_end_on_the_import_dialog();
    front_end
        .open_import_dialog(opened_dialog(&bytes, &rules))
        .expect("the dialog screen takes the offered source");
    let dialog = front_end.import_dialog().expect("the dialog is on screen");

    assert_eq!(dialog.verdict(), Some(MigrationVerdict::Full));
    assert_eq!(dialog.refusal_code(), None);
    assert!(
        dialog.fixture_admitted(),
        "the designed fixture layout can only be read through the fixture admission"
    );

    // The verdict the screen draws, and the fixture marker, are the producer's
    // own rows.
    assert!(
        dialog.lines().iter().any(|line| matches!(
            line,
            ReportLine::Verdict {
                verdict: MigrationVerdict::Full
            }
        )),
        "the screen draws the verdict row: {:?}",
        dialog.lines()
    );
    assert!(
        dialog.lines().iter().any(|line| matches!(
            line,
            ReportLine::Notice {
                code: "fixture_admitted",
                ..
            }
        )),
        "a fixture-admitted report must carry its marker to the screen: {:?}",
        dialog.lines()
    );
    assert!(
        dialog.lines().iter().any(|line| matches!(
            line,
            ReportLine::InventoryEvidence {
                class: LegacyArtifactClass::CustomAircraft,
                evidence: ClaimStatus::Unknown,
            }
        )),
        "and the inventory row it was read under, unmeasured: {:?}",
        dialog.lines()
    );

    // The identities are the declared bindings, in layout declaration order:
    // the gun is legacy weapon id 1 and the missile is ordnance id 5 in
    // *every* catalog order.
    let gun = cid(
        ContentKind::Weapon,
        cs_content::construction::SYNTHETIC_GUN_KEY,
    );
    let missile = cid(
        ContentKind::Weapon,
        cs_content::construction::SYNTHETIC_MISSILE_KEY,
    );
    assert_eq!(
        record_identities(dialog.lines()),
        vec![
            (
                cs_formats::legacy_profile::LegacyIdClass::Airframe,
                cid(
                    ContentKind::Airframe,
                    cs_content::construction::SYNTHETIC_AIRFRAME_KEY
                )
            ),
            (
                cs_formats::legacy_profile::LegacyIdClass::Engine,
                cid(
                    ContentKind::Engine,
                    cs_content::construction::SYNTHETIC_ENGINE_KEY
                )
            ),
            (
                cs_formats::legacy_profile::LegacyIdClass::Weapon,
                gun.clone()
            ),
            (
                cs_formats::legacy_profile::LegacyIdClass::Weapon,
                gun.clone()
            ),
            (
                cs_formats::legacy_profile::LegacyIdClass::Weapon,
                gun.clone()
            ),
            (cs_formats::legacy_profile::LegacyIdClass::Weapon, gun),
            (
                cs_formats::legacy_profile::LegacyIdClass::Ordnance,
                missile.clone()
            ),
            (cs_formats::legacy_profile::LegacyIdClass::Ordnance, missile),
        ],
        "the record rows carry the identities the table declared"
    );

    // The screen does not re-derive a line: the rows it draws are exactly the
    // rows F64-C's own flow renders for the same offer.
    let (layout, map) = blueprint_layout_and_map();
    let ids = id_map();
    let catalog = catalog(CatalogRows::Base);
    let (rules, policy, book) = stock();
    let context = context(&ids, &catalog, &rules, &policy, &book, true);
    let source = proposal(&bytes, LegacyArtifactClass::CustomAircraft).expect("proposable");
    let target = target();
    let mut flow = ImportFlow::new();
    let producer_lines = flow
        .offer(
            &context,
            &offer(&source, &bytes, Some(&layout), Some(&map), &target),
        )
        .view()
        .expect("the producer renders a report")
        .lines()
        .to_vec();
    assert_eq!(
        dialog.lines(),
        producer_lines.as_slice(),
        "the screen draws the producer's rows, not a second rendering of them"
    );
}

/// Confirming asks the domain for the one transaction this screen has — and
/// only when the report says it may: a report whose every blueprint the stock
/// rules reject is refused by the producer's own reason, with the report left
/// on screen.
#[test]
fn accept_f64_c_import_dialog_confirm_asks_the_domain_only_when_the_report_is_confirmable() {
    let bytes = blueprint_document(&[full_record()]);
    let (rules, _, _) = stock();

    // The confirmable case: the request is emitted, and the confirmed value
    // with the verified source fingerprint stays in the dialog for whoever
    // runs the transaction.
    let mut front_end = front_end_on_the_import_dialog();
    front_end
        .open_import_dialog(opened_dialog(&bytes, &rules))
        .expect("the dialog screen takes the offered source");
    assert!(
        front_end
            .import_dialog()
            .expect("on screen")
            .confirmable()
            .is_ok(),
        "the fixture report is confirmable"
    );
    let outcome = front_end
        .apply(Action::ConfirmImport)
        .expect("a confirmable report may be confirmed");
    assert_eq!(
        requests(&outcome.effects),
        vec![&Request::ConfirmImport],
        "confirming asks the domain for the import transaction"
    );
    let confirmed = front_end
        .import_dialog()
        .expect("the dialog keeps the confirmed value")
        .confirmed()
        .expect("the confirmed import is retained");
    assert_eq!(
        confirmed.source().sha256(),
        &cs_assets::install::sha256(&bytes),
        "the retained fingerprint is the verified digest of the source"
    );
    assert_eq!(
        confirmed.importable_records(),
        &[0],
        "exactly the record that resolved and conforms may be carried"
    );
    assert!(
        confirmed.fixture_admitted(),
        "the fixture marker travels with the confirmed import"
    );

    // The reject-only case: the same screen, a rule profile the single
    // blueprint is over, so nothing may be carried.
    let mut front_end = front_end_on_the_import_dialog();
    front_end
        .open_import_dialog(opened_dialog(&bytes, &tight_mass_rules(5_000)))
        .expect("the dialog screen takes the offered source");
    let dialog = front_end.import_dialog().expect("on screen");
    assert_eq!(
        dialog.confirmable().expect_err("no record conforms"),
        ConfirmError::NoConformingRecord
    );
    assert!(
        dialog
            .lines()
            .iter()
            .any(|line| matches!(line, ReportLine::BlueprintRejected { index: 0 })),
        "the report says which record was rejected: {:?}",
        dialog.lines()
    );
    let error = front_end
        .apply(Action::ConfirmImport)
        .expect_err("a report that would carry nothing cannot be confirmed");
    assert_eq!(error, Refusal::Import(ConfirmError::NoConformingRecord));
    let dialog = front_end
        .import_dialog()
        .expect("the refused confirmation leaves the report on screen");
    assert!(dialog.confirmed().is_none(), "and confirms nothing");
    assert!(
        dialog
            .lines()
            .iter()
            .any(|line| matches!(line, ReportLine::BlueprintRejected { index: 0 })),
        "with the refusal's own record still drawn"
    );

    // With no source offered at all there is nothing to confirm, and the
    // refusal says exactly that rather than inventing a report.
    let mut bare = front_end_on_the_import_dialog();
    assert_eq!(
        bare.apply(Action::ConfirmImport)
            .expect_err("no source was offered"),
        Refusal::NoImportAttempt
    );
    assert!(bare.import_dialog().is_none());
}

/// Cancel is the teardown: the attempt, its rows and its confirmation are gone
/// — from the screen and from the dialog — and the only way out of the screen
/// asks the domain for nothing.
#[test]
fn accept_f64_c_import_dialog_cancel_tears_the_attempt_down_and_asks_for_nothing() {
    let bytes = blueprint_document(&[full_record()]);
    let (rules, _, _) = stock();
    let mut front_end = front_end_on_the_import_dialog();
    front_end
        .open_import_dialog(opened_dialog(&bytes, &rules))
        .expect("the dialog screen takes the offered source");
    let attempt = front_end.import_dialog().expect("on screen").attempt();
    assert!(
        !front_end
            .import_dialog()
            .expect("on screen")
            .lines()
            .is_empty()
    );

    let outcome = front_end
        .apply(Action::Cancel)
        .expect("Cancel leaves the dialog");
    assert_eq!(front_end.screen(), Screen::ProfileSelect);
    assert!(
        requests(&outcome.effects).is_empty(),
        "cancelling asks the domain for nothing: {:?}",
        outcome.effects
    );
    assert!(
        front_end.import_dialog().is_none(),
        "the dismissed attempt does not survive on the profile screen"
    );

    // The dialog's own teardown keeps the attempt *history* and drops
    // everything the attempt produced.
    let mut dialog = opened_dialog(&bytes, &rules);
    assert_eq!(dialog.attempt(), attempt);
    dialog.cancel();
    assert!(dialog.is_idle());
    assert!(dialog.lines().is_empty());
    assert!(dialog.verdict().is_none());
    assert!(dialog.refusal_code().is_none());
    assert!(!dialog.fixture_admitted());
    assert!(dialog.confirmed().is_none());
    assert_eq!(
        dialog.attempt(),
        attempt,
        "teardown keeps the attempt counter, so a log line still names its attempt"
    );
}

/// Retry runs the whole pipeline again over the same source, from a clean
/// stage: the attempt counter advances and the report is re-derived rather
/// than replayed — and a retry after a cancel brings the report back instead
/// of reviving a stale one.
#[test]
fn accept_f64_c_import_dialog_retry_reruns_the_pipeline_from_a_clean_stage() {
    let bytes = blueprint_document(&[full_record(), second_record()]);
    let (rules, _, _) = stock();
    let mut dialog = opened_dialog(&bytes, &rules);
    let first = dialog.lines().to_vec();
    assert_eq!(dialog.attempt(), 1);
    assert_eq!(dialog.verdict(), Some(MigrationVerdict::Full));

    let retried = dialog.retry();
    assert!(
        retried.view().is_some(),
        "the retry produced a report, not a refusal"
    );
    assert_eq!(dialog.attempt(), 2, "the retry is a second attempt");
    assert_eq!(
        dialog.lines(),
        first.as_slice(),
        "reading the same source through the same layout is deterministic"
    );

    // Through the machine: Retry works on the screen and leaves the report in
    // place, asking the domain for nothing.
    let mut front_end = front_end_on_the_import_dialog();
    front_end
        .open_import_dialog(opened_dialog(&bytes, &rules))
        .expect("the dialog screen takes the offered source");
    let outcome = front_end
        .apply(Action::Retry)
        .expect("a retry needs an attempt, and there is one");
    assert_eq!(front_end.screen(), Screen::ImportDialog);
    assert!(
        requests(&outcome.effects).is_empty(),
        "a retry asks the domain for nothing"
    );
    assert_eq!(
        front_end.import_dialog().expect("on screen").attempt(),
        2,
        "the retry re-ran the pipeline on the screen too"
    );
    let report = front_end.import_dialog().expect("on screen");
    assert!(!report.lines().is_empty());

    // And with no source offered, a retry is refused by name rather than
    // re-running nothing.
    let mut bare = front_end_on_the_import_dialog();
    assert_eq!(
        bare.apply(Action::Retry).expect_err("nothing was offered"),
        Refusal::NoImportAttempt
    );
}

/// No path through the screen writes anything: the source bytes, their
/// timestamp and every file of a destination tree are unchanged after every
/// action, including a confirmed one. The screen holds bytes, not a writer.
#[test]
fn accept_f64_c_import_dialog_no_screen_path_writes_anything() {
    let base = TempBase::new("dialog");
    let source_path = base.path().join("Planes/custom.pln");
    let destination = base.path().join("userdata/production");
    fs::create_dir_all(source_path.parent().expect("the source has a parent"))
        .expect("the source directory is created");
    fs::create_dir_all(&destination).expect("the destination root is created");
    fs::write(destination.join("existing.save"), b"fresh-engine-save")
        .expect("the existing save is written");

    let bytes = blueprint_document(&[full_record()]);
    fs::write(&source_path, &bytes).expect("the source is written");
    let before_meta = fs::metadata(&source_path).expect("the source exists");
    let before_modified = before_meta.modified().expect("the platform reports a time");
    let destination_before = tree(&destination);

    let (rules, _, _) = stock();
    let mut front_end = front_end_on_the_import_dialog();
    front_end
        .open_import_dialog(opened_dialog(&bytes, &rules))
        .expect("the dialog screen takes the offered source");
    // Every action the screen offers, including the one that asks the domain
    // for a transaction.
    front_end.apply(Action::Retry).expect("retry");
    front_end.apply(Action::ConfirmImport).expect("confirm");
    let refused = front_end
        .apply(Action::ConfirmImport)
        .expect_err("the report was already confirmed");
    assert_eq!(
        refused,
        Refusal::Import(ConfirmError::NothingToConfirm),
        "a confirmed import is not a second import"
    );
    front_end.apply(Action::Cancel).expect("cancel");
    assert_eq!(front_end.screen(), Screen::ProfileSelect);
    assert!(front_end.import_dialog().is_none());

    assert_eq!(
        fs::read(&source_path).expect("the source still reads"),
        bytes,
        "the source bytes are unchanged"
    );
    let after_meta = fs::metadata(&source_path).expect("the source still exists");
    assert_eq!(after_meta.len(), before_meta.len());
    assert_eq!(
        after_meta.modified().ok(),
        Some(before_modified),
        "the source is not re-stamped"
    );
    assert_eq!(
        tree(&destination),
        destination_before,
        "no action wrote into the destination: {:?}",
        tree(&destination)
    );
}

/// A second conforming record, so a report has more than one record row to
/// draw.
fn second_record() -> BlueprintRecord {
    BlueprintRecord {
        name: "kestrel",
        airframe: 7,
        engine: 8,
        guns: [1, 1, 1, 1],
        rockets: [5, 5],
    }
}

/// The screen and its producer agree on an unsupported document: a plan that
/// would carry nothing reaches the screen as `unsupported` with the producer's
/// reason row, and the owner action is refused with that same reason rather
/// than confirming a blank profile as imported.
#[test]
fn accept_f64_c_import_dialog_an_unsupported_document_reaches_the_screen_with_its_reason() {
    use cs_content::legacy_import::{LayoutAdmission, LegacyIdMap, UnresolvedReason};

    let bytes = blueprint_document(&[full_record()]);
    let (layout, _map) = blueprint_layout_and_map();
    let ids = LegacyIdMap::new();
    let catalog = catalog(CatalogRows::Base);
    let (rules, policy, book) = stock();
    let context = context(&ids, &catalog, &rules, &policy, &book, true);
    let source = proposal(&bytes, LegacyArtifactClass::CustomAircraft).expect("proposable");
    let target = target();
    let dialog_input = ImportOffer {
        source: &source,
        bytes: &bytes,
        layout: Some(&layout),
        limits: cs_formats::legacy_profile::LegacyLimits::designed(),
        field_map: None,
        target: &target,
        install_identity: None,
    };
    assert_eq!(
        context.admission,
        LayoutAdmission::AllowDesignedFixtures,
        "the fixture layout is reachable only through the fixture admission"
    );
    let mut front_end = front_end_on_the_import_dialog();
    front_end
        .open_import_dialog(ImportDialog::open(&context, &dialog_input))
        .expect("the dialog screen takes the offered source");
    let dialog = front_end.import_dialog().expect("on screen");

    assert_eq!(dialog.verdict(), Some(MigrationVerdict::Unsupported));
    assert!(
        dialog.lines().iter().any(|line| matches!(
            line,
            ReportLine::Unresolved {
                code: "id_not_mapped",
                ..
            }
        )),
        "the screen says why nothing could be carried: {:?}",
        dialog.lines()
    );
    let error = front_end
        .apply(Action::ConfirmImport)
        .expect_err("a plan that carries nothing cannot be confirmed");
    assert!(
        matches!(
            &error,
            Refusal::Import(ConfirmError::Unsupported { reason })
                if matches!(reason, UnresolvedReason::IdNotMapped { .. })
                    || reason.code() == "id_not_mapped"
        ),
        "the refusal carries the producer's own reason: {error:?}"
    );
    assert!(
        front_end
            .import_dialog()
            .expect("on screen")
            .confirmed()
            .is_none(),
        "never a blank profile called imported"
    );
}

/// The screen the presentation drives is the same one: a `ScreenSession` with
/// no authored deck (the dialogs are not deck-dependent) reaches the dialog
/// through the table, and the presentation's own open/read calls go to the
/// same machine.
#[test]
fn accept_f64_c_import_dialog_is_the_same_screen_the_presentation_drives() {
    let bytes = blueprint_document(&[full_record()]);
    let (rules, _, _) = stock();
    let deck = ScreenDeck::new(Vec::new()).expect("an empty deck is a valid deck");
    let mut session = ScreenSession::new(deck);
    session.press(Action::InstallVerified).expect("install");
    session.press(Action::NewProfile).expect("new profile");
    session
        .press(Action::OpenImportDialog)
        .expect("the profile screen offers the dialog");
    assert_eq!(session.front_end().screen(), Screen::ImportDialog);
    assert!(session.import_dialog().is_none());

    session
        .open_import_dialog(opened_dialog(&bytes, &rules))
        .expect("the presentation hands the screen its source");
    let dialog = session.import_dialog().expect("the dialog is on screen");
    assert_eq!(dialog.source_spelling(), support::FIXTURE_SPELLING);
    assert_eq!(dialog.verdict(), Some(MigrationVerdict::Full));

    let outcome = session
        .press(Action::ConfirmImport)
        .expect("the presentation confirms the report");
    assert_eq!(requests(&outcome.effects), vec![&Request::ConfirmImport]);
    assert!(
        session
            .import_dialog()
            .expect("on screen")
            .confirmed()
            .is_some()
    );
    session.press(Action::Cancel).expect("cancel");
    assert_eq!(session.front_end().screen(), Screen::ProfileSelect);
}

/// The application-level flow reaches the same screen, and the one
/// transaction nothing can run yet is refused by name: the confirmed import is
/// a value (Rally #756 writes it into a new profile), so the flow refuses the
/// request and — because the domain runs *before* the machine moves — changes
/// nothing on the way out.
#[test]
fn accept_f64_c_import_dialog_the_flow_refuses_a_confirm_it_cannot_persist() {
    let bytes = blueprint_document(&[full_record()]);
    let (rules, _, _) = stock();
    let graph = lower_campaign(&declared_synthetic_campaign()).expect("the fixture lowers");
    let base = TempBase::new("dialog-flow");
    let setup = FlowSetup::new(
        base.path().to_path_buf(),
        graph,
        CampaignRunId::new("run.one").expect("valid run id"),
        DifficultyId::new("standard").expect("valid difficulty"),
    )
    .sandbox();
    let deck = ScreenDeck::new(Vec::new()).expect("an empty deck is a valid deck");
    let mut flow = FrontEndFlow::new(deck, setup);

    flow.press(Action::InstallVerified).expect("install");
    flow.press(Action::NewProfile).expect("new profile");
    flow.press(Action::OpenImportDialog)
        .expect("open the dialog");
    flow.open_import_dialog(opened_dialog(&bytes, &rules))
        .expect("the flow hands the screen its source");
    assert_eq!(
        flow.import_dialog().expect("on screen").verdict(),
        Some(MigrationVerdict::Full)
    );

    let refused = flow
        .press(Action::ConfirmImport)
        .expect_err("nothing writes a confirmed import into a new profile yet");
    assert!(
        matches!(refused, FlowError::ImportNotPersistable),
        "the refusal names the missing transaction: {refused:?} ({refused})"
    );
    // The domain runs before the transition, so a refusal leaves the machine
    // exactly where it was: the report is still on screen and nothing is
    // confirmed.
    assert_eq!(flow.front_end().screen(), Screen::ImportDialog);
    assert!(
        flow.import_dialog()
            .expect("on screen")
            .confirmed()
            .is_none()
    );
    assert!(!flow.import_dialog().expect("on screen").lines().is_empty());

    // The rest of the dialog's path does run end to end through the flow.
    flow.press(Action::Retry).expect("retry");
    flow.press(Action::Cancel).expect("cancel");
    assert_eq!(flow.front_end().screen(), Screen::ProfileSelect);
    assert!(flow.import_dialog().is_none());
}
