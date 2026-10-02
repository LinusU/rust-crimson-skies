//! Acceptance scenarios F27-C through the per-tick weapon session: selection,
//! fire, the accepted-shot effects, the swept damage, the ECS mirror and the
//! teardown.
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`, stage
//! `### F27-C`. Task test prefix: `accept_f27_c_`. Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`. Decision records:
//! `docs/findings/2026-10-02-f27-c-candidate-filtering-and-hit-damage-routing.md`
//! and `docs/findings/2026-10-02-f27-c-weapon-session-wiring.md`.
//!
//! These tests drive production code only: [`cs_app::weapons`]'s
//! [`WeaponSession`], [`step_weapon_session`] and [`sync_round_mirrors`], over
//! the live ECS reads ([`live_mount_transforms`], [`part_sweep_candidates`]),
//! the `cs_sim` cadence and router they feed, and the session's real
//! `DamageResolver`. The minimum acceptance scenario (AC03) runs through the
//! step: the bank fires, its shots emit exactly the declared effects, the bank
//! is switched mid-cooldown, and the switched bank fires nothing, duplicates
//! nothing and refills nothing.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data. No `CS_GAME_DIR` access.

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{ChildOf, GlobalTransform, Vec3, World};
use cs_app::scene::{NodeVisualTransform, SceneGeneration};
use cs_app::weapons::{
    MountPoseRefusal, OrderRefusal, PartSweptBox, RegisteredWeapon, SessionRefusal, StepRefusal,
    WeaponActorBinding, WeaponOrder, WeaponRegistrationError, WeaponRoundMirror, WeaponSession,
    WeaponStep, step_weapon_session, sync_round_mirrors,
};
use cs_content::weapons::{
    DeclaredDamageChannel, DeclaredGunDefinition, DeclaredGunMountKind, declared_synthetic_gun,
};
use cs_sim::damage::{
    ActorId, AttributionRule, DamageEventKind, DamageNodeKey, DamagePolicy, DamageResolver,
    synthetic_airframe_graph,
};
use cs_sim::targeting::Allegiance;
use cs_sim::weapons::{
    CadenceRefusal, FireDenialReason, FireIntent, FireIntentId, GunBank, IntentRefusal,
    SYNTHETIC_STARTING_ROUNDS,
};
use cs_types::Tick;
use cs_types::content::{Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;

const SESSION: u64 = 41;
const ROUTER_PRODUCER: u32 = 91;
const DAMAGE_PRODUCER: u32 = 92;
/// The scene generation every live entity in these tests is stamped under.
const GENERATION: SceneGeneration = SceneGeneration(1);
/// The tick length the fixed step runs at.
const DT_S: f64 = 1.0 / 30.0;
/// The synthetic fixture's own mount, used here as the nose gun.
const NOSE_MOUNT: &str = "gun_mount_1";
const WING_MOUNT: &str = "wing_mount_1";
const HULL: &str = "hull";
/// The muzzle's z, well behind the target box the round crosses.
const MUZZLE_Z: f32 = 10.0;

fn session_id() -> SessionId {
    SessionId::new(SESSION).expect("the test session generation is nonzero")
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session_id(),
        serial,
    }
}

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("test node keys are valid")
}

fn claim() -> ClaimId {
    ClaimId::new("f27c.session-step-test").expect("a valid claim id")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim())))
}

/// The declared fixture gun moved onto `mount` and declared as a `kind` gun.
///
/// `lifetime` is the only value these tests change; everything else is the
/// fixture's own declared record, so the assertions read the same numbers the
/// content schema holds.
fn declared_on(mount: &str, kind: DeclaredGunMountKind, lifetime: u64) -> DeclaredGunDefinition {
    let fixture = declared_synthetic_gun();
    DeclaredGunDefinition::try_new(
        fixture.gun().clone(),
        Origin::SyntheticFixture,
        cs_content::damage::DamageNodeKey::new(mount).expect("a valid mount key"),
        kind,
        fixture.scene_binding().cloned(),
        fixture.caliber().clone(),
        fixture.ammunition().clone(),
        fixture.rate().clone(),
        fixture.muzzle_velocity_mps().clone(),
        known(lifetime),
        fixture.spread().clone(),
        fixture.damage().clone(),
        fixture.inheritance().clone(),
        fixture.effect().clone(),
        fixture.sound().clone(),
        fixture.rules().clone(),
        Provenance::designed(claim()),
    )
    .expect("a valid declared gun")
}

/// The declared internal damage of the fixture gun, read from the declared
/// record rather than restated.
fn declared_internal_damage() -> f64 {
    declared_synthetic_gun()
        .damage()
        .known_amount(DeclaredDamageChannel::Internal)
        .expect("the fixture declares an internal amount")
}

/// The declared muzzle-effect and shot-sound catalog ids, read from the
/// declared record rather than restated.
fn declared_ids() -> (String, String) {
    let declared = declared_synthetic_gun();
    let effect = declared
        .effect()
        .clone()
        .known()
        .expect("the fixture declares a muzzle effect");
    let sound = declared
        .sound()
        .clone()
        .known()
        .expect("the fixture declares a shot sound");
    (effect.as_str().to_owned(), sound.as_str().to_owned())
}

/// The shooter's live hierarchy: an actor root carrying the binding and the
/// airframe velocity, with one mount node per mount.
fn shooter_world(world: &mut World, mounts: &[&str]) -> bevy::prelude::Entity {
    let fixture = declared_synthetic_gun();
    let root = world
        .spawn((
            WeaponActorBinding {
                actor: actor(1),
                guns: vec![fixture.gun().clone()],
                loadout: fixture.gun().clone(),
                generation: GENERATION,
            },
            LinearVelocity(Vec3::ZERO),
        ))
        .id();
    for (index, mount) in mounts.iter().enumerate() {
        let x = 2.0 * index as f32;
        world.spawn((
            cs_app::weapons::MountPoseBinding {
                mount: key(mount),
                generation: GENERATION,
            },
            NodeVisualTransform(GlobalTransform::from_xyz(x, 0.0, MUZZLE_Z)),
            ChildOf(root),
        ));
    }
    root
}

/// The target aircraft: a root with a live velocity and one part box at the
/// origin whose declared geometry the swept query tests.
fn target_world(world: &mut World, half_extents_m: [f64; 3]) {
    let root = world.spawn(LinearVelocity(Vec3::ZERO)).id();
    world.spawn((
        PartSweptBox::new(actor(2), key(HULL), half_extents_m),
        NodeVisualTransform(GlobalTransform::from_xyz(0.0, 0.0, 0.0)),
        ChildOf(root),
    ));
}

/// The session's damage authority, with the target's synthetic airframe graph
/// registered.
fn damage_resolver() -> DamageResolver {
    let mut damage = DamageResolver::new(session_id(), DAMAGE_PRODUCER);
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

/// A weapon session with the fixture's two declared guns mounted on the nose
/// and the left wing, both selected.
fn session_with_two_guns(lifetime: u64) -> (WeaponSession, Vec<RegisteredWeapon>) {
    let nose = declared_on(NOSE_MOUNT, DeclaredGunMountKind::Nose, lifetime);
    let wing = declared_on(WING_MOUNT, DeclaredGunMountKind::WingLeft, lifetime);
    let mut session = WeaponSession::new(SESSION, Tick(0), ROUTER_PRODUCER)
        .expect("a nonzero session generation opens");
    let registered = session
        .register(
            actor(1),
            &[nose, wing],
            GunBank::try_new([key(NOSE_MOUNT), key(WING_MOUNT)]).expect("a valid bank"),
            SYNTHETIC_STARTING_ROUNDS,
        )
        .expect("the declared guns register");
    (session, registered)
}

/// A weapon session with only the fixture's nose gun mounted and selected.
fn session_with_one_gun(lifetime: u64) -> (WeaponSession, Vec<RegisteredWeapon>) {
    let nose = declared_on(NOSE_MOUNT, DeclaredGunMountKind::Nose, lifetime);
    let mut session = WeaponSession::new(SESSION, Tick(0), ROUTER_PRODUCER)
        .expect("a nonzero session generation opens");
    let registered = session
        .register(
            actor(1),
            &[nose],
            bank(&[NOSE_MOUNT]),
            SYNTHETIC_STARTING_ROUNDS,
        )
        .expect("the declared gun registers");
    (session, registered)
}

/// The declared relation of the shooter to the one hostile target.
fn hostile_relation(actor_id: ActorId) -> Option<Allegiance> {
    (actor_id == actor(2)).then_some(Allegiance::Hostile)
}

fn step<'a>(at: Tick, relation: &'a dyn Fn(ActorId) -> Option<Allegiance>) -> WeaponStep<'a> {
    WeaponStep {
        at,
        dt_s: DT_S,
        wind_velocity_m_s: [0.0; 3],
        generation: GENERATION,
        relation,
    }
}

fn intent(tick: u64, sequence: u32) -> FireIntent {
    FireIntent {
        id: FireIntentId {
            session: SESSION,
            tick: Tick(tick),
            producer: 1,
            sequence,
        },
        shooter: actor(1),
    }
}

fn bank(mounts: &[&str]) -> GunBank {
    GunBank::try_new(mounts.iter().map(|mount| key(mount))).expect("a valid bank")
}

/// The accepted AC03 scenario through the per-tick step: the whole bank fires,
/// its shots emit the declared effects and consume one round each, the bank is
/// switched mid-cooldown, and the switched bank fires nothing: no duplicate
/// shot, no second effect, no refilled ammunition and no second damage.
#[test]
fn accept_f27_c_ac03_a_switched_bank_neither_duplicates_fire_nor_refills_ammo() {
    let mut world = World::new();
    shooter_world(&mut world, &[NOSE_MOUNT, WING_MOUNT]);
    let (mut session, registered) = session_with_two_guns(90);
    let mut damage = damage_resolver();
    let relation = hostile_relation;

    assert_eq!(
        registered
            .iter()
            .map(|gun| gun.mount.as_str())
            .collect::<Vec<_>>(),
        vec![NOSE_MOUNT, WING_MOUNT],
        "registration reports the lowered mount keys the scene wiring binds"
    );
    let nose = registered[0].mount.clone();
    let wing = registered[1].mount.clone();

    // Tick 0: the whole bank fires.
    let fired = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[WeaponOrder::Fire(intent(0, 0))],
        &step(Tick(0), &relation),
    );
    assert_eq!(
        fired.accepted.len(),
        2,
        "a two-mount bank fires both mounts"
    );
    assert_eq!(
        fired.effects.len(),
        2,
        "each accepted shot emits one effect"
    );
    assert!(fired.denied.is_empty(), "both mounts were ready: {fired:?}");
    assert_eq!(fired.routed.len(), 2, "both fresh rounds were swept");
    assert!(
        fired.routed.iter().all(|routed| routed.outcome.is_empty()),
        "an empty world is crossed by nothing: {fired:?}"
    );
    let rounds = session.round_ids();
    assert_eq!(rounds.len(), 2, "two rounds are live");

    let ammunition = |session: &WeaponSession| {
        [
            session
                .state(&actor(1))
                .expect("registered")
                .ammunition(&nose),
            session
                .state(&actor(1))
                .expect("registered")
                .ammunition(&wing),
        ]
    };
    assert_eq!(
        ammunition(&session),
        [SYNTHETIC_STARTING_ROUNDS - 1, SYNTHETIC_STARTING_ROUNDS - 1],
        "each accepted shot consumed exactly one round"
    );

    // The same order replayed on the same tick is refused by the once-only
    // intent id and consumes nothing.
    let duplicate = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[WeaponOrder::Fire(intent(0, 0))],
        &step(Tick(1), &relation),
    );
    assert_eq!(
        duplicate.fire_refused,
        vec![CadenceRefusal::Intent(IntentRefusal::ForeignTick {
            expected: Tick(1),
            found: Tick(0),
        })],
        "a packet for an already-resolved tick is refused by name"
    );
    assert!(
        duplicate.accepted.is_empty() && duplicate.effects.is_empty(),
        "the duplicate packet fired nothing and emitted nothing"
    );
    assert_eq!(
        ammunition(&session),
        [SYNTHETIC_STARTING_ROUNDS - 1, SYNTHETIC_STARTING_ROUNDS - 1],
        "the duplicate packet drained nothing"
    );
    assert_eq!(session.round_ids().len(), 2, "no third round exists");

    // Tick 2: the bank is switched to the nose alone while it is still cooling
    // down, and the switched bank is asked to fire.
    let switched = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[
            WeaponOrder::SelectBank {
                shooter: actor(1),
                bank: bank(&[NOSE_MOUNT]),
            },
            WeaponOrder::Fire(intent(2, 1)),
        ],
        &step(Tick(2), &relation),
    );
    assert!(
        switched.accepted.is_empty(),
        "switching bank during a cooldown does not let a mount fire again"
    );
    assert_eq!(
        switched.denied,
        vec![cs_app::weapons::DeniedShot {
            intent: intent(2, 1).id,
            reason: FireDenialReason::Cooldown {
                mount: nose.clone(),
                remaining_ticks: 2,
            },
        }],
        "the refusal names the cooldown the switched-to mount is still serving"
    );
    assert!(
        switched.effects.is_empty(),
        "a denied shot emits no sound and no muzzle effect"
    );
    assert_eq!(
        ammunition(&session),
        [SYNTHETIC_STARTING_ROUNDS - 1, SYNTHETIC_STARTING_ROUNDS - 1],
        "the switch refilled nothing and the denied shot drained nothing"
    );
    assert_eq!(session.round_ids().len(), 2, "no new round was spawned");
}

/// An accepted fire reaches the ECS: one effect carrying the *declared* muzzle
/// and sound ids at the muzzle's live position, and one mirror entity per round
/// whose `Transform` is written from the authoritative position every tick.
#[test]
fn accept_f27_c_an_accepted_fire_emits_one_effect_and_mirrors_the_round() {
    let mut world = World::new();
    shooter_world(&mut world, &[NOSE_MOUNT]);
    let (mut session, _) = session_with_one_gun(90);
    let mut damage = damage_resolver();
    let relation = hostile_relation;

    let fired = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[WeaponOrder::Fire(intent(0, 0))],
        &step(Tick(0), &relation),
    );
    assert_eq!(fired.effects.len(), 1);
    let first = &fired.effects[0];
    assert_eq!(
        (first.shooter, first.mount.clone(), first.at),
        (actor(1), key(NOSE_MOUNT), Tick(0)),
        "the effect names the shooter, the mount and the tick it happened on"
    );
    let (declared_effect, declared_sound) = declared_ids();
    assert_eq!(
        (
            first.effect.as_str(),
            first.sound.as_str(),
            first.origin.y(),
            first.origin.z()
        ),
        (
            declared_effect.as_str(),
            declared_sound.as_str(),
            0.0,
            f64::from(MUZZLE_Z),
        ),
        "the effect carries the declared catalog ids at the muzzle's live position"
    );
    assert_eq!(session.effects().len(), 1, "the session's log holds it");

    // The mirror follows the authoritative position, not a second integration.
    let projectile = fired.accepted[0].projectile.projectile;
    let authoritative = session
        .cadence()
        .projectiles()
        .get(projectile)
        .expect("the round is live")
        .current()
        .to_array();
    assert!(
        authoritative[2] < f64::from(MUZZLE_Z) - 1.0,
        "the round moved"
    );
    let mirror = world
        .iter_entities()
        .find_map(|entity_ref| {
            let mirror = entity_ref.get::<WeaponRoundMirror>()?;
            (mirror.projectile == projectile).then(|| (entity_ref.id(), mirror.clone()))
        })
        .expect("the round has a mirror");
    let translation = world
        .get::<bevy::prelude::Transform>(mirror.0)
        .expect("the mirror carries a transform")
        .translation;
    assert_eq!(
        (mirror.1.shooter, mirror.1.generation),
        (actor(1), GENERATION),
        "the mirror names the shooter and the scene generation it was stamped under"
    );
    assert_eq!(
        translation.z, authoritative[2] as f32,
        "the mirror's position is the authoritative one"
    );

    // A second pass writes it forward and spawns nothing new.
    let next = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[],
        &step(Tick(1), &relation),
    );
    assert_eq!(next.mirrors.moved, 1, "the mirror was written forward");
    assert!(
        next.mirrors.spawned.is_empty(),
        "no mirror was spawned twice"
    );
    assert!(next.mirrors.despawned.is_empty());
}

/// A disabled wing gun emits neither projectile, sound nor ammunition through
/// the step, while its enabled sibling on the same order still fires.
#[test]
fn accept_f27_c_a_disabled_mount_emits_nothing_through_the_step() {
    let mut world = World::new();
    shooter_world(&mut world, &[NOSE_MOUNT, WING_MOUNT]);
    let (mut session, registered) = session_with_two_guns(90);
    let mut damage = damage_resolver();
    let relation = hostile_relation;
    let wing = registered[1].mount.clone();

    // The damage-driven disable: the same transition F29's consumer applies.
    session
        .state_mut(&actor(1))
        .expect("registered")
        .disable(&wing);

    let fired = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[WeaponOrder::Fire(intent(0, 0))],
        &step(Tick(0), &relation),
    );
    assert_eq!(fired.accepted.len(), 1, "the enabled nose mount fired");
    assert_eq!(
        fired.denied,
        vec![cs_app::weapons::DeniedShot {
            intent: intent(0, 0).id,
            reason: FireDenialReason::MountDisabled {
                mount: wing.clone()
            },
        }],
        "the disabled wing mount is refused by name"
    );
    assert_eq!(
        fired
            .effects
            .iter()
            .map(|effect| effect.mount.clone())
            .collect::<Vec<_>>(),
        vec![key(NOSE_MOUNT)],
        "the disabled mount emitted no sound and no muzzle effect"
    );
    assert_eq!(
        session
            .state(&actor(1))
            .expect("registered")
            .ammunition(&wing),
        SYNTHETIC_STARTING_ROUNDS,
        "the disabled mount's ammunition is untouched"
    );
    assert_eq!(
        session.round_ids().len(),
        1,
        "only the accepted round exists"
    );
}

/// The damage wiring through the step: a live round's own segment is swept
/// against the live part boxes, routed through the declared rules and applied
/// by the session's authority — and the ledger stops it applying twice.
#[test]
fn accept_f27_c_a_live_round_sweeps_its_declared_damage_into_the_authority() {
    let mut world = World::new();
    shooter_world(&mut world, &[NOSE_MOUNT, WING_MOUNT]);
    // Deep enough along the flight axis that the round is still inside the box
    // on the next tick, so the ledger is what stops a second application.
    target_world(&mut world, [3.0, 3.0, 40.0]);
    let (mut session, _) = session_with_two_guns(90);
    let mut damage = damage_resolver();
    let relation = hostile_relation;
    let internal = declared_internal_damage();

    // Tick 0: both rounds fire and their first segments cross the target.
    let first = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[WeaponOrder::Fire(intent(0, 0))],
        &step(Tick(0), &relation),
    );
    assert_eq!(first.routed.len(), 2, "both rounds were swept");
    for routed in &first.routed {
        assert_eq!(routed.outcome.sweep.hits.len(), 1, "{routed:?}");
        assert_eq!(routed.outcome.sweep.hits[0].node, key(HULL));
        assert!(
            routed
                .outcome
                .damage
                .as_ref()
                .expect("the routed batch resolves")
                .events
                .iter()
                .any(|event| matches!(event.kind, DamageEventKind::HitApplied { .. })),
            "the authority applied the routed hits"
        );
    }
    assert_eq!(
        damage.remaining_integrity(&actor(2), &key(HULL)),
        Some(40.0 - 2.0 * internal),
        "each round applied exactly the declared internal amount"
    );

    // Tick 1: the rounds are still inside the box, and the once-per-projectile
    // ledger makes that a second miss rather than a second hit.
    let second = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[],
        &step(Tick(1), &relation),
    );
    assert_eq!(second.routed.len(), 2, "both rounds were swept again");
    assert!(
        second.routed.iter().all(|routed| routed.outcome.is_empty()),
        "a round that crosses the same actor again hits nothing: {second:?}"
    );
    assert_eq!(
        damage.remaining_integrity(&actor(2), &key(HULL)),
        Some(40.0 - 2.0 * internal),
        "no round applied its damage twice"
    );
}

/// A mount whose live pose cannot be read is named, and its gun fires from
/// nowhere: no projectile, no effect, no ammunition.
#[test]
fn accept_f27_c_an_unreadable_mount_pose_is_named_and_nothing_fires_from_it() {
    let mut world = World::new();
    shooter_world(&mut world, &[NOSE_MOUNT]);
    // The wing mount node exists but carries a stale generation: its pose
    // belongs to a hierarchy that was replaced.
    let root = world
        .iter_entities()
        .find(|entity_ref| entity_ref.contains::<WeaponActorBinding>())
        .map(|entity_ref| entity_ref.id())
        .expect("the shooter root");
    world.spawn((
        cs_app::weapons::MountPoseBinding {
            mount: key(WING_MOUNT),
            generation: SceneGeneration(7),
        },
        NodeVisualTransform(GlobalTransform::from_xyz(-2.0, 0.0, MUZZLE_Z)),
        ChildOf(root),
    ));

    let (mut session, _) = session_with_two_guns(90);
    let mut damage = damage_resolver();
    let relation = hostile_relation;

    let fired = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[WeaponOrder::Fire(intent(0, 0))],
        &step(Tick(0), &relation),
    );
    assert_eq!(
        fired.unreadable_mounts,
        vec![cs_app::weapons::UnreadableMount {
            shooter: actor(1),
            refusal: MountPoseRefusal::StaleGeneration {
                mount: key(WING_MOUNT),
                found: SceneGeneration(7),
                expected: GENERATION,
            },
        }],
        "the stale mount pose is reported by name"
    );
    assert_eq!(
        fired.denied,
        vec![cs_app::weapons::DeniedShot {
            intent: intent(0, 0).id,
            reason: FireDenialReason::MissingMountTransform {
                mount: key(WING_MOUNT),
            },
        }],
        "the gun with no readable pose is refused instead of firing from the origin"
    );
    assert_eq!(fired.accepted.len(), 1, "the readable mount still fired");
    assert_eq!(fired.effects.len(), 1, "only the readable mount emitted");
}

/// The step consumes its tick: a repeat of it is refused whole, and nothing is
/// walked, fired, spawned or drained a second time.
#[test]
fn accept_f27_c_a_repeated_tick_is_refused_whole() {
    let mut world = World::new();
    shooter_world(&mut world, &[NOSE_MOUNT, WING_MOUNT]);
    let (mut session, _) = session_with_two_guns(90);
    let mut damage = damage_resolver();
    let relation = hostile_relation;

    let fired = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[WeaponOrder::Fire(intent(0, 0))],
        &step(Tick(0), &relation),
    );
    assert_eq!(fired.accepted.len(), 2);
    let ammunition = session
        .state(&actor(1))
        .expect("registered")
        .ammunition(&key(NOSE_MOUNT));

    let repeated = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[WeaponOrder::Fire(intent(0, 1))],
        &step(Tick(0), &relation),
    );
    assert_eq!(
        repeated.refused,
        Some(StepRefusal::StaleTick {
            resolved_through: Tick(0),
            at: Tick(0),
        }),
        "the repeat of a resolved tick is refused whole"
    );
    assert!(
        repeated.accepted.is_empty() && repeated.effects.is_empty(),
        "a refused step fires nothing"
    );
    assert_eq!(
        session
            .state(&actor(1))
            .expect("registered")
            .ammunition(&key(NOSE_MOUNT)),
        ammunition,
        "a refused step drains nothing"
    );
    assert_eq!(
        session.round_ids().len(),
        2,
        "a refused step spawns nothing"
    );
}

/// A spent lifetime retires the round: the mirror goes with it and the session
/// releases the accepted shot, so nothing stale is left behind.
#[test]
fn accept_f27_c_a_spent_lifetime_retires_the_round_and_its_mirror() {
    let mut world = World::new();
    shooter_world(&mut world, &[NOSE_MOUNT, WING_MOUNT]);
    // Two ticks of flight: the rounds fire on tick 0 and spend their lifetime
    // on tick 1, after their last segment was swept.
    let (mut session, _) = session_with_two_guns(2);
    let mut damage = damage_resolver();
    let relation = hostile_relation;

    let _ = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[WeaponOrder::Fire(intent(0, 0))],
        &step(Tick(0), &relation),
    );
    assert_eq!(session.round_ids().len(), 2);
    assert_eq!(
        world
            .iter_entities()
            .filter(|entity_ref| entity_ref.contains::<WeaponRoundMirror>())
            .count(),
        2
    );

    let spent = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[],
        &step(Tick(1), &relation),
    );
    assert_eq!(spent.retired.len(), 2, "both rounds spent their lifetime");
    assert_eq!(
        spent.mirrors.despawned.len(),
        2,
        "their mirrors went with them"
    );
    assert!(
        session.round_ids().is_empty(),
        "no live round is left in the cadence"
    );
    for retired in &spent.retired {
        assert_eq!(
            session.shot(retired),
            None,
            "the retired round's accepted shot was released"
        );
    }
    assert_eq!(
        world
            .iter_entities()
            .filter(|entity_ref| entity_ref.contains::<WeaponRoundMirror>())
            .count(),
        0,
        "no mirror survives its round"
    );
    // And a fresh pass over the same world reports nothing to do.
    let reconciled = sync_round_mirrors(&mut world, &session, GENERATION);
    assert!(reconciled.spawned.is_empty() && reconciled.despawned.is_empty());
}

/// Teardown releases the live rounds and their mirrors, and the closed session
/// refuses every later order, step and registration.
#[test]
fn accept_f27_c_teardown_releases_the_rounds_and_refuses_later_orders() {
    let mut world = World::new();
    shooter_world(&mut world, &[NOSE_MOUNT, WING_MOUNT]);
    let (mut session, _) = session_with_two_guns(90);
    let mut damage = damage_resolver();
    let relation = hostile_relation;

    let _ = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[WeaponOrder::Fire(intent(0, 0))],
        &step(Tick(0), &relation),
    );
    assert_eq!(session.round_ids().len(), 2);

    let teardown = session.close(&mut world);
    assert_eq!(teardown.rounds.len(), 2, "every live round was released");
    assert_eq!(teardown.mirrors, 2, "every mirror was despawned");
    assert!(session.is_closed());
    assert!(
        session.round_ids().is_empty() && session.shot(&teardown.rounds[0]).is_none(),
        "no round and no accepted shot survive the teardown"
    );
    assert_eq!(
        world
            .iter_entities()
            .filter(|entity_ref| entity_ref.contains::<WeaponRoundMirror>())
            .count(),
        0
    );

    let later = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[WeaponOrder::Fire(intent(1, 1))],
        &step(Tick(1), &relation),
    );
    assert_eq!(later.refused, Some(StepRefusal::Closed));
    assert_eq!(
        later.orders_refused,
        Vec::<OrderRefusal>::new(),
        "a closed session reads no order at all"
    );
    assert_eq!(
        session.register(
            actor(1),
            &[declared_on(NOSE_MOUNT, DeclaredGunMountKind::Nose, 90)],
            bank(&[NOSE_MOUNT]),
            SYNTHETIC_STARTING_ROUNDS,
        ),
        Err(WeaponRegistrationError::Closed),
        "a closed session registers nothing"
    );
}

/// A selection for an actor this session never registered is refused by name
/// and the standing selection is left alone.
#[test]
fn accept_f27_c_an_unknown_shooter_selection_is_refused_by_name() {
    let mut world = World::new();
    shooter_world(&mut world, &[NOSE_MOUNT, WING_MOUNT]);
    let (mut session, _) = session_with_two_guns(90);
    let mut damage = damage_resolver();
    let relation = hostile_relation;

    let refused = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[
            WeaponOrder::SelectBank {
                shooter: actor(7),
                bank: bank(&[NOSE_MOUNT]),
            },
            WeaponOrder::SelectBank {
                shooter: ActorId {
                    session: SessionId::new(SESSION + 1).expect("nonzero"),
                    serial: 1,
                },
                bank: bank(&[NOSE_MOUNT]),
            },
        ],
        &step(Tick(0), &relation),
    );
    assert_eq!(
        refused.orders_refused,
        vec![
            OrderRefusal::UnknownShooter { shooter: actor(7) },
            OrderRefusal::ForeignSession {
                expected: SESSION,
                found: SESSION + 1,
            },
        ],
        "both refused selections are named"
    );
    let state = session.state(&actor(1)).expect("registered");
    assert_eq!(
        state.selected().mounts().len(),
        2,
        "the registered shooter's standing selection is unchanged"
    );
    assert_eq!(
        state.ammunition(&key(NOSE_MOUNT)),
        SYNTHETIC_STARTING_ROUNDS,
        "a refused selection drains nothing"
    );
}

/// A session generation of zero cannot be a weapon session: it is refused at
/// the door rather than opened to refuse everything later.
#[test]
fn accept_f27_c_a_session_on_generation_zero_refuses_at_the_door() {
    assert_eq!(
        WeaponSession::new(0, Tick(0), ROUTER_PRODUCER).err(),
        Some(SessionRefusal::NoSession),
        "generation zero is not a session"
    );
}

/// The declared rules decide admission through the step: the shooter's own
/// parts are excluded by the declared self-hit rule and an ally is excluded by
/// the declared friendly-fire rule, while the hostile box takes the damage.
#[test]
fn accept_f27_c_the_declared_rules_admit_candidates_through_the_step() {
    let mut world = World::new();
    let shooter_root = shooter_world(&mut world, &[NOSE_MOUNT, WING_MOUNT]);
    // The shooter's own hull, on the flight path: the declared self-hit rule
    // decides whether the fighter's own rounds may hit it.
    world.spawn((
        PartSweptBox::new(actor(1), key(HULL), [3.0, 3.0, 40.0]),
        NodeVisualTransform(GlobalTransform::from_xyz(0.0, 0.0, 0.0)),
        ChildOf(shooter_root),
    ));
    // A hostile box and an ally box on exactly the same flight path, so only
    // the declared relation differs.
    target_world(&mut world, [3.0, 3.0, 40.0]);
    let ally_root = world.spawn(LinearVelocity(Vec3::ZERO)).id();
    world.spawn((
        PartSweptBox::new(actor(3), key(HULL), [3.0, 3.0, 40.0]),
        NodeVisualTransform(GlobalTransform::from_xyz(0.0, 0.0, 0.0)),
        ChildOf(ally_root),
    ));

    let (mut session, _) = session_with_two_guns(90);
    let mut damage = damage_resolver();
    let relation = |actor_id: ActorId| match actor_id {
        id if id == actor(2) => Some(Allegiance::Hostile),
        id if id == actor(3) => Some(Allegiance::Friendly),
        _ => None,
    };

    let fired = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[WeaponOrder::Fire(intent(0, 0))],
        &step(Tick(0), &relation),
    );
    assert_eq!(fired.routed.len(), 2, "both rounds were swept");
    assert!(
        fired
            .routed
            .iter()
            .all(|routed| routed.outcome.sweep.hits.len() == 1),
        "each round crossed exactly one admitted candidate: {fired:?}"
    );
    assert_eq!(
        fired.routed[0].outcome.sweep.admitted[0].target.actor,
        actor(2),
        "only the hostile candidate was admitted"
    );
    assert_eq!(
        fired.routing_refused,
        Vec::new(),
        "an excluded candidate is a miss, not a refusal"
    );
    assert_eq!(
        damage.remaining_integrity(&actor(2), &key(HULL)),
        Some(40.0 - 2.0 * declared_internal_damage()),
        "the hostile aircraft took exactly the two declared amounts"
    );
    damage
        .register_actor(
            actor(3),
            synthetic_airframe_graph(),
            DamagePolicy {
                attribution: AttributionRule::FirstLethalHit,
            },
        )
        .expect("the ally registers under the same synthetic graph");
    assert_eq!(
        damage.remaining_integrity(&actor(3), &key(HULL)),
        Some(40.0),
        "the declared friendly-fire rule kept the ally untouched"
    );
    assert!(
        !session
            .state(&actor(1))
            .expect("registered")
            .is_disabled(&key(NOSE_MOUNT)),
        "self-hit exclusion is admission, not damage on the shooter"
    );
}

/// An explicit round removal releases the round's routing record with it, so
/// the next reconciliation despawns its mirror instead of leaving it standing.
#[test]
fn accept_f27_c_removing_a_round_releases_its_record_and_its_mirror() {
    let mut world = World::new();
    shooter_world(&mut world, &[NOSE_MOUNT, WING_MOUNT]);
    let (mut session, _) = session_with_two_guns(90);
    let mut damage = damage_resolver();
    let relation = hostile_relation;

    let _ = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[WeaponOrder::Fire(intent(0, 0))],
        &step(Tick(0), &relation),
    );
    let removed = session.round_ids()[0];
    assert!(session.shot(&removed).is_some(), "the round has its record");
    assert!(
        session.remove_round(removed).is_some(),
        "the round was live"
    );

    let next = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[],
        &step(Tick(1), &relation),
    );
    assert_eq!(next.mirrors.despawned, vec![removed], "its mirror went");
    assert_eq!(
        next.mirrors.moved, 1,
        "the surviving round keeps its mirror"
    );
    assert_eq!(
        session.shot(&removed),
        None,
        "the removed round's accepted shot went with it"
    );
    assert_eq!(session.round_ids().len(), 1, "one round is left");
}
