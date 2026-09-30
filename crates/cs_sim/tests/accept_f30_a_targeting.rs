//! Acceptance scenario F30-A (the minimum scenario — cycling through
//! equal-distance targets deterministically — and its failure cases):
//! stable ordering, eligibility, allegiance re-evaluation, selection
//! pruning, authoritative threat cues and session confinement.
//!
//! Spec: `specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stage `### F30-A`. Task test prefix: `accept_f30_a_`.
//!
//! These tests drive production code only: [`cs_sim::targeting`]'s
//! [`TargetStore`], [`AllegianceTable`], [`SelectionRequest`] and the
//! `synthetic_roster`/`synthetic_allegiance_table`/
//! `synthetic_target_policy` fixtures. Removing the deterministic
//! ordering, the eligibility gates, the allegiance table or the threat
//! ledger fails the tests — the modules are production code, so removing
//! them fails to compile.
//!
//! Every value here is newly authored synthetic fixture data, never
//! original game data.

use std::collections::BTreeSet;

use cs_sim::damage::{ActorId, HitEventId, LifecycleKind};
use cs_sim::targeting::{
    Allegiance, CycleDirection, SelectionRequest, TargetClass, TargetError, TargetFilter,
    TargetSelection, TargetStore, synthetic_allegiance_table, synthetic_player_faction,
    synthetic_raider_faction, synthetic_roster, synthetic_target_policy, synthetic_trader_faction,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::space::{Radians, UnitVec3, WorldPosition};

const SESSION: u64 = 7;

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: SESSION,
        serial,
    }
}

/// Builds a store seeded with the synthetic roster, optionally shuffling
/// the registration order to prove the ordering is insertion-independent.
fn store(registration_order: &[u64]) -> TargetStore {
    let mut store = TargetStore::new(
        SESSION,
        synthetic_target_policy(),
        synthetic_allegiance_table(),
    );
    let roster = synthetic_roster(SESSION);
    for serial in registration_order {
        let record = roster
            .iter()
            .find(|record| record.actor.serial == *serial)
            .expect("the roster contains every fixture serial")
            .clone();
        store.register(record).expect("fixture records register");
    }
    store
}

/// The roster's registration order, and a deliberately different one.
const ROSTER_ORDER: [u64; 7] = [1, 9, 2, 5, 3, 4, 7];
const SHUFFLED_ORDER: [u64; 7] = [5, 1, 7, 2, 9, 4, 3];

fn cycle_next(store: &TargetStore, selection: &mut TargetSelection) -> Option<ActorId> {
    store
        .apply(
            actor(1),
            selection,
            &SelectionRequest::Cycle {
                direction: CycleDirection::Next,
                filter: TargetFilter::Allegiance(Allegiance::Hostile),
            },
        )
        .expect("the observer is registered")
}

/// AC01 minimum scenario: the three raiders sit exactly 100 m away on
/// three axes, so the distance ordering ties and the stable actor id is
/// the tie-break — the cycle is serials 2, 5, 9 in that order, wrapping,
/// regardless of the order the roster registered in.
#[test]
fn accept_f30_a_equal_distance_targets_cycle_deterministically() {
    for order in [ROSTER_ORDER, SHUFFLED_ORDER] {
        let store = store(&order);
        let mut selection = TargetSelection::new();

        let forward: Vec<ActorId> = (0..5)
            .map(|_| cycle_next(&store, &mut selection).expect("hostiles exist"))
            .collect();
        assert_eq!(
            forward,
            vec![actor(2), actor(5), actor(9), actor(2), actor(5)],
            "equal-distance cycling must follow the stable actor-id order, registration order {order:?}"
        );
        assert_eq!(selection.current(), Some(actor(5)));

        // Prev walks the same total order backwards and wraps.
        let mut selection = TargetSelection::new();
        let backward: Vec<ActorId> = (0..4)
            .map(|_| {
                store
                    .apply(
                        actor(1),
                        &mut selection,
                        &SelectionRequest::Cycle {
                            direction: CycleDirection::Previous,
                            filter: TargetFilter::Allegiance(Allegiance::Hostile),
                        },
                    )
                    .expect("registered")
                    .expect("hostiles exist")
            })
            .collect();
        assert_eq!(backward, vec![actor(9), actor(5), actor(2), actor(9)]);
    }
}

/// Cycling includes only eligible live actors: a destroyed, a hidden and
/// a phase-gated actor are skipped; a held selection on one of them is
/// cleared by `prune` before a consumer can render it stale (AC03's
/// contract half).
#[test]
fn accept_f30_a_cycle_includes_only_eligible_live_actors() {
    let mut store = store(&ROSTER_ORDER);
    let mut selection = TargetSelection::new();

    // The hidden raider (serial 7, 10 m away, revealed=false) is never
    // eligible even though it is the nearest hostile.
    let ordered = store
        .ordered(actor(1), TargetFilter::Allegiance(Allegiance::Hostile))
        .expect("registered");
    assert_eq!(ordered, vec![actor(2), actor(5), actor(9)]);
    assert!(
        !store.eligible(&actor(7)),
        "the hidden raider is ineligible"
    );

    // Destroy raider 5 and phase-gate raider 9: the cycle reduces to 2.
    store
        .record_lifecycle(actor(5), LifecycleKind::Destroyed)
        .expect("registered");
    store
        .set_phase_eligible(actor(9), false)
        .expect("registered");
    assert_eq!(cycle_next(&store, &mut selection), Some(actor(2)));
    assert_eq!(cycle_next(&store, &mut selection), Some(actor(2)));

    // A bailout does not remove targetability; the airframe stays a
    // physical object. Destruction does.
    store
        .record_lifecycle(actor(2), LifecycleKind::PilotBailout)
        .expect("registered");
    assert!(store.eligible(&actor(2)), "a bailed-out airframe remains");
    store
        .record_lifecycle(actor(2), LifecycleKind::Destroyed)
        .expect("registered");
    assert!(!store.eligible(&actor(2)));

    // The selection held actor 2; pruning clears it safely.
    assert_eq!(selection.current(), Some(actor(2)));
    store.prune(&mut selection);
    assert_eq!(selection.current(), None, "a destroyed target clears");
}

/// Non-negotiable 1's contract half: an ownership change
/// (`set_faction`) and a scripted relation change (`set_allegiance`) are
/// reflected on the next query, in the same phase boundary — a target
/// that became friendly is never still reported hostile, and an
/// undeclared relation is `None`, never a guessed allegiance.
#[test]
fn accept_f30_a_faction_and_relation_changes_reclassify_immediately() {
    let mut store = store(&ROSTER_ORDER);
    let (player, raiders) = (synthetic_player_faction(), synthetic_raider_faction());

    assert_eq!(
        store.allegiance(&player, &raiders),
        Some(Allegiance::Hostile)
    );
    assert_eq!(
        store
            .info(actor(1), actor(2))
            .expect("registered")
            .allegiance,
        Some(Allegiance::Hostile)
    );

    // Ownership change: raider 2 is captured into the player's faction.
    store
        .set_faction(actor(2), player.clone())
        .expect("registered");
    let info = store.info(actor(1), actor(2)).expect("registered");
    assert_eq!(info.faction, player);
    assert_eq!(
        info.allegiance,
        Some(Allegiance::Friendly),
        "a captured target is friendly, not silently still hostile"
    );
    let hostiles = store
        .ordered(actor(1), TargetFilter::Allegiance(Allegiance::Hostile))
        .expect("registered");
    assert_eq!(hostiles, vec![actor(5), actor(9)]);

    // Scripted relation change: the factions make peace.
    store.set_allegiance(raiders.clone(), player.clone(), Allegiance::Friendly);
    assert_eq!(
        store.allegiance(&raiders, &player),
        Some(Allegiance::Friendly),
        "the directed pair updates; the reverse stays declared separately"
    );
    assert_eq!(
        store.allegiance(&player, &raiders),
        Some(Allegiance::Hostile)
    );

    // An undeclared relation is an explicit unknown: move raider 9 into a
    // faction with no declared relations and it matches no filter.
    let pirates = ContentId::from_source(ContentKind::Faction, "synthetic.pirates")
        .expect("valid faction id");
    store
        .set_faction(actor(9), pirates.clone())
        .expect("registered");
    assert_eq!(store.allegiance(&player, &pirates), None);
    assert_eq!(
        store
            .info(actor(1), actor(9))
            .expect("registered")
            .allegiance,
        None,
        "undeclared means unknown, never silently hostile"
    );
    for filter in [
        TargetFilter::Allegiance(Allegiance::Hostile),
        TargetFilter::Allegiance(Allegiance::Neutral),
        TargetFilter::Allegiance(Allegiance::Friendly),
    ] {
        let matched = store.ordered(actor(1), filter).expect("registered");
        assert!(
            !matched.contains(&actor(9)),
            "an undeclared relation matches no allegiance filter"
        );
    }
}

/// The remaining filters are distinguishable: class, non-aircraft and
/// objective select different subsets of the same roster.
#[test]
fn accept_f30_a_filters_select_distinguishable_subsets() {
    let store = store(&ROSTER_ORDER);

    let any = store
        .ordered(actor(1), TargetFilter::Any)
        .expect("registered");
    assert_eq!(
        any,
        vec![actor(3), actor(4), actor(2), actor(5), actor(9)],
        "eligible actors by distance: wingman 50 m, trader 60 m, raiders 100 m"
    );
    assert_eq!(
        store
            .ordered(actor(1), TargetFilter::NotClass(TargetClass::Aircraft))
            .expect("registered"),
        vec![actor(4)],
        "the trader is the only eligible non-aircraft"
    );
    assert_eq!(
        store
            .ordered(actor(1), TargetFilter::Objective)
            .expect("registered"),
        vec![actor(4)],
        "the trader is the declared objective"
    );
    assert_eq!(
        store
            .ordered(actor(1), TargetFilter::Allegiance(Allegiance::Friendly))
            .expect("registered"),
        vec![actor(3)],
        "the wingman shares the player's faction — friendly to itself"
    );
    assert_eq!(
        store
            .ordered(actor(1), TargetFilter::Allegiance(Allegiance::Neutral))
            .expect("registered"),
        vec![actor(4)]
    );
}

/// Non-negotiable 4: threat cues come only from recorded authoritative
/// attack events — hostiles circling in range produce no cue — and
/// `NearestAttacker` selects among attackers by the same total order.
#[test]
fn accept_f30_a_threat_cues_come_from_authoritative_attack_events() {
    let mut store = store(&ROSTER_ORDER);
    let mut selection = TargetSelection::new();

    // Raiders in range, no attack recorded: no cue.
    assert!(
        store.threats(actor(1), Tick(10)).is_empty(),
        "proximity alone never mints a threat cue"
    );

    let attack = |attacker: u64, tick: u64, sequence: u32| cs_sim::targeting::AttackEvent {
        attacker: actor(attacker),
        victim: actor(1),
        at: Tick(tick),
        evidence: HitEventId {
            session: SESSION,
            tick: Tick(tick),
            producer: attacker as u32,
            sequence,
        },
    };

    store.record_attack(attack(5, 10, 0)).expect("registered");
    store.record_attack(attack(9, 12, 0)).expect("registered");
    // Redelivery of the same evidence id is idempotent.
    store.record_attack(attack(9, 12, 0)).expect("idempotent");

    let cues = store.threats(actor(1), Tick(20));
    assert_eq!(
        cues.iter()
            .map(|cue| (cue.attacker, cue.last_attack))
            .collect::<Vec<_>>(),
        vec![(actor(9), Tick(12)), (actor(5), Tick(10))],
        "most recent attack first, one cue per attacker"
    );

    // Both attackers are at exactly 100 m: the nearest-attacker pick is
    // the stable-id tie-break again.
    let picked = store
        .apply(
            actor(1),
            &mut selection,
            &SelectionRequest::NearestAttacker { now: Tick(20) },
        )
        .expect("registered");
    assert_eq!(picked, Some(actor(5)));

    // A destroyed attacker is still evidenced but cannot be selected.
    store.record_attack(attack(2, 15, 0)).expect("registered");
    store
        .record_lifecycle(actor(2), LifecycleKind::Destroyed)
        .expect("registered");
    let picked = store
        .apply(
            actor(1),
            &mut selection,
            &SelectionRequest::NearestAttacker { now: Tick(20) },
        )
        .expect("registered");
    assert_eq!(picked, Some(actor(5)), "the destroyed attacker is skipped");

    // Window expiry is the declared policy: at tick 131 raider 5's
    // attack is 121 ticks old (window 120) and has expired, while raider
    // 2's (15) and raider 9's (12) are still inside it — destruction does
    // not erase attack evidence, it only removes the actor from
    // selection.
    let cues = store.threats(actor(1), Tick(131));
    assert_eq!(
        cues.iter()
            .map(|cue| (cue.attacker, cue.last_attack))
            .collect::<Vec<_>>(),
        vec![(actor(2), Tick(15)), (actor(9), Tick(12))]
    );
    assert!(
        store.threats(actor(1), Tick(500)).is_empty(),
        "expired attacks are no longer cues"
    );

    // Attacks must be session-qualified and name registered actors.
    let foreign = cs_sim::targeting::AttackEvent {
        attacker: ActorId {
            session: SESSION + 1,
            serial: 1,
        },
        ..attack(9, 20, 1)
    };
    assert_eq!(
        store.record_attack(foreign),
        Err(TargetError::ForeignSession {
            expected: SESSION,
            found: SESSION + 1,
        })
    );
    let mut foreign_evidence = attack(9, 20, 1);
    foreign_evidence.evidence.session = SESSION + 1;
    assert_eq!(
        store.record_attack(foreign_evidence),
        Err(TargetError::ForeignSession {
            expected: SESSION,
            found: SESSION + 1,
        })
    );
    assert_eq!(
        store.record_attack(attack(42, 20, 1)),
        Err(TargetError::UnknownActor { actor: actor(42) })
    );
}

/// AC04's contract half: the under-crosshair query picks the eligible,
/// unoccluded actor nearest the ray within its cone — nearest by angle
/// then by distance — and reports nothing when nothing qualifies.
#[test]
fn accept_f30_a_crosshair_respects_cone_and_occlusion() {
    let mut store = store(&ROSTER_ORDER);
    let mut selection = TargetSelection::new();
    let origin = WorldPosition::try_new([0.0, 0.0, 0.0]).expect("finite");
    let along_x = UnitVec3::try_new([1.0, 0.0, 0.0]).expect("unit");
    let cone = Radians(std::f64::consts::PI / 18.0); // 10°

    // The +X ray passes through the wingman (50 m) and raider 9 (100 m)
    // dead-centre: equal angle, so the nearer eligible actor wins.
    let query = cs_sim::targeting::CrosshairQuery::try_new(origin, along_x, cone, BTreeSet::new())
        .expect("valid query");
    let picked = store
        .apply(
            actor(1),
            &mut selection,
            &SelectionRequest::UnderCrosshair(query.clone()),
        )
        .expect("registered");
    assert_eq!(picked, Some(actor(3)));

    // The angular tie-break is distance, then actor id: moving raider 9 —
    // the higher serial — nearer than the wingman on the same ray makes
    // it win. An actor-id-only tie-break would still pick serial 3.
    store
        .set_pose(
            actor(9),
            WorldPosition::try_new([40.0, 0.0, 0.0]).expect("finite"),
        )
        .expect("registered");
    let picked = store
        .apply(
            actor(1),
            &mut selection,
            &SelectionRequest::UnderCrosshair(query),
        )
        .expect("registered");
    assert_eq!(
        picked,
        Some(actor(9)),
        "the nearer equal-angle target wins the tie"
    );
    store
        .set_pose(
            actor(9),
            WorldPosition::try_new([100.0, 0.0, 0.0]).expect("finite"),
        )
        .expect("registered");

    // The observer is never its own crosshair pick: from a chase origin
    // behind the player, the observer is dead-centre and nearest, yet the
    // wingman wins — the observer is excluded, not merely skipped when it
    // sits exactly on the ray origin.
    let chase_origin = WorldPosition::try_new([-50.0, 0.0, 0.0]).expect("finite");
    let query =
        cs_sim::targeting::CrosshairQuery::try_new(chase_origin, along_x, cone, BTreeSet::new())
            .expect("valid query");
    let picked = store
        .apply(
            actor(1),
            &mut selection,
            &SelectionRequest::UnderCrosshair(query),
        )
        .expect("registered");
    assert_eq!(
        picked,
        Some(actor(3)),
        "the observer cannot be selected under its own crosshair"
    );

    // Occlude the wingman: the same ray now resolves to raider 9 — the
    // occlusion set is evidence the producer supplies, not a radius guess.
    let query = cs_sim::targeting::CrosshairQuery::try_new(
        origin,
        along_x,
        cone,
        BTreeSet::from([actor(3)]),
    )
    .expect("valid query");
    let picked = store
        .apply(
            actor(1),
            &mut selection,
            &SelectionRequest::UnderCrosshair(query),
        )
        .expect("registered");
    assert_eq!(picked, Some(actor(9)));

    // Occlude everything on the axis: nothing under the crosshair.
    let query = cs_sim::targeting::CrosshairQuery::try_new(
        origin,
        along_x,
        cone,
        BTreeSet::from([actor(3), actor(9)]),
    )
    .expect("valid query");
    let picked = store
        .apply(
            actor(1),
            &mut selection,
            &SelectionRequest::UnderCrosshair(query),
        )
        .expect("registered");
    assert_eq!(picked, None);
    assert_eq!(selection.current(), None);

    // 15° off-axis is outside the 10° cone: nothing qualifies even though
    // an eligible raider sits there.
    let off_axis = {
        let (sin, cos) = (15.0_f64).to_radians().sin_cos();
        UnitVec3::try_new([cos, sin, 0.0]).expect("unit")
    };
    let query = cs_sim::targeting::CrosshairQuery::try_new(origin, off_axis, cone, BTreeSet::new())
        .expect("valid query");
    let picked = store
        .apply(
            actor(1),
            &mut selection,
            &SelectionRequest::UnderCrosshair(query),
        )
        .expect("registered");
    assert_eq!(picked, None, "a target outside the cone is not picked");

    // A malformed cone is refused at the boundary.
    assert_eq!(
        cs_sim::targeting::CrosshairQuery::try_new(
            origin,
            along_x,
            Radians(f64::NAN),
            BTreeSet::new(),
        ),
        Err(cs_sim::targeting::CrosshairError::NonFiniteCone)
    );
    assert_eq!(
        cs_sim::targeting::CrosshairQuery::try_new(origin, along_x, Radians(-0.5), BTreeSet::new(),),
        Err(cs_sim::targeting::CrosshairError::ConeOutOfRange { radians: -0.5 })
    );
}

/// Session confinement: registrations and per-actor updates carrying
/// another session generation are refused by name, and requests from an
/// unregistered observer fail instead of producing a selection.
#[test]
fn accept_f30_a_foreign_session_and_unknown_actors_are_refused() {
    let mut store = store(&ROSTER_ORDER);
    let mut selection = TargetSelection::new();

    let foreign = cs_sim::targeting::TargetRecord {
        actor: ActorId {
            session: SESSION + 1,
            serial: 1,
        },
        ..synthetic_roster(SESSION)[0].clone()
    };
    assert_eq!(
        store.register(foreign),
        Err(TargetError::ForeignSession {
            expected: SESSION,
            found: SESSION + 1,
        })
    );
    assert_eq!(
        store.register(synthetic_roster(SESSION)[0].clone()),
        Err(TargetError::DuplicateActor { actor: actor(1) })
    );

    let unknown = actor(42);
    let position = WorldPosition::try_new([1.0, 0.0, 0.0]).expect("finite");
    assert_eq!(
        store.set_pose(unknown, position),
        Err(TargetError::UnknownActor { actor: unknown })
    );
    assert_eq!(
        store.set_revealed(unknown, true),
        Err(TargetError::UnknownActor { actor: unknown })
    );
    assert_eq!(
        store.record_lifecycle(unknown, LifecycleKind::Destroyed),
        Err(TargetError::UnknownActor { actor: unknown })
    );
    assert_eq!(
        store.apply(
            unknown,
            &mut selection,
            &SelectionRequest::Nearest {
                filter: TargetFilter::Any,
            },
        ),
        Err(TargetError::UnknownActor { actor: unknown }),
        "an unregistered observer gets no selection"
    );

    // The trader faction was declared; the fixture is internally
    // consistent.
    assert_eq!(
        store.allegiance(&synthetic_trader_faction(), &synthetic_raider_faction()),
        Some(Allegiance::Hostile)
    );
}
