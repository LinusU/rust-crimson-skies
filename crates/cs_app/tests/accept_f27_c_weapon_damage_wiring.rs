//! Acceptance scenario F27-C through the application boundary: a declared gun
//! lowered into the session, its swept hit routed into damage, and AC03
//! ("switch gun bank during cooldown without duplicating fire or refilling
//! ammo") exercised end to end.
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`,
//! stage `### F27-C`. Task test prefix: `accept_f27_c_`.
//! Decision record:
//! `docs/findings/2026-10-02-f27-c-candidate-filtering-and-hit-damage-routing.md`.
//!
//! These tests drive production code only: [`cs_app::weapons`]'s
//! [`lower_gun`], [`lower_rules`] and [`resolve_swept_damage`], over the
//! `cs_sim` resolver, sweep and damage authority they feed. The whole
//! acceptance criterion runs here: a gun the *declared* schema described
//! fires, its round's swept hit becomes `HitEvent`s carrying that gun's own
//! per-channel damage, the session's `DamageResolver` applies them, and
//! switching bank mid-cooldown neither duplicates the fire nor refills the
//! ammunition.
//!
//! Removing the lowering, the rules filter, the per-channel conversion or the
//! resolve-through-the-authority step makes one of them fail.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data. No `CS_GAME_DIR` access.

use std::collections::BTreeMap;

use cs_app::weapons::{lower_gun, lower_rules, resolve_swept_damage};
use cs_content::weapons::{
    DeclaredDamageChannel, DeclaredGunDefinition, DeclaredGunMountKind, DeclaredSpreadCone,
    DeclaredWeaponDamage, declared_synthetic_gun,
};
use cs_sim::damage::{
    ActorId, AttributionRule, DamageChannel, DamageEventKind, DamageNodeKey, DamagePolicy,
    DamageResolver, PartState, synthetic_airframe_graph,
};
use cs_sim::targeting::Allegiance;
use cs_sim::weapons::{
    FireDenialReason, GunBank, GunHitRouter, MountTransform, ProjectileSegment, SweepCandidate,
    SweepTarget, WeaponState,
};
use cs_types::Tick;
use cs_types::content::{Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::{UnitVec3, WorldPosition};

const SESSION: u64 = 29;
const ROUTER_PRODUCER: u32 = 77;
const DAMAGE_PRODUCER: u32 = 78;
const HULL_NODE: &str = "hull";
const WING_MOUNT: &str = "wing_mount_1";

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: SESSION,
        serial,
    }
}

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("test node keys are valid")
}

fn position(value: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(value).expect("test positions are finite")
}

fn claim() -> ClaimId {
    ClaimId::new("f27c.boundary-test").expect("a valid claim id")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim())))
}

/// The declared fixture gun, moved onto another mount so a test can register a
/// two-mount bank. Everything else is the fixture's own declared values.
fn declared_on(mount: &str) -> DeclaredGunDefinition {
    let fixture = declared_synthetic_gun();
    DeclaredGunDefinition::try_new(
        fixture.gun().clone(),
        Origin::SyntheticFixture,
        cs_content::damage::DamageNodeKey::new(mount).expect("a valid mount key"),
        DeclaredGunMountKind::WingLeft,
        fixture.scene_binding().cloned(),
        fixture.caliber().clone(),
        fixture.ammunition().clone(),
        fixture.rate().clone(),
        fixture.muzzle_velocity_mps().clone(),
        fixture.lifetime_ticks().clone(),
        DeclaredSpreadCone {
            half_angle: known(cs_types::space::Radians(0.004)),
        },
        DeclaredWeaponDamage {
            armor: known(1.0),
            internal: known(1.0),
        },
        fixture.inheritance().clone(),
        fixture.effect().clone(),
        fixture.sound().clone(),
        fixture.rules().clone(),
        Provenance::designed(claim()),
    )
    .expect("a valid declared gun")
}

fn mount_transform(origin: [f64; 3]) -> MountTransform {
    MountTransform::try_new(
        position(origin),
        UnitVec3::try_new([0.0, 0.0, -1.0]).expect("forward is a unit axis"),
        [0.0; 3],
    )
    .expect("a valid mount transform")
}

/// The declared gun's own damage amounts, read from the declared record rather
/// than restated, so the assertions cannot drift from the schema.
fn declared_amounts(gun: &DeclaredGunDefinition, channel: DeclaredDamageChannel) -> f64 {
    gun.damage()
        .known_amount(channel)
        .expect("the fixture declares both channel amounts")
}

/// The target box the round crosses: thin along the flight axis and centred on
/// the origin, well inside the fixture muzzle's declared 640 m/s over a 1/30 s
/// tick (21.3 m of travel against 0.5 m of thickness).
fn target_candidate(node: &str) -> SweepCandidate {
    let target = SweepTarget::try_new(actor(2), [0.0; 3], [0.0; 3], [3.0, 3.0, 0.25])
        .expect("a valid target");
    SweepCandidate::new(target, key(node), Some(Allegiance::Hostile))
}

fn tick_segment(shot: &cs_sim::weapons::FireEvent) -> ProjectileSegment {
    let travel_m = shot.projectile.velocity_mps[2].abs() / 30.0;
    ProjectileSegment {
        projectile: shot.projectile.projectile,
        previous: shot.projectile.origin,
        current: position([
            shot.projectile.origin.x(),
            shot.projectile.origin.y(),
            shot.projectile.origin.z() - travel_m,
        ]),
    }
}

fn damage_resolver() -> DamageResolver {
    let mut damage = DamageResolver::new(SESSION, DAMAGE_PRODUCER);
    damage
        .register_actor(
            actor(2),
            synthetic_airframe_graph(),
            DamagePolicy {
                attribution: AttributionRule::FirstLethalHit,
            },
        )
        .expect("the fixture graph registers");
    damage
}

/// The acceptance criterion, end to end through the boundary: a *declared*
/// gun, lowered, fires; its round's swept hit becomes `HitEvent`s whose
/// damage is the declared per-channel amount; and the session's damage
/// authority applies exactly that much.
#[test]
fn accept_f27_c_a_lowered_guns_swept_hit_applies_its_declared_damage() {
    let declared = declared_synthetic_gun();
    let gun = lower_gun(&declared).expect("the declared fixture gun lowers");
    let rules = lower_rules(declared.rules()).expect("the declared rules lower");

    let armor = declared_amounts(&declared, DeclaredDamageChannel::Armor);
    let internal = declared_amounts(&declared, DeclaredDamageChannel::Internal);
    assert_eq!(
        (gun.damage().armor, gun.damage().internal),
        (armor, internal),
        "the lowered gun carries the declared per-channel amounts unchanged"
    );

    let mount = gun.mount().clone();
    let state = WeaponState::try_new(
        std::slice::from_ref(&gun),
        GunBank::try_new([mount.clone()]).expect("a valid bank"),
        cs_sim::weapons::SYNTHETIC_STARTING_ROUNDS,
    )
    .expect("a valid weapon state");
    let mut fire = cs_sim::weapons::FireResolver::new(SESSION, Tick(0));
    fire.register(actor(1), vec![gun], state)
        .expect("the lowered gun registers");

    let resolution = fire
        .resolve(
            &cs_sim::weapons::FireIntent {
                id: cs_sim::weapons::FireIntentId {
                    session: SESSION,
                    tick: Tick(0),
                    producer: 1,
                    sequence: 0,
                },
                shooter: actor(1),
            },
            &BTreeMap::from([(mount, mount_transform([0.0, 0.0, 10.0]))]),
        )
        .expect("the lowered gun fires");
    assert_eq!(resolution.accepted.len(), 1, "the lowered gun fires once");
    let shot = &resolution.accepted[0];
    assert_eq!(
        shot.damage.armor, armor,
        "the accepted shot carries the declared armor amount"
    );
    assert_eq!(shot.damage.internal, internal);

    // The production seam: routing and resolution in one call, against the
    // session's real damage authority.
    let mut router = GunHitRouter::new(SESSION, ROUTER_PRODUCER);
    let mut damage = damage_resolver();
    let outcome = resolve_swept_damage(
        &mut router,
        &rules,
        &mut damage,
        shot,
        &tick_segment(shot),
        [target_candidate(HULL_NODE)],
        Tick(1),
    );

    assert!(
        outcome.refused_contacts().is_empty(),
        "nothing was refused: {outcome:?}"
    );
    assert_eq!(
        outcome.sweep.hits.len(),
        1,
        "the round crossed the target once"
    );
    assert_eq!(outcome.sweep.hits[0].node, key(HULL_NODE));
    let resolution = outcome
        .damage
        .as_ref()
        .expect("the routed batch resolves against the damage authority");
    assert!(
        resolution
            .events
            .iter()
            .any(|event| matches!(event.kind, DamageEventKind::HitApplied { .. })),
        "the session's damage authority applied the routed hits"
    );
    assert_eq!(
        damage.remaining_integrity(&actor(2), &key(HULL_NODE)),
        Some(40.0 - internal),
        "the target lost exactly the declared internal amount"
    );
    assert_eq!(
        damage.remaining_integrity(&actor(2), &key("nose_armor")),
        Some(20.0 - armor),
        "the declared armor zone absorbed exactly the declared armor amount"
    );
    assert_eq!(
        damage.part_state(&actor(2), &key(HULL_NODE)),
        Some(PartState::Damaged)
    );

    // Routing the same segment again applies nothing more: the ledger is what
    // stops it, so a retried schedule step is safe.
    let retry = resolve_swept_damage(
        &mut router,
        &rules,
        &mut damage,
        shot,
        &tick_segment(shot),
        [target_candidate(HULL_NODE)],
        Tick(1),
    );
    assert!(
        retry.is_empty(),
        "a retried route of the same segment is a miss, not a second hit"
    );
    assert_eq!(
        damage.remaining_integrity(&actor(2), &key(HULL_NODE)),
        Some(40.0 - internal),
        "the retry applied no further damage"
    );
}

/// AC03 end to end through the boundary: two *declared* guns on two mounts,
/// a bank switch during the cooldown, and the one shot that happened becoming
/// exactly the declared damage. No duplicate fire, no refilled ammunition and
/// no extra damage.
#[test]
fn accept_f27_c_ac03_through_lowered_guns_switches_bank_mid_cooldown() {
    let nose = declared_synthetic_gun();
    let wing = declared_on(WING_MOUNT);
    let nose_armor = declared_amounts(&nose, DeclaredDamageChannel::Armor);
    let nose_internal = declared_amounts(&nose, DeclaredDamageChannel::Internal);
    let wing_armor = declared_amounts(&wing, DeclaredDamageChannel::Armor);
    let wing_internal = declared_amounts(&wing, DeclaredDamageChannel::Internal);

    let lowered_nose = lower_gun(&nose).expect("the nose gun lowers");
    let lowered_wing = lower_gun(&wing).expect("the wing gun lowers");
    let rules = lower_rules(nose.rules()).expect("the declared rules lower");
    let nose_mount = lowered_nose.mount().clone();
    let wing_mount = lowered_wing.mount().clone();

    let both = GunBank::try_new([nose_mount.clone(), wing_mount.clone()]).expect("a valid bank");
    let mut fire = cs_sim::weapons::FireResolver::new(SESSION, Tick(0));
    fire.register(
        actor(1),
        vec![lowered_nose, lowered_wing],
        WeaponState::try_new(
            &[
                lower_gun(&nose).expect("lowers"),
                lower_gun(&wing).expect("lowers"),
            ],
            both,
            cs_sim::weapons::SYNTHETIC_STARTING_ROUNDS,
        )
        .expect("a valid weapon state"),
    )
    .expect("both lowered guns register");
    let transforms = BTreeMap::from([
        (nose_mount.clone(), mount_transform([0.0, 0.0, 10.0])),
        (wing_mount.clone(), mount_transform([-2.0, 0.0, 10.0])),
    ]);

    let mut router = GunHitRouter::new(SESSION, ROUTER_PRODUCER);
    let mut damage = damage_resolver();

    let before: Vec<u64> = [nose_mount.clone(), wing_mount.clone()]
        .iter()
        .map(|m| fire.state(&actor(1)).expect("registered").ammunition(m))
        .collect();

    // Tick 0: the whole bank fires and both rounds apply their declared
    // damage through the authority.
    let fired = fire
        .resolve(
            &cs_sim::weapons::FireIntent {
                id: cs_sim::weapons::FireIntentId {
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

    // Each accepted shot's round is routed *and* resolved through the production
    // seam. Both resolve on the same tick, so the damage authority sees two
    // same-tick batches and applies each mount's own declared amounts.
    for shot in &fired.accepted {
        let outcome = resolve_swept_damage(
            &mut router,
            &rules,
            &mut damage,
            shot,
            &tick_segment(shot),
            [target_candidate(HULL_NODE)],
            Tick(0),
        );
        assert_eq!(outcome.sweep.hits.len(), 1);
        assert_eq!(
            outcome
                .damage
                .as_ref()
                .expect("each round's batch resolves")
                .tick,
            Tick(0)
        );
    }
    assert_eq!(
        damage.remaining_integrity(&actor(2), &key(HULL_NODE)),
        Some(40.0 - nose_internal - wing_internal),
        "each mount's round applied its own declared internal amount"
    );
    assert_eq!(
        damage.remaining_integrity(&actor(2), &key("nose_armor")),
        Some(20.0 - nose_armor - wing_armor),
        "each mount's round applied its own declared armor amount"
    );

    // Switch to the nose alone mid-cooldown: no refill, no drain.
    fire.state_mut(&actor(1))
        .expect("registered")
        .select(GunBank::try_new([nose_mount.clone()]).expect("a valid bank"));
    let after: Vec<u64> = [nose_mount.clone(), wing_mount.clone()]
        .iter()
        .map(|m| fire.state(&actor(1)).expect("registered").ammunition(m))
        .collect();
    assert_eq!(
        after,
        vec![before[0] - 1, before[1] - 1],
        "selecting a different bank neither refills nor drains either mount"
    );

    // Tick 1: the nose mount is still cooling, so the switched bank fires
    // nothing — no duplicate fire and no damage.
    fire.advance_to(Tick(1));
    let switched = fire
        .resolve(
            &cs_sim::weapons::FireIntent {
                id: cs_sim::weapons::FireIntentId {
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
        vec![FireDenialReason::Cooldown {
            mount: nose_mount.clone(),
            remaining_ticks: u64::from(4u32 - 1),
        }],
        "the refusal names the cooldown the switched-to mount is still serving"
    );

    let hull_after_switch = damage
        .remaining_integrity(&actor(2), &key(HULL_NODE))
        .expect("a known pool");
    damage
        .resolve(Tick(1), &[])
        .expect("a denied fire input applies no damage");
    assert_eq!(
        damage.remaining_integrity(&actor(2), &key(HULL_NODE)),
        Some(hull_after_switch),
        "a denied fire input applied no damage at all"
    );
    assert_eq!(
        fire.state(&actor(1))
            .expect("registered")
            .ammunition(&nose_mount),
        before[0] - 1,
        "the denied shot refilled nothing"
    );
}

/// A round that crosses nothing is reported as a miss with no resolution
/// claims, rather than as an applied batch that happened to be empty.
#[test]
fn accept_f27_c_a_round_that_misses_reports_no_damage_resolution() {
    let declared = declared_synthetic_gun();
    let gun = lower_gun(&declared).expect("the declared fixture gun lowers");
    let rules = lower_rules(declared.rules()).expect("the declared rules lower");
    let mount = gun.mount().clone();

    let state = WeaponState::try_new(
        std::slice::from_ref(&gun),
        GunBank::try_new([mount.clone()]).expect("a valid bank"),
        cs_sim::weapons::SYNTHETIC_STARTING_ROUNDS,
    )
    .expect("a valid weapon state");
    let mut fire = cs_sim::weapons::FireResolver::new(SESSION, Tick(0));
    fire.register(actor(1), vec![gun], state)
        .expect("the lowered gun registers");
    let resolution = fire
        .resolve(
            &cs_sim::weapons::FireIntent {
                id: cs_sim::weapons::FireIntentId {
                    session: SESSION,
                    tick: Tick(0),
                    producer: 1,
                    sequence: 0,
                },
                shooter: actor(1),
            },
            &BTreeMap::from([(mount, mount_transform([0.0, 0.0, 10.0]))]),
        )
        .expect("the lowered gun fires");
    let shot = &resolution.accepted[0];

    // A hostile box the round never reaches, plus the shooter's own box,
    // which the declared self-hit rule excludes.
    let away = SweepTarget::try_new(actor(2), [900.0; 3], [900.0; 3], [1.0; 3])
        .expect("a valid distant target");
    let shooter_box = SweepTarget::try_new(actor(1), [0.0; 3], [0.0; 3], [3.0, 3.0, 0.25])
        .expect("a valid target");

    let mut router = GunHitRouter::new(SESSION, ROUTER_PRODUCER);
    let mut damage = damage_resolver();
    let outcome = resolve_swept_damage(
        &mut router,
        &rules,
        &mut damage,
        shot,
        &tick_segment(shot),
        [
            SweepCandidate::new(away, key(HULL_NODE), Some(Allegiance::Hostile)),
            SweepCandidate::new(shooter_box, key(HULL_NODE), Some(Allegiance::Hostile)),
        ],
        Tick(1),
    );

    assert!(outcome.is_empty(), "the round crossed nothing: {outcome:?}");
    assert!(
        outcome.refused_contacts().is_empty(),
        "a miss is not a refusal"
    );
    assert_eq!(
        outcome.damage,
        Ok(cs_sim::damage::TickResolution {
            tick: Tick(1),
            events: Vec::new()
        }),
        "no batch was resolved, and the tick is still named so a caller can \
         tell a miss from a pass that never ran"
    );
    assert_eq!(
        damage.remaining_integrity(&actor(2), &key(HULL_NODE)),
        Some(40.0),
        "the target is untouched"
    );
}

/// The declared rules decide admission through the boundary: an ally the
/// declared `HostileOnly` rule excludes is never hit, and widening the
/// *declared* rule widens the hit with it.
#[test]
fn accept_f27_c_the_declared_rules_decide_admission_through_the_boundary() {
    let declared = declared_synthetic_gun();
    let gun = lower_gun(&declared).expect("the declared fixture gun lowers");
    let rules = lower_rules(declared.rules()).expect("the declared rules lower");
    let mount = gun.mount().clone();

    let state = WeaponState::try_new(
        std::slice::from_ref(&gun),
        GunBank::try_new([mount.clone()]).expect("a valid bank"),
        cs_sim::weapons::SYNTHETIC_STARTING_ROUNDS,
    )
    .expect("a valid weapon state");
    let mut fire = cs_sim::weapons::FireResolver::new(SESSION, Tick(0));
    fire.register(actor(1), vec![gun], state)
        .expect("the lowered gun registers");
    let resolution = fire
        .resolve(
            &cs_sim::weapons::FireIntent {
                id: cs_sim::weapons::FireIntentId {
                    session: SESSION,
                    tick: Tick(0),
                    producer: 1,
                    sequence: 0,
                },
                shooter: actor(1),
            },
            &BTreeMap::from([(mount, mount_transform([0.0, 0.0, 10.0]))]),
        )
        .expect("the lowered gun fires");
    let shot = &resolution.accepted[0];

    // An ally sitting exactly on the flight path: the same box the hostile
    // test uses, so only the declared relation differs.
    let ally = SweepCandidate::new(
        SweepTarget::try_new(actor(2), [0.0; 3], [0.0; 3], [3.0, 3.0, 0.25])
            .expect("a valid target"),
        key(HULL_NODE),
        Some(Allegiance::Friendly),
    );

    let mut router = GunHitRouter::new(SESSION, ROUTER_PRODUCER);
    let mut damage = damage_resolver();
    let outcome = resolve_swept_damage(
        &mut router,
        &rules,
        &mut damage,
        shot,
        &tick_segment(shot),
        [ally],
        Tick(1),
    );
    assert!(outcome.is_empty(), "the declared rule excluded the ally");
    assert_eq!(
        damage.remaining_integrity(&actor(2), &key(HULL_NODE)),
        Some(40.0),
        "the ally is untouched"
    );

    // And with `DamageChannel` in the picture, the routed amounts are the
    // declared ones on both channels.
    let hostile = resolve_swept_damage(
        &mut router,
        &rules,
        &mut damage,
        shot,
        &tick_segment(shot),
        [target_candidate(HULL_NODE)],
        Tick(1),
    );
    let routed = hostile.sweep.hits[0].damage.clone();
    assert_eq!(
        routed
            .iter()
            .map(|hit| (hit.channel, hit.damage))
            .collect::<Vec<_>>(),
        vec![
            (
                DamageChannel::Armor,
                declared_amounts(&declared, DeclaredDamageChannel::Armor)
            ),
            (
                DamageChannel::Internal,
                declared_amounts(&declared, DeclaredDamageChannel::Internal)
            ),
        ],
        "the routed damage is the declared per-channel amount, in channel order"
    );
}
