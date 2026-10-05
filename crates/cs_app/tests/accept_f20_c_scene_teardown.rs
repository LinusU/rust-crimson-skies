//! Acceptance scenarios F20-C: the scene load/despawn path releases what the
//! animation applied before it despawns a superseded scene.
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-C`, non-negotiable behavior 4 (release attachments before
//! despawning parents). Task test prefix: `accept_f20_c_scene_teardown_`.
//!
//! A reload that despawns the superseded scene takes every descendant of the
//! old entities with it, so an animated node attached under an old node would
//! die with a parent it was only animation-attached to and leave an instance
//! and a binding of the old generation behind. The two entries
//! (`release_superseded_instances`, `release_attachments_before_despawn`) must
//! run in the same step as the despawn; each scenario here fails when one of
//! them is removed from [`process_airframe_scene_request`].
//!
//! Every value is newly authored fixture data, not measured original game data.

use std::sync::Arc;

use bevy::ecs::world::World;
use bevy::prelude::{ChildOf, Entity};
use cs_app::airframe_visual::AirframeVisual;
use cs_app::animation::{
    AnimatedNodeBinding, AnimationInstance, AnimationLog, AnimationPlayback, AppliedAttachment,
    NodeAnimatedPose, advance_animation, play_animation,
};
use cs_app::scene::{
    AirframeSceneLog, AirframeSceneRequest, LiveAirframeScene, SceneEvent, SceneNodeBinding,
    process_airframe_scene_request,
};
use cs_content::animation::{
    SYNTHETIC_BREAKABLE_NODE, SYNTHETIC_CARGO_NODE, declared_synthetic_cargo_clip,
};
use cs_content::coordinates::SourceAdapter;
use cs_content::scene::{BindingMap, ParsedNode, ParsedNodeKind, SceneGraph, SceneNodeId};
use cs_sim::animated_object::PosePolicy;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;

/// The container the fixture's nodes live in; with the root named `hatch` its
/// id is exactly [`SYNTHETIC_BREAKABLE_NODE`].
const CONTAINER: &str = "synthetic.plane";

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("valid content id")
}

fn node_id(path: &str) -> SceneNodeId {
    SceneNodeId::from_content_id(cid(ContentKind::SceneNode, path)).expect("scene node id")
}

fn instance(value: u32) -> AnimationInstance {
    AnimationInstance::new(value).expect("a nonzero instance identity")
}

fn fixture_adapter() -> SourceAdapter {
    SourceAdapter::declared()
        .into_iter()
        .find(|adapter| adapter.source().label() == "fixture.left-handed-z-up-centimeters-degrees")
        .expect("the F16-A registry declares the left-handed centimeters fixture")
}

/// One root node, `hatch`, with no semantic rules.
fn fixture_graph() -> Arc<SceneGraph> {
    Arc::new(
        SceneGraph::build(
            &cid(ContentKind::InstallFile, CONTAINER),
            &[ParsedNode::new(0, "hatch", ParsedNodeKind::Object3d)],
            &fixture_adapter(),
            &BindingMap::new(Vec::new()).expect("an empty rule table"),
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

fn load(world: &mut World) {
    world.insert_resource(AirframeSceneRequest::load(
        fixture_visual(),
        fixture_graph(),
    ));
    process_airframe_scene_request(world);
}

fn live_root(world: &World) -> Entity {
    let live = world.resource::<LiveAirframeScene>();
    live.entity(&node_id(SYNTHETIC_BREAKABLE_NODE))
        .expect("the live scene has its root")
}

// ------------------------------------------------------------- scenarios ---

/// Generation N is loaded with an animated cargo node attached under its root
/// and a second node whose link the animation manages without any playing
/// instance; the reload to N+1 despawns the old entities.
#[test]
fn accept_f20_c_scene_teardown_a_reload_releases_instances_and_attachments_before_the_despawn() {
    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(
        SessionId::new(5).expect("a nonzero session generation"),
    ));

    load(&mut world);
    let old_generation = world.resource::<LiveAirframeScene>().generation();
    let old_root = live_root(&world);

    let clip = declared_synthetic_cargo_clip();
    let clip_id = clip.id().clone();
    let cargo_node = cid(ContentKind::SceneNode, SYNTHETIC_CARGO_NODE);

    // The playing cargo: attached under the old root, bound to a generation-N
    // instance.
    let playing = world
        .spawn((
            SceneNodeBinding {
                node: cargo_node.clone(),
                generation: old_generation,
            },
            ChildOf(old_root),
            AppliedAttachment {
                parent: Some(cid(ContentKind::SceneNode, SYNTHETIC_BREAKABLE_NODE)),
                pose: PosePolicy::KeepWorldPose,
            },
            AnimatedNodeBinding {
                clip: clip_id.clone(),
                node: cargo_node.clone(),
                instance: instance(1),
                generation: old_generation,
            },
        ))
        .id();
    play_animation(&mut world, &clip, instance(1), old_generation, Tick(0))
        .expect("the generation-N instance starts");
    advance_animation(&mut world, Tick(1));
    assert!(
        world
            .resource::<AnimationPlayback>()
            .is_playing(&clip_id, instance(1))
    );

    // A node whose link the animation applied earlier and whose instance has
    // long finished: no instance can release it, only the despawn-side walk.
    let orphaned = world
        .spawn((
            ChildOf(old_root),
            AppliedAttachment {
                parent: Some(cid(ContentKind::SceneNode, SYNTHETIC_BREAKABLE_NODE)),
                pose: PosePolicy::KeepWorldPose,
            },
        ))
        .id();

    // Whatever playing the clip published is not the release; start counting
    // from here.
    let _ = world.resource_mut::<AnimationLog>().drain();

    // Reload: generation N+1 supersedes N and the old entities are despawned.
    load(&mut world);
    let new_generation = world.resource::<LiveAirframeScene>().generation();
    assert_ne!(new_generation, old_generation);
    assert!(
        world.get_entity(old_root).is_err(),
        "the old root is despawned"
    );

    // Both animated nodes survive their despawned parent, unlinked.
    for (entity, what) in [(playing, "playing"), (orphaned, "orphaned")] {
        assert!(
            world.get_entity(entity).is_ok(),
            "the {what} animated node must not die with the old parent"
        );
        assert_eq!(
            world.get::<ChildOf>(entity),
            None,
            "the {what} link is released"
        );
    }

    // No stale instance or binding of generation N remains.
    let playback = world.resource::<AnimationPlayback>();
    assert!(
        !playback.is_playing(&clip_id, instance(1)),
        "the stale instance is released"
    );
    assert_eq!(playback.len(), 0);
    assert_eq!(world.get::<AnimatedNodeBinding>(playing), None);
    assert_eq!(world.get::<NodeAnimatedPose>(playing), None);

    // The next tick drives nothing stale and does not trip over the parent.
    advance_animation(&mut world, Tick(2));
    assert_eq!(world.get::<NodeAnimatedPose>(playing), None);

    // The release is on the record: the cargo's detach inherited no velocity
    // (the fixture carries none), and the scene log reports the release.
    let log = world.resource::<AnimationLog>();
    assert_eq!(
        log.attachments().len(),
        1,
        "one release record, for the playing cargo"
    );
    let events = world.resource::<AirframeSceneLog>().events();
    assert!(events.iter().any(|event| matches!(
        event,
        SceneEvent::Released { generation, .. } if *generation == old_generation
    )));
}
