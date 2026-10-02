//! Acceptance scenarios F27-B through the application boundary: the live mount
//! poses and part boxes the swept runtime resolves and sweeps against.
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`, stage
//! `### F27-B`. Task test prefix: `accept_f27_b_`. Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`. Decision record:
//! `docs/findings/2026-10-02-f27-b-gun-cadence-mounts-and-swept-ballistics.md`.
//!
//! These tests drive production code only: [`cs_app::weapons`]'s
//! [`live_mount_transforms`] and [`part_sweep_candidates`], the `cs_sim`
//! [`GunCadence`] they feed and the [`SweepTarget`] geometry they build. The
//! minimum scenario (a disabled wing gun emits neither projectile nor sound nor
//! ammo decrement) runs *through the live read*: the mounts the cadence fires
//! with are the ones read from the ECS hierarchy, the mount is disabled through
//! its own damage-node key, and the refusal is observed off the cadence.
//!
//! Removing the hierarchy read, the stale-generation gate, the box provider or
//! the shared-geometry refusal makes one of them fail.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data. No `CS_GAME_DIR` access.

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{ChildOf, GlobalTransform, Vec3, World};
use cs_app::scene::{NodeVisualTransform, SceneGeneration};
use cs_app::weapons::{
    MountPoseBinding, PartSweepRefusal, PartSweptBox, WeaponActorBinding, live_mount_transforms,
    part_sweep_candidates,
};
use cs_content::weapons::{DeclaredGunDefinition, DeclaredGunMountKind, declared_synthetic_gun};
use cs_sim::damage::ActorId;
use cs_sim::targeting::Allegiance;
use cs_sim::weapons::{
    FireDenialReason, FireIntent, FireIntentId, GunBank, GunCadence, GunMountKind,
    SYNTHETIC_STARTING_ROUNDS, WeaponState,
};
use cs_types::Tick;
use cs_types::content::DamageNodeKey;
use cs_types::content::{Origin, Provenance};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;

const SESSION: u64 = 63;
const WING_MOUNT: &str = "wing_left_mount";
const HULL: &str = "hull";
const WING: &str = "wing_left";

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
    DamageNodeKey::new(name).expect("test node keys are valid")
}

fn claim() -> ClaimId {
    ClaimId::new("f27b.live-test").expect("a valid claim id")
}

/// The declared fixture gun, moved onto `mount` and declared a wing gun, so the
/// minimum scenario really is a disabled *wing* gun.
fn declared_wing_gun(mount: &str) -> DeclaredGunDefinition {
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
        fixture.spread().clone(),
        fixture.damage().clone(),
        fixture.inheritance().clone(),
        fixture.effect().clone(),
        fixture.sound().clone(),
        fixture.rules().clone(),
        Provenance::designed(claim()),
    )
    .expect("a valid declared wing gun")
}

/// The actor root: the entity carrying the [`WeaponActorBinding`] and the
/// airframe body's live velocity.
fn actor_root(
    world: &mut World,
    generation: SceneGeneration,
    velocity: [f32; 3],
) -> bevy::prelude::Entity {
    let fixture = declared_synthetic_gun();
    world
        .spawn((
            WeaponActorBinding {
                actor: actor(1),
                guns: vec![fixture.gun().clone()],
                loadout: fixture.gun().clone(),
                generation,
            },
            LinearVelocity(Vec3::from_array(velocity)),
        ))
        .id()
}

/// A mount node under `root`, at `translation`, with the given scene
/// generation.
fn mount_node(
    world: &mut World,
    root: bevy::prelude::Entity,
    mount: &DamageNodeKey,
    generation: SceneGeneration,
    translation: [f32; 3],
) -> bevy::prelude::Entity {
    world
        .spawn((
            MountPoseBinding {
                mount: mount.clone(),
                generation,
            },
            NodeVisualTransform(GlobalTransform::from_xyz(
                translation[0],
                translation[1],
                translation[2],
            )),
            ChildOf(root),
        ))
        .id()
}

/// A part box under `root`, at `translation`.
fn part_node(
    world: &mut World,
    root: bevy::prelude::Entity,
    part: PartSweptBox,
    translation: [f32; 3],
) -> bevy::prelude::Entity {
    world
        .spawn((
            part,
            NodeVisualTransform(GlobalTransform::from_xyz(
                translation[0],
                translation[1],
                translation[2],
            )),
            ChildOf(root),
        ))
        .id()
}

/// A cadence with one actor's wing gun registered on `mount`.
fn cadence(gun: cs_sim::weapons::GunDefinition, mount: &DamageNodeKey) -> GunCadence {
    let bank = GunBank::try_new([mount.clone()]).expect("the fixture bank names a mount");
    let state = WeaponState::try_new(std::slice::from_ref(&gun), bank, SYNTHETIC_STARTING_ROUNDS)
        .expect("the fixture state is valid");
    let mut cadence = GunCadence::new(SESSION, Tick(0));
    cadence
        .register(actor(1), vec![gun], state)
        .expect("the fixture gun registers");
    cadence
}

fn intent(sequence: u32) -> FireIntent {
    FireIntent {
        id: FireIntentId {
            session: SESSION,
            tick: Tick(0),
            producer: 1,
            sequence,
        },
        shooter: actor(1),
    }
}

/// The minimum scenario through the live read: a disabled wing gun emits
/// neither projectile nor sound nor ammo decrement.
///
/// The wing gun's own mount is disabled through its damage-node key, its live
/// pose is read from the ECS hierarchy, and an otherwise-valid intent resolves
/// to an empty `accepted` list that names the mount. The cadence spawns no
/// round and consumes no ammunition. The enabled control then fires the same
/// live mount on a fresh intent: exactly one shot, one round, one round gone.
#[test]
fn accept_f27_b_a_disabled_wing_gun_emits_nothing_through_the_live_read() {
    let generation = SceneGeneration(1);
    let mut world = World::new();
    let root = actor_root(&mut world, generation, [0.0; 3]);
    let mount = key(WING_MOUNT);
    mount_node(&mut world, root, &mount, generation, [12.0, 0.0, 0.0]);

    let declared = declared_wing_gun(WING_MOUNT);
    assert_eq!(
        declared.mount_kind(),
        DeclaredGunMountKind::WingLeft,
        "the scenario's gun is a wing gun"
    );
    let gun = cs_app::weapons::lower_gun(&declared).expect("the declared wing gun lowers");
    assert_eq!(gun.kind(), GunMountKind::WingLeft);
    let live = live_mount_transforms(&world, actor(1), generation);
    assert!(
        live.refused.is_empty(),
        "the live mount reads cleanly: {:?}",
        live.refused
    );
    assert_eq!(
        live.get(&mount).expect("the mount was read").origin,
        cs_types::space::WorldPosition::try_new([12.0, 0.0, 0.0]).expect("finite"),
        "the muzzle origin is the live node's translation"
    );

    let mut cadence = cadence(gun, &mount);
    cadence
        .state_mut(&actor(1))
        .expect("the actor is registered")
        .disable(&mount);
    let rounds_before = cadence
        .state(&actor(1))
        .expect("the actor is registered")
        .ammunition(&mount);

    let resolution = cadence
        .fire(&intent(1), &live.transforms, [0.0; 3])
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

    // Control: the same live mount, enabled, really does fire one round.
    cadence
        .state_mut(&actor(1))
        .expect("the actor is registered")
        .enable(&mount);
    let firing = cadence
        .fire(&intent(2), &live.transforms, [0.0; 3])
        .expect("the enabled intent resolves");
    assert_eq!(firing.accepted.len(), 1, "the enabled mount fires once");
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

/// The mount read carries the airframe's live velocity as the inherited
/// velocity, and a mount with no reachable body velocity is refused by name
/// rather than silently inheriting zero.
#[test]
fn accept_f27_b_the_live_mount_inherits_the_airframe_velocity() {
    let generation = SceneGeneration(1);
    let mut world = World::new();
    let root = actor_root(&mut world, generation, [3.0, 0.0, -4.0]);
    let mount = key(WING_MOUNT);
    mount_node(&mut world, root, &mount, generation, [1.0, 2.0, 3.0]);

    let live = live_mount_transforms(&world, actor(1), generation);
    let transform = live.get(&mount).expect("the mount was read");
    assert_eq!(
        transform.inherited_velocity_mps,
        [3.0, 0.0, -4.0],
        "the inherited velocity is the airframe body's live LinearVelocity"
    );
    assert_eq!(
        transform.forward.to_array(),
        [0.0, 0.0, -1.0],
        "an unrotated node's forward is the canonical -Z"
    );

    // A second actor's root has no LinearVelocity: its mount is refused.
    let other = actor(2);
    let other_root = world
        .spawn(WeaponActorBinding {
            actor: other,
            guns: Vec::new(),
            loadout: declared_synthetic_gun().gun().clone(),
            generation,
        })
        .id();
    mount_node(
        &mut world,
        other_root,
        &key("other_mount"),
        generation,
        [0.0; 3],
    );
    let other_live = live_mount_transforms(&world, other, generation);
    assert!(
        other_live.transforms.is_empty(),
        "a root with no body velocity yields no readable pose"
    );
    assert_eq!(
        other_live.refused,
        vec![cs_app::weapons::MountPoseRefusal::MissingAirframeVelocity {
            mount: key("other_mount")
        }],
        "the missing airframe velocity is named"
    );
}

/// A stale binding, a mount with no pose, and a missing actor root are each
/// reported or empty rather than producing a wrong pose.
#[test]
fn accept_f27_b_unreadable_mounts_are_refused_by_name() {
    let generation = SceneGeneration(4);
    let mut world = World::new();
    let root = actor_root(&mut world, generation, [0.0; 3]);

    // A stale-generation binding under the live root.
    let stale = key("stale_mount");
    mount_node(&mut world, root, &stale, SceneGeneration(3), [0.0; 3]);
    // A current binding with no pose.
    let no_pose = key("no_pose_mount");
    world.spawn((
        MountPoseBinding {
            mount: no_pose.clone(),
            generation,
        },
        ChildOf(root),
    ));

    let live = live_mount_transforms(&world, actor(1), generation);
    assert!(live.transforms.is_empty(), "nothing readable was found");
    assert!(
        live.refused
            .contains(&cs_app::weapons::MountPoseRefusal::StaleGeneration {
                mount: stale,
                found: SceneGeneration(3),
                expected: generation,
            }),
        "the stale binding is named: {:?}",
        live.refused
    );
    assert!(
        live.refused
            .contains(&cs_app::weapons::MountPoseRefusal::MissingPose { mount: no_pose }),
        "the pose-less mount is named: {:?}",
        live.refused
    );

    // No live actor root at all: an empty read, not a panic or a guess.
    let missing = live_mount_transforms(&world, actor(9), generation);
    assert!(missing.is_empty() && missing.refused.is_empty());
}

/// The part provider builds swept boxes from live poses, carrying the target's
/// motion through the tick and the declared relation.
#[test]
fn accept_f27_b_parts_become_swept_candidates_with_relative_motion() {
    let generation = SceneGeneration(1);
    let mut world = World::new();
    let root = actor_root(&mut world, generation, [30.0, 0.0, 0.0]);

    let hull = key(HULL);
    part_node(
        &mut world,
        root,
        PartSweptBox::new(actor(2), hull.clone(), [3.0, 3.0, 0.25]),
        [0.0, 0.0, 0.0],
    );
    let wing = key(WING);
    part_node(
        &mut world,
        root,
        PartSweptBox::new(actor(2), wing.clone(), [1.0, 1.0, 1.0]),
        [-2.0, 0.0, 0.0],
    );

    let dt = 1.0 / 30.0;
    let read = part_sweep_candidates(&world, dt, |target| {
        (target == actor(2)).then_some(Allegiance::Hostile)
    });
    assert!(
        read.refused.is_empty(),
        "both parts read cleanly: {:?}",
        read.refused
    );
    assert_eq!(read.candidates.len(), 2);

    let hull_candidate = read
        .candidates
        .iter()
        .find(|candidate| candidate.node == hull)
        .expect("the hull candidate exists");
    assert_eq!(hull_candidate.target.actor, actor(2));
    assert_eq!(
        hull_candidate.target.current,
        cs_types::space::WorldPosition::try_new([0.0, 0.0, 0.0]).expect("finite")
    );
    // The airframe moves +x at 30 m/s, so a 1/30 s tick starts 1 m behind the
    // current centre: the round sweeps the box where it actually was.
    assert_eq!(
        hull_candidate.target.previous,
        cs_types::space::WorldPosition::try_new([-1.0, 0.0, 0.0]).expect("finite")
    );
    assert_eq!(hull_candidate.relation, Some(Allegiance::Hostile));
    assert_eq!(hull_candidate.target.half_extents_m, [3.0, 3.0, 0.25]);
}

/// A part with a refused box is named without dropping the others: an
/// undeclared relation stays `None`, and a negative half extent is reported.
#[test]
fn accept_f27_b_unreadable_parts_are_refused_without_losing_the_rest() {
    let generation = SceneGeneration(1);
    let mut world = World::new();
    let root = actor_root(&mut world, generation, [0.0; 3]);

    let good = key(HULL);
    part_node(
        &mut world,
        root,
        PartSweptBox::new(actor(2), good.clone(), [1.0; 3]),
        [0.0; 3],
    );
    let bad = key(WING);
    part_node(
        &mut world,
        root,
        PartSweptBox::new(actor(3), bad.clone(), [-1.0, 1.0, 1.0]),
        [0.0; 3],
    );
    // A part with no pose at all.
    let no_pose = key("no_pose_part");
    world.spawn((
        PartSweptBox::new(actor(4), no_pose.clone(), [1.0; 3]),
        ChildOf(root),
    ));

    let read = part_sweep_candidates(&world, 0.0, |_| None);
    assert_eq!(read.candidates.len(), 1, "the good part survives");
    assert_eq!(read.candidates[0].node, good);
    assert_eq!(
        read.candidates[0].relation, None,
        "an undeclared pair stays None, not Friendly"
    );
    assert!(
        read.refused.iter().any(|refusal| matches!(
            refusal,
            PartSweepRefusal::Target { node, .. } if *node == bad
        )),
        "the negative half extent is refused by name: {:?}",
        read.refused
    );
    assert!(
        read.refused.iter().any(|refusal| matches!(
            refusal,
            PartSweepRefusal::MissingPose { node } if *node == no_pose
        )),
        "the pose-less part is refused by name: {:?}",
        read.refused
    );
}

/// A part whose airframe velocity is unreadable is refused by name rather than
/// assumed to be still. The relative motion is a sweep input (F27
/// non-negotiable 3), so "no velocity is reachable" and "the target is not
/// moving" are different statements and the first must not silently become the
/// second.
#[test]
fn accept_f27_b_a_part_with_no_airframe_velocity_is_refused() {
    let generation = SceneGeneration(1);
    let mut world = World::new();
    // A live actor root with no `LinearVelocity`: the part's motion over the
    // tick cannot be reconstructed.
    let root = world
        .spawn(WeaponActorBinding {
            actor: actor(9),
            guns: Vec::new(),
            loadout: declared_synthetic_gun().gun().clone(),
            generation,
        })
        .id();
    let node = key(HULL);
    part_node(
        &mut world,
        root,
        PartSweptBox::new(actor(2), node.clone(), [1.0; 3]),
        [0.0; 3],
    );

    let read = part_sweep_candidates(&world, 1.0 / 30.0, |_| Some(Allegiance::Hostile));
    assert!(read.candidates.is_empty(), "no candidate is fabricated");
    assert_eq!(
        read.refused,
        vec![PartSweepRefusal::MissingAirframeVelocity { node }],
        "the unreadable airframe velocity is named"
    );
}
