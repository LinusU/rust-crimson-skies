//! Acceptance scenarios for F43-V2: the campaign best/latest store and the
//! F47-A scrapbook `RecordRule` direction reconciled into one production
//! path (task #626).
//!
//! Spec: `specs/F43-campaign-progression-outcomes-and-economy-rules.md`
//! (F43-A/F43-B pins) and
//! `specs/F47-scrapbook-records-mementos-and-mission-replay.md` (F47-A).
//! Task test prefix: `accept_f43_v2_`. Decision record:
//! `docs/findings/2026-10-13-f43-v2-campaign-scrapbook-record-reconciliation.md`.
//!
//! Every test drives `cs_sim::mission_records::apply_mission_record` — the
//! production entry point — and pins one part of the reconciliation:
//!
//! * one outcome moves **both** stores;
//! * a worse replay preserves best and records latest in both;
//! * a `BetterIs::Lower` subject agrees in both stores (the campaign side
//!   cannot keep a bare `max`);
//! * a campaign-ledger hit (the crash shape) still finishes the scrapbook
//!   half, and a complete replay is a no-op in both;
//! * a refusal touches neither store;
//! * the difficulty scope lives in the profile-level book, not in the
//!   run-scoped campaign state.
//!
//! All fixture data is newly authored synthetic data, never original game
//! data.

use cs_sim::campaign::{
    CampaignGraph, CampaignNodeKey, CampaignRunId, CampaignState, DifficultyId, EventKey,
    MissionOutcome, Outcome, OutcomeAuthority, OutcomeId, OutcomeReceipt, ProfileId, Reward,
    RuntimeEdge, RuntimeNode, RuntimeNodeKind, SessionGeneration, SymbolId,
};
use cs_sim::mission_records::{
    MissionRecordError, MissionRecordReceipt, MissionRecordSubmission, apply_mission_record,
};
use cs_sim::records::{
    BestChange, BetterIs, DifficultyScope, RecordKey, RecordReceipt, RecordRule, ScrapbookRecords,
    Slot,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

fn mission(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Mission, key).expect("synthetic mission id")
}

fn node(key: &str) -> CampaignNodeKey {
    CampaignNodeKey::new(key).expect("synthetic node key")
}

fn key_of(key: &str) -> RecordKey {
    RecordKey {
        subject: mission(key),
        difficulty: None,
    }
}

fn per_difficulty(key: &str, difficulty: &DifficultyId) -> RecordKey {
    RecordKey {
        subject: mission(key),
        difficulty: Some(difficulty.clone()),
    }
}

fn outcome(
    run: &str,
    session: u32,
    tick: u64,
    sequence: u32,
    at: &str,
    kind: Outcome,
    score: u64,
) -> MissionOutcome {
    MissionOutcome {
        id: OutcomeId {
            profile: ProfileId::new("pilot.nathan").expect("valid"),
            run: CampaignRunId::new(run).expect("valid"),
            session: SessionGeneration(session),
            terminal_event: EventKey {
                session: SessionGeneration(session),
                tick: Tick(tick),
                source: SymbolId(3),
                sequence,
            },
        },
        node: node(at),
        outcome: kind,
        score,
        authority: OutcomeAuthority::Authorized,
    }
}

/// A two-mission synthetic campaign: `m01` and `m02`, both leading to the
/// `end` ending on victory. `m01` is the entry.
fn graph() -> CampaignGraph {
    let mission_edge = |to: &str, currency: u64| RuntimeEdge {
        on: Outcome::Succeeded,
        to: node(to),
        grant: Reward {
            currency,
            unlocks: Vec::new(),
        },
    };
    let retry_edge = RuntimeEdge {
        on: Outcome::Failed,
        to: node("m01"),
        grant: Reward::default(),
    };
    let abort_edge = RuntimeEdge {
        on: Outcome::Aborted,
        to: node("m01"),
        grant: Reward::default(),
    };
    CampaignGraph::try_new(
        vec![
            RuntimeNode {
                id: node("m01"),
                kind: RuntimeNodeKind::Mission {
                    mission: mission("m01"),
                },
                edges: vec![
                    mission_edge("m02", 500),
                    retry_edge.clone(),
                    abort_edge.clone(),
                ],
            },
            RuntimeNode {
                id: node("m02"),
                kind: RuntimeNodeKind::Mission {
                    mission: mission("m02"),
                },
                edges: vec![mission_edge("end", 750), retry_edge, abort_edge],
            },
            RuntimeNode {
                id: node("end"),
                kind: RuntimeNodeKind::Ending,
                edges: Vec::new(),
            },
        ],
        node("m01"),
        Vec::new(),
    )
    .expect("the synthetic graph validates")
}

fn fresh_run(difficulty: &str) -> (CampaignGraph, CampaignState, ScrapbookRecords) {
    let graph = graph();
    let difficulty = DifficultyId::new(difficulty).expect("valid difficulty");
    let state = CampaignState::begin(
        ProfileId::new("pilot.nathan").expect("valid"),
        CampaignRunId::new("run.one").expect("valid"),
        difficulty,
        &graph,
    );
    (graph, state, ScrapbookRecords::default())
}

fn submit<'a>(
    outcome: &'a MissionOutcome,
    mission: &'a ContentId,
    rule: RecordRule,
) -> MissionRecordSubmission<'a> {
    MissionRecordSubmission {
        outcome,
        mission,
        rule,
    }
}

fn slot<'a>(book: &'a ScrapbookRecords, key: &RecordKey) -> &'a Slot {
    book.records.slot(key).expect("the record exists")
}

/// One outcome moves **both** stores: the campaign's per-node record and
/// the profile-level scrapbook slot, under one declared rule.
#[test]
fn accept_f43_v2_one_outcome_moves_both_stores() {
    let (graph, mut state, mut book) = fresh_run("standard");
    let victory = outcome("run.one", 1, 480, 0, "m01", Outcome::Succeeded, 900);

    let receipt = apply_mission_record(
        &mut state,
        &graph,
        &mut book,
        submit(&victory, &mission("m01"), RecordRule::DEFAULT),
    )
    .expect("the first submission commits");
    let MissionRecordReceipt { campaign, record } = receipt;
    let OutcomeReceipt::Applied(plan) = campaign.receipt else {
        panic!("the campaign half must apply, got {campaign:?}")
    };
    assert!(
        plan.best_score_raised,
        "the first victory establishes the campaign best"
    );
    assert_eq!(plan.currency_delta, 500, "the declared grant is paid once");
    let progress = state.progress(&node("m01")).expect("recorded");
    assert_eq!(progress.victories, 1);
    assert_eq!(progress.best_score, 900, "the campaign store recorded best");
    assert_eq!(
        progress.latest,
        Some((Outcome::Succeeded, 900)),
        "the campaign store recorded latest"
    );
    assert_eq!(
        record,
        RecordReceipt::Applied {
            key: key_of("m01"),
            best: BestChange::First,
        },
        "the scrapbook store recorded the run too"
    );
    let slot = slot(&book, &key_of("m01"));
    assert_eq!(slot.best.score, 900, "the scrapbook store recorded best");
    assert_eq!(
        slot.latest.score, 900,
        "the scrapbook store recorded latest"
    );
    assert_eq!(
        slot.best.outcome, victory.id,
        "the record names the outcome that produced it"
    );
}

/// F43 AC02 on the reconciled path: a worse replay records `latest` and
/// preserves `best` — identically, in both stores.
#[test]
fn accept_f43_v2_worse_replay_preserves_best_in_both_stores() {
    let (graph, mut state, mut book) = fresh_run("standard");
    let win = outcome("run.one", 1, 480, 0, "m01", Outcome::Succeeded, 900);
    apply_mission_record(
        &mut state,
        &graph,
        &mut book,
        submit(&win, &mission("m01"), RecordRule::DEFAULT),
    )
    .expect("victory applies");

    // Replay m01 in a later session with a worse score. The run has moved
    // on (current is m02), but m01 was visited, so the replay is eligible.
    let replay = outcome("run.one", 2, 300, 0, "m01", Outcome::Succeeded, 400);
    let receipt = apply_mission_record(
        &mut state,
        &graph,
        &mut book,
        submit(&replay, &mission("m01"), RecordRule::DEFAULT),
    )
    .expect("a new outcome id applies");
    let OutcomeReceipt::Applied(plan) = receipt.campaign.receipt else {
        panic!("a new outcome id applies, got {:?}", receipt.campaign)
    };
    assert!(!plan.best_score_raised, "the worse replay raises nothing");

    let progress = state.progress(&node("m01")).expect("recorded");
    assert_eq!(progress.best_score, 900, "the campaign store keeps best");
    assert_eq!(
        progress.latest,
        Some((Outcome::Succeeded, 400)),
        "the campaign store records latest"
    );
    let slot = slot(&book, &key_of("m01"));
    assert_eq!(slot.best.score, 900, "the scrapbook store keeps best");
    assert_eq!(
        slot.best.outcome, win.id,
        "the earlier run keeps the best record"
    );
    assert_eq!(slot.latest.score, 400, "the scrapbook store records latest");
    assert_eq!(
        slot.latest.outcome, replay.id,
        "the replay is the latest run"
    );
    assert_eq!(
        progress.best_score, slot.best.score,
        "the two stores cannot disagree about the best score"
    );
}

/// The core direction reconciliation: a `BetterIs::Lower` subject (a time,
/// say) makes **both** stores keep the smallest score as best. The campaign
/// side must not keep its old bare `max`.
#[test]
fn accept_f43_v2_lower_is_better_agrees_across_stores() {
    let (graph, mut state, mut book) = fresh_run("standard");
    let rule = RecordRule {
        better: BetterIs::Lower,
        scope: DifficultyScope::AllDifficulties,
    };

    let first = outcome("run.one", 1, 600, 0, "m01", Outcome::Succeeded, 120);
    apply_mission_record(
        &mut state,
        &graph,
        &mut book,
        submit(&first, &mission("m01"), rule),
    )
    .expect("the first run applies");
    assert_eq!(
        state.progress(&node("m01")).expect("recorded").best_score,
        120,
        "the first victory establishes the campaign record under the rule"
    );
    assert_eq!(slot(&book, &key_of("m01")).best.score, 120);

    // A faster run is the better one.
    let faster = outcome("run.one", 2, 610, 0, "m01", Outcome::Succeeded, 90);
    let receipt = apply_mission_record(
        &mut state,
        &graph,
        &mut book,
        submit(&faster, &mission("m01"), rule),
    )
    .expect("the faster run applies");
    let OutcomeReceipt::Applied(plan) = receipt.campaign.receipt else {
        panic!("a new outcome id applies, got {:?}", receipt.campaign)
    };
    assert!(
        plan.best_score_raised,
        "under a lower-is-better rule a smaller score raises best"
    );
    assert_eq!(
        state.progress(&node("m01")).expect("recorded").best_score,
        90,
        "the campaign store keeps the smallest score as best"
    );
    assert_eq!(
        slot(&book, &key_of("m01")).best.score,
        90,
        "the scrapbook store keeps the smallest score as best"
    );

    // A slower run records latest only.
    let slower = outcome("run.one", 3, 620, 0, "m01", Outcome::Succeeded, 110);
    apply_mission_record(
        &mut state,
        &graph,
        &mut book,
        submit(&slower, &mission("m01"), rule),
    )
    .expect("the slower run applies");
    let progress = state.progress(&node("m01")).expect("recorded");
    assert_eq!(progress.best_score, 90, "the campaign best survives");
    assert_eq!(progress.latest, Some((Outcome::Succeeded, 110)));
    let slot = slot(&book, &key_of("m01"));
    assert_eq!(slot.best.score, 90, "the scrapbook best survives");
    assert_eq!(slot.latest.score, 110);
}

/// The crash shape: the campaign half committed, the scrapbook half did not.
/// Replaying the same outcome reports `AlreadyApplied` on the campaign side
/// — and still finishes the scrapbook half, because the book's own ledger
/// makes a complete replay a no-op.
#[test]
fn accept_f43_v2_campaign_ledger_hit_repairs_the_scrapbook() {
    let (graph, mut state, mut book) = fresh_run("standard");
    let victory = outcome("run.one", 1, 480, 0, "m01", Outcome::Succeeded, 900);

    // The campaign half applied without the scrapbook (a crash between the
    // two persistence commits, or a pre-F43-V2 caller).
    state
        .apply_outcome_with_rule(&graph, &victory, RecordRule::DEFAULT)
        .expect("the campaign half applies");
    assert!(
        book.records.is_empty(),
        "the scrapbook half is missing — the crash shape"
    );
    let revision = state.revision();

    // Replay: the campaign ledger refuses to double-apply, the book records
    // the run the first application never reached.
    let receipt = apply_mission_record(
        &mut state,
        &graph,
        &mut book,
        submit(&victory, &mission("m01"), RecordRule::DEFAULT),
    )
    .expect("the replay is a valid call");
    assert_eq!(
        receipt.campaign.receipt,
        OutcomeReceipt::AlreadyApplied,
        "the campaign store does not double-apply"
    );
    assert_eq!(
        receipt.record,
        RecordReceipt::Applied {
            key: key_of("m01"),
            best: BestChange::First,
        },
        "the replay finishes the scrapbook half"
    );
    assert_eq!(state.revision(), revision, "no second campaign revision");
    assert_eq!(
        state.progress(&node("m01")).expect("recorded").victories,
        1,
        "no second campaign application"
    );

    // A third replay — both stores now complete — changes nothing at all.
    let again = apply_mission_record(
        &mut state,
        &graph,
        &mut book,
        submit(&victory, &mission("m01"), RecordRule::DEFAULT),
    )
    .expect("the replay is a valid call");
    assert_eq!(again.campaign.receipt, OutcomeReceipt::AlreadyApplied);
    assert_eq!(again.record, RecordReceipt::AlreadyApplied);
    assert_eq!(state.revision(), revision);
    assert_eq!(state.progress(&node("m01")).expect("recorded").victories, 1);
    let slot = slot(&book, &key_of("m01"));
    assert_eq!(slot.best.outcome, victory.id);
    assert_eq!(slot.latest.outcome, victory.id);
}

/// A refusal changes nothing — in either store. The submission that names
/// the wrong subject is refused before anything is filed under it.
#[test]
fn accept_f43_v2_refused_outcome_touches_neither_store() {
    let (graph, mut state, mut book) = fresh_run("standard");
    let untouched_state = state.clone();
    let untouched_book = book.clone();

    // m02 exists but was neither selected nor visited.
    let ineligible = outcome("run.one", 1, 100, 0, "m02", Outcome::Succeeded, 500);
    let error = apply_mission_record(
        &mut state,
        &graph,
        &mut book,
        submit(&ineligible, &mission("m02"), RecordRule::DEFAULT),
    )
    .expect_err("an unvisited node cannot report results");
    assert!(
        matches!(error, MissionRecordError::Campaign(_)),
        "the campaign refusal is reported, got {error}"
    );
    assert_eq!(state, untouched_state, "the campaign store is untouched");
    assert_eq!(book, untouched_book, "the scrapbook store is untouched too");

    // A valid outcome filed under the wrong mission would poison every
    // unlock predicate that reads the subject, so it is refused.
    let win = outcome("run.one", 2, 200, 0, "m01", Outcome::Succeeded, 900);
    let error = apply_mission_record(
        &mut state,
        &graph,
        &mut book,
        submit(&win, &mission("m02"), RecordRule::DEFAULT),
    )
    .expect_err("the subject must be the node's bound mission");
    assert!(
        matches!(error, MissionRecordError::SubjectMismatch { .. }),
        "the mismatch is reported, got {error}"
    );
    assert_eq!(state, untouched_state, "the campaign store is untouched");
    assert_eq!(book, untouched_book, "the scrapbook store is untouched too");
}

/// The difficulty scope is honored where runs meet: the profile-level book
/// keeps one slot per difficulty (`PerDifficulty`) or one shared slot
/// (`AllDifficulties`), while each run's campaign state keeps its own
/// per-run record.
#[test]
fn accept_f43_v2_difficulty_scope_lives_in_the_book() {
    let graph = graph();
    let standard = DifficultyId::new("standard").expect("valid");
    let ace = DifficultyId::new("ace").expect("valid");
    let mut book = ScrapbookRecords::default();
    let run = |run: &str, difficulty: &DifficultyId| {
        CampaignState::begin(
            ProfileId::new("pilot.nathan").expect("valid"),
            CampaignRunId::new(run).expect("valid"),
            difficulty.clone(),
            &graph,
        )
    };
    let per = RecordRule {
        better: BetterIs::Higher,
        scope: DifficultyScope::PerDifficulty,
    };

    let standard_win = outcome("run.standard", 1, 100, 0, "m01", Outcome::Succeeded, 900);
    let ace_win = outcome("run.ace", 1, 100, 0, "m01", Outcome::Succeeded, 400);

    let mut standard_run = run("run.standard", &standard);
    apply_mission_record(
        &mut standard_run,
        &graph,
        &mut book,
        submit(&standard_win, &mission("m01"), per),
    )
    .expect("the standard run applies");
    let mut ace_run = run("run.ace", &ace);
    apply_mission_record(
        &mut ace_run,
        &graph,
        &mut book,
        submit(&ace_win, &mission("m01"), per),
    )
    .expect("the ace run applies");

    assert_eq!(
        slot(&book, &per_difficulty("m01", &standard)).best.score,
        900,
        "the standard difficulty keeps its own slot"
    );
    assert_eq!(
        slot(&book, &per_difficulty("m01", &ace)).best.score,
        400,
        "the harder difficulty keeps its own slot"
    );
    assert_eq!(
        standard_run
            .progress(&node("m01"))
            .expect("recorded")
            .best_score,
        900,
        "each run's campaign record is its own, by construction"
    );
    assert_eq!(
        ace_run.progress(&node("m01")).expect("recorded").best_score,
        400,
        "each run's campaign record is its own, by construction"
    );
    assert_eq!(book.records.len(), 2, "per difficulty means two slots");

    // Under an all-difficulties rule the two runs share one slot.
    let mut shared_book = ScrapbookRecords::default();
    let mut standard_run = run("run.standard", &standard);
    apply_mission_record(
        &mut standard_run,
        &graph,
        &mut shared_book,
        submit(&standard_win, &mission("m01"), RecordRule::DEFAULT),
    )
    .expect("the standard run applies");
    let mut ace_run = run("run.ace", &ace);
    apply_mission_record(
        &mut ace_run,
        &graph,
        &mut shared_book,
        submit(&ace_win, &mission("m01"), RecordRule::DEFAULT),
    )
    .expect("the ace run applies");
    assert_eq!(
        shared_book.records.len(),
        1,
        "all difficulties means one shared slot"
    );
    let shared = slot(&shared_book, &key_of("m01"));
    assert_eq!(shared.best.score, 900, "the shared slot keeps the best run");
    assert_eq!(shared.latest.score, 400, "the shared slot's latest run");
}
