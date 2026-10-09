//! Acceptance scenario F58-B: the rate and resource caps behind
//! `ImpossibleRate` and `ResourceExhaustion`.
//!
//! Spec: `specs/F58-network-abuse-resistance-reconnect-and-match-recovery.md`,
//! stage `### F58-B`; contract `docs/contracts/UI-NETWORK.md` ("Wire ids are
//! stable ... Protocol messages carry session epoch and sequence/tick") and
//! spec behavior 1 ("Prevent ... impossible rates ... and resource
//! exhaustion. Disconnect abusive peers with a bounded reason, not a process
//! panic."). Task test prefix: `accept_f58_b_`.
//!
//! Every test drives production [`cs_net::validation`] code with synthetic
//! values, never original game data.

use std::collections::BTreeSet;

use cs_net::bounds::MAX_SESSION_PEERS;
use cs_net::lobby::Phase;
use cs_net::validation::{
    ActorOwnership, ClientIntent, IntentRefusal, IntentValidator, MatchStage, RateBudget,
    RateLimits, ThreatCase, ThreatDisposition,
};
use cs_types::Tick;
use cs_types::net::PeerId;

fn peer(value: u16) -> PeerId {
    PeerId::new(value).expect("a nonzero peer")
}

fn stage<'a>(
    server_tick: Tick,
    bans: &'a BTreeSet<cs_types::content::ContentId>,
) -> MatchStage<'a> {
    MatchStage::new(Phase::InMatch, server_tick, bans)
}

#[test]
fn accept_f58_b_a_flooding_peer_is_cut_off_at_the_designed_rate() {
    let pilot = peer(1);
    let ownership = ActorOwnership::new();
    let bans = BTreeSet::new();
    let mut validator = IntentValidator::designed();

    let limits = validator.budget().limits();
    assert_eq!(limits, RateLimits::DESIGNED);
    assert!(
        limits.max_intents_per_window > 0,
        "the design must allow an honest peer through"
    );

    let intent = ClientIntent::Input {
        actor: None,
        tick: Tick(0),
    };
    let window = stage(Tick(0), &bans);
    for charged in 0..limits.max_intents_per_window {
        if let Err(refusal) = validator.validate(pilot, &intent, &ownership, window) {
            panic!("intent {charged} must be inside the design: {refusal}");
        }
    }

    // One intent past the design: refused with a bounded reason that names
    // the cap, and the session cuts the peer off — no panic, no unbounded
    // work.
    let refusal = validator
        .validate(pilot, &intent, &ownership, window)
        .expect_err("a peer over the design must be refused");
    assert_eq!(
        refusal,
        IntentRefusal::RateExceeded {
            limit: limits.max_intents_per_window,
            window: 0,
        }
    );
    assert_eq!(refusal.threat(), Some(ThreatCase::ImpossibleRate));
    assert_eq!(refusal.disposition(), ThreatDisposition::Disconnect);
    assert!(!refusal.to_string().is_empty());

    // Retrying in the same window gains nothing — the count never dropped
    // below the cap — and the budget's state is still one counter for one
    // peer, however many packets arrived.
    assert_eq!(
        validator.validate(pilot, &intent, &ownership, window),
        Err(IntentRefusal::RateExceeded {
            limit: limits.max_intents_per_window,
            window: 0,
        })
    );
    assert_eq!(
        validator.budget().peer_count(),
        1,
        "state is bounded by peers, not by packets"
    );

    // The window is keyed on the *server's* clock, so only the host's own
    // tick movement opens the next one.
    let next = stage(Tick(limits.window_ticks), &bans);
    let intent_next = ClientIntent::Input {
        actor: None,
        tick: Tick(limits.window_ticks),
    };
    assert_eq!(
        validator.validate(pilot, &intent_next, &ownership, next),
        Ok(())
    );
}

#[test]
fn accept_f58_b_the_rate_budget_is_bounded_by_peers_not_packets() {
    let ownership = ActorOwnership::new();
    let bans = BTreeSet::new();
    let window = stage(Tick(0), &bans);
    let intent = ClientIntent::Input {
        actor: None,
        tick: Tick(0),
    };
    let mut validator = IntentValidator::designed();

    for index in 1..=MAX_SESSION_PEERS as u16 {
        let who = peer(index);
        assert_eq!(
            validator.validate(who, &intent, &ownership, window),
            Ok(()),
            "peer {index} charges inside the design"
        );
    }
    assert_eq!(validator.budget().peer_count(), MAX_SESSION_PEERS);

    // A peer the budget would have to start tracking refuses to grow it: the
    // map stays at the session's peer cap instead of following traffic.
    let stranger = peer(MAX_SESSION_PEERS as u16 + 1);
    let refusal = validator
        .validate(stranger, &intent, &ownership, window)
        .expect_err("the budget must refuse to track another peer");
    assert_eq!(
        refusal,
        IntentRefusal::TooManyPeers {
            max: MAX_SESSION_PEERS,
        }
    );
    assert_eq!(refusal.threat(), Some(ThreatCase::ResourceExhaustion));
    assert_eq!(refusal.disposition(), ThreatDisposition::Disconnect);
    assert_eq!(
        validator.budget().peer_count(),
        MAX_SESSION_PEERS,
        "the refused peer was not tracked"
    );
}

#[test]
fn accept_f58_b_the_budget_charges_only_what_it_admits() {
    let pilot = peer(1);
    let mut budget = RateBudget::designed();
    let limits = budget.limits();

    for charged in 1..=limits.max_intents_per_window {
        assert_eq!(
            budget.charge(pilot, Tick(0), 1),
            Ok(charged),
            "the {charged}th intent charges"
        );
    }
    assert_eq!(
        budget.charge(pilot, Tick(0), 1),
        Err(cs_net::validation::RateViolation::Exceeded {
            limit: limits.max_intents_per_window,
            window: 0,
        })
    );
    // A refused charge writes nothing, so a flood cannot walk the counter
    // upward or grow the map.
    assert_eq!(budget.peer_count(), 1);

    // A peer nobody charged yet is still admitted, independently of the
    // first peer's flood.
    assert_eq!(budget.charge(peer(2), Tick(0), 1), Ok(1));

    // Nothing about the window may be moved by a client: a *cheaper* charge
    // in the same window still finds the cap.
    assert_eq!(
        budget.charge(pilot, Tick(limits.window_ticks - 1), 1),
        Err(cs_net::validation::RateViolation::Exceeded {
            limit: limits.max_intents_per_window,
            window: 0,
        })
    );

    budget.reset();
    assert_eq!(budget.peer_count(), 0, "a fresh epoch starts empty");
    assert_eq!(budget.charge(pilot, Tick(0), 1), Ok(1));
}
