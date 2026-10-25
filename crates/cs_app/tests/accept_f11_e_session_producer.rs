//! Acceptance scenarios F11-E: the session producer of the airframe scene
//! request.
//!
//! Spec: `specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md`;
//! task test prefix `accept_f11_e_`. A session start loads the named
//! airframe's subtree, a retry supersedes the old generation without a hidden
//! old root, a session end releases every entity, and a refused load reports
//! its error without taking the running scene down. The plugin scenario runs
//! the production schedule.
//!
//! Every value is newly authored fixture data, not measured original game data.

use std::sync::Arc;

use bevy::app::App;
use bevy::ecs::world::World;
use bevy::prelude::Entity;
use cs_app::airframe_visual::AirframeVisual;
use cs_app::scene::{
    AirframeSceneLog, AirframeScenePlugin, AirframeSceneRequest, AirframeSessionError,
    LiveAirframeScene, SceneCollisionGeometry, SceneEvent, SceneImportError, SceneNodeBinding,
    end_airframe_scene, start_airframe_scene,
};
use cs_content::coordinates::SourceAdapter;
use cs_content::scene::{BindingMap, ParsedNode, ParsedNodeKind, SceneGraph, SceneNodeId};
use cs_types::content::{ContentId, ContentKind};

const CONTAINER: &str = "synthetic.plane";

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("valid content id")
}

fn node_id(path: &str) -> SceneNodeId {
    SceneNodeId::from_content_id(cid(ContentKind::SceneNode, path)).expect("scene node id")
}

fn graph() -> Arc<SceneGraph> {
    let adapter = SourceAdapter::declared()
        .into_iter()
        .find(|a| a.source().label() == "fixture.left-handed-z-up-centimeters-degrees")
        .expect("the declared fixture adapter");
    Arc::new(
        SceneGraph::build(
            &cid(ContentKind::InstallFile, CONTAINER),
            &[ParsedNode::new(0, "hatch", ParsedNodeKind::Object3d)],
            &adapter,
            &BindingMap::new(Vec::new()).expect("an empty rule table"),
        )
        .expect("the fixture converts"),
    )
}

fn visual(container: &str) -> AirframeVisual {
    AirframeVisual::new(
        cid(ContentKind::Airframe, "alpha"),
        cid(ContentKind::InstallFile, container),
        node_id(&format!("{container}.hatch")),
    )
    .expect("a well formed root reference")
}

fn start(world: &mut World) -> Result<cs_app::scene::SceneGeneration, AirframeSessionError> {
    start_airframe_scene(
        world,
        visual(CONTAINER),
        graph(),
        SceneCollisionGeometry::new(),
    )
}

fn live_entities(world: &World) -> Vec<Entity> {
    world
        .resource::<LiveAirframeScene>()
        .import()
        .entities()
        .map(|(_, e)| e)
        .collect()
}

fn bound_count(world: &mut World) -> usize {
    world.query::<&SceneNodeBinding>().iter(world).count()
}

#[test]
fn accept_f11_e_session_start_loads_the_named_airframe() {
    let mut world = World::new();
    let generation = start(&mut world).expect("the load is served");
    let live = world.resource::<LiveAirframeScene>();
    assert_eq!(live.generation(), generation);
    assert_eq!(live.airframe(), &cid(ContentKind::Airframe, "alpha"));
    assert!(!live.is_empty());
    let len = live.len();
    assert_eq!(bound_count(&mut world), len);
    // The request is one-shot: nothing is left to be served twice.
    assert!(world.get_resource::<AirframeSceneRequest>().is_none());
}

#[test]
fn accept_f11_e_retry_supersedes_the_old_generation() {
    let mut world = World::new();
    let first = start(&mut world).expect("first load");
    let old = live_entities(&world);
    let second = start(&mut world).expect("retry");
    assert!(second > first);
    for entity in old {
        assert!(world.get_entity(entity).is_err(), "no hidden old root");
    }
    let live = world.resource::<LiveAirframeScene>();
    assert_eq!(live.generation(), second);
    assert_eq!(bound_count(&mut world), live_entities(&world).len());
}

#[test]
fn accept_f11_e_session_end_releases_every_entity() {
    let mut world = World::new();
    start(&mut world).expect("load");
    let entities = live_entities(&world);
    assert_eq!(end_airframe_scene(&mut world), entities.len());
    assert!(world.get_resource::<LiveAirframeScene>().is_none());
    assert_eq!(bound_count(&mut world), 0);
    // A repeated end has nothing to release and is not an error.
    assert_eq!(end_airframe_scene(&mut world), 0);
}

#[test]
fn accept_f11_e_refused_load_propagates_and_keeps_the_running_scene() {
    let mut world = World::new();
    let running = start(&mut world).expect("load");
    let entities = live_entities(&world);
    let error = start_airframe_scene(
        &mut world,
        visual("other.container"),
        graph(),
        SceneCollisionGeometry::new(),
    )
    .expect_err("a root in a foreign container is refused");
    let AirframeSessionError::Refused {
        generation,
        error: import_error,
        ..
    } = error
    else {
        panic!("expected a refusal");
    };
    assert!(generation > running, "the refused attempt burned a number");
    assert!(matches!(
        *import_error,
        SceneImportError::ForeignContainer { .. }
    ));
    assert_eq!(world.resource::<LiveAirframeScene>().generation(), running);
    for entity in entities {
        assert!(world.get_entity(entity).is_ok());
    }
    assert!(matches!(
        world.resource::<AirframeSceneLog>().last(),
        Some(SceneEvent::Refused { .. })
    ));
}

#[test]
fn accept_f11_e_plugin_serves_a_request_in_the_schedule() {
    let mut app = App::new();
    app.add_plugins(AirframeScenePlugin);
    app.insert_resource(AirframeSceneRequest::load(visual(CONTAINER), graph()));
    // No viewer distance yet: the load is still served, the LOD pass waits.
    app.update();
    assert!(app.world().get_resource::<LiveAirframeScene>().is_some());
    app.insert_resource(AirframeSceneRequest::unload());
    app.update();
    assert!(app.world().get_resource::<LiveAirframeScene>().is_none());
}
