//! Acceptance scenarios F20-C.05: the **spawn wiring** for a scene node's
//! collider — the stage that puts a node whose authored
//! [`CollisionRole::Collider`](cs_content::scene::CollisionRole::Collider) under
//! the collision-presence policy, with a real Avian collider on the node's own
//! entity.
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-C` (AC01, non-negotiable behavior 3). Task test prefix:
//! `accept_f20_c_05_`.
//!
//! # The gap this closes
//!
//! F20-C.04 built the collision-side consumer
//! ([`NodeColliderPresence`], [`apply_collider_presence`]) and left its
//! opt-in unwired, on purpose: the record is inserted by a **spawner**, and no
//! production path inserted one, so a clip-hidden node kept colliding. Its
//! finding recorded the cost out loud — "a clip-hidden node whose spawner never
//! inserted the record keeps colliding" — and named the constraint the wiring
//! had to honour: the presence record, the Avian `Collider` and the clip's
//! `NodeAnimatedVisibility` must all live on the **same** entity, because the
//! pass reads all three from one entity.
//!
//! # What is observed, and from where
//!
//! Every physics assertion is read off the **production** channels:
//!
//! * the components the load path actually put on the node entity —
//!   [`NodeColliderPresence::Live`] and a real `Collider` (not a "should have
//!   one"), and *nothing* on a node whose authored role is `None`;
//! * Avian's own collision outcome, through the production contact reporter
//!   ([`ContactReports`]): a probe flying at a shown node is stopped and the
//!   contact is reported, and after the clip's authored hide tick the same probe
//!   **flies through the same volume** with no contact reported against any
//!   node of the live scene. The scene is loaded and the clip played through the
//!   production entries — [`AirframeSceneRequest`] served by
//!   [`process_airframe_scene_request`], [`bind_animated_node`],
//!   [`AnimationSchedulePlugin`] and [`ColliderPresencePlugin`] on the real
//!   fixed loop — so a design that inserted the record somewhere a load never
//!   goes, or attached the collider to a child entity, fails here rather than
//!   passing a component check.
//!
//! The fixture is newly authored synthetic content under
//! `Origin::SyntheticFixture`. The original's node collision flags, its part
//! geometry and the layer a part collider sat on are **unmeasured**, so nothing
//! here is a fidelity claim; F20-D keeps the original-family validation gate.
//!
//! [`NodeColliderPresence`]: cs_app::physics::NodeColliderPresence
//! [`apply_collider_presence`]: cs_app::physics::apply_collider_presence

use std::sync::Arc;
use std::time::Duration;

use avian3d::prelude::{Collider, ColliderDisabled, Gravity, Position};
use bevy::{
    ecs::schedule::{IntoScheduleConfigs, Schedule},
    ecs::world::World,
    prelude::{App, Entity},
    time::{Real, Time, TimeUpdateStrategy},
};
use cs_app::airframe_visual::AirframeVisual;
use cs_app::animation::{
    AnimationInstance, AnimationPlayback, AnimationSchedulePlugin, CommittedSessionTick,
    NodeAnimatedVisibility, bind_animated_node,
};
use cs_app::asset_stack::headless_app;
use cs_app::physics::{
    BASELINE_FIXED_HZ, BodyMode, BodySpec, ColliderPresencePlugin, ContactReports,
    NodeColliderPresence, PhysicsAdapterPlugin, PhysicsBodiesPlugin, apply_collider_presence,
    spawn_body,
};
use cs_app::scene::{
    AirframeDamageState, AirframeSceneLog, AirframeSceneRequest, LiveAirframeScene, LodDistance,
    NodeCollisionError, NodeCollisionShape, PartBinding, SceneCollisionGeometry, SceneEvent,
    SceneGeneration, SceneNodeBinding, apply_airframe_damage, process_airframe_scene_request,
    select_lod_presentation,
};
use cs_content::animation::{
    SYNTHETIC_BREAKABLE_HIDDEN_TICK, SYNTHETIC_BREAKABLE_NODE, SYNTHETIC_BREAKABLE_SHOWN_TICK,
    declared_synthetic_breakable_clip,
};
use cs_content::coordinates::SourceAdapter;
use cs_content::scene::{
    BindingMap, CollisionRole, ParsedNode, ParsedNodeKind, SceneGraph, SceneNodeId, SemanticBinding,
};
use cs_sim::animated_object::Visibility;
use cs_sim::collision::{CollisionLayer, ShapeClass};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;
use cs_types::space::Meters;

// -------------------------------------------------------------- constants ---

/// The session the playback stamps its events with.
const SESSION: u64 = 71;
/// The container the fixture's nodes live in. The node key is the container
/// key plus the authored name-path, so choosing `synthetic.plane` is what makes
/// the root's id exactly [`SYNTHETIC_BREAKABLE_NODE`] — the identity the
/// declared breakable clip drives.
const CONTAINER: &str = "synthetic.plane";
/// The non-colliding child, whose authored role is `CollisionRole::None`.
const PANEL_NODE: &str = "synthetic.plane.hatch.panel";
/// The child whose collision role the evidence left an explicit unknown.
const SENSOR_NODE: &str = "synthetic.plane.hatch.sensor";

/// Half the declared box on every axis, in metres: a 1 m cube at the origin,
/// which is where every fixture node sits.
const NODE_HALF_M: [f64; 3] = [0.5; 3];

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

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("valid content id")
}

/// The stable id of a fixture node: `scene_node/<container>.<path>`.
fn node_id(path: &str) -> SceneNodeId {
    SceneNodeId::from_content_id(cid(ContentKind::SceneNode, path)).expect("scene node id")
}

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

fn instance(value: u32) -> AnimationInstance {
    AnimationInstance::new(value).expect("a nonzero instance identity")
}

/// The declared synthetic left-handed-centimeters-degrees adapter from F16-A's
/// registry, the same one F11's own fixtures convert through.
fn fixture_adapter() -> SourceAdapter {
    SourceAdapter::declared()
        .into_iter()
        .find(|adapter| adapter.source().label() == "fixture.left-handed-z-up-centimeters-degrees")
        .expect("the F16-A registry declares the left-handed centimeters fixture")
}

fn designed(id: &str) -> Provenance {
    Provenance::designed(ClaimId::new(id).expect("test claim id is valid"))
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, designed("f20c05.test.rule")))
}

fn unmeasured<T>(id: &str, reason: &str) -> Resolved<T> {
    Resolved::unknown(ClaimId::new(id).expect("test claim id is valid"), reason)
        .expect("the unknown carries a reason")
}

fn rule(
    path: &str,
    role: Resolved<cs_content::scene::PartRole>,
    collision: Resolved<CollisionRole>,
) -> SemanticBinding {
    SemanticBinding {
        path: path.to_owned(),
        role,
        collision,
        animation: Vec::new(),
        provenance: designed("f20c05.test.rule"),
    }
}

fn object(index: u32, name: &str, parent: u32) -> ParsedNode {
    let mut node = ParsedNode::new(index, name, ParsedNodeKind::Object3d);
    node.parent = Some(parent);
    node
}

/// The fixture hierarchy: the breakable hatch (the root, and the node the
/// declared clip drives) with two children under it — a control surface whose
/// authored collision role is `None` and a camera anchor whose collision role
/// is an explicit **unknown**.
///
/// Both children sit at the origin, exactly where the hatch does, so the second
/// probe of the hide phase flies through the volume of all three: a collider
/// wrongly attached to either child would be in the lane and would be reported.
fn fixture_nodes() -> Vec<ParsedNode> {
    let mut hatch = ParsedNode::new(0, "hatch", ParsedNodeKind::Object3d);
    hatch.children = vec![1, 2];
    vec![hatch, object(1, "panel", 0), object(2, "sensor", 0)]
}

/// The binding table: the hatch collides, the panel does not, and the sensor's
/// collision role was never evidenced.
fn fixture_bindings() -> BindingMap {
    BindingMap::new(vec![
        rule(
            "hatch",
            known(cs_content::scene::PartRole::DamageZone),
            known(CollisionRole::Collider),
        ),
        rule(
            "hatch.panel",
            known(cs_content::scene::PartRole::ControlSurface),
            known(CollisionRole::None),
        ),
        rule(
            "hatch.sensor",
            known(cs_content::scene::PartRole::CameraAnchor),
            unmeasured(
                "f20c05.test.sensor-collision-unmeasured",
                "the fixture's sensor collision role was never evidenced",
            ),
        ),
    ])
    .expect("the fixture rules name distinct paths")
}

fn fixture_graph() -> Arc<SceneGraph> {
    Arc::new(
        SceneGraph::build(
            &cid(ContentKind::InstallFile, CONTAINER),
            &fixture_nodes(),
            &fixture_adapter(),
            &fixture_bindings(),
        )
        .expect("the fixture converts"),
    )
}

fn fixture_visual() -> AirframeVisual {
    AirframeVisual::new(
        cid(ContentKind::Airframe, "alpha"),
        cid(ContentKind::InstallFile, CONTAINER),
        node_id(SYNTHETIC_BREAKABLE_NODE),
    )
    .expect("the fixture root reference is well formed")
}

fn box_shape() -> NodeCollisionShape {
    NodeCollisionShape::cuboid(NODE_HALF_M).expect("the declared box bounds a solid")
}

/// The declared collision geometry of the load: the hatch's box, and **the
/// panel's box too**.
///
/// The panel is the load's proof that the *authored role* decides, not the
/// availability of a shape: its geometry is declared, so a wiring that spawned
/// a collider for any declared node would put one on the panel and the hidden
/// probe would be stopped by it. The sensor declares nothing and its role is
/// unknown, so it is never consulted at all.
fn fixture_geometry() -> SceneCollisionGeometry {
    SceneCollisionGeometry::new()
        .declare(node_id(SYNTHETIC_BREAKABLE_NODE), box_shape())
        .expect("one declaration per node")
        .declare(node_id(PANEL_NODE), box_shape())
        .expect("one declaration per node")
}

/// The probe: a dynamic `projectile`-layer body flying at the nodes.
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

/// The world a test drives: the production fixed loop (the real Avian plugin
/// group, the adapter at the declared rate, the bodies plugin, the animation
/// schedule and the collision-presence pass) plus the F11-C scene systems, in
/// the order the crate requires of them: the request first (it publishes the
/// live record the damage pass reads), then the damage markers, then the
/// presentation pass.
fn world_with_the_real_loop() -> (App, Schedule) {
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
    // already produces the full manual delta and one fixed step.
    let startup = app.world().resource::<Time<Real>>().startup();
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .update_with_instant(startup);
    app.insert_resource(AnimationPlayback::new(session(SESSION)));
    app.insert_resource(LodDistance::new(Meters(50.0)).expect("a usable distance"));
    app.insert_resource(AirframeDamageState::new());
    app.finish();
    app.cleanup();
    let mut scene = Schedule::default();
    scene.add_systems(
        (
            process_airframe_scene_request,
            apply_airframe_damage,
            select_lod_presentation,
        )
            .chain(),
    );
    (app, scene)
}

/// Serves one production load request, with the declared geometry the producer
/// inserted beside it, and returns the generation the load stamped.
fn load(app: &mut App, scene: &mut Schedule, geometry: SceneCollisionGeometry) -> SceneGeneration {
    app.world_mut().insert_resource(geometry);
    app.world_mut().insert_resource(AirframeSceneRequest::load(
        fixture_visual(),
        fixture_graph(),
    ));
    scene.run(app.world_mut());
    // `resource` panics when the load published nothing, which is the point:
    // the generation this test binds the clip to is the live record's own.
    app.world().resource::<LiveAirframeScene>().generation()
}

/// The entity a node of the live scene was imported as.
fn node_entity(app: &App, path: &str) -> Entity {
    app.world()
        .resource::<LiveAirframeScene>()
        .entity(&node_id(path))
        .expect("the node is part of the live scene")
}

/// Every entity the live scene owns, in stable-id order.
fn live_entities(app: &App) -> Vec<Entity> {
    app.world()
        .resource::<LiveAirframeScene>()
        .import()
        .entities()
        .map(|(_, entity)| entity)
        .collect()
}

fn presence(world: &World, entity: Entity) -> Option<NodeColliderPresence> {
    world.get::<NodeColliderPresence>(entity).copied()
}

fn has_collider(world: &World, entity: Entity) -> bool {
    world.get::<Collider>(entity).is_some()
}

fn engine_disabled(world: &World, entity: Entity) -> bool {
    world.get::<ColliderDisabled>(entity).is_some()
}

fn applied_visibility(world: &World, entity: Entity) -> Option<Visibility> {
    world
        .get::<NodeAnimatedVisibility>(entity)
        .map(NodeAnimatedVisibility::visibility)
}

fn position_x(world: &World, entity: Entity) -> f32 {
    world
        .get::<Position>(entity)
        .expect("a spawned body carries a Position")
        .0
        .x
}

fn spawn_probe(app: &mut App) -> Entity {
    spawn_body(app.world_mut(), &probe_spec()).expect("a valid dynamic body spec")
}

/// Removes a probe that has served its assertion: two `projectile`-layer bodies
/// are a designed contact pair, so a spent probe left in the lane would make
/// the next phase a contact between two probes.
fn retire(app: &mut App, probe: Entity) {
    app.world_mut().entity_mut(probe).despawn();
}

/// Commits one session tick and runs one real fixed step, which is what a
/// session driver plus `App::update` do.
fn commit_and_step(app: &mut App, tick: u64) {
    app.world_mut()
        .insert_resource(CommittedSessionTick::new(Tick(tick)));
    app.update();
}

/// Whether the production contact reporter reported a contact between `probe`
/// and **any** entity of the live scene on the tick that just ran.
///
/// Every scene entity is checked, not only the one under test, so a collider
/// wrongly attached to a node the test is not looking at would still be caught.
fn contact_with_the_scene(app: &App, probe: Entity) -> bool {
    let nodes = live_entities(app);
    app.world()
        .resource::<ContactReports>()
        .reports()
        .iter()
        .any(|report| {
            report.bodies.contains(&probe) && nodes.iter().any(|node| report.bodies.contains(node))
        })
}

/// Holds the committed session tick and runs `ticks` fixed steps; reports
/// whether the probe ever reached a node of the live scene during them.
fn run_ticks(app: &mut App, session_tick: u64, ticks: u64, probe: Entity) -> bool {
    let mut contacted = false;
    for _ in 0..ticks {
        commit_and_step(app, session_tick);
        contacted |= contact_with_the_scene(app, probe);
    }
    contacted
}

/// The scene load's log events of one kind, as a count.
fn count_events(app: &App, matches: impl Fn(&SceneEvent) -> bool) -> usize {
    let Some(log) = app.world().get_resource::<AirframeSceneLog>() else {
        return 0;
    };
    log.events().iter().filter(|event| matches(event)).count()
}

// ------------------------------------------------- 1. the spawn wiring ---

/// The minimum scenario, end to end: a node the content authored as a collider
/// is loaded through the production request and comes out of the load carrying
/// the collision-presence record **and** a real Avian collider, and a node
/// authored `None` — or left unknown — comes out with neither.
#[test]
fn accept_f20_c_05_a_collider_node_is_loaded_with_its_presence_record_and_a_real_collider() {
    let (mut app, mut scene) = world_with_the_real_loop();
    let at = load(&mut app, &mut scene, fixture_geometry());

    let hatch = node_entity(&app, SYNTHETIC_BREAKABLE_NODE);
    let panel = node_entity(&app, PANEL_NODE);
    let sensor = node_entity(&app, SENSOR_NODE);

    // The collider node: both records, on the node's own entity, which is also
    // the entity the clip will drive (`bind_animated_node` below binds that same
    // entity, so the same-entity constraint the presence pass needs holds).
    assert_eq!(
        presence(app.world(), hatch),
        Some(NodeColliderPresence::Live),
        "a node authored CollisionRole::Collider is opted into the collision policy"
    );
    assert!(
        has_collider(app.world(), hatch),
        "and the load attached a real Avian collider to it"
    );
    assert!(
        !engine_disabled(app.world(), hatch),
        "which is in the simulation: the engine holds no disable marker"
    );

    // The authored collision role is what the record follows, and the load read
    // it off the bound `PartBinding` rather than off the declared geometry —
    // which the panel also has.
    assert_eq!(
        app.world()
            .get::<PartBinding>(panel)
            .expect("the panel is a bound socket")
            .collision(),
        &known(CollisionRole::None),
        "the panel's authored collision role is None"
    );
    assert_eq!(presence(app.world(), panel), None);
    assert!(
        !has_collider(app.world(), panel),
        "a node authored CollisionRole::None gets no collider even when its geometry is declared"
    );

    // An explicit unknown is not read as a collider: the node is still bound
    // (its gameplay role was evidenced) and it gets neither record nor collider.
    assert_eq!(
        app.world()
            .get::<PartBinding>(sensor)
            .expect("the sensor is a bound socket: only its collision role is unknown")
            .collision(),
        &unmeasured::<CollisionRole>(
            "f20c05.test.sensor-collision-unmeasured",
            "the fixture's sensor collision role was never evidenced",
        ),
        "the sensor's collision role is still the explicit unknown the evidence left"
    );
    assert_eq!(presence(app.world(), sensor), None);
    assert!(!has_collider(app.world(), sensor));
    assert_eq!(
        count_events(&app, |event| matches!(
            event,
            SceneEvent::Loaded { uncollidable, .. } if uncollidable.is_empty()
        )),
        1,
        "and the load reported no collision gap, because every collider-role node in the \
         fixture had declared geometry"
    );

    // The collider is on the node's **own** entity, which is the collider-on-body
    // rule: Avian resolves a swept body through `Query<(&Collider, &ColliderOf)>`,
    // so a collider on a child of the body is invisible to a swept sweep.
    let body = app
        .world()
        .get::<avian3d::prelude::RigidBody>(hatch)
        .expect("the collider node is a body, not a bare collider");
    assert!(
        body.is_static(),
        "the node's body is static: nothing may move an airframe's collision yet"
    );
    assert_eq!(
        app.world()
            .get::<avian3d::prelude::ColliderOf>(hatch)
            .map(|of| of.body),
        Some(hatch),
        "and the collider is bound to that same entity, not to a child of it"
    );
    assert_eq!(
        app.world()
            .get::<cs_app::physics::BodyLayer>(hatch)
            .map(|layer| layer.0),
        Some(CollisionLayer::Aircraft),
        "on the declared layer for an airframe part"
    );

    // The node's composed pose is the collider's pose: the hatch sits at the
    // origin and its declared box is centred there.
    let position = app
        .world()
        .get::<Position>(hatch)
        .expect("the node body carries a Position");
    assert_eq!(position.0, bevy::math::Vec3::ZERO);
    let aabb = app
        .world()
        .get::<avian3d::prelude::ColliderAabb>(hatch)
        .expect("the collider has a broad-phase box");
    let half = (aabb.max.x - aabb.min.x) / 2.0;
    assert!(
        (half - NODE_HALF_M[0] as f32).abs() < 0.05,
        "the broad-phase box is the declared 1 m box plus the engine's margin (half extent {half})"
    );

    // Nothing else in the world was given a presence record: the policy is
    // opt-in and only the authored collider node took it.
    let records: Vec<Entity> = {
        let mut query = app
            .world_mut()
            .query_filtered::<Entity, bevy::ecs::query::With<NodeColliderPresence>>();
        query.iter(app.world()).collect()
    };
    assert_eq!(records, vec![hatch]);

    // The generation the load stamped is the one the node's binding carries, so
    // the clip can be bound to the very same entity below.
    assert_eq!(
        app.world()
            .get::<SceneNodeBinding>(hatch)
            .map(|binding| binding.generation),
        Some(at)
    );
}

// ------------------------------- 2. the wired node on the collision channel ---

/// The spawn-wiring version of the observation F20-C.04 already makes for a
/// body spawned directly: through the production contact reporter, a probe
/// flying at the loaded node is stopped while the clip shows it and **flies
/// through** the same volume once the clip's authored hide tick has run.
#[test]
fn accept_f20_c_05_a_clip_hidden_loaded_node_is_no_longer_an_obstacle() {
    let (mut app, mut scene) = world_with_the_real_loop();
    let at = load(&mut app, &mut scene, fixture_geometry());
    let hatch = node_entity(&app, SYNTHETIC_BREAKABLE_NODE);

    // The clip, bound to the loaded node through the production producer the
    // scene spawn path uses.
    bind_animated_node(
        app.world_mut(),
        &declared_synthetic_breakable_clip(),
        hatch,
        &cid(ContentKind::SceneNode, SYNTHETIC_BREAKABLE_NODE),
        instance(1),
        at,
        Tick(0),
    )
    .expect("the declared clip drives the loaded node");

    // Tick 0 shows the node, so the authored collider is live.
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
    assert!(!engine_disabled(app.world(), hatch));
    let shown_probe = spawn_probe(&mut app);
    assert!(
        run_ticks(&mut app, 0, APPROACH_TICKS, shown_probe),
        "a loaded node the clip does not hide stops a body: the contact is reported"
    );
    assert!(
        position_x(app.world(), shown_probe) < THROUGH_X_M,
        "and the probe was stopped short of the node's far face (x = {})",
        position_x(app.world(), shown_probe)
    );
    retire(&mut app, shown_probe);

    // The authored hide tick, through the wired fixed-tick entry: the
    // collision-presence pass runs after the animation advance in the same fixed
    // step, so the engine marker is in place before the next integration.
    commit_and_step(&mut app, SYNTHETIC_BREAKABLE_HIDDEN_TICK);
    assert_eq!(
        applied_visibility(app.world(), hatch),
        Some(Visibility::Hidden),
        "the clip's hide key applied to the loaded node"
    );
    assert_eq!(
        presence(app.world(), hatch),
        Some(NodeColliderPresence::HiddenByAnimation),
        "the collision side records the clip's hide as its own decision"
    );
    assert!(
        engine_disabled(app.world(), hatch),
        "and the engine's own marker is what leaves the broad phase"
    );

    // A second probe, fired at the now-hidden node. Holding the committed tick
    // keeps the clip from advancing, so the node stays hidden for every tick of
    // the approach — and the lane holds the whole fixture hierarchy, so this is
    // also the observation that the `None` and unknown nodes contributed no
    // collider of their own.
    let hidden_probe = spawn_probe(&mut app);
    assert!(
        !run_ticks(
            &mut app,
            SYNTHETIC_BREAKABLE_HIDDEN_TICK,
            APPROACH_TICKS,
            hidden_probe
        ),
        "a clip-hidden loaded node is not an obstacle: no contact with any node of the scene"
    );
    assert!(
        position_x(app.world(), hidden_probe) > THROUGH_X_M,
        "and the probe flew through the volume (x = {}) rather than being stopped",
        position_x(app.world(), hidden_probe)
    );
    retire(&mut app, hidden_probe);

    // The show tick puts the collider back, and a third probe is stopped again.
    commit_and_step(&mut app, SYNTHETIC_BREAKABLE_SHOWN_TICK);
    assert_eq!(
        presence(app.world(), hatch),
        Some(NodeColliderPresence::Live),
        "the clip stops deciding collision the moment it stops hiding"
    );
    assert!(!engine_disabled(app.world(), hatch));
    let restored_probe = spawn_probe(&mut app);
    assert!(
        run_ticks(
            &mut app,
            SYNTHETIC_BREAKABLE_SHOWN_TICK,
            APPROACH_TICKS,
            restored_probe
        ),
        "the restored collider is a real obstacle again"
    );
    assert!(
        position_x(app.world(), restored_probe) < THROUGH_X_M,
        "and the probe is stopped short of it (x = {})",
        position_x(app.world(), restored_probe)
    );
}

// ------------------------------------------- 3. a reload releases the record ---

/// A reload releases the record with the node: nothing of a superseded
/// generation keeps a [`NodeColliderPresence`] or a collider, and the new
/// generation's node is wired from scratch.
#[test]
fn accept_f20_c_05_a_reload_releases_the_presence_record_with_the_node() {
    let (mut app, mut scene) = world_with_the_real_loop();
    let first = load(&mut app, &mut scene, fixture_geometry());
    let old_hatch = node_entity(&app, SYNTHETIC_BREAKABLE_NODE);
    assert_eq!(
        presence(app.world(), old_hatch),
        Some(NodeColliderPresence::Live)
    );
    assert!(has_collider(app.world(), old_hatch));

    let second = load(&mut app, &mut scene, fixture_geometry());
    assert!(
        second > first,
        "a reload consumes a fresh generation ({} then {})",
        first.0,
        second.0
    );
    assert!(
        app.world().get_entity(old_hatch).is_err(),
        "the superseded node is gone, so its record and its collider went with it"
    );

    // No live entity of the old generation carries the record, and the new
    // generation's node carries both again.
    let stale: Vec<(Entity, u64)> = {
        let mut query = app
            .world_mut()
            .query::<(Entity, &SceneNodeBinding, &NodeColliderPresence)>();
        query
            .iter(app.world())
            .map(|(entity, binding, _)| (entity, binding.generation.0))
            .filter(|(_, generation)| *generation != second.0)
            .collect()
    };
    assert!(
        stale.is_empty(),
        "no live entity of a superseded generation keeps a NodeColliderPresence: {stale:?}"
    );
    let new_hatch = node_entity(&app, SYNTHETIC_BREAKABLE_NODE);
    assert_ne!(
        new_hatch, old_hatch,
        "the reload imported a new entity rather than reusing the old one"
    );
    assert_eq!(
        presence(app.world(), new_hatch),
        Some(NodeColliderPresence::Live)
    );
    assert!(has_collider(app.world(), new_hatch));

    // And the new node really is an obstacle, so the release was a rebuild
    // rather than a wiring that stopped working.
    let probe = spawn_probe(&mut app);
    assert!(
        run_ticks(&mut app, 0, APPROACH_TICKS, probe),
        "the reloaded node's collider is in the simulation"
    );
    assert!(position_x(app.world(), probe) < THROUGH_X_M);
}

/// A node authored as a collider whose geometry nobody declared is **reported**
/// and gets no collider — the gap the opt-in used to hide is now visible, and
/// the presence record is still on it so the physics layer counts it rather than
/// losing the node.
#[test]
fn accept_f20_c_05_a_collider_node_without_declared_geometry_is_reported_and_gets_no_collider() {
    let (mut app, mut scene) = world_with_the_real_loop();
    // Only the panel's geometry: the hatch is authored as a collider and nothing
    // declared its.
    let geometry = SceneCollisionGeometry::new()
        .declare(node_id(PANEL_NODE), box_shape())
        .expect("one declaration per node");
    let at = load(&mut app, &mut scene, geometry);

    let hatch = node_entity(&app, SYNTHETIC_BREAKABLE_NODE);
    assert_eq!(
        presence(app.world(), hatch),
        Some(NodeColliderPresence::Live),
        "the opt-in follows the authored role, not the availability of a shape"
    );
    assert!(
        !has_collider(app.world(), hatch),
        "and a shape nobody measured is not invented for it"
    );
    assert_eq!(
        count_events(&app, |event| matches!(
            event,
            SceneEvent::Loaded { generation, uncollidable, .. }
                if *generation == at
                    && uncollidable.as_slice()
                        == [(
                            node_id(SYNTHETIC_BREAKABLE_NODE),
                            NodeCollisionError::UndeclaredGeometry {
                                node: node_id(SYNTHETIC_BREAKABLE_NODE)
                            }
                        )]
        )),
        1,
        "the load reported the node and the reason by name, inside the event that describes \
         what the load did"
    );

    // A clip hiding that node cannot take a collider that was never built, and
    // the pass says so instead of adopting the node: the record still follows
    // the clip, and the pass counts the node as having no engine collider to
    // apply the record to — the designed report for exactly this state.
    bind_animated_node(
        app.world_mut(),
        &declared_synthetic_breakable_clip(),
        hatch,
        &cid(ContentKind::SceneNode, SYNTHETIC_BREAKABLE_NODE),
        instance(1),
        at,
        Tick(0),
    )
    .expect("the declared clip drives the loaded node");
    commit_and_step(&mut app, SYNTHETIC_BREAKABLE_HIDDEN_TICK);
    assert_eq!(
        applied_visibility(app.world(), hatch),
        Some(Visibility::Hidden)
    );
    assert_eq!(
        presence(app.world(), hatch),
        Some(NodeColliderPresence::HiddenByAnimation),
        "the record follows the clip even where there is no collider to take out"
    );
    assert!(
        !has_collider(app.world(), hatch),
        "and no collider is conjured up to make the state mean something"
    );
    let report = apply_collider_presence(app.world_mut());
    assert_eq!(
        report.without_collider, 1,
        "the pass counts the node as a record with nothing to apply it to"
    );
    assert_eq!(
        report.collider_writes, 0,
        "and wrote no engine marker for it"
    );
}

/// The shape declaration itself: a box nobody can build is refused where it is
/// declared, and a node cannot be declared twice.
#[test]
fn accept_f20_c_05_the_geometry_table_refuses_a_degenerate_box_and_a_duplicate_node() {
    assert!(
        NodeCollisionShape::cuboid([0.5, 0.0, 0.5]).is_err(),
        "a zero extent bounds no solid"
    );
    assert!(
        NodeCollisionShape::cuboid([f64::NAN, 0.5, 0.5]).is_err(),
        "and neither does a non-finite one"
    );
    let shape = box_shape();
    assert_eq!(shape.half_extents_m(), NODE_HALF_M);
    assert_eq!(shape.label(), "cuboid");

    let once = SceneCollisionGeometry::new()
        .declare(node_id(SYNTHETIC_BREAKABLE_NODE), shape)
        .expect("one declaration per node");
    assert_eq!(once.len(), 1);
    assert!(!once.is_empty());
    assert_eq!(once.shape(&node_id(SYNTHETIC_BREAKABLE_NODE)), Some(&shape));
    assert_eq!(once.shape(&node_id(PANEL_NODE)), None);
    let again = once.declare(node_id(SYNTHETIC_BREAKABLE_NODE), shape);
    assert!(
        again.is_err(),
        "an ambiguous mapping is refused where it is built, not resolved by whoever reads it first"
    );
    assert!(SceneCollisionGeometry::new().is_empty());
}
