//! Acceptance scenarios for F43-V3: `CampaignRun::report_mission_record`, the
//! results screen's transaction over **both** record stores (the campaign
//! progression store and the profile scrapbook), committed as one profile
//! revision. Task test prefix: `accept_f43_v3_`.
//!
//! Implemented: the one-revision commit (not the campaign-first fallback).
//! The flow-level pin (the shipped `Action::MissionSucceeded` path) is
//! `accept_f43_v3_the_results_screen_writes_the_scrapbook_in_the_campaign_revision`
//! in `ui/wiring.rs`.
//!
//! Every value is newly authored synthetic data in a temporary directory.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_app::campaign::{CampaignRun, lower_campaign};
use cs_app::profile::ProfileSession;
use cs_app::ui::scrapbook::stored;
use cs_content::campaign::{
    CampaignDefinition, CampaignDraft, CampaignEdge, CampaignNode, CampaignNodeId, EdgeCondition,
    NodeKind, RewardSpec,
};
use cs_content::save::settings::SettingCatalog;
use cs_sim::campaign::{
    CampaignNodeKey, CampaignRunId, DifficultyId, EventKey, MissionOutcome, Outcome,
    OutcomeAuthority, OutcomeId, OutcomeReceipt, ProfileId, SessionGeneration, SymbolId,
};
use cs_sim::records::{
    BestChange, BetterIs, DifficultyScope, Fact, FactKind, RecordKey, RecordReceipt, RecordRule,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;

const PROFILE: &str = "pilot.v3";
const RUN: &str = "run.v3";

fn designed() -> Provenance {
    Provenance::designed(ClaimId::new("f43v3.economy").expect("valid claim id"))
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, designed()))
}

fn node(id: &str) -> CampaignNodeId {
    CampaignNodeId::new(id).expect("valid node id")
}

fn mission() -> ContentId {
    ContentId::from_source(ContentKind::Mission, "m01").expect("valid mission id")
}

/// `m01 --Victory(+500)--> ending`, `m01 --Defeat--> m01`.
fn campaign() -> CampaignDefinition {
    CampaignDefinition::try_new(CampaignDraft {
        nodes: vec![
            CampaignNode {
                id: node("m01"),
                kind: NodeKind::Mission {
                    mission: known(mission()),
                },
                edges: vec![
                    CampaignEdge {
                        on: EdgeCondition::Victory,
                        to: node("ending"),
                        grant: Some(RewardSpec {
                            currency: known(500),
                            unlocks: Vec::new(),
                        }),
                        provenance: designed(),
                    },
                    CampaignEdge {
                        on: EdgeCondition::Defeat,
                        to: node("m01"),
                        grant: None,
                        provenance: designed(),
                    },
                ],
                provenance: designed(),
            },
            CampaignNode {
                id: node("ending"),
                kind: NodeKind::Ending,
                edges: Vec::new(),
                provenance: designed(),
            },
        ],
        entry: node("m01"),
        roster: Vec::new(),
        provenance: designed(),
    })
    .expect("the fixture is a valid declared campaign")
}

fn result(session: u32, kind: Outcome, score: u64) -> MissionOutcome {
    MissionOutcome {
        id: OutcomeId {
            profile: ProfileId::new(PROFILE).expect("valid profile id"),
            run: CampaignRunId::new(RUN).expect("valid run id"),
            session: SessionGeneration(session),
            terminal_event: EventKey {
                session: SessionGeneration(session),
                tick: Tick(10),
                source: SymbolId(7),
                sequence: 1,
            },
        },
        node: CampaignNodeKey::new("m01").expect("valid node key"),
        outcome: kind,
        score,
        authority: OutcomeAuthority::Authorized,
    }
}

struct TempBase(PathBuf);

impl TempBase {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f43-v3-{}-{}",
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
    ProfileSession::open_sandbox(base, &SettingCatalog::new([]).expect("an empty catalog"))
        .expect("the sandbox session opens")
}

fn open_run(session: &mut ProfileSession) -> CampaignRun {
    CampaignRun::open(
        session,
        lower_campaign(&campaign()).expect("the fixture lowers"),
        &ProfileId::new(PROFILE).expect("valid profile id"),
        &CampaignRunId::new(RUN).expect("valid run id"),
        &DifficultyId::new("standard").expect("valid difficulty"),
    )
    .expect("the campaign opens")
}

fn key() -> RecordKey {
    RecordKey {
        subject: mission(),
        difficulty: None,
    }
}

fn revision(session: &ProfileSession) -> cs_types::profile::Revision {
    session.document().expect("a profile is open").revision
}

fn won_fact() -> Fact {
    Fact {
        kind: FactKind::MissionSucceeded,
        subject: mission(),
    }
}

#[test]
fn accept_f43_v3_a_completed_mission_is_in_the_persisted_scrapbook_in_one_revision() {
    let base = TempBase::new();
    let mut session = sandbox(base.path());
    session.create("Pilot").expect("a pilot is created");
    let mut run = open_run(&mut session);
    let before = revision(&session);

    let receipt = run
        .report_mission_record(
            &mut session,
            &result(1, Outcome::Succeeded, 900),
            RecordRule::DEFAULT,
        )
        .expect("the outcome applies");
    assert!(matches!(
        receipt.campaign.receipt,
        OutcomeReceipt::Applied(_)
    ));
    assert!(matches!(
        receipt.record,
        RecordReceipt::Applied {
            best: BestChange::First,
            ..
        }
    ));

    assert_eq!(run.state().currency(), 500);
    assert_eq!(
        revision(&session),
        before.next().expect("a successor"),
        "both stores are one profile revision"
    );
    // Reopen: what is on disk, not what is in memory.
    drop(run);
    drop(session);
    let mut session = sandbox(base.path());
    let run = open_run(&mut session);
    assert_eq!(run.state().currency(), 500, "the campaign half survived");
    let scrapbook = stored(&session).expect("the scrapbook reads");
    let slot = scrapbook
        .records
        .slot(&key())
        .expect("the mission has a persisted slot");
    assert_eq!((slot.best.score, slot.latest.score), (900, 900));
    assert!(scrapbook.facts.has(&won_fact()), "mission-succeeded fact");
}

/// A crash between the two commits (the campaign half persisted by the old
/// campaign-only path, the scrapbook half not) is repaired by replaying the
/// same outcome through the real results path, without paying twice.
#[test]
fn accept_f43_v3_a_replay_after_a_campaign_only_commit_repairs_the_scrapbook() {
    let base = TempBase::new();
    let mut session = sandbox(base.path());
    session.create("Pilot").expect("a pilot is created");
    let mut run = open_run(&mut session);
    let win = result(1, Outcome::Succeeded, 900);

    run.report_outcome(&mut session, &win)
        .expect("the campaign half commits alone");
    assert!(
        stored(&session)
            .expect("reads")
            .records
            .slot(&key())
            .is_none()
    );
    drop(run);
    drop(session); // crash before the scrapbook half

    let mut session = sandbox(base.path());
    let mut run = open_run(&mut session);
    assert_eq!(run.state().currency(), 500);
    let campaign_revision = run.state().revision();
    let receipt = run
        .report_mission_record(&mut session, &win, RecordRule::DEFAULT)
        .expect("the replay applies");
    assert_eq!(receipt.campaign.receipt, OutcomeReceipt::AlreadyApplied);
    assert!(matches!(receipt.record, RecordReceipt::Applied { .. }));
    assert_eq!(
        run.state().currency(),
        500,
        "the campaign is not paid twice"
    );
    assert_eq!(run.state().revision(), campaign_revision);
    let scrapbook = stored(&session).expect("reads");
    assert_eq!(
        scrapbook.records.slot(&key()).expect("repaired").best.score,
        900
    );
    assert!(scrapbook.facts.has(&won_fact()));

    // A replay of the complete application writes nothing.
    let settled = revision(&session);
    let again = run
        .report_mission_record(&mut session, &win, RecordRule::DEFAULT)
        .expect("a complete replay is acknowledged");
    assert_eq!(again.record, RecordReceipt::AlreadyApplied);
    assert_eq!(revision(&session), settled, "nothing was written");
}

/// A worse later run keeps best in both stores and records latest.
#[test]
fn accept_f43_v3_a_worse_replay_keeps_best_in_both_stores() {
    let base = TempBase::new();
    let mut session = sandbox(base.path());
    session.create("Pilot").expect("a pilot is created");
    let mut run = open_run(&mut session);
    run.report_mission_record(
        &mut session,
        &result(1, Outcome::Succeeded, 900),
        RecordRule::DEFAULT,
    )
    .expect("the first win");
    run.report_mission_record(
        &mut session,
        &result(2, Outcome::Succeeded, 300),
        RecordRule::DEFAULT,
    )
    .expect("the worse win");

    let progress = run
        .state()
        .progress(&CampaignNodeKey::new("m01").expect("valid node key"))
        .expect("the node has a record");
    assert_eq!(progress.best_score, 900, "campaign best kept");
    assert_eq!(progress.latest, Some((Outcome::Succeeded, 300)));
    let scrapbook = stored(&session).expect("reads");
    let slot = scrapbook.records.slot(&key()).expect("slot");
    assert_eq!(slot.best.score, 900, "scrapbook best kept");
    assert_eq!(slot.latest.score, 300, "scrapbook latest recorded");
}

/// The declared rule governs both stores: under lower-is-better the smaller
/// score is best in the campaign gate and in the scrapbook.
#[test]
fn accept_f43_v3_the_declared_rule_governs_both_stores() {
    let rule = RecordRule {
        better: BetterIs::Lower,
        scope: DifficultyScope::AllDifficulties,
    };
    let base = TempBase::new();
    let mut session = sandbox(base.path());
    session.create("Pilot").expect("a pilot is created");
    let mut run = open_run(&mut session);
    for (generation, score) in [(1, 900), (2, 300)] {
        run.report_mission_record(
            &mut session,
            &result(generation, Outcome::Succeeded, score),
            rule,
        )
        .expect("applies");
    }
    let progress = run
        .state()
        .progress(&CampaignNodeKey::new("m01").expect("valid node key"))
        .expect("record");
    assert_eq!(progress.best_score, 300);
    let scrapbook = stored(&session).expect("reads");
    assert_eq!(
        scrapbook.records.slot(&key()).expect("slot").best.score,
        300
    );
}
