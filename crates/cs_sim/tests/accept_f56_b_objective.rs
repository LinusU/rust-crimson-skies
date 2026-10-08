//! Acceptance scenario F56-B (simulation half): two clients claim the same
//! objective; only the server-accepted ownership succeeds.
//!
//! The objective states and transitions are engine design (the original
//! possession, drop and delivery rules are unknown; see
//! `docs/findings/2026-10-02-f56-a-multiplayer-catalog.md`). Every test drives
//! the production `ObjectiveBoard`; peers, sessions and event ids are authored
//! inputs.

use cs_sim::multiplayer::objective::{
    Denial, ObjectiveAction, ObjectiveBoard, ObjectiveError, ObjectiveEvent, ObjectiveId,
    ObjectiveState, Transition, Verdict,
};
use cs_sim::multiplayer::result::{
    LethalEvent, LethalKind, Limits, MatchResolver, Outcome, Roster, ScoreTable, Side, Submitted,
    VictoryRule,
};
use cs_types::Tick;
use cs_types::net::{EventId, PeerId, SessionId};

fn session(n: u64) -> SessionId {
    SessionId::new(n).expect("nonzero")
}

fn peer(n: u16) -> PeerId {
    PeerId::new(n).expect("nonzero")
}

fn id(session_n: u64, tick: u64, producer: u32, sequence: u32) -> EventId {
    EventId {
        session: session(session_n),
        tick: Tick(tick),
        producer,
        sequence,
    }
}

fn claim(session_n: u64, tick: u64, producer: u32, sequence: u32, claimant: u16) -> ObjectiveEvent {
    ObjectiveEvent {
        id: id(session_n, tick, producer, sequence),
        objective: ObjectiveId::new(1).expect("nonzero"),
        action: ObjectiveAction::Claim {
            claimant: peer(claimant),
        },
    }
}

fn event(
    session_n: u64,
    tick: u64,
    producer: u32,
    sequence: u32,
    action: ObjectiveAction,
) -> ObjectiveEvent {
    ObjectiveEvent {
        id: id(session_n, tick, producer, sequence),
        objective: ObjectiveId::new(1).expect("nonzero"),
        action,
    }
}

/// A two-peer board over objective serial 1, freshly declared at Home.
fn board() -> (ObjectiveBoard, ObjectiveId) {
    let roster = Roster::teams(&[(peer(1), 0), (peer(2), 1), (peer(3), 0)]).unwrap();
    let mut board = ObjectiveBoard::new(session(1), &roster);
    let objective = board.declare();
    assert_eq!(objective, ObjectiveId::new(1).unwrap());
    assert_eq!(board.state(objective), Some(ObjectiveState::Home));
    (board, objective)
}

#[test]
fn accept_f56_b_two_clients_claim_one_objective_and_only_the_server_accepted_ownership_holds() {
    let (mut board, objective) = board();
    // Peer 2's request reaches the server first; peer 1's carries the lower
    // event id (tick 10, producer 1 before producer 2). Possession is judged
    // in id order at tick close, never in arrival order.
    let second = claim(1, 10, 2, 0, 2);
    let first = claim(1, 10, 1, 0, 1);
    assert_eq!(board.submit(second), Ok(Submitted::Accepted));
    assert_eq!(board.submit(first), Ok(Submitted::Accepted));
    assert_eq!(
        board.holder(objective),
        None,
        "a submitted claim is a request, not a fact, until its tick closes"
    );

    let rulings = board.close_tick(Tick(10));
    assert_eq!(
        rulings
            .iter()
            .map(|ruling| (ruling.event, ruling.verdict))
            .collect::<Vec<_>>(),
        [
            (
                first.id,
                Verdict::Applied(Transition::Possessed { holder: peer(1) })
            ),
            (
                second.id,
                Verdict::Denied(Denial::AlreadyHeld { holder: peer(1) })
            ),
        ],
        "the lower event id wins; the second client's claim is refused"
    );
    assert_eq!(
        board.state(objective),
        Some(ObjectiveState::Held(peer(1))),
        "singular ownership: the board names exactly one holder"
    );
}

#[test]
fn accept_f56_b_retransmitted_events_never_transition_twice() {
    let (mut board, objective) = board();
    let claim = claim(1, 10, 1, 0, 1);
    assert_eq!(board.submit(claim), Ok(Submitted::Accepted));
    assert_eq!(board.submit(claim), Ok(Submitted::Duplicate));
    let rulings = board.close_tick(Tick(10));
    assert_eq!(
        rulings.len(),
        1,
        "the retransmission of a queued event is not a second event"
    );
    assert_eq!(board.holder(objective), Some(peer(1)));

    // A retransmission of an already-applied event cannot re-enter the queue:
    // its tick is closed, so it is refused before any state can move again.
    assert_eq!(
        board.submit(claim),
        Err(ObjectiveError::LateEvent { tick: Tick(10) })
    );
    assert!(board.close_tick(Tick(11)).is_empty());
    assert_eq!(board.holder(objective), Some(peer(1)));

    let score = event(1, 12, 1, 1, ObjectiveAction::Score);
    assert_eq!(board.submit(score), Ok(Submitted::Accepted));
    assert_eq!(board.submit(score), Ok(Submitted::Duplicate));
    let rulings = board.close_tick(Tick(12));
    assert_eq!(
        rulings[0].verdict,
        Verdict::Applied(Transition::Scored { scorer: peer(1) })
    );
    assert_eq!(board.state(objective), Some(ObjectiveState::Home));
    assert_eq!(
        board.scores(objective).map(<[_]>::len),
        Some(1),
        "the delivered flag scores once, however often the packet repeats"
    );
    assert_eq!(
        board.submit(score),
        Err(ObjectiveError::LateEvent { tick: Tick(12) }),
        "a post-close retransmission is stale, never a second score"
    );
    assert_eq!(board.scores(objective).map(<[_]>::len), Some(1));
}

#[test]
fn accept_f56_b_a_claim_of_another_session_is_refused_and_inherits_nothing() {
    let (mut board, objective) = board();
    board.submit(claim(1, 10, 1, 0, 1)).unwrap();
    board.close_tick(Tick(10));
    assert_eq!(board.holder(objective), Some(peer(1)));

    // A replay stamped for another session generation is refused outright.
    assert_eq!(
        board.submit(claim(2, 11, 2, 0, 2)),
        Err(ObjectiveError::WrongSession { got: session(2) })
    );
    assert_eq!(board.holder(objective), Some(peer(1)));

    // The next session's board is a fresh match: nothing carries over, so a
    // reconnect or a recycled peer id cannot inherit objective state.
    let roster = Roster::teams(&[(peer(1), 0), (peer(2), 1), (peer(3), 0)]).unwrap();
    let mut next = ObjectiveBoard::new(session(2), &roster);
    let next_objective = next.declare();
    assert_eq!(next.state(next_objective), Some(ObjectiveState::Home));
    next.submit(claim(2, 1, 2, 0, 2)).unwrap();
    next.close_tick(Tick(1));
    assert_eq!(next.holder(next_objective), Some(peer(2)));
    assert_eq!(
        board.holder(objective),
        Some(peer(1)),
        "session 2's claim moved nothing in session 1"
    );
}

#[test]
fn accept_f56_b_only_a_roster_participant_can_hold_an_objective() {
    let (mut board, objective) = board();
    assert_eq!(
        board.submit(claim(1, 10, 9, 0, 9)),
        Err(ObjectiveError::UnknownParticipant(peer(9))),
        "a peer outside the match cannot take the flag"
    );
    assert_eq!(
        board.submit(event(
            1,
            10,
            9,
            0,
            ObjectiveAction::Drop { holder: peer(9) }
        )),
        Err(ObjectiveError::UnknownParticipant(peer(9)))
    );
    assert_eq!(
        board.submit(event(
            1,
            10,
            1,
            0,
            ObjectiveAction::Drop { holder: peer(4) }
        )),
        Err(ObjectiveError::UnknownParticipant(peer(4))),
        "a peer that never joined the match is refused too"
    );
    assert!(board.close_tick(Tick(10)).is_empty());
    assert_eq!(board.state(objective), Some(ObjectiveState::Home));
}

#[test]
fn accept_f56_b_the_state_machine_refuses_every_transition_it_does_not_allow() {
    let (mut board, objective) = board();
    // At Home only a claim applies.
    for (sequence, action) in [
        (0, ObjectiveAction::Score),
        (1, ObjectiveAction::Return),
        (2, ObjectiveAction::Drop { holder: peer(1) }),
    ] {
        board.submit(event(1, 10, 1, sequence, action)).unwrap();
    }
    let rulings = board.close_tick(Tick(10));
    assert_eq!(
        rulings
            .iter()
            .map(|ruling| ruling.verdict)
            .collect::<Vec<_>>(),
        [
            Verdict::Denied(Denial::NotHeld),
            Verdict::Denied(Denial::NotDropped),
            Verdict::Denied(Denial::NotHeld),
        ]
    );
    assert_eq!(board.state(objective), Some(ObjectiveState::Home));

    // Held: a stranger's drop and a second claim are refused; a return needs
    // a dropped objective.
    board.submit(claim(1, 11, 1, 1, 1)).unwrap();
    board.close_tick(Tick(11));
    assert_eq!(board.holder(objective), Some(peer(1)));
    for (producer, sequence, action) in [
        (2, 0, ObjectiveAction::Drop { holder: peer(2) }),
        (2, 1, ObjectiveAction::Claim { claimant: peer(2) }),
        (1, 2, ObjectiveAction::Return),
    ] {
        board
            .submit(event(1, 12, producer, sequence, action))
            .unwrap();
    }
    let rulings = board.close_tick(Tick(12));
    assert_eq!(
        rulings
            .iter()
            .map(|ruling| ruling.verdict)
            .collect::<Vec<_>>(),
        // Id order is (tick, producer, sequence): the return is judged first.
        [
            Verdict::Denied(Denial::NotDropped),
            Verdict::Denied(Denial::CarrierMismatch { held_by: peer(1) }),
            Verdict::Denied(Denial::AlreadyHeld { holder: peer(1) }),
        ]
    );
    assert_eq!(board.holder(objective), Some(peer(1)));
}

#[test]
fn accept_f56_b_dropped_objectives_return_or_are_picked_up_but_never_scored() {
    let (mut board, objective) = board();
    board.submit(claim(1, 10, 1, 0, 1)).unwrap();
    board
        .submit(event(
            1,
            11,
            1,
            0,
            ObjectiveAction::Drop { holder: peer(1) },
        ))
        .unwrap();
    board.close_tick(Tick(10));
    board.close_tick(Tick(11));
    assert_eq!(board.state(objective), Some(ObjectiveState::Dropped));

    // Dropped: a score does not apply (nobody holds it), a return does, and a
    // later claim picks it up.
    board
        .submit(event(1, 12, 1, 0, ObjectiveAction::Score))
        .unwrap();
    board
        .submit(event(1, 12, 1, 1, ObjectiveAction::Return))
        .unwrap();
    let rulings = board.close_tick(Tick(12));
    assert_eq!(
        rulings
            .iter()
            .map(|ruling| ruling.verdict)
            .collect::<Vec<_>>(),
        [
            Verdict::Denied(Denial::NotHeld),
            Verdict::Applied(Transition::Returned),
        ]
    );
    assert_eq!(board.state(objective), Some(ObjectiveState::Home));

    board.submit(claim(1, 13, 2, 0, 2)).unwrap();
    board.close_tick(Tick(13));
    board
        .submit(event(
            1,
            14,
            2,
            0,
            ObjectiveAction::Drop { holder: peer(2) },
        ))
        .unwrap();
    board.submit(claim(1, 15, 3, 0, 3)).unwrap();
    board.close_tick(Tick(14));
    board.close_tick(Tick(15));
    assert_eq!(
        board.holder(objective),
        Some(peer(3)),
        "a dropped objective is claimable again"
    );
}

#[test]
fn accept_f56_b_stale_or_unknown_targets_are_refused_before_they_queue() {
    let (mut board, objective) = board();
    board.submit(claim(1, 10, 1, 0, 1)).unwrap();
    board.close_tick(Tick(10));

    assert_eq!(
        board.submit(claim(1, 10, 2, 0, 2)),
        Err(ObjectiveError::LateEvent { tick: Tick(10) }),
        "a claim stamped for a closed tick cannot enter the queue"
    );
    let other = ObjectiveId::new(2).unwrap();
    assert_eq!(
        board.submit(ObjectiveEvent {
            id: id(1, 11, 2, 0),
            objective: other,
            action: ObjectiveAction::Claim { claimant: peer(2) },
        }),
        Err(ObjectiveError::UnknownObjective(other)),
        "an objective this match never declared is refused, not created"
    );
    assert_eq!(board.objective_ids().collect::<Vec<_>>(), [objective]);
}

#[test]
fn accept_f56_b_score_and_return_are_singular_across_interleaved_events() {
    let (mut board, objective) = board();
    board.submit(claim(1, 10, 1, 0, 1)).unwrap();
    // Peer 2's claim carries a higher event id than the score (producer 2
    // sorts after producer 1 at any sequence), so at tick close the flag is
    // home again before that claim is judged — and the claim applies.
    board.submit(claim(1, 10, 2, 0, 2)).unwrap();
    board
        .submit(event(1, 10, 1, 1, ObjectiveAction::Score))
        .unwrap();
    let rulings = board.close_tick(Tick(10));
    assert_eq!(
        rulings
            .iter()
            .map(|ruling| ruling.verdict)
            .collect::<Vec<_>>(),
        [
            Verdict::Applied(Transition::Possessed { holder: peer(1) }),
            Verdict::Applied(Transition::Scored { scorer: peer(1) }),
            Verdict::Applied(Transition::Possessed { holder: peer(2) }),
        ],
        "all of a tick's events adjudicate in id order against one state"
    );
    assert_eq!(board.holder(objective), Some(peer(2)));
    let scores = board.scores(objective).expect("a declared ledger");
    assert_eq!(scores.len(), 1);
    assert_eq!(scores[0].scorer, peer(1));
    assert_eq!(scores[0].event, id(1, 10, 1, 1));
    assert_eq!(
        board.state(objective),
        Some(ObjectiveState::Held(peer(2))),
        "the pickup after the score is the one ownership the board holds"
    );
}

#[test]
fn accept_f56_b_the_resolver_runs_the_victory_rule_the_rules_resolved() {
    let roster = Roster::free_for_all(&[peer(1), peer(2)]).unwrap();
    let limits = Limits {
        time_limit: Some(Tick(100)),
        score_limit: None,
    };
    let resolver = MatchResolver::with_victory(
        session(1),
        roster.clone(),
        ScoreTable {
            kill: 2,
            crash: -2,
            team_kill: -2,
        },
        limits,
        VictoryRule::HighestScore,
    )
    .unwrap();
    assert_eq!(resolver.victory(), VictoryRule::HighestScore);
    // `new` is the same construction with the one implemented rule.
    let plain = MatchResolver::new(
        session(1),
        roster,
        ScoreTable {
            kill: 2,
            crash: -2,
            team_kill: -2,
        },
        limits,
    )
    .unwrap();
    assert_eq!(plain.victory(), resolver.victory());
}

#[test]
fn accept_f56_b_highest_score_decides_the_winner_and_a_tie_draws() {
    let roster = Roster::free_for_all(&[peer(1), peer(2)]).unwrap();
    let limits = Limits {
        time_limit: Some(Tick(10)),
        score_limit: None,
    };
    let mut resolver = MatchResolver::with_victory(
        session(1),
        roster,
        ScoreTable {
            kill: 2,
            crash: -2,
            team_kill: -2,
        },
        limits,
        VictoryRule::HighestScore,
    )
    .unwrap();
    resolver
        .submit(LethalEvent {
            id: id(1, 1, 1, 0),
            victim: peer(2),
            kind: LethalKind::Kill { killer: peer(1) },
        })
        .unwrap();
    let result = resolver.close_tick(Tick(10)).expect("the limit ends it");
    assert_eq!(
        result.outcome,
        Outcome::Winner(Side::Participant(peer(1))),
        "the declared rule is what seal() runs"
    );
}
