//! Acceptance scenario F56-C (simulation half): one `MatchSession` owns the
//! match's scoring, pickups and clock, is restarted without leaking any of
//! them into the next generation, and renders its sealed result into the
//! end-of-match record a results screen consumes.
//!
//! Spec: `specs/F56-original-multiplayer-scenarios-and-mode-rules.md`, stage
//! `### F56-C`; minimum scenario "Restart match and ensure no score, pickup
//! or timer leaks". Contract: `docs/contracts/UI-NETWORK.md`.
//!
//! Every test drives the production `cs_sim::multiplayer::session` API; the
//! limits, score table and rosters are authored test inputs, because the
//! original per-mode values are still unknown (F56-A finding).

use cs_sim::multiplayer::objective::{
    ObjectiveAction, ObjectiveError, ObjectiveEvent, ObjectiveId, ObjectiveState, Ruling,
    Transition, Verdict,
};
use cs_sim::multiplayer::result::{
    ConfigError, EndReason, LethalEvent, LethalKind, Limits, Outcome, Roster, ScoreTable, Side,
    SubmitError, VictoryRule,
};
use cs_sim::multiplayer::session::{
    MAX_OBJECTIVES, MatchSession, SessionConfig, SessionError, TickClose,
};
use cs_types::Tick;
use cs_types::content::ContentId;
use cs_types::net::{EventId, PeerId, SessionId};

fn peer(number: u16) -> PeerId {
    PeerId::new(number).expect("a nonzero peer number")
}

fn generation(number: u64) -> SessionId {
    SessionId::new(number).expect("a nonzero session number")
}

fn scenario() -> ContentId {
    ContentId::parse("multiplayer_scenario/slot.c1.mp2").expect("a scenario id")
}

fn event(session: SessionId, tick: u64, producer: u32, sequence: u32) -> EventId {
    EventId {
        session,
        tick: Tick(tick),
        producer,
        sequence,
    }
}

/// A free-for-all over two pilots, one point per down, ending at 100 ticks or
/// 2 points — authored inputs, not original rules.
fn config(session: SessionId) -> SessionConfig {
    SessionConfig {
        scenario: scenario(),
        session,
        roster: Roster::free_for_all(&[peer(1), peer(2)]).expect("two pilots"),
        table: ScoreTable {
            kill: 1,
            crash: 1,
            team_kill: -1,
        },
        limits: Limits {
            time_limit: Some(Tick(100)),
            score_limit: Some(2),
        },
        victory: VictoryRule::HighestScore,
        objectives: 2,
    }
}

fn kill(session: SessionId, tick: u64, sequence: u32, killer: u16, victim: u16) -> LethalEvent {
    LethalEvent {
        id: event(session, tick, 0, sequence),
        victim: peer(victim),
        kind: LethalKind::Kill {
            killer: peer(killer),
        },
    }
}

fn claim(session: SessionId, tick: u64, objective: ObjectiveId, claimant: u16) -> ObjectiveEvent {
    ObjectiveEvent {
        id: event(session, tick, 1, tick as u32),
        objective,
        action: ObjectiveAction::Claim {
            claimant: peer(claimant),
        },
    }
}

fn delivery(session: SessionId, tick: u64, objective: ObjectiveId) -> ObjectiveEvent {
    ObjectiveEvent {
        id: event(session, tick, 1, tick as u32),
        objective,
        action: ObjectiveAction::Score,
    }
}

/// The minimum scenario: a match is played to its result — points scored, an
/// objective picked up and delivered, ticks closed — then restarted, and the
/// new generation holds no score, no pickup and no timer of the old one.
#[test]
fn accept_f56_c_restart_leaves_no_score_pickup_or_timer_leak() {
    let mut played = MatchSession::start(config(generation(1))).expect("the match starts");
    let flag = played.objective_ids().next().expect("two objectives");

    // A point on the board, a pickup in hand and a clock that has moved.
    played
        .submit_lethal(kill(generation(1), 5, 0, 1, 2))
        .expect("queued");
    let close = played.close_tick(Tick(5));
    assert!(!close.ended, "two points are needed");
    played
        .submit_objective(claim(generation(1), 6, flag, 2))
        .expect("queued");
    played.close_tick(Tick(6));
    assert_eq!(
        played.objective_state(flag),
        Some(ObjectiveState::Held(peer(2))),
        "the pickup is in hand"
    );
    played
        .submit_objective(delivery(generation(1), 7, flag))
        .expect("queued");
    played.close_tick(Tick(7));
    assert_eq!(
        played.captures_of(flag).map(<[_]>::len),
        Some(1),
        "the delivery is on the ledger"
    );
    played.close_tick(Tick(60));
    played
        .submit_lethal(kill(generation(1), 61, 1, 1, 2))
        .expect("queued");
    let close = played.close_tick(Tick(61));
    assert!(close.ended, "two points end the match");
    let first = played.end_of_match().expect("the match ended");
    assert_eq!(first.session, generation(1));
    assert_eq!(first.captures.len(), 1, "the first match's delivery");

    // Restart: a new generation of the same match.
    played
        .restart(config(generation(2)))
        .expect("the restart is accepted");

    // Score leak: every side is back at zero and the old result is gone.
    assert_eq!(played.score(Side::Participant(peer(1))), Some(0));
    assert_eq!(played.score(Side::Participant(peer(2))), Some(0));
    assert_eq!(played.result(), None, "the new match has not ended");
    assert_eq!(
        played.end_of_match(),
        None,
        "the old match's record is not the new match's"
    );

    // Pickup leak: nothing is held and no ledger carries a delivery.
    for objective in played.objective_ids() {
        assert_eq!(
            played.objective_state(objective),
            Some(ObjectiveState::Home),
            "{objective} must start home"
        );
        assert_eq!(
            played.captures_of(objective).map(<[_]>::len),
            Some(0),
            "{objective} must start with an empty ledger"
        );
    }

    // Timer leak: the clock starts again, so ticks the old generation closed
    // are not this generation's.
    assert_eq!(played.clock(), None, "a fresh generation has no clock");

    // The old generation's packets are refused even though the objective ids
    // were allocated again: identity is the session, not the serial.
    assert!(
        matches!(
            played.submit_lethal(kill(generation(1), 70, 2, 1, 2)),
            Err(SessionError::Lethal(SubmitError::WrongSession { got }))
                if got == generation(1)
        ),
        "a replayed packet of the finished match must not score"
    );
    assert!(
        matches!(
            played.submit_objective(claim(generation(1), 70, flag, 2)),
            Err(SessionError::Objective(ObjectiveError::WrongSession { got }))
                if got == generation(1)
        ),
        "a replayed pickup request of the finished match must not apply"
    );

    // The new generation accepts its own events from tick 1 on.
    played
        .submit_objective(claim(generation(2), 1, flag, 1))
        .expect("the new generation's first pickup is not late");
    let TickClose { rulings, ended } = played.close_tick(Tick(1));
    assert_eq!(rulings.len(), 1, "the claim is judged");
    assert!(!ended);
    assert_eq!(played.clock(), Some(Tick(1)));
    assert_eq!(
        played.objective_state(flag),
        Some(ObjectiveState::Held(peer(1)))
    );
    assert_eq!(
        first.session,
        generation(1),
        "the finished match's record still names its own session"
    );
}

/// Teardown and retry: a refused restart changes nothing, so the caller can
/// correct the configuration and try again.
#[test]
fn accept_f56_c_a_refused_restart_changes_nothing_and_can_be_retried() {
    let mut played = MatchSession::start(config(generation(1))).expect("the match starts");
    let flag = played.objective_ids().next().expect("two objectives");
    played
        .submit_lethal(kill(generation(1), 5, 0, 1, 2))
        .expect("queued");
    played.close_tick(Tick(5));
    played
        .submit_objective(claim(generation(1), 6, flag, 2))
        .expect("queued");
    played.close_tick(Tick(6));

    // A restart into the running generation is refused: a restart is a new
    // session epoch, or the old packets would become the new match's.
    assert_eq!(
        played.restart(config(generation(1))),
        Err(SessionError::SameGeneration {
            session: generation(1)
        })
    );
    // A configuration the session cannot run is refused before anything is
    // torn down.
    let mut oversized = config(generation(2));
    oversized.objectives = MAX_OBJECTIVES + 1;
    assert_eq!(
        played.restart(oversized),
        Err(SessionError::TooManyObjectives {
            got: MAX_OBJECTIVES + 1,
            max: MAX_OBJECTIVES
        })
    );

    assert_eq!(
        played.score(Side::Participant(peer(1))),
        Some(1),
        "a refused restart leaves the running match's score alone"
    );
    assert_eq!(
        played.objective_state(flag),
        Some(ObjectiveState::Held(peer(2))),
        "a refused restart leaves the running match's pickup alone"
    );
    assert_eq!(
        played.clock(),
        Some(Tick(6)),
        "a refused restart leaves the running match's clock alone"
    );

    // The corrected configuration is accepted and starts from nothing.
    played
        .restart(config(generation(3)))
        .expect("the retry is accepted");
    assert_eq!(played.session(), generation(3));
    assert_eq!(played.score(Side::Participant(peer(1))), Some(0));
    assert_eq!(played.clock(), None);
    assert_eq!(played.objective_state(flag), Some(ObjectiveState::Home));
    assert_eq!(played.end_of_match(), None);
}

/// Error propagation at the start: a configuration the match cannot run is
/// refused by name, and no half-built session exists afterwards.
#[test]
fn accept_f56_c_a_configuration_the_match_cannot_run_is_refused_by_name() {
    let mut no_limits = config(generation(1));
    no_limits.limits = Limits {
        time_limit: None,
        score_limit: None,
    };
    assert!(matches!(
        MatchSession::start(no_limits),
        Err(SessionError::Config(ConfigError::NoLimit))
    ));

    let mut zero_score = config(generation(1));
    zero_score.limits = Limits {
        time_limit: None,
        score_limit: Some(0),
    };
    assert!(matches!(
        MatchSession::start(zero_score),
        Err(SessionError::Config(ConfigError::NonPositiveScoreLimit))
    ));

    // A roster refuses an empty match at construction, so `start` never sees
    // one; the two limits it *can* be given wrong are refused by name.
    assert_eq!(Roster::free_for_all(&[]), Err(ConfigError::EmptyRoster));

    let mut oversized = config(generation(1));
    oversized.objectives = MAX_OBJECTIVES + 1;
    assert!(matches!(
        MatchSession::start(oversized),
        Err(SessionError::TooManyObjectives { max, .. }) if max == MAX_OBJECTIVES
    ));
}

/// The end-of-match record: it names the map it ran on, its own session, why
/// it ended, the standings and every recorded delivery — and it exists only
/// while that match is finished.
#[test]
fn accept_f56_c_the_end_of_match_record_is_scoped_to_the_match_that_made_it() {
    let mut played = MatchSession::start(config(generation(4))).expect("the match starts");
    let flag = played.objective_ids().next().expect("two objectives");
    assert_eq!(
        played.end_of_match(),
        None,
        "a running match has no results screen"
    );

    played
        .submit_objective(claim(generation(4), 2, flag, 1))
        .expect("queued");
    played.close_tick(Tick(2));
    played
        .submit_objective(delivery(generation(4), 3, flag))
        .expect("queued");
    played.close_tick(Tick(3));
    played
        .submit_lethal(kill(generation(4), 4, 0, 1, 2))
        .expect("queued");
    played
        .submit_lethal(kill(generation(4), 4, 1, 1, 2))
        .expect("queued");
    let TickClose { rulings, ended } = played.close_tick(Tick(4));
    assert!(rulings.is_empty(), "no objective event on the scoring tick");
    assert!(ended, "the second point ends the match");

    let report = played.end_of_match().expect("the match ended");
    assert_eq!(
        report.scenario,
        scenario(),
        "the results screen names the map"
    );
    assert_eq!(report.session, generation(4));
    assert_eq!(report.decided_at, Tick(4));
    assert_eq!(report.reason, EndReason::ScoreLimit);
    assert_eq!(report.reason_key(), "match.end.score_limit");
    assert_eq!(report.outcome_key(), "match.outcome.win");
    assert_eq!(report.winner(), Some(Side::Participant(peer(1))));
    assert_eq!(
        report.standings,
        vec![
            (Side::Participant(peer(1)), 2),
            (Side::Participant(peer(2)), 0)
        ],
        "highest first: the two kills are the winner's, a delivery scores no points"
    );
    assert_eq!(report.captures.len(), 1, "one delivery was recorded");
    assert_eq!(report.captures[0].objective, flag);
    assert_eq!(report.captures[0].scorer, peer(1));

    // Nothing may move the finished match.
    assert!(matches!(
        played.submit_lethal(kill(generation(4), 5, 2, 2, 1)),
        Err(SessionError::MatchOver)
    ));
    assert!(matches!(
        played.submit_objective(claim(generation(4), 5, flag, 2)),
        Err(SessionError::MatchOver)
    ));
    assert_eq!(
        played.close_tick(Tick(9)),
        TickClose {
            rulings: Vec::<Ruling>::new(),
            ended: true
        },
        "closing later ticks keeps returning the same finished match"
    );
    assert_eq!(
        played.end_of_match().map(|report| report.decided_at),
        Some(Tick(4)),
        "the record does not move either"
    );
}

/// One tick is judged once, on both consumers, with possession first: a
/// pickup made in the closing tick is applied, and nothing is applied after
/// the time limit.
#[test]
fn accept_f56_c_one_tick_judges_possession_scoring_and_the_limits_once() {
    let mut played = MatchSession::start(config(generation(1))).expect("the match starts");
    let flags: Vec<_> = played.objective_ids().collect();
    assert_eq!(flags.len(), 2);

    // Possession and a point on one tick: the board is judged first, then the
    // resolver, then the limits once.
    played
        .submit_objective(claim(generation(1), 9, flags[0], 1))
        .expect("queued");
    played
        .submit_lethal(kill(generation(1), 9, 0, 1, 2))
        .expect("queued");
    let TickClose { rulings, ended } = played.close_tick(Tick(9));
    assert_eq!(
        rulings.len(),
        1,
        "the claim is the only objective event of the tick"
    );
    assert!(
        matches!(
            rulings[0].verdict,
            Verdict::Applied(Transition::Possessed { .. })
        ),
        "the claim applies"
    );
    assert_eq!(played.score(Side::Participant(peer(1))), Some(1));
    assert!(!ended);
    assert_eq!(played.clock(), Some(Tick(9)));

    // Closing an older tick changes nothing: the clock never runs backwards.
    assert_eq!(
        played.close_tick(Tick(4)),
        TickClose {
            rulings: Vec::<Ruling>::new(),
            ended: false
        }
    );
    assert_eq!(played.clock(), Some(Tick(9)));

    // Events stamped after the time limit are never applied, on either
    // consumer, and the match still ends exactly once — on the limit tick.
    played
        .submit_objective(claim(generation(1), 150, flags[1], 2))
        .expect("queued before the caller knows the limit");
    played
        .submit_lethal(kill(generation(1), 150, 1, 2, 1))
        .expect("queued");
    let TickClose { rulings, ended } = played.close_tick(Tick(150));
    assert!(
        rulings.is_empty(),
        "a pickup stamped after the time limit is not applied"
    );
    assert!(ended, "the time limit ends the match");
    assert_eq!(
        played.clock(),
        Some(Tick(100)),
        "the clock stops at the limit"
    );
    assert_eq!(
        played.objective_state(flags[1]),
        Some(ObjectiveState::Home),
        "the late pickup did not happen"
    );
    assert_eq!(
        played.score(Side::Participant(peer(1))),
        Some(1),
        "the late point did not happen"
    );
    let report = played.end_of_match().expect("the match ended");
    assert_eq!(report.reason, EndReason::TimeLimit);
    assert_eq!(report.reason_key(), "match.end.time_limit");
}

/// A draw is reported as a draw of exactly the tied sides, and the session
/// runs the victory rule the wiring handed it — never an assumed one.
#[test]
fn accept_f56_c_the_session_runs_the_victory_rule_it_was_handed() {
    let mut time_only = config(generation(1));
    time_only.limits = Limits {
        time_limit: Some(Tick(10)),
        score_limit: None,
    };
    let mut played = MatchSession::start(time_only).expect("the match starts");
    assert_eq!(played.victory(), VictoryRule::HighestScore);

    played
        .submit_lethal(kill(generation(1), 2, 0, 1, 2))
        .expect("queued");
    played
        .submit_lethal(kill(generation(1), 3, 1, 2, 1))
        .expect("queued");
    let TickClose { ended, .. } = played.close_tick(Tick(10));
    assert!(ended, "the time limit ends the match");

    let report = played.end_of_match().expect("the match ended");
    assert_eq!(report.reason, EndReason::TimeLimit);
    assert_eq!(
        report.outcome,
        Outcome::Draw(vec![Side::Participant(peer(1)), Side::Participant(peer(2))]),
        "equal top scores draw"
    );
    assert_eq!(report.winner(), None, "a draw names no single winner");
    assert_eq!(report.outcome_key(), "match.outcome.draw");

    // The label pair the wiring uses: an unknown rule is refused instead of
    // being coerced onto the only rule implemented.
    assert_eq!(
        VictoryRule::from_label(VictoryRule::HighestScore.label()),
        Some(VictoryRule::HighestScore)
    );
    assert_eq!(VictoryRule::from_label("territory_control"), None);
}
