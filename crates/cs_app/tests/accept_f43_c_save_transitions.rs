//! Acceptance scenarios for F43-C: the briefing / loadout / results / save
//! transitions, wired from the campaign state into the profile save.
//! Task test prefix: `accept_f43_c_`.
//!
//! Spec: `specs/F43-campaign-progression-outcomes-and-economy-rules.md`, stage
//! `### F43-C` and AC03; contract `docs/contracts/STATE-TRANSACTIONS.md`
//! ("Outcome and economy transaction", "Persistence").
//!
//! **AC03, "Crash during reward save; recovery applies either the old or the
//! new complete transaction"**, is
//! `accept_f43_c_a_crash_during_the_reward_save_recovers_the_old_or_the_new_revision`.
//! It drives `CampaignRun::report_outcome` over a real `ProfileSession`, stops
//! the save at each window (before any write; temp written and the old file
//! rotated but nothing installed; after the commit but before the results
//! screen acknowledged), reopens, and checks the recovered campaign is exactly
//! the old state or exactly the new one, and that replaying the same result
//! pays the reward once.
//!
//! Every value is newly authored synthetic data in a temporary directory; no
//! amount is an original economy value.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_app::campaign::{
    CampaignRun, CampaignSaveError, lower_campaign, read_snapshot, write_snapshot,
};
use cs_app::profile::ProfileSession;
use cs_content::campaign::{
    CampaignDefinition, CampaignDraft, CampaignEdge, CampaignNode, CampaignNodeId, EdgeCondition,
    NodeKind, RewardSpec,
};
use cs_content::save::fs::DirStorage;
use cs_content::save::library::{load_profile_slot, slot_name};
use cs_content::save::settings::SettingCatalog;
use cs_content::save::store::{PROFILE_PREFIX, SaveStorage};
use cs_sim::campaign::{
    CampaignError, CampaignNodeKey, CampaignRunId, DifficultyId, EventKey, LoadoutWeight,
    MissionOutcome, Outcome, OutcomeAuthority, OutcomeId, OutcomeReceipt, ProfileId, PurchaseDraft,
    SellDraft, SessionGeneration, SymbolId,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::profile::{ExtraField, ProfileKind};

const PROFILE: &str = "pilot.nathan";
const RUN: &str = "run.one";

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("valid claim id")
}

fn designed() -> Provenance {
    Provenance::designed(claim("f43b.economy"))
}

/// An interlude asset that is still unsurveyed — the optional unknown the
/// lowering boundary is specified to accept.
fn unknown_asset(reason: &str) -> Resolved<ContentId> {
    Resolved::Unknown {
        claim_id: claim("f43b.interlude_asset"),
        reason: reason.to_owned(),
    }
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, designed()))
}

fn node(id: &str) -> CampaignNodeId {
    CampaignNodeId::new(id).expect("valid node id")
}

fn key(id: &str) -> CampaignNodeKey {
    CampaignNodeKey::new(id).expect("valid node key")
}

fn content(kind: ContentKind, id: &str) -> ContentId {
    ContentId::from_source(kind, id).expect("valid content id")
}

fn mission_content(id: &str) -> ContentId {
    content(ContentKind::Mission, id)
}

/// An empty declared campaign entering at `entry`.
fn empty_draft(entry: CampaignNodeId) -> CampaignDraft {
    CampaignDraft {
        nodes: Vec::new(),
        entry,
        roster: Vec::new(),
        provenance: designed(),
    }
}

fn event(session: u32, tick: u64, sequence: u32) -> EventKey {
    EventKey {
        session: SessionGeneration(session),
        tick: Tick(tick),
        source: SymbolId(7),
        sequence,
    }
}

fn profile() -> ProfileId {
    ProfileId::new(PROFILE).expect("valid profile id")
}

/// A terminal report for `node`, in `session` generation.
///
/// `session` is part of the outcome identity, so two calls with different
/// sessions are two genuinely different transactions — a *replay* — while the
/// same session and sequence is the same transaction replayed after a crash.
fn outcome(
    session: u32,
    tick: u64,
    sequence: u32,
    node: &str,
    kind: Outcome,
    score: u64,
) -> MissionOutcome {
    MissionOutcome {
        id: OutcomeId {
            profile: profile(),
            run: CampaignRunId::new(RUN).expect("valid run id"),
            session: SessionGeneration(session),
            terminal_event: event(session, tick, sequence),
        },
        node: key(node),
        outcome: kind,
        score,
        authority: OutcomeAuthority::Authorized,
    }
}

/// A campaign whose victory path crosses a narrative beat and then a **later**
/// unlock, so a replay of `m01` has something it could damage:
///
/// ```text
/// m01 --Victory--> interlude --Victory--> m02 --Victory--> ending
/// ```
///
/// `m01`'s victory grants 500 and unlocks `plane_a`; the interlude grants 25 and
/// unlocks nothing; `m02`'s victory grants 900 and unlocks `plane_b`. Roster
/// availability: `plane_b` opens once `m01` is won, `plane_c` once `m02` is won.
fn declared_interlude_campaign() -> CampaignDefinition {
    let mut draft = empty_draft(node("m01"));
    draft.nodes.push(CampaignNode {
        id: node("m01"),
        kind: NodeKind::Mission {
            mission: known(mission_content("m01")),
        },
        edges: vec![
            CampaignEdge {
                on: EdgeCondition::Victory,
                to: node("interlude"),
                grant: Some(RewardSpec {
                    currency: known(500),
                    unlocks: vec![content(ContentKind::Airframe, "plane_a")],
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
    });
    draft.nodes.push(CampaignNode {
        id: node("interlude"),
        kind: NodeKind::Interlude {
            asset: unknown_asset("the beat's media asset is unsurveyed"),
        },
        edges: vec![CampaignEdge {
            on: EdgeCondition::Victory,
            to: node("m02"),
            grant: Some(RewardSpec {
                currency: known(25),
                unlocks: Vec::new(),
            }),
            provenance: designed(),
        }],
        provenance: designed(),
    });
    draft.nodes.push(CampaignNode {
        id: node("m02"),
        kind: NodeKind::Mission {
            mission: known(mission_content("m02")),
        },
        edges: vec![CampaignEdge {
            on: EdgeCondition::Victory,
            to: node("ending"),
            grant: Some(RewardSpec {
                currency: known(900),
                unlocks: vec![content(ContentKind::Airframe, "plane_b")],
            }),
            provenance: designed(),
        }],
        provenance: designed(),
    });
    draft.nodes.push(CampaignNode {
        id: node("ending"),
        kind: NodeKind::Ending,
        edges: Vec::new(),
        provenance: designed(),
    });
    draft.roster.push(cs_content::campaign::RosterEntry {
        item: content(ContentKind::Airframe, "plane_b"),
        available_from: node("m01"),
        provenance: designed(),
    });
    draft.roster.push(cs_content::campaign::RosterEntry {
        item: content(ContentKind::Airframe, "plane_c"),
        available_from: node("m02"),
        provenance: designed(),
    });
    CampaignDefinition::try_new(draft).expect("the fixture is a valid declared campaign")
}

struct TempBase(PathBuf);

impl TempBase {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f43-c-{label}-{}-{}",
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

fn catalog() -> SettingCatalog {
    SettingCatalog::new([]).expect("an empty catalog holds together")
}

fn sandbox(base: &Path) -> ProfileSession {
    ProfileSession::open_sandbox(base, &catalog()).expect("the sandbox session opens")
}

fn run_id() -> CampaignRunId {
    CampaignRunId::new(RUN).expect("valid run id")
}

fn difficulty() -> DifficultyId {
    DifficultyId::new("standard").expect("valid difficulty")
}

fn open_run(session: &mut ProfileSession) -> Result<CampaignRun, CampaignSaveError> {
    let graph = lower_campaign(&declared_interlude_campaign()).expect("the fixture lowers");
    CampaignRun::open(session, graph, &profile(), &run_id(), &difficulty())
}

/// A fresh sandbox with one pilot and a campaign begun on it.
fn begun(base: &Path) -> (ProfileSession, CampaignRun, cs_types::profile::ProfileId) {
    let mut session = sandbox(base);
    let id = session.create("Pilot").expect("a pilot is created");
    let run = open_run(&mut session).expect("the campaign begins");
    (session, run, id)
}

fn win_m01() -> MissionOutcome {
    outcome(1, 10, 1, "m01", Outcome::Succeeded, 900)
}

fn slot(base: &Path, id: cs_types::profile::ProfileId) -> PathBuf {
    base.join(ProfileKind::Synthetic.label())
        .join(slot_name(id))
}

#[test]
fn accept_f43_c_a_crash_during_the_reward_save_recovers_the_old_or_the_new_revision() {
    // Window 1: the process dies before anything is written. The old complete
    // revision is what recovery finds, and the replay pays once.
    {
        let base = TempBase::new("crash-before");
        let (session, run, _id) = begun(base.path());
        let old = run.state().snapshot();
        drop(run);
        drop(session); // the result was computed in memory only
        let mut session = sandbox(base.path());
        let mut run = open_run(&mut session).expect("recovery opens the old revision");
        assert_eq!(run.state().snapshot(), old, "the old transaction, whole");
        let receipt = run
            .report_outcome(&mut session, &win_m01())
            .expect("replay");
        assert!(matches!(receipt.receipt, OutcomeReceipt::Applied(_)));
        assert_eq!(run.state().currency(), 500, "paid once");
    }

    // Window 2: the new revision reached the temp file and the old file was
    // rotated to the backup, but nothing was installed. Recovery must land on
    // one whole revision — never a mixture.
    {
        let base = TempBase::new("crash-torn");
        let (session, run, id) = begun(base.path());
        let old = run.state().snapshot();
        let mut next = run.state().clone();
        next.apply_outcome(run.graph(), &win_m01())
            .expect("applies in memory");
        let new = next.snapshot();
        let directory = slot(base.path(), id);
        let mut document = load_profile_slot(&directory)
            .expect("readable")
            .expect("a revision");
        document.revision = document.revision.next().expect("a successor");
        write_snapshot(&mut document, &new).expect("the new campaign encodes");
        let bytes = cs_content::save::codec::encode(&document).expect("encodes");
        drop(run);
        drop(session);
        let mut storage = DirStorage::new(&directory, PROFILE_PREFIX);
        storage.write_temp(&bytes).expect("temp write");
        storage.sync_temp().expect("temp sync");
        storage.rotate_backup().expect("rotation");
        // ... the process dies here.
        let mut session = sandbox(base.path());
        let mut run = open_run(&mut session).expect("recovery opens a whole revision");
        let recovered = run.state().snapshot();
        assert!(
            recovered == old || recovered == new,
            "recovered neither the old nor the new transaction: {recovered:?}"
        );
        // Whichever it was, the same result reported again pays exactly once.
        run.report_outcome(&mut session, &win_m01())
            .expect("replay");
        assert_eq!(run.state().currency(), 500);
        assert_eq!(run.state().revision(), 1);
    }

    // Window 3: the save committed, the process died before the results screen
    // acknowledged. The reward is in the new revision; the replay is suppressed.
    {
        let base = TempBase::new("crash-after");
        let (mut session, mut run, _id) = begun(base.path());
        run.report_outcome(&mut session, &win_m01())
            .expect("applied");
        let new = run.state().snapshot();
        drop(run);
        drop(session);
        let mut session = sandbox(base.path());
        let mut run = open_run(&mut session).expect("resumes");
        assert_eq!(run.state().snapshot(), new, "the new transaction, whole");
        let before = session.document().expect("doc").revision;
        let replay = run
            .report_outcome(&mut session, &win_m01())
            .expect("replay");
        assert_eq!(replay.receipt, OutcomeReceipt::AlreadyApplied);
        assert_eq!(run.state().currency(), 500, "not paid twice");
        assert_eq!(
            session.document().expect("doc").revision,
            before,
            "a suppressed replay writes nothing"
        );
    }
}

#[test]
fn accept_f43_c_the_briefing_loadout_results_path_survives_a_restart() {
    let base = TempBase::new("path");
    let (mut session, mut run, _id) = begun(base.path());
    run.report_outcome(&mut session, &win_m01())
        .expect("results");
    let crossing = run.advance_interludes(&mut session).expect("briefing");
    assert_eq!(crossing.traversed, vec![key("interlude")]);
    let plane_b = content(ContentKind::Airframe, "plane_b");
    let draft = PurchaseDraft {
        item: plane_b.clone(),
        price: 100,
        weight: LoadoutWeight::Measured {
            total: 5,
            limit: 10,
        },
        expected_revision: run.state().revision(),
    };
    // plane_b is open after m01; the balance is 525 after the beat's grant.
    run.purchase(&mut session, &draft).expect("loadout buys");
    assert_eq!(run.state().currency(), 425);
    run.sell(
        &mut session,
        &SellDraft {
            item: plane_b.clone(),
            expected_revision: run.state().revision(),
        },
    )
    .expect("loadout sells");
    run.purchase(
        &mut session,
        &PurchaseDraft {
            expected_revision: run.state().revision(),
            ..draft
        },
    )
    .expect("buys again");
    let live = run.state().snapshot();
    drop(run);
    drop(session);

    let mut session = sandbox(base.path());
    let run = open_run(&mut session).expect("resumes");
    assert_eq!(run.state().snapshot(), live, "every transition was saved");
    assert_eq!(run.state().currency(), 425);
    assert!(run.state().paid_for(&plane_b).is_some());
    assert_eq!(run.state().current(), &key("m02"));
}

#[test]
fn accept_f43_c_a_refused_transition_writes_nothing_and_leaves_the_run_alone() {
    let base = TempBase::new("refused");
    let (mut session, mut run, _id) = begun(base.path());
    let before = run.state().snapshot();
    let stored = session.document().expect("doc").revision;
    // m02 is not reachable yet.
    let error = run
        .report_outcome(
            &mut session,
            &outcome(1, 9, 1, "m02", Outcome::Succeeded, 1),
        )
        .expect_err("a future mission cannot report");
    assert!(matches!(
        error,
        CampaignSaveError::Campaign(CampaignError::IneligibleNode { .. })
    ));
    assert_eq!(run.state().snapshot(), before);
    assert_eq!(session.document().expect("doc").revision, stored);
}

#[test]
fn accept_f43_c_a_save_another_writer_moved_is_not_overwritten() {
    let base = TempBase::new("stale");
    let (mut session, mut run, _id) = begun(base.path());
    let before = run.state().snapshot();
    // Another writer commits a campaign revision this run has not seen.
    let mut rival = before.clone();
    rival.revision = 7;
    rival.currency = 77;
    session
        .commit_with(|document| {
            write_snapshot(document, &rival).expect("rival encodes");
            Ok(())
        })
        .expect("the rival commits");
    let error = run
        .report_outcome(&mut session, &win_m01())
        .expect_err("the stale transition is refused");
    assert!(matches!(
        error,
        CampaignSaveError::Stale {
            expected: 0,
            stored: 7
        }
    ));
    assert_eq!(run.state().snapshot(), before, "the live run is unchanged");
    let held = read_snapshot(session.document().expect("doc"))
        .expect("readable")
        .expect("present");
    assert_eq!(held, rival, "the other writer's progression survives");
}

#[test]
fn accept_f43_c_a_damaged_or_foreign_save_is_refused_not_resumed() {
    // Another run's save is not adopted or overwritten.
    let base = TempBase::new("foreign");
    let (mut session, run, _id) = begun(base.path());
    let graph = lower_campaign(&declared_interlude_campaign()).expect("lowers");
    drop(run);
    let other = CampaignRunId::new("run.two").expect("run id");
    let error = CampaignRun::open(&mut session, graph, &profile(), &other, &difficulty())
        .expect_err("another run's save");
    assert!(matches!(error, CampaignSaveError::WrongRun { .. }));

    // A node the campaign does not declare is a corrupt save.
    let mut snapshot = read_snapshot(session.document().expect("doc"))
        .expect("readable")
        .expect("present");
    snapshot.current = key("nowhere");
    session
        .commit_with(|document| {
            write_snapshot(document, &snapshot).expect("encodes");
            Ok(())
        })
        .expect("committed");
    let error = open_run(&mut session).expect_err("an undeclared node");
    assert!(matches!(
        error,
        CampaignSaveError::Campaign(CampaignError::CorruptSnapshot { .. })
    ));

    // A garbled field is refused as damaged.
    session
        .commit_with(|document| {
            document.extra.push(ExtraField {
                key: "campaign.node.9".to_owned(),
                value: "m01;x;0;0;0;-".to_owned(),
            });
            Ok(())
        })
        .expect("committed");
    let error = open_run(&mut session).expect_err("a garbled counter");
    assert!(matches!(error, CampaignSaveError::Corrupt(_)));
}
