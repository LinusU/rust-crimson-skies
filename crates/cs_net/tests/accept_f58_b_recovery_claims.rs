//! Acceptance scenario F58-B: a reconnect's client-authored damage and score
//! claims are refused, and the resumed state comes from the server alone.
//!
//! Spec: `specs/F58-network-abuse-resistance-reconnect-and-match-recovery.md`,
//! stage `### F58-B`, minimum scenario "Client requests damage/score directly;
//! server rejects it"; contract `docs/contracts/UI-NETWORK.md` ("Reconnect
//! uses fresh authenticated session identity and a full authoritative
//! snapshot; a client-provided score, health, faction or outcome is never
//! accepted"). Task test prefix: `accept_f58_b_`.
//!
//! Every test drives production [`cs_net::recovery`] code with synthetic
//! values, never original game data.

use cs_net::compat::PeerAllocator;
use cs_net::lobby::{HostLoss, LateJoin};
use cs_net::recovery::{
    ClientClaim, RecoveryDecision, RecoveryKind, RecoveryPolicy, RecoveryRequest, ResumeState,
    SessionGenerations, decide_recovery,
};
use cs_types::net::{ActorId, SessionId};

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session")
}

fn policy() -> RecoveryPolicy {
    RecoveryPolicy {
        late_join: LateJoin::Closed,
        match_running: true,
        authoritative_state: true,
        host_loss: HostLoss::EndSession,
    }
}

#[test]
fn accept_f58_b_a_reconnect_refuses_a_client_damage_and_score_claim() {
    let prior = session(41);
    let claimed = vec![
        ClientClaim::Score { points: 9_999 },
        ClientClaim::Damage {
            target: ActorId {
                session: prior,
                serial: 5,
            },
            amount: 1_000,
        },
    ];
    let request = RecoveryRequest {
        kind: RecoveryKind::Reconnect,
        resumed_from: prior,
        claimed,
    };
    let mut generations = SessionGenerations::new();
    let mut peers = PeerAllocator::new();

    let decision = decide_recovery(&policy(), &mut generations, &mut peers, &request);
    let (session, state, refused_claims) = match decision {
        RecoveryDecision::Resume {
            session,
            peer: _,
            state,
            refused_claims,
        } => (session, state, refused_claims),
        other => panic!("a reconnect with a live match must resume: {other:?}"),
    };

    // The fresh epoch, not the one the client named.
    assert_ne!(session, prior, "a reconnect never resumes the old epoch");
    assert_eq!(state, ResumeState::FullAuthoritativeSnapshot);

    // Both hostile asks are carried back as refused, in order, and neither is
    // applied anywhere: the only state the client gets is the snapshot.
    assert_eq!(refused_claims.len(), 2, "every claim is refused");
    assert_eq!(refused_claims[0], ClientClaim::Score { points: 9_999 });
    assert_eq!(refused_claims[0].label(), "score");
    assert_eq!(
        refused_claims[1],
        ClientClaim::Damage {
            target: ActorId {
                session: prior,
                serial: 5,
            },
            amount: 1_000,
        }
    );
    assert_eq!(refused_claims[1].label(), "damage");
}

#[test]
fn accept_f58_b_no_reconnect_claim_is_ever_applied_whatever_it_says() {
    let prior = session(42);
    let targets = [
        ClientClaim::Score { points: -1 },
        ClientClaim::Damage {
            target: ActorId {
                session: prior,
                serial: 9,
            },
            amount: u32::MAX,
        },
        ClientClaim::Health { fraction: 1.0 },
        ClientClaim::Faction {
            team: cs_net::lobby::TeamId(1),
        },
        ClientClaim::Outcome,
    ];
    let request = RecoveryRequest {
        kind: RecoveryKind::Reconnect,
        resumed_from: prior,
        claimed: targets.to_vec(),
    };
    let mut generations = SessionGenerations::new();
    let mut peers = PeerAllocator::new();

    let decision = decide_recovery(&policy(), &mut generations, &mut peers, &request);
    let (state, peer, refused_claims) = match decision {
        RecoveryDecision::Resume {
            session: _,
            peer,
            state,
            refused_claims,
        } => (state, peer, refused_claims),
        other => panic!("a reconnect with a live match must resume: {other:?}"),
    };
    assert_eq!(state, ResumeState::FullAuthoritativeSnapshot);
    assert_eq!(
        refused_claims.len(),
        targets.len(),
        "the server refuses every claim it was offered"
    );
    for (offered, refused) in targets.iter().zip(&refused_claims) {
        assert_eq!(offered, refused, "the refusal names the exact claim");
    }

    // The identity is server-allocated, never client-named: the returning
    // pilot comes back under a peer id the server issued.
    assert_eq!(
        peer.get(),
        1,
        "the server allocates a fresh peer, not one the client asked for"
    );
}
