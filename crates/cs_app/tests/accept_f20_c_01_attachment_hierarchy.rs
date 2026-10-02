//! Acceptance scenario F20-C.01: an evaluated attachment record becomes a
//! real parent change, with the inherited velocity of a detach (AC03).
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-C`, non-negotiable behavior 4 and acceptance case **AC03**
//! "Detach cargo from a moving parent with correct inherited velocity."
//! Task test prefix: `accept_f20_c_01_`.
//!
//! These tests drive the production path end to end: the declared
//! `cs_content::animation` fixture → `play_animation` (lowering + one live
//! instance) → `advance_animation` per fixed tick, which publishes the
//! record and runs the attachment consumer
//! (`cs_app::animation::apply_attachment_transitions`). They are
//! discriminating because:
//!
//! * the `ChildOf` link, the composed world pose and the inherited velocity
//!   are all read back from the world — a consumer that only wrote a
//!   velocity without reparenting, or that re-applied the transition every
//!   tick, fails the exact assertions;
//! * `KeepWorldPose`/`KeepLocalPose` are checked on both ends (world and
//!   parent-relative), including a descendant whose world pose must be
//!   recomposed behind the parent change;
//! * a refusal must be published once, not once per tick, and must apply
//!   nothing;
//! * removing the consumer call, the applied-state bookkeeping or the
//!   velocity write fails these tests (mutation probes recorded in
//!   `docs/findings/2026-09-30-f20-c-01-attachment-hierarchy-and-detach-velocity.md`).
//!
//! Every value here is newly authored fixture data, not measured original
//! game data.

use avian3d::prelude::{AngularVelocity, LinearVelocity, Position};
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::math::Mat4;
use bevy::prelude::{ChildOf, GlobalTransform, Vec3};
use cs_app::animation::{
    AnimatedNodeBinding, AnimationInstance, AnimationLog, AnimationPlayback, AppliedAttachment,
    AttachmentRecord, AttachmentRefusalReason, NodeAnimatedAttachment, RefusedAttachment,
    VelocitySkipReason, advance_animation, play_animation, release_attachments_before_despawn,
};
use cs_app::scene::{NodeVisualTransform, SceneGeneration, SceneNodeBinding};
use cs_content::animation::{
    SYNTHETIC_CARGO_ATTACH_TICK, SYNTHETIC_CARGO_BAY_NODE, SYNTHETIC_CARGO_DETACH_TICK,
    SYNTHETIC_CARGO_NODE, declared_synthetic_cargo_clip,
};
use cs_sim::animated_object::{AttachmentState, PosePolicy};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;

/// Tolerance for one f32 affine composition over this fixture's small
/// integer offsets: the composed translations here round to well under
/// `1e-6`, so `1e-5` separates real motion from rounding without hiding a
/// wrong parent.
const POSE_TOLERANCE_M: f32 = 1.0e-5;

/// Tolerance for the inherited velocity. The fixture's numbers (10 m/s,
/// 2 rad/s, 4 m) are exactly representable in f32 and the cross product
/// yields integers, so the inherited value is exact; `1e-5` m/s is kept as
/// the documented bound in case the expression is reordered.
const VELOCITY_TOLERANCE_M_S: f32 = 1.0e-5;

// -------------------------------------------------------------- helpers ---

fn node(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::SceneNode, key).expect("the fixture key is a scene node")
}

/// The shared nonzero session generation the playback stamps into event ids.
fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

/// One live instance identity: every scenario of this stage plays a single
/// instance, and F20-C.02 is where several instances of one track are
/// exercised.
fn instance(value: u32) -> AnimationInstance {
    AnimationInstance::new(value).expect("a nonzero instance identity")
}

/// Spawns one scene node: its stable binding, its composed world pose.
fn spawn_node(world: &mut World, key: &str, generation: SceneGeneration, at: Vec3) -> Entity {
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

fn pose(world: &World, entity: Entity) -> GlobalTransform {
    world
        .get::<NodeVisualTransform>(entity)
        .expect("the node carries its composed world pose")
        .0
}

fn translation(world: &World, entity: Entity) -> Vec3 {
    Vec3::from(pose(world, entity).affine().translation)
}

/// The parent-relative translation of `child` under `parent`, derived from
/// the composed world poses the way the consumer derives it.
fn local_translation(world: &World, child: Entity, parent: Entity) -> Vec3 {
    Vec3::from((pose(world, parent).affine().inverse() * pose(world, child).affine()).translation)
}

/// Everything the playback published since the previous drain.
fn drain(world: &mut World) -> AnimationLog {
    match world.get_resource_mut::<AnimationLog>() {
        Some(mut log) => log.drain(),
        None => AnimationLog::new(),
    }
}

// --------------------------------------------------------------- AC03 ------

/// AC03, the stage's minimum scenario: the declared cargo clip is played
/// over a **moving** parent (nonzero linear *and* angular velocity) and the
/// cargo it detaches at `SYNTHETIC_CARGO_DETACH_TICK` leaves the bay,
/// keeps its composed world pose and inherits `v + ω × r` exactly once.
#[test]
fn accept_f20_c_01_detaching_cargo_from_a_moving_parent_inherits_the_parent_velocity() {
    let declared = declared_synthetic_cargo_clip();
    let clip = declared.id().clone();
    let generation = SceneGeneration::default().next();

    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(3)));

    // The bay: a scene node of this generation that is moving — 10 m/s
    // along +x and spinning at 2 rad/s about +z. It is a scene node, so its
    // composed pose is the velocity reference point.
    let bay = spawn_node(&mut world, SYNTHETIC_CARGO_BAY_NODE, generation, Vec3::ZERO);
    world.entity_mut(bay).insert((
        LinearVelocity(Vec3::new(10.0, 0.0, 0.0)),
        AngularVelocity(Vec3::new(0.0, 0.0, 2.0)),
    ));

    // The cargo, four metres along +x of the bay and parented to it: a body
    // that carries velocity components the detach can inherit into, with an
    // Avian `Position` that agrees with its composed pose.
    let cargo = spawn_node(
        &mut world,
        SYNTHETIC_CARGO_NODE,
        generation,
        Vec3::new(4.0, 0.0, 0.0),
    );
    world.entity_mut(cargo).insert((
        AnimatedNodeBinding {
            clip: clip.clone(),
            node: node(SYNTHETIC_CARGO_NODE),
            instance: instance(1),
            generation,
        },
        ChildOf(bay),
        Position(Vec3::new(4.0, 0.0, 0.0)),
        LinearVelocity(Vec3::ZERO),
        AngularVelocity(Vec3::ZERO),
    ));

    play_animation(&mut world, &declared, instance(1), generation, Tick(0))
        .expect("the playback starts");
    let carried_pose = pose(&world, cargo);

    // ω × r for a cargo four metres along +x of the spinning bay: the term
    // that makes this an inheritance and not a copy of the parent's speed.
    let offset = Vec3::new(4.0, 0.0, 0.0);
    let expected_linear = Vec3::new(10.0, 0.0, 0.0) + Vec3::new(0.0, 0.0, 2.0).cross(offset);
    assert!(
        expected_linear.distance(Vec3::new(10.0, 0.0, 0.0)) > 1.0,
        "the ω × r term is observable in the expectation: {expected_linear}"
    );

    // Before the detach tick the cargo stays in the bay and inherits
    // nothing: an attached body is not given a velocity.
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
    assert_eq!(
        world.get::<NodeVisualTransform>(cargo).map(|pose| pose.0),
        Some(carried_pose),
        "KeepWorldPose keeps the composed world pose bit for bit across the detach"
    );

    let linear = world
        .get::<LinearVelocity>(cargo)
        .expect("the body carries a linear velocity")
        .0;
    assert!(
        linear.distance(expected_linear) <= VELOCITY_TOLERANCE_M_S,
        "inherited velocity is v + ω × r: got {linear}, expected {expected_linear}"
    );
    let angular = world
        .get::<AngularVelocity>(cargo)
        .expect("the body carries an angular velocity")
        .0;
    assert!(
        angular.distance(Vec3::new(0.0, 0.0, 2.0)) <= VELOCITY_TOLERANCE_M_S,
        "the detached node spins with its parent: got {angular}"
    );

    // The double-add failure: advancing further ticks must change that
    // velocity *not at all*. The perturbation proves the write is driven by
    // the transition, not by the tick.
    world.get_mut::<LinearVelocity>(cargo).expect("a body").0 = Vec3::new(1.0, 2.0, 3.0);
    for at in (SYNTHETIC_CARGO_DETACH_TICK + 1)..=(SYNTHETIC_CARGO_DETACH_TICK + 4) {
        advance_animation(&mut world, Tick(at));
        assert_eq!(
            world.get::<LinearVelocity>(cargo).expect("a body").0,
            Vec3::new(1.0, 2.0, 3.0),
            "tick {at}: the detach inherits exactly once, not once per tick"
        );
        assert_eq!(
            world.get::<AngularVelocity>(cargo).expect("a body").0,
            Vec3::new(0.0, 0.0, 2.0),
            "tick {at}: the inherited spin is not written again"
        );
        assert!(world.get::<ChildOf>(cargo).is_none());
        assert_eq!(
            world.get::<NodeVisualTransform>(cargo).map(|pose| pose.0),
            Some(carried_pose),
            "tick {at}: the preserved pose never drifts"
        );
    }

    let published = drain(&mut world);
    assert!(
        published.is_empty(),
        "nothing was refused and nothing went unwritten in this fixture: {published:?}"
    );
}

// ------------------------------------------------------- pose policies -----

/// The other policy, and the subtree behind it: `Attach` with
/// `KeepLocalPose` keeps the parent-relative pose of the node *and* of its
/// descendant while their composed world poses follow the new parent — and
/// a later `KeepWorldPose` detach keeps both, reporting that the chain has
/// no velocity to inherit instead of inventing one.
#[test]
fn accept_f20_c_01_attaching_with_keep_local_pose_keeps_the_local_pose_and_moves_the_world_pose() {
    let declared = declared_synthetic_cargo_clip();
    let clip = declared.id().clone();
    let generation = SceneGeneration::default().next();

    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(5)));

    // The bay sits three metres up: attaching to it must move the cargo's
    // world pose while its parent-relative pose survives.
    let bay = spawn_node(
        &mut world,
        SYNTHETIC_CARGO_BAY_NODE,
        generation,
        Vec3::new(0.0, 3.0, 0.0),
    );
    let cargo = spawn_node(
        &mut world,
        SYNTHETIC_CARGO_NODE,
        generation,
        Vec3::new(5.0, 0.0, 0.0),
    );
    world.entity_mut(cargo).insert((
        AnimatedNodeBinding {
            clip: clip.clone(),
            node: node(SYNTHETIC_CARGO_NODE),
            instance: instance(1),
            generation,
        },
        // A body that already moves in its own right: an attach inherits
        // nothing, so this value must survive it untouched.
        LinearVelocity(Vec3::new(1.0, 2.0, 3.0)),
    ));
    // The cargo's child: its world pose must be recomposed behind the
    // parent change instead of staying behind (F20-B boundary 1).
    let crate_node = spawn_node(
        &mut world,
        "synthetic.cargo.crate",
        generation,
        Vec3::new(6.0, 0.0, 0.0),
    );
    world.entity_mut(crate_node).insert(ChildOf(cargo));

    play_animation(&mut world, &declared, instance(1), generation, Tick(0))
        .expect("the playback starts");
    advance_animation(&mut world, Tick(0));

    assert_eq!(
        world.get::<ChildOf>(cargo),
        Some(&ChildOf(bay)),
        "the attach resolves the bay by its scene node id in this generation"
    );
    let cargo_world = translation(&world, cargo);
    assert!(
        cargo_world.distance(Vec3::new(5.0, 3.0, 0.0)) <= POSE_TOLERANCE_M,
        "KeepLocalPose lets the world pose follow the new parent: {cargo_world}"
    );
    assert!(
        local_translation(&world, cargo, bay).distance(Vec3::new(5.0, 0.0, 0.0))
            <= POSE_TOLERANCE_M,
        "the parent-relative pose is what was kept"
    );
    assert!(
        translation(&world, crate_node).distance(Vec3::new(6.0, 3.0, 0.0)) <= POSE_TOLERANCE_M,
        "the descendant's composed world pose is recomposed behind the change"
    );
    assert!(
        local_translation(&world, crate_node, cargo).distance(Vec3::new(1.0, 0.0, 0.0))
            <= POSE_TOLERANCE_M,
        "the descendant keeps its own parent-relative pose"
    );
    assert_eq!(
        world.get::<ChildOf>(crate_node),
        Some(&ChildOf(cargo)),
        "the descendant's own link is untouched"
    );
    assert_eq!(
        world
            .get::<LinearVelocity>(cargo)
            .map(|velocity| velocity.0),
        Some(Vec3::new(1.0, 2.0, 3.0)),
        "an attach never writes a velocity"
    );

    // Through to the detach, which keeps the world pose of both.
    for at in 1..SYNTHETIC_CARGO_DETACH_TICK {
        advance_animation(&mut world, Tick(at));
    }
    advance_animation(&mut world, Tick(SYNTHETIC_CARGO_DETACH_TICK));
    assert!(world.get::<ChildOf>(cargo).is_none());
    assert!(
        translation(&world, cargo).distance(cargo_world) <= POSE_TOLERANCE_M,
        "KeepWorldPose keeps the composed world pose across the detach"
    );
    assert!(
        translation(&world, crate_node).distance(Vec3::new(6.0, 3.0, 0.0)) <= POSE_TOLERANCE_M,
        "the descendant keeps its composed world pose too"
    );
    assert_eq!(
        world
            .get::<LinearVelocity>(cargo)
            .map(|velocity| velocity.0),
        Some(Vec3::new(1.0, 2.0, 3.0)),
        "this bay's chain carries no velocity component, so nothing is written"
    );

    // ... and says so, once, instead of inventing a velocity.
    let published = drain(&mut world);
    assert_eq!(
        published.attachments().len(),
        1,
        "exactly one publication: the detach that inherited nothing"
    );
    assert_eq!(
        published.attachments()[0],
        AttachmentRecord::VelocityNotInherited {
            clip: clip.clone(),
            node: node(SYNTHETIC_CARGO_NODE),
            reason: VelocitySkipReason::NoVelocitySource,
        },
        "an ancestor chain with no velocity component is reported, never guessed"
    );
}

// ------------------------------------------------------- failure paths -----

/// Error propagation instead of guessing: a parent id that resolves to no
/// live entity **of this binding's generation**, and a binding the live
/// instance no longer serves, both apply nothing and report exactly once —
/// not once per tick, and never a half-reparent.
#[test]
fn accept_f20_c_01_unresolved_parent_and_stale_binding_reparent_nothing_and_report_once() {
    let declared = declared_synthetic_cargo_clip();
    let clip = declared.id().clone();
    let generation = SceneGeneration::default().next();

    // --- a known parent id that names no entity of this generation -------
    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(6)));
    // The bay exists — but of another generation, so it is not *this*
    // generation's parent (the `Resolved::Unknown` variant never reaches a
    // component at all: F20-B blocks and reports it).
    let _bay_of_another_generation = spawn_node(
        &mut world,
        SYNTHETIC_CARGO_BAY_NODE,
        SceneGeneration::default(),
        Vec3::ZERO,
    );
    let cargo = spawn_node(
        &mut world,
        SYNTHETIC_CARGO_NODE,
        generation,
        Vec3::new(4.0, 0.0, 0.0),
    );
    world.entity_mut(cargo).insert(AnimatedNodeBinding {
        clip: clip.clone(),
        node: node(SYNTHETIC_CARGO_NODE),
        instance: instance(1),
        generation,
    });

    play_animation(&mut world, &declared, instance(1), generation, Tick(0))
        .expect("the playback starts");
    for at in 0..3 {
        advance_animation(&mut world, Tick(at));
        assert!(
            world.get::<ChildOf>(cargo).is_none(),
            "tick {at}: nothing is reparented when the parent resolves to nothing"
        );
        assert!(
            world.get::<AppliedAttachment>(cargo).is_none(),
            "tick {at}: nothing is applied"
        );
    }
    let published = drain(&mut world);
    assert_eq!(
        published.attachments().len(),
        1,
        "the refusal is reported once, not once per tick"
    );
    assert_eq!(
        published.attachments()[0],
        AttachmentRecord::Refused {
            clip: clip.clone(),
            node: node(SYNTHETIC_CARGO_NODE),
            parent: Some(node(SYNTHETIC_CARGO_BAY_NODE)),
            pose: PosePolicy::KeepLocalPose,
            reason: AttachmentRefusalReason::UnknownParent,
        }
    );
    assert_eq!(
        world
            .get::<RefusedAttachment>(cargo)
            .map(|record| record.reason.clone()),
        Some(AttachmentRefusalReason::UnknownParent),
        "the refusal stays on the entity so it is not republished"
    );

    // --- a binding the live instance does not serve ----------------------
    let superseded = SceneGeneration::default().next();
    let live = superseded.next();
    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(7)));
    let bay = spawn_node(&mut world, SYNTHETIC_CARGO_BAY_NODE, superseded, Vec3::ZERO);
    let cargo = spawn_node(
        &mut world,
        SYNTHETIC_CARGO_NODE,
        superseded,
        Vec3::new(4.0, 0.0, 0.0),
    );
    world.entity_mut(cargo).insert((
        AnimatedNodeBinding {
            clip: clip.clone(),
            node: node(SYNTHETIC_CARGO_NODE),
            instance: instance(1),
            generation: superseded,
        },
        ChildOf(bay),
        // The record a playback under the superseded generation left on
        // this entity when the session reloaded the scene.
        NodeAnimatedAttachment(AttachmentState {
            parent: None,
            pose: PosePolicy::KeepWorldPose,
        }),
    ));

    play_animation(&mut world, &declared, instance(1), live, Tick(0)).expect("the playback starts");
    advance_animation(&mut world, Tick(SYNTHETIC_CARGO_DETACH_TICK));
    assert!(
        world.get::<ChildOf>(cargo).is_some(),
        "a stale binding reparents nothing"
    );
    let published = drain(&mut world);
    assert_eq!(published.attachments().len(), 1, "reported once");
    assert_eq!(
        published.attachments()[0],
        AttachmentRecord::Refused {
            clip: clip.clone(),
            node: node(SYNTHETIC_CARGO_NODE),
            parent: None,
            pose: PosePolicy::KeepWorldPose,
            reason: AttachmentRefusalReason::StaleBinding { serving: live },
        },
        "the refusal names the generation the live instance serves"
    );
    assert!(
        world.get::<AppliedAttachment>(cargo).is_none(),
        "nothing was applied to the stale binding"
    );

    advance_animation(&mut world, Tick(SYNTHETIC_CARGO_DETACH_TICK + 1));
    assert!(
        drain(&mut world).is_empty(),
        "the same stale state is not republished on the next tick"
    );
}

// -------------------------------------------------------- idempotence -----

/// The evaluated record is re-published every tick, so applying the same
/// state again must be a no-op: one transition, not one per tick.
#[test]
fn accept_f20_c_01_the_same_advance_never_writes_the_transition_twice() {
    let declared = declared_synthetic_cargo_clip();
    let clip = declared.id().clone();
    let generation = SceneGeneration::default().next();

    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(8)));
    let bay = spawn_node(&mut world, SYNTHETIC_CARGO_BAY_NODE, generation, Vec3::ZERO);
    world.entity_mut(bay).insert((
        LinearVelocity(Vec3::new(10.0, 0.0, 0.0)),
        AngularVelocity(Vec3::new(0.0, 0.0, 2.0)),
    ));
    let cargo = spawn_node(
        &mut world,
        SYNTHETIC_CARGO_NODE,
        generation,
        Vec3::new(4.0, 0.0, 0.0),
    );
    world.entity_mut(cargo).insert((
        AnimatedNodeBinding {
            clip: clip.clone(),
            node: node(SYNTHETIC_CARGO_NODE),
            instance: instance(1),
            generation,
        },
        ChildOf(bay),
        LinearVelocity(Vec3::ZERO),
        AngularVelocity(Vec3::ZERO),
    ));

    play_animation(&mut world, &declared, instance(1), generation, Tick(0))
        .expect("the playback starts");
    advance_animation(&mut world, Tick(SYNTHETIC_CARGO_DETACH_TICK));
    assert!(world.get::<ChildOf>(cargo).is_none());

    let applied = world
        .get::<AppliedAttachment>(cargo)
        .expect("the transition is on record");
    assert_eq!(applied.parent, None, "the record says: detached");
    assert_eq!(applied.pose, PosePolicy::KeepWorldPose);

    // The same tick, run again: the state it evaluates is the state that
    // was already applied, so nothing moves and nothing is published.
    world.get_mut::<LinearVelocity>(cargo).expect("a body").0 = Vec3::new(1.0, 2.0, 3.0);
    advance_animation(&mut world, Tick(SYNTHETIC_CARGO_DETACH_TICK));
    assert_eq!(
        world.get::<LinearVelocity>(cargo).expect("a body").0,
        Vec3::new(1.0, 2.0, 3.0),
        "the same advance writes no second transition"
    );
    assert!(world.get::<ChildOf>(cargo).is_none());
    let published = drain(&mut world);
    assert!(
        published.is_empty(),
        "the applied transition is not performed again: {published:?}"
    );

    // And neither does the next tick.
    world.get_mut::<LinearVelocity>(cargo).expect("a body").0 = Vec3::new(4.0, 5.0, 6.0);
    advance_animation(&mut world, Tick(SYNTHETIC_CARGO_DETACH_TICK + 1));
    assert_eq!(
        world.get::<LinearVelocity>(cargo).expect("a body").0,
        Vec3::new(4.0, 5.0, 6.0),
        "the transition applies once, not once per tick"
    );
    let published = drain(&mut world);
    assert!(
        published.is_empty(),
        "the next tick publishes nothing either: {published:?}"
    );
    assert_eq!(
        world
            .get::<AppliedAttachment>(cargo)
            .map(|record| record.parent.clone()),
        Some(None),
        "the applied record still describes the one transition"
    );
}

// ------------------------------------------------------------- despawn -----

/// Non-negotiable behavior 4: "release attachments before despawning
/// parents." The Bevy behavior the rule exists for is **measured** on the
/// pinned Bevy 0.19, not assumed: `despawn` is recursive over `Children`,
/// so an attachment that is still linked goes with its parent — and a
/// released one survives it.
#[test]
fn accept_f20_c_01_attachments_are_released_before_a_parent_is_despawned() {
    // --- the measurement -------------------------------------------------
    let mut world = World::new();
    let parent = spawn_node(
        &mut world,
        "synthetic.bay.unreleased",
        SceneGeneration::default(),
        Vec3::ZERO,
    );
    let child = spawn_node(
        &mut world,
        "synthetic.cargo.unreleased",
        SceneGeneration::default(),
        Vec3::new(1.0, 0.0, 0.0),
    );
    world.entity_mut(child).insert(ChildOf(parent));
    world.entity_mut(parent).despawn();
    assert!(
        world.get_entity(child).is_err(),
        "measured on the pinned Bevy 0.19: `EntityWorldMut::despawn` is \
         recursive over `Children`, so an unreleased child is despawned with \
         its parent"
    );

    // --- the rule --------------------------------------------------------
    let declared = declared_synthetic_cargo_clip();
    let clip = declared.id().clone();
    let generation = SceneGeneration::default().next();
    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(9)));

    let bay = spawn_node(&mut world, SYNTHETIC_CARGO_BAY_NODE, generation, Vec3::ZERO);
    world.entity_mut(bay).insert((
        LinearVelocity(Vec3::new(10.0, 0.0, 0.0)),
        AngularVelocity(Vec3::new(0.0, 0.0, 2.0)),
    ));
    let cargo = spawn_node(
        &mut world,
        SYNTHETIC_CARGO_NODE,
        generation,
        Vec3::new(4.0, 0.0, 0.0),
    );
    let carried_pose = pose(&world, cargo);
    world.entity_mut(cargo).insert((
        AnimatedNodeBinding {
            clip: clip.clone(),
            node: node(SYNTHETIC_CARGO_NODE),
            instance: instance(1),
            generation,
        },
        ChildOf(bay),
        Position(Vec3::new(4.0, 0.0, 0.0)),
        LinearVelocity(Vec3::ZERO),
        AngularVelocity(Vec3::ZERO),
    ));

    play_animation(&mut world, &declared, instance(1), generation, Tick(0))
        .expect("the playback starts");
    for at in 0..SYNTHETIC_CARGO_DETACH_TICK {
        advance_animation(&mut world, Tick(at));
    }
    assert!(
        world.get::<AppliedAttachment>(cargo).is_some(),
        "the attach was applied while the clip played"
    );

    let released = release_attachments_before_despawn(&mut world, bay);
    assert_eq!(
        released,
        vec![cargo],
        "the animated child is the one released"
    );
    assert!(
        world.get::<ChildOf>(cargo).is_none(),
        "the link is gone before the parent is despawned"
    );
    assert_eq!(
        world.get::<NodeVisualTransform>(cargo).map(|pose| pose.0),
        Some(carried_pose),
        "a released attachment keeps its composed world pose"
    );
    assert_eq!(
        world
            .get::<LinearVelocity>(cargo)
            .map(|velocity| velocity.0),
        Some(Vec3::new(10.0, 8.0, 0.0)),
        "a release inherits the parent's velocity by the same rule a detach does"
    );
    let applied = world
        .get::<AppliedAttachment>(cargo)
        .expect("the release is on record");
    assert_eq!(applied.parent, None);
    assert_eq!(applied.pose, PosePolicy::KeepWorldPose);

    // A release is a detach that already happened: the clip reaching its own
    // detach tick must not inherit a second time.
    world.get_mut::<LinearVelocity>(cargo).expect("a body").0 = Vec3::new(1.0, 2.0, 3.0);
    advance_animation(&mut world, Tick(SYNTHETIC_CARGO_DETACH_TICK));
    assert_eq!(
        world.get::<LinearVelocity>(cargo).expect("a body").0,
        Vec3::new(1.0, 2.0, 3.0),
        "the clip's detach finds the release already applied"
    );

    world.entity_mut(bay).despawn();
    assert!(
        world.get_entity(cargo).is_ok(),
        "the released attachment survives its parent's despawn"
    );
    let published = drain(&mut world);
    assert!(
        published.is_empty(),
        "the release was clean: nothing refused, nothing unwritten: {published:?}"
    );
}

// ----------------------------------------------------------- hierarchy ------

/// The despawn the release rule guards is **recursive**, so the release has
/// to reach an animated attachment the despawn would take through a child
/// the animation never touched: an unreleased grandchild goes with the doomed
/// subtree, a released one survives it — while the unmanaged child between
/// them still dies with its parent.
#[test]
fn accept_f20_c_01_release_reaches_an_animated_attachment_below_an_unmanaged_child() {
    let declared = declared_synthetic_cargo_clip();
    let clip = declared.id().clone();
    let generation = SceneGeneration::default().next();

    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(11)));

    // The doomed node despawns a whole subtree. Its direct child is the bay,
    // which the animation never touches (it is only the clip's attachment
    // target); the cargo hangs below the bay, at depth two.
    let doomed = spawn_node(&mut world, "synthetic.doomed.root", generation, Vec3::ZERO);
    let bay = spawn_node(
        &mut world,
        SYNTHETIC_CARGO_BAY_NODE,
        generation,
        Vec3::new(0.0, 3.0, 0.0),
    );
    world
        .entity_mut(bay)
        .insert((ChildOf(doomed), LinearVelocity(Vec3::new(3.0, 0.0, 0.0))));
    let cargo = spawn_node(
        &mut world,
        SYNTHETIC_CARGO_NODE,
        generation,
        Vec3::new(4.0, 3.0, 0.0),
    );
    world.entity_mut(cargo).insert((
        AnimatedNodeBinding {
            clip: clip.clone(),
            node: node(SYNTHETIC_CARGO_NODE),
            instance: instance(1),
            generation,
        },
        ChildOf(bay),
        LinearVelocity(Vec3::ZERO),
    ));
    let carried_pose = pose(&world, cargo);

    play_animation(&mut world, &declared, instance(1), generation, Tick(0))
        .expect("the playback starts");
    advance_animation(&mut world, Tick(SYNTHETIC_CARGO_ATTACH_TICK));
    assert!(
        world.get::<AppliedAttachment>(cargo).is_some(),
        "the attach was applied while the clip played"
    );

    // --- the measurement: without the release, the grandchild dies too ----
    let mut unreleased = World::new();
    let root = spawn_node(
        &mut unreleased,
        "synthetic.doomed.root.unreleased",
        SceneGeneration::default(),
        Vec3::ZERO,
    );
    let unmanaged = spawn_node(
        &mut unreleased,
        "synthetic.doomed.mid.unreleased",
        SceneGeneration::default(),
        Vec3::ZERO,
    );
    let deep = spawn_node(
        &mut unreleased,
        "synthetic.doomed.deep.unreleased",
        SceneGeneration::default(),
        Vec3::ONE,
    );
    unreleased.entity_mut(unmanaged).insert(ChildOf(root));
    unreleased.entity_mut(deep).insert(ChildOf(unmanaged));
    unreleased.entity_mut(root).despawn();
    assert!(
        unreleased.get_entity(deep).is_err(),
        "measured: a linked child below an unmanaged child is despawned \
         recursively too, so an unreleased grandchild never survives"
    );

    // --- the rule --------------------------------------------------------
    let released = release_attachments_before_despawn(&mut world, doomed);
    assert_eq!(
        released,
        vec![cargo],
        "the animated attachment below the unmanaged child is released"
    );
    assert!(
        world.get::<ChildOf>(cargo).is_none(),
        "its link is gone before the subtree is despawned"
    );
    assert_eq!(
        world.get::<NodeVisualTransform>(cargo).map(|pose| pose.0),
        Some(carried_pose),
        "a released attachment keeps its composed world pose"
    );
    assert_eq!(
        world
            .get::<LinearVelocity>(cargo)
            .map(|velocity| velocity.0),
        Some(Vec3::new(3.0, 0.0, 0.0)),
        "a deep release inherits the velocity of the parent it was linked to"
    );

    world.entity_mut(doomed).despawn();
    assert!(
        world.get_entity(bay).is_err(),
        "the unmanaged child still dies with the doomed subtree"
    );
    assert!(
        world.get_entity(cargo).is_ok(),
        "the released attachment survives the recursive despawn"
    );
    let published = drain(&mut world);
    assert!(
        published.is_empty(),
        "nothing was refused and nothing went unwritten: {published:?}"
    );
}

/// The `ω × r` term is measured only where there is a spin. A chain that does
/// not spin contributes exactly zero however unmeasurable the offset between
/// the two reference points is, so the linear source alone is inherited — and
/// the node's own spin is left alone rather than zeroed. A chain that *does*
/// spin while the detaching node has no location of its own inherits nothing
/// and says exactly why, once, instead of being released silently.
#[test]
fn accept_f20_c_01_an_unmeasurable_spin_term_still_inherits_the_linear_source() {
    let declared = declared_synthetic_cargo_clip();
    let clip = declared.id().clone();
    let generation = SceneGeneration::default().next();

    // --- an authored detach under a source with no reference point --------
    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(13)));
    // The hull the bay hangs from: it carries a world linear velocity but no
    // composed scene pose and no Avian `Position`, so the offset `r` from it
    // to the cargo cannot be measured — and nothing in the chain spins.
    let hull = world.spawn(LinearVelocity(Vec3::new(5.0, 0.0, 0.0))).id();
    let bay = spawn_node(&mut world, SYNTHETIC_CARGO_BAY_NODE, generation, Vec3::ZERO);
    world.entity_mut(bay).insert(ChildOf(hull));
    let cargo = spawn_node(
        &mut world,
        SYNTHETIC_CARGO_NODE,
        generation,
        Vec3::new(4.0, 0.0, 0.0),
    );
    world.entity_mut(cargo).insert((
        AnimatedNodeBinding {
            clip: clip.clone(),
            node: node(SYNTHETIC_CARGO_NODE),
            instance: instance(1),
            generation,
        },
        LinearVelocity(Vec3::ZERO),
        // A spin the node already has. No ancestor of it spins, so the detach
        // must leave it exactly as it is: writing a zero over it would itself
        // be an invention.
        AngularVelocity(Vec3::new(0.0, 1.0, 0.0)),
    ));

    play_animation(&mut world, &declared, instance(1), generation, Tick(0))
        .expect("the playback starts");
    advance_animation(&mut world, Tick(SYNTHETIC_CARGO_ATTACH_TICK));
    assert_eq!(
        world.get::<ChildOf>(cargo),
        Some(&ChildOf(bay)),
        "the attach at tick {} applied, so the detach below has a chain to walk \
         and nothing below is proven vacuously",
        SYNTHETIC_CARGO_ATTACH_TICK
    );
    for at in (SYNTHETIC_CARGO_ATTACH_TICK + 1)..SYNTHETIC_CARGO_DETACH_TICK {
        advance_animation(&mut world, Tick(at));
    }
    advance_animation(&mut world, Tick(SYNTHETIC_CARGO_DETACH_TICK));
    assert!(world.get::<ChildOf>(cargo).is_none(), "the cargo detached");
    assert_eq!(
        world
            .get::<LinearVelocity>(cargo)
            .map(|velocity| velocity.0),
        Some(Vec3::new(5.0, 0.0, 0.0)),
        "ω is zero, so ω × r is exactly zero: the linear source is inherited \
         whole and the unmeasurable offset costs nothing"
    );
    assert_eq!(
        world
            .get::<AngularVelocity>(cargo)
            .map(|velocity| velocity.0),
        Some(Vec3::new(0.0, 1.0, 0.0)),
        "the node's own spin is never overwritten with a zero"
    );
    assert!(
        drain(&mut world).is_empty(),
        "nothing was refused and nothing went unwritten: the linear source was \
         measurable after all"
    );

    // --- a release of a node that has no composed pose at all -------------
    let declared = declared_synthetic_cargo_clip();
    let clip = declared.id().clone();
    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(14)));
    // A moving, spinning parent of a managed attachment that has neither a
    // composed pose nor an Avian `Position`: the parent's reference point is
    // known, the child's is not, so `r` has a missing end.
    let parent = world
        .spawn((
            Position(Vec3::ZERO),
            LinearVelocity(Vec3::new(3.0, 0.0, 0.0)),
            AngularVelocity(Vec3::new(0.0, 0.0, 2.0)),
        ))
        .id();
    let child = world
        .spawn((
            ChildOf(parent),
            AnimatedNodeBinding {
                clip: clip.clone(),
                node: node(SYNTHETIC_CARGO_NODE),
                instance: instance(1),
                generation,
            },
            NodeAnimatedAttachment(AttachmentState {
                parent: Some(Resolved::Known(Known::new(
                    node(SYNTHETIC_CARGO_BAY_NODE),
                    Provenance::designed(ClaimId::new("f20c01.review").expect("claim id")),
                ))),
                pose: PosePolicy::KeepLocalPose,
            }),
            LinearVelocity(Vec3::ZERO),
        ))
        .id();

    let released = release_attachments_before_despawn(&mut world, parent);
    assert_eq!(released, vec![child], "the managed attachment is released");
    assert!(world.get::<ChildOf>(child).is_none());
    assert_eq!(
        world
            .get::<LinearVelocity>(child)
            .map(|velocity| velocity.0),
        Some(Vec3::ZERO),
        "the chain spins and the node has no location, so nothing is invented"
    );
    let published = drain(&mut world);
    assert_eq!(
        published.attachments().len(),
        1,
        "the release reports the missing measurement once, it does not pass \
         in silence: {published:?}"
    );
    assert_eq!(
        published.attachments()[0],
        AttachmentRecord::VelocityNotInherited {
            clip: clip.clone(),
            node: node(SYNTHETIC_CARGO_NODE),
            reason: VelocitySkipReason::NoNodeReferencePoint,
        },
        "the record names which end of the offset could not be measured"
    );
    world.entity_mut(parent).despawn();
    assert!(
        world.get_entity(child).is_ok(),
        "a released attachment survives the despawn even with no pose of its own"
    );
}

/// Cycles in an ownership/parent hierarchy are invalid
/// (`docs/contracts/IDENTITY-CONTENT.md`), so a parent id that resolves to a
/// node inside the animated node's own subtree is refused instead of
/// applied: no `ChildOf` is written, nothing half-reparents, and the refusal
/// is published exactly once.
#[test]
fn accept_f20_c_01_a_parent_inside_the_nodes_own_subtree_is_refused() {
    let declared = declared_synthetic_cargo_clip();
    let clip = declared.id().clone();
    let generation = SceneGeneration::default().next();

    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(12)));

    let cargo = spawn_node(&mut world, SYNTHETIC_CARGO_NODE, generation, Vec3::ZERO);
    // The bay the clip attaches the cargo to is itself a child of the cargo:
    // attaching would make the node its own ancestor and every subtree walk
    // behind it loop forever.
    let bay = spawn_node(
        &mut world,
        SYNTHETIC_CARGO_BAY_NODE,
        generation,
        Vec3::new(4.0, 0.0, 0.0),
    );
    world.entity_mut(bay).insert(ChildOf(cargo));
    world.entity_mut(cargo).insert(AnimatedNodeBinding {
        clip: clip.clone(),
        node: node(SYNTHETIC_CARGO_NODE),
        instance: instance(1),
        generation,
    });

    play_animation(&mut world, &declared, instance(1), generation, Tick(0))
        .expect("the playback starts");
    advance_animation(&mut world, Tick(SYNTHETIC_CARGO_ATTACH_TICK));

    assert!(
        world.get::<ChildOf>(cargo).is_none(),
        "the cyclic link is never written"
    );
    assert_eq!(
        world.get::<ChildOf>(bay),
        Some(&ChildOf(cargo)),
        "the existing hierarchy is untouched — nothing half-applies"
    );
    assert!(
        world.get::<AppliedAttachment>(cargo).is_none(),
        "nothing was applied"
    );

    let published = drain(&mut world);
    assert_eq!(
        published.attachments().len(),
        1,
        "the refusal is reported once: {published:?}"
    );
    assert_eq!(
        published.attachments()[0],
        AttachmentRecord::Refused {
            clip: clip.clone(),
            node: node(SYNTHETIC_CARGO_NODE),
            parent: Some(node(SYNTHETIC_CARGO_BAY_NODE)),
            pose: PosePolicy::KeepLocalPose,
            reason: AttachmentRefusalReason::CyclicParent,
        },
        "the refusal names the parent it refused"
    );

    // The refusal is a state, not a frame: the next tick adds nothing.
    advance_animation(&mut world, Tick(SYNTHETIC_CARGO_ATTACH_TICK + 1));
    assert!(
        drain(&mut world).is_empty(),
        "the same refused state is not republished"
    );
    assert!(world.get::<ChildOf>(cargo).is_none());
}
