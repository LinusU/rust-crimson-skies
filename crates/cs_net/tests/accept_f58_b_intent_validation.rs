//! Acceptance scenario F58-B: intent validation against server-owned actors,
//! loadouts, tick windows and match state — and the refusal of a client that
//! requests damage/score directly.
//!
//! Spec: `specs/F58-network-abuse-resistance-reconnect-and-match-recovery.md`,
//! stage `### F58-B`, minimum scenario "Client requests damage/score directly;
//! server rejects it"; contract `docs/contracts/UI-NETWORK.md` ("Server owns
//! ... hit/damage, faction/interaction ... score and result. Client owns only
//! local input requests"; "A UI action requests a domain transaction; it does
//! not directly edit ... fields"). Task test prefix: `accept_f58_b_`.
//!
//! Every test drives production [`cs_net::validation`] code. Sessions, actors,
//! loadouts and content ids are newly authored synthetic fixtures, never
//! original game data.

use std::collections::BTreeSet;

use cs_net::authority::AuthorityDomain;
use cs_net::lobby::{Loadout, LoadoutProblem, Phase};
use cs_net::validation::{
    ActorOwnership, ClientIntent, IntentRefusal, IntentValidator, MAX_INPUT_TICKS_AHEAD,
    MAX_INPUT_TICKS_BEHIND, MatchStage, SessionViolation, ThreatCase, ThreatDisposition,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
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

fn content(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("a synthetic content id")
}

fn loadout(components: Vec<ContentId>) -> Loadout {
    Loadout {
        blueprint: content(ContentKind::Blueprint, "synthetic_zephyr"),
        components,
    }
}

#[test]
fn accept_f58_b_client_requested_damage_and_score_are_refused() {
    let live = session(11);
    let pilot = peer(1);
    let aircraft = actor(live, 3);
    let mut ownership = ActorOwnership::new();
    ownership
        .bind(pilot, aircraft)
        .expect("the pilot flies this aircraft");

    let bans = BTreeSet::new();
    let stage = MatchStage::new(Phase::InMatch, Tick(500), &bans);
    let mut validator = IntentValidator::designed();

    // The whole family of server-authored truth, each against the authority
    // domain (F54-A) the contract gives the server.
    for (intent, domain) in [
        (
            ClientIntent::ClaimDamage {
                target: aircraft,
                amount: 25,
            },
            AuthorityDomain::HitDamage,
        ),
        (
            ClientIntent::ClaimScore { points: 1_000 },
            AuthorityDomain::ScoreResult,
        ),
        (
            ClientIntent::ClaimHealth {
                actor: aircraft,
                fraction: 1.0,
            },
            AuthorityDomain::HitDamage,
        ),
        (
            ClientIntent::ClaimFaction {
                team: cs_net::lobby::TeamId(1),
            },
            AuthorityDomain::FactionInteraction,
        ),
        (ClientIntent::ClaimOutcome, AuthorityDomain::MissionProgram),
    ] {
        let Err(refusal) = validator.validate(pilot, &intent, &ownership, stage) else {
            panic!("the server accepted a client-authored {intent}");
        };
        assert_eq!(
            refusal,
            IntentRefusal::ClientAuthoredTruth { domain },
            "{intent} must be refused by its own domain"
        );
        assert_eq!(refusal.label(), "client_authored_truth");
        assert_eq!(refusal.threat(), Some(ThreatCase::ClientAuthoredTruth));
        assert_eq!(refusal.disposition(), ThreatDisposition::Disconnect);
        assert!(
            !refusal.to_string().is_empty(),
            "the refusal is a bounded, reportable reason"
        );
    }

    // The refusal changed nothing: no budget was charged for a forged ask,
    // and the ownership table still says what it said.
    assert_eq!(
        validator.budget().peer_count(),
        0,
        "a refused claim is not even charged to the rate budget"
    );
    assert_eq!(ownership.owner(aircraft), Some(pilot));

    // Positive control: the same peer's honest ask is admitted, so the zeroes
    // above are the refusals and not a validator that refuses everything.
    let input = ClientIntent::Input {
        actor: Some(aircraft),
        tick: Tick(500),
    };
    assert_eq!(validator.validate(pilot, &input, &ownership, stage), Ok(()));
    assert_eq!(validator.budget().peer_count(), 1);
}

#[test]
fn accept_f58_b_an_ask_naming_another_pilots_aircraft_is_refused() {
    let live = session(12);
    let owner = peer(1);
    let stranger = peer(2);
    let aircraft = actor(live, 7);
    let mut ownership = ActorOwnership::new();
    ownership
        .bind(owner, aircraft)
        .expect("the owner flies this aircraft");

    let bans = BTreeSet::new();
    let stage = MatchStage::new(Phase::InMatch, Tick(30), &bans);
    let mut validator = IntentValidator::designed();

    // The owner may act for its own aircraft.
    let own = ClientIntent::Input {
        actor: Some(aircraft),
        tick: Tick(30),
    };
    assert_eq!(validator.validate(owner, &own, &ownership, stage), Ok(()));

    // Another pilot may not, and the refusal names the owner (contract:
    // "Server owns ActorId allocation"; the client never names its own).
    let foreign = ClientIntent::Input {
        actor: Some(aircraft),
        tick: Tick(30),
    };
    let refusal = validator
        .validate(stranger, &foreign, &ownership, stage)
        .expect_err("another pilot's aircraft must be refused");
    assert_eq!(
        refusal,
        IntentRefusal::NotOwned(SessionViolation::ActorNotOwned {
            peer: stranger,
            actor: aircraft,
            owner: Some(owner),
        })
    );
    assert_eq!(refusal.threat(), Some(ThreatCase::InvalidOwnership));
    assert_eq!(refusal.disposition(), ThreatDisposition::Disconnect);

    // An aircraft nothing has bound is refused too, and by name.
    let unknown = ClientIntent::Input {
        actor: Some(actor(live, 99)),
        tick: Tick(30),
    };
    let refusal = validator
        .validate(stranger, &unknown, &ownership, stage)
        .expect_err("an unbound aircraft must be refused");
    assert_eq!(
        refusal,
        IntentRefusal::NotOwned(SessionViolation::UnknownActor {
            peer: stranger,
            actor: actor(live, 99),
        })
    );
    assert_eq!(ownership.owner(aircraft), Some(owner), "nothing moved");
}

#[test]
fn accept_f58_b_input_must_sit_inside_the_server_tick_window() {
    let pilot = peer(1);
    let ownership = ActorOwnership::new();
    let bans = BTreeSet::new();
    let server_tick = Tick(1_000);
    let stage = MatchStage::new(Phase::InMatch, server_tick, &bans);
    let mut validator = IntentValidator::designed();

    let mut ask = |tick: Tick| {
        validator.validate(
            pilot,
            &ClientIntent::Input { actor: None, tick },
            &ownership,
            stage,
        )
    };

    // The window edges themselves are accepted...
    assert_eq!(ask(server_tick), Ok(()));
    assert_eq!(ask(Tick(server_tick.0 + MAX_INPUT_TICKS_AHEAD)), Ok(()));
    assert_eq!(ask(Tick(server_tick.0 - MAX_INPUT_TICKS_BEHIND)), Ok(()));

    // ...and one tick past either edge is not.
    let latest = Tick(server_tick.0 + MAX_INPUT_TICKS_AHEAD);
    let too_new = ask(Tick(latest.0 + 1)).expect_err("too far ahead");
    assert_eq!(
        too_new,
        IntentRefusal::TickOutsideWindow {
            tick: Tick(latest.0 + 1),
            earliest: Tick(server_tick.0 - MAX_INPUT_TICKS_BEHIND),
            latest,
        }
    );
    assert_eq!(too_new.disposition(), ThreatDisposition::Absorb);
    assert_eq!(too_new.threat(), None, "an out-of-window tick is not abuse");

    let earliest = Tick(server_tick.0 - MAX_INPUT_TICKS_BEHIND);
    let too_old = ask(Tick(earliest.0 - 1)).expect_err("too far behind");
    assert_eq!(
        too_old,
        IntentRefusal::TickOutsideWindow {
            tick: Tick(earliest.0 - 1),
            earliest,
            latest,
        }
    );
    assert!(too_old.to_string().contains("acceptance window"));
}

#[test]
fn accept_f58_b_a_loadout_ask_is_judged_against_the_phase_and_the_hosts_bans() {
    let pilot = peer(1);
    let ownership = ActorOwnership::new();
    let gun = content(ContentKind::Gun, "synthetic_vulcan");
    let ammo = content(ContentKind::Ammo, "synthetic_20mm");
    let mut bans = BTreeSet::new();
    bans.insert(gun.clone());

    let mut validator = IntentValidator::designed();
    let legal = ClientIntent::EquipLoadout {
        loadout: loadout(vec![ammo.clone()]),
    };
    let smuggled = ClientIntent::EquipLoadout {
        loadout: loadout(vec![gun.clone(), ammo.clone()]),
    };

    // Gathering: a legal loadout is admitted.
    let gathering = MatchStage::new(Phase::Gathering, Tick(5), &bans);
    assert_eq!(
        validator.validate(pilot, &legal, &ownership, gathering),
        Ok(())
    );

    // The host's ban holds even though the shape is perfect.
    let refusal = validator
        .validate(pilot, &smuggled, &ownership, gathering)
        .expect_err("a banned component must be refused");
    assert_eq!(
        refusal,
        IntentRefusal::Loadout(LoadoutProblem::Banned {
            component: gun.clone(),
        })
    );
    assert_eq!(refusal.disposition(), ThreatDisposition::Absorb);
    assert_eq!(refusal.threat(), None);

    // A launch locks loadouts (lobby: "rules and readiness are locked").
    let launching = MatchStage::new(Phase::Launching, Tick(6), &bans);
    let refusal = validator
        .validate(pilot, &legal, &ownership, launching)
        .expect_err("a launch locks loadouts");
    assert_eq!(
        refusal,
        IntentRefusal::WrongPhase {
            phase: Phase::Launching,
            allowed: "a Gathering or InMatch session (a launch locks loadouts)",
        }
    );

    // A running match still allows the swap the docking/pickup path needs.
    let in_match = MatchStage::new(Phase::InMatch, Tick(600), &bans);
    assert_eq!(
        validator.validate(pilot, &legal, &ownership, in_match),
        Ok(())
    );

    // A malformed list is refused before rules are even consulted.
    let malformed = ClientIntent::EquipLoadout {
        loadout: loadout(vec![ammo.clone(), ammo.clone()]),
    };
    let refusal = validator
        .validate(pilot, &malformed, &ownership, gathering)
        .expect_err("a duplicated component must be refused");
    assert_eq!(refusal, IntentRefusal::Loadout(LoadoutProblem::Malformed));

    // ...and so is a loadout whose "blueprint" is not a blueprint at all:
    // the intent layer judges shape exactly as the lobby's own check_loadout
    // does, so an in-session ask cannot slip a malformed list past it. The
    // id below is neither banned nor a component of the list, so the only
    // thing that can refuse it is the blueprint-kind rule itself.
    let not_a_blueprint = ClientIntent::EquipLoadout {
        loadout: Loadout {
            blueprint: content(ContentKind::Engine, "synthetic_merlin"),
            components: vec![ammo.clone()],
        },
    };
    let refusal = validator
        .validate(pilot, &not_a_blueprint, &ownership, gathering)
        .expect_err("a non-blueprint blueprint must be refused");
    assert_eq!(
        refusal,
        IntentRefusal::Loadout(LoadoutProblem::Malformed),
        "the blueprint id must be a blueprint, as the lobby already judges it"
    );

    // Match state also guards input: nothing flies before the match runs.
    let input = ClientIntent::Input {
        actor: None,
        tick: Tick(5),
    };
    let refusal = validator
        .validate(pilot, &input, &ownership, gathering)
        .expect_err("input needs a running match");
    assert_eq!(
        refusal,
        IntentRefusal::WrongPhase {
            phase: Phase::Gathering,
            allowed: "an InMatch session",
        }
    );
    let live_input = ClientIntent::Input {
        actor: None,
        tick: Tick(600),
    };
    assert_eq!(
        validator.validate(pilot, &live_input, &ownership, in_match),
        Ok(()),
        "the same input is fine once the match runs"
    );
}
