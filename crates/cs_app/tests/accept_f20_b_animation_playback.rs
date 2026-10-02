//! Acceptance scenarios F20-B: the verified transform, material and
//! attachment tracks in the fixed-tick playback.
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-B`. Task test prefix: `accept_f20_b_`. Minimum scenario
//! (AC02): "A looping propeller never emits repeated one-shot gameplay
//! events."
//!
//! These tests drive the production path end to end at this stage's scope:
//! a declared `cs_content::animation` fixture →
//! `cs_app::animation::play_animation` (lowering + one live instance) →
//! `cs_app::animation::advance_animation` (the fixed-tick entry that
//! publishes markers and applies the tracks to bound entities). They are
//! discriminating because:
//!
//! * the one-shot gameplay marker lives in the evaluator's per-activation
//!   dedup, which the playback must hold across ticks — rebuilding the
//!   evaluator per tick re-fires it every loop pass and fails the exact
//!   count;
//! * a track value is written only through a verified binding (playing clip,
//!   live scene generation, driven node), so an application that keyed
//!   entities by node id alone drives the stale-generation entity and fails
//!   the `is_none` assertions;
//! * an unknown material or parent must not reach a component and must be
//!   reported once with its claim id and reason;
//! * removing the playback (`play_animation`, `advance_animation`, the
//!   components) fails to compile.
//!
//! Every value here is newly authored fixture data, not measured original
//! game data.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use cs_app::animation::AnimatedNodeBinding;
use cs_app::animation::AnimationInstance;
use cs_app::animation::{
    AnimationLog, AnimationPlayError, AnimationPlayback, AnimationRefusal, NodeAnimatedAttachment,
    NodeAnimatedMaterial, NodeAnimatedPose, TrackKind, advance_animation, lower, play_animation,
    stop_animation,
};
use cs_app::scene::SceneGeneration;
use cs_content::animation::{
    AnimationChannel, AnimationClip, AttachmentChannel, AttachmentKey, AttachmentOp, Interpolation,
    LoopMode, MaterialChannel, MaterialKey, PosePolicy, SYNTHETIC_CARGO_BAY_NODE,
    SYNTHETIC_CARGO_DETACH_TICK, SYNTHETIC_CARGO_MATERIAL, SYNTHETIC_CARGO_NODE,
    SYNTHETIC_CARGO_SCORCH_TICK, SYNTHETIC_CARGO_SCORCHED_MATERIAL,
    SYNTHETIC_PROPELLER_GAMEPLAY_MARKER, SYNTHETIC_PROPELLER_NODE,
    SYNTHETIC_PROPELLER_PRESENTATION_MARKER, TransformChannel, TransformKey, TransformSample,
    declared_synthetic_cargo_clip, declared_synthetic_door_clip, declared_synthetic_propeller_clip,
};
use cs_content::scene::SceneNodeId;
use cs_sim::animated_object::{
    MarkerEffect as RuntimeMarkerEffect, PosePolicy as SimPosePolicy, PoseSample,
    synthetic_propeller_clip,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;
use cs_types::space::{Quaternion, Radians, UnitVec3};

// -------------------------------------------------------------- helpers ---

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("a valid claim id")
}

/// The shared nonzero session generation the playback stamps into event ids.
fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

fn content_id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("valid content id")
}

fn track(key: &str) -> ContentId {
    content_id(ContentKind::AnimationTrack, key)
}

fn node(key: &str) -> ContentId {
    content_id(ContentKind::SceneNode, key)
}

fn scene_node(key: &str) -> SceneNodeId {
    SceneNodeId::from_content_id(node(key)).expect("the fixture key names a scene node")
}

/// One live instance identity; F20-B's scenarios each play a single instance,
/// and F20-C.02 is where several instances of one track are exercised.
fn instance(value: u32) -> AnimationInstance {
    AnimationInstance::new(value).expect("a nonzero instance identity")
}

/// Binds one entity to one node of one clip under `generation`.
fn bind(
    world: &mut World,
    clip: &ContentId,
    bound_node: &ContentId,
    generation: SceneGeneration,
) -> Entity {
    world
        .spawn(AnimatedNodeBinding {
            clip: clip.clone(),
            node: bound_node.clone(),
            instance: instance(1),
            generation,
        })
        .id()
}

fn turn(axis: UnitVec3, quarter: u8) -> PoseSample {
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

/// The declared-record twin of [`turn`]: the same authored pose in
/// `cs_content::animation`'s `TransformSample`.
fn turn_sample(axis: UnitVec3, quarter: u8) -> TransformSample {
    TransformSample::try_new(
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

/// Everything the playback published since the previous drain.
fn drain(world: &mut World) -> AnimationLog {
    match world.get_resource_mut::<AnimationLog>() {
        Some(mut log) => log.drain(),
        None => AnimationLog::new(),
    }
}

fn material(key: &str) -> ContentId {
    content_id(ContentKind::Material, key)
}

// --------------------------------------------------------------- AC02 -----

/// AC02, the stage's minimum scenario: the looping propeller runs for four
/// passes through the playback and its one-shot gameplay marker fires once,
/// while its presentation cue fires once per pass and the transform track
/// keeps wrapping on the bound entity.
#[test]
fn accept_f20_b_looping_propeller_never_repeats_one_shot_gameplay_event() {
    let declared = declared_synthetic_propeller_clip();
    // The declared fixture is the same clip the F20-A evaluator was tested
    // against: lowering it must be lossless, field for field.
    assert_eq!(
        lower::lower_clip(&declared).expect("the declared propeller lowers"),
        synthetic_propeller_clip()
    );

    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(2)));
    let generation = SceneGeneration::default().next();
    let clip_id = declared.id().clone();
    let rotor = node(SYNTHETIC_PROPELLER_NODE);
    let entity = bind(&mut world, &clip_id, &rotor, generation);
    play_animation(&mut world, &declared, instance(1), generation, Tick(0))
        .expect("the playback starts");

    let mut gameplay = Vec::new();
    let mut presentation = Vec::new();
    // Four full passes of a 4-tick clip: clip time 0 ..= 16.
    for at in 0..=16u64 {
        advance_animation(&mut world, Tick(at));

        // The transform track reaches the bound entity every tick, wrapping
        // with the clip position.
        assert_eq!(
            world.get::<NodeAnimatedPose>(entity),
            Some(&NodeAnimatedPose(turn(UnitVec3::FORWARD, (at % 4) as u8))),
            "the rotor pose follows the clip position at tick {at}"
        );

        let published = drain(&mut world);
        assert!(
            published.blocked_markers().is_empty()
                && published.blocked_tracks().is_empty()
                && published.refusals().is_empty(),
            "nothing is blocked in this fixture"
        );
        for event in published.events() {
            match &event.effect {
                RuntimeMarkerEffect::Gameplay { .. } => gameplay.push(event.clone()),
                RuntimeMarkerEffect::Presentation { .. } => presentation.push(event.clone()),
            }
        }
    }

    assert_eq!(
        gameplay.len(),
        1,
        "the one-shot marker fired once across four loop passes"
    );
    assert_eq!(gameplay[0].marker, SYNTHETIC_PROPELLER_GAMEPLAY_MARKER);
    assert_eq!(gameplay[0].pass, 0);
    assert_eq!(gameplay[0].id.tick, Tick(1), "fired at its own clip tick");

    assert_eq!(
        presentation.len(),
        5,
        "the presentation cue repeats once per pass"
    );
    assert!(
        presentation
            .iter()
            .all(|event| event.marker == SYNTHETIC_PROPELLER_PRESENTATION_MARKER)
    );
    assert_eq!(
        presentation
            .iter()
            .map(|event| event.id.tick)
            .collect::<Vec<_>>(),
        vec![Tick(0), Tick(4), Tick(8), Tick(12), Tick(16)],
        "each pass fires the cue at the tick its pass reached it"
    );

    // Every id is unique and carries this session's generation plus the
    // instance's producer serial.
    let producer = world
        .resource::<AnimationPlayback>()
        .producer(&clip_id, instance(1))
        .expect("the instance is still playing");
    let ids: Vec<_> = gameplay
        .iter()
        .chain(&presentation)
        .map(|event| event.id)
        .collect();
    let unique: std::collections::BTreeSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "event ids never repeat");
    assert!(
        ids.iter()
            .all(|id| id.session == session(2) && id.producer == producer)
    );

    // Four passes later the instance is still one live instance: the clip
    // time kept counting instead of restarting per pass.
    let playback = world.resource::<AnimationPlayback>();
    assert_eq!(playback.time(&clip_id, instance(1)), Some(16));
    assert_eq!(playback.len(), 1, "one live instance per playing track");
}

// -------------------------------------------------- verified bindings -----

/// The transform track reaches exactly the entities whose binding verifies:
/// a playing clip, the scene generation the instance serves, and a node that
/// clip drives. Everything else keeps its state, and the applied pose is one
/// value for the mesh and the collider consumers.
#[test]
fn accept_f20_b_transform_track_applies_to_verified_bindings_only() {
    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(7)));
    let live = SceneGeneration::default().next();
    let superseded = SceneGeneration::default();

    let declared = declared_synthetic_door_clip();
    let clip_id = declared.id().clone();
    let door = node("synthetic.hangar.door");

    let driven = bind(&mut world, &clip_id, &door, live);
    let stale = bind(&mut world, &clip_id, &door, superseded);
    let undriven_node = bind(&mut world, &clip_id, &node("synthetic.hangar.ramp"), live);
    let unplayed = bind(&mut world, &track("synthetic.never_started"), &door, live);

    play_animation(&mut world, &declared, instance(1), live, Tick(0)).expect("the playback starts");
    assert_eq!(
        world
            .resource::<AnimationPlayback>()
            .generation(&clip_id, instance(1)),
        Some(live),
        "the instance serves the scene generation it was started under"
    );

    advance_animation(&mut world, Tick(0));
    assert_eq!(
        world.get::<NodeAnimatedPose>(driven),
        Some(&NodeAnimatedPose(PoseSample::IDENTITY)),
        "the verified binding is driven from the first tick"
    );
    assert!(
        world.get::<NodeAnimatedPose>(stale).is_none(),
        "a binding stamped by a superseded scene generation is never driven"
    );
    assert!(
        world.get::<NodeAnimatedPose>(undriven_node).is_none(),
        "the clip drives no channel on that node"
    );
    assert!(
        world.get::<NodeAnimatedPose>(unplayed).is_none(),
        "an entity bound to a track that is not playing is never driven"
    );

    // Before the open tick nothing moves.
    advance_animation(&mut world, Tick(9));
    assert_eq!(
        world.get::<NodeAnimatedPose>(driven),
        Some(&NodeAnimatedPose(PoseSample::IDENTITY))
    );

    // At the open tick the pose changes for the verified binding alone, and
    // the mesh and the collider read the one stored value.
    advance_animation(&mut world, Tick(10));
    let applied = world
        .get::<NodeAnimatedPose>(driven)
        .expect("the driven entity has a pose");
    assert_ne!(applied.pose(), PoseSample::IDENTITY, "the door opened");
    assert_eq!(
        applied.mesh(),
        applied.collider(),
        "one stored pose cannot put the mesh and the collider on different ticks"
    );
    assert!(world.get::<NodeAnimatedPose>(stale).is_none());
    assert!(world.get::<NodeAnimatedPose>(undriven_node).is_none());
    assert!(world.get::<NodeAnimatedPose>(unplayed).is_none());

    let published = drain(&mut world);
    assert_eq!(
        published.events().len(),
        1,
        "the door's gameplay marker fired once on the way"
    );
    assert_eq!(published.events()[0].marker, "door_opened");
}

// ------------------------------------ material and attachment tracks ------

/// The two `Resolved` track kinds: known references apply with their authored
/// values, and unknown references are blocked and reported instead of being
/// guessed — while the node's other tracks keep applying.
#[test]
fn accept_f20_b_material_and_attachment_tracks_apply_and_block_unknown_references() {
    // --- known references from the declared fixture -----------------------
    let declared = declared_synthetic_cargo_clip();
    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(3)));
    let generation = SceneGeneration::default().next();
    let clip_id = declared.id().clone();
    let cargo = node(SYNTHETIC_CARGO_NODE);
    let entity = bind(&mut world, &clip_id, &cargo, generation);
    play_animation(&mut world, &declared, instance(1), generation, Tick(0))
        .expect("the playback starts");

    advance_animation(&mut world, Tick(0));
    assert_eq!(
        world.get::<NodeAnimatedMaterial>(entity),
        Some(&NodeAnimatedMaterial(material(SYNTHETIC_CARGO_MATERIAL))),
        "the material track's first key applies"
    );
    let attached = world
        .get::<NodeAnimatedAttachment>(entity)
        .expect("the attachment track's first key applies");
    assert_eq!(
        attached.attachment().parent,
        Some(Resolved::Known(cs_types::content::Known::new(
            node(SYNTHETIC_CARGO_BAY_NODE),
            cs_types::content::Provenance::designed(claim("f20b.synthetic-cargo")),
        ))),
        "the parent is the authored bay node"
    );
    assert_eq!(attached.attachment().pose, SimPosePolicy::KeepLocalPose);
    assert!(
        world.get::<NodeAnimatedPose>(entity).is_none(),
        "the clip has no transform channel, so no pose is invented"
    );

    advance_animation(&mut world, Tick(SYNTHETIC_CARGO_SCORCH_TICK));
    assert_eq!(
        world.get::<NodeAnimatedMaterial>(entity),
        Some(&NodeAnimatedMaterial(material(
            SYNTHETIC_CARGO_SCORCHED_MATERIAL
        ))),
        "the material track swaps at its authored tick"
    );

    advance_animation(&mut world, Tick(SYNTHETIC_CARGO_DETACH_TICK));
    let detached = world
        .get::<NodeAnimatedAttachment>(entity)
        .expect("the detach is a recorded transition, not a disappearance");
    assert_eq!(
        detached.attachment().parent,
        None,
        "detached means no parent"
    );
    assert_eq!(detached.attachment().pose, SimPosePolicy::KeepWorldPose);
    assert!(
        drain(&mut world).is_empty(),
        "known references publish nothing but their markers (this clip has none)"
    );

    // --- unknown references block one track each --------------------------
    let unknown = unknown_tracks_clip();
    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(4)));
    let generation = SceneGeneration::default().next();
    let clip_id = unknown.id().clone();
    let panel = node("synthetic.unknown_panel");
    let entity = bind(&mut world, &clip_id, &panel, generation);
    play_animation(&mut world, &unknown, instance(1), generation, Tick(0))
        .expect("unknowns are data, not errors");

    advance_animation(&mut world, Tick(8));
    assert!(
        world.get::<NodeAnimatedPose>(entity).is_some(),
        "the transform track still applies while the unknown tracks are blocked"
    );
    assert!(
        world.get::<NodeAnimatedMaterial>(entity).is_none(),
        "an unresolved material never reaches the component"
    );
    assert!(
        world.get::<NodeAnimatedAttachment>(entity).is_none(),
        "an unresolved parent never reaches the component"
    );

    let published = drain(&mut world);
    assert_eq!(
        published.blocked_tracks().len(),
        2,
        "each unknown track is reported"
    );
    assert!(
        published.events().is_empty(),
        "this clip carries no markers"
    );
    let material_block = published
        .blocked_tracks()
        .iter()
        .find(|blocked| blocked.track == TrackKind::Material)
        .expect("the material block is reported");
    assert_eq!(material_block.clip, clip_id);
    assert_eq!(material_block.node, panel);
    assert_eq!(material_block.claim_id, claim("f20b.unknown-material"));
    assert_eq!(material_block.reason, "the material slot is undecoded");
    let attachment_block = published
        .blocked_tracks()
        .iter()
        .find(|blocked| blocked.track == TrackKind::Attachment)
        .expect("the attachment block is reported");
    assert_eq!(attachment_block.claim_id, claim("f20b.unknown-parent"));
    assert_eq!(attachment_block.reason, "the parent node is not identified");

    // The gap is reported once, not once per tick: an unresolved unknown
    // does not append one entry per frame.
    advance_animation(&mut world, Tick(9));
    assert!(
        drain(&mut world).is_empty(),
        "a persistent unknown publishes nothing new"
    );
}

/// A declared clip whose material and parent both resolve to nothing, with a
/// transform channel that must keep applying beside them.
fn unknown_tracks_clip() -> AnimationClip {
    let panel = scene_node("synthetic.unknown_panel");
    AnimationClip::try_new(
        track("synthetic.unknown_tracks"),
        Origin::SyntheticFixture,
        8,
        LoopMode::Once,
        vec![
            AnimationChannel::Transform(TransformChannel {
                target: panel.clone(),
                interpolation: Interpolation::Step,
                keys: vec![
                    TransformKey {
                        tick: 0,
                        pose: TransformSample::IDENTITY,
                    },
                    TransformKey {
                        tick: 4,
                        pose: turn_sample(UnitVec3::UP, 1),
                    },
                ],
            }),
            AnimationChannel::Material(MaterialChannel {
                target: panel.clone(),
                keys: vec![MaterialKey {
                    tick: 2,
                    material: Resolved::unknown(
                        claim("f20b.unknown-material"),
                        "the material slot is undecoded",
                    )
                    .expect("a nonempty reason"),
                }],
            }),
            AnimationChannel::Attachment(AttachmentChannel {
                target: panel,
                keys: vec![AttachmentKey {
                    tick: 3,
                    op: AttachmentOp::Attach {
                        parent: Box::new(
                            Resolved::unknown(
                                claim("f20b.unknown-parent"),
                                "the parent node is not identified",
                            )
                            .expect("a nonempty reason"),
                        ),
                        pose: PosePolicy::KeepWorldPose,
                    },
                }],
            }),
        ],
        Vec::new(),
        Provenance::designed(claim("f20b.test")),
    )
    .expect("an unknown reference is data the runtime must block on, not an authoring error")
}

// ------------------------------------------------------------- refusals ---

/// A session tick that goes backwards holds every clip where it is: the head
/// does not move, no marker re-offers, and the hold is reported once instead
/// of once per tick (F20 non-negotiable behavior 5).
#[test]
fn accept_f20_b_holding_the_head_never_replays_a_one_shot_marker() {
    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(5)));
    let generation = SceneGeneration::default().next();
    let declared = declared_synthetic_door_clip();
    let clip_id = declared.id().clone();
    let door = node("synthetic.hangar.door");
    let entity = bind(&mut world, &clip_id, &door, generation);
    play_animation(&mut world, &declared, instance(1), generation, Tick(0))
        .expect("the playback starts");

    advance_animation(&mut world, Tick(10));
    let opened = drain(&mut world);
    assert_eq!(opened.events().len(), 1, "the marker fired on the way up");
    let open_pose = world
        .get::<NodeAnimatedPose>(entity)
        .expect("the door has a pose")
        .pose();
    assert_ne!(open_pose, PoseSample::IDENTITY);

    // The session goes back to tick 3: nothing moves and nothing re-fires.
    advance_animation(&mut world, Tick(3));
    let held = drain(&mut world);
    assert!(held.events().is_empty(), "a rewind re-offers no marker");
    assert_eq!(
        held.refusals(),
        &[AnimationRefusal::Held {
            clip: clip_id.clone(),
            from: 10,
            to: 3,
        }],
        "the hold is reported with the positions it spans"
    );
    assert_eq!(
        world
            .get::<NodeAnimatedPose>(entity)
            .map(|pose| pose.pose()),
        Some(open_pose),
        "the held head keeps the state it had reached"
    );

    advance_animation(&mut world, Tick(3));
    assert!(
        drain(&mut world).is_empty(),
        "the same hold is not reported on every tick it lasts"
    );

    // Catching up again does not replay the marker either.
    advance_animation(&mut world, Tick(10));
    let caught_up = drain(&mut world);
    assert!(
        caught_up.events().is_empty(),
        "the one-shot marker stays fired after the rewind"
    );
    assert!(caught_up.refusals().is_empty());
    assert!(
        world
            .resource::<AnimationPlayback>()
            .is_playing(&clip_id, instance(1)),
        "a held instance keeps playing"
    );
}

// ----------------------------------------------------------- lifecycle ----

/// The playback refuses what it cannot honestly do instead of guessing: no
/// session, a second instance of the *same* identity, an advance with nothing
/// to advance, and a stop that releases what the instance applied.
#[test]
fn accept_f20_b_play_requires_a_session_and_refuses_a_second_instance() {
    let mut world = World::new();
    let declared = declared_synthetic_propeller_clip();
    let clip_id = declared.id().clone();
    let generation = SceneGeneration::default().next();
    let rotor = node(SYNTHETIC_PROPELLER_NODE);
    let entity = bind(&mut world, &clip_id, &rotor, generation);

    assert_eq!(
        play_animation(&mut world, &declared, instance(1), generation, Tick(0)),
        Err(AnimationPlayError::NoSession),
        "an animation never plays in no session at all"
    );
    advance_animation(&mut world, Tick(1));
    assert!(
        drain(&mut world).is_empty(),
        "with no session there is nothing to advance"
    );
    assert!(
        !stop_animation(&mut world, &clip_id, instance(1)),
        "stopping with no session reports nothing"
    );

    world.insert_resource(AnimationPlayback::new(session(9)));
    play_animation(&mut world, &declared, instance(1), generation, Tick(0))
        .expect("the first instance starts");
    assert_eq!(
        play_animation(&mut world, &declared, instance(1), generation, Tick(0)),
        Err(AnimationPlayError::AlreadyPlaying {
            clip: clip_id.clone(),
            instance: instance(1),
        }),
        "a second instance of the same identity is refused, not silently substituted"
    );
    assert_eq!(world.resource::<AnimationPlayback>().len(), 1);

    advance_animation(&mut world, Tick(1));
    assert_eq!(
        drain(&mut world).events().len(),
        2,
        "one presentation and one gameplay marker crossed at clip time 1"
    );

    assert!(stop_animation(&mut world, &clip_id, instance(1)));
    assert!(
        !world
            .resource::<AnimationPlayback>()
            .is_playing(&clip_id, instance(1))
    );
    advance_animation(&mut world, Tick(2));
    assert!(
        drain(&mut world).is_empty(),
        "a stopped instance publishes nothing"
    );
    assert!(
        world.get::<NodeAnimatedPose>(entity).is_none(),
        "the stop released the state the instance applied (F20-C.02 teardown)"
    );
    assert!(
        world.get::<AnimatedNodeBinding>(entity).is_none(),
        "the binding named an instance that no longer plays, so it goes with it"
    );
}

// ------------------------------------------------------- start boundary ---

/// An instance whose start tick has not arrived plays nothing at all: no
/// marker (not even one authored at clip tick 0), no applied state, no
/// refusal — so a clip scheduled to start at tick `N` is silent at `N - 1`
/// and first fires at `N` (F20 non-negotiable behavior 1: markers fire at
/// their authored tick of the fixed-tick simulation).
#[test]
fn accept_f20_b_a_clip_never_plays_before_its_start_tick() {
    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(11)));
    let generation = SceneGeneration::default().next();
    let declared = declared_synthetic_propeller_clip();
    let clip_id = declared.id().clone();
    let rotor = node(SYNTHETIC_PROPELLER_NODE);
    let entity = bind(&mut world, &clip_id, &rotor, generation);

    play_animation(&mut world, &declared, instance(1), generation, Tick(10))
        .expect("the playback starts");

    // The session is one tick before the instance's own start tick.
    advance_animation(&mut world, Tick(9));
    assert!(
        drain(&mut world).is_empty(),
        "nothing is offered before the clip starts"
    );
    assert!(
        world.get::<NodeAnimatedPose>(entity).is_none(),
        "no state is applied before the clip starts"
    );
    assert_eq!(
        world
            .resource::<AnimationPlayback>()
            .time(&clip_id, instance(1)),
        Some(0),
        "the head has not moved"
    );

    // At the start tick the clip time reaches 0: the tick-0 marker fires
    // there, stamped with that tick, and not one tick earlier.
    advance_animation(&mut world, Tick(10));
    let started = drain(&mut world);
    assert_eq!(
        started.events().len(),
        1,
        "only the clip's tick-0 marker crosses"
    );
    assert_eq!(
        started.events()[0].marker,
        SYNTHETIC_PROPELLER_PRESENTATION_MARKER
    );
    assert_eq!(started.events()[0].id.tick, Tick(10));
    assert!(
        started.refusals().is_empty(),
        "a pending start is not a hold"
    );
    assert!(world.get::<NodeAnimatedPose>(entity).is_some());

    advance_animation(&mut world, Tick(11));
    let next = drain(&mut world);
    assert_eq!(
        next.events().len(),
        1,
        "the gameplay marker follows at its own clip tick"
    );
    assert_eq!(next.events()[0].marker, SYNTHETIC_PROPELLER_GAMEPLAY_MARKER);
    assert_eq!(next.events()[0].id.tick, Tick(11));
}

// --------------------------------------------------------- unbound gaps ---

/// A gap in the clip's own content is reported from the evaluated state
/// alone: an unknown material or parent is visible in the log even when no
/// entity is bound to that node, exactly like a blocked marker — while an
/// unbound entity keeps its state (nothing is guessed into the world).
#[test]
fn accept_f20_b_an_unknown_reference_is_reported_without_a_bound_entity() {
    let unknown = unknown_tracks_clip();
    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(12)));
    let generation = SceneGeneration::default().next();
    let clip_id = unknown.id().clone();
    // Deliberately no AnimatedNodeBinding anywhere in this world.
    play_animation(&mut world, &unknown, instance(1), generation, Tick(0))
        .expect("unknowns are data, not errors");

    advance_animation(&mut world, Tick(8));
    let published = drain(&mut world);
    assert_eq!(
        published.blocked_tracks().len(),
        2,
        "both unknown tracks are reported without any binding"
    );
    assert!(
        published
            .blocked_tracks()
            .iter()
            .all(|blocked| blocked.clip == clip_id),
        "each report names the clip it belongs to"
    );
    let mut poses = world.query::<&NodeAnimatedPose>();
    assert_eq!(
        poses.iter(&world).count(),
        0,
        "an unbound world is left untouched"
    );
    let mut materials = world.query::<&NodeAnimatedMaterial>();
    assert_eq!(materials.iter(&world).count(), 0);

    // The gap is reported once per instance, not once per tick.
    advance_animation(&mut world, Tick(9));
    assert!(drain(&mut world).is_empty());
}
