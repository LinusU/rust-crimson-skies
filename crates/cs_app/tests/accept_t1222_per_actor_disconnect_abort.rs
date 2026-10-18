//! Acceptance scenario for task #1222: a single peer's mid-dock disconnect,
//! while the match continues, aborts only the interactions its aircraft is a
//! party to.
//!
//! Spec: `specs/F58-network-abuse-resistance-reconnect-and-match-recovery.md`,
//! AC03 "Disconnect during docking or objective carry; authoritative state
//! resolves once" — the mid-match, single-peer docking half, which F58-C
//! recorded as limitation 2 of
//! `docs/findings/2026-10-10-f58-c-disconnect-reconnect-and-host-loss.md`
//! (its `depart` could only `abort_all` or mislabel the reason). Contract:
//! `docs/contracts/UI-NETWORK.md`. Task test prefix: `accept_t1222_`.
//!
//! The producer is the receive path exactly as in F58-C: an admitted
//! farewell through [`RecoveryFlow::receive_and_depart`]. The new consumer is
//! [`InteractionSession::disconnect`], called from [`RecoveryFlow::depart`]
//! on the aircraft the departing peer was bound to: only the non-terminal
//! interactions that name that aircraft — as initiator **or** as target —
//! abort with [`AbortReason::Disconnect`], an honest pilot's in-flight
//! attempt is untouched, and a second report of the same departure aborts
//! nothing.
//!
//! Every value is synthetic fixture data; nothing here is original game data.

use std::collections::BTreeSet;

use cs_app::network::recovery::RecoveryFlow;
use cs_net::lobby::Phase;
use cs_net::message::{ClientMessage, ClientPayload, MessageHeader};
use cs_net::recovery::{DisconnectCause, Settlement};
use cs_net::validation::MatchStage;
use cs_sim::interaction::{
    AbortReason, ControlHolder, EligibilityRefusal, InitiatorMotion, InteractionId,
    InteractionKind, InteractionSession, InteractionState, InteractionTransaction, PilotId,
    SYNTHETIC_INITIATOR, SYNTHETIC_SESSION, TransferLedger, TransferPolicy,
    evaluate_motion_eligibility, synthetic_docking_anchor, synthetic_docking_authorization,
    synthetic_docking_envelope, synthetic_docking_transaction, synthetic_hook_trajectory,
};
use cs_types::Tick;
use cs_types::content::ContentId;
use cs_types::net::{ActorId, PeerId, SessionId};

fn session() -> SessionId {
    SessionId::new(SYNTHETIC_SESSION).expect("the synthetic session is nonzero")
}

fn peer(number: u16) -> PeerId {
    PeerId::new(number).expect("a nonzero peer number")
}

/// The honest pilot's aircraft, in the same session generation as the
/// synthetic fixture.
fn honest_initiator() -> ActorId {
    ActorId {
        session: session(),
        serial: 3,
    }
}

/// The hook the honest pilot is docking onto — not the departing peer's
/// aircraft.
fn honest_target() -> ActorId {
    ActorId {
        session: session(),
        serial: 4,
    }
}

/// A third pilot's aircraft: the initiator whose dock *targets* the
/// departing peer's aircraft.
fn inbound_initiator() -> ActorId {
    ActorId {
        session: session(),
        serial: 5,
    }
}

/// The identity of `inbound_initiator`'s attempt on the departed aircraft.
fn inbound_id() -> InteractionId {
    InteractionId::new(
        SYNTHETIC_SESSION,
        3,
        inbound_initiator(),
        SYNTHETIC_INITIATOR,
    )
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

fn stage(tick: u64, banned: &BTreeSet<ContentId>) -> MatchStage<'_> {
    MatchStage::new(Phase::InMatch, Tick(tick), banned)
}

fn good_pass() -> Result<cs_sim::world_actors::anchor::AnchorSample, EligibilityRefusal> {
    let motion = InitiatorMotion::try_new([9.4, 0.0, 0.0], [10.0, 0.0, 0.0], 1, 10)
        .expect("a finite approach");
    evaluate_motion_eligibility(
        Tick(20),
        &synthetic_hook_trajectory(),
        &synthetic_docking_anchor(),
        &motion,
        &synthetic_docking_envelope(),
    )
}

/// Three pilots, three latched docking attempts — the in-flight state a
/// mid-match disconnect must settle:
///
/// * the **departing** peer's own attempt (`departing`, initiator
///   [`SYNTHETIC_INITIATOR`]),
/// * an **honest** peer's attempt on another hook (`honest`) — the one
///   `abort_all` would wrongly kill — and
/// * a third peer's attempt whose *target* is the departing peer's aircraft
///   (`inbound`), so the per-actor matcher cannot be initiator-only.
fn docking_trio() -> (
    InteractionSession,
    RecoveryFlow,
    InteractionId,
    InteractionId,
) {
    let mut interactions = InteractionSession::new(TransferLedger::new(
        SYNTHETIC_SESSION,
        [
            (SYNTHETIC_INITIATOR, PilotId(9)),
            (honest_initiator(), PilotId(10)),
            (inbound_initiator(), PilotId(11)),
        ],
        [(SYNTHETIC_INITIATOR, 3)],
    ));

    let departing = synthetic_docking_transaction().id();
    interactions
        .open(synthetic_docking_transaction())
        .expect("the departing peer's attempt opens");
    let honest = InteractionId::new(SYNTHETIC_SESSION, 2, honest_initiator(), honest_target());
    interactions
        .open(InteractionTransaction::begin(
            honest,
            InteractionKind::Docking,
            synthetic_docking_authorization(),
            TransferPolicy::docking(),
        ))
        .expect("the honest peer's attempt opens");
    interactions
        .open(InteractionTransaction::begin(
            inbound_id(),
            InteractionKind::Docking,
            synthetic_docking_authorization(),
            TransferPolicy::docking(),
        ))
        .expect("the inbound attempt opens");
    for id in [departing, honest, inbound_id()] {
        assert_eq!(
            interactions
                .observe(id, good_pass())
                .expect("a known attempt"),
            Ok(InteractionState::Latching),
            "the fixture starts mid-dock"
        );
    }

    let mut flow = RecoveryFlow::new(session());
    for (number, aircraft) in [
        (1, SYNTHETIC_INITIATOR),
        (2, honest_initiator()),
        (3, inbound_initiator()),
    ] {
        flow.receiver_mut().admit_peer(peer(number));
        flow.receiver_mut()
            .bind_actor(peer(number), aircraft)
            .expect("the aircraft is free");
    }
    (interactions, flow, departing, honest)
}

/// The minimum scenario: peer 1 departs mid-dock in a running match. Its own
/// attempt and the attempt latched onto its aircraft abort with
/// [`AbortReason::Disconnect`] — exactly once — while the honest pilot's
/// latched attempt is untouched.
#[test]
fn accept_t1222_a_departing_peers_interactions_abort_and_an_honest_pilots_does_not() {
    let (mut interactions, mut flow, departing, honest) = docking_trio();
    let banned = BTreeSet::new();

    // Producer 1: the client's own farewell. The boundary teardown and the
    // departed aircraft's in-flight interactions settle in the same step.
    let settled = flow
        .receive_and_depart(
            peer(1),
            &farewell(1),
            stage(20, &banned),
            None,
            Some(&mut interactions),
        )
        .expect("a farewell settles");
    let departure = settled
        .departure
        .expect("an admitted farewell asks for a departure");
    assert_eq!(departure.cause, DisconnectCause::Voluntary);
    assert_eq!(
        departure.actor,
        Some(SYNTHETIC_INITIATOR),
        "the report names the aircraft the peer flew"
    );
    assert_eq!(
        departure.settlement,
        Settlement::Applied { tick: Tick(20) },
        "the first report is the one that settles"
    );
    assert_eq!(
        departure.aborted.len(),
        2,
        "the departed peer's own attempt and the attempt on its aircraft abort"
    );
    assert_eq!(departure.aborted[0].id, departing);
    assert_eq!(departure.aborted[0].from, InteractionState::Latching);
    assert_eq!(
        departure.aborted[0].reason,
        AbortReason::Disconnect,
        "a disconnect aborts — never a destroy, never a completion"
    );
    assert_eq!(departure.aborted[1].id, inbound_id());
    assert_eq!(departure.aborted[1].reason, AbortReason::Disconnect);

    // The departing peer's attempt reached its terminal stage, and control
    // of its aircraft is back with the aircraft, not a dead latch.
    assert_eq!(
        interactions.state(departing),
        Some(InteractionState::Aborted)
    );
    assert_eq!(
        interactions.state(inbound_id()),
        Some(InteractionState::Aborted),
        "an interaction cannot keep a hook on a departed aircraft"
    );
    assert_eq!(
        interactions.control_of(SYNTHETIC_INITIATOR),
        ControlHolder::Actor(SYNTHETIC_INITIATOR)
    );
    assert_eq!(
        interactions.control_of(inbound_initiator()),
        ControlHolder::Actor(inbound_initiator()),
        "the surviving initiator of the aborted inbound attempt flies again"
    );

    // The honest pilot's attempt is untouched: still latching, its latch
    // controller still owning its pose.
    assert_eq!(
        interactions.state(honest),
        Some(InteractionState::Latching),
        "an honest pilot's in-flight dock never pays for another's disconnect"
    );
    assert_eq!(
        interactions.control_of(honest_initiator()),
        ControlHolder::Latch(honest)
    );

    // The boundary teardown ran in the same step.
    assert_eq!(flow.receiver().actor_of(peer(1)), None);
    assert!(!flow.receiver().gate().is_member(peer(1)));
    assert_eq!(
        flow.receiver().actor_of(peer(2)),
        Some(honest_initiator()),
        "the honest peers keep their aircraft"
    );

    // Producer 2: the transport's dead-link report of the same dead link.
    // The departure already settled; nothing aborts a second time.
    let again = flow
        .depart(
            peer(1),
            DisconnectCause::Timeout,
            Tick(21),
            None,
            Some(&mut interactions),
        )
        .expect("a duplicate report is not an error");
    assert_eq!(
        again.settlement,
        Settlement::Duplicate { tick: Tick(20) },
        "the second report of the same peer must not settle again"
    );
    assert_eq!(
        again.cause,
        DisconnectCause::Voluntary,
        "the cause of record is the first report's"
    );
    assert!(
        again.aborted.is_empty(),
        "an aborted interaction is terminal: nothing aborts again"
    );
    assert!(again.dropped.is_empty());
    assert_eq!(
        interactions.state(departing),
        Some(InteractionState::Aborted),
        "still exactly one abort"
    );
    assert_eq!(
        interactions.state(honest),
        Some(InteractionState::Latching),
        "the honest attempt survived both reports"
    );
}

/// A departing peer that holds no aircraft binding cannot name an actor in
/// the interaction space, so it aborts nothing — the abort is bound to the
/// peer's aircraft, never guessed from the peer id.
#[test]
fn accept_t1222_a_departure_without_an_aircraft_aborts_no_interaction() {
    let (mut interactions, mut flow, departing, honest) = docking_trio();
    // Peer 4 is admitted but never bound to an aircraft.
    flow.receiver_mut().admit_peer(peer(4));

    let departure = flow
        .depart(
            peer(4),
            DisconnectCause::Timeout,
            Tick(20),
            None,
            Some(&mut interactions),
        )
        .expect("a timeout settles");
    assert_eq!(departure.actor, None, "the peer held no aircraft");
    assert_eq!(departure.settlement, Settlement::Applied { tick: Tick(20) });
    assert!(
        departure.aborted.is_empty(),
        "with no bound aircraft there is no interaction to attribute to the peer"
    );
    for id in [departing, honest, inbound_id()] {
        assert_eq!(
            interactions.state(id),
            Some(InteractionState::Latching),
            "every in-flight attempt is untouched"
        );
    }
}
