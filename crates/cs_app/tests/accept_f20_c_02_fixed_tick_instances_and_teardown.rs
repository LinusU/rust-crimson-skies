//! Acceptance scenarios F20-C.02: the animation advance on the fixed-tick
//! schedule, one `animation_track` played as several instances, and the
//! teardown of a stopped instance.
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-C` (non-negotiable behaviors 1, 3 and 4). Task test prefix:
//! `accept_f20_c_02_`.
//!
//! These tests are discriminating in three separate ways, and each has its own
//! test:
//!
//! * **Schedule placement.** The first three drive the real
//!   `cs_app::animation::AnimationSchedulePlugin` inside the **real Avian
//!   `FixedPostUpdate` loop** of `cs_app::synthetic::SyntheticScene`, with one
//!   render frame deliberately carrying **two** fixed ticks. The advance
//!   happens once per *committed session tick*, so a system that ran per
//!   frame, that ran without the driver's stamp, or that ran again for a
//!   repeated stamp fails. None of them calls `advance_animation` directly, so
//!   a dead schedule path cannot pass.
//! * **Instance identity.** One `animation_track` is played as two
//!   instances: two entities each receive their own evaluated state, and each
//!   instance fires its one-shot gameplay marker exactly once, with event ids
//!   that cannot collide. Counting map entries would not be enough — both
//!   entities are driven.
//! * **Teardown and retry.** Stopping one instance removes its applied
//!   components, its animation-managed `ChildOf` and its binding, while the
//!   other instance of the same track keeps playing untouched, a state
//!   belonging to another clip survives, and playing the same identity again
//!   afterwards works with a fresh producer serial.
//!
//! Every value here is newly authored fixture data, not measured original game
//! data: the original animation containers are still undecoded (F13), and
//! F20-D keeps the original-family validation gate.

use core::time::Duration;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use avian3d::prelude::{LinearVelocity, PhysicsSystems, Position};
use bevy::ecs::entity::Entity;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::world::World;
use bevy::math::Mat4;
use bevy::prelude::{
    ChildOf, Commands, FixedPostUpdate, GlobalTransform, Res, ResMut, Resource, Vec3,
};
use bevy::time::{Fixed, Time, TimeUpdateStrategy};
use cs_app::animation::{
    AnimatedNodeBinding, AnimationInstance, AnimationLog, AnimationPlayError, AnimationPlayback,
    AnimationRefusal, AnimationSchedulePlugin, AppliedAttachment, AttachmentRecord,
    CommittedSessionTick, InstanceKey, NodeAnimatedAttachment, NodeAnimatedMaterial,
    NodeAnimatedPose, VelocitySkipReason, advance_animation, play_animation,
    release_superseded_instances, stop_animation,
};
use cs_app::scene::{NodeVisualTransform, SceneGeneration, SceneGenerations, SceneNodeBinding};
use cs_app::synthetic::SyntheticScene;
use cs_content::animation::{
    AnimationClip, SYNTHETIC_CARGO_BAY_NODE, SYNTHETIC_CARGO_DETACH_TICK, SYNTHETIC_CARGO_MATERIAL,
    SYNTHETIC_CARGO_NODE, SYNTHETIC_CARGO_SCORCH_TICK, SYNTHETIC_CARGO_SCORCHED_MATERIAL,
    SYNTHETIC_DOOR_OPEN_TICK, SYNTHETIC_PROPELLER_DURATION, SYNTHETIC_PROPELLER_NODE,
    declared_synthetic_cargo_clip, declared_synthetic_door_clip, declared_synthetic_propeller_clip,
};
use cs_sim::animated_object::{PoseSample, synthetic_propeller_clip};
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;
use cs_types::space::{Quaternion, Radians, UnitVec3};
use cs_types::{BodyKind, SyntheticBodySpec, Tick};

/// How many fixed ticks one render frame of the schedule tests carries, so
/// "once per committed tick" and "once per frame" cannot be confused.
const FRAME_FIXED_TICKS: u32 = 2;

/// The session the schedule tests stamp their events with.
const SCHEDULE_SESSION: u64 = 21;

// -------------------------------------------------------------- helpers ---

fn content_id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("valid content id")
}

fn node(key: &str) -> ContentId {
    content_id(ContentKind::SceneNode, key)
}

fn track(key: &str) -> ContentId {
    content_id(ContentKind::AnimationTrack, key)
}

fn material(key: &str) -> ContentId {
    content_id(ContentKind::Material, key)
}

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

fn instance(value: u32) -> AnimationInstance {
    AnimationInstance::new(value).expect("a nonzero instance identity")
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

/// The quarter turn the synthetic rotor reaches at `quarter`: the same pose
/// `cs_content::animation`'s fixture authors, and the runtime twin
/// [`synthetic_propeller_clip`] that the lowering must reproduce
/// (`accept_f20_c_02_the_propeller_fixture_drives_the_production_lowering`).
fn rotor_turn(quarter: u8) -> PoseSample {
    PoseSample::try_new(
        Quaternion::from_axis_angle(
            UnitVec3::FORWARD,
            Radians(f64::from(quarter) * std::f64::consts::FRAC_PI_2),
        )
        .expect("a quarter turn is unit length"),
        [0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0],
    )
    .expect("the pose is finite")
}

/// The pose the rotor presents at session tick `at` (the clip started at tick 0
/// and loops every [`SYNTHETIC_PROPELLER_DURATION`] ticks).
fn rotor_pose_at(at: u64) -> PoseSample {
    rotor_turn((at % SYNTHETIC_PROPELLER_DURATION) as u8)
}

/// Spawns one scene node: its stable binding and its composed world pose.
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

/// Binds one entity to one node of one instance of `clip`.
fn bind(
    world: &mut World,
    clip: &ContentId,
    bound_node: &ContentId,
    which: AnimationInstance,
    generation: SceneGeneration,
) -> Entity {
    world
        .spawn(AnimatedNodeBinding {
            clip: clip.clone(),
            node: bound_node.clone(),
            instance: which,
            generation,
        })
        .id()
}

fn applied_pose(world: &World, entity: Entity) -> Option<PoseSample> {
    world
        .get::<NodeAnimatedPose>(entity)
        .map(|pose| pose.pose())
}

// ------------------------------------------------------- the session driver --

/// The test's stand-in for the session driver: the ticks it commits, one per
/// fixed step, in order. An empty queue commits nothing, which is what a
/// render frame carrying no new fixed tick looks like from here.
#[derive(Resource, Clone)]
struct SessionDriver(Arc<Mutex<VecDeque<u64>>>);

impl SessionDriver {
    fn new(ticks: impl IntoIterator<Item = u64>) -> Self {
        Self(Arc::new(Mutex::new(ticks.into_iter().collect())))
    }

    /// Queues the ticks the next fixed steps commit.
    fn push(&self, ticks: impl IntoIterator<Item = u64>) {
        self.0
            .lock()
            .expect("the test owns this lock")
            .extend(ticks);
    }

    /// How many committed ticks are still queued.
    fn pending(&self) -> usize {
        self.0.lock().expect("the test owns this lock").len()
    }
}

/// The system a session driver runs: it opens the fixed tick by committing the
/// session tick it is simulating. It is ordered before the physics step — the
/// same place the physics adapter's own `record_tick_boundary` sits — so the
/// animation advance, which is ordered *after* the step, sees the tick of the
/// fixed step it runs in.
///
/// The stamp is *published* on the first commit rather than existing from the
/// start: a world whose driver has never committed has no
/// [`CommittedSessionTick`] at all, which is exactly the state the schedule
/// must refuse to advance.
fn commit_session_tick(
    mut committed: Option<ResMut<CommittedSessionTick>>,
    mut commands: Commands,
    driver: Res<SessionDriver>,
) {
    let mut queue = driver.0.lock().expect("the test owns this lock");
    let Some(tick) = queue.pop_front() else {
        return;
    };
    match committed.as_mut() {
        Some(committed) => committed.0 = Tick(tick),
        None => {
            commands.insert_resource(CommittedSessionTick::new(Tick(tick)));
        }
    }
}

/// Which clip the schedule tests play.
#[derive(Clone)]
enum Fixture {
    /// The looping propeller, on its own rotor node.
    Propeller,
    /// The one-shot hangar door.
    Door,
}

impl Fixture {
    fn clip(&self) -> AnimationClip {
        match self {
            Self::Propeller => declared_synthetic_propeller_clip(),
            Self::Door => declared_synthetic_door_clip(),
        }
    }

    fn node(&self) -> ContentId {
        match self {
            Self::Propeller => node(SYNTHETIC_PROPELLER_NODE),
            Self::Door => node("synthetic.hangar.door"),
        }
    }
}

/// Builds the real fixed loop: Avian's own plugin group, a manual clock whose
/// frame delta carries [`FRAME_FIXED_TICKS`] fixed ticks, the animation
/// schedule plugin, the driver (when asked for) and the playback with one
/// node bound and playing.
fn fixed_loop(
    fixture: Fixture,
    driver: Option<SessionDriver>,
    generation: SceneGeneration,
) -> SyntheticScene {
    let frame = Duration::from_secs_f64(1.0 / 64.0);
    let clip = fixture.clip();
    let clip_id = clip.id().clone();
    let bound_node = fixture.node();
    let which = instance(1);

    SyntheticScene::builder(SyntheticBodySpec::falling_box(BodyKind::Dynamic))
        .configure(move |app| {
            // One render frame, two fixed ticks: the animation advance must
            // follow the committed ticks, not the frame.
            app.insert_resource(TimeUpdateStrategy::ManualDuration(
                frame * FRAME_FIXED_TICKS,
            ));
            app.insert_resource(Time::<Fixed>::from_seconds(1.0 / 64.0));
            if let Some(driver) = driver.clone() {
                app.insert_resource(driver);
                app.add_systems(
                    FixedPostUpdate,
                    commit_session_tick.before(PhysicsSystems::Prepare),
                );
            }
            app.add_plugins(AnimationSchedulePlugin);

            let world = app.world_mut();
            world.insert_resource(AnimationPlayback::new(session(SCHEDULE_SESSION)));
            let bound = bind(world, &clip_id, &bound_node, which, generation);
            world.insert_resource(BoundEntity(bound));
            play_animation(world, &clip, which, generation, Tick(0)).expect("the playback starts");
        })
        .build()
        .expect("the fixture spec must build a scene")
}

/// The one entity the schedule world bound, handed back to the test: a
/// `&World` cannot run a `Query` in this Bevy version, so the id is recorded
/// when the world is built.
#[derive(Resource, Clone, Copy)]
struct BoundEntity(Entity);

/// The one bound entity of a schedule world.
fn bound_entity(scene: &SyntheticScene) -> Entity {
    scene.world().resource::<BoundEntity>().0
}

// ------------------------------------------------- 1. schedule placement ---

/// The minimum scenario of this slice: the schedule system advances the
/// playback **once per committed session tick** in the real `FixedPostUpdate`
/// loop, and a tick repeated inside one frame is not a second pass.
///
/// Each render frame of this scene carries two fixed ticks, so a per-frame
/// advance is distinguishable from a per-tick one, and the driver commits the
/// same tick twice in one frame to show that a repeat advances nothing.
#[test]
fn accept_f20_c_02_the_fixed_tick_advance_runs_once_per_committed_session_tick() {
    let generation = SceneGeneration::default().next();
    let driver = SessionDriver::new([0, 1]);
    let mut scene = fixed_loop(Fixture::Propeller, Some(driver.clone()), generation);
    let rotor = bound_entity(&scene);

    // One render frame, two fixed steps, two *different* committed ticks.
    scene.step(1);
    {
        let world = scene.world();
        let playback = world.resource::<AnimationPlayback>();
        assert_eq!(
            playback.advances(),
            2,
            "one frame carried two fixed ticks and two committed ones, so there were two passes"
        );
        assert_eq!(playback.advanced_through(), Some(Tick(1)));
        assert_eq!(
            playback.time(&track("synthetic.propeller"), instance(1)),
            Some(1),
            "clip time is the committed session tick"
        );
        let log = world.resource::<AnimationLog>();
        assert_eq!(
            log.events().len(),
            2,
            "the clip-tick-0 presentation cue and the clip-tick-1 gameplay cue crossed \
             once each, in the fixed ticks that committed them"
        );
        assert_eq!(
            applied_pose(world, rotor),
            Some(rotor_pose_at(1)),
            "the rotor carries the pose of the committed tick"
        );
    }
    assert_eq!(driver.pending(), 0, "the driver committed what it queued");

    // The next frame commits tick 1 twice: a repeated tick is not a new tick.
    driver.push([1, 1]);
    let before = scene.world().resource::<AnimationLog>().clone();
    scene.step(1);
    {
        let world = scene.world();
        assert_eq!(
            world.resource::<AnimationPlayback>().advances(),
            2,
            "a repeated committed tick must not run a second pass"
        );
        assert_eq!(
            *world.resource::<AnimationLog>(),
            before,
            "a repeat publishes nothing at all"
        );
        assert_eq!(applied_pose(world, rotor), Some(rotor_pose_at(1)));
    }

    // A changed tick advances exactly once; the second fixed step of the frame
    // commits nothing and adds nothing.
    driver.push([2]);
    scene.step(1);
    {
        let world = scene.world();
        assert_eq!(
            world.resource::<AnimationPlayback>().advances(),
            3,
            "one changed tick is one pass, whatever else the frame did"
        );
        assert_eq!(applied_pose(world, rotor), Some(rotor_pose_at(2)));
    }

    // Whole frames with no committed tick change nothing at all.
    scene.step(2);
    {
        let world = scene.world();
        assert_eq!(
            world.resource::<AnimationPlayback>().advances(),
            3,
            "frames that commit no tick are not passes"
        );
        assert_eq!(applied_pose(world, rotor), Some(rotor_pose_at(2)));
    }
}

/// The same loop without a driver: the schedule plugin alone advances nothing,
/// because nothing committed a session tick. "No session, no animation", the
/// rule `play_animation` already enforces, applied to the schedule.
#[test]
fn accept_f20_c_02_the_schedule_advances_nothing_without_a_committed_tick() {
    let generation = SceneGeneration::default().next();
    let mut scene = fixed_loop(Fixture::Propeller, None, generation);
    let rotor = bound_entity(&scene);

    scene.step(4);

    let world = scene.world();
    assert!(
        world.get_resource::<CommittedSessionTick>().is_none(),
        "this world has no driver, so no tick was ever committed into it"
    );
    assert_eq!(
        world.resource::<AnimationPlayback>().advances(),
        0,
        "no committed tick means no pass, however many fixed ticks ran"
    );
    assert_eq!(
        world.resource::<AnimationPlayback>().advanced_through(),
        None
    );
    assert!(
        applied_pose(world, rotor).is_none(),
        "an un-advanced playback applies no state"
    );
    assert!(
        world
            .get_resource::<AnimationLog>()
            .is_none_or(AnimationLog::is_empty),
        "an un-advanced playback publishes nothing"
    );
}

/// A repeated stamp publishes nothing, and a reversed one is forwarded so the
/// existing hold rule still reports it once while the applied pose stays: the
/// schedule is only the caller, the rule is F20-B's
/// ([`AnimationRefusal::Held`]).
#[test]
fn accept_f20_c_02_a_repeated_or_reversed_stamp_publishes_nothing_new() {
    let generation = SceneGeneration::default().next();
    let driver = SessionDriver::new([0]);
    let mut scene = fixed_loop(Fixture::Door, Some(driver.clone()), generation);
    let door = bound_entity(&scene);

    // The open tick, which authors the clip's only gameplay marker.
    driver.push([SYNTHETIC_DOOR_OPEN_TICK]);
    scene.step(1);
    let opened = scene.world().resource::<AnimationLog>().clone();
    assert_eq!(
        gameplay(&opened),
        vec!["door_opened"],
        "the door's gameplay marker fired on the tick that opened it"
    );
    let open_pose = applied_pose(scene.world(), door).expect("the door has a pose");
    assert_ne!(
        open_pose,
        PoseSample::IDENTITY,
        "the door is open after its open tick"
    );

    // The same tick again: no pass, nothing published, nothing moved.
    driver.push([SYNTHETIC_DOOR_OPEN_TICK]);
    scene.step(1);
    {
        let world = scene.world();
        assert_eq!(
            *world.resource::<AnimationLog>(),
            opened,
            "a repeated committed tick publishes nothing"
        );
        assert_eq!(applied_pose(world, door), Some(open_pose));
    }

    // A tick behind the head: forwarded, held once, and the pose stays.
    driver.push([3]);
    scene.step(1);
    {
        let world = scene.world();
        let refusals = &world.resource::<AnimationLog>().refusals();
        assert_eq!(
            refusals.len(),
            1,
            "a reversed committed tick is reported once, not swallowed: {refusals:?}"
        );
        assert!(matches!(
            refusals[0],
            AnimationRefusal::Held {
                ref clip,
                instance: held,
                from,
                to: 3
            } if *clip == track("synthetic.door_open")
                && held == instance(1)
                && from == SYNTHETIC_DOOR_OPEN_TICK
        ));
        assert_eq!(
            applied_pose(world, door),
            Some(open_pose),
            "a held instance keeps the pose it reached"
        );
    }

    // The same reversed tick once more: a repeat, so no second report.
    driver.push([3]);
    scene.step(1);
    assert_eq!(
        scene.world().resource::<AnimationLog>().refusals().len(),
        1,
        "one occurrence of a hold is reported once, however often the tick repeats"
    );

    // Forward again: the instance resumes, and the marker stays fired.
    driver.push([SYNTHETIC_DOOR_OPEN_TICK + 1]);
    let held = scene.world().resource::<AnimationLog>().clone();
    scene.step(1);
    {
        let world = scene.world();
        let resumed = world.resource::<AnimationLog>();
        assert_eq!(
            resumed.events(),
            held.events(),
            "catching up must not re-offer the one-shot marker"
        );
        assert_eq!(
            resumed.refusals(),
            held.refusals(),
            "the forward pass does not repeat the old hold"
        );
        assert_eq!(applied_pose(world, door), Some(open_pose));
    }
}

// -------------------------------------------------- 2. instance identity ---

/// One `animation_track` played as two instances: two entities each receive
/// their own evaluated state on every tick, and each instance fires its
/// one-shot gameplay marker exactly once with event ids that cannot collide —
/// same session, same tick, different producer serial.
#[test]
fn accept_f20_c_02_two_instances_of_one_track_each_drive_their_own_entity() {
    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(3)));
    let generation = SceneGeneration::default().next();

    let clip = declared_synthetic_propeller_clip();
    let clip_id = clip.id().clone();
    let rotor = node(SYNTHETIC_PROPELLER_NODE);
    let first = bind(&mut world, &clip_id, &rotor, instance(1), generation);
    let second = bind(&mut world, &clip_id, &rotor, instance(2), generation);

    play_animation(&mut world, &clip, instance(1), generation, Tick(0))
        .expect("the first instance starts");
    play_animation(&mut world, &clip, instance(2), generation, Tick(0))
        .expect("a second instance of the same track starts beside it");

    let playback = world.resource::<AnimationPlayback>();
    assert_eq!(playback.len(), 2, "two live instances of one track");
    assert_eq!(
        playback.playing().cloned().collect::<Vec<_>>(),
        vec![
            InstanceKey::new(&clip_id, instance(1)),
            InstanceKey::new(&clip_id, instance(2)),
        ],
        "the live map is keyed by (track, instance) in stable order"
    );
    let first_producer = playback.producer(&clip_id, instance(1)).expect("live");
    let second_producer = playback.producer(&clip_id, instance(2)).expect("live");
    assert_ne!(
        first_producer, second_producer,
        "two instances must not share a producer serial, or their event ids could collide"
    );

    // Every tick: both entities carry the pose of that tick.
    for at in 1..=9 {
        advance_animation(&mut world, Tick(at));
        assert_eq!(
            applied_pose(&world, first),
            Some(rotor_pose_at(at)),
            "the first instance's rotor is driven on tick {at}"
        );
        assert_eq!(
            applied_pose(&world, second),
            Some(rotor_pose_at(at)),
            "the second instance's rotor is driven on tick {at}"
        );
    }

    let published = drain(&mut world);
    // Clip-tick 0 fires the presentation cue once per pass and clip-tick 1 the
    // gameplay cue once per activation, for **each** instance.
    let per_instance_presentation = 1 + 9 / SYNTHETIC_PROPELLER_DURATION;
    assert_eq!(
        published
            .events()
            .iter()
            .filter(|event| !event.effect.is_gameplay())
            .count(),
        2 * per_instance_presentation as usize,
        "both instances present every loop pass"
    );
    assert_eq!(
        gameplay(&published).len(),
        2,
        "each instance fired its one-shot gameplay marker exactly once"
    );
    let gameplay_ids: Vec<_> = published
        .events()
        .iter()
        .filter(|event| event.effect.is_gameplay())
        .map(|event| event.id)
        .collect();
    assert_eq!(gameplay_ids.len(), 2);
    assert_ne!(
        gameplay_ids[0], gameplay_ids[1],
        "the two instances' events never share an id"
    );
    assert_eq!(gameplay_ids[0].session, gameplay_ids[1].session);
    assert_eq!(gameplay_ids[0].tick, gameplay_ids[1].tick);
    assert_eq!(gameplay_ids[0].producer, first_producer);
    assert_eq!(gameplay_ids[1].producer, second_producer);

    // One instance held back does not stop the other: a still head is per
    // instance, and both are advanced by the same committed tick.
    assert_eq!(
        world
            .resource::<AnimationPlayback>()
            .time(&clip_id, instance(1)),
        Some(9)
    );
    assert_eq!(
        world
            .resource::<AnimationPlayback>()
            .time(&clip_id, instance(2)),
        Some(9)
    );
}

// -------------------------------------------------------- 3. teardown ------

/// Stopping one instance releases exactly what that instance applied: the
/// applied values, the animation-managed `ChildOf` (with the velocity the
/// departing parent had) and the binding — for its own entity only. The other
/// instance of the same track keeps playing and keeps its hierarchy, and a
/// state that belongs to another clip survives untouched.
#[test]
fn accept_f20_c_02_stopping_one_instance_releases_only_its_own_entities() {
    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(5)));
    let generation = SceneGeneration::default().next();

    let clip = declared_synthetic_cargo_clip();
    let clip_id = clip.id().clone();
    let cargo_node = node(SYNTHETIC_CARGO_NODE);

    // The bay: a scene node of this generation, moving at 10 m/s along +x.
    let bay = spawn_node(&mut world, SYNTHETIC_CARGO_BAY_NODE, generation, Vec3::ZERO);
    world
        .entity_mut(bay)
        .insert(LinearVelocity(Vec3::new(10.0, 0.0, 0.0)));

    // Two cargo nodes bound to the same track as two instances, both parented
    // to the bay and both carrying the velocity components a detach inherits
    // into.
    let spawn_cargo = |world: &mut World, at: Vec3| {
        let entity = spawn_node(world, SYNTHETIC_CARGO_NODE, generation, at);
        world
            .entity_mut(entity)
            .insert((ChildOf(bay), Position(at), LinearVelocity(Vec3::ZERO)));
        entity
    };
    let stopped_cargo = spawn_cargo(&mut world, Vec3::new(4.0, 0.0, 0.0));
    let kept_cargo = spawn_cargo(&mut world, Vec3::new(-3.0, 0.0, 0.0));
    for (entity, which) in [(stopped_cargo, instance(1)), (kept_cargo, instance(2))] {
        world.entity_mut(entity).insert(AnimatedNodeBinding {
            clip: clip_id.clone(),
            node: cargo_node.clone(),
            instance: which,
            generation,
        });
    }

    // A third entity holds state that belongs to **another** track: a teardown
    // of this instance must not clear it, whatever it looks like.
    let foreign = world
        .spawn(AnimatedNodeBinding {
            clip: track("synthetic.never_started"),
            node: cargo_node.clone(),
            instance: instance(1),
            generation,
        })
        .id();
    let foreign_pose = rotor_turn(3);
    world
        .entity_mut(foreign)
        .insert(NodeAnimatedPose(foreign_pose));

    play_animation(&mut world, &clip, instance(1), generation, Tick(0))
        .expect("the first instance starts");
    play_animation(&mut world, &clip, instance(2), generation, Tick(0))
        .expect("the second instance starts");
    advance_animation(&mut world, Tick(1));

    for entity in [stopped_cargo, kept_cargo] {
        assert_eq!(
            world.get::<ChildOf>(entity).map(ChildOf::parent),
            Some(bay),
            "the clip attached both cargo nodes to the bay on its attach tick"
        );
        assert_eq!(
            world
                .get::<NodeAnimatedMaterial>(entity)
                .map(|m| m.material().clone()),
            Some(material(SYNTHETIC_CARGO_MATERIAL)),
            "both instances applied the clip's first material"
        );
    }
    let stopped_pose = world
        .get::<NodeVisualTransform>(stopped_cargo)
        .expect("the cargo node has a composed pose")
        .0;

    assert!(stop_animation(&mut world, &clip_id, instance(1)));

    // What the stopped instance applied is gone from its own entity.
    assert_eq!(
        world.get::<NodeAnimatedAttachment>(stopped_cargo),
        None,
        "the stopped instance's attachment record is released"
    );
    assert_eq!(
        world.get::<NodeAnimatedMaterial>(stopped_cargo),
        None,
        "the stopped instance's material is released"
    );
    assert_eq!(world.get::<NodeAnimatedPose>(stopped_cargo), None);
    assert_eq!(
        world.get::<AnimatedNodeBinding>(stopped_cargo),
        None,
        "the binding named an instance that no longer plays"
    );
    assert_eq!(
        world.get::<AppliedAttachment>(stopped_cargo),
        None,
        "the consumer's bookkeeping for that instance is released"
    );
    assert_eq!(
        world.get::<ChildOf>(stopped_cargo),
        None,
        "the animation-managed link is released before its parent could go away \
         (non-negotiable behavior 4)"
    );
    assert_eq!(
        world
            .get::<NodeVisualTransform>(stopped_cargo)
            .map(|pose| pose.0),
        Some(stopped_pose),
        "a release preserves the composed world pose by construction"
    );
    assert_eq!(
        world.get::<LinearVelocity>(stopped_cargo).map(|v| v.0),
        Some(Vec3::new(10.0, 0.0, 0.0)),
        "the released node inherits the velocity of the parent it was linked to"
    );

    // The other instance of the same track is untouched.
    assert_eq!(
        world.get::<ChildOf>(kept_cargo).map(ChildOf::parent),
        Some(bay),
        "the other instance's hierarchy link is not this teardown's to release"
    );
    assert_eq!(
        world
            .get::<NodeAnimatedAttachment>(kept_cargo)
            .map(|record| record.attachment().parent.is_some()),
        Some(true),
        "the other instance's attachment record survives"
    );
    assert_eq!(
        world
            .get::<AnimatedNodeBinding>(kept_cargo)
            .map(|b| b.instance),
        Some(instance(2)),
        "the other instance's binding survives"
    );
    assert_eq!(
        world.get::<LinearVelocity>(kept_cargo).map(|v| v.0),
        Some(Vec3::ZERO),
        "the other instance's velocity is untouched"
    );
    assert!(
        world
            .resource::<AnimationPlayback>()
            .is_playing(&clip_id, instance(2)),
        "the other instance of the track keeps playing"
    );
    assert_eq!(world.resource::<AnimationPlayback>().len(), 1);

    // A state that belongs to another clip is not this teardown's to clear.
    assert_eq!(
        world
            .get::<NodeAnimatedPose>(foreign)
            .map(|pose| pose.pose()),
        Some(foreign_pose),
        "a state belonging to another track survives another instance's teardown"
    );
    assert!(
        world.get::<AnimatedNodeBinding>(foreign).is_some(),
        "a binding to a track that never played is not this instance's"
    );

    // The kept instance keeps running; the released one is never re-attached.
    advance_animation(&mut world, Tick(SYNTHETIC_CARGO_SCORCH_TICK));
    assert_eq!(
        world
            .get::<NodeAnimatedMaterial>(kept_cargo)
            .map(|m| m.material().clone()),
        Some(material(SYNTHETIC_CARGO_SCORCHED_MATERIAL)),
        "the surviving instance still plays its material swap"
    );
    assert_eq!(
        world.get::<ChildOf>(kept_cargo).map(ChildOf::parent),
        Some(bay)
    );
    assert_eq!(
        world.get::<ChildOf>(stopped_cargo),
        None,
        "the advance must not re-attach what the teardown released"
    );
    assert_eq!(world.get::<NodeAnimatedMaterial>(stopped_cargo), None);

    // The clip's own detach (its last authored transition) does not double
    // inherit on the node the teardown already released.
    advance_animation(&mut world, Tick(SYNTHETIC_CARGO_DETACH_TICK));
    assert_eq!(
        world.get::<LinearVelocity>(stopped_cargo).map(|v| v.0),
        Some(Vec3::new(10.0, 0.0, 0.0)),
        "the released node inherits once, not again"
    );
}

/// The identity rules around a live instance: a second play of the *same*
/// identity is refused, a different identity plays beside it, and the same
/// identity plays again after a stop — with a fresh producer serial, so its
/// one-shot marker fires once more under ids the earlier activation could not
/// have used.
#[test]
fn accept_f20_c_02_a_second_play_of_the_same_identity_is_refused_and_a_new_one_starts_after_a_stop()
{
    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(7)));
    let generation = SceneGeneration::default().next();

    let clip = declared_synthetic_propeller_clip();
    let clip_id = clip.id().clone();
    let rotor = bind(
        &mut world,
        &clip_id,
        &node(SYNTHETIC_PROPELLER_NODE),
        instance(1),
        generation,
    );

    play_animation(&mut world, &clip, instance(1), generation, Tick(0))
        .expect("the first instance starts");
    assert_eq!(
        play_animation(&mut world, &clip, instance(1), generation, Tick(0)),
        Err(AnimationPlayError::AlreadyPlaying {
            clip: clip_id.clone(),
            instance: instance(1),
        }),
        "a second play of the same identity is refused, never substituted"
    );
    play_animation(&mut world, &clip, instance(2), generation, Tick(0))
        .expect("a different identity is a different live instance");
    assert_eq!(world.resource::<AnimationPlayback>().len(), 2);

    advance_animation(&mut world, Tick(1));
    let first_activation = drain(&mut world);
    assert_eq!(
        gameplay(&first_activation).len(),
        2,
        "both live instances fired their one-shot gameplay cue"
    );
    let first_ids: Vec<_> = first_activation
        .events()
        .iter()
        .filter(|event| event.effect.is_gameplay())
        .map(|event| event.id)
        .collect();
    let first_producer = world
        .resource::<AnimationPlayback>()
        .producer(&clip_id, instance(1))
        .expect("still live");

    // Stop it, and play the same identity again: a fresh activation.
    assert!(stop_animation(&mut world, &clip_id, instance(1)));
    assert_eq!(
        applied_pose(&world, rotor),
        None,
        "the stop released the previous activation's applied state"
    );
    let replay_rotor = bind(
        &mut world,
        &clip_id,
        &node(SYNTHETIC_PROPELLER_NODE),
        instance(1),
        generation,
    );
    play_animation(&mut world, &clip, instance(1), generation, Tick(1))
        .expect("the identity is free again, so the play is not refused");

    let replayed_producer = world
        .resource::<AnimationPlayback>()
        .producer(&clip_id, instance(1))
        .expect("the replay is live");
    assert_ne!(
        first_producer, replayed_producer,
        "a replayed activation gets a fresh producer serial"
    );

    advance_animation(&mut world, Tick(2));
    let replay = drain(&mut world);
    assert_eq!(
        gameplay(&replay).len(),
        1,
        "the replayed activation fires its one-shot marker once, and the \
         untouched instance does not fire it again"
    );
    let replay_id = replay
        .events()
        .iter()
        .find(|event| event.effect.is_gameplay())
        .expect("the replayed instance fired")
        .id;
    assert_eq!(replay_id.producer, replayed_producer);
    assert!(
        !first_ids.contains(&replay_id),
        "the replayed event cannot reuse an id the first activation fired with"
    );
    assert_eq!(
        applied_pose(&world, replay_rotor),
        Some(rotor_pose_at(1)),
        "the replayed activation drives the entity bound to it"
    );
    assert_eq!(
        applied_pose(&world, rotor),
        None,
        "the released entity of the first activation stays released"
    );
}

/// A scene load that superseded a generation releases what its instances
/// applied, and leaves the instance of the live generation alone.
#[test]
fn accept_f20_c_02_a_superseded_scene_generation_releases_what_its_instances_applied() {
    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(9)));

    // The load path's own counter: two generations consumed, the second is
    // the live one.
    let mut generations = SceneGenerations::default();
    let superseded = generations.take_next();
    let live = generations.take_next();
    world.insert_resource(generations);

    let clip = declared_synthetic_propeller_clip();
    let clip_id = clip.id().clone();
    let rotor_node = node(SYNTHETIC_PROPELLER_NODE);
    let stale = bind(&mut world, &clip_id, &rotor_node, instance(1), superseded);
    let current = bind(&mut world, &clip_id, &rotor_node, instance(2), live);

    play_animation(&mut world, &clip, instance(1), superseded, Tick(0))
        .expect("the superseded instance starts");
    play_animation(&mut world, &clip, instance(2), live, Tick(0))
        .expect("the live instance starts");
    advance_animation(&mut world, Tick(1));
    assert!(applied_pose(&world, stale).is_some());
    assert!(applied_pose(&world, current).is_some());

    let released = release_superseded_instances(&mut world);

    assert_eq!(
        released,
        vec![InstanceKey::new(&clip_id, instance(1))],
        "only the instance of the superseded generation is released"
    );
    assert_eq!(
        world.get::<NodeAnimatedPose>(stale),
        None,
        "the superseded instance's applied state is released"
    );
    assert_eq!(
        world.get::<AnimatedNodeBinding>(stale),
        None,
        "the superseded instance's binding is released"
    );
    assert_eq!(
        applied_pose(&world, current),
        Some(rotor_pose_at(1)),
        "the live generation's instance keeps its state"
    );
    assert_eq!(
        world
            .get::<AnimatedNodeBinding>(current)
            .map(|b| b.instance),
        Some(instance(2))
    );
    let playback = world.resource::<AnimationPlayback>();
    assert_eq!(playback.len(), 1);
    assert!(!playback.is_playing(&clip_id, instance(1)));
    assert!(playback.is_playing(&clip_id, instance(2)));

    // A second call is a no-op: nothing superseded is left to release.
    assert!(release_superseded_instances(&mut world).is_empty());
}

/// A teardown propagates what its release could not measure instead of
/// dropping it: a cargo released from a chain that carries no velocity at all
/// publishes one `VelocityNotInherited` naming the reason, while the other
/// instance of the same track — still playing — publishes nothing.
#[test]
fn accept_f20_c_02_the_teardown_reports_a_release_it_could_not_inherit_a_velocity_for() {
    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(11)));
    let generation = SceneGeneration::default().next();

    let clip = declared_synthetic_cargo_clip();
    let clip_id = clip.id().clone();
    let cargo_node = node(SYNTHETIC_CARGO_NODE);

    // The bay is a scene node that never moved: it carries no velocity
    // component at all, so nothing measurable can be inherited from it.
    let bay = spawn_node(&mut world, SYNTHETIC_CARGO_BAY_NODE, generation, Vec3::ZERO);
    let spawn_cargo = |world: &mut World, at: Vec3| {
        let entity = spawn_node(world, SYNTHETIC_CARGO_NODE, generation, at);
        world
            .entity_mut(entity)
            .insert((ChildOf(bay), Position(at), LinearVelocity(Vec3::ZERO)));
        entity
    };
    let released = spawn_cargo(&mut world, Vec3::new(4.0, 0.0, 0.0));
    let kept = spawn_cargo(&mut world, Vec3::new(-3.0, 0.0, 0.0));
    for (entity, which) in [(released, instance(1)), (kept, instance(2))] {
        world.entity_mut(entity).insert(AnimatedNodeBinding {
            clip: clip_id.clone(),
            node: cargo_node.clone(),
            instance: which,
            generation,
        });
    }

    play_animation(&mut world, &clip, instance(1), generation, Tick(0))
        .expect("the first instance starts");
    play_animation(&mut world, &clip, instance(2), generation, Tick(0))
        .expect("the second instance starts");
    advance_animation(&mut world, Tick(1));
    drain(&mut world);

    assert!(stop_animation(&mut world, &clip_id, instance(1)));

    let published = drain(&mut world);
    assert_eq!(
        published.attachments(),
        &[AttachmentRecord::VelocityNotInherited {
            clip: clip_id.clone(),
            node: cargo_node.clone(),
            reason: VelocitySkipReason::NoVelocitySource,
        }],
        "the teardown publishes what its release could not inherit, and only for \
         the instance it tore down"
    );
    assert_eq!(
        world.get::<ChildOf>(released),
        None,
        "the link is released either way"
    );
    assert_eq!(
        world.get::<ChildOf>(kept).map(ChildOf::parent),
        Some(bay),
        "the surviving instance keeps its hierarchy"
    );

    // A second teardown of a chain that inherits nothing publishes once: the
    // log grows with transitions, never with frames.
    assert!(stop_animation(&mut world, &clip_id, instance(2)));
    let again = drain(&mut world);
    assert_eq!(
        again.attachments().len(),
        1,
        "each instance's teardown publishes its own single record"
    );
}

/// The lowering boundary still produces exactly the runtime twin the
/// multi-instance and teardown scenarios pose values from — the fixtures those
/// tests drive are the declared IR, not a hand-written runtime clip.
#[test]
fn accept_f20_c_02_the_propeller_fixture_drives_the_production_lowering() {
    let declared = declared_synthetic_propeller_clip();
    let lowered = cs_app::animation::lower::lower_clip(&declared).expect("a validated clip lowers");
    let runtime = synthetic_propeller_clip();
    assert_eq!(lowered.id(), runtime.id());
    assert_eq!(lowered.duration_ticks(), runtime.duration_ticks());
    assert_eq!(lowered.channels(), runtime.channels());
    assert_eq!(lowered.markers(), runtime.markers());
}
