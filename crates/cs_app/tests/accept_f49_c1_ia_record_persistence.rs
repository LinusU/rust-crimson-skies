//! Acceptance scenarios for F49-C1: the profile-scoped Instant Action record
//! scope persists through the production profile commit path.
//! Task test prefix: `accept_f49_c1_`.
//!
//! Spec: `specs/F49-instant-action-presets-and-custom-scenarios.md`, finding
//! `docs/findings/2026-10-08-f49-c-instant-action-screen-flow.md`; contract
//! `docs/contracts/STATE-TRANSACTIONS.md` ("Persistence", "Persistent profile
//! data receives only an explicit outcome transaction").
//!
//! Every value is newly authored synthetic data in a temporary directory.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_app::campaign::{CampaignRun, campaign_profile_id, lower_campaign, read_snapshot};
use cs_app::profile::ProfileSession;
use cs_app::ui::instant_action::{
    IaAction, IaPersistError, IaRecordEntry, IaRecordError, InstantActionFlow, ScenarioResult,
    ScenarioSnapshot, read_records,
};
use cs_content::campaign::declared_synthetic_campaign;
use cs_content::instant_action::{
    RosterSlot, ScenarioSide, synthetic_dogfight_preset, synthetic_instant_action_catalog,
};
use cs_content::save::settings::SettingCatalog;
use cs_sim::campaign::{
    CampaignNodeKey, CampaignRunId, DifficultyId, EventKey, MissionOutcome, Outcome,
    OutcomeAuthority, OutcomeId, ProfileId as CampaignProfileId, SessionGeneration, SymbolId,
};
use cs_types::Tick;
use cs_types::profile::{ExtraField, ProfileId, ProfileKind};

struct TempBase(PathBuf);

impl TempBase {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f49-c1-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the fixture base is created");
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempBase {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sandbox(base: &Path) -> ProfileSession {
    let catalog = SettingCatalog::new([]).expect("an empty catalog holds together");
    ProfileSession::open_sandbox(base, &catalog).expect("the sandbox session opens")
}

/// The flow's profile identity for a slot, the way production derives it.
fn flow_profile(slot: ProfileId) -> CampaignProfileId {
    campaign_profile_id(ProfileKind::Synthetic, slot)
}

/// Plays one Instant Action session to `Finish` and returns the entry it
/// settled, with the flow that holds it.
fn play_one(mut flow: InstantActionFlow, win: bool) -> (InstantActionFlow, IaRecordEntry) {
    let preset = synthetic_dogfight_preset().id().clone();
    flow.select_preset(&preset).expect("the preset selects");
    flow.apply(IaAction::Launch).expect("the preset lowers");
    let generation = flow.generation().expect("a session is running");
    let snapshot = if win {
        ScenarioSnapshot::new(20, vec![(ScenarioSide::Player, RosterSlot(0))])
    } else {
        ScenarioSnapshot::new(9, vec![(ScenarioSide::Enemy, RosterSlot(1))])
    };
    flow.report(generation, snapshot)
        .expect("the session reports");
    flow.apply(IaAction::Finish).expect("an outcome settles");
    let entry = flow
        .records()
        .entries()
        .last()
        .expect("finishing recorded the session")
        .clone();
    (flow, entry)
}

fn campaign_win(profile: &CampaignProfileId) -> MissionOutcome {
    MissionOutcome {
        id: OutcomeId {
            profile: profile.clone(),
            run: CampaignRunId::new("run.one").expect("run id"),
            session: SessionGeneration(1),
            terminal_event: EventKey {
                session: SessionGeneration(1),
                tick: Tick(480),
                source: SymbolId(7),
                sequence: 0,
            },
        },
        node: CampaignNodeKey::new("m01").expect("node key"),
        outcome: Outcome::Succeeded,
        score: 900,
        authority: OutcomeAuthority::Authorized,
    }
}

/// An entry written and re-read, across a session restart, carries the same
/// profile, generation, subject, seed, result, ended tick and attempt.
#[test]
fn accept_f49_c1_a_stored_record_round_trips_across_a_restart() {
    let base = TempBase::new();
    let mut session = sandbox(base.path());
    let slot = session.create("Pilot").expect("a pilot is created");
    let profile = flow_profile(slot);

    let flow = InstantActionFlow::new(profile.clone(), synthetic_instant_action_catalog());
    let (_flow, entry) = play_one(flow, true);
    let before = session.document().expect("selected").revision;
    let revision = session
        .record_instant_action(&entry)
        .expect("the settlement is written");
    assert_eq!(
        revision,
        before.next().expect("a successor"),
        "one revision"
    );
    session.finish().expect("the session ends");

    let mut reopened = sandbox(base.path());
    if reopened.selected().is_none() {
        reopened.select(slot).expect("the pilot selects");
    }
    let book = reopened
        .instant_action_records()
        .expect("the stored records read back");
    assert_eq!(book.profile(), &profile);
    assert_eq!(book.entries(), std::slice::from_ref(&entry));
    let read = &book.entries()[0];
    assert_eq!(read.profile(), entry.profile());
    assert_eq!(read.generation(), entry.generation());
    assert_eq!(read.subject(), entry.subject());
    assert_eq!(read.seed(), entry.seed());
    assert_eq!(read.result(), ScenarioResult::Victory);
    assert_eq!(read.ended_tick(), entry.ended_tick());
    assert_eq!(read.attempt(), entry.attempt());
}

/// An Instant Action settlement leaves every campaign field exactly as it
/// was, proved on a real played campaign with a control that moves it.
#[test]
fn accept_f49_c1_settling_ia_leaves_the_campaign_unchanged() {
    let base = TempBase::new();
    let mut session = sandbox(base.path());
    let slot = session.create("Pilot").expect("a pilot is created");
    let profile = flow_profile(slot);
    let graph = lower_campaign(&declared_synthetic_campaign()).expect("the fixture lowers");
    let mut run = CampaignRun::open(
        &mut session,
        graph,
        &profile,
        &CampaignRunId::new("run.one").expect("run id"),
        &DifficultyId::new("standard").expect("difficulty"),
    )
    .expect("the campaign begins");
    run.report_outcome(&mut session, &campaign_win(&profile))
        .expect("the campaign victory is applied");
    assert!(run.state().currency() > 0, "there is a balance to protect");

    let state_before = run.state().snapshot();
    let doc_before = session.document().expect("selected").clone();
    let saved_before = read_snapshot(&doc_before).expect("readable");

    let flow = InstantActionFlow::new(profile.clone(), synthetic_instant_action_catalog());
    let (flow, first) = play_one(flow, false);
    let (_flow, second) = play_one(flow, true);
    session
        .record_instant_action(&first)
        .expect("first settles");
    session
        .record_instant_action(&second)
        .expect("second settles");

    let doc_after = session.document().expect("selected");
    assert_eq!(doc_after.campaign, doc_before.campaign, "ledger and cash");
    assert_eq!(read_snapshot(doc_after).expect("readable"), saved_before);
    assert_eq!(run.state().snapshot(), state_before);
    let campaign_extras = |d: &cs_types::profile::ProfileDocument| {
        d.extra
            .iter()
            .filter(|field| field.key.starts_with("campaign."))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(campaign_extras(doc_after), campaign_extras(&doc_before));
    assert_eq!(
        Some(doc_after.revision),
        doc_before.revision.next().and_then(|r| r.next()),
        "one whole revision per settlement"
    );
    assert_eq!(
        read_records(doc_after).expect("readable").len(),
        2,
        "the Instant Action records did land"
    );

    // Control: a real campaign outcome does move the campaign snapshot.
    let mut other = campaign_win(&profile);
    other.id.session = SessionGeneration(2);
    other.id.terminal_event.session = SessionGeneration(2);
    other.node = CampaignNodeKey::new("m02").expect("node key");
    run.report_outcome(&mut session, &other)
        .expect("a second campaign outcome applies");
    assert_ne!(run.state().snapshot(), state_before);
}

/// A foreign profile's entry and a repeated generation are refused, and the
/// refused settlement writes nothing.
#[test]
fn accept_f49_c1_foreign_and_duplicate_settlements_are_refused() {
    let base = TempBase::new();
    let mut session = sandbox(base.path());
    let slot = session.create("Pilot").expect("a pilot is created");
    let flow = InstantActionFlow::new(flow_profile(slot), synthetic_instant_action_catalog());
    let (_flow, entry) = play_one(flow, true);

    let other_slot = ProfileId::new(slot.get() + 1).expect("another slot");
    let foreign_flow =
        InstantActionFlow::new(flow_profile(other_slot), synthetic_instant_action_catalog());
    let (_f, foreign) = play_one(foreign_flow, true);

    let start = session.document().expect("selected").revision;
    match session.record_instant_action(&foreign) {
        Err(IaPersistError::Record(IaRecordError::ForeignProfile { .. })) => {}
        other => panic!("expected ForeignProfile, got {other:?}"),
    }
    assert_eq!(session.document().expect("selected").revision, start);

    session.record_instant_action(&entry).expect("settles once");
    let stored = session.document().expect("selected").revision;
    match session.record_instant_action(&entry) {
        Err(IaPersistError::Record(IaRecordError::DuplicateGeneration { generation })) => {
            assert_eq!(generation, entry.generation());
        }
        other => panic!("expected DuplicateGeneration, got {other:?}"),
    }
    assert_eq!(session.document().expect("selected").revision, stored);
    assert_eq!(session.instant_action_records().expect("reads").len(), 1);
}

/// A flow resumed over the persisted book mints generations above the stored
/// ones, so a later visit is never refused as a duplicate.
#[test]
fn accept_f49_c1_a_resumed_flow_mints_generations_above_the_stored_ones() {
    let base = TempBase::new();
    let mut session = sandbox(base.path());
    let slot = session.create("Pilot").expect("a pilot is created");
    let flow = InstantActionFlow::new(flow_profile(slot), synthetic_instant_action_catalog());
    let (_flow, first) = play_one(flow, false);
    session
        .record_instant_action(&first)
        .expect("first settles");

    let book = session.instant_action_records().expect("reads");
    let resumed = InstantActionFlow::resume(book, synthetic_instant_action_catalog());
    let (_flow, second) = play_one(resumed, true);
    assert!(second.generation() > first.generation());
    assert_eq!(second.attempt(), first.attempt() + 1, "same subject again");
    session
        .record_instant_action(&second)
        .expect("a later visit settles");
    assert_eq!(session.instant_action_records().expect("reads").len(), 2);
}

/// A damaged stored record is reported, not skipped.
#[test]
fn accept_f49_c1_a_damaged_stored_record_is_reported() {
    let base = TempBase::new();
    let mut session = sandbox(base.path());
    session.create("Pilot").expect("a pilot is created");
    session
        .document_mut()
        .expect("selected")
        .extra
        .push(ExtraField {
            key: "ia.record.1".to_owned(),
            value: "not;a;record".to_owned(),
        });
    match session.instant_action_records() {
        Err(IaPersistError::Damaged { key, .. }) => assert_eq!(key, "ia.record.1"),
        other => panic!("expected Damaged, got {other:?}"),
    }
}
