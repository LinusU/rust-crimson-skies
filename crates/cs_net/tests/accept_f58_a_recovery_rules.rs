//! Acceptance scenario F58-A: reconnect, late-join, binding and reward-replay
//! rules.
//!
//! Spec: `specs/F58-network-abuse-resistance-reconnect-and-match-recovery.md`,
//! stage `### F58-A`, non-negotiable behaviors 3 and 4; contract
//! `docs/contracts/UI-NETWORK.md` ("a client-provided score, health, faction or
//! outcome is never accepted"). Task test prefix: `accept_f58_a_`.
//!
//! Every test drives production [`cs_net::recovery`] code with synthetic
//! values, never original data.

use cs_net::compat::PeerAllocator;
use cs_net::lobby::{HostLoss, LateJoin};
use cs_net::recovery::{
    AwardId, AwardOutcome, BindOutcome, ClientClaim, PilotBindings, RecoveryDecision,
    RecoveryError, RecoveryKind, RecoveryPolicy, RecoveryRefusal, RecoveryRequest, ResumeState,
    RewardLedger, SessionGenerations,
};
use cs_net::validation::SessionViolation;
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

fn policy(late_join: LateJoin) -> RecoveryPolicy {
    RecoveryPolicy {
        late_join,
        match_running: true,
        authoritative_state: true,
        host_loss: HostLoss::EndSession,
    }
}

fn request(kind: RecoveryKind, from: SessionId, claimed: Vec<ClientClaim>) -> RecoveryRequest {
    RecoveryRequest {
        kind,
        resumed_from: from,
        claimed,
    }
}

#[test]
fn accept_f58_a_fresh_epochs_are_issued_and_never_reused() {
    let mut generations = SessionGenerations::new();
    let first = generations.issue().expect("the space is fresh");
    let second = generations.issue().expect("the space is fresh");
    let third = generations.issue().expect("the space is fresh");
    assert!(first < second && second < third, "epochs must increase");
    assert_ne!(first, second);
    assert_ne!(second, third);

    // The top of the space is refused rather than issued and reused.
    let mut exhausted = SessionGenerations::starting_at(u64::MAX);
    assert_eq!(exhausted.issue(), Err(RecoveryError::SessionSpaceExhausted));
    assert_eq!(exhausted.issue(), Err(RecoveryError::SessionSpaceExhausted));
}

#[test]
fn accept_f58_a_a_reconnect_mints_a_new_epoch_and_refuses_every_client_claim() {
    let mut generations = SessionGenerations::new();
    let live = generations
        .issue()
        .expect("the host allocated the live epoch");
    let mut peers = PeerAllocator::new();
    let claims = vec![
        ClientClaim::Score { points: 99 },
        ClientClaim::Health { fraction: 1.0 },
        ClientClaim::Faction {
            team: cs_net::lobby::TeamId(2),
        },
        ClientClaim::Outcome,
        ClientClaim::Aircraft {
            actor: actor(live, 4),
        },
        ClientClaim::Rewards { count: 3 },
    ];
    let decision = cs_net::recovery::decide_recovery(
        &policy(LateJoin::Closed),
        &mut generations,
        &mut peers,
        &request(RecoveryKind::Reconnect, live, claims.clone()),
    );
    match decision {
        RecoveryDecision::Resume {
            session,
            peer,
            state,
            refused_claims,
        } => {
            assert_ne!(session, live, "a reconnect must not resume the old epoch");
            assert!(session > live, "the new epoch is monotonic");
            assert_eq!(state, ResumeState::FullAuthoritativeSnapshot);
            assert_eq!(refused_claims, claims, "every claim is refused, in order");
            assert!(peer.get() >= 1);
        }
        other => panic!("a reconnect while running must resume: {other:?}"),
    }
}

#[test]
fn accept_f58_a_late_join_is_refused_only_when_the_mode_closed_it() {
    let live = session(11);

    let mut closed_generations = SessionGenerations::starting_at(11);
    let mut closed_peers = PeerAllocator::new();
    assert_eq!(
        cs_net::recovery::decide_recovery(
            &policy(LateJoin::Closed),
            &mut closed_generations,
            &mut closed_peers,
            &request(RecoveryKind::LateJoin, live, Vec::new()),
        )
        .refusal(),
        Some(RecoveryRefusal::LateJoinClosed)
    );

    // The same closed mode still readmits a returning pilot.
    let mut open_peers = PeerAllocator::new();
    assert!(
        cs_net::recovery::decide_recovery(
            &policy(LateJoin::Closed),
            &mut closed_generations,
            &mut open_peers,
            &request(RecoveryKind::Reconnect, live, Vec::new()),
        )
        .resumed()
    );
}

#[test]
fn accept_f58_a_a_finished_match_or_missing_state_cannot_be_resumed() {
    let live = session(12);
    let mut generations = SessionGenerations::starting_at(12);

    let ended = RecoveryPolicy {
        match_running: false,
        ..policy(LateJoin::Open)
    };
    assert_eq!(
        cs_net::recovery::decide_recovery(
            &ended,
            &mut generations,
            &mut PeerAllocator::new(),
            &request(RecoveryKind::Reconnect, live, Vec::new()),
        )
        .refusal(),
        Some(RecoveryRefusal::MatchEnded)
    );

    let no_state = RecoveryPolicy {
        authoritative_state: false,
        ..policy(LateJoin::Open)
    };
    assert_eq!(
        cs_net::recovery::decide_recovery(
            &no_state,
            &mut generations,
            &mut PeerAllocator::new(),
            &request(RecoveryKind::Reconnect, live, Vec::new()),
        )
        .refusal(),
        Some(RecoveryRefusal::NoAuthoritativeState)
    );
}

#[test]
fn accept_f58_a_a_reconnect_reclaims_its_own_aircraft_and_no_other() {
    let live = session(13);
    let aircraft = actor(live, 8);
    let old_peer = peer(1);
    let new_peer = peer(2);
    let intruder = peer(3);

    let mut bindings = PilotBindings::new();
    assert_eq!(bindings.bind(old_peer, aircraft), BindOutcome::Bound);
    assert_eq!(bindings.owner(aircraft), Some(old_peer));

    // The returning pilot reclaims the aircraft its own old peer flew.
    assert_eq!(
        bindings.rebind(new_peer, aircraft, old_peer),
        BindOutcome::Reclaimed { from: old_peer }
    );
    assert_eq!(bindings.owner(aircraft), Some(new_peer));

    // A different pilot cannot take it, even by claiming an unrelated prior
    // identity.
    assert_eq!(
        bindings.rebind(intruder, aircraft, intruder),
        BindOutcome::RefusedOtherPilot { owner: new_peer }
    );
    assert_eq!(bindings.owner(aircraft), Some(new_peer));

    // A free aircraft binds fresh.
    let free = actor(live, 9);
    assert_eq!(bindings.bind(intruder, free), BindOutcome::Bound);
}

#[test]
fn accept_f58_a_a_replayed_reward_is_awarded_once_across_epochs() {
    let award = AwardId::new(1).expect("a nonzero award");
    let first_peer = peer(1);
    let returning_peer = peer(2);

    let mut ledger = RewardLedger::new();
    assert!(ledger.is_empty());
    assert_eq!(
        ledger.award(award, first_peer),
        AwardOutcome::Awarded,
        "the first delivery grants the reward"
    );
    assert_eq!(ledger.len(), 1);

    // The same match-stable award replayed after a reconnect (a different
    // peer id, a fresh epoch) is refused, not granted again.
    assert_eq!(
        ledger.award(award, returning_peer),
        AwardOutcome::AlreadyAwarded { first: first_peer }
    );
    assert!(!ledger.award(award, returning_peer).awarded());
    assert_eq!(ledger.len(), 1, "a replay must not grow the ledger");
    assert!(ledger.is_awarded(award));
    assert_eq!(AwardId::new(0), None);
}

#[test]
fn accept_f58_a_a_reconnect_reopens_on_a_fresh_epoch_with_no_old_windows() {
    // The [fresh epoch] rule at the gate: reopening a session receiver on the
    // new epoch drops the old membership and replay windows, so the first
    // packet of the new connection is admitted.
    let old = session(20);
    let new = session(21);
    let p = peer(1);
    let mut gate = cs_net::validation::SessionGate::new(old);
    gate.admit_peer(p);
    assert!(
        gate.admit(
            p,
            &cs_net::validation::synthetic_idle_message(old, cs_types::Tick(1), 1)
        )
        .accepted()
    );

    gate = cs_net::validation::SessionGate::new(new);
    gate.admit_peer(p);
    let stale = gate.admit(
        p,
        &cs_net::validation::synthetic_idle_message(old, cs_types::Tick(2), 2),
    );
    assert!(matches!(
        stale.violation(),
        Some(SessionViolation::StaleSession { .. })
    ));
    assert!(
        gate.admit(
            p,
            &cs_net::validation::synthetic_idle_message(new, cs_types::Tick(3), 2)
        )
        .accepted(),
        "sequence 2 is legal in the fresh epoch"
    );
}

#[test]
fn accept_f58_a_a_full_session_refuses_a_returning_pilot() {
    let live = session(30);
    let mut generations = SessionGenerations::starting_at(30);
    let mut peers = PeerAllocator::new();
    for _ in 0..cs_net::bounds::MAX_SESSION_PEERS {
        peers
            .allocate()
            .expect("the session is not yet at its peer cap");
    }

    // Even a reconnect is refused once no peer id remains, with the cap named
    // rather than silently admitted beyond the declared bound.
    assert_eq!(
        cs_net::recovery::decide_recovery(
            &policy(LateJoin::Open),
            &mut generations,
            &mut peers,
            &request(RecoveryKind::Reconnect, live, Vec::new()),
        )
        .refusal(),
        Some(RecoveryRefusal::SessionFull {
            max: cs_net::bounds::MAX_SESSION_PEERS,
        })
    );
}

#[test]
fn accept_f58_a_an_exhausted_epoch_space_refuses_a_reconnect() {
    let live = session(31);
    let mut generations = SessionGenerations::starting_at(u64::MAX);
    // No fresh epoch can be issued, so the reconnect is refused rather than
    // resumed under a reused or wrapped session id.
    assert_eq!(
        cs_net::recovery::decide_recovery(
            &policy(LateJoin::Open),
            &mut generations,
            &mut PeerAllocator::new(),
            &request(RecoveryKind::Reconnect, live, Vec::new()),
        )
        .refusal(),
        Some(RecoveryRefusal::SessionSpaceExhausted)
    );
}
