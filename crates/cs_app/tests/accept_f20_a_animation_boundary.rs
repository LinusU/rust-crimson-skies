//! Acceptance scenario F20-A across the application boundary: a declared
//! clip lowers into the runtime record and the door opens coherently.
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-A`. Task test prefix: `accept_f20_a_`. Minimum scenario
//! (AC01): "A door opening at a fixed tick changes collider and mesh state
//! coherently."
//!
//! These tests drive the production path end to end at this stage's scope:
//! `cs_content::animation::declared_synthetic_door_clip` (the declared IR)
//! → [`cs_app::animation::lower::lower_clip`] (the conversion boundary) →
//! `cs_sim::animated_object::AnimatedObject` (the fixed-tick evaluator).
//! They are discriminating because removing any of the three layers fails
//! to compile, and because mesh/collider divergence inside the evaluator
//! fails the equality assertion.
//!
//! Every value is newly authored fixture data, not measured original game
//! data.

use cs_app::animation::AnimatedNodeBinding;
use cs_app::animation::AnimationInstance;
use cs_app::animation::lower::lower_clip;
use cs_app::animation::presentation::interpolated_pose;
use cs_app::scene::SceneGeneration;
use cs_content::animation::{MarkerEffect, declared_synthetic_door_clip};
use cs_sim::animated_object::{
    AnimatedObject, AnimationError, LoopMode, MarkerEffect as RuntimeMarkerEffect, PoseSample,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;

fn content_id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("valid content id")
}

/// The shared nonzero session generation the evaluator stamps into event ids.
fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

/// The full path: the declared door clip lowers to a runtime clip, plays to
/// its open tick, and the mesh and collider states change together while
/// the gameplay marker fires exactly once.
#[test]
fn accept_f20_a_door_clip_lowers_and_opens_coherently() {
    let declared = declared_synthetic_door_clip();
    let clip = lower_clip(&declared).expect("a validated declared clip lowers");

    // The lowering preserves identity, timing and loop semantics.
    assert_eq!(clip.id(), declared.id());
    assert_eq!(clip.duration_ticks(), declared.duration_ticks());
    assert_eq!(clip.loop_mode(), LoopMode::Once);
    assert_eq!(clip.channels().len(), 1);
    assert_eq!(clip.markers().len(), 1);
    assert_eq!(
        declared.markers()[0].effect.clone().known(),
        Some(MarkerEffect::Gameplay {
            cue: "synthetic.hangar.door_opened".to_owned()
        }),
        "the declared marker is a gameplay cue"
    );
    assert_eq!(
        clip.markers()[0].effect.clone().known(),
        Some(RuntimeMarkerEffect::Gameplay {
            cue: "synthetic.hangar.door_opened".to_owned()
        }),
        "lowering keeps the marker effect known and unchanged"
    );

    let mut object = AnimatedObject::new(clip, session(1), 1);
    let door = content_id(ContentKind::SceneNode, "synthetic.hangar.door");

    // Bound entity record: the node's entity tracks clip + instance +
    // generation.
    let binding = AnimatedNodeBinding {
        clip: content_id(ContentKind::AnimationTrack, "synthetic.door_open"),
        node: door.clone(),
        instance: AnimationInstance::new(1).expect("a nonzero instance identity"),
        generation: SceneGeneration::default().next(),
    };
    assert_eq!(binding.generation, SceneGeneration(1));
    assert_eq!(binding.instance.get(), 1);

    let closed = object.states()[&door].pose().copied();
    assert_eq!(closed, Some(PoseSample::IDENTITY));

    let outcome = object.advance_to(10, Tick(10)).expect("the open tick");
    assert_eq!(outcome.events.len(), 1);
    assert_eq!(outcome.events[0].marker, "door_opened");

    let state = &object.states()[&door];
    let opened = state.mesh_pose().expect("a pose").to_owned();
    assert_ne!(opened, PoseSample::IDENTITY, "the door opened");
    assert_eq!(
        state.collider_pose(),
        Some(&opened),
        "the collider changed coherently with the mesh"
    );

    // Presentation interpolation between the committed poses is pose-only:
    // it emits nothing and changes no evaluator state.
    let before = object.states().clone();
    let halfway =
        interpolated_pose(&PoseSample::IDENTITY, &opened, 0.5).expect("an in-range alpha blends");
    assert_ne!(halfway, PoseSample::IDENTITY);
    assert_ne!(halfway, opened);
    assert_eq!(
        interpolated_pose(&PoseSample::IDENTITY, &opened, 0.0).expect("alpha 0"),
        PoseSample::IDENTITY,
        "alpha 0 is exactly the committed previous pose"
    );
    assert_eq!(
        interpolated_pose(&PoseSample::IDENTITY, &opened, 1.0).expect("alpha 1"),
        opened,
        "alpha 1 is exactly the committed next pose"
    );
    assert_eq!(
        interpolated_pose(&PoseSample::IDENTITY, &opened, 1.5),
        Err(AnimationError::AlphaOutOfRange { alpha: 1.5 }),
        "extrapolation is refused, not performed"
    );
    assert_eq!(
        object.states(),
        before,
        "sampling a render pose changed no simulation state"
    );
    assert_eq!(object.time(), 10, "the evaluator's head did not move");
}
