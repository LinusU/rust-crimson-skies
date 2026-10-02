//! Acceptance scenarios F20-C: the integration of the animation producers and
//! consumers into a real running session.
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-C`. Task test prefix: `accept_f20_c_`. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! The F20-C subtasks built the pieces (the attachment consumer, the fixed-tick
//! schedule, the visibility composition) but left two producers missing:
//! nothing wrote a session tick into [`CommittedSessionTick`], and nothing
//! produced an [`AnimatedNodeBinding`], so a real session could play an
//! instance and drive no entity. These tests exercise the integration step
//! that adds them:
//!
//! * [`cs_app::animation::AnimationPlugin`] installed in the **production**
//!   [`PhysicsSession`], whose driver copies the F23-A physics ledger's
//!   committed tick and whose schedule advances the playback once per fixed
//!   tick — so a per-frame advance, a dead schedule or a missing driver fails;
//! * [`cs_app::animation::bind_animated_node`], the spawn-side producer, is the
//!   only way the worlds here bind an entity to a playing instance;
//! * the stage's acceptance cases: AC01 (the door's mesh and collider pose are
//!   one value, applied through the wired path), AC02/behavior 1 (a looping
//!   propeller fires its one-shot gameplay marker once), AC03 (detaching cargo
//!   from a moving parent inherits its velocity end to end) and AC04 (a skip
//!   reaches the final semantic state exactly once);
//! * teardown/retry and error propagation at the producer and the teardown.
//!
//! Every value is newly authored fixture data, not measured original game data:
//! the original animation containers are still undecoded (F13) and F20-D keeps
//! the original-family validation gate.

use avian3d::prelude::{AngularVelocity, LinearVelocity, Position};
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::math::Mat4;
use bevy::prelude::{ChildOf, GlobalTransform, Vec3};
use cs_app::animation::{
    AnimatedNodeBindError, AnimationInstance, AnimationLog, AnimationPlayback, AnimationPlugin,
    CommittedSessionTick, NodeAnimatedPose, advance_animation, bind_animated_node, play_animation,
    stop_animation,
};
use cs_app::physics::PhysicsSession;
use cs_app::scene::{NodeVisualTransform, SceneGeneration, SceneNodeBinding};
use cs_content::animation::{
    SYNTHETIC_CARGO_BAY_NODE, SYNTHETIC_CARGO_DETACH_TICK, SYNTHETIC_CARGO_NODE,
    SYNTHETIC_DOOR_OPEN_TICK, SYNTHETIC_PROPELLER_DURATION, SYNTHETIC_PROPELLER_NODE,
    declared_synthetic_cargo_clip, declared_synthetic_door_clip, declared_synthetic_propeller_clip,
};
use cs_sim::animated_object::PoseSample;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;
use cs_types::space::{Quaternion, Radians, UnitVec3};

/// Tolerance for the inherited velocity of the AC03 scenario: the fixture's
/// numbers are exactly representable and the cross product yields integers, so
/// `1e-5` m/s only guards against an expression reorder.
const VELOCITY_TOLERANCE_M_S: f32 = 1.0e-5;

// -------------------------------------------------------------- helpers ---

fn content_id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("valid content id")
}

fn node(key: &str) -> ContentId {
    content_id(ContentKind::SceneNode, key)
}

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

fn instance(value: u32) -> AnimationInstance {
    AnimationInstance::new(value).expect("a nonzero instance identity")
}

/// The quarter turn the door reaches at its open tick and the propeller reaches
/// every quarter of a loop.
fn quarter_turn(axis: UnitVec3, quarter: u8) -> PoseSample {
    PoseSample::try_new(
        Quaternion::from_axis_angle(
            axis,
            Radians(f64::from(quarter) * std::f64::consts::FRAC_PI_2),
        )
        .expect("a quarter turn is unit length"),
        [0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0],
    )
    .expect("the pose is finite")
}

/// The pose the door presents once it has opened.
fn door_open_pose() -> PoseSample {
    quarter_turn(UnitVec3::UP, 1)
}

/// The pose the rotor presents at session tick `at` (the clip started at tick 0
/// and loops every [`SYNTHETIC_PROPELLER_DURATION`] ticks).
fn rotor_pose_at(at: u64) -> PoseSample {
    quarter_turn(UnitVec3::FORWARD, (at % SYNTHETIC_PROPELLER_DURATION) as u8)
}

/// Spawns one scene node: its stable binding and its composed world pose.
fn spawn_scene_node(world: &mut World, key: &str, generation: SceneGeneration, at: Vec3) -> Entity {
    world
        .spawn((
            SceneNodeBinding {
                node: node(key),
                generation,
            },
            NodeVisualTransform(GlobalTransform::from(Mat4::from_translation(at))),
        ))
        .id()
}

fn applied_pose(world: &World, entity: Entity) -> Option<PoseSample> {
    world
        .get::<NodeAnimatedPose>(entity)
        .map(NodeAnimatedPose::pose)
}

/// The whole animated-pose component for `entity`, so both of its named
/// projections ([`NodeAnimatedPose::mesh`], [`NodeAnimatedPose::collider`])
/// can be read at once.
fn applied_component(world: &World, entity: Entity) -> Option<NodeAnimatedPose> {
    world.get::<NodeAnimatedPose>(entity).copied()
}

/// Everything the playback published since the previous drain.
fn drain(world: &mut World) -> AnimationLog {
    match world.get_resource_mut::<AnimationLog>() {
        Some(mut log) => log.drain(),
        None => AnimationLog::new(),
    }
}

/// The gameplay events of a drained batch, in publication order.
fn gameplay(log: &AnimationLog) -> Vec<&str> {
    log.events()
        .iter()
        .filter(|event| event.effect.is_gameplay())
        .map(|event| event.marker.as_str())
        .collect()
}

/// The production session the wiring tests drive: the real Avian fixed loop
/// with the one-stop [`AnimationPlugin`] installed through the session's own
/// `configure` seam. The animation playback resource is inserted after the
/// build, the way a session driver would own it.
fn wired_session(session_value: u64) -> PhysicsSession {
    let mut physics = PhysicsSession::builder()
        .fixed_hz(64)
        .configure(|app| {
            app.add_plugins(AnimationPlugin);
        })
        .build();
    physics
        .world_mut()
        .expect("a fresh session owns a world")
        .insert_resource(AnimationPlayback::new(session(session_value)));
    physics
}

// ------------------------------------------- the wired path, in a session ---

/// The minimum of this integration step: [`AnimationPlugin`] inside the real
/// [`PhysicsSession`] commits the physics ledger's tick and advances the
/// playback once per fixed tick, and [`bind_animated_node`] is what put the
/// entity on that path.
///
/// AC01 is asserted where it is structural: the door's [`NodeAnimatedPose`]
/// answers both the mesh and the collider question with the same stored value,
/// and both move together at the open tick.
#[test]
fn accept_f20_c_the_wired_plugin_and_binder_drive_a_bound_node() {
    let generation = SceneGeneration::default().next();
    let declared = declared_synthetic_door_clip();
    let track = declared.id().clone();
    let door_node = node("synthetic.hangar.door");
    let mut scene = wired_session(31);

    let door = {
        let world = scene.world_mut().expect("the session is active");
        let door = spawn_scene_node(world, "synthetic.hangar.door", generation, Vec3::ZERO);
        bind_animated_node(
            world,
            &declared,
            door,
            &door_node,
            instance(1),
            generation,
            Tick(0),
        )
        .expect("the production spawn path binds the door");
        assert!(
            world
                .resource::<AnimationPlayback>()
                .is_playing(&track, instance(1)),
            "the binder started the instance the binding names"
        );
        door
    };

    // One fixed step is one committed tick, and the driver must have copied it:
    // the advance runs after the physics step, in the same tick.
    scene.step(1).expect("the session is active");
    {
        let world = scene.world().expect("the session is active");
        assert_eq!(
            world.resource::<CommittedSessionTick>().tick(),
            Tick(1),
            "the driver committed the physics ledger's first fixed tick"
        );
        assert_eq!(
            world.resource::<AnimationPlayback>().advanced_through(),
            Some(Tick(1)),
            "the schedule advanced through the tick the driver committed"
        );
        let pose = applied_pose(world, door).expect("the bound door was driven");
        let component =
            applied_component(world, door).expect("the door carries its pose component");
        assert_eq!(
            component.mesh(),
            component.collider(),
            "AC01: the mesh pose and the collider pose are the same stored value"
        );
        assert_eq!(
            pose,
            quarter_turn(UnitVec3::UP, 0),
            "the door's closed pose is the clip's tick-0 pose"
        );
    }

    // Advance to the open tick: the pose becomes the authored quarter turn and
    // its one-shot gameplay marker fires exactly once.
    scene
        .step(SYNTHETIC_DOOR_OPEN_TICK - 1)
        .expect("the session is active");
    {
        let world = scene.world().expect("the session is active");
        let pose = applied_pose(world, door).expect("the door stays driven");
        assert_eq!(
            pose,
            door_open_pose(),
            "AC01: the door reaches its authored open pose through the wired path"
        );
        let component =
            applied_component(world, door).expect("the door carries its pose component");
        assert_eq!(
            component.mesh(),
            component.collider(),
            "AC01: the open pose is coherent for the mesh and the collider"
        );
        assert_eq!(
            world
                .resource::<AnimationPlayback>()
                .time(&track, instance(1)),
            Some(SYNTHETIC_DOOR_OPEN_TICK),
            "clip time is the committed session tick since the bind tick"
        );
    }
    let published = drain(scene.world_mut().expect("the session is active"));
    assert_eq!(
        gameplay(&published),
        vec!["door_opened"],
        "the door's gameplay marker fired exactly once, at its authored tick"
    );
    assert!(
        published.blocked_markers().is_empty() && published.blocked_tracks().is_empty(),
        "no track of this fixture is blocked: {published:?}"
    );

    // Later fixed ticks change nothing: the one-shot clip is finished, not
    // re-offered, and the pose never drifts.
    scene.step(3).expect("the session is active");
    let world = scene.world().expect("the session is active");
    assert_eq!(applied_pose(world, door), Some(door_open_pose()));
    assert!(
        drain(scene.world_mut().expect("the session is active")).is_empty(),
        "a finished one-shot clip publishes nothing on later ticks"
    );
}

/// AC02 and non-negotiable behavior 1: a looping propeller fires its one-shot
/// gameplay marker once across many loop passes, while its pose keeps moving.
#[test]
fn accept_f20_c_a_looping_propeller_fires_its_one_shot_marker_once() {
    let generation = SceneGeneration::default().next();
    let declared = declared_synthetic_propeller_clip();
    let track = declared.id().clone();
    let rotor_node = node(SYNTHETIC_PROPELLER_NODE);
    let mut scene = wired_session(32);

    let rotor = {
        let world = scene.world_mut().expect("the session is active");
        let rotor = spawn_scene_node(world, SYNTHETIC_PROPELLER_NODE, generation, Vec3::ZERO);
        bind_animated_node(
            world,
            &declared,
            rotor,
            &rotor_node,
            instance(1),
            generation,
            Tick(0),
        )
        .expect("the production spawn path binds the rotor");
        rotor
    };

    // Let the clip loop several times. Every committed tick advances it once.
    let passes = SYNTHETIC_PROPELLER_DURATION * 3 + 1;
    scene.step(passes).expect("the session is active");
    let world = scene.world().expect("the session is active");
    assert_eq!(
        world
            .resource::<AnimationPlayback>()
            .time(&track, instance(1)),
        Some(passes),
        "clip time is monotone across every loop pass"
    );
    assert_eq!(
        applied_pose(world, rotor),
        Some(rotor_pose_at(passes)),
        "the rotor keeps presenting the clip's current pose through the loops"
    );
    let published = drain(scene.world_mut().expect("the session is active"));
    assert_eq!(
        gameplay(&published),
        vec!["engine_started"],
        "the looping propeller's one-shot gameplay marker fires exactly once"
    );
    assert_eq!(
        published
            .events()
            .iter()
            .filter(|event| event.marker == "blade_pass")
            .count(),
        passes.div_ceil(SYNTHETIC_PROPELLER_DURATION) as usize,
        "the presentation cue keeps firing once per completed loop pass"
    );
}

// --------------------------------------------------- AC04: the skip shape ---

/// AC04 at the production evaluator's scope: a skip is one advance that jumps
/// the head past the marker, and it reaches the final semantic state exactly
/// once. The binding is written by the production spawn entry, not by hand.
///
/// A fixed-tick session only ever advances one tick at a time, so the skip is
/// driven through the same `advance_animation` entry the schedule calls; what
/// the fixed loop adds is *when* it is called, not what a skip means.
#[test]
fn accept_f20_c_a_skipped_animation_reaches_its_final_state_exactly_once() {
    let generation = SceneGeneration::default().next();
    let declared = declared_synthetic_door_clip();
    let door_node = node("synthetic.hangar.door");

    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(33)));
    let door = spawn_scene_node(&mut world, "synthetic.hangar.door", generation, Vec3::ZERO);
    bind_animated_node(
        &mut world,
        &declared,
        door,
        &door_node,
        instance(1),
        generation,
        Tick(0),
    )
    .expect("the production spawn path binds the door");

    // The skip: one committed tick 30 ticks after the bind, straight past the
    // marker at the open tick.
    advance_animation(&mut world, Tick(SYNTHETIC_DOOR_OPEN_TICK + 20));
    assert_eq!(
        applied_pose(&world, door),
        Some(door_open_pose()),
        "the skip reaches the final authored pose"
    );
    let skipped = drain(&mut world);
    assert_eq!(
        gameplay(&skipped),
        vec!["door_opened"],
        "the marker the skip crossed fired exactly once"
    );

    // The same state reached again by a later tick must not re-offer anything:
    // the final semantic state is reached exactly once.
    advance_animation(&mut world, Tick(SYNTHETIC_DOOR_OPEN_TICK + 25));
    assert_eq!(applied_pose(&world, door), Some(door_open_pose()));
    let after = drain(&mut world);
    assert!(
        after.is_empty(),
        "arriving at the same final state again publishes nothing: {after:?}"
    );
    assert_eq!(
        world
            .resource::<AnimationPlayback>()
            .is_finished(&declared.id().clone(), instance(1)),
        Some(true),
        "the one-shot clip reports itself finished"
    );
}

// --------------------------------------------- AC03: detach inherited motion ---

/// AC03 end to end through the production spawn entry: the cargo is bound by
/// [`bind_animated_node`], the declared clip attaches it to the moving bay and
/// detaches it at `SYNTHETIC_CARGO_DETACH_TICK`, and the consumer gives it
/// `v + ω × r` exactly once.
#[test]
fn accept_f20_c_detaching_cargo_from_a_moving_parent_inherits_its_velocity() {
    let generation = SceneGeneration::default().next();
    let declared = declared_synthetic_cargo_clip();
    let cargo_node = node(SYNTHETIC_CARGO_NODE);

    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(34)));
    let bay = spawn_scene_node(&mut world, SYNTHETIC_CARGO_BAY_NODE, generation, Vec3::ZERO);
    world.entity_mut(bay).insert((
        LinearVelocity(Vec3::new(10.0, 0.0, 0.0)),
        AngularVelocity(Vec3::new(0.0, 0.0, 2.0)),
    ));
    let cargo = spawn_scene_node(
        &mut world,
        SYNTHETIC_CARGO_NODE,
        generation,
        Vec3::new(4.0, 0.0, 0.0),
    );
    world.entity_mut(cargo).insert((
        ChildOf(bay),
        Position(Vec3::new(4.0, 0.0, 0.0)),
        LinearVelocity(Vec3::ZERO),
        AngularVelocity(Vec3::ZERO),
    ));
    bind_animated_node(
        &mut world,
        &declared,
        cargo,
        &cargo_node,
        instance(1),
        generation,
        Tick(0),
    )
    .expect("the production spawn path binds the cargo");

    for at in 0..SYNTHETIC_CARGO_DETACH_TICK {
        advance_animation(&mut world, Tick(at));
        assert_eq!(
            world.get::<ChildOf>(cargo),
            Some(&ChildOf(bay)),
            "tick {at}: the cargo is still parented to the bay"
        );
        assert_eq!(
            world
                .get::<LinearVelocity>(cargo)
                .map(|velocity| velocity.0),
            Some(Vec3::ZERO),
            "tick {at}: nothing is inherited while the cargo is attached"
        );
    }

    advance_animation(&mut world, Tick(SYNTHETIC_CARGO_DETACH_TICK));
    assert!(
        world.get::<ChildOf>(cargo).is_none(),
        "the ChildOf link is gone at the detach tick"
    );
    let offset = Vec3::new(4.0, 0.0, 0.0);
    let expected = Vec3::new(10.0, 0.0, 0.0) + Vec3::new(0.0, 0.0, 2.0).cross(offset);
    let linear = world
        .get::<LinearVelocity>(cargo)
        .expect("the cargo carries a velocity")
        .0;
    assert!(
        linear.distance(expected) <= VELOCITY_TOLERANCE_M_S,
        "the detach inherits v + ω × r: got {linear}, expected {expected}"
    );
    assert_eq!(
        world
            .get::<AngularVelocity>(cargo)
            .map(|velocity| velocity.0),
        Some(Vec3::new(0.0, 0.0, 2.0)),
        "the detached cargo inherits the parent's spin"
    );

    // The double-add failure: later ticks do not write the velocity again.
    world.get_mut::<LinearVelocity>(cargo).expect("a body").0 = Vec3::new(-1.0, -2.0, -3.0);
    advance_animation(&mut world, Tick(SYNTHETIC_CARGO_DETACH_TICK + 1));
    assert_eq!(
        world.get::<LinearVelocity>(cargo).expect("a body").0,
        Vec3::new(-1.0, -2.0, -3.0),
        "the detach inherits exactly once, not once per tick"
    );
    assert!(
        drain(&mut world).is_empty(),
        "this fixture has no refusal and no missing velocity source"
    );
}

// ------------------------------------------------- error propagation --------

/// A binding to a node the clip does not drive would drive nothing forever, so
/// the production spawn entry refuses it and starts no instance.
#[test]
fn accept_f20_c_the_binder_refuses_a_node_the_clip_does_not_drive() {
    let generation = SceneGeneration::default().next();
    let declared = declared_synthetic_door_clip();
    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(35)));
    let entity = spawn_scene_node(&mut world, "synthetic.hangar.door", generation, Vec3::ZERO);

    let error = bind_animated_node(
        &mut world,
        &declared,
        entity,
        &node(SYNTHETIC_PROPELLER_NODE),
        instance(1),
        generation,
        Tick(0),
    )
    .expect_err("a node the clip does not drive is refused");
    match error {
        AnimatedNodeBindError::NodeNotDriven {
            clip,
            node: refused,
        } => {
            assert_eq!(clip, *declared.id());
            assert_eq!(refused, node(SYNTHETIC_PROPELLER_NODE));
        }
        other => panic!("expected NodeNotDriven, got {other:?}"),
    }
    assert!(
        world
            .get::<cs_app::animation::AnimatedNodeBinding>(entity)
            .is_none(),
        "the refusal wrote no binding"
    );
    assert_eq!(
        world.resource::<AnimationPlayback>().len(),
        0,
        "the refusal started no instance"
    );
}

/// The producer propagates every failure instead of binding:
/// [`AnimatedNodeBindError::UnknownEntity`] for a despawned entity,
/// [`AnimationPlayError::NoSession`](cs_app::animation::AnimationPlayError)
/// with no playback, and [`AnimatedNodeBindError::StaleInstance`] for a live
/// identity serving another generation.
#[test]
fn accept_f20_c_the_binder_propagates_missing_entity_session_and_stale_instance() {
    let generation = SceneGeneration::default().next();
    let declared = declared_synthetic_door_clip();
    let door_node = node("synthetic.hangar.door");

    // No session: the playback boundary refuses before anything is written.
    let mut bare = World::new();
    let entity = spawn_scene_node(&mut bare, "synthetic.hangar.door", generation, Vec3::ZERO);
    let error = bind_animated_node(
        &mut bare,
        &declared,
        entity,
        &door_node,
        instance(1),
        generation,
        Tick(0),
    )
    .expect_err("a world with no playback has no session");
    assert!(
        matches!(error, AnimatedNodeBindError::Play(_)),
        "expected the playback error to propagate, got {error:?}"
    );

    // Unknown entity.
    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(36)));
    let doomed = world.spawn(()).id();
    world.entity_mut(doomed).despawn();
    let error = bind_animated_node(
        &mut world,
        &declared,
        doomed,
        &door_node,
        instance(1),
        generation,
        Tick(0),
    )
    .expect_err("a despawned entity is not part of the world");
    assert_eq!(error, AnimatedNodeBindError::UnknownEntity(doomed));

    // A live instance serving another generation is stale for this binding.
    let other_generation = generation.next();
    let entity = spawn_scene_node(&mut world, "synthetic.hangar.door", generation, Vec3::ZERO);
    play_animation(&mut world, &declared, instance(1), generation, Tick(0))
        .expect("the instance starts");
    let error = bind_animated_node(
        &mut world,
        &declared,
        entity,
        &door_node,
        instance(1),
        other_generation,
        Tick(0),
    )
    .expect_err("a stale live instance is refused");
    match error {
        AnimatedNodeBindError::StaleInstance {
            live, requested, ..
        } => {
            assert_eq!(live, generation);
            assert_eq!(requested, other_generation);
        }
        other => panic!("expected StaleInstance, got {other:?}"),
    }
    assert!(
        world
            .get::<cs_app::animation::AnimatedNodeBinding>(entity)
            .is_none(),
        "the stale refusal wrote no binding"
    );
}

// ------------------------------------------------- teardown and retry -------

/// Stopping an instance releases what it applied and the binding, and binding
/// the same identity again starts a fresh instance whose one-shot marker fires
/// once more under a new producer serial.
#[test]
fn accept_f20_c_stopping_an_instance_releases_it_and_a_rebind_plays_again() {
    let generation = SceneGeneration::default().next();
    let declared = declared_synthetic_door_clip();
    let track = declared.id().clone();
    let door_node = node("synthetic.hangar.door");

    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(37)));
    let door = spawn_scene_node(&mut world, "synthetic.hangar.door", generation, Vec3::ZERO);
    bind_animated_node(
        &mut world,
        &declared,
        door,
        &door_node,
        instance(1),
        generation,
        Tick(0),
    )
    .expect("the door binds");
    let first_producer = world
        .resource::<AnimationPlayback>()
        .producer(&track, instance(1))
        .expect("the first instance has a producer serial");

    for at in 1..=SYNTHETIC_DOOR_OPEN_TICK {
        advance_animation(&mut world, Tick(at));
    }
    assert_eq!(
        gameplay(&drain(&mut world)),
        vec!["door_opened"],
        "the first activation fired its gameplay marker once"
    );

    assert!(
        stop_animation(&mut world, &track, instance(1)),
        "the instance was playing and is stopped"
    );
    assert!(
        world.get::<NodeAnimatedPose>(door).is_none(),
        "the teardown released the applied pose"
    );
    assert!(
        world
            .get::<cs_app::animation::AnimatedNodeBinding>(door)
            .is_none(),
        "the teardown released the binding"
    );
    assert!(
        !world
            .resource::<AnimationPlayback>()
            .is_playing(&track, instance(1)),
        "the stopped instance is out of the live map"
    );

    // Retry: binding the same identity again starts a fresh evaluator with a
    // fresh producer serial, and its one-shot marker fires once more.
    bind_animated_node(
        &mut world,
        &declared,
        door,
        &door_node,
        instance(1),
        generation,
        Tick(SYNTHETIC_DOOR_OPEN_TICK),
    )
    .expect("the retry binds");
    let second_producer = world
        .resource::<AnimationPlayback>()
        .producer(&track, instance(1))
        .expect("the retried instance has a producer serial");
    assert_ne!(
        first_producer, second_producer,
        "the retry is a fresh activation with a fresh producer serial"
    );
    for at in (SYNTHETIC_DOOR_OPEN_TICK + 1)..=(SYNTHETIC_DOOR_OPEN_TICK * 2) {
        advance_animation(&mut world, Tick(at));
    }
    assert_eq!(
        gameplay(&drain(&mut world)),
        vec!["door_opened"],
        "the retried activation fires its marker exactly once"
    );
    assert_eq!(applied_pose(&world, door), Some(door_open_pose()));
}
