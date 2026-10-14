//! F29-C.3b acceptance: the kill-score ledger is called by the mission's own
//! host, and a finished run hands its score to both outcome records
//! (`specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-C`, task #704).
//!
//! Spec: `specs/F29-*.md` section `### F29-C`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md` — results are always
//! generation-qualified, and an outcome transaction applies at most once per
//! `OutcomeId`. Task test prefix: `accept_f29_c_3b_`. Design and the
//! follow-ups it leaves are in
//! `docs/findings/2026-10-14-f29-c-3b-score-wiring.md`.
//!
//! Three wirings, one test each, all driving production code:
//!
//! * [`drive_damage_schedule`] hands a resolved batch to the session's
//!   [`SessionScoreLedger`] — a kill resolved during a session moves the
//!   score **once**, and a replayed batch moves nothing;
//! * a settled run reads that ledger back onto its
//!   [`MissionTerminal::score`], and `cs_app::score`'s builders hand it to
//!   [`MissionOutcome::score`] and [`MissionResult::score`], which the
//!   reconciled entry point `cs_sim::mission_records::apply_mission_record`
//!   and `scrapbook::record_mission` then pay **once per `OutcomeId`**;
//! * a restart opens a fresh ledger, so the rebuilt session starts from zero.
//!
//! Removing any one of those calls fails its test: no batch reaches the
//! ledger (score stays 0 on the terminal and in both records), the terminal
//! carries 0 instead of the recorded tally, or the second generation inherits
//! the first one's score.
//!
//! Every value here is newly authored synthetic fixture data (the harbor
//! stage `vs_m01_rt_window` builds) — never original game data — and the
//! points are the engine's designed `KillScoreValue`
//! (`f29-c3.designed-kill-score`, one per kill): the original's per-kill
//! scoring numbers are unmeasured (F29 "Research boundary").

use std::path::Path;

use cs_app::campaign::lower_campaign;
use cs_app::damage::{DamageScheduleReport, PendingDamageEvents, drive_damage_schedule};
use cs_app::mission_session::{
    MissionHost, MissionHostRestartReport, MissionHostRestartRequest, MissionStage,
    MissionTerminal, build_headless, teardown,
};
use cs_app::score::{SessionScoreLedger, mission_outcome, mission_result};
use cs_app::ui::scrapbook::record_mission;
use cs_app::weapons::SessionDamageResolver;
use cs_content::campaign::declared_synthetic_campaign;
use cs_script::ir::{Action, Condition, IR_VERSION, MissionProgram, Objective, Outcome, SymbolId};
use cs_sim::campaign::{
    CampaignNodeKey, CampaignRunId, CampaignState, DifficultyId, EventKey, OutcomeAuthority,
    OutcomeId, OutcomeReceipt, ProfileId, SessionGeneration,
};
use cs_sim::damage::{
    AttributionRule, DamageChannel, DamageNodeKey, DamagePolicy, DamageResolver, HitEvent,
    HitEventId, synthetic_airframe_graph,
};
use cs_sim::mission_records::{
    MissionRecordReceipt, MissionRecordSubmission, apply_mission_record,
};
use cs_sim::net_state::NetActorState;
use cs_sim::records::{RecordKey, RecordRule, ScrapbookRecords, Slot};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::{ActorAllocator, ActorId, SessionId};
use cs_types::space::{Quaternion, WorldPosition};

use crate::common::cid;
use crate::vs_m01_rt_window::{Scratch, synthetic_stage};

/// The resolver's producer stamp, as the session schedule allocates one.
const RESOLVER_PRODUCER: u32 = 1;
/// The tick the fixture hits resolve on.
const RESOLVED_TICK: u64 = 5;

/// The composed fixed loop steps twice per rendered frame, so one `update`
/// is enough to run a pushed batch through the installed schedule.
const UPDATES: usize = 16;

/// The composed synthetic stage's control program, settling `outcome` on its
/// first tick — the shape `vs_m01_rt_host_02` drives its funnel with, authored
/// here as this file's own synthetic content.
fn settling_program(outcome: Outcome) -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-f29-c-3b-score"),
        variables: Vec::new(),
        objectives: vec![Objective {
            id: SymbolId(0),
            content: cid(ContentKind::Objective, "synthetic-f29-c-3b-score-objective"),
            condition: Condition::Const(true),
            actions: vec![Action::Finish(outcome)],
            span: None,
        }],
    }
}

/// The synthetic stage whose run settles `outcome` on its first tick.
fn settling_stage(scratch: &Path, outcome: Outcome) -> MissionStage {
    let mut stage = synthetic_stage(scratch);
    stage.host.control = settling_program(outcome);
    stage
}

/// Drives the composed entry until the host settles, and answers with the
/// terminal it settled on.
fn run_until_terminal(app: &mut bevy::prelude::App) -> MissionTerminal {
    for _ in 0..UPDATES {
        app.update();
        if let Some(host) = app.world().get_resource::<MissionHost>()
            && let Some(terminal) = host.terminal()
        {
            return terminal.clone();
        }
    }
    panic!("the composed entry never settled the run");
}

/// Two distinct session-qualified actor ids (victim, attacker), from the real
/// allocator so each id carries its session exactly as the production path's
/// do.
fn actors(session: SessionId) -> (ActorId, ActorId) {
    let mut allocator = ActorAllocator::new(session);
    (
        allocator.allocate().expect("the serial space exists"),
        allocator.allocate().expect("the serial space exists"),
    )
}

/// One actor admitted to the session's score ledger at a fixture pose — the
/// spawn host's own admission (`SessionScoreLedger::admit`).
fn admit(ledger: &mut SessionScoreLedger, actor: ActorId) {
    let state = NetActorState::spawn(
        actor,
        WorldPosition::try_new([0.0, 100.0, 0.0]).expect("finite"),
        Quaternion::IDENTITY,
        400,
    );
    ledger.admit(state).expect("the actor is admitted once");
}

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("the fixture node keys are valid")
}

/// One hit of the F29-A/AC01 pair: individually lethal on the synthetic
/// hull, attributed to `attacker`.
fn hit(
    session: SessionId,
    producer: u32,
    attacker: ActorId,
    target: ActorId,
    sequence: u32,
) -> HitEvent {
    HitEvent::try_new(
        HitEventId {
            session,
            tick: Tick(RESOLVED_TICK),
            producer,
            sequence,
        },
        Some(attacker),
        target,
        key("hull"),
        DamageChannel::Internal,
        50.0,
    )
    .expect("the fixture hit is valid")
}

/// Two individually lethal hits on one tick, resolved by the real resolver:
/// exactly one `KillAwarded`, beside the hits the batch carries.
struct KillBatch {
    hits: Vec<HitEvent>,
    events: Vec<cs_sim::damage::DamageEvent>,
}

/// The session's damage authority resolves the AC01 pair itself: the batch a
/// producer publishes beside the authority the schedule serves, so the test
/// installs exactly what the running session's producer would have left
/// behind.
fn resolve_kill(
    session: SessionId,
    victim: ActorId,
    attacker: ActorId,
) -> (KillBatch, DamageResolver) {
    let mut resolver = DamageResolver::new(session, RESOLVER_PRODUCER);
    resolver
        .register_actor(
            victim,
            synthetic_airframe_graph(),
            DamagePolicy {
                attribution: AttributionRule::FirstLethalHit,
            },
        )
        .expect("the synthetic actor registers");
    // Fed out of id order on purpose: the declared ordering rule decides the
    // blow, not the input order.
    let hits = vec![
        hit(session, RESOLVER_PRODUCER, attacker, victim, 0),
        hit(session, RESOLVER_PRODUCER + 4, attacker, victim, 0),
    ];
    let resolution = resolver
        .resolve(Tick(RESOLVED_TICK), &hits)
        .expect("valid batch");
    (
        KillBatch {
            hits,
            events: resolution.events,
        },
        resolver,
    )
}

/// The score one designed kill is worth, read back off the ledger the
/// session actually scores with.
fn designed_points(app: &bevy::prelude::App) -> i64 {
    app.world()
        .resource::<SessionScoreLedger>()
        .ledger()
        .kill_score()
        .points()
}

/// Pushes `batch` where the running session's producers publish theirs.
fn publish(app: &mut bevy::prelude::App, batch: &KillBatch) {
    app.world_mut().init_resource::<PendingDamageEvents>();
    app.world_mut()
        .resource_mut::<PendingDamageEvents>()
        .push(batch.hits.clone(), batch.events.clone());
}

/// **A kill resolved during a session moves the score exactly once: the
/// damage schedule hands its batch to the session's ledger, and a replayed
/// batch is absorbed.**
///
/// This is wiring 1 of the task: `drive_damage_schedule` — the consumer the
/// F29-C session schedule installs in the composed mission — is the host that
/// applies a resolution, and `SessionScoreLedger::apply_kill_awards` is the
/// call it makes. Delete that call and this member fails three times over:
/// `report.score` stays `None`, the tally never moves, and the replay has
/// nothing to absorb.
#[test]
fn accept_f29_c_3b_a_kill_resolved_during_a_session_moves_the_score_once() {
    let scratch = Scratch::new("f29-c-3b-schedule");
    let stage = synthetic_stage(scratch.path());
    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    let served = app.world().resource::<MissionHost>().served();

    // The composition opened this session's ledger: without it nothing scores.
    assert_eq!(
        app.world()
            .get_resource::<SessionScoreLedger>()
            .map(SessionScoreLedger::session),
        Some(served),
        "the composed mission opens a score ledger for the session it serves"
    );
    let (victim, attacker) = actors(served);
    {
        let mut ledger = app.world_mut().resource_mut::<SessionScoreLedger>();
        admit(&mut ledger, victim);
        admit(&mut ledger, attacker);
    }
    let points = designed_points(&app);

    // The session's own authority resolves the AC01 pair, and is installed
    // the way the running session installs it — the batch is what its
    // producers publish.
    let (batch, resolver) = resolve_kill(served, victim, attacker);
    app.world_mut()
        .insert_resource(SessionDamageResolver::new(resolver));
    publish(&mut app, &batch);

    // The production consumer run, exactly as the installed schedule runs it.
    drive_damage_schedule(app.world_mut());

    let report = app.world().resource::<DamageScheduleReport>();
    let scored = report
        .score
        .as_ref()
        .expect("the session's score consumer ran for this batch");
    assert_eq!(
        scored.awarded(),
        1,
        "the resolved kill scores once: {scored:?}"
    );
    assert_eq!(
        scored.refusals(),
        &[],
        "nothing about this batch is refused"
    );
    assert_eq!(scored.points(), points, "the designed value is what moved");
    assert_eq!(
        app.world().resource::<SessionScoreLedger>().score(),
        points,
        "the session's score is the kill's value"
    );
    assert_eq!(
        app.world()
            .resource::<SessionScoreLedger>()
            .ledger()
            .scored_kills(),
        1,
        "exactly one kill is scored"
    );

    // The identical batch again — a replayed tick, a retransmitted report.
    publish(&mut app, &batch);
    drive_damage_schedule(app.world_mut());
    let replay = app
        .world()
        .resource::<DamageScheduleReport>()
        .score
        .clone()
        .expect("the score consumer ran again");
    assert_eq!(
        replay.awarded(),
        0,
        "a replayed batch must not score a second time"
    );
    assert_eq!(
        replay.already_awarded(),
        1,
        "the repeat is absorbed by name"
    );
    assert_eq!(replay.points(), 0, "and it pays nothing");
    assert_eq!(
        app.world().resource::<SessionScoreLedger>().score(),
        points,
        "the score does not move on a replay"
    );

    teardown(&mut app);
}

/// **A settled run's outcome carries the score the session recorded, exactly
/// once per `OutcomeId`: a replayed outcome pays neither store a second
/// time.**
///
/// This is wiring 2: the terminal reads the ledger back, `cs_app::score`
/// hands that number to `campaign::MissionOutcome::score` and
/// `ui::scrapbook::MissionResult::score`, and the two production entry
/// points — `apply_mission_record` and `scrapbook::record_mission` — apply it
/// once and answer `AlreadyApplied` for the replay. Build the records with a
/// `score` of `0` instead and both store assertions fail.
#[test]
fn accept_f29_c_3b_the_run_carries_its_score_once_per_outcome_id() {
    let scratch = Scratch::new("f29-c-3b-outcome");
    let stage = settling_stage(scratch.path(), Outcome::Succeeded);
    let mut app = build_headless(&stage).expect("the settling synthetic stage composes");
    let served = app.world().resource::<MissionHost>().served();

    let (victim, attacker) = actors(served);
    {
        let mut ledger = app.world_mut().resource_mut::<SessionScoreLedger>();
        admit(&mut ledger, victim);
        admit(&mut ledger, attacker);
    }
    let points = designed_points(&app);
    assert!(points > 0, "the designed kill value awards something");
    let (batch, resolver) = resolve_kill(served, victim, attacker);
    app.world_mut()
        .insert_resource(SessionDamageResolver::new(resolver));
    publish(&mut app, &batch);

    // The composed entry runs the installed damage schedule before it settles
    // (`install_mission_host` orders the host tick behind it), so the terminal
    // reads back what this run's kill recorded.
    let terminal = run_until_terminal(&mut app);
    assert_eq!(
        terminal.score, points,
        "the settled run carries the score its own ledger recorded"
    );
    assert_eq!(
        terminal.score,
        app.world().resource::<SessionScoreLedger>().score(),
        "the terminal's score is the ledger's, read back and not recomputed"
    );

    // Both outcome records, through the two production builders.
    let outcome_id = OutcomeId {
        profile: ProfileId::new("pilot.nathan").expect("valid profile"),
        run: CampaignRunId::new("run.one").expect("valid run"),
        session: SessionGeneration(served.get() as u32),
        terminal_event: EventKey {
            session: SessionGeneration(served.get() as u32),
            tick: terminal.tick,
            source: SymbolId(0),
            sequence: 0,
        },
    };
    let node = CampaignNodeKey::new("m01").expect("the fixture node key");
    let campaign_outcome = mission_outcome(
        &terminal,
        outcome_id.clone(),
        node.clone(),
        OutcomeAuthority::Authorized,
    )
    .expect("a settled success builds its campaign record");
    assert_eq!(
        campaign_outcome.score,
        u64::try_from(points).expect("a non-negative tally"),
        "campaign::MissionOutcome::score is the run's own score"
    );
    assert_eq!(campaign_outcome.outcome, Outcome::Succeeded);

    let difficulty = DifficultyId::new("standard").expect("valid difficulty");
    let mission = ContentId::from_source(ContentKind::Mission, "synthetic.m01")
        .expect("the fixture mission id");
    let record = mission_result(
        &terminal,
        outcome_id.clone(),
        mission.clone(),
        difficulty.clone(),
        RecordRule::DEFAULT,
    )
    .expect("a settled success builds its scrapbook record");
    assert_eq!(
        record.score,
        u64::try_from(points).expect("a non-negative tally"),
        "ui::scrapbook::MissionResult::score is the run's own score"
    );

    // The reconciled production entry point: one submission moves both
    // stores, under the score the record carries.
    let graph = lower_campaign(&declared_synthetic_campaign()).expect("the fixture lowers");
    let mut state = CampaignState::begin(
        ProfileId::new("pilot.nathan").expect("valid profile"),
        CampaignRunId::new("run.one").expect("valid run"),
        difficulty.clone(),
        &graph,
    );
    let mut book = ScrapbookRecords::default();
    let submission = || MissionRecordSubmission {
        outcome: &campaign_outcome,
        mission: &mission,
        rule: RecordRule::DEFAULT,
    };
    let MissionRecordReceipt {
        campaign,
        record: applied,
    } = apply_mission_record(&mut state, &graph, &mut book, submission())
        .expect("the finished run's record commits");
    let OutcomeReceipt::Applied(plan) = campaign.receipt else {
        panic!("the first submission applies, got {campaign:?}")
    };
    assert!(plan.best_score_raised, "the first victory establishes best");
    assert_eq!(
        state.progress(&node).expect("the node recorded").best_score,
        u64::try_from(points).expect("a non-negative tally"),
        "the campaign store recorded the run's score"
    );
    assert_eq!(
        slot(&book, &mission).best.score,
        u64::try_from(points).expect("a non-negative tally"),
        "the scrapbook recorded the run's score"
    );
    assert!(
        matches!(applied, cs_sim::records::RecordReceipt::Applied { .. }),
        "the book applied the first submission, got {applied:?}"
    );

    // A replayed outcome — the crash-recovery shape — pays neither store again.
    let MissionRecordReceipt {
        campaign: replayed,
        record: replayed_record,
    } = apply_mission_record(&mut state, &graph, &mut book, submission())
        .expect("a replay is a no-op, not a refusal");
    assert!(
        matches!(replayed.receipt, OutcomeReceipt::AlreadyApplied),
        "the campaign pays nothing the second time: {replayed:?}"
    );
    assert!(
        matches!(
            replayed_record,
            cs_sim::records::RecordReceipt::AlreadyApplied
        ),
        "the book pays nothing the second time: {replayed_record:?}"
    );
    assert_eq!(
        state.progress(&node).expect("the node recorded").best_score,
        u64::try_from(points).expect("a non-negative tally"),
        "a replayed outcome cannot raise the record"
    );
    assert_eq!(
        state.progress(&node).expect("the node recorded").victories,
        1,
        "a replayed outcome cannot count as a second victory"
    );
    assert_eq!(
        slot(&book, &mission).best.score,
        u64::try_from(points).expect("a non-negative tally"),
        "a replayed outcome cannot move the book either"
    );

    // The scrapbook's own entry point, over the same MissionResult.
    let mut records = ScrapbookRecords::default();
    let receipt =
        record_mission(&mut records, &record).expect("the scrapbook accepts the run's record");
    assert!(
        matches!(receipt, cs_sim::records::RecordReceipt::Applied { .. }),
        "the first submission applies: {receipt:?}"
    );
    assert_eq!(
        slot(&records, &mission).best.score,
        u64::try_from(points).expect("a non-negative tally")
    );
    let replayed =
        record_mission(&mut records, &record).expect("a replayed result is a no-op, not a refusal");
    assert!(
        matches!(replayed, cs_sim::records::RecordReceipt::AlreadyApplied),
        "a replayed result writes nothing: {replayed:?}"
    );
    assert_eq!(
        slot(&records, &mission).best.score,
        u64::try_from(points).expect("a non-negative tally")
    );

    teardown(&mut app);
}

/// **A restarted session starts from zero: the fresh generation's ledger
/// carries no score of the generation it replaced.**
#[test]
fn accept_f29_c_3b_a_restarted_session_starts_from_zero() {
    let scratch = Scratch::new("f29-c-3b-restart");
    let stage = synthetic_stage(scratch.path());
    let mut app = build_headless(&stage).expect("the synthetic stage composes");

    // Generation one scores a kill.
    let served = app.world().resource::<MissionHost>().served();
    let (victim, attacker) = actors(served);
    {
        let mut ledger = app.world_mut().resource_mut::<SessionScoreLedger>();
        admit(&mut ledger, victim);
        admit(&mut ledger, attacker);
    }
    let points = designed_points(&app);
    let (batch, resolver) = resolve_kill(served, victim, attacker);
    app.world_mut()
        .insert_resource(SessionDamageResolver::new(resolver));
    publish(&mut app, &batch);
    app.update();
    assert_eq!(
        app.world().resource::<SessionScoreLedger>().score(),
        points,
        "the first generation scored its kill"
    );

    // A restart: the composed entry rebuilds the authored initial state under
    // a fresh generation, and the score ledger is rebuilt with it.
    app.world_mut().insert_resource(MissionHostRestartRequest);
    app.update();
    let report = app
        .world_mut()
        .remove_resource::<MissionHostRestartReport>()
        .expect("the composed restart ran");
    assert_ne!(
        report.served, served,
        "the restart mints a new session generation"
    );
    let ledger = app.world().resource::<SessionScoreLedger>();
    assert_eq!(
        ledger.session(),
        report.served,
        "the fresh generation's ledger scores the fresh session"
    );
    assert_eq!(
        ledger.score(),
        0,
        "a restarted session starts from zero: the torn-down tally is gone"
    );
    assert_eq!(
        ledger.ledger().scored_kills(),
        0,
        "and no destruction record of the old generation carries over"
    );

    // The rebuilt session's own kill scores exactly one kill's worth again.
    let fresh = report.served;
    let (victim, attacker) = actors(fresh);
    {
        let mut ledger = app.world_mut().resource_mut::<SessionScoreLedger>();
        admit(&mut ledger, victim);
        admit(&mut ledger, attacker);
    }
    let (batch, resolver) = resolve_kill(fresh, victim, attacker);
    app.world_mut()
        .insert_resource(SessionDamageResolver::new(resolver));
    publish(&mut app, &batch);
    app.update();
    assert_eq!(
        app.world().resource::<SessionScoreLedger>().score(),
        points,
        "the rebuilt session scores its own kill once — not the old tally plus a new one"
    );

    // The app idles one frame after the assertion so the run ends tidy.
    app.update();
    teardown(&mut app);
}

/// The scrapbook slot one subject is recorded under.
fn slot<'a>(book: &'a ScrapbookRecords, mission: &ContentId) -> &'a Slot {
    book.records
        .slot(&RecordKey {
            subject: mission.clone(),
            difficulty: None,
        })
        .expect("the run recorded this mission")
}
