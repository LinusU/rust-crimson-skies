//! Acceptance scenario F27-C: the F27-C candidate query and the routing of a
//! swept hit into `cs_sim::damage::HitEvent` inputs.
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`,
//! stage `### F27-C`. Task test prefix: `accept_f27_c_`.
//! Decision record:
//! `docs/findings/2026-10-02-f27-c-candidate-filtering-and-hit-damage-routing.md`.
//!
//! These tests drive production code only: [`cs_sim::weapons`]'s
//! [`WeaponRules::admit_candidates`], [`Ballistics::sweep_with_sources`] and
//! [`GunHitRouter::route`], plus the real [`FireResolver`] that produced the
//! accepted shot and the real [`DamageResolver`] that consumes the routed
//! hits. The scenario is the acceptance criterion end to end: a hit the sweep
//! found becomes a `HitEvent` whose damage is the gun definition's own
//! per-channel amount, and AC03 ("switch gun bank during cooldown without
//! duplicating fire or refilling ammo") runs through a session that also
//! damages a target.
//!
//! Removing the rules filter, the once-per-`(projectile, actor)` ledger, the
//! per-channel conversion or the candidate's node join makes one of them
//! fail.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data. No `CS_GAME_DIR` access: these tests prove the contract, never
//! the original game.

use std::collections::BTreeMap;

use cs_sim::damage::{
    ActorId, AttributionRule, DamageChannel, DamageEventKind, DamageNodeKey, DamagePolicy,
    DamageResolver, PartState, synthetic_airframe_graph,
};
use cs_sim::targeting::Allegiance;
use cs_sim::weapons::{
    Ballistics, FireEvent, FireIntent, FireIntentId, FireResolver, FriendlyFireRule, GunBank,
    GunHitRouter, SYNTHETIC_ARMOR_DAMAGE, SYNTHETIC_INTERNAL_DAMAGE, SYNTHETIC_STARTING_ROUNDS,
    SelfHitRule, SweepCandidate, SweepRefusal, SweepTarget, WeaponRules, WeaponState,
    synthetic_gun_definition,
};
use cs_types::Tick;
use cs_types::net::SessionId;
use cs_types::space::{UnitVec3, WorldPosition};

/// The session generation, as the weapons module's own ids carry it: a `u64`
/// (F27-A, and the `cs_types` migration task #442).
const SESSION: u64 = 71;
/// The same generation as the shared `cs_types::net` type the routed
/// [`HitEvent`]s carry.
const SESSION_ID: SessionId = match SessionId::new(SESSION) {
    Some(id) => id,
    None => panic!("the test session generation is nonzero"),
};
/// The routing system's producer serial, allocated by the session's schedule.
const ROUTER_PRODUCER: u32 = 900;
/// The damage resolver's producer serial.
const DAMAGE_PRODUCER: u32 = 901;

const HULL_NODE: &str = "hull";
const ARMOR_NODE: &str = "nose_armor";

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: SESSION_ID,
        serial,
    }
}

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("test node keys are valid")
}

fn position(value: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(value).expect("test positions are finite")
}

fn hostile_only() -> WeaponRules {
    WeaponRules {
        self_hit: SelfHitRule::Excluded,
        friendly_fire: FriendlyFireRule::HostileOnly,
        penetration: false,
        ricochet: false,
        ammo_switching: false,
    }
}

/// The fixture gun registered on actor 1, with its bank selected and its mount
/// transform supplied as F27-B's hierarchy walk would.
fn armed_shooter() -> (FireResolver, FireEvent) {
    let definition = synthetic_gun_definition();
    let mount = definition.mount().clone();
    let state = WeaponState::try_new(
        std::slice::from_ref(&definition),
        GunBank::try_new([mount.clone()]).expect("the fixture bank names a mount"),
        SYNTHETIC_STARTING_ROUNDS,
    )
    .expect("the fixture state is valid");
    let mut fire = FireResolver::new(SESSION, Tick(0));
    fire.register(actor(1), vec![definition], state)
        .expect("the fixture gun registers");

    let transform = cs_sim::weapons::MountTransform::try_new(
        position([0.0, 0.0, 10.0]),
        UnitVec3::try_new([0.0, 0.0, -1.0]).expect("forward is a unit axis"),
        [0.0; 3],
    )
    .expect("a valid mount transform");
    let transforms = BTreeMap::from([(mount, transform)]);

    let resolution = fire
        .resolve(
            &FireIntent {
                id: FireIntentId {
                    session: SESSION,
                    tick: Tick(0),
                    producer: 1,
                    sequence: 0,
                },
                shooter: actor(1),
            },
            &transforms,
        )
        .expect("the fixture gun fires");
    assert_eq!(
        resolution.accepted.len(),
        1,
        "the fixture gun fires one shot"
    );
    (
        fire,
        resolution
            .accepted
            .into_iter()
            .next()
            .expect("one accepted shot"),
    )
}

/// The tick-1 segment of `shot`'s projectile: it leaves the muzzle at
/// `z = 10` and, at the fixture's declared 640 m/s over a 1/30 s tick, crosses
/// the target slab at the origin — 21.3 m of travel against a 0.5 m thick
/// target, forty times the thickness, so an endpoint-only test cannot see the
/// hit.
fn tick_segment(shot: &FireEvent) -> cs_sim::weapons::ProjectileSegment {
    let travel_m = shot.projectile.velocity_mps[2].abs() / 30.0;
    cs_sim::weapons::ProjectileSegment {
        projectile: shot.projectile.projectile,
        previous: shot.projectile.origin,
        current: position([
            shot.projectile.origin.x(),
            shot.projectile.origin.y(),
            shot.projectile.origin.z() - travel_m,
        ]),
    }
}

/// A target slab at the origin, thin along the flight axis, whose contact
/// lands on `node`.
fn candidate(node: &str, relation: Option<Allegiance>) -> SweepCandidate {
    let target = SweepTarget::try_new(actor(2), [0.0; 3], [0.0; 3], [3.0, 3.0, 0.25])
        .expect("test targets are valid");
    SweepCandidate::new(target, key(node), relation)
}

/// A damage resolver with the synthetic airframe graph registered for the
/// target and for the shooter.
fn damage_resolver() -> DamageResolver {
    let mut damage = DamageResolver::new(SESSION_ID, DAMAGE_PRODUCER);
    for serial in [1, 2] {
        damage
            .register_actor(
                actor(serial),
                synthetic_airframe_graph(),
                DamagePolicy {
                    attribution: AttributionRule::FirstLethalHit,
                },
            )
            .expect("the fixture graph registers");
    }
    damage
}

/// The acceptance criterion: a swept hit becomes a `HitEvent` whose damage is
/// the gun definition's own per-channel amount, on the node the contact's
/// candidate named — and applying it lands exactly that much on the graph.
#[test]
fn accept_f27_c_a_swept_hit_becomes_one_hit_event_per_declared_channel() {
    let (_fire, shot) = armed_shooter();
    assert_eq!(
        shot.damage,
        *synthetic_gun_definition().damage(),
        "the accepted shot carries the gun definition's own damage profile"
    );

    let segment = tick_segment(&shot);
    let rules = hostile_only();
    let mut router = GunHitRouter::new(SESSION, ROUTER_PRODUCER);

    let outcome = router.route(
        &shot,
        &segment,
        [candidate(HULL_NODE, Some(Allegiance::Hostile))],
        &rules,
        Tick(1),
    );

    assert!(
        outcome.refused.is_empty(),
        "nothing was refused: {outcome:?}"
    );
    assert_eq!(outcome.hits.len(), 1, "the segment crossed the target once");
    let routed = &outcome.hits[0];
    assert_eq!(routed.hit.target, actor(2));
    assert_eq!(
        routed.node,
        key(HULL_NODE),
        "the hit routes to the node the winning candidate named, not a default"
    );

    // One hit per channel, in the declared channel order, each carrying the
    // gun definition's own amount for that channel.
    assert_eq!(
        routed
            .damage
            .iter()
            .map(|hit| (hit.channel, hit.damage))
            .collect::<Vec<_>>(),
        vec![
            (DamageChannel::Armor, SYNTHETIC_ARMOR_DAMAGE),
            (DamageChannel::Internal, SYNTHETIC_INTERNAL_DAMAGE),
        ],
        "each channel carries the gun definition's declared amount, with no multiplier"
    );
    assert!(
        routed
            .damage
            .iter()
            .all(|hit| hit.attacker == Some(actor(1)) && hit.target == actor(2)),
        "every routed hit names the shooter as attacker and the swept actor as target"
    );
    assert!(
        routed.damage.iter().all(|hit| hit.id.session == SESSION_ID),
        "every routed hit is stamped with the routing session"
    );
    assert!(
        routed.damage.iter().all(|hit| hit.id.tick == Tick(1)),
        "the hit is stamped with the tick it landed on, not the tick the round was fired"
    );
    assert!(
        routed.damage[0].id < routed.damage[1].id,
        "the two channels of one contact take ordered, distinct hit ids"
    );

    // The hits are real damage inputs: the authoritative resolver applies them
    // and the graph's pools fall by exactly those amounts. The armor channel
    // enters through the node's declared armor zone, which is the resolver's
    // rule and not the gun's.
    let mut damage = damage_resolver();
    let resolution = damage
        .resolve(Tick(1), &outcome.damage())
        .expect("the routed batch resolves");
    assert_eq!(
        damage.remaining_integrity(&actor(2), &key(HULL_NODE)),
        Some(40.0 - SYNTHETIC_INTERNAL_DAMAGE),
        "the internal channel lands on the hull directly"
    );
    assert_eq!(
        damage.remaining_integrity(&actor(2), &key(ARMOR_NODE)),
        Some(20.0 - SYNTHETIC_ARMOR_DAMAGE),
        "the armor channel is intercepted by the hull's declared armor zone"
    );
    assert!(
        resolution
            .events
            .iter()
            .any(|event| matches!(event.kind, DamageEventKind::HitApplied { .. })),
        "the resolver applied the routed hits"
    );
    assert_eq!(
        damage.part_state(&actor(2), &key(HULL_NODE)),
        Some(PartState::Damaged),
        "the target is damaged, not destroyed, by one fixture round"
    );
}

/// The boundary decision: candidate filtering lives in the **rules query**,
/// not in the geometry. Four actors sit on the flight path — a hostile, a
/// friendly, the shooter itself and an undeclared pair — and only the hostile
/// is admissible under the fixture's `Excluded` / `HostileOnly` rules, so the
/// geometry never runs on the other three.
#[test]
fn accept_f27_c_the_declared_rules_filter_candidates_before_the_geometry() {
    let (_fire, shot) = armed_shooter();
    let segment = tick_segment(&shot);

    // Four distinct actors, every one of them a box the segment crosses.
    let on_path = |serial: u64, relation: Option<Allegiance>| {
        let target = SweepTarget::try_new(actor(serial), [0.0; 3], [0.0; 3], [3.0, 3.0, 0.25])
            .expect("test targets are valid");
        SweepCandidate::new(target, key(HULL_NODE), relation)
    };
    let candidates = [
        on_path(2, Some(Allegiance::Hostile)),
        on_path(3, Some(Allegiance::Friendly)),
        on_path(1, Some(Allegiance::Hostile)),
        on_path(4, None),
    ];

    let admitted = hostile_only().admit_candidates(actor(1), candidates.clone());
    assert_eq!(
        admitted.iter().map(|c| c.target.actor).collect::<Vec<_>>(),
        vec![actor(2)],
        "the declared rules admit the hostile alone: self-hit is excluded, an ally is not \
         admissible, and an undeclared pair is not hostile"
    );

    // The same four boxes handed straight to the geometry — with no rules at
    // all — are all hit. That is what makes the filter load-bearing rather
    // than decorative, and it is why filtering cannot live inside the sweep:
    // the sweep has no declared rule to consult and says yes to all four.
    let mut unfiltered = Ballistics::new();
    let boxes: Vec<_> = candidates.iter().map(|c| c.target).collect();
    assert_eq!(
        unfiltered.sweep_with_sources(&segment, &boxes).len(),
        4,
        "the geometry alone hits every box; only the declared rules decide which are legal"
    );

    // And end to end: routing the same four candidates hits the hostile only.
    let mut router = GunHitRouter::new(SESSION, ROUTER_PRODUCER);
    let outcome = router.route(
        &shot,
        &segment,
        candidates.clone(),
        &hostile_only(),
        Tick(1),
    );
    assert_eq!(outcome.admitted.len(), 1, "only the hostile is admitted");
    assert_eq!(outcome.hits.len(), 1, "only the hostile is hit");
    assert_eq!(outcome.hits[0].hit.target, actor(2));
    assert!(
        !router
            .ballistics()
            .has_hit(shot.projectile.projectile, actor(3)),
        "an ally the rules excluded never reaches the ledger at all"
    );

    // Widening the *declared friendly-fire* rule widens the admission with
    // it, which is the other half of the decision: the filter is data, not a
    // hardcoded hostility test buried in the geometry. The shooter itself
    // stays excluded, because its own `self_hit` rule is still `Excluded` —
    // the two declared rules are independent and neither is inferred from the
    // other.
    let permissive = WeaponRules {
        friendly_fire: FriendlyFireRule::Everyone,
        ..hostile_only()
    };
    assert_eq!(
        permissive
            .admit_candidates(actor(1), candidates)
            .iter()
            .map(|c| c.target.actor)
            .collect::<Vec<_>>(),
        vec![actor(2), actor(3), actor(4)],
        "the declared friendly-fire rule decides admission, not a hardcoded hostility test, \
         and the declared self-hit rule still excludes the shooter"
    );
}

/// The once-per-`(projectile, actor)` guarantee survives routing: a retried
/// route of the same segment — the retry path a schedule takes when a
/// collision feature reports the same contact twice — applies nothing more,
/// and a second projectile over the same path is a separate application.
#[test]
fn accept_f27_c_a_retry_of_the_same_segment_applies_no_further_damage() {
    let (_fire, shot) = armed_shooter();
    let segment = tick_segment(&shot);
    let rules = hostile_only();
    let mut router = GunHitRouter::new(SESSION, ROUTER_PRODUCER);

    let first = router.route(
        &shot,
        &segment,
        [candidate(HULL_NODE, Some(Allegiance::Hostile))],
        &rules,
        Tick(1),
    );
    assert_eq!(first.hits.len(), 1);

    let retry = router.route(
        &shot,
        &segment,
        [candidate(HULL_NODE, Some(Allegiance::Hostile))],
        &rules,
        Tick(1),
    );
    assert!(
        retry.hits.is_empty(),
        "a retried sweep of the same segment applies no further hit"
    );
    assert!(
        retry.refused.is_empty(),
        "a retried sweep is not a refusal either: the round simply already hit"
    );
}

/// A round that crosses nothing is a miss, not a lost hit: the outcome says so
/// rather than being a vector a caller has to interpret, and nothing is
/// routed into damage.
#[test]
fn accept_f27_c_a_round_that_crosses_nothing_reports_a_miss() {
    let (_fire, shot) = armed_shooter();
    let segment = tick_segment(&shot);
    let rules = hostile_only();
    let mut router = GunHitRouter::new(SESSION, ROUTER_PRODUCER);

    // Every candidate is filtered out by the declared rules: one ally and the
    // shooter itself. An empty admitted list with an empty hit list and an
    // empty refusal list is the readable statement "this round crossed
    // nothing it was allowed to hit".
    let ally = candidate(HULL_NODE, Some(Allegiance::Friendly));
    let outcome = router.route(&shot, &segment, [ally], &rules, Tick(1));
    assert!(outcome.is_empty(), "no contact was produced");
    assert!(
        outcome.refused.is_empty(),
        "a filtered-out candidate is a miss, not a refusal: {outcome:?}"
    );
    assert!(
        outcome.damage().is_empty(),
        "nothing was routed into damage, so nothing can be applied"
    );

    // A round whose path misses every box is the same statement.
    let wide_away = SweepTarget::try_new(actor(2), [900.0; 3], [900.0; 3], [1.0; 3])
        .expect("a valid distant box");
    let outcome = router.route(
        &shot,
        &segment,
        [SweepCandidate::new(
            wide_away,
            key(HULL_NODE),
            Some(Allegiance::Hostile),
        )],
        &rules,
        Tick(1),
    );
    assert!(
        outcome.is_empty() && outcome.refused.is_empty(),
        "a hostile box the round never touches is a miss too"
    );
}

/// A channel whose declared amount is zero produces no hit at all: a round
/// that declares no internal damage must not put a zero-damage event in the
/// damage stream.
#[test]
fn accept_f27_c_a_channel_with_no_declared_amount_produces_no_hit() {
    let definition = synthetic_gun_definition();
    let internal_free = cs_sim::weapons::GunDefinition::try_new(
        definition.mount().clone(),
        definition.kind(),
        definition.caliber(),
        definition.ammunition().clone(),
        definition.rate(),
        definition.muzzle_velocity_mps(),
        definition.lifetime_ticks(),
        definition.spread(),
        cs_sim::weapons::WeaponDamage::try_new(SYNTHETIC_ARMOR_DAMAGE, 0.0)
            .expect("a zero internal amount is a valid declared profile"),
        definition.inheritance(),
        definition.effect().clone(),
        definition.sound().clone(),
    )
    .expect("the internal-free gun is valid");

    let mount = internal_free.mount().clone();
    let state = WeaponState::try_new(
        std::slice::from_ref(&internal_free),
        GunBank::try_new([mount.clone()]).expect("a valid bank"),
        SYNTHETIC_STARTING_ROUNDS,
    )
    .expect("the internal-free state is valid");
    let mut fire = FireResolver::new(SESSION, Tick(0));
    fire.register(actor(1), vec![internal_free], state)
        .expect("the internal-free gun registers");
    let transform = cs_sim::weapons::MountTransform::try_new(
        position([0.0, 0.0, 10.0]),
        UnitVec3::try_new([0.0, 0.0, -1.0]).expect("forward is a unit axis"),
        [0.0; 3],
    )
    .expect("a valid mount transform");
    let resolution = fire
        .resolve(
            &FireIntent {
                id: FireIntentId {
                    session: SESSION,
                    tick: Tick(0),
                    producer: 1,
                    sequence: 0,
                },
                shooter: actor(1),
            },
            &BTreeMap::from([(mount, transform)]),
        )
        .expect("the internal-free gun fires");
    let shot = resolution.accepted.into_iter().next().expect("one shot");

    let mut router = GunHitRouter::new(SESSION, ROUTER_PRODUCER);
    let outcome = router.route(
        &shot,
        &tick_segment(&shot),
        [candidate(HULL_NODE, Some(Allegiance::Hostile))],
        &hostile_only(),
        Tick(1),
    );

    assert_eq!(outcome.hits.len(), 1, "the round still hits");
    let routed = &outcome.hits[0];
    assert_eq!(
        routed
            .damage
            .iter()
            .map(|hit| (hit.channel, hit.damage))
            .collect::<Vec<_>>(),
        vec![(DamageChannel::Armor, SYNTHETIC_ARMOR_DAMAGE)],
        "only the channel the gun declares damage on produces a hit"
    );
    assert_eq!(
        routed.amount_on(DamageChannel::Internal),
        0.0,
        "the undeclared channel routed nothing"
    );
}

/// Several part boxes of one actor are separate candidates, and the contact
/// that wins the sweep is the one whose box the round reached first — so the
/// routed node is the part actually reached, not whichever box the caller
/// listed first.
#[test]
fn accept_f27_c_the_routed_node_is_the_part_the_round_reached_first() {
    let (_fire, shot) = armed_shooter();
    let segment = tick_segment(&shot);

    // Two part boxes of the same actor: the engine, whose box is centred 1 m
    // *behind* the slab the round crosses first, and the hull at the slab.
    let engine_box = SweepTarget::try_new(
        actor(2),
        [0.0, 0.0, -1.0],
        [0.0, 0.0, -1.0],
        [3.0, 3.0, 0.25],
    )
    .expect("a valid part box");
    let engine = SweepCandidate::new(engine_box, key("engine_1"), Some(Allegiance::Hostile));
    // Listed second on purpose: the geometry, not the caller's order, must
    // decide which part was reached first.
    let hull = candidate(HULL_NODE, Some(Allegiance::Hostile));

    // A fresh ledger for each pass: the probe below and the routing below are
    // two independent passes over the same segment, and the once-per-
    // `(projectile, actor)` rule would otherwise make the second see nothing.
    let mut probe = Ballistics::new();
    let contacts = probe.sweep_with_sources(&segment, &[engine.target, hull.target]);
    assert_eq!(contacts.len(), 1, "one projectile hits one actor once");
    assert_eq!(
        contacts[0].candidate, 1,
        "the hull box is reached first, so its candidate index is reported"
    );

    let mut router = GunHitRouter::new(SESSION, ROUTER_PRODUCER);
    let outcome = router.route(&shot, &segment, [engine, hull], &hostile_only(), Tick(1));
    assert_eq!(outcome.hits.len(), 1, "one routed hit");
    assert_eq!(
        outcome.hits[0].node,
        key(HULL_NODE),
        "the routed node is the part the round reached first"
    );
    assert!(
        outcome.hits[0]
            .damage
            .iter()
            .all(|hit| hit.node == key(HULL_NODE)),
        "both damage inputs name the same reached part, not the first-listed box"
    );
}

/// AC03 end to end, through a session that also does damage: switching the
/// gun bank mid-cooldown duplicates no fire, refills no ammunition, and the
/// one shot that did happen becomes exactly the gun definition's declared
/// damage on the target — once, because the ledger says so.
#[test]
fn accept_f27_c_ac03_switching_bank_mid_cooldown_neither_duplicates_fire_nor_refills_ammo() {
    let nose = synthetic_gun_definition();
    let nose_mount = nose.mount().clone();

    // A second mount so a bank switch has somewhere to go, with its own
    // declared damage profile so the two are distinguishable in the resolver.
    let wing_mount = key("fixture_wing_mount");
    let wing = cs_sim::weapons::GunDefinition::try_new(
        wing_mount.clone(),
        cs_sim::weapons::GunMountKind::WingLeft,
        nose.caliber(),
        nose.ammunition().clone(),
        cs_sim::weapons::GunRate::try_new(1).expect("a valid rate"),
        nose.muzzle_velocity_mps(),
        nose.lifetime_ticks(),
        nose.spread(),
        cs_sim::weapons::WeaponDamage::try_new(1.0, 1.0).expect("valid damage"),
        nose.inheritance(),
        nose.effect().clone(),
        nose.sound().clone(),
    )
    .expect("the second fixture gun is valid");

    let both = GunBank::try_new([nose_mount.clone(), wing_mount.clone()]).expect("a valid bank");
    let state = WeaponState::try_new(
        &[nose.clone(), wing.clone()],
        both,
        SYNTHETIC_STARTING_ROUNDS,
    )
    .expect("a valid state");
    let mut fire = FireResolver::new(SESSION, Tick(0));
    fire.register(actor(1), vec![nose, wing], state)
        .expect("both guns register");

    let nose_transform = cs_sim::weapons::MountTransform::try_new(
        position([0.0, 0.0, 10.0]),
        UnitVec3::try_new([0.0, 0.0, -1.0]).expect("forward is a unit axis"),
        [0.0; 3],
    )
    .expect("a valid mount transform");
    // The wing gun's muzzle is offset laterally but still inside the target's
    // swept box, so both mounts' rounds reach the target this tick.
    let wing_transform = cs_sim::weapons::MountTransform::try_new(
        position([-2.0, 0.0, 10.0]),
        UnitVec3::try_new([0.0, 0.0, -1.0]).expect("forward is a unit axis"),
        [0.0; 3],
    )
    .expect("a valid mount transform");
    let transforms = BTreeMap::from([
        (nose_mount.clone(), nose_transform),
        (wing_mount.clone(), wing_transform),
    ]);

    let mut damage = damage_resolver();
    let mut router = GunHitRouter::new(SESSION, ROUTER_PRODUCER);
    let rules = hostile_only();

    // Tick 0: the whole bank fires.
    let before: Vec<u64> = [nose_mount.clone(), wing_mount.clone()]
        .iter()
        .map(|mount| fire.state(&actor(1)).expect("registered").ammunition(mount))
        .collect();
    let fired = fire
        .resolve(
            &FireIntent {
                id: FireIntentId {
                    session: SESSION,
                    tick: Tick(0),
                    producer: 1,
                    sequence: 0,
                },
                shooter: actor(1),
            },
            &transforms,
        )
        .expect("the whole bank fires");
    assert_eq!(
        fired.accepted.len(),
        2,
        "a two-mount bank fires both mounts"
    );

    // Both accepted shots become real damage: the nose round's own declared
    // amounts, the wing round's.
    let mut routed_damage = Vec::new();
    for shot in &fired.accepted {
        let segment = tick_segment(shot);
        let outcome = router.route(
            shot,
            &segment,
            [candidate(HULL_NODE, Some(Allegiance::Hostile))],
            &rules,
            Tick(0),
        );
        assert_eq!(
            outcome.hits.len(),
            1,
            "each accepted shot's segment crosses the target once"
        );
        routed_damage.extend(outcome.damage());
    }
    damage
        .resolve(Tick(0), &routed_damage)
        .expect("both rounds' damage resolves together");
    assert_eq!(
        damage.remaining_integrity(&actor(2), &key(HULL_NODE)),
        Some(40.0 - SYNTHETIC_INTERNAL_DAMAGE - 1.0),
        "both rounds' internal damage landed"
    );

    // Switch to the nose alone mid-cooldown.
    fire.state_mut(&actor(1))
        .expect("registered")
        .select(GunBank::try_new([nose_mount.clone()]).expect("a valid bank"));
    let after: Vec<u64> = [nose_mount.clone(), wing_mount]
        .iter()
        .map(|mount| fire.state(&actor(1)).expect("registered").ammunition(mount))
        .collect();
    assert_eq!(
        after,
        vec![before[0] - 1, before[1] - 1],
        "selecting a different bank neither refills nor drains either mount"
    );

    // Tick 1: the nose mount is still cooling, so the switched bank fires
    // nothing — no duplicate fire.
    fire.advance_to(Tick(1));
    let switched = fire
        .resolve(
            &FireIntent {
                id: FireIntentId {
                    session: SESSION,
                    tick: Tick(1),
                    producer: 1,
                    sequence: 1,
                },
                shooter: actor(1),
            },
            &transforms,
        )
        .expect("the switched bank resolves");
    assert!(
        switched.accepted.is_empty(),
        "switching bank during a cooldown does not let the mount fire again"
    );
    assert_eq!(
        switched.refused,
        vec![cs_sim::weapons::FireDenialReason::Cooldown {
            mount: nose_mount.clone(),
            remaining_ticks: u64::from(synthetic_gun_definition().rate().ticks_between_shots() - 1),
        }],
        "the switched-to mount is refused for the cooldown it is still serving"
    );

    // No accepted shot means no damage at all for this tick: a denied input
    // applies nothing anywhere. The already-landed projectile is re-presented
    // to prove the ledger — not a refusal — is what stops it, so even a
    // caller that routed it again would drain nothing.
    let retry = router.route(
        &fired.accepted[0],
        &tick_segment(&fired.accepted[0]),
        [candidate(HULL_NODE, Some(Allegiance::Hostile))],
        &rules,
        Tick(1),
    );
    assert!(
        retry.hits.is_empty(),
        "the already-applied projectile applies no further damage on the switched tick"
    );
    let hull_before = damage
        .remaining_integrity(&actor(2), &key(HULL_NODE))
        .expect("a known pool");
    damage
        .resolve(Tick(1), &retry.damage())
        .expect("an empty batch resolves");
    assert_eq!(
        damage.remaining_integrity(&actor(2), &key(HULL_NODE)),
        Some(hull_before),
        "a denied fire input drains no ammunition and applies no damage"
    );
    assert_eq!(
        fire.state(&actor(1))
            .expect("registered")
            .ammunition(&nose_mount),
        before[0] - 1,
        "the switched bank's denied shot refilled nothing"
    );
}

/// A shot from another session generation, or a segment from another
/// projectile, is refused whole: nothing is admitted and nothing is routed.
#[test]
fn accept_f27_c_foreign_sessions_and_projectiles_are_refused_whole() {
    let (_fire, shot) = armed_shooter();
    let segment = tick_segment(&shot);
    let rules = hostile_only();

    let mut other_session = GunHitRouter::new(SESSION + 1, ROUTER_PRODUCER);
    let refused = other_session.route(
        &shot,
        &segment,
        [candidate(HULL_NODE, Some(Allegiance::Hostile))],
        &rules,
        Tick(1),
    );
    assert_eq!(
        refused.refused,
        vec![SweepRefusal::ForeignSession {
            expected: SESSION + 1,
            found: SESSION,
        }],
        "another session's shot is refused by name"
    );
    assert!(refused.admitted.is_empty() && refused.hits.is_empty());
    assert!(
        !other_session
            .ballistics()
            .has_hit(shot.projectile.projectile, actor(2)),
        "a refused routing applies no geometry either, so a later legitimate \
         pass still finds the contact"
    );

    let mut router = GunHitRouter::new(SESSION, ROUTER_PRODUCER);
    let other_projectile = cs_sim::weapons::ProjectileSegment {
        projectile: cs_sim::weapons::ProjectileId {
            session: SESSION,
            serial: shot.projectile.projectile.serial + 100,
        },
        ..segment
    };
    let refused = router.route(
        &shot,
        &other_projectile,
        [candidate(HULL_NODE, Some(Allegiance::Hostile))],
        &rules,
        Tick(1),
    );
    assert_eq!(
        refused.refused,
        vec![SweepRefusal::ForeignProjectile {
            expected: shot.projectile.projectile,
            found: other_projectile.projectile,
        }],
        "another projectile's segment is refused: a round may not apply another round's damage"
    );
    assert!(refused.hits.is_empty());
}

/// A router opened on session generation zero refuses whole, by name rather
/// than by panicking: the routed [`cs_sim::damage::HitEvent`]s carry the
/// shared `cs_types::net::EventId`, whose session is a nonzero `SessionId` —
/// zero *is* "no session" in that type — so a zero-generation router has
/// nothing a hit could be stamped into.
///
/// A real session never produces this pair. The generation comes from
/// `SessionAllocator`, which issues from 1, and an `ActorId` cannot be built
/// for session 0 at all, so a `FireResolver` on generation zero can never
/// even register a shooter. The test therefore restamps the accepted shot's
/// generation to reach the arm deliberately — the point being that a caller
/// who does the impossible gets a **named refusal**, not a panic from
/// `SessionId::new(..).expect(..)` in the middle of a tick.
#[test]
fn accept_f27_c_a_router_on_session_zero_refuses_whole() {
    let (_fire, shot) = armed_shooter();
    let segment = tick_segment(&shot);
    let mut router = GunHitRouter::new(0, ROUTER_PRODUCER);

    // Reachable in production only as a caller error: the foreign-session
    // refusal fires first for a real shot.
    let foreign = router.route(
        &shot,
        &segment,
        [candidate(HULL_NODE, Some(Allegiance::Hostile))],
        &hostile_only(),
        Tick(1),
    );
    assert_eq!(
        foreign.refused,
        vec![SweepRefusal::ForeignSession {
            expected: 0,
            found: SESSION,
        }],
        "a real shot belongs to a nonzero session, so the foreign-session \
         refusal is what a session-zero router actually sees"
    );
    assert!(foreign.hits.is_empty());

    // And the zero-generation arm itself, reached deliberately.
    let mut zero_shot = shot.clone();
    zero_shot.id.session = 0;
    let refused = router.route(
        &zero_shot,
        &segment,
        [candidate(HULL_NODE, Some(Allegiance::Hostile))],
        &hostile_only(),
        Tick(1),
    );
    assert_eq!(
        refused.refused,
        vec![SweepRefusal::NoSession],
        "there is no session to stamp a hit into, and that is refused by name"
    );
    assert!(refused.hits.is_empty());
    assert_eq!(
        router.ballistics().len(),
        0,
        "a refused routing applies no geometry either"
    );
    assert_eq!(router.routed(), 0, "no hit identity was consumed");
}

/// The routed hit ids are unique and ordered within a session, so a resolver
/// batch can never contain two identical `HitEventId`s.
#[test]
fn accept_f27_c_routed_hit_ids_are_unique_and_ordered() {
    let (_fire, shot) = armed_shooter();
    let rules = hostile_only();
    let mut router = GunHitRouter::new(SESSION, ROUTER_PRODUCER);
    let mut ids = Vec::new();

    for sequence in 0..3u64 {
        let fired = cs_sim::weapons::ProjectileId {
            session: SESSION,
            serial: sequence,
        };
        let mut this_shot = shot.clone();
        this_shot.projectile.projectile = fired;
        let outcome = router.route(
            &this_shot,
            &cs_sim::weapons::ProjectileSegment {
                projectile: fired,
                ..tick_segment(&shot)
            },
            [candidate(HULL_NODE, Some(Allegiance::Hostile))],
            &rules,
            Tick(1),
        );
        ids.extend(outcome.damage().into_iter().map(|hit| hit.id));
    }

    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        ids.len(),
        "every routed hit carries a distinct identity within the session"
    );
    assert_eq!(router.routed() as usize, ids.len());
}
