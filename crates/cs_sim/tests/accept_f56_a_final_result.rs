//! Acceptance scenario F56-A (simulation half), AC01: simultaneous lethal
//! events and limit expiry produce one documented final result.
//!
//! The score table and limits are authored test inputs: the original values
//! are unknown (see `docs/findings/2026-10-02-f56-a-multiplayer-catalog.md`).
//! Every test drives the production `MatchResolver`.

use cs_sim::multiplayer::result::{
    ConfigError, EndReason, FinalResult, LethalEvent, LethalKind, Limits, MatchResolver, Outcome,
    Roster, ScoreTable, Side, SubmitError, Submitted,
};
use cs_types::Tick;
use cs_types::net::{EventId, PeerId, SessionId};

const TABLE: ScoreTable = ScoreTable {
    kill: 2,
    crash: -2,
    team_kill: -2,
};

fn session(n: u64) -> SessionId {
    SessionId::new(n).expect("nonzero")
}

fn peer(n: u16) -> PeerId {
    PeerId::new(n).expect("nonzero")
}

fn event(
    session_n: u64,
    tick: u64,
    producer: u32,
    sequence: u32,
    victim: u16,
    kind: LethalKind,
) -> LethalEvent {
    LethalEvent {
        id: EventId {
            session: session(session_n),
            tick: Tick(tick),
            producer,
            sequence,
        },
        victim: peer(victim),
        kind,
    }
}

fn kill(session_n: u64, tick: u64, sequence: u32, killer: u16, victim: u16) -> LethalEvent {
    event(
        session_n,
        tick,
        0,
        sequence,
        victim,
        LethalKind::Kill {
            killer: peer(killer),
        },
    )
}

fn ffa(limits: Limits) -> MatchResolver {
    let roster = Roster::free_for_all(&[peer(1), peer(2), peer(3)]).unwrap();
    MatchResolver::new(session(1), roster, TABLE, limits).unwrap()
}

fn time(tick: u64) -> Limits {
    Limits {
        time_limit: Some(Tick(tick)),
        score_limit: None,
    }
}

fn close(resolver: &mut MatchResolver, tick: u64) -> Option<FinalResult> {
    resolver.close_tick(Tick(tick)).cloned()
}

#[test]
fn accept_f56_a_simultaneous_kills_on_the_limit_tick_all_count_in_one_result() {
    let mut m = ffa(time(100));
    // Two pilots shoot each other down on the very tick the clock expires.
    m.submit(kill(1, 100, 0, 1, 2)).unwrap();
    m.submit(kill(1, 100, 1, 2, 1)).unwrap();
    m.submit(kill(1, 100, 2, 3, 2)).unwrap();
    assert_eq!(close(&mut m, 99), None, "the limit tick is not over yet");
    let result = close(&mut m, 100).expect("the limit tick ends the match");

    assert_eq!(result.reason, EndReason::TimeLimit);
    assert_eq!(result.decided_at, Tick(100));
    // 1 and 2 killed each other (a pilot killed on the tick still scores) and
    // 3 also downed 2: 3 holds 2, 1 holds 2, 2 holds 2 -> a three-way draw.
    assert_eq!(
        result.outcome,
        Outcome::Draw(vec![
            Side::Participant(peer(1)),
            Side::Participant(peer(2)),
            Side::Participant(peer(3)),
        ])
    );
    assert_eq!(
        close(&mut m, 100),
        Some(result.clone()),
        "one sealed result"
    );
    assert_eq!(
        close(&mut m, 500),
        Some(result),
        "later ticks do not re-decide"
    );
}

#[test]
fn accept_f56_a_arrival_order_does_not_change_the_result() {
    let events = [
        kill(1, 7, 0, 1, 2),
        kill(1, 7, 1, 1, 3),
        kill(1, 9, 0, 2, 1),
        event(1, 9, 1, 0, 3, LethalKind::Crash),
        kill(1, 10, 0, 1, 3),
    ];
    let mut reference: Option<FinalResult> = None;
    // Every rotation and the reverse are different arrival orders.
    let mut orders: Vec<Vec<LethalEvent>> = (0..events.len())
        .map(|shift| {
            events
                .iter()
                .cycle()
                .skip(shift)
                .take(events.len())
                .copied()
                .collect()
        })
        .collect();
    orders.push(events.iter().rev().copied().collect());
    for order in orders {
        let mut m = ffa(Limits {
            time_limit: Some(Tick(10)),
            score_limit: None,
        });
        for e in order {
            m.submit(e).unwrap();
        }
        let result = close(&mut m, 10).expect("ends");
        match &reference {
            None => reference = Some(result),
            Some(first) => assert_eq!(&result, first),
        }
    }
    let result = reference.unwrap();
    assert_eq!(result.outcome, Outcome::Winner(Side::Participant(peer(1))));
    assert_eq!(
        result.standings,
        vec![
            (Side::Participant(peer(1)), 6),
            (Side::Participant(peer(2)), 2),
            (Side::Participant(peer(3)), -2),
        ]
    );
}

#[test]
fn accept_f56_a_score_limit_and_time_limit_on_one_tick_end_it_once() {
    let mut m = ffa(Limits {
        time_limit: Some(Tick(50)),
        score_limit: Some(4),
    });
    m.submit(kill(1, 50, 0, 1, 2)).unwrap();
    m.submit(kill(1, 50, 1, 1, 3)).unwrap();
    let result = close(&mut m, 50).unwrap();
    assert_eq!(result.reason, EndReason::ScoreAndTimeLimit);
    assert_eq!(result.outcome, Outcome::Winner(Side::Participant(peer(1))));
    assert_eq!(
        m.submit(kill(1, 51, 0, 2, 1)),
        Err(SubmitError::AlreadyFinal),
        "a sealed match takes no further event"
    );
    assert_eq!(m.result(), Some(&result));
}

#[test]
fn accept_f56_a_every_event_of_the_scoring_tick_counts_even_past_the_score_limit() {
    let mut m = ffa(Limits {
        time_limit: None,
        score_limit: Some(2),
    });
    // 1 reaches the limit with the first kill; 2 scores two on the same tick
    // and so passes it. Applied in event order, judged once at the tick's end.
    m.submit(kill(1, 5, 0, 1, 3)).unwrap();
    m.submit(kill(1, 5, 1, 2, 3)).unwrap();
    m.submit(kill(1, 5, 2, 2, 1)).unwrap();
    let result = close(&mut m, 5).unwrap();
    assert_eq!(result.reason, EndReason::ScoreLimit);
    assert_eq!(result.outcome, Outcome::Winner(Side::Participant(peer(2))));
}

#[test]
fn accept_f56_a_an_event_stamped_after_the_limit_is_never_scored() {
    let mut m = ffa(time(10));
    m.submit(kill(1, 10, 0, 1, 2)).unwrap();
    m.submit(kill(1, 11, 0, 2, 1)).unwrap();
    m.submit(kill(1, 12, 0, 2, 3)).unwrap();
    // The host closes a later tick; the match is still judged at the limit.
    let result = close(&mut m, 12).unwrap();
    assert_eq!(result.decided_at, Tick(10));
    assert_eq!(result.outcome, Outcome::Winner(Side::Participant(peer(1))));
    assert_eq!(m.score(Side::Participant(peer(2))), Some(0));
}

#[test]
fn accept_f56_a_a_retransmitted_event_scores_once() {
    let mut m = ffa(time(10));
    let e = kill(1, 3, 0, 1, 2);
    assert_eq!(m.submit(e), Ok(Submitted::Accepted));
    assert_eq!(m.submit(e), Ok(Submitted::Duplicate));
    assert_eq!(close(&mut m, 5), None);
    assert_eq!(m.submit(e), Err(SubmitError::LateEvent { tick: Tick(3) }));
    assert_eq!(m.score(Side::Participant(peer(1))), Some(2), "not 4");
    // Duplicate after scoring but within an open tick of the same id is also once.
    let mut m = ffa(time(10));
    m.submit(kill(1, 3, 0, 1, 2)).unwrap();
    m.submit(kill(1, 3, 0, 1, 2)).unwrap();
    close(&mut m, 3);
    assert_eq!(m.score(Side::Participant(peer(1))), Some(2));
}

#[test]
fn accept_f56_a_another_sessions_events_and_recycled_peer_ids_never_inherit_score() {
    let mut first = ffa(time(10));
    first.submit(kill(1, 1, 0, 1, 2)).unwrap();
    let first_result = close(&mut first, 10).unwrap();
    assert_eq!(first_result.session, session(1));

    // A restarted match is a new session with the same peer numbers.
    let roster = Roster::free_for_all(&[peer(1), peer(2), peer(3)]).unwrap();
    let mut second = MatchResolver::new(session(2), roster, TABLE, time(10)).unwrap();
    assert_eq!(second.score(Side::Participant(peer(1))), Some(0));
    assert_eq!(
        second.submit(kill(1, 4, 0, 1, 2)),
        Err(SubmitError::WrongSession { got: session(1) }),
        "a straggler of the old session is refused"
    );
    let result = close(&mut second, 10).unwrap();
    assert_eq!(result.session, session(2));
    assert!(
        matches!(result.outcome, Outcome::Draw(_)),
        "nothing carried over"
    );
}

#[test]
fn accept_f56_a_team_kills_and_crashes_cost_the_side_and_teams_score_as_teams() {
    let roster = Roster::teams(&[(peer(1), 0), (peer(2), 0), (peer(3), 1)]).unwrap();
    let mut m = MatchResolver::new(session(1), roster, TABLE, time(10)).unwrap();
    m.submit(kill(1, 1, 0, 1, 3)).unwrap(); // team 0 +2
    m.submit(kill(1, 2, 0, 1, 2)).unwrap(); // friendly: team 0 -2
    m.submit(event(1, 3, 0, 0, 3, LethalKind::Crash)).unwrap(); // team 1 -2
    m.submit(kill(1, 4, 0, 2, 3)).unwrap(); // team 0 +2
    let result = close(&mut m, 10).unwrap();
    assert_eq!(
        result.standings,
        vec![(Side::Team(0), 2), (Side::Team(1), -2)]
    );
    assert_eq!(result.outcome, Outcome::Winner(Side::Team(0)));
}

#[test]
fn accept_f56_a_equal_top_scores_are_a_draw_of_exactly_those_sides() {
    let mut m = ffa(time(10));
    m.submit(kill(1, 1, 0, 1, 3)).unwrap();
    m.submit(kill(1, 2, 0, 2, 3)).unwrap();
    let result = close(&mut m, 10).unwrap();
    assert_eq!(
        result.outcome,
        Outcome::Draw(vec![Side::Participant(peer(1)), Side::Participant(peer(2))])
    );
}

#[test]
fn accept_f56_a_malformed_events_are_refused_and_change_nothing() {
    let mut m = ffa(time(10));
    assert_eq!(
        m.submit(kill(1, 1, 0, 1, 1)),
        Err(SubmitError::SelfKill(peer(1)))
    );
    assert_eq!(
        m.submit(kill(1, 1, 0, 9, 1)),
        Err(SubmitError::UnknownParticipant(peer(9)))
    );
    assert_eq!(
        m.submit(event(1, 1, 0, 0, 9, LethalKind::Crash)),
        Err(SubmitError::UnknownParticipant(peer(9)))
    );
    // The refused ids were not remembered as seen.
    assert_eq!(m.submit(kill(1, 1, 0, 1, 2)), Ok(Submitted::Accepted));
}

#[test]
fn accept_f56_a_a_match_that_could_never_end_or_has_no_roster_is_refused() {
    let roster = || Roster::free_for_all(&[peer(1), peer(2)]).unwrap();
    let none = Limits {
        time_limit: None,
        score_limit: None,
    };
    assert_eq!(
        MatchResolver::new(session(1), roster(), TABLE, none).err(),
        Some(ConfigError::NoLimit)
    );
    let zero = Limits {
        time_limit: None,
        score_limit: Some(0),
    };
    assert_eq!(
        MatchResolver::new(session(1), roster(), TABLE, zero).err(),
        Some(ConfigError::NonPositiveScoreLimit)
    );
    assert_eq!(Roster::free_for_all(&[]), Err(ConfigError::EmptyRoster));
    assert_eq!(
        Roster::free_for_all(&[peer(1), peer(1)]),
        Err(ConfigError::DuplicateParticipant(peer(1)))
    );
    assert_eq!(
        Roster::teams(&[(peer(1), 0), (peer(2), 0)]),
        Err(ConfigError::TooFewTeams)
    );
}
