//! Acceptance scenario F58-C: a clean host loss and the reconnect retry.
//!
//! Spec: `specs/F58-network-abuse-resistance-reconnect-and-match-recovery.md`,
//! stage `### F58-C`; non-negotiable 3 — "if the host is gone and the HostLoss
//! policy has no migration, the match ends cleanly and returns clients to
//! menu. Do not claim seamless recovery without implementation." Contract:
//! `docs/contracts/UI-NETWORK.md`. Task test prefix: `accept_f58_c_`.
//!
//! The docking half of the minimum scenario ("disconnect during docking or
//! objective carry") is driven here through [`RecoveryFlow::host_lost`]: the
//! synthetic docking attempt is latched — the state a pilot is in when the
//! link dies — and must reach a terminal state exactly once. The objective
//! carry half lives in
//! `accept_f58_c_disconnect_settles_carried_objectives.rs`.
//!
//! Every value is synthetic fixture data; nothing here is original game data.

use cs_app::network::recovery::{HostLossAction, RecoveryFlow};
use cs_net::compat::PeerAllocator;
use cs_net::lobby::{HostLoss, LateJoin};
use cs_net::recovery::{
    ClientClaim, DisconnectCause, RecoveryDecision, RecoveryKind, RecoveryPolicy, RecoveryRefusal,
    RecoveryRequest, ResumeState, SessionGenerations, Settlement,
};
use cs_net::validation::{SessionViolation, synthetic_fire_message};
use cs_sim::interaction::{
    AbortReason, ControlHolder, EligibilityRefusal, InitiatorMotion, InteractionSession,
    InteractionState, PilotId, SYNTHETIC_INITIATOR, SYNTHETIC_SESSION, TransferLedger,
    evaluate_motion_eligibility, synthetic_docking_anchor, synthetic_docking_envelope,
    synthetic_docking_id, synthetic_docking_transaction, synthetic_hook_trajectory,
};
use cs_types::Tick;
use cs_types::net::{ActorId, PeerId, SessionId};

const SESSION: u64 = 71;
const PILOT: PilotId = PilotId(9);

fn session() -> SessionId {
    SessionId::new(SESSION).expect("a nonzero session")
}

fn peer(number: u16) -> PeerId {
    PeerId::new(number).expect("a nonzero peer number")
}

fn ledger() -> TransferLedger {
    TransferLedger::new(
        SYNTHETIC_SESSION,
        [(SYNTHETIC_INITIATOR, PILOT)],
        [(SYNTHETIC_INITIATOR, 3)],
    )
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

/// A docking attempt latched onto the moving hook: the in-flight
/// authoritative state a disconnect must resolve.
fn docking_in_progress() -> InteractionSession {
    let transaction = synthetic_docking_transaction();
    let id = transaction.id();
    let mut interactions = InteractionSession::new(ledger());
    interactions.open(transaction).expect("the attempt opens");
    assert_eq!(
        interactions
            .observe(id, good_pass())
            .expect("a known attempt"),
        Ok(InteractionState::Latching),
        "the fixture starts mid-dock"
    );
    interactions
}

fn policy() -> RecoveryPolicy {
    RecoveryPolicy {
        late_join: LateJoin::Closed,
        match_running: true,
        authoritative_state: true,
        host_loss: HostLoss::EndSession,
    }
}

fn reconnect(from: SessionId, claimed: Vec<ClientClaim>) -> RecoveryRequest {
    RecoveryRequest {
        kind: RecoveryKind::Reconnect,
        resumed_from: from,
        claimed,
    }
}

/// The docking half of the minimum scenario, driven by the host-loss flow:
/// the match ends cleanly once, the in-flight dock aborts exactly once, and a
/// later report of the same loss changes nothing.
#[test]
fn accept_f58_c_host_loss_ends_the_match_once_and_sends_every_client_to_menu() {
    let mut flow = RecoveryFlow::new(session());
    let mut interactions = docking_in_progress();
    let id = synthetic_docking_id();

    // The host is gone: the first report ends the match and resolves the
    // in-flight dock.
    let first = flow.host_lost(Tick(20), Some(&mut interactions));
    assert_eq!(
        first.settlement,
        Settlement::Applied { tick: Tick(20) },
        "the first report is the one that ends the match"
    );
    assert_eq!(
        first.action,
        HostLossAction::ReturnToMenu,
        "spec non-negotiable 3: clients return to the menu, no migration"
    );
    assert_eq!(
        first.aborted.len(),
        1,
        "the docking attempt the pilot was in is aborted"
    );
    assert_eq!(first.aborted[0].from, InteractionState::Latching);
    assert_eq!(
        first.aborted[0].reason,
        AbortReason::Disconnect,
        "a disconnect aborts, it never completes"
    );
    assert_eq!(
        interactions.state(id),
        Some(InteractionState::Aborted),
        "the authoritative state reached its terminal stage"
    );
    assert_eq!(
        interactions.control_of(SYNTHETIC_INITIATOR),
        ControlHolder::Actor(SYNTHETIC_INITIATOR),
        "pose/control is back with the aircraft, not a dead latch"
    );
    assert!(flow.is_ended());

    // A second report of the same loss (a retransmitted notice) resolves
    // nothing a second time.
    let second = flow.host_lost(Tick(21), Some(&mut interactions));
    assert_eq!(
        second.settlement,
        Settlement::Duplicate { tick: Tick(20) },
        "the match ends once"
    );
    assert!(
        second.aborted.is_empty(),
        "an aborted interaction is terminal: nothing aborts again"
    );
    assert_eq!(
        second.action,
        HostLossAction::ReturnToMenu,
        "the UI action is repeatable; the state is not"
    );
    assert_eq!(
        interactions.state(id),
        Some(InteractionState::Aborted),
        "still exactly one abort"
    );

    // And a returning client cannot resume a match that ended: the flow's own
    // ended state forces `match_running` false for the decision.
    let mut generations = SessionGenerations::starting_at(SESSION + 1);
    let mut peers = PeerAllocator::new();
    let decision = flow.recover(
        reconnect(session(), Vec::new()),
        &policy(),
        &mut generations,
        &mut peers,
    );
    assert_eq!(
        decision.refusal(),
        Some(RecoveryRefusal::MatchEnded),
        "a host loss ends the match; nothing resumes into it"
    );
    assert_eq!(flow.session(), session(), "and the epoch never moved");
}

/// The retry half: a departure followed by a reconnect reopens the boundary
/// on a fresh epoch, admits the newly allocated peer id, and leaves every
/// packet of the old connection stale — while the match-scoped settlement
/// record survives the epoch change.
#[test]
fn accept_f58_c_a_reconnect_reopens_the_boundary_on_a_fresh_epoch() {
    let live = session();
    let aircraft = ActorId {
        session: live,
        serial: 3,
    };
    // One allocator for the whole match, as the host keeps: peer ids are
    // never recycled, so the returning pilot is allocated a *new* one.
    let mut peers = PeerAllocator::new();
    let original = peers.allocate().expect("the first peer id");
    assert_eq!(original, peer(1));

    let mut flow = RecoveryFlow::new(live);
    flow.receiver_mut().admit_peer(original);
    flow.receiver_mut()
        .bind_actor(original, aircraft)
        .expect("the aircraft is free");
    assert_eq!(
        flow.receiver_mut()
            .receive(original, &synthetic_fire_message(live, Tick(9), 1))
            .fires()
            .len(),
        1,
        "the live connection fires before the departure"
    );

    let departure = flow
        .depart(original, DisconnectCause::Voluntary, Tick(11), None)
        .expect("a farewell settles");
    assert_eq!(departure.settlement, Settlement::Applied { tick: Tick(11) });
    assert_eq!(departure.actor, Some(aircraft));
    assert_eq!(flow.receiver().actor_of(original), None);

    // The returning pilot offers state of its own. Every claim is refused
    // and the only state source stays the full authoritative snapshot.
    let claims = vec![ClientClaim::Score { points: 42 }, ClientClaim::Outcome];
    let mut generations = SessionGenerations::starting_at(SESSION + 1);
    let decision = flow.recover(
        reconnect(live, claims.clone()),
        &policy(),
        &mut generations,
        &mut peers,
    );
    let (fresh, returning) = match &decision {
        RecoveryDecision::Resume {
            session,
            peer,
            state,
            refused_claims,
        } => {
            assert_ne!(*session, live, "a reconnect must not resume the old epoch");
            assert!(*session > live, "epochs are monotonic");
            assert_eq!(*state, ResumeState::FullAuthoritativeSnapshot);
            assert_eq!(
                *refused_claims, claims,
                "every client-authored claim is refused, in order"
            );
            (*session, *peer)
        }
        other => panic!("a running match resumes a reconnect: {other:?}"),
    };
    assert_ne!(
        returning, original,
        "a peer id is never recycled within a session"
    );

    // The boundary reopened on the fresh epoch: the old membership and replay
    // windows died with it, and the newly allocated peer id is admitted.
    assert_eq!(flow.session(), fresh);
    assert!(!flow.receiver().gate().is_member(original));
    assert!(
        flow.receiver().gate().is_member(returning),
        "the reconnecting peer is admitted under its new identity"
    );
    assert_eq!(
        flow.receiver().actor_of(returning),
        None,
        "the aircraft is reclaimed through the match's PilotBindings, not by the client"
    );

    // Every packet of the prior connection is stale by construction.
    let stale = flow
        .receiver_mut()
        .receive(returning, &synthetic_fire_message(live, Tick(10), 2));
    assert!(matches!(
        stale.violation(),
        Some(SessionViolation::StaleSession { .. })
    ));
    // …and the fresh epoch admits the new connection's own packets.
    let fresh_packet = flow
        .receiver_mut()
        .receive(returning, &synthetic_fire_message(fresh, Tick(11), 1));
    assert!(
        fresh_packet.accepted(),
        "the reopened boundary admits the reconnect: {fresh_packet:?}"
    );

    // The settlement record is match-scoped: it survives the epoch change, so
    // a duplicate report of the departed peer is still a duplicate.
    assert!(flow.departures().is_settled(original));
    assert_eq!(
        flow.departures().len(),
        1,
        "one departure, one settlement, one epoch later"
    );
}
