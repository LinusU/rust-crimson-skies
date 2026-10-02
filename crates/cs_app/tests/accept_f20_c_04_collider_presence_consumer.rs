//! Acceptance scenarios F20-C.04: the collision-side consumer of a clip-hidden
//! node — the record a hidden node's collision state is written to, and who may
//! lift it.
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-C` (AC01, non-negotiable behavior 3). Task test prefix:
//! `accept_f20_c_04_`.
//!
//! # What these tests observe
//!
//! Every assertion is read from the **physics/collision** side, never from the
//! verdict the animation side published. The two are different observations and
//! the difference is the point of the stage:
//!
//! * the engine's own state — Avian's [`ColliderDisabled`] marker on the node's
//!   entity, which is what takes its collider out of the broad-phase tree
//!   (`On<Add, ColliderDisabled>` / `On<Remove, ColliderDisabled>` in
//!   `avian3d-0.7.0/src/collider_tree/update.rs`);
//! * the **contact reports** the production contact reporter classifies out of
//!   Avian's collision events ([`ContactReports`], one entry per contact
//!   episode between two declared layers) — a hidden node that still stopped a
//!   body would be reported, and a shown node that no longer stops one would
//!   not. That is the observable F20's AC01 asks for ("a door opening at a
//!   fixed tick changes collider and mesh state coherently"), measured on the
//!   collision channel rather than read off a component the animation owns.
//!
//! The animation side is driven through its **production** path only: the
//! declared [`AnimationClip`] through [`play_animation`] and the committed-tick
//! entry [`advance_animation_on_session_tick`], installed by
//! [`AnimationSchedulePlugin`]. No test here writes
//! [`NodeAnimatedVisibility`] by hand, so a design where the collision layer
//! read a test-authored value instead of the clip's own evaluation fails.
//!
//! # The real fixed loop
//!
//! The world is the production composition: [`headless_app`] (the real Avian
//! plugin group), [`PhysicsAdapterPlugin`] at the declared rate,
//! [`PhysicsBodiesPlugin`], [`AnimationSchedulePlugin`] and
//! [`ColliderPresencePlugin`], on a manual clock that runs exactly one fixed
//! step per update. Each session tick is committed by writing
//! [`CommittedSessionTick`] and then running one `App::update`, so the
//! animation advance and the collision-presence pass execute inside the real
//! `FixedPostUpdate` in the order the plugin constrains them, with a real
//! integration between the hide and the next tick's broad phase.
//!
//! Damage's `NodeDisabled` marker and the real `select_lod_presentation` pass
//! run too, so the destruction and LOD paths are exercised around the collision
//! answer rather than assumed. The **damage-side collision decision** is driven
//! through [`remove_collider_for_damage`] / [`restore_collider_after_repair`],
//! because whether a destroyed part's collider goes is F29's decision and
//! F11-C's damage pass writes presentation markers only — this layer applies
//! the decision, it does not make it. That boundary is why the seam refuses a
//! node the spawner never managed, which
//! `accept_f20_c_04_the_damage_seam_refuses_a_node_the_spawner_never_managed`
//! pins.
//!
//! Every value here is newly authored fixture data, not measured original game
//! data: the original animation containers are still undecoded (F13), and
//! F20-D keeps the original-family validation gate.

use std::time::Duration;

use avian3d::prelude::{ColliderDisabled, Gravity, Position};
use bevy::{
    ecs::lifecycle::{Insert, Remove},
    ecs::observer::On,
    ecs::schedule::Schedule,
    ecs::world::World,
    prelude::{App, Entity, GlobalTransform, ResMut, Resource},
    time::{Real, Time, TimeUpdateStrategy},
};
use cs_app::animation::{
    AnimatedNodeBinding, AnimationInstance, AnimationPlayback, AnimationSchedulePlugin,
    CommittedSessionTick, NodeAnimatedVisibility, play_animation,
};
use cs_app::asset_stack::headless_app;
use cs_app::physics::{
    BASELINE_FIXED_HZ, BodyMode, BodySpec, ColliderDecisionError, ColliderPresenceLedger,
    ColliderPresencePlugin, ContactReports, NodeColliderPresence, PhysicsAdapterPlugin,
    PhysicsBodiesPlugin, apply_collider_presence, remove_collider_for_damage,
    restore_collider_after_repair, spawn_body,
};
use cs_app::scene::{
    LodDistance, NodeDisabled, NodeLodVariant, NodePresentation, NodeVisualTransform,
    PresentationState, SceneGeneration, SceneNodeBinding, select_lod_presentation,
};
use cs_content::animation::{
    SYNTHETIC_BREAKABLE_DURATION, SYNTHETIC_BREAKABLE_HIDDEN_TICK, SYNTHETIC_BREAKABLE_NODE,
    SYNTHETIC_BREAKABLE_SHOWN_TICK, declared_synthetic_breakable_clip,
};
use cs_content::scene::LodInfo;
use cs_sim::animated_object::Visibility;
use cs_sim::collision::{CollisionLayer, ShapeClass};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;
use cs_types::space::Meters;

// -------------------------------------------------------------- constants ---

/// The session the playback stamps its events with.
const SESSION: u64 = 41;
/// A viewer distance inside the hatch's near band, where it is presented.
const NEAR_METRES: f64 = 50.0;
/// A viewer distance outside that band, where the group presents its other
/// variant and the hatch is culled.
const FAR_METRES: f64 = 200.0;
/// The near band the hatch carries.
const NEAR_BAND: (f64, f64) = (0.0, 100.0);
/// The far band of the same group, so the hatch really can be culled.
const FAR_BAND: (f64, f64) = (100.0, 500.0);

/// The node under test: a static solid 1 m box at the origin, on the
/// `static_world` layer, so the declared interaction matrix makes it a real
/// obstacle for a `projectile`-layer probe.
const NODE_HALF_M: f32 = 0.5;
/// The far band's node: the same part at another distance, culled at the near
/// one. It carries no collider and no presence record; it is presentation only.
const FAR_NODE: &str = "synthetic.plane.hatch.lod1";

/// The probe that flies at the node: 0.2 m across, 40 m/s. 0.33 m of travel per
/// tick at the declared 120 Hz, so the discrete phase cannot tunnel the 1 m
/// node and the observation is the collider's presence, not swept detection.
const PROBE_HALF_M: f32 = 0.1;
const PROBE_SPEED_M_S: f32 = 40.0;
/// Where the probe starts, in metres: 3 m short of the node's near face.
const PROBE_START_X_M: f32 = -3.0;
/// How many fixed ticks the probe is given to arrive.
const APPROACH_TICKS: u64 = 40;
/// The `x` past the node's far face, i.e. proof it went through rather than
/// being stopped.
const THROUGH_X_M: f32 = 1.5;

// ---------------------------------------------------------------- helpers ---

fn node(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::SceneNode, key).expect("valid content id")
}

fn track(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::AnimationTrack, key).expect("valid content id")
}

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

fn instance(value: u32) -> AnimationInstance {
    AnimationInstance::new(value).expect("a nonzero instance identity")
}

fn generation() -> SceneGeneration {
    SceneGeneration::default().next()
}

fn band(min: f64, max: f64) -> NodeLodVariant {
    NodeLodVariant::new(LodInfo {
        level: false,
        range_min: Meters(min),
        range_max: Meters(max),
    })
    .expect("the band range is usable")
}

/// Counts the engine-side writes to Avian's marker.
///
/// A value comparison cannot tell "written again with the same value" from "not
/// written", and the content of the idempotence rule is which of those happened.
/// Bevy's component hooks fire on every real add and every real remove, so this
/// observes the engine channel itself rather than this layer's own report.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
struct MarkerWrites {
    disabled_inserted: u32,
    disabled_removed: u32,
}

/// The static node: one real body, one real collider, the scene binding and its
/// LOD band.
fn node_spec() -> BodySpec {
    BodySpec {
        layer: CollisionLayer::StaticWorld,
        shape: ShapeClass::Solid,
        mode: BodyMode::Static,
        // A static body is not accelerated by forces, so no mass is declared.
        mass_kg: 0.0,
        half_extents_m: [NODE_HALF_M; 3],
        position_m: [0.0; 3],
        linear_velocity_m_s: [0.0; 3],
    }
}

/// The probe: a dynamic `projectile`-layer body flying at the node.
fn probe_spec() -> BodySpec {
    BodySpec {
        layer: CollisionLayer::Projectile,
        shape: ShapeClass::Solid,
        mode: BodyMode::Dynamic,
        mass_kg: 1.0,
        half_extents_m: [PROBE_HALF_M; 3],
        position_m: [PROBE_START_X_M, 0.0, 0.0],
        linear_velocity_m_s: [PROBE_SPEED_M_S, 0.0, 0.0],
    }
}

/// The world a test drives: the production fixed loop, plus the two observer
/// counters the assertions read.
fn world_with_the_real_loop() -> App {
    let mut app = headless_app();
    app.add_plugins(PhysicsAdapterPlugin::new(BASELINE_FIXED_HZ));
    app.add_plugins(PhysicsBodiesPlugin);
    app.add_plugins(AnimationSchedulePlugin);
    app.add_plugins(ColliderPresencePlugin);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / BASELINE_FIXED_HZ as f64,
    )));
    app.insert_resource(Gravity::ZERO);
    // The same baseline the production fixtures seed, so the first update
    // already produces the full manual delta and one fixed step
    // (`docs/findings/2026-09-23-t334-first-frame-fixed-step.md`).
    let startup = app.world().resource::<Time<Real>>().startup();
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .update_with_instant(startup);
    app.insert_resource(AnimationPlayback::new(session(SESSION)));
    app.insert_resource(LodDistance::new(Meters(NEAR_METRES)).expect("a usable distance"));
    count_marker_writes(app.world_mut());
    app.finish();
    app.cleanup();
    app
}

fn count_marker_writes(world: &mut World) {
    world.init_resource::<MarkerWrites>();
    world.add_observer(
        |_insert: On<Insert, ColliderDisabled>, mut writes: ResMut<MarkerWrites>| {
            writes.disabled_inserted += 1;
        },
    );
    world.add_observer(
        |_remove: On<Remove, ColliderDisabled>, mut writes: ResMut<MarkerWrites>| {
            writes.disabled_removed += 1;
        },
    );
}

/// The node under test, with the collision-presence record a spawner inserts
/// for an authored `CollisionRole::Collider` and the animation binding the
/// breakable clip verifies.
fn spawn_hatch(app: &mut App, at: SceneGeneration) -> Entity {
    let world = app.world_mut();
    let hatch = spawn_body(world, &node_spec()).expect("a valid static body spec");
    world.entity_mut(hatch).insert((
        SceneNodeBinding {
            node: node(SYNTHETIC_BREAKABLE_NODE),
            generation: at,
        },
        NodeVisualTransform(GlobalTransform::IDENTITY),
        NodePresentation(PresentationState::Drawn),
        band(NEAR_BAND.0, NEAR_BAND.1),
        // The opt-in this layer's boundary is. A node without it is never
        // touched by the pass, which its own test pins.
        NodeColliderPresence::Live,
        AnimatedNodeBinding {
            clip: track("synthetic.breakable"),
            node: node(SYNTHETIC_BREAKABLE_NODE),
            instance: instance(1),
            generation: at,
        },
    ));
    // The far band of the same group — a sibling under the same (absent) parent,
    // which is how `select_lod_presentation` groups variants — so the LOD pass
    // can really cull the hatch. It is presentation only: no collider, no
    // presence record.
    world.spawn((
        SceneNodeBinding {
            node: node(FAR_NODE),
            generation: at,
        },
        NodeVisualTransform(GlobalTransform::IDENTITY),
        NodePresentation(PresentationState::Drawn),
        band(FAR_BAND.0, FAR_BAND.1),
    ));
    hatch
}

/// Spawns a probe flying at the node.
fn spawn_probe(app: &mut App) -> Entity {
    spawn_body(app.world_mut(), &probe_spec()).expect("a valid dynamic body spec")
}

/// Removes a probe that has served its assertion.
///
/// A spent probe is not inert: two `projectile`-layer bodies are a **designed
/// contact pair** (`cs_sim::collision`, `Projectile`×`Projectile`), so a probe
/// left resting against the node exchanges momentum with the next one fired down
/// the same lane, and the phase under test becomes a contact between two probes
/// rather than between a probe and the node. Each phase therefore fires into a
/// lane that is empty but for the node.
fn retire(app: &mut App, probe: Entity) {
    app.world_mut().entity_mut(probe).despawn();
}

/// A second static body the spawner never put under the collision policy: a
/// real collider, no [`NodeColliderPresence`].
fn spawn_unmanaged(app: &mut App) -> Entity {
    let mut spec = node_spec();
    spec.position_m = [40.0, 0.0, 0.0];
    spawn_body(app.world_mut(), &spec).expect("a valid static body spec")
}

fn play_breakable(app: &mut App, at: SceneGeneration) {
    play_animation(
        app.world_mut(),
        &declared_synthetic_breakable_clip(),
        instance(1),
        at,
        Tick(0),
    )
    .expect("the declared breakable clip starts");
}

/// Commits one session tick and runs one real fixed step, which is what a
/// session driver plus `App::update` do.
fn commit_and_step(app: &mut App, tick: u64) {
    app.world_mut()
        .insert_resource(CommittedSessionTick::new(Tick(tick)));
    app.update();
}

/// Runs the real F11-C LOD pass — the crate's only writer of the presentation
/// record — in its own schedule, the way the product runs it.
fn run_lod_pass(world: &mut World) {
    let mut schedule = Schedule::default();
    schedule.add_systems(select_lod_presentation);
    schedule.run(world);
}

fn set_distance(app: &mut App, distance: f64) {
    app.world_mut()
        .insert_resource(LodDistance::new(Meters(distance)).expect("a usable distance"));
}

fn presence(world: &World, entity: Entity) -> Option<NodeColliderPresence> {
    world.get::<NodeColliderPresence>(entity).copied()
}

/// The engine-side collision state: Avian's own marker on the entity.
fn engine_disabled(world: &World, entity: Entity) -> bool {
    world.get::<ColliderDisabled>(entity).is_some()
}

fn applied_visibility(world: &World, entity: Entity) -> Option<Visibility> {
    world
        .get::<NodeAnimatedVisibility>(entity)
        .map(NodeAnimatedVisibility::visibility)
}

fn presentation(world: &World, entity: Entity) -> Option<PresentationState> {
    world
        .get::<NodePresentation>(entity)
        .map(|presentation| presentation.0)
}

fn position_x(world: &World, entity: Entity) -> f32 {
    world
        .get::<Position>(entity)
        .expect("a spawned body carries a Position")
        .0
        .x
}

fn marker_writes(world: &World) -> MarkerWrites {
    *world.resource::<MarkerWrites>()
}

fn ledger(world: &World) -> ColliderPresenceLedger {
    *world.resource::<ColliderPresenceLedger>()
}

/// Whether the production contact reporter reported a contact between `probe`
/// and `node` on the tick that just ran.
///
/// Read straight out of [`ContactReports`], the resource the crate hands a
/// consumer each frame; the reporter clears its batch when the tick advances,
/// and one `App::update` runs exactly one fixed step, so this is that tick's
/// account of whether the node was an obstacle.
fn contact_this_tick(world: &World, probe: Entity, node: Entity) -> bool {
    world
        .resource::<ContactReports>()
        .reports()
        .iter()
        .any(|report| report.bodies.contains(&probe) && report.bodies.contains(&node))
}

/// Holds the committed session tick and runs `ticks` fixed steps; reports
/// whether the probe and the node were ever in contact during them.
fn run_ticks(app: &mut App, session_tick: u64, ticks: u64, probe: Entity, node: Entity) -> bool {
    let mut contacted = false;
    for _ in 0..ticks {
        commit_and_step(app, session_tick);
        contacted |= contact_this_tick(app.world(), probe, node);
    }
    contacted
}

// --------------------------------- 1. the minimum scenario: an AC01 door ---

/// The minimum scenario, on the collision channel: a clip that hides a node at
/// its authored tick takes that node's collider out of the simulation, and the
/// clip's show tick puts it back.
///
/// The observation is deliberately **not** the verdict. Each phase fires a real
/// probe at the node through the real physics step and reads the production
/// contact reporter, so a design that published `ColliderVerdict::NoCollider`
/// and stopped there — leaving the collider in the broad phase — fails at the
/// contact assertion rather than at a component check. The engine marker is
/// asserted too, as the state the engine itself holds.
#[test]
fn accept_f20_c_04_a_clip_hidden_node_stops_colliding_and_its_show_restores_the_collider() {
    let at = generation();
    let mut app = world_with_the_real_loop();
    let hatch = spawn_hatch(&mut app, at);
    play_breakable(&mut app, at);

    // The clip shows the node at tick 0, so the authored collider is live.
    commit_and_step(&mut app, 0);
    assert_eq!(
        applied_visibility(app.world(), hatch),
        Some(Visibility::Visible),
        "the clip's first key shows the node"
    );
    assert_eq!(
        presence(app.world(), hatch),
        Some(NodeColliderPresence::Live)
    );
    assert!(
        !engine_disabled(app.world(), hatch),
        "a node the clip does not hide carries its authored collider"
    );
    let shown_probe = spawn_probe(&mut app);
    assert!(
        run_ticks(&mut app, 0, APPROACH_TICKS, shown_probe, hatch),
        "a shown node stops a body: the probe's contact with it is reported"
    );
    assert!(
        position_x(app.world(), shown_probe) < THROUGH_X_M,
        "and the probe was stopped short of the node's far face (x = {})",
        position_x(app.world(), shown_probe)
    );
    retire(&mut app, shown_probe);

    // The authored hide tick, through the wired fixed-tick entry. The
    // collision-presence pass runs after it in the same fixed step, so the
    // engine marker is in place before the next integration.
    commit_and_step(&mut app, SYNTHETIC_BREAKABLE_HIDDEN_TICK);
    assert_eq!(
        applied_visibility(app.world(), hatch),
        Some(Visibility::Hidden),
        "the clip's hide key applied to the verified binding"
    );
    assert_eq!(
        presence(app.world(), hatch),
        Some(NodeColliderPresence::HiddenByAnimation),
        "the collision side records the clip's hide as its own decision"
    );
    assert!(
        engine_disabled(app.world(), hatch),
        "the engine's own marker is what removes the collider from the broad phase"
    );

    // A second probe, fired at the now-hidden node. Holding the committed tick
    // keeps the clip from advancing, so the node stays hidden for every tick of
    // the approach.
    let hidden_probe = spawn_probe(&mut app);
    assert!(
        !run_ticks(
            &mut app,
            SYNTHETIC_BREAKABLE_HIDDEN_TICK,
            APPROACH_TICKS,
            hidden_probe,
            hatch
        ),
        "a clip-hidden node is not an obstacle: no contact is reported against it"
    );
    assert!(
        position_x(app.world(), hidden_probe) > THROUGH_X_M,
        "and the probe flew through the node's volume (x = {}), rather than being stopped",
        position_x(app.world(), hidden_probe)
    );
    retire(&mut app, hidden_probe);

    // The clip's show tick puts the collider back, and a third probe is stopped
    // by it again.
    commit_and_step(&mut app, SYNTHETIC_BREAKABLE_SHOWN_TICK);
    assert_eq!(
        applied_visibility(app.world(), hatch),
        Some(Visibility::Visible)
    );
    assert_eq!(
        presence(app.world(), hatch),
        Some(NodeColliderPresence::Live),
        "the clip stops deciding collision the moment it stops hiding"
    );
    assert!(
        !engine_disabled(app.world(), hatch),
        "the engine marker is removed, so the collider re-enters the broad phase"
    );
    let restored_probe = spawn_probe(&mut app);
    assert!(
        run_ticks(
            &mut app,
            SYNTHETIC_BREAKABLE_SHOWN_TICK,
            APPROACH_TICKS,
            restored_probe,
            hatch
        ),
        "the restored collider is a real obstacle again"
    );
    assert!(
        position_x(app.world(), restored_probe) < THROUGH_X_M,
        "and the probe is stopped short of it (x = {})",
        position_x(app.world(), restored_probe)
    );

    // The engine channel moved exactly twice: disabled at the hide, enabled at
    // the show. Every other tick in between ran the pass and wrote nothing.
    let writes = marker_writes(app.world());
    assert_eq!(
        (writes.disabled_inserted, writes.disabled_removed),
        (1, 1),
        "one disable and one enable, from the hide and the show and nothing else"
    );
}

// ------------------------------- 2. destruction outranks the clip, forever ---

/// Non-negotiable behavior 3, on the collision channel: once the damage side
/// has removed the collider, no clip pass brings it back.
///
/// The clip **loops** and re-shows the node on every pass, so this walks four
/// passes tick by tick, with the re-shows counted so the test cannot pass
/// vacuously. A distance change and the instance teardown are in the same
/// assertion, because both are ways a node can come back without the clip
/// asking.
#[test]
fn accept_f20_c_04_a_damage_removed_collider_is_never_restored_by_a_clip_loop_or_a_lod_pass() {
    let at = generation();
    let mut app = world_with_the_real_loop();
    let hatch = spawn_hatch(&mut app, at);
    play_breakable(&mut app, at);

    // The clip hides the node first, so the two removals are distinguishable:
    // the animation's is the clip's, the damage one is the damage side's.
    commit_and_step(&mut app, SYNTHETIC_BREAKABLE_HIDDEN_TICK);
    assert_eq!(
        presence(app.world(), hatch),
        Some(NodeColliderPresence::HiddenByAnimation)
    );

    // Damage: its own presentation marker, the real LOD pass that folds it,
    // and then the collision decision itself, which is F29's to make and this
    // layer's to apply.
    app.world_mut().entity_mut(hatch).insert(NodeDisabled);
    run_lod_pass(app.world_mut());
    assert_eq!(
        presentation(app.world(), hatch),
        Some(PresentationState::Disabled)
    );
    assert!(
        remove_collider_for_damage(app.world_mut(), hatch)
            .expect("the spawner put this node under the collision policy"),
        "the damage removal changes the physics-side state"
    );
    assert_eq!(
        presence(app.world(), hatch),
        Some(NodeColliderPresence::RemovedByDamage),
        "the record names damage as the owner of the removal, not the clip"
    );
    assert!(engine_disabled(app.world(), hatch));
    let writes_after_removal = marker_writes(app.world());

    // Four loop passes. The clip re-shows the node on every one of them, and the
    // collider stays off at every tick.
    let mut re_shown = 0u32;
    let mut hidden_by_clip = 0u32;
    for tick in 1..=SYNTHETIC_BREAKABLE_DURATION * 4 {
        commit_and_step(&mut app, tick);
        match applied_visibility(app.world(), hatch) {
            Some(Visibility::Visible) => re_shown += 1,
            Some(Visibility::Hidden) => hidden_by_clip += 1,
            None => {}
        }
        assert_eq!(
            presence(app.world(), hatch),
            Some(NodeColliderPresence::RemovedByDamage),
            "session tick {tick}: a damage removal is terminal for the collision policy"
        );
        assert!(
            engine_disabled(app.world(), hatch),
            "session tick {tick}: the engine marker is still in place"
        );
    }
    assert!(
        re_shown >= 4,
        "the loop really did try to re-show the node on every pass ({re_shown} times)"
    );
    assert!(
        hidden_by_clip >= 4,
        "and it really did re-hide it on every pass ({hidden_by_clip} times), so the clip was \
         driving the visibility channel throughout"
    );

    // A distance change cannot bring it back either.
    for distance in [FAR_METRES, NEAR_METRES] {
        set_distance(&mut app, distance);
        run_lod_pass(app.world_mut());
        assert_eq!(
            presence(app.world(), hatch),
            Some(NodeColliderPresence::RemovedByDamage),
            "at {distance} m a destroyed part's collider stays removed"
        );
    }

    // And the teardown does not either.
    assert!(
        cs_app::animation::stop_animation(
            app.world_mut(),
            &track("synthetic.breakable"),
            instance(1)
        ),
        "the instance is torn down"
    );
    assert!(
        app.world().get::<NodeAnimatedVisibility>(hatch).is_none(),
        "the teardown released the clip's own record"
    );
    assert_eq!(
        presence(app.world(), hatch),
        Some(NodeColliderPresence::RemovedByDamage),
        "with the clip gone the damage decision is still the one that stands"
    );
    assert_eq!(
        marker_writes(app.world()),
        writes_after_removal,
        "none of those four passes, distances or the teardown wrote to the engine marker"
    );

    // A probe fired at the destroyed part is not stopped: the removal is real
    // collision state, not only a record nobody reads.
    let probe = spawn_probe(&mut app);
    let tick = SYNTHETIC_BREAKABLE_DURATION * 4;
    assert!(
        !run_ticks(&mut app, tick, APPROACH_TICKS, probe, hatch),
        "a part whose collider damage removed does not stop a body"
    );
    assert!(position_x(app.world(), probe) > THROUGH_X_M);

    // Only the damage side lifts it: a repair with nothing hiding the node puts
    // the collider back, and the engine sees it.
    assert!(
        restore_collider_after_repair(app.world_mut(), hatch).expect("the node is still managed"),
        "the repair changes the physics-side state"
    );
    assert_eq!(
        presence(app.world(), hatch),
        Some(NodeColliderPresence::Live)
    );
    assert!(!engine_disabled(app.world(), hatch));
    let repaired_probe = spawn_probe(&mut app);
    assert!(
        run_ticks(&mut app, tick, APPROACH_TICKS, repaired_probe, hatch),
        "the repaired part is an obstacle again"
    );
}

/// A repair must not expose a node the clip is still hiding: the damage side
/// lifting its own removal hands the answer back to the clip's verdict rather
/// than to a default.
///
/// This is the direction the merge rule has to get right in both halves. A
/// repair that wrote `Live` unconditionally would put a node the animation has
/// hidden back into the broad phase, which is the invisible obstacle F20-A's
/// rule exists to prevent — reached from the other side.
#[test]
fn accept_f20_c_04_a_repair_under_a_still_hiding_clip_leaves_the_collider_off() {
    let at = generation();
    let mut app = world_with_the_real_loop();
    let hatch = spawn_hatch(&mut app, at);
    play_breakable(&mut app, at);

    commit_and_step(&mut app, SYNTHETIC_BREAKABLE_HIDDEN_TICK);
    remove_collider_for_damage(app.world_mut(), hatch).expect("the node is managed");
    assert_eq!(
        presence(app.world(), hatch),
        Some(NodeColliderPresence::RemovedByDamage)
    );

    assert!(
        restore_collider_after_repair(app.world_mut(), hatch).expect("the node is still managed"),
        "the damage side lifted its own removal, so the record changed owner"
    );
    assert_eq!(
        presence(app.world(), hatch),
        Some(NodeColliderPresence::HiddenByAnimation),
        "the clip still hides the node, so the clip's removal is what stands"
    );
    assert!(
        engine_disabled(app.world(), hatch),
        "and the engine marker never came off: no hidden node is exposed by a repair"
    );

    // A probe confirms it from the collision channel rather than the component.
    let probe = spawn_probe(&mut app);
    assert!(
        !run_ticks(
            &mut app,
            SYNTHETIC_BREAKABLE_HIDDEN_TICK,
            APPROACH_TICKS,
            probe,
            hatch
        ),
        "still no contact: the hidden node is not an obstacle after a repair"
    );
    assert!(position_x(app.world(), probe) > THROUGH_X_M);

    // The clip's show tick then restores the collider, with no damage record in
    // force any more.
    commit_and_step(&mut app, SYNTHETIC_BREAKABLE_SHOWN_TICK);
    assert_eq!(
        presence(app.world(), hatch),
        Some(NodeColliderPresence::Live)
    );
    assert!(!engine_disabled(app.world(), hatch));
}

// ---------------------------------------- 2b. the teardown gives it back ---

/// The instance teardown returns the collider a hiding clip had removed.
///
/// The third arm of the merge rule — `Undecided` never removes anything — has a
/// production trigger of its own besides the clip's show key: the teardown
/// releases the clip's own record, so a node stops being hidden without anything
/// asking to be shown. That is the release path a scene load or a
/// superseding-generation teardown uses, and it is the one way a node can come
/// back here that the clip did not arrange, so it is pinned on the collision
/// channel rather than left to the pure merge table. A visibility record that
/// outlived its instance would leave a node nothing draws and nothing may hit,
/// which is the invisible obstacle from the other end.
#[test]
fn accept_f20_c_04_a_clip_teardown_returns_the_collider_a_hiding_clip_had_removed() {
    let at = generation();
    let mut app = world_with_the_real_loop();
    let hatch = spawn_hatch(&mut app, at);
    play_breakable(&mut app, at);

    commit_and_step(&mut app, SYNTHETIC_BREAKABLE_HIDDEN_TICK);
    assert_eq!(
        presence(app.world(), hatch),
        Some(NodeColliderPresence::HiddenByAnimation)
    );
    assert!(engine_disabled(app.world(), hatch));
    let hidden_probe = spawn_probe(&mut app);
    assert!(
        !run_ticks(
            &mut app,
            SYNTHETIC_BREAKABLE_HIDDEN_TICK,
            APPROACH_TICKS,
            hidden_probe,
            hatch
        ),
        "the clip's hide really did take the collider out of the simulation"
    );
    assert!(position_x(app.world(), hidden_probe) > THROUGH_X_M);
    retire(&mut app, hidden_probe);
    let writes = marker_writes(app.world());

    // The teardown, with no damage decision anywhere in this world: the only
    // thing that changes is that the clip stopped driving the node.
    assert!(
        cs_app::animation::stop_animation(
            app.world_mut(),
            &track("synthetic.breakable"),
            instance(1)
        ),
        "the instance is torn down"
    );
    assert!(
        app.world().get::<NodeAnimatedVisibility>(hatch).is_none(),
        "the teardown released the clip's own record, so the verdict is Undecided again"
    );
    assert_eq!(
        cs_app::animation::composed_visibility_verdict(app.world(), hatch).collider(),
        cs_app::animation::ColliderVerdict::Undecided
    );

    // One more fixed step, with the same committed tick so the clip cannot
    // re-hide the node: the pass re-merges and the collider is back.
    commit_and_step(&mut app, SYNTHETIC_BREAKABLE_HIDDEN_TICK);
    assert_eq!(
        presence(app.world(), hatch),
        Some(NodeColliderPresence::Live),
        "with no clip and no damage decision, the authored collider is what stands"
    );
    assert!(
        !engine_disabled(app.world(), hatch),
        "and the engine marker is off, so the collider re-enters the broad phase"
    );
    assert_eq!(
        marker_writes(app.world()),
        MarkerWrites {
            disabled_inserted: writes.disabled_inserted,
            disabled_removed: writes.disabled_removed + 1,
        },
        "the teardown cost exactly one engine write, the enable"
    );

    let restored_probe = spawn_probe(&mut app);
    assert!(
        run_ticks(
            &mut app,
            SYNTHETIC_BREAKABLE_HIDDEN_TICK,
            APPROACH_TICKS,
            restored_probe,
            hatch
        ),
        "a probe is stopped again: the collider came back with the record"
    );
    assert!(position_x(app.world(), restored_probe) < THROUGH_X_M);
}

// ----------------------------------------------------- 3. the merge rule ---

/// The merge is pure, so its whole truth table is checkable without a world —
/// and this is where the terminal-ness of a damage removal is pinned directly
/// rather than only through a tick-by-tick walk.
#[test]
fn accept_f20_c_04_the_merge_rule_never_lets_a_clip_leave_a_damage_removal() {
    use NodeColliderPresence::{HiddenByAnimation, Live, RemovedByDamage};
    use cs_app::animation::ColliderVerdict;

    // The clip's hide is the only thing that removes a collider on the
    // animation's account, and its own show is the only thing that restores it.
    assert_eq!(Live.merge(ColliderVerdict::Undecided), Live);
    assert_eq!(Live.merge(ColliderVerdict::NoCollider), HiddenByAnimation);
    assert_eq!(
        HiddenByAnimation.merge(ColliderVerdict::NoCollider),
        HiddenByAnimation
    );
    assert_eq!(
        HiddenByAnimation.merge(ColliderVerdict::Undecided),
        Live,
        "the clip stopped hiding the node, so the authored collider is back"
    );

    // A damage removal is terminal: neither verdict enters or leaves it.
    for verdict in [ColliderVerdict::Undecided, ColliderVerdict::NoCollider] {
        assert_eq!(
            RemovedByDamage.merge(verdict),
            RemovedByDamage,
            "a clip verdict of {verdict:?} cannot lift a damage removal"
        );
    }

    // The label and the engine-facing question agree with the state.
    assert!(Live.collider_enabled());
    assert!(!HiddenByAnimation.collider_enabled());
    assert!(!RemovedByDamage.collider_enabled());
    assert_eq!(Live.label(), "collider live");
}

// ------------------------------------------------------- 4. idempotence ---

/// Applying the same verdict twice performs no second physics-side change.
///
/// Checked on both observable channels, because either alone would be weak: the
/// pass's own report could be a no-op while the engine marker moved, and the
/// marker counter could hold still while the record was rewritten under it.
#[test]
fn accept_f20_c_04_applying_the_same_verdict_twice_changes_nothing_on_the_physics_side() {
    let at = generation();
    let mut app = world_with_the_real_loop();
    let hatch = spawn_hatch(&mut app, at);
    play_breakable(&mut app, at);

    // Drive the real loop to the hide, so the pass has already done its work
    // through the plugin, and read the state it left.
    commit_and_step(&mut app, SYNTHETIC_BREAKABLE_HIDDEN_TICK);
    assert_eq!(
        presence(app.world(), hatch),
        Some(NodeColliderPresence::HiddenByAnimation)
    );
    let writes = marker_writes(app.world());
    let ledger_before = ledger(app.world());
    assert!(
        ledger_before.total_collider_writes >= 1,
        "the plugin's pass really wrote the marker in the fixed loop"
    );

    // The same verdict, applied again and again, directly.
    for _ in 0..3 {
        let report = apply_collider_presence(app.world_mut());
        assert!(
            report.is_noop(),
            "a repeated verdict is a no-op: {report:?}"
        );
        assert_eq!(
            report.without_collider, 0,
            "the node carries a real collider"
        );
    }
    assert_eq!(marker_writes(app.world()), writes);

    // And the same through the installed system: holding the committed tick
    // re-runs the pass every fixed step with an unchanged verdict.
    for _ in 0..5 {
        commit_and_step(&mut app, SYNTHETIC_BREAKABLE_HIDDEN_TICK);
    }
    assert_eq!(
        marker_writes(app.world()),
        writes,
        "five more fixed steps wrote nothing to the engine marker"
    );
    let ledger_after = ledger(app.world());
    assert!(
        ledger_after.last.is_noop(),
        "and the pass reported a no-op on the last of them: {:?}",
        ledger_after.last
    );
    assert_eq!(
        (
            ledger_after.total_presence_updates,
            ledger_after.total_collider_writes
        ),
        (
            ledger_before.total_presence_updates,
            ledger_before.total_collider_writes
        ),
        "so the plugin's own counters did not move either: the only writes were the hide's"
    );

    // A repeated *removal* is equally quiet, and so is a repeated repair.
    assert!(
        remove_collider_for_damage(app.world_mut(), hatch).expect("the node is managed"),
        "the first removal changes the record"
    );
    let after_removal = marker_writes(app.world());
    assert!(
        !remove_collider_for_damage(app.world_mut(), hatch).expect("the node is still managed"),
        "a repeated removal changes nothing"
    );
    assert_eq!(marker_writes(app.world()), after_removal);
    assert!(
        restore_collider_after_repair(app.world_mut(), hatch).expect("the node is still managed")
    );
    let after_repair = marker_writes(app.world());
    assert!(
        !restore_collider_after_repair(app.world_mut(), hatch).expect("the node is still managed"),
        "a repeated repair changes nothing"
    );
    assert_eq!(marker_writes(app.world()), after_repair);
}

// --------------------------------------------------------- 5. the boundary ---

/// A node the spawner never put under the collision policy is never touched:
/// the pass does not adopt it, and the damage seam refuses it by name instead
/// of silently doing nothing.
///
/// This pins the opt-in boundary from both sides, because it has a real cost
/// that must not be hidden: a clip-hidden node with no presence record keeps
/// colliding. That cost belongs to the spawn path (F11-C's scene import, or
/// F29's part colliders — neither attaches an Avian collider to a scene node
/// yet), and the test asserts the boundary rather than letting the gap pass
/// unnoticed. The mutation that drops the record from the merge input fails the
/// first two tests, which is how the record's necessity is shown.
#[test]
fn accept_f20_c_04_the_damage_seam_refuses_a_node_the_spawner_never_managed() {
    let at = generation();
    let mut app = world_with_the_real_loop();
    let hatch = spawn_hatch(&mut app, at);
    let unmanaged = spawn_unmanaged(&mut app);
    assert!(
        app.world().get::<NodeColliderPresence>(unmanaged).is_none(),
        "the unmanaged body carries a real collider and no presence record"
    );

    // Even a clip that hides a node does not make this layer adopt one.
    app.world_mut()
        .entity_mut(unmanaged)
        .insert(NodeAnimatedVisibility::new(
            cs_sim::animated_object::Visibility::Hidden,
        ));
    assert_eq!(
        cs_app::animation::composed_visibility_verdict(app.world(), unmanaged).collider(),
        cs_app::animation::ColliderVerdict::NoCollider,
        "the clip does hide this node, so the verdict is there to be read: the pass declines to \
         act on it because the node is not under the policy, not because it saw nothing"
    );
    let report = apply_collider_presence(app.world_mut());
    assert!(
        report.is_noop(),
        "the pass has nothing to do for an unmanaged node, and the managed one is already in \
         the state its record says: {report:?}"
    );
    assert_eq!(
        presence(app.world(), unmanaged),
        None,
        "the pass does not adopt a node the spawner never put under the policy: the opt-in is \
         the spawner's decision, not this layer's"
    );
    assert!(
        !engine_disabled(app.world(), unmanaged),
        "an unmanaged collider is never written to, whatever the animation record says"
    );

    // The damage seam refuses it, and names the entity.
    assert_eq!(
        remove_collider_for_damage(app.world_mut(), unmanaged),
        Err(ColliderDecisionError::UnmanagedNode(unmanaged)),
        "a damage decision against an unmanaged node is refused, not silently dropped"
    );
    assert_eq!(
        restore_collider_after_repair(app.world_mut(), unmanaged),
        Err(ColliderDecisionError::UnmanagedNode(unmanaged))
    );

    // An entity that is not in this world is refused as such.
    let absent = app.world_mut().spawn_empty().id();
    app.world_mut().entity_mut(absent).despawn();
    assert_eq!(
        remove_collider_for_damage(app.world_mut(), absent),
        Err(ColliderDecisionError::UnknownEntity(absent)),
        "a despawned node is reported as unknown, not as unmanaged"
    );

    // The managed node beside it is unaffected by any of that.
    assert_eq!(
        presence(app.world(), hatch),
        Some(NodeColliderPresence::Live),
        "the refused calls changed nothing about the managed node"
    );
    assert!(!engine_disabled(app.world(), hatch));
}

// ----------------------------------------------------- 6. LOD is not a cull ---

/// LOD decides presentation, so a culled node keeps its collider — and a clip
/// that hides a culled node still takes it away.
///
/// The first half is F11-C's own rule ("collision … live on the bound node
/// regardless of which variant is active") observed on the collision channel: a
/// node the LOD pass culls is still an obstacle. The second half is the
/// reason the F20-C.03 composition reads the clip's record rather than the draw
/// reason, and it is the case a design that derived the collider verdict from
/// `DrawVerdict` would get wrong.
#[test]
fn accept_f20_c_04_an_lod_cull_keeps_the_collider_while_a_clip_hide_takes_it_away() {
    let at = generation();
    let mut app = world_with_the_real_loop();
    let hatch = spawn_hatch(&mut app, at);
    play_breakable(&mut app, at);

    // The far distance culls this band; the real LOD pass really rewrites the
    // shared presentation record, which is what makes the rest of the test mean
    // anything.
    set_distance(&mut app, FAR_METRES);
    run_lod_pass(app.world_mut());
    assert_eq!(
        presentation(app.world(), hatch),
        Some(PresentationState::LodCulled),
        "the far distance presents the other band, so the LOD pass really ran"
    );

    // The clip shows the node, so the cull must not remove the collider.
    commit_and_step(&mut app, 0);
    assert_eq!(
        applied_visibility(app.world(), hatch),
        Some(Visibility::Visible)
    );
    assert_eq!(
        presence(app.world(), hatch),
        Some(NodeColliderPresence::Live)
    );
    assert!(!engine_disabled(app.world(), hatch));
    let culled_probe = spawn_probe(&mut app);
    assert!(
        run_ticks(&mut app, 0, APPROACH_TICKS, culled_probe, hatch),
        "an LOD-culled node is still an obstacle: culling is presentation only"
    );
    assert!(position_x(app.world(), culled_probe) < THROUGH_X_M);
    retire(&mut app, culled_probe);

    // The clip's hide reaches collision even while the node is culled, so a
    // render consumer and a collision consumer cannot disagree about whether
    // the clip hid it.
    commit_and_step(&mut app, SYNTHETIC_BREAKABLE_HIDDEN_TICK);
    assert_eq!(
        presentation(app.world(), hatch),
        Some(PresentationState::LodCulled),
        "the LOD pass's own reason is still what the presentation record says"
    );
    assert_eq!(
        presence(app.world(), hatch),
        Some(NodeColliderPresence::HiddenByAnimation),
        "the clip's hide reaches collision whatever the draw reason is"
    );
    assert!(engine_disabled(app.world(), hatch));
    let culled_hidden_probe = spawn_probe(&mut app);
    assert!(
        !run_ticks(
            &mut app,
            SYNTHETIC_BREAKABLE_HIDDEN_TICK,
            APPROACH_TICKS,
            culled_hidden_probe,
            hatch
        ),
        "and a culled node the clip hides is no obstacle either"
    );
    assert!(position_x(app.world(), culled_hidden_probe) > THROUGH_X_M);
}
