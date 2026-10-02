//! Acceptance scenario F27-B (minimum scenario AC02 and its failure cases):
//! the per-tick gun cadence, the live projectile runtime and the shared wind
//! conversion that carries a round through the air mass.
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`, stage
//! `### F27-B`. Task test prefix: `accept_f27_b_`.
//! Decision record:
//! `docs/findings/2026-10-02-f27-b-gun-cadence-mounts-and-swept-ballistics.md`.
//!
//! These tests drive production code only: [`cs_sim::weapons::GunCadence`]
//! over the real [`FireResolver`] and the real [`ProjectileRuntime`]. Removing
//! the disabled-mount gate, the spawn-on-accepted-event step, the
//! air-relative spawn conversion, the per-tick world velocity reconstruction
//! or the lifetime retirement makes one of them fail.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data. No `CS_GAME_DIR` access: these tests prove the interface and the
//! contract, never the original game.

use std::collections::BTreeMap;

use cs_sim::damage::{ActorId, DamageNodeKey};
use cs_sim::weapons::{
    CadenceRefusal, FireDenialReason, FireIntent, FireIntentId, GunBank, GunCadence, GunDefinition,
    GunMountKind, GunRate, InheritanceRule, LiveProjectile, MountTransform, ProjectileId,
    ProjectileRuntime, ProjectileRuntimeError, SYNTHETIC_ARMOR_DAMAGE, SYNTHETIC_CALIBER,
    SYNTHETIC_INTERNAL_DAMAGE, SYNTHETIC_MUZZLE_VELOCITY_MPS, SYNTHETIC_SPREAD_HALF_ANGLE_RAD,
    SYNTHETIC_STARTING_ROUNDS, SYNTHETIC_TICKS_BETWEEN_SHOTS, SpreadCone, WeaponDamage,
    WeaponState, synthetic_ammunition, synthetic_effect, synthetic_sound,
};
use cs_types::Tick;
use cs_types::net::SessionId;
use cs_types::space::{UnitVec3, WorldPosition};

const SESSION: u64 = 27;

/// The simulation rate the fixture numbers are written against.
const TICKS_PER_SECOND: f64 = 30.0;

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session(SESSION),
        serial,
    }
}

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("test mount keys are valid")
}

fn position(value: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(value).expect("test positions are finite")
}

fn tick_seconds() -> f64 {
    1.0 / TICKS_PER_SECOND
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

/// A forward-facing mount transform at `origin`, carrying `velocity`.
fn transform(origin: [f64; 3], velocity: [f64; 3]) -> MountTransform {
    MountTransform::try_new(
        position(origin),
        UnitVec3::try_new([0.0, 0.0, -1.0]).expect("forward is a unit axis"),
        velocity,
    )
    .expect("test transforms are valid")
}

fn transforms_for(
    mount: &DamageNodeKey,
    origin: [f64; 3],
) -> BTreeMap<DamageNodeKey, MountTransform> {
    BTreeMap::from([(mount.clone(), transform(origin, [0.0; 3]))])
}

/// A gun on `mount` with the given kind and round lifetime.
fn gun(mount: &DamageNodeKey, kind: GunMountKind, lifetime_ticks: u64) -> GunDefinition {
    gun_with_muzzle(mount, kind, lifetime_ticks, SYNTHETIC_MUZZLE_VELOCITY_MPS)
}

/// The same fixture gun with a chosen muzzle velocity, so a test can drive the
/// runtime's representable-range refusal.
fn gun_with_muzzle(
    mount: &DamageNodeKey,
    kind: GunMountKind,
    lifetime_ticks: u64,
    muzzle_velocity_mps: f64,
) -> GunDefinition {
    GunDefinition::try_new(
        mount.clone(),
        kind,
        SYNTHETIC_CALIBER,
        synthetic_ammunition(),
        GunRate::try_new(SYNTHETIC_TICKS_BETWEEN_SHOTS).expect("the fixture rate is valid"),
        muzzle_velocity_mps,
        lifetime_ticks,
        SpreadCone::try_new(SYNTHETIC_SPREAD_HALF_ANGLE_RAD).expect("the fixture spread is valid"),
        WeaponDamage::try_new(SYNTHETIC_ARMOR_DAMAGE, SYNTHETIC_INTERNAL_DAMAGE)
            .expect("the fixture damage is valid"),
        InheritanceRule::Full,
        synthetic_effect(),
        synthetic_sound(),
    )
    .expect("the fixture gun definition is valid")
}

/// A cadence with one actor carrying `gun`, whose selected bank is `mount`.
fn cadence(gun: GunDefinition, mount: &DamageNodeKey) -> GunCadence {
    let state = WeaponState::try_new(
        std::slice::from_ref(&gun),
        GunBank::try_new([mount.clone()]).expect("the fixture bank names a mount"),
        SYNTHETIC_STARTING_ROUNDS,
    )
    .expect("the fixture state is valid");
    let mut cadence = GunCadence::new(SESSION, Tick(0));
    cadence
        .register(actor(1), vec![gun], state)
        .expect("the fixture gun registers");
    cadence
}

/// AC02 minimum scenario: a disabled wing gun emits neither projectile nor
/// sound nor ammo decrement.
///
/// A wing gun (`GunMountKind::WingLeft`) is disabled through its own
/// damage-node key, its mount transform is still supplied, and the intent is
/// otherwise valid. The cadence resolves to an empty `accepted` list with the
/// mount named in `refused`, spawns **no** projectile and leaves the
/// ammunition untouched — the sound and muzzle effect the sheet names live on
/// the fire event, and there is none.
///
/// The enabled control then fires the same mount on a fresh intent: exactly one
/// shot is accepted, exactly one projectile is live and exactly one round is
/// gone. Without the disabled-mount gate the first intent would fire and the
/// test would observe a projectile and a decrement; without spawn-on-accepted
/// the control would fire but leave the runtime empty.
#[test]
fn accept_f27_b_a_disabled_wing_gun_emits_nothing_through_the_cadence() {
    let mount = key("wing_left_mount");
    let mut cadence = cadence(gun(&mount, GunMountKind::WingLeft, 90), &mount);
    let transforms = transforms_for(&mount, [12.0, 0.0, 0.0]);

    cadence
        .state_mut(&actor(1))
        .expect("the actor is registered")
        .disable(&mount);
    let rounds_before = cadence
        .state(&actor(1))
        .expect("the actor is registered")
        .ammunition(&mount);

    let resolution = cadence
        .fire(&intent(0, 1), &transforms, [0.0; 3])
        .expect("a registered actor's intent resolves");
    assert!(
        resolution.accepted.is_empty(),
        "a disabled wing gun must accept nothing, got {:?}",
        resolution.accepted
    );
    assert_eq!(
        resolution.refused,
        vec![FireDenialReason::MountDisabled {
            mount: mount.clone()
        }],
        "the disabled mount must be named as the refusal"
    );
    assert!(
        cadence.projectiles().is_empty(),
        "a disabled gun must spawn no projectile"
    );
    assert_eq!(
        cadence
            .state(&actor(1))
            .expect("the actor is registered")
            .ammunition(&mount),
        rounds_before,
        "a disabled gun must not consume a round"
    );

    // Control: the same mount, enabled, on a fresh intent really does fire.
    cadence
        .state_mut(&actor(1))
        .expect("the actor is registered")
        .enable(&mount);
    let firing = cadence
        .fire(&intent(0, 2), &transforms, [0.0; 3])
        .expect("a registered actor's intent resolves");
    assert_eq!(firing.accepted.len(), 1, "the enabled mount fires once");
    assert_eq!(
        firing.accepted[0].sound,
        synthetic_sound(),
        "the accepted shot names the gun's own sound"
    );
    assert_eq!(cadence.projectiles().len(), 1, "the shot spawns one round");
    assert_eq!(
        cadence
            .state(&actor(1))
            .expect("the actor is registered")
            .ammunition(&mount),
        rounds_before - 1,
        "an accepted shot consumes exactly one round"
    );
}

/// The live round keeps the air-relative velocity and reconstructs its world
/// velocity from the current wind through the shared conversion.
///
/// The round is spawned with no wind as `(-5, 0, -600)` in the air; the gust
/// then adds `(5, 0, 0)` of world drift, so the world velocity becomes
/// `(0, 0, -600)` and the tick's displacement is `(5*dt, 0, -600*dt)`. That is
/// exactly the shared `air_relative_velocity_m_s` /
/// `world_velocity_from_air_m_s` contract: one subtraction at spawn, the world
/// frame rebuilt every tick, and no private wind arithmetic in the weapons
/// module. A round that baked its world velocity at spawn would not move with
/// the gust.
#[test]
fn accept_f27_b_a_round_carries_its_air_velocity_through_a_gust() {
    let mount = key("nose_mount");
    let mut cadence = cadence(gun(&mount, GunMountKind::Nose, 90), &mount);
    // 600 m/s of muzzle plus 0 airframe velocity, spawned in still air.
    let still_air = [0.0, 0.0, 0.0];
    cadence
        .fire(
            &intent(0, 1),
            &transforms_for(&mount, [0.0, 0.0, 0.0]),
            still_air,
        )
        .expect("the fixture shot resolves");
    let projectile = ProjectileId {
        session: SESSION,
        serial: 0,
    };
    let live = cadence
        .projectiles()
        .get(projectile)
        .expect("the shot's round is live");
    assert_eq!(
        live.air_velocity_m_s(),
        [0.0, 0.0, -SYNTHETIC_MUZZLE_VELOCITY_MPS],
        "still air leaves the muzzle velocity as the air-relative velocity"
    );

    // A gust from the side during the first tick of flight.
    let gust = [5.0, 0.0, 0.0];
    assert_eq!(
        live.world_velocity_m_s(gust),
        [5.0, 0.0, -SYNTHETIC_MUZZLE_VELOCITY_MPS],
        "the world frame adds the wind back exactly once"
    );
    let tick = cadence
        .advance_projectiles(tick_seconds(), gust)
        .expect("the tick is representable");
    assert_eq!(
        tick.segments.len(),
        1,
        "one live round produces one segment"
    );
    let segment = tick.segments[0];
    assert_eq!(
        segment.previous,
        position([0.0, 0.0, 0.0]),
        "the segment starts at the muzzle"
    );
    let expected = [
        5.0 * tick_seconds(),
        0.0,
        -SYNTHETIC_MUZZLE_VELOCITY_MPS * tick_seconds(),
    ];
    assert!(
        (segment.current.to_array()[0] - expected[0]).abs() < 1e-12
            && (segment.current.to_array()[1] - expected[1]).abs() < 1e-12
            && (segment.current.to_array()[2] - expected[2]).abs() < 1e-12,
        "the gust moves the round by its own world velocity, got {:?}",
        segment.current.to_array()
    );
}

/// A round retires after exactly its declared lifetime, producing its final
/// segment first. A lifetime of two ticks therefore yields two segments and
/// then no live round; the id is gone, not recycled.
#[test]
fn accept_f27_b_a_round_retires_after_its_declared_lifetime() {
    let mount = key("nose_mount");
    let mut cadence = cadence(gun(&mount, GunMountKind::Nose, 2), &mount);
    cadence
        .fire(
            &intent(0, 1),
            &transforms_for(&mount, [0.0, 0.0, 0.0]),
            [0.0; 3],
        )
        .expect("the fixture shot resolves");
    let projectile = ProjectileId {
        session: SESSION,
        serial: 0,
    };

    let first = cadence
        .advance_projectiles(tick_seconds(), [0.0; 3])
        .expect("the first tick is representable");
    assert_eq!(
        first.segments.len(),
        1,
        "the round is live for its first tick"
    );
    assert!(
        first.expired.is_empty(),
        "two ticks of life remain after one"
    );
    assert!(cadence.projectiles().get(projectile).is_some());

    let second = cadence
        .advance_projectiles(tick_seconds(), [0.0; 3])
        .expect("the second tick is representable");
    assert_eq!(
        second.segments.len(),
        1,
        "the round still produces its final segment"
    );
    assert_eq!(
        second.expired,
        vec![projectile],
        "the round retires on its last declared tick"
    );
    assert!(
        cadence.projectiles().get(projectile).is_none(),
        "an expired round is no longer live"
    );

    let third = cadence
        .advance_projectiles(tick_seconds(), [0.0; 3])
        .expect("the third tick is representable");
    assert!(third.is_empty(), "a retired round produces nothing further");
}

/// A spawn refuses a non-finite wind and a duplicate id by name rather than
/// silently dropping the round, so a caller can never lose a projectile.
#[test]
fn accept_f27_b_projectile_spawn_refuses_corrupt_inputs_by_name() {
    let mount = key("nose_mount");
    let cadence = cadence(gun(&mount, GunMountKind::Nose, 90), &mount);
    let mut resolver = cadence.resolver().clone();
    let mut runtime = ProjectileRuntime::new(SESSION);
    let transforms = transforms_for(&mount, [0.0, 0.0, 0.0]);
    let resolution = resolver
        .resolve(&intent(0, 1), &transforms)
        .expect("the fixture shot resolves");
    let event = resolution.accepted[0].clone();

    assert_eq!(
        runtime.spawn(&event, [f64::NAN, 0.0, 0.0]),
        Err(ProjectileRuntimeError::NonFiniteWind { component: 0 }),
        "a non-finite wind is refused by name"
    );
    runtime
        .spawn(&event, [0.0; 3])
        .expect("the first spawn succeeds");
    assert_eq!(
        runtime.spawn(&event, [0.0; 3]),
        Err(ProjectileRuntimeError::DuplicateProjectile {
            projectile: event.projectile.projectile
        }),
        "one fire event must not spawn two rounds"
    );

    // A foreign-session event is refused whole.
    let mut foreign = event.clone();
    foreign.id.session = SESSION + 1;
    assert_eq!(
        runtime.spawn(&foreign, [0.0; 3]),
        Err(ProjectileRuntimeError::ForeignSession {
            expected: SESSION,
            found: SESSION + 1
        })
    );
}

/// A fire whose caller-supplied wind is corrupt is refused **before** the
/// intent is resolved. The resolver consumes a round and starts a cooldown the
/// moment it accepts a shot, so a wind that would make the projectile
/// impossible to spawn must not let it run: the round, the cooldown and the
/// intent are all left as they were, and the same intent can be retried once
/// the wind is valid.
#[test]
fn accept_f27_b_a_refused_fire_changes_no_state() {
    let mount = key("nose_mount");
    let mut cadence = cadence(gun(&mount, GunMountKind::Nose, 90), &mount);
    let transforms = transforms_for(&mount, [0.0, 0.0, 0.0]);
    match cadence.fire(&intent(0, 1), &transforms, [f64::NAN, 0.0, 0.0]) {
        Err(CadenceRefusal::Projectile(ProjectileRuntimeError::NonFiniteWind { component: 0 })) => {
        }
        other => panic!("a corrupt wind must be refused by name, got {other:?}"),
    }
    assert!(
        cadence.projectiles().is_empty(),
        "a refused fire spawns no projectile"
    );
    assert_eq!(
        cadence
            .state(&actor(1))
            .expect("the actor is registered")
            .ammunition(&mount),
        SYNTHETIC_STARTING_ROUNDS,
        "a refused fire consumes no round"
    );

    // The same intent id is still unresolved, so a valid wind fires it.
    let firing = cadence
        .fire(&intent(0, 1), &transforms, [0.0; 3])
        .expect("the same intent fires once the wind is valid");
    assert_eq!(firing.accepted.len(), 1, "the retried intent fires once");
    assert_eq!(cadence.projectiles().len(), 1, "the shot spawns one round");
}

/// The cadence refusal type keeps a whole-intent refusal and a spawn refusal
/// distinguishable, and an acceptance test can drive the `CadenceRefusal`
/// path: a duplicate intent id is refused whole and spawns nothing.
#[test]
fn accept_f27_b_a_duplicate_intent_is_refused_whole() {
    let mount = key("nose_mount");
    let mut cadence = cadence(gun(&mount, GunMountKind::Nose, 90), &mount);
    let transforms = transforms_for(&mount, [0.0, 0.0, 0.0]);
    let fire_intent = intent(0, 1);
    cadence
        .fire(&fire_intent, &transforms, [0.0; 3])
        .expect("the first intent resolves");
    let projectiles_before = cadence.projectiles().len();
    match cadence.fire(&fire_intent, &transforms, [0.0; 3]) {
        Err(CadenceRefusal::Intent(refusal)) => {
            assert_eq!(refusal.label(), "duplicate_intent");
        }
        other => panic!("a duplicate intent must be refused whole, got {other:?}"),
    }
    assert_eq!(
        cadence.projectiles().len(),
        projectiles_before,
        "a refused intent spawns nothing"
    );
    assert_eq!(
        cadence
            .state(&actor(1))
            .expect("the actor is registered")
            .ammunition(&mount),
        SYNTHETIC_STARTING_ROUNDS - 1,
        "a refused intent consumes nothing further"
    );
}

/// The `LiveProjectile` record exposes the motion state the ECS mirror needs:
/// identity, shooter, both segment endpoints and the remaining lifetime.
#[test]
fn accept_f27_b_the_live_round_reports_its_motion_state() {
    let mount = key("tail_mount");
    let mut cadence = cadence(gun(&mount, GunMountKind::Tail, 5), &mount);
    cadence
        .fire(
            &intent(0, 1),
            &transforms_for(&mount, [1.0, 2.0, 3.0]),
            [0.0; 3],
        )
        .expect("the fixture shot resolves");
    let live: &LiveProjectile = cadence
        .projectiles()
        .iter()
        .next()
        .expect("the shot's round is live");
    assert_eq!(live.projectile().serial, 0);
    assert_eq!(live.shooter(), actor(1));
    assert_eq!(live.previous(), position([1.0, 2.0, 3.0]));
    assert_eq!(live.current(), position([1.0, 2.0, 3.0]));
    assert_eq!(live.ticks_remaining(), 5);
    assert_eq!(live.segment().previous, live.previous());
}

/// A tick that would push one round outside the representable position range
/// is refused **whole**: the error names that round and *every* round — the
/// earlier, finite one included — is left exactly where it was, so a caller can
/// retry a corrected tick without a half-advanced world.
///
/// The runtime is driven directly so the failing round can be placed after a
/// finite one: a resolver-built pair would not guarantee that order. All inputs
/// are finite; the refusal is the arithmetic overflow `WorldPosition` refuses.
#[test]
fn accept_f27_b_an_unrepresentable_tick_is_refused_whole() {
    let mount = key("nose_mount");
    let cadence = cadence(gun(&mount, GunMountKind::Nose, 90), &mount);
    let mut resolver = cadence.resolver().clone();
    let transforms = transforms_for(&mount, [0.0, 0.0, 0.0]);
    let resolution = resolver
        .resolve(&intent(0, 1), &transforms)
        .expect("the fixture shot resolves");
    let mut event = resolution.accepted[0].clone();

    let mut runtime = ProjectileRuntime::new(SESSION);
    let finite = ProjectileId {
        session: SESSION,
        serial: 0,
    };
    let overflow = ProjectileId {
        session: SESSION,
        serial: 1,
    };
    runtime
        .spawn(&event, [0.0; 3])
        .expect("the finite round spawns");
    event.projectile.projectile = overflow;
    event.projectile.velocity_mps = [0.0, 0.0, -1e300];
    runtime
        .spawn(&event, [0.0; 3])
        .expect("the huge round spawns while its velocity is finite");

    let before_finite = runtime
        .get(finite)
        .cloned()
        .expect("the finite round is live");
    let before_overflow = runtime
        .get(overflow)
        .cloned()
        .expect("the huge round is live");

    assert_eq!(
        runtime.advance(1e300, [0.0; 3]),
        Err(ProjectileRuntimeError::NonFinitePosition {
            projectile: overflow
        }),
        "the overflowing tick is refused by name"
    );
    assert_eq!(
        runtime.get(finite).cloned(),
        Some(before_finite),
        "the earlier finite round was not half-advanced by the refused tick"
    );
    assert_eq!(
        runtime.get(overflow).cloned(),
        Some(before_overflow),
        "the overflowing round is exactly where it was"
    );
}
