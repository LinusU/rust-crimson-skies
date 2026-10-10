//! Acceptance scenario F58-C: a disconnect while carrying an objective
//! resolves the authoritative state once.
//!
//! Spec: `specs/F58-network-abuse-resistance-reconnect-and-match-recovery.md`,
//! stage `### F58-C`, minimum scenario "Disconnect during docking or objective
//! carry; authoritative state resolves once". Contract:
//! `docs/contracts/UI-NETWORK.md` ("Server owns ... mission program, score and
//! result", "Reliable delivery does not replace application idempotency
//! because reconnect/retry can replay requests"). Task test prefix:
//! `accept_f58_c_`.
//!
//! The path is the whole designed chain: a decoded packet through
//! [`RecoveryFlow::receive_and_depart`], which classifies the departure
//! (`departure_cause`), tears the peer down at the boundary, and settles what
//! the peer still held on the authoritative [`MatchSession`] — the board, not
//! the flow, applies it, exactly once, when the tick closes.
//!
//! Every value is synthetic fixture data; nothing here is original game data.

use std::collections::BTreeSet;

use cs_app::network::recovery::{FlowError, RecoveryFlow};
use cs_net::lobby::Phase;
use cs_net::message::{ClientMessage, ClientPayload, InputBatch, MessageHeader};
use cs_net::recovery::{DisconnectCause, Settlement};
use cs_net::validation::{MatchStage, SessionViolation, ThreatDisposition};
use cs_sim::multiplayer::objective::{
    ObjectiveAction, ObjectiveError, ObjectiveEvent, ObjectiveId, ObjectiveState, Transition,
    Verdict,
};
use cs_sim::multiplayer::result::{Limits, Roster, ScoreTable, VictoryRule};
use cs_sim::multiplayer::session::{MatchSession, SessionConfig, SessionError};
use cs_types::Tick;
use cs_types::content::ContentId;
use cs_types::input::InputFrame;
use cs_types::net::{ActorId, EventId, PeerId, SessionId};

const SESSION: u64 = 901;

fn session() -> SessionId {
    SessionId::new(SESSION).expect("a nonzero session")
}

fn peer(number: u16) -> PeerId {
    PeerId::new(number).expect("a nonzero peer number")
}

fn actor() -> ActorId {
    ActorId {
        session: session(),
        serial: 7,
    }
}

/// A free-for-all over two pilots with one possessable objective, ending at
/// 100 ticks or 2 points: authored inputs, not original rules.
fn config() -> SessionConfig {
    SessionConfig {
        scenario: ContentId::parse("multiplayer_scenario/slot.c1.mp2").expect("a scenario id"),
        session: session(),
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
        objectives: 1,
    }
}

fn stage(tick: u64, banned: &BTreeSet<ContentId>) -> MatchStage<'_> {
    MatchStage::new(Phase::InMatch, Tick(tick), banned)
}

fn claim(tick: Tick, objective: ObjectiveId, claimant: PeerId) -> ObjectiveEvent {
    ObjectiveEvent {
        id: EventId {
            session: session(),
            tick,
            producer: 1,
            sequence: tick.0 as u32,
        },
        objective,
        action: ObjectiveAction::Claim { claimant },
    }
}

fn farewell(sequence: u32) -> ClientMessage {
    ClientMessage {
        header: MessageHeader {
            session: session(),
            sequence,
        },
        payload: ClientPayload::Leave,
    }
}

/// An admitted input packet that carries more frames than the wire allows.
fn oversized() -> ClientMessage {
    let frames = (0..cs_net::bounds::MAX_INPUT_FRAMES_PER_PACKET + 1)
        .map(|offset| InputFrame::new(Tick(u64::try_from(offset).expect("a small offset"))))
        .collect();
    ClientMessage {
        header: MessageHeader {
            session: session(),
            sequence: 1,
        },
        payload: ClientPayload::Input(InputBatch { frames }),
    }
}

/// A match whose objective is carried by peer 1, and a boundary with peer 1
/// admitted and flying an aircraft.
fn carrying() -> (MatchSession, RecoveryFlow, ObjectiveId) {
    let mut played = MatchSession::start(config()).expect("the match starts");
    let flag = played.objective_ids().next().expect("one objective");
    played
        .submit_objective(claim(Tick(5), flag, peer(1)))
        .expect("the pickup is queued");
    played.close_tick(Tick(5));
    assert_eq!(
        played.objective_state(flag),
        Some(ObjectiveState::Held(peer(1))),
        "the fixture starts with peer 1 carrying the objective"
    );

    let mut flow = RecoveryFlow::new(session());
    flow.receiver_mut().admit_peer(peer(1));
    flow.receiver_mut().admit_peer(peer(2));
    flow.receiver_mut()
        .bind_actor(peer(1), actor())
        .expect("the aircraft is free");
    (played, flow, flag)
}

/// The F58-C minimum scenario: the peer that carries the objective
/// disconnects, and the objective resolves exactly once — dropped by the
/// board, at the closing tick, with no second application however often the
/// departure is reported.
#[test]
fn accept_f58_c_a_disconnect_while_carrying_resolves_the_objective_once() {
    let (mut played, mut flow, flag) = carrying();
    let banned = BTreeSet::new();

    // Producer 1: the client's own farewell, on the wire.
    let settled = flow
        .receive_and_depart(peer(1), &farewell(1), stage(7, &banned), Some(&mut played))
        .expect("a farewell settles");
    assert!(
        settled.inbound.admitted(),
        "the farewell itself is admitted"
    );
    let departure = settled
        .departure
        .expect("an admitted farewell asks for a departure");
    assert_eq!(departure.cause, DisconnectCause::Voluntary);
    assert_eq!(
        departure.cause.wire_reason(),
        Some(cs_net::message::DisconnectReason::Voluntary)
    );
    assert_eq!(
        departure.actor,
        Some(actor()),
        "the report names its aircraft"
    );
    assert_eq!(
        departure.settlement,
        Settlement::Applied { tick: Tick(7) },
        "the first report is the one that settles"
    );
    assert_eq!(
        departure.dropped,
        vec![flag],
        "the carried objective is queued to drop"
    );

    // The boundary is torn down in the same step: no binding, no membership,
    // no replay window left behind for a peer id that is never recycled.
    assert_eq!(flow.receiver().actor_of(peer(1)), None);
    assert!(!flow.receiver().gate().is_member(peer(1)));

    // The authoritative board applies it when the tick closes: one ruling,
    // one transition, and the state the contract says it must reach.
    let close = played.close_tick(Tick(7));
    assert_eq!(
        close.rulings.len(),
        1,
        "the board judged the settlement exactly once"
    );
    assert_eq!(
        close.rulings[0].verdict,
        Verdict::Applied(Transition::Dropped)
    );
    assert_eq!(
        played.objective_state(flag),
        Some(ObjectiveState::Dropped),
        "the departed carrier no longer holds it"
    );

    // Producer 2: the transport reports the same dead link. Nothing is
    // applied a second time, and the report names what actually settled.
    let again = flow
        .depart(
            peer(1),
            DisconnectCause::Timeout,
            Tick(7),
            Some(&mut played),
        )
        .expect("a duplicate report is not an error");
    assert_eq!(
        again.settlement,
        Settlement::Duplicate { tick: Tick(7) },
        "the second report of the same peer must not settle again"
    );
    assert_eq!(
        again.cause,
        DisconnectCause::Voluntary,
        "the cause of record is the first report's"
    );
    assert_eq!(again.actor, None, "the teardown already ran");
    assert!(
        again.dropped.is_empty(),
        "a duplicate report queues no drop of its own"
    );

    // The next tick has nothing left to judge: the state resolved once.
    let close = played.close_tick(Tick(8));
    assert!(
        close.rulings.is_empty(),
        "no second transition follows a duplicate report"
    );
    assert_eq!(
        played.objective_state(flag),
        Some(ObjectiveState::Dropped),
        "still dropped, not dropped twice"
    );
    assert_eq!(
        played.captures_of(flag).map(<[_]>::len),
        Some(0),
        "a drop is not a delivery: the disconnect earns no capture"
    );

    // A late packet from the departed peer is refused by the torn-down
    // boundary, and that refusal is itself a disconnection by the declared
    // dispositions — a duplicate report of a departure already settled.
    let late = flow.receiver_mut().receive(peer(1), &farewell(2));
    let violation = late.violation().expect("the departed peer is refused");
    assert!(matches!(
        violation,
        SessionViolation::UnauthenticatedPeer { .. }
    ));
    assert_eq!(violation.disposition(), ThreatDisposition::Disconnect);
    assert!(flow.departures().is_settled(peer(1)));
    assert_eq!(flow.departures().len(), 1, "one peer, one settlement");
}

/// A refusal the declared dispositions cut off is a departure too: the abuse
/// producer of the same flow, with the bounded label of the refusal that
/// caused it.
#[test]
fn accept_f58_c_an_abusive_packet_settles_a_cut_off_with_a_bounded_reason() {
    let (mut played, mut flow, flag) = carrying();
    let banned = BTreeSet::new();

    let settled = flow
        .receive_and_depart(peer(1), &oversized(), stage(7, &banned), Some(&mut played))
        .expect("the cut-off settles");
    assert!(
        !settled.inbound.admitted(),
        "an oversized packet never reaches the payload"
    );
    let departure = settled
        .departure
        .expect("the declared disposition asks for a departure");
    assert_eq!(
        departure.cause,
        DisconnectCause::Abusive {
            reason: "oversized_message"
        },
        "the reason is the refusal's own bounded label"
    );
    assert!(departure.settlement.applied());
    assert!(
        departure.cause.is_abuse(),
        "an abuse cut-off is named as one"
    );
    assert_eq!(
        departure.cause.wire_reason(),
        None,
        "the wire has no abuse arm (#815): the peer is hung up, not mislabelled"
    );
    assert_eq!(
        departure.actor,
        Some(actor()),
        "the abusive peer's aircraft binding is released with it"
    );
    assert_eq!(flow.receiver().actor_of(peer(1)), None);
    assert!(!flow.receiver().gate().is_member(peer(1)));
    assert_eq!(
        departure.dropped,
        vec![flag],
        "its carried objective settles exactly like any other departure"
    );

    // Reporting the cut-off again settles nothing further.
    let again = flow
        .depart(
            peer(1),
            DisconnectCause::Abusive {
                reason: "oversized_message",
            },
            Tick(7),
            Some(&mut played),
        )
        .expect("a duplicate report is not an error");
    assert_eq!(again.settlement, Settlement::Duplicate { tick: Tick(7) });
    assert!(again.dropped.is_empty());
    played.close_tick(Tick(7));
    assert_eq!(played.objective_state(flag), Some(ObjectiveState::Dropped));
    let close = played.close_tick(Tick(8));
    assert!(close.rulings.is_empty(), "the drop applied only once");
}

/// Error propagation: a settlement the match refuses is reported under its own
/// name, records nothing, and a corrected report still settles — a refusal is
/// never turned into a silent success, and never into a second application.
#[test]
fn accept_f58_c_a_refused_settlement_is_named_and_recorded_nothing() {
    let (mut played, mut flow, flag) = carrying();

    // The board has already closed tick 5, so a departure stamped at tick 3
    // is late: refused by name, with nothing recorded.
    let error = flow
        .depart(
            peer(1),
            DisconnectCause::Timeout,
            Tick(3),
            Some(&mut played),
        )
        .expect_err("a closed tick cannot take the settlement");
    assert!(
        matches!(
            &error,
            FlowError::Settlement(SessionError::Objective(ObjectiveError::LateEvent { tick }))
                if *tick == Tick(3)
        ),
        "the refusal names the layer and the field: {error}"
    );
    assert!(
        error.to_string().contains("could not be settled"),
        "the error reports itself for a caller that only has Display: {error}"
    );
    assert!(
        !flow.departures().is_settled(peer(1)),
        "a refused departure records nothing, so it can still be corrected"
    );

    // The teardown that already ran is idempotent, and a corrected tick
    // settles the very same departure.
    assert!(!flow.receiver().gate().is_member(peer(1)));
    let settled = flow
        .depart(
            peer(1),
            DisconnectCause::Timeout,
            Tick(6),
            Some(&mut played),
        )
        .expect("the corrected report settles");
    assert_eq!(
        settled.settlement,
        Settlement::Applied { tick: Tick(6) },
        "the first *successful* report is the one that settles"
    );
    assert_eq!(settled.dropped, vec![flag]);
    assert_eq!(flow.departures().len(), 1);
    played.close_tick(Tick(6));
    assert_eq!(played.objective_state(flag), Some(ObjectiveState::Dropped));
}
