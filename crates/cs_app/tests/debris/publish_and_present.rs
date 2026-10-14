//! Acceptance scenarios F29-C.5: the published debris bindings and the
//! presented wreck.
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-C`, task F29-C.5 "Publish the authored debris bindings and present
//! spawned debris instances". Task test prefix: `accept_f29_c5_`. Shared
//! contract: `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! The two deliverables, each read off production code:
//!
//! * **the bindings are published.** The production airframe scene load
//!   ([`cs_app::scene::process_airframe_scene_request`]) inserts one
//!   [`PartDebrisBinding`] per authored debris part of the import, exactly as
//!   the damage side's graph names it, stamped with the load generation —
//!   `Resolved::Unknown` verbatim, never a guessed object.
//! * **the instance is presented.** [`cs_app::debris::present_debris`] resolves
//!   the spawned instance's source through the content boundary's own
//!   [`DebrisObjects`] table and spawns one [`DebrisWreck`] — placed at the
//!   destroyed part's own pose, on the caller's designed lifetime, as a
//!   dynamic rigid body on the debris collision layer. An unresolvable source
//!   and an unplaceable part are refused by name; a repair or a reload leaves
//!   no wreckage entity.
//!
//! The chain under test is the production one end to end: the declared
//! fixture graph lowered by [`lower_graph`] / [`lower_policy`], the real
//! [`DamageResolver`], [`apply_debris_state`] / [`release_debris`] from
//! F29-C.2, and the scene load that publishes the bindings. Every scene node,
//! damage node and content id here is newly authored synthetic fixture data,
//! never original game data; whether the original authored debris per part,
//! which object it used and how long a wreck lasts are unmeasured (F29
//! "Research boundary") — see
//! `docs/findings/2026-10-14-f29-c5-debris-bindings-and-presentation.md`.
//!
//! **Module, not target.** Every Bevy/Avian-linked integration-test binary
//! costs the CI runner's disk ~100 MB it does not have, so this file is
//! compiled into the F29-C.2 debris binary by
//! `crates/cs_app/tests/accept_f29_c_debris_spawn.rs`.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use avian3d::prelude::{
    Collider, CollisionLayers, Gravity, LinearVelocity, Position, RigidBody, Rotation,
    TransformInterpolation,
};
use bevy::prelude::{Entity, Transform, World};
use bevy::time::TimeUpdateStrategy;
use cs_app::asset_stack::headless_app;
use cs_app::damage::{lower_graph, lower_policy};
use cs_app::debris::{
    DESIGNED_DEBRIS_LIFETIME, DebrisLifetime, DebrisObject, DebrisObjects, DebrisPresentationEvent,
    DebrisPresentationRefusal, DebrisWreck, PartDebrisBinding, SpawnedDebris, apply_debris_state,
    present_debris, release_debris,
};
use cs_app::physics::{BASELINE_FIXED_HZ, BodyLayer};
use cs_app::scene::{
    AirframeSceneRequest, NodeVisualTransform, PartDebrisPublish, SceneEvent, SceneGeneration,
    process_airframe_scene_request,
};
use cs_content::coordinates::SourceAdapter;
use cs_content::damage::{
    AttributionRule, DamageNodeKind as DeclaredNodeKind, DeclaredDamageGraph, DeclaredDamageNode,
    GraphRules, GraphSubjectKind, SystemKind as DeclaredSystemKind,
};
use cs_content::scene::{BindingMap, ParsedNode, ParsedNodeKind, SceneGraph, SceneNodeId};
use cs_sim::collision::CollisionLayer;
use cs_sim::damage::{
    ActorId, DamageChannel, DamageGraph, DamageNode, DamageNodeKey, DamageResolver, HitEvent,
    HitEventId, PartState, SYNTHETIC_ENGINE_INTEGRITY, SYNTHETIC_ENGINE_NODE,
    SYNTHETIC_MOUNT_INTEGRITY, SYNTHETIC_MOUNT_NODE,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;
use cs_types::space::Meters;

const SESSION: u64 = 47;
const PRODUCER: u32 = 1;

// ----------------------------------------------------------------- helpers ---

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

fn engine() -> DamageNodeKey {
    key(SYNTHETIC_ENGINE_NODE)
}

fn mount() -> DamageNodeKey {
    key(SYNTHETIC_MOUNT_NODE)
}

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("test claim id is valid")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim("f29c5.test"))))
}

/// The explicit unknown: the fixture stands in for evidence that could not
/// resolve the value, and nothing may default it.
fn unmeasured<T>(id: &str, reason: &str) -> Resolved<T> {
    Resolved::unknown(claim(id), reason).expect("the unknown carries a reason")
}

/// The stable id of a fixture scene node.
fn node_id(path: &str) -> SceneNodeId {
    SceneNodeId::from_content_id(
        ContentId::from_source(ContentKind::SceneNode, path).expect("test id is valid"),
    )
    .expect("test id names a scene node")
}

/// The stable id of the fixture scene node one damage part corresponds to:
/// the container key is `synthetic.devastator` and the fixture's parts hang
/// under its `main` root.
fn part_node(part: &str) -> SceneNodeId {
    node_id(&format!("synthetic.devastator.main.{part}"))
}

/// The authored debris object a fixture part spawns: synthetic content, named
/// like the part it belongs to.
fn debris_id(part: &str) -> ContentId {
    ContentId::from_source(
        ContentKind::SceneNode,
        &format!("synthetic.devastator.{part}_wreck"),
    )
    .expect("a valid debris object id")
}

/// The declared synthetic left-handed-centimeters-degrees adapter from F16-A's
/// registry, the same one the scene fixture converts through.
fn fixture_adapter() -> SourceAdapter {
    SourceAdapter::declared()
        .into_iter()
        .find(|adapter| adapter.source().label() == "fixture.left-handed-z-up-centimeters-degrees")
        .expect("the F16-A registry declares the left-handed centimeters fixture")
}

/// The F29-C.5 scene fixture: the synthetic Devastator container with a root
/// and one part node per damage-graph part, whose stable ids are exactly the
/// ones the declared damage fixture's scene bindings name, plus a camera node
/// the damage graph says nothing about.
fn part_fixture() -> Vec<ParsedNode> {
    let mut main = ParsedNode::new(0, "main", ParsedNodeKind::World);
    main.children = vec![1, 2, 3, 4, 5];

    let mut engine = ParsedNode::new(1, SYNTHETIC_ENGINE_NODE, ParsedNodeKind::Object3d);
    engine.parent = Some(0);
    engine.transform.translation = [200.0, 0.0, 0.0];

    let mut hull = ParsedNode::new(2, "hull", ParsedNodeKind::Object3d);
    hull.parent = Some(0);

    let mut mount = ParsedNode::new(3, SYNTHETIC_MOUNT_NODE, ParsedNodeKind::Object3d);
    mount.parent = Some(0);

    let mut armor = ParsedNode::new(4, "nose_armor", ParsedNodeKind::Object3d);
    armor.parent = Some(0);

    let mut camera = ParsedNode::new(5, "camera", ParsedNodeKind::Camera);
    camera.parent = Some(0);

    vec![main, engine, hull, mount, armor, camera]
}

/// The converted container the scene load imports: the synthetic Devastator
/// container, no sockets, whose node ids the declared damage fixture binds.
fn fixture_graph() -> SceneGraph {
    SceneGraph::build(
        &ContentId::from_source(ContentKind::InstallFile, "synthetic.devastator")
            .expect("the fixture container id is valid"),
        &part_fixture(),
        &fixture_adapter(),
        &BindingMap::default(),
    )
    .expect("the F29-C.5 fixture converts")
}

fn fixture_visual() -> cs_app::airframe_visual::AirframeVisual {
    cs_app::airframe_visual::AirframeVisual::new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.devastator")
            .expect("the fixture airframe id is valid"),
        ContentId::from_source(ContentKind::InstallFile, "synthetic.devastator")
            .expect("the fixture container id is valid"),
        node_id("synthetic.devastator.main"),
    )
    .expect("the root reference is well formed")
}

/// The declared fixture: the synthetic Devastator damage graph, whose scene
/// bindings name this file's scene fixture's own node ids — the same declared
/// shape (and the same production authoring surface)
/// `cs_content::damage::declared_synthetic_airframe_damage` uses, with the
/// paths this fixture's container spells.
fn declared() -> DeclaredDamageGraph {
    let scene = |part: &str| known(part_node(part));
    let node = |part: &str,
                kind: DeclaredNodeKind,
                integrity: f64,
                lethal: bool,
                disables: Option<DeclaredSystemKind>| DeclaredDamageNode {
        key: key(part),
        kind,
        scene_binding: Some(scene(part)),
        integrity: known(integrity),
        lethal,
        disables,
        guarded_by: None,
        overflow: None,
    };
    DeclaredDamageGraph::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.devastator")
            .expect("the fixture subject id is valid"),
        Origin::SyntheticFixture,
        GraphSubjectKind::Aircraft,
        GraphRules {
            lethal_attribution: known(AttributionRule::FirstLethalHit),
        },
        vec![
            node(
                SYNTHETIC_ENGINE_NODE,
                DeclaredNodeKind::Engine,
                SYNTHETIC_ENGINE_INTEGRITY,
                false,
                Some(DeclaredSystemKind::Propulsion),
            ),
            node(
                SYNTHETIC_MOUNT_NODE,
                DeclaredNodeKind::WeaponMount,
                SYNTHETIC_MOUNT_INTEGRITY,
                false,
                Some(DeclaredSystemKind::Weapon),
            ),
            node(
                "hull",
                DeclaredNodeKind::InternalStructure,
                40.0,
                true,
                None,
            ),
        ],
        Provenance::designed(claim("f29c5.test")),
    )
    .expect("the F29-C.5 declared fixture is valid")
}

/// The production declared → lowered graph the session registers, with every
/// `Resolved` carried through.
fn lowered_graph() -> DamageGraph {
    lower_graph(&declared()).expect("the declared graph lowers")
}

/// The session's resolver, registered the production way.
fn registered() -> DamageResolver {
    let declared = declared();
    let policy = lower_policy(&declared).expect("the declared policy lowers");
    let mut resolver = DamageResolver::new(session(SESSION), PRODUCER);
    resolver
        .register_actor(actor(1), lowered_graph(), policy)
        .expect("the lowered actor registers");
    resolver
}

/// Loads the fixture scene through the production request path with the
/// damage side's debris publish source beside the request.
fn load_with_publish(world: &mut World, objects: BTreeMap<DamageNodeKey, Resolved<ContentId>>) {
    world.insert_resource(PartDebrisPublish::new(actor(1), lowered_graph(), objects));
    world.insert_resource(AirframeSceneRequest::load(
        fixture_visual(),
        Arc::new(fixture_graph()),
    ));
    process_airframe_scene_request(world);
}

/// The entity the fixture scene imported one node as.
fn node_entity(world: &mut World, node: &str) -> Entity {
    let live = world
        .get_resource::<cs_app::scene::LiveAirframeScene>()
        .expect("the load published the live record");
    let entity = live.entity(&part_node(node));
    assert!(entity.is_some(), "the fixture imports the {node} node");
    entity.expect("the node entity is in the import")
}

/// The `PartDebrisBinding` one part entity carries, if any.
fn binding(world: &World, entity: Entity) -> Option<PartDebrisBinding> {
    world.get::<PartDebrisBinding>(entity).cloned()
}

/// Every debris instance in the world, in entity order.
fn instances(world: &World) -> Vec<(Entity, SpawnedDebris)> {
    world
        .iter_entities()
        .filter_map(|entity_ref| {
            let debris = entity_ref.get::<SpawnedDebris>()?;
            Some((entity_ref.id(), debris.clone()))
        })
        .collect()
}

/// Every wreck in the world, in entity order.
fn wrecks(world: &World) -> Vec<(Entity, DebrisWreck)> {
    world
        .iter_entities()
        .filter_map(|entity_ref| {
            let wreck = entity_ref.get::<DebrisWreck>()?;
            Some((entity_ref.id(), wreck.clone()))
        })
        .collect()
}

/// An internal hit that lands on `node` for `damage`, at the resolver's tick.
fn hit(node: &DamageNodeKey, damage: f64) -> HitEvent {
    HitEvent::try_new(
        HitEventId {
            session: session(SESSION),
            tick: Tick(0),
            producer: PRODUCER,
            sequence: 0,
        },
        Some(actor(2)),
        actor(1),
        node.clone(),
        DamageChannel::Internal,
        damage,
    )
    .expect("a finite, non-negative hit")
}

/// Destroys one part outright through the production resolver path.
fn destroy(resolver: &mut DamageResolver, node: &DamageNodeKey, integrity: f64) {
    resolver
        .resolve(Tick(0), &[hit(node, integrity)])
        .expect("the resolver accepts the batch");
    assert_eq!(
        resolver.part_state(&actor(1), node),
        Some(PartState::Destroyed),
        "the fixture hit destroyed the part"
    );
}

/// The content boundary's debris resolution table for one authored object.
fn objects_of(part: &str) -> DebrisObjects {
    DebrisObjects::new()
        .with(DebrisObject::new(debris_id(part), Meters(0.5)).expect("a usable object radius"))
}

/// The last `Loaded` event the scene log recorded, or a precise complaint.
fn last_loaded(world: &World) -> SceneEvent {
    let log = world
        .get_resource::<cs_app::scene::AirframeSceneLog>()
        .expect("the systems reported what they did");
    log.events()
        .iter()
        .rev()
        .find(|event| matches!(event, SceneEvent::Loaded { .. }))
        .cloned()
        .expect("the load reported a Loaded event")
}

// ------------------------------------------- the published bindings ---

/// A load publishes one binding per authored debris part of the import: the
/// engine's resolved object verbatim, the mount's explicit unknown verbatim,
/// the load's generation on every one — and nothing for a part the damage side
/// published no debris for, or a node the graph does not name at all.
#[test]
fn accept_f29_c5_a_load_publishes_the_authored_debris_binding_on_each_part_entity() {
    let mut world = World::new();
    let reason = "the mount's debris object is unmeasured".to_owned();
    load_with_publish(
        &mut world,
        BTreeMap::from([
            (engine(), known(debris_id(SYNTHETIC_ENGINE_NODE))),
            (mount(), unmeasured("f29c5.test.mount-debris", &reason)),
        ]),
    );

    // The engine's part entity carries the authored, resolved binding,
    // stamped with the generation of the load that published it.
    let engine_entity = node_entity(&mut world, SYNTHETIC_ENGINE_NODE);
    let published = binding(&world, engine_entity).expect("the engine part is bound");
    assert_eq!(published.actor, actor(1));
    assert_eq!(published.node, engine());
    assert_eq!(
        published.debris,
        known(debris_id(SYNTHETIC_ENGINE_NODE)),
        "the content boundary's resolution is published verbatim"
    );
    assert_eq!(published.generation, SceneGeneration(1));

    // An explicit unknown is published as the unknown it is, with its claim.
    let mount_entity = node_entity(&mut world, SYNTHETIC_MOUNT_NODE);
    let published = binding(&world, mount_entity).expect("the mount part is bound");
    assert_eq!(published.node, mount());
    assert_eq!(
        published.debris,
        unmeasured::<ContentId>("f29c5.test.mount-debris", &reason),
        "the unknown is a first-class value, never replaced by a stand-in"
    );
    assert_eq!(published.generation, SceneGeneration(1));

    // A part the content boundary says nothing about is not an authored
    // debris part, and a node the damage graph does not name is not a part at
    // all: neither carries a binding, and that is not a gap.
    for node in ["hull", "nose_armor", "camera"] {
        let entity = node_entity(&mut world, node);
        assert!(
            binding(&world, entity).is_none(),
            "the {node} node is not an authored debris part"
        );
    }

    assert_eq!(
        last_loaded(&world),
        SceneEvent::Loaded {
            airframe: fixture_visual().airframe().clone(),
            root: node_id("synthetic.devastator.main"),
            generation: SceneGeneration(1),
            nodes: 6,
            sockets: 0,
            unresolved: vec![],
            uncollidable: vec![],
            unpublished_debris: vec![],
        },
        "every authored binding found its entity; nothing was dropped"
    );
}

/// A part whose scene identity is unresolved is reported by node key instead
/// of being bound by guess, and a node whose binding names another airframe's
/// subtree is neither bound nor reported.
#[test]
fn accept_f29_c5_an_unresolved_scene_binding_is_reported_not_bound() {
    // A graph whose engine binds the fixture's scene node and whose mount's
    // scene binding is an explicit unknown: the mount is an authored debris
    // part that cannot be placed.
    let reason = "the mount's scene binding is unmeasured".to_owned();
    let graph = DamageGraph::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.devastator")
            .expect("a valid airframe id"),
        vec![
            DamageNode::new(
                engine(),
                cs_sim::damage::DamageNodeKind::Engine,
                known(SYNTHETIC_ENGINE_INTEGRITY),
            )
            .with_scene_binding(known(
                part_node(SYNTHETIC_ENGINE_NODE).as_content_id().clone(),
            )),
            DamageNode::new(
                mount(),
                cs_sim::damage::DamageNodeKind::WeaponMount,
                known(SYNTHETIC_MOUNT_INTEGRITY),
            )
            .with_scene_binding(unmeasured("f29c5.test.mount-scene", &reason)),
        ],
    )
    .expect("the graph validates");
    let objects = BTreeMap::from([
        (engine(), known(debris_id(SYNTHETIC_ENGINE_NODE))),
        (mount(), known(debris_id(SYNTHETIC_MOUNT_NODE))),
    ]);

    let mut world = World::new();
    world.insert_resource(PartDebrisPublish::new(actor(1), graph, objects));
    world.insert_resource(AirframeSceneRequest::load(
        fixture_visual(),
        Arc::new(fixture_graph()),
    ));
    process_airframe_scene_request(&mut world);

    let engine_entity = node_entity(&mut world, SYNTHETIC_ENGINE_NODE);
    assert!(
        binding(&world, engine_entity).is_some(),
        "the resolvable part is still bound"
    );
    let mount_entity = node_entity(&mut world, SYNTHETIC_MOUNT_NODE);
    assert!(
        binding(&world, mount_entity).is_none(),
        "the unresolved scene identity binds nothing"
    );
    let SceneEvent::Loaded {
        unpublished_debris, ..
    } = last_loaded(&world)
    else {
        panic!("the load reported a Loaded event");
    };
    assert_eq!(
        unpublished_debris,
        vec![mount()],
        "the gap is named, not dropped: {unpublished_debris:?}"
    );
}

/// The published unknown reaches the F29-C.2 pass intact: destroying the part
/// is refused by name with its claim and spawns nothing — the spawn path
/// published the explicit unknown, and no object is guessed anywhere.
#[test]
fn accept_f29_c5_the_published_unknown_binding_is_refused_by_name_and_spawns_nothing() {
    let mut world = World::new();
    let reason = "the mount's debris object is unmeasured".to_owned();
    load_with_publish(
        &mut world,
        BTreeMap::from([(mount(), unmeasured("f29c5.test.mount-debris", &reason))]),
    );

    let mut resolver = registered();
    destroy(&mut resolver, &mount(), SYNTHETIC_MOUNT_INTEGRITY);
    let outcome = apply_debris_state(&mut world, &resolver, actor(1));
    assert_eq!(outcome.report.spawned, 0, "{outcome:?}");
    assert_eq!(
        outcome.log.last(),
        Some(&cs_app::debris::DamageDebrisEvent::Refused(
            cs_app::debris::DamageDebrisRefusal::UnresolvedDebris {
                actor: actor(1),
                node: mount(),
                claim_id: claim("f29c5.test.mount-debris"),
                reason,
            }
        )),
        "the published unknown is refused by name: {:?}",
        outcome.log
    );
    assert!(
        instances(&world).is_empty(),
        "nothing was guessed into the world"
    );
    assert!(wrecks(&world).is_empty(), "and nothing was presented");
}

// ------------------------------------------- the presented wreck ---

/// An instance is presented as a wreck the engine could one day see: placed
/// at the destroyed part's own pose, carrying the resolved object, the tick
/// and the designed lifetime, and simulated — a dynamic body on the debris
/// layer with the object's collider and no invented velocity. A second pass
/// is a no-op and keeps the same wreck.
#[test]
fn accept_f29_c5_presenting_an_instance_gives_the_wreck_placement_lifetime_and_physics() {
    let mut world = World::new();
    load_with_publish(
        &mut world,
        BTreeMap::from([(engine(), known(debris_id(SYNTHETIC_ENGINE_NODE)))]),
    );
    let engine_entity = node_entity(&mut world, SYNTHETIC_ENGINE_NODE);

    let mut resolver = registered();
    destroy(&mut resolver, &engine(), SYNTHETIC_ENGINE_INTEGRITY);
    let outcome = apply_debris_state(&mut world, &resolver, actor(1));
    assert_eq!(outcome.report.spawned, 1, "{outcome:?}");
    let (instance, spawned) = instances(&world).into_iter().next().expect("one instance");

    let presented = present_debris(
        &mut world,
        &objects_of(SYNTHETIC_ENGINE_NODE),
        Tick(10),
        DebrisLifetime(5),
    );
    assert_eq!(
        presented.report.presented, 1,
        "the instance got its wreck: {presented:?}"
    );
    let (wreck_entity, wreck) = wrecks(&world).into_iter().next().expect("one wreck");
    assert_eq!(wreck.actor, actor(1));
    assert_eq!(wreck.node, engine());
    assert_eq!(wreck.instance, instance);
    assert_eq!(wreck.object, debris_id(SYNTHETIC_ENGINE_NODE));
    assert_eq!(wreck.object, spawned.source);
    assert_eq!(wreck.placed_at, Tick(10));
    assert_eq!(wreck.lifetime, DebrisLifetime(5));
    assert_eq!(wreck.generation, spawned.generation);

    // Placement is the destroyed part's own composed pose — the one visual
    // pose owner — bound the production way: the Bevy `Transform` and Avian's
    // `Position`/`Rotation` pair all carry it (Avian's body init reads those,
    // never a bare `GlobalTransform`).
    let part_pose = world
        .get::<NodeVisualTransform>(engine_entity)
        .expect("the part entity carries its pose")
        .global();
    let (_, rotation, translation) = part_pose.to_scale_rotation_translation();
    assert_eq!(
        world
            .get::<Transform>(wreck_entity)
            .expect("the wreck carries its placement"),
        &Transform::from_translation(translation).with_rotation(rotation),
        "the wreck sits where the wreckage came off"
    );
    assert_eq!(
        world.get::<Position>(wreck_entity),
        Some(&Position(translation)),
        "the physics pose is the part's own"
    );
    assert_eq!(
        world.get::<Rotation>(wreck_entity),
        Some(&Rotation::from(rotation)),
        "the physics rotation is the part's own"
    );

    // Physics: a dynamic body on the declared debris layer, the object's own
    // collider, and no invented velocity.
    assert_eq!(
        world.get::<RigidBody>(wreck_entity),
        Some(&RigidBody::Dynamic)
    );
    let radius = world
        .get::<Collider>(wreck_entity)
        .expect("the wreck carries a collider")
        .shape()
        .as_ball()
        .expect("the object's radius is a sphere")
        .radius;
    assert_eq!(radius, 0.5);
    assert_eq!(
        world.get::<LinearVelocity>(wreck_entity),
        Some(&LinearVelocity::ZERO),
        "the wreck starts at rest and the physics step takes it from there"
    );
    assert_eq!(
        world.get::<TransformInterpolation>(wreck_entity),
        Some(&TransformInterpolation),
        "a simulated body eases its render transform between fixed ticks (F23-C)"
    );
    assert_eq!(
        world.get::<BodyLayer>(wreck_entity),
        Some(&BodyLayer(CollisionLayer::Debris))
    );
    assert_eq!(
        world.get::<CollisionLayers>(wreck_entity),
        Some(&CollisionLayers::from_bits(
            CollisionLayer::Debris.bit().into(),
            CollisionLayer::Debris.designed_partners().bits().into(),
        )),
        "the wreck meets aircraft, projectiles, world geometry and debris"
    );

    // A second pass over unchanged state changes nothing and keeps the wreck.
    let again = present_debris(
        &mut world,
        &objects_of(SYNTHETIC_ENGINE_NODE),
        Tick(11),
        DebrisLifetime(5),
    );
    assert!(
        again.report.is_noop() && again.log.is_empty(),
        "the pass converges: {again:?}"
    );
    let (same, _) = wrecks(&world).into_iter().next().expect("the one wreck");
    assert_eq!(same, wreck_entity, "the same wreck survives");
}

/// What the stage refuses: a source the content boundary did not resolve and
/// a part whose entity carries no pose. Both are named, and neither spawns a
/// wreck or a guessed shape.
#[test]
fn accept_f29_c5_an_unresolvable_source_and_an_unplaced_part_are_refused_by_name() {
    let mut world = World::new();
    load_with_publish(
        &mut world,
        BTreeMap::from([
            (engine(), known(debris_id(SYNTHETIC_ENGINE_NODE))),
            (mount(), known(debris_id(SYNTHETIC_MOUNT_NODE))),
        ]),
    );

    let mut resolver = registered();
    destroy(&mut resolver, &engine(), SYNTHETIC_ENGINE_INTEGRITY);
    destroy(&mut resolver, &mount(), SYNTHETIC_MOUNT_INTEGRITY);
    let spawned = apply_debris_state(&mut world, &resolver, actor(1));
    assert_eq!(spawned.report.spawned, 2, "{spawned:?}");

    // The boundary knows the engine's object only.
    let presented = present_debris(
        &mut world,
        &objects_of(SYNTHETIC_ENGINE_NODE),
        Tick(4),
        DESIGNED_DEBRIS_LIFETIME,
    );
    assert_eq!(presented.report.presented, 1, "{presented:?}");
    assert_eq!(presented.report.refused, 1, "{presented:?}");
    assert_eq!(
        presented.log.last(),
        Some(&DebrisPresentationEvent::Refused(
            DebrisPresentationRefusal::UnresolvedObject {
                actor: actor(1),
                node: mount(),
                source: debris_id(SYNTHETIC_MOUNT_NODE),
            }
        )),
        "the unresolved source is named: {:?}",
        presented.log
    );

    // The mount's part entity carries the binding but no pose (a fixture
    // stand-in for a part whose scene entity was never imported): the wreck
    // is not placed at a guessed position.
    let mount_entity = node_entity(&mut world, SYNTHETIC_MOUNT_NODE);
    world
        .entity_mut(mount_entity)
        .remove::<NodeVisualTransform>();
    let placed = present_debris(
        &mut world,
        &DebrisObjects::new()
            .with(DebrisObject::new(debris_id(SYNTHETIC_ENGINE_NODE), Meters(0.5)).expect("usable"))
            .with(DebrisObject::new(debris_id(SYNTHETIC_MOUNT_NODE), Meters(0.5)).expect("usable")),
        Tick(5),
        DESIGNED_DEBRIS_LIFETIME,
    );
    assert_eq!(placed.report.refused, 1, "{placed:?}");
    assert_eq!(
        placed.log.last(),
        Some(&DebrisPresentationEvent::Refused(
            DebrisPresentationRefusal::UnplacedDebris {
                actor: actor(1),
                node: mount(),
            }
        )),
        "the unplaced part is named: {:?}",
        placed.log
    );
    assert_eq!(
        wrecks(&world).len(),
        1,
        "only the placed wreck exists — the refusals spawned nothing"
    );
}

/// The teardown the task names: a repair and a reload each leave no wreckage
/// entity, and a scene release takes the bindings with it.
#[test]
fn accept_f29_c5_a_repair_and_a_reload_leave_no_wreckage_entity_and_no_dangling_binding() {
    let mut world = World::new();
    load_with_publish(
        &mut world,
        BTreeMap::from([(engine(), known(debris_id(SYNTHETIC_ENGINE_NODE)))]),
    );

    let mut resolver = registered();
    destroy(&mut resolver, &engine(), SYNTHETIC_ENGINE_INTEGRITY);
    let outcome = apply_debris_state(&mut world, &resolver, actor(1));
    assert_eq!(outcome.report.spawned, 1, "{outcome:?}");
    let presented = present_debris(
        &mut world,
        &objects_of(SYNTHETIC_ENGINE_NODE),
        Tick(3),
        DESIGNED_DEBRIS_LIFETIME,
    );
    assert_eq!(presented.report.presented, 1, "{presented:?}");
    assert_eq!(wrecks(&world).len(), 1);

    // Repair: the authority no longer calls the part destroyed, so the
    // instance leaves and the wreck leaves with it.
    let repaired = registered();
    let outcome = apply_debris_state(&mut world, &repaired, actor(1));
    assert_eq!(outcome.report.despawned, 1, "{outcome:?}");
    let torn = present_debris(
        &mut world,
        &objects_of(SYNTHETIC_ENGINE_NODE),
        Tick(4),
        DESIGNED_DEBRIS_LIFETIME,
    );
    assert_eq!(torn.report.released, 1, "{torn:?}");
    assert!(
        wrecks(&world).is_empty(),
        "the repair left no wreckage entity"
    );
    assert!(instances(&world).is_empty());

    // Reload: `release_debris` is the reload/swap teardown entry, and the
    // pass converges the wreck away with the instance it presented.
    destroy(&mut resolver, &engine(), SYNTHETIC_ENGINE_INTEGRITY);
    let respawned = apply_debris_state(&mut world, &resolver, actor(1));
    assert_eq!(respawned.report.spawned, 1, "{respawned:?}");
    let presented = present_debris(
        &mut world,
        &objects_of(SYNTHETIC_ENGINE_NODE),
        Tick(5),
        DESIGNED_DEBRIS_LIFETIME,
    );
    assert_eq!(presented.report.presented, 1, "{presented:?}");
    assert_eq!(wrecks(&world).len(), 1);

    assert_eq!(
        release_debris(&mut world, actor(1)),
        1,
        "the release reports it"
    );
    let torn = present_debris(
        &mut world,
        &objects_of(SYNTHETIC_ENGINE_NODE),
        Tick(6),
        DESIGNED_DEBRIS_LIFETIME,
    );
    assert_eq!(torn.report.released, 1, "{torn:?}");
    assert!(
        wrecks(&world).is_empty(),
        "the reload left no wreckage entity"
    );
    assert!(instances(&world).is_empty());

    // Scene release: the part entities and their published bindings go with
    // the unload, so no dangling binding survives the swap.
    world.insert_resource(AirframeSceneRequest::unload());
    process_airframe_scene_request(&mut world);
    assert!(
        world
            .iter_entities()
            .all(|entity| entity.get::<PartDebrisBinding>().is_none()),
        "the unload took every published binding with it"
    );
    let after = present_debris(
        &mut world,
        &objects_of(SYNTHETIC_ENGINE_NODE),
        Tick(7),
        DESIGNED_DEBRIS_LIFETIME,
    );
    assert!(
        after.report.is_noop() && after.log.is_empty(),
        "a released scene leaves nothing to present: {after:?}"
    );
}

/// The designed lifetime bounds the wreck: it is presented through
/// `placed_at + lifetime - 1` and expires at the first pass at or after
/// `placed_at + lifetime`, whatever the authority says.
#[test]
fn accept_f29_c5_a_wreck_expires_at_the_end_of_its_designed_lifetime() {
    let mut world = World::new();
    load_with_publish(
        &mut world,
        BTreeMap::from([(engine(), known(debris_id(SYNTHETIC_ENGINE_NODE)))]),
    );

    let mut resolver = registered();
    destroy(&mut resolver, &engine(), SYNTHETIC_ENGINE_INTEGRITY);
    let outcome = apply_debris_state(&mut world, &resolver, actor(1));
    assert_eq!(outcome.report.spawned, 1, "{outcome:?}");
    let objects = objects_of(SYNTHETIC_ENGINE_NODE);
    let lifetime = DebrisLifetime(3);
    let placed = present_debris(&mut world, &objects, Tick(10), lifetime);
    assert_eq!(placed.report.presented, 1, "{placed:?}");
    let (wreck_entity, _) = wrecks(&world).into_iter().next().expect("one wreck");

    // One tick before the end: still there.
    let kept = present_debris(&mut world, &objects, Tick(12), lifetime);
    assert!(
        kept.report.is_noop() && kept.log.is_empty(),
        "the wreck is inside its lifetime: {kept:?}"
    );
    assert_eq!(wrecks(&world)[0].0, wreck_entity);

    // At the end: it goes, named as expired rather than silently dropped.
    let expired = present_debris(&mut world, &objects, Tick(13), lifetime);
    assert_eq!(expired.report.expired, 1, "{expired:?}");
    assert!(
        matches!(
            expired.log.last(),
            Some(DebrisPresentationEvent::Expired { .. })
        ),
        "the expiry is recorded: {:?}",
        expired.log
    );
    assert!(wrecks(&world).is_empty());
    assert_eq!(
        instances(&world).len(),
        1,
        "the authority still calls the part destroyed; only the wreck went"
    );

    // The next pass presents a fresh wreck at the part's current pose: the
    // lifetime bounds one placement, and the authority — not the stage's own
    // memory — decides whether a wreck exists at all.
    let replaced = present_debris(&mut world, &objects, Tick(14), lifetime);
    assert_eq!(replaced.report.presented, 1, "{replaced:?}");
    let (_, wreck) = wrecks(&world).into_iter().next().expect("one wreck");
    assert_eq!(wreck.placed_at, Tick(14));
}

/// The placement is real physics, not a decorative component: through the
/// real Avian init and transform-sync loop the wreck simulates at the
/// destroyed part's pose — a body spawned with only a bare `GlobalTransform`
/// would initialize at the world origin (the F29-C.5 review's measurement,
/// which is why the wreck binds `Transform` + `Position` + `Rotation`).
#[test]
fn accept_f29_c5_the_wreck_simulates_at_the_parts_pose() {
    let mut app = headless_app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / BASELINE_FIXED_HZ as f64,
    )));
    app.insert_resource(Gravity::ZERO);

    let world = app.world_mut();
    load_with_publish(
        world,
        BTreeMap::from([(engine(), known(debris_id(SYNTHETIC_ENGINE_NODE)))]),
    );
    let engine_entity = node_entity(world, SYNTHETIC_ENGINE_NODE);
    let part_pose = world
        .get::<NodeVisualTransform>(engine_entity)
        .expect("the part entity carries its pose")
        .global();

    let mut resolver = registered();
    destroy(&mut resolver, &engine(), SYNTHETIC_ENGINE_INTEGRITY);
    let outcome = apply_debris_state(world, &resolver, actor(1));
    assert_eq!(outcome.report.spawned, 1, "{outcome:?}");
    let presented = present_debris(
        world,
        &objects_of(SYNTHETIC_ENGINE_NODE),
        Tick(0),
        DESIGNED_DEBRIS_LIFETIME,
    );
    assert_eq!(presented.report.presented, 1, "{presented:?}");
    let wreck = wrecks(world)[0].0;

    // One fixed tick of the real physics loop: Avian initializes the body and
    // the transform sync runs on the spawned wreck.
    app.update();

    let position = app
        .world()
        .get::<Position>(wreck)
        .expect("the wreck is a simulated body");
    let expected = part_pose.translation();
    assert!(
        (position.0 - expected).length() < 1e-4,
        "the wreck simulated at the part's pose {expected:?}, not the origin: {position:?}"
    );
}

/// The designed default lifetime is the stage's stated value, not a hidden
/// one: a wreck presented under it expires at exactly that many ticks.
#[test]
fn accept_f29_c5_the_designed_default_lifetime_bounds_a_wreck() {
    assert_eq!(DESIGNED_DEBRIS_LIFETIME.ticks(), 600);
    let wreck = DebrisWreck {
        actor: actor(1),
        node: engine(),
        instance: Entity::from_raw_u32(1).expect("a placeholder entity"),
        object: debris_id(SYNTHETIC_ENGINE_NODE),
        placed_at: Tick(7),
        lifetime: DESIGNED_DEBRIS_LIFETIME,
        generation: SceneGeneration(1),
    };
    assert!(
        !wreck.expired(Tick(606)),
        "the last presented tick is age 599"
    );
    assert!(wreck.expired(Tick(607)), "the expiry tick is age 600");
}
