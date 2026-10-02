//! Acceptance scenario F58-A: the threat model and the session identity gate.
//!
//! Spec: `specs/F58-network-abuse-resistance-reconnect-and-match-recovery.md`,
//! stage `### F58-A`; contract `docs/contracts/UI-NETWORK.md` ("Epoch mismatch
//! rejects stale packets", "Reliable delivery does not replace application
//! idempotency because reconnect/retry can replay requests"). Task test
//! prefix: `accept_f58_a_`.
//!
//! Every test drives production [`cs_net::validation`] code. The session ids,
//! actors and packets are newly authored synthetic fixtures, never original
//! data.

use cs_net::message::{ClientMessage, ClientPayload, InputBatch, MessageHeader};
use cs_net::validation::{
    Admission, SessionGate, SessionIdentity, SessionViolation, ThreatCase, ThreatDisposition,
    synthetic_fire_message, synthetic_idle_message,
};
use cs_types::Tick;
use cs_types::input::InputFrame;
use cs_types::net::{ActorId, PeerId, SessionId};

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session")
}

fn peer(value: u16) -> PeerId {
    PeerId::new(value).expect("a nonzero peer")
}

fn actor(session: SessionId, serial: u64) -> ActorId {
    ActorId { session, serial }
}

#[test]
fn accept_f58_a_every_threat_case_is_named_and_dispositioned_once() {
    let mut labels = Vec::new();
    for case in ThreatCase::ALL {
        let label = case.label();
        assert!(!label.is_empty(), "{case:?} has no label");
        assert!(
            !labels.contains(&label),
            "two threat cases share the label {label}"
        );
        labels.push(label);
        // Every case resolves to one of the three designed responses.
        match case.disposition() {
            ThreatDisposition::Absorb
            | ThreatDisposition::Disconnect
            | ThreatDisposition::Structural => {}
        }
    }
    assert_eq!(labels.len(), ThreatCase::ALL.len());
    assert_eq!(
        ThreatCase::StaleSession.disposition(),
        ThreatDisposition::Absorb
    );
    assert_eq!(
        ThreatCase::ReplayedRequest.disposition(),
        ThreatDisposition::Absorb
    );
    assert_eq!(
        ThreatCase::InvalidOwnership.disposition(),
        ThreatDisposition::Disconnect
    );
    assert_eq!(
        ThreatCase::ResourceExhaustion.disposition(),
        ThreatDisposition::Disconnect
    );
    assert_eq!(
        ThreatCase::ForgedServerAuthority.disposition(),
        ThreatDisposition::Structural
    );
    assert_eq!(
        ThreatCase::ClientAuthoredTruth.disposition(),
        ThreatDisposition::Structural
    );
}

#[test]
fn accept_f58_a_a_stale_epoch_is_absorbed_and_does_not_advance_the_window() {
    let live = session(2);
    let dead = session(1);
    let p = peer(1);
    let mut gate = SessionGate::new(live);
    gate.admit_peer(p);

    // The packet is valid input for a *different* epoch.
    let stale = synthetic_idle_message(dead, Tick(4), 9);
    let admission = gate.admit(p, &stale);
    match admission {
        Admission::Refused(violation) => {
            assert_eq!(
                violation,
                SessionViolation::StaleSession {
                    expected: live,
                    found: dead
                }
            );
            assert_eq!(violation.threat(), ThreatCase::StaleSession);
            assert_eq!(violation.disposition(), ThreatDisposition::Absorb);
        }
        other => panic!("a stale-epoch packet must be refused: {other:?}"),
    }

    // The refused packet did not consume sequence 9: the first packet of the
    // live epoch may still use it.
    let fresh = synthetic_idle_message(live, Tick(5), 9);
    assert!(
        gate.admit(p, &fresh).accepted(),
        "a packet from the live epoch was suppressed by a dead one"
    );
}

#[test]
fn accept_f58_a_a_replayed_sequence_is_absorbed_and_changes_nothing() {
    let live = session(3);
    let p = peer(2);
    let mut gate = SessionGate::new(live);
    gate.admit_peer(p);

    assert!(
        gate.admit(p, &synthetic_idle_message(live, Tick(1), 5))
            .accepted()
    );

    // The same sequence again, and an older one: both replays.
    for presented in [5u32, 4] {
        let admission = gate.admit(p, &synthetic_idle_message(live, Tick(2), presented));
        match &admission {
            Admission::Refused(SessionViolation::ReplayedSequence {
                peer: who,
                presented: at,
                admitted,
            }) => {
                assert_eq!(*who, p);
                assert_eq!(*at, presented);
                assert_eq!(*admitted, 5);
            }
            other => panic!("sequence {presented} must be a replay: {other:?}"),
        }
        assert_eq!(
            admission.violation().unwrap().disposition(),
            ThreatDisposition::Absorb
        );
    }

    // A strictly newer sequence is still admitted after the replays.
    assert!(
        gate.admit(p, &synthetic_idle_message(live, Tick(3), 6))
            .accepted()
    );
}

#[test]
fn accept_f58_a_unauthenticated_peers_and_oversized_packets_disconnect() {
    let live = session(4);
    let mut gate = SessionGate::new(live);
    let unknown = peer(9);
    let admission = gate.admit(unknown, &synthetic_idle_message(live, Tick(1), 1));
    match admission {
        Admission::Refused(violation) => {
            assert_eq!(
                violation,
                SessionViolation::UnauthenticatedPeer { peer: unknown }
            );
            assert_eq!(violation.disposition(), ThreatDisposition::Disconnect);
        }
        other => panic!("an unauthenticated peer must be refused: {other:?}"),
    }

    // Nine frames exceed the declared per-packet cap, so the packet is
    // oversized rather than merely malformed.
    let member = peer(1);
    gate.admit_peer(member);
    let frames: Vec<InputFrame> = (0..9).map(|i| InputFrame::new(Tick(i))).collect();
    let oversized = ClientMessage {
        header: MessageHeader {
            session: live,
            sequence: 1,
        },
        payload: ClientPayload::Input(InputBatch { frames }),
    };
    let admission = gate.admit(member, &oversized);
    match admission {
        Admission::Refused(violation) => {
            assert_eq!(violation.threat(), ThreatCase::OversizedMessage);
            assert_eq!(violation.disposition(), ThreatDisposition::Disconnect);
        }
        other => panic!("an oversized packet must be refused: {other:?}"),
    }
}

#[test]
fn accept_f58_a_ownership_refuses_another_pilots_aircraft() {
    let live = session(5);
    let first = peer(1);
    let second = peer(2);
    let aircraft = actor(live, 7);

    let mut gate = SessionGate::new(live);
    gate.admit_peer(first);
    gate.admit_peer(second);
    gate.ownership_mut()
        .bind(first, aircraft)
        .expect("the first pilot binds its aircraft");

    // The owner is authorized; anyone else is refused and names the owner.
    assert_eq!(gate.ownership().authorize(first, aircraft), Ok(()));
    match gate.ownership().authorize(second, aircraft) {
        Err(SessionViolation::ActorNotOwned { peer, actor, owner }) => {
            assert_eq!(peer, second);
            assert_eq!(actor, aircraft);
            assert_eq!(owner, Some(first));
        }
        other => panic!("another pilot's aircraft must be refused: {other:?}"),
    }
    match gate.ownership().authorize(second, actor(live, 8)) {
        Err(SessionViolation::UnknownActor { peer, actor: named }) => {
            assert_eq!(peer, second);
            assert_eq!(named, actor(live, 8));
        }
        other => panic!("an unbound aircraft must be refused: {other:?}"),
    }
    assert_eq!(
        gate.ownership()
            .authorize(second, aircraft)
            .unwrap_err()
            .disposition(),
        ThreatDisposition::Disconnect
    );
    assert_eq!(gate.ownership().owner(aircraft), Some(first));
    assert_eq!(gate.ownership().actor_of(first), Some(aircraft));
}

#[test]
fn accept_f58_a_the_replay_window_is_bounded_by_peers_not_packets() {
    let live = session(6);
    let p = peer(1);
    let mut gate = SessionGate::new(live);
    gate.admit_peer(p);

    assert!(
        gate.admit(p, &synthetic_idle_message(live, Tick(1), 100))
            .accepted()
    );
    // A thousand replays of the same packet must all be absorbed and must not
    // grow the gate's state.
    for _ in 0..1000 {
        assert!(
            !gate
                .admit(p, &synthetic_idle_message(live, Tick(1), 100))
                .accepted()
        );
    }
    assert_eq!(gate.peer_count(), 1);
    // The window still admits exactly the next sequence.
    assert!(
        gate.admit(p, &synthetic_idle_message(live, Tick(2), 101))
            .accepted()
    );
    assert_eq!(gate.identity(), SessionIdentity::new(live));
}

#[test]
fn accept_f58_a_the_synthetic_fire_fixture_is_a_valid_live_packet() {
    let live = session(7);
    let message = synthetic_fire_message(live, Tick(3), 1);
    assert_eq!(message.validate(), Ok(()));
    assert!(matches!(message.payload, ClientPayload::Input(_)));
    assert!(
        synthetic_idle_message(live, Tick(3), 2).validate().is_ok(),
        "the non-fire fixture is bounded and valid too"
    );
}
