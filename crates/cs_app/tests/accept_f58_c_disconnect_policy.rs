//! Acceptance scenario F58-C follow-up (#1223): the mode's resolved
//! `DisconnectPolicy` is applied once at the departure site.
//!
//! Spec: `specs/F58-network-abuse-resistance-reconnect-and-match-recovery.md`,
//! stage `### F58-C`; `cs_net::rules::MatchRules::disconnect`. Task test
//! prefix: `accept_f58_c_disconnect_policy_`.
//!
//! The path is production end to end: [`RecoveryFlow::depart`], configured
//! with the policy a resolved rules value names, applies it to the
//! authoritative [`MatchSession`] on the peer's first departure report and on
//! no later one.
//!
//! Every value is synthetic fixture data; nothing here is original game data.

use cs_app::network::recovery::{RecoveryFlow, StandingChange};
use cs_net::recovery::{DisconnectCause, Settlement};
use cs_net::rules::DisconnectPolicy;
use cs_sim::multiplayer::result::{
    EndReason, LethalEvent, LethalKind, Limits, Roster, ScoreTable, Side, Strike, VictoryRule,
};
use cs_sim::multiplayer::session::{MatchSession, SessionConfig};
use cs_types::Tick;
use cs_types::content::ContentId;
use cs_types::net::{EventId, PeerId, SessionId};

fn session() -> SessionId {
    SessionId::new(903).expect("a nonzero session")
}

fn peer(number: u16) -> PeerId {
    PeerId::new(number).expect("a nonzero peer number")
}

fn config(roster: Roster) -> SessionConfig {
    SessionConfig {
        scenario: ContentId::parse("multiplayer_scenario/slot.c1.mp2").expect("a scenario id"),
        session: session(),
        roster,
        table: ScoreTable {
            kill: 2,
            crash: -1,
            team_kill: -2,
        },
        limits: Limits {
            time_limit: Some(Tick(100)),
            score_limit: Some(50),
        },
        victory: VictoryRule::HighestScore,
        objectives: 0,
    }
}

fn kill(tick: u64, killer: u16, victim: u16) -> LethalEvent {
    LethalEvent {
        id: EventId {
            session: session(),
            tick: Tick(tick),
            producer: 2,
            sequence: u32::from(killer) * 100 + u32::from(victim),
        },
        victim: peer(victim),
        kind: LethalKind::Kill {
            killer: peer(killer),
        },
    }
}

/// Three pilots; peer 1 has downed peers 2 and 3 and holds 4 points.
fn scored(roster: Roster, kills: &[(u16, u16)]) -> MatchSession {
    let mut played = MatchSession::start(config(roster)).expect("the match starts");
    for (index, (killer, victim)) in kills.iter().enumerate() {
        played
            .submit_lethal(kill(3 + index as u64, *killer, *victim))
            .expect("the kill is queued");
    }
    played.close_tick(Tick(10));
    played
}

fn ffa() -> MatchSession {
    let roster = Roster::free_for_all(&[peer(1), peer(2), peer(3)]).expect("three pilots");
    scored(roster, &[(1, 2), (1, 3), (2, 3)])
}

fn flow(policy: DisconnectPolicy) -> RecoveryFlow {
    let mut flow = RecoveryFlow::new(session()).with_disconnect_policy(policy);
    for number in 1..=3 {
        flow.receiver_mut().admit_peer(peer(number));
    }
    flow
}

fn depart(
    flow: &mut RecoveryFlow,
    who: u16,
    tick: u64,
    played: &mut MatchSession,
) -> cs_app::network::recovery::Departure {
    flow.depart(
        peer(who),
        DisconnectCause::Voluntary,
        Tick(tick),
        Some(played),
        None,
    )
    .expect("the departure settles")
}

#[test]
fn accept_f58_c_disconnect_policy_keep_score_leaves_the_board_alone() {
    let mut played = ffa();
    let mut flow = flow(DisconnectPolicy::KeepScore);
    assert_eq!(played.score(Side::Participant(peer(1))), Some(4));

    let first = depart(&mut flow, 1, 12, &mut played);
    assert_eq!(first.settlement, Settlement::Applied { tick: Tick(12) });
    assert_eq!(first.standing, Some(StandingChange::Kept));
    assert_eq!(played.score(Side::Participant(peer(1))), Some(4));
    assert!(!played.is_over());

    let again = depart(&mut flow, 1, 13, &mut played);
    assert_eq!(again.standing, None, "a duplicate report applies nothing");
}

#[test]
fn accept_f58_c_disconnect_policy_strike_score_strikes_once() {
    let mut played = ffa();
    let mut flow = flow(DisconnectPolicy::StrikeScore);

    let first = depart(&mut flow, 1, 12, &mut played);
    assert_eq!(
        first.standing,
        Some(StandingChange::Struck(Strike::Struck { removed: 4 }))
    );
    assert_eq!(played.score(Side::Participant(peer(1))), None);
    assert_eq!(played.score(Side::Participant(peer(2))), Some(2));
    assert!(!played.is_over(), "a strike does not end the match");

    // A second report of the same peer, and a direct second strike, change
    // nothing.
    let again = depart(&mut flow, 1, 13, &mut played);
    assert_eq!(again.standing, None);
    assert_eq!(played.strike_score(peer(1)), Strike::Duplicate);
    assert_eq!(played.score(Side::Participant(peer(2))), Some(2));

    // Another pilot's departure strikes only their own score.
    let other = depart(&mut flow, 2, 14, &mut played);
    assert_eq!(
        other.standing,
        Some(StandingChange::Struck(Strike::Struck { removed: 2 }))
    );
    assert_eq!(played.score(Side::Participant(peer(3))), Some(0));
}

#[test]
fn accept_f58_c_disconnect_policy_strike_score_takes_back_only_the_leavers_share() {
    let roster = Roster::teams(&[(peer(1), 0), (peer(2), 0), (peer(3), 1)]).expect("two teams");
    let mut played = scored(roster, &[(1, 3), (2, 3)]);
    assert_eq!(played.score(Side::Team(0)), Some(4));
    let mut flow = flow(DisconnectPolicy::StrikeScore);

    let first = depart(&mut flow, 1, 12, &mut played);
    assert_eq!(
        first.standing,
        Some(StandingChange::Struck(Strike::Struck { removed: 2 }))
    );
    assert_eq!(
        played.score(Side::Team(0)),
        Some(2),
        "the teammate's points stay"
    );
}

#[test]
fn accept_f58_c_disconnect_policy_end_match_ends_once() {
    let mut played = ffa();
    let mut flow = flow(DisconnectPolicy::EndMatch);

    let first = depart(&mut flow, 3, 12, &mut played);
    assert_eq!(first.standing, Some(StandingChange::EndedMatch(true)));
    assert!(played.is_over());
    let record = played.end_of_match().expect("a sealed record");
    assert_eq!(record.reason, EndReason::Departure);
    assert_eq!(record.decided_at, Tick(12));
    assert_eq!(record.winner(), Some(Side::Participant(peer(1))));
    assert_eq!(record.reason_key(), "match.end.departure");

    // The same peer again: a duplicate. A different peer: the match is
    // already over, so nothing ends a second time.
    let again = depart(&mut flow, 3, 13, &mut played);
    assert_eq!(again.standing, None);
    let other = depart(&mut flow, 2, 14, &mut played);
    assert_eq!(other.standing, Some(StandingChange::EndedMatch(false)));
    let after = played.end_of_match().expect("still sealed");
    assert_eq!(after.decided_at, Tick(12), "the result never moves");
}

#[test]
fn accept_f58_c_disconnect_policy_unconfigured_flow_applies_none() {
    let mut played = ffa();
    let mut flow = RecoveryFlow::new(session());
    flow.receiver_mut().admit_peer(peer(1));
    assert_eq!(flow.disconnect_policy(), None);

    let departure = depart(&mut flow, 1, 12, &mut played);
    assert_eq!(departure.standing, None);
    assert_eq!(played.score(Side::Participant(peer(1))), Some(4));
    assert!(!played.is_over());
}
