//! Acceptance scenarios F20-A: animation channels and event markers in the
//! fixed-tick evaluator.
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-A`. Task test prefix: `accept_f20_a_`. Minimum scenario
//! (AC01): "A door opening at a fixed tick changes collider and mesh state
//! coherently."
//!
//! These tests call production code only: [`cs_sim::animated_object`] is the
//! runtime record and evaluator the simulation consumes. They are
//! discriminating because:
//!
//! * the door's mesh and collider poses are the same evaluated value, so a
//!   fix that moved one and not the other, or moved them one tick apart,
//!   fails the equality assertion;
//! * the gameplay-marker dedup is in the evaluator, so a clip that re-fired
//!   `engine_started` on every loop pass fails the exact-count assertion;
//! * an unknown marker effect produces a [`BlockedMarker`], so an
//!   implementation that skipped or guessed it emits nothing here;
//! * removing the evaluator (`AnimatedObject`, `advance_to`, `states`)
//!   fails to compile.
//!
//! Every value here is newly authored fixture data, not measured original
//! game data.

use cs_sim::animated_object::{
    AnimatedClip, AnimatedObject, AnimationError, AttachmentKey, AttachmentOp, ClipMarker,
    Interpolation, LoopMode, MarkerEffect, NodeChannel, PosePolicy, PoseSample,
    SYNTHETIC_DOOR_MARKER, SYNTHETIC_DOOR_OPEN_TICK, TransformKey, Visibility, VisibilityKey,
    synthetic_door_clip, synthetic_propeller_clip,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::{Quaternion, Radians, UnitVec3};

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("a valid claim id")
}

fn designed() -> Provenance {
    Provenance::designed(claim("f20a.test"))
}

fn content_id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("valid content id")
}

fn scene_node(key: &str) -> ContentId {
    content_id(ContentKind::SceneNode, key)
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, designed()))
}

fn turn(axis: UnitVec3, quarter_turns: u8) -> PoseSample {
    PoseSample::try_new(
        Quaternion::from_axis_angle(
            axis,
            Radians(f64::from(quarter_turns) * std::f64::consts::FRAC_PI_2),
        )
        .expect("a quarter turn is unit length"),
        [0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0],
    )
    .expect("the pose is finite")
}

/// AC01: the door opens at its fixed tick and the collider and mesh states
/// change together — they are one evaluated pose, so they cannot diverge.
#[test]
fn accept_f20_a_door_opens_at_fixed_tick_changing_collider_and_mesh_coherently() {
    let clip = synthetic_door_clip();
    let door = scene_node("synthetic.hangar.door");
    let mut object = AnimatedObject::new(clip, 1, 7);

    let before = object.states();
    let closed = before.get(&door).expect("the door node has a state");
    assert_eq!(closed.mesh_pose(), Some(&PoseSample::IDENTITY));
    assert_eq!(
        closed.mesh_pose(),
        closed.collider_pose(),
        "mesh and collider read the same evaluated pose"
    );
    assert!(closed.collider_enabled());

    // Before the open tick nothing changes: not the pose, not the events.
    let early = object.advance_to(9, Tick(9)).expect("forward advance");
    assert!(early.events.is_empty(), "the marker has not fired yet");
    assert_eq!(
        object.states()[&door].mesh_pose(),
        Some(&PoseSample::IDENTITY),
        "the door is still closed at tick 9"
    );

    // At the fixed tick the mesh and collider poses change in the same
    // advance and the gameplay marker fires once.
    let outcome = object
        .advance_to(SYNTHETIC_DOOR_OPEN_TICK, Tick(SYNTHETIC_DOOR_OPEN_TICK))
        .expect("the open tick");
    assert_eq!(outcome.events.len(), 1);
    let event = &outcome.events[0];
    assert_eq!(event.marker, SYNTHETIC_DOOR_MARKER);
    assert_eq!(
        event.effect,
        MarkerEffect::Gameplay {
            cue: "synthetic.hangar.door_opened".to_owned()
        }
    );
    assert_eq!(
        event.id,
        cs_sim::animated_object::AnimationEventId {
            session: 1,
            tick: Tick(10),
            producer: 7,
            sequence: 0,
        }
    );

    let opened = &object.states()[&door];
    let open = opened
        .mesh_pose()
        .expect("the door still has a pose")
        .to_owned();
    assert_ne!(open, PoseSample::IDENTITY, "the door moved");
    assert_eq!(
        opened.collider_pose(),
        Some(&open),
        "the collider moved with the mesh in the same tick"
    );
    assert!(opened.collider_enabled());

    // Re-advancing to or past the same time fires nothing again and keeps
    // the terminal state.
    let again = object.advance_to(30, Tick(30)).expect("clip end");
    assert!(again.events.is_empty(), "the one-shot marker stays fired");
    assert!(again.finished && object.is_finished());
    assert_eq!(object.states()[&door].mesh_pose(), Some(&open));
}

/// AC02 shape at this stage's scope: a looping propeller emits its
/// presentation cue once per pass but its one-shot gameplay marker exactly
/// once across the whole activation.
#[test]
fn accept_f20_a_looping_propeller_never_repeats_one_shot_gameplay_event() {
    let mut object = AnimatedObject::new(synthetic_propeller_clip(), 2, 3);

    // Three full passes: clip time 0 -> 12 with the head wrapping each 4.
    let mut gameplay = Vec::new();
    let mut presentation = Vec::new();
    for (tick, clip_time) in [0u64, 4, 8, 12].into_iter().enumerate() {
        let outcome = object
            .advance_to(clip_time, Tick(tick as u64))
            .expect("forward");
        for event in outcome.events {
            match &event.effect {
                MarkerEffect::Gameplay { .. } => gameplay.push(event),
                MarkerEffect::Presentation { .. } => presentation.push(event),
            }
        }
    }

    assert_eq!(
        gameplay.len(),
        1,
        "the one-shot marker fired once in three loops"
    );
    assert_eq!(gameplay[0].marker, "engine_started");
    assert_eq!(gameplay[0].pass, 0);
    assert_eq!(
        presentation.len(),
        4,
        "the presentation cue may repeat once per pass"
    );
    assert!(
        presentation
            .iter()
            .all(|event| event.marker == "blade_pass")
    );
    assert_eq!(
        presentation
            .iter()
            .map(|event| event.pass)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );

    // Every fired event has a unique id carrying the session, tick and
    // producer the caller supplied.
    let ids: Vec<_> = gameplay
        .iter()
        .chain(&presentation)
        .map(|event| event.id)
        .collect();
    let unique: std::collections::BTreeSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "event ids never repeat");
    assert!(ids.iter().all(|id| id.session == 2 && id.producer == 3));

    // The rotor pose keeps wrapping with the clip position.
    let rotor = scene_node("synthetic.plane.prop");
    assert_eq!(object.position(), 0);
    assert_eq!(
        object.states()[&rotor].mesh_pose(),
        Some(&turn(UnitVec3::FORWARD, 0)),
        "pass 3 position 0 shows the same rotor pose as activation"
    );
}

/// F20 non-negotiable behavior 2: a marker whose effect is unknown does not
/// fire an event and is not skipped — the crossing surfaces a blocked
/// record with the unknown's claim and reason, once per activation.
#[test]
fn accept_f20_a_unknown_marker_effect_blocks_the_transition() {
    let clip = AnimatedClip::try_new(
        content_id(ContentKind::AnimationTrack, "synthetic.blocked"),
        8,
        LoopMode::Once,
        vec![NodeChannel::Transform {
            target: scene_node("synthetic.gate"),
            interpolation: Interpolation::Step,
            keys: vec![TransformKey {
                tick: 0,
                pose: PoseSample::IDENTITY,
            }],
        }],
        vec![ClipMarker {
            tick: 4,
            key: "mystery".to_owned(),
            effect: Resolved::unknown(
                claim("f20a.unknown-marker"),
                "the marker's effect field is undecoded",
            )
            .expect("a nonempty reason"),
        }],
    )
    .expect("a clip may carry unknown markers");

    let mut object = AnimatedObject::new(clip, 5, 1);
    let outcome = object.advance_to(6, Tick(6)).expect("crossing the marker");
    assert!(
        outcome.events.is_empty(),
        "an unknown effect fires no event"
    );
    assert_eq!(outcome.blocked.len(), 1);
    let blocked = &outcome.blocked[0];
    assert_eq!(blocked.marker, "mystery");
    assert_eq!(blocked.claim_id, claim("f20a.unknown-marker"));
    assert_eq!(blocked.reason, "the marker's effect field is undecoded");

    // Reported once: a second crossing attempt is a no-op, and the same
    // marker inside a looping clip does not stack blocked reports either.
    let again = object.advance_to(8, Tick(8)).expect("end of clip");
    assert!(again.blocked.is_empty() && again.events.is_empty());
}

/// Skipping ahead lands the terminal channel state and fires each pending
/// gameplay marker exactly once — the AC04 shape inside this stage.
#[test]
fn accept_f20_a_skip_to_end_reaches_final_state_once() {
    let door = scene_node("synthetic.hangar.door");
    let mut object = AnimatedObject::new(synthetic_door_clip(), 3, 2);

    let outcome = object
        .advance_to(30, Tick(99))
        .expect("a single skip to the end");
    assert_eq!(outcome.events.len(), 1);
    assert_eq!(outcome.events[0].marker, SYNTHETIC_DOOR_MARKER);
    assert!(outcome.finished);
    let states = object.states();
    let pose = states[&door].mesh_pose().expect("has a pose");
    assert_ne!(*pose, PoseSample::IDENTITY, "the terminal state is open");

    // Skipping again — or replaying the span — fires nothing twice.
    let replay = object.advance_to(30, Tick(100)).expect("already ended");
    assert!(replay.events.is_empty());
}

/// A hidden node's collider drops with its mesh: the designed coherence
/// rule (a visibility swap must not leave an invisible wall).
#[test]
fn accept_f20_a_hidden_node_loses_its_collider_coherently() {
    let node = scene_node("synthetic.pane");
    let clip = AnimatedClip::try_new(
        content_id(ContentKind::AnimationTrack, "synthetic.hide"),
        6,
        LoopMode::Once,
        vec![NodeChannel::Visibility {
            target: node.clone(),
            keys: vec![
                VisibilityKey {
                    tick: 0,
                    visibility: Visibility::Visible,
                },
                VisibilityKey {
                    tick: 3,
                    visibility: Visibility::Hidden,
                },
            ],
        }],
        Vec::new(),
    )
    .expect("valid clip");

    let mut object = AnimatedObject::new(clip, 1, 1);
    assert_eq!(
        object.states()[&node].visibility(),
        Some(Visibility::Visible)
    );
    assert!(object.states()[&node].collider_enabled());
    object.advance_to(3, Tick(3)).expect("the hide tick");
    let state = &object.states()[&node];
    assert_eq!(state.visibility(), Some(Visibility::Hidden));
    assert!(
        !state.collider_enabled(),
        "a hidden node carries no collider"
    );
}

/// Attachment records evaluate to an explicit parent change with the
/// authored pose policy — F20-C owns the inherited-velocity physics; this
/// stage owns that the record survives evaluation faithfully.
#[test]
fn accept_f20_a_attachment_channel_records_parent_change_explicitly() {
    let cargo = scene_node("synthetic.cargo");
    let parent = scene_node("synthetic.cargo.bay");
    let clip = AnimatedClip::try_new(
        content_id(ContentKind::AnimationTrack, "synthetic.attach"),
        10,
        LoopMode::Once,
        vec![NodeChannel::Attachment {
            target: cargo.clone(),
            keys: vec![
                AttachmentKey {
                    tick: 0,
                    op: AttachmentOp::Attach {
                        parent: Box::new(known(parent.clone())),
                        pose: PosePolicy::KeepLocalPose,
                    },
                },
                AttachmentKey {
                    tick: 6,
                    op: AttachmentOp::Detach {
                        pose: PosePolicy::KeepWorldPose,
                    },
                },
            ],
        }],
        Vec::new(),
    )
    .expect("valid clip");

    let mut object = AnimatedObject::new(clip, 1, 1);
    let states = object.states();
    let attached = states[&cargo].attachment().expect("attached from tick 0");
    assert_eq!(attached.parent, Some(known(parent)));
    assert_eq!(attached.pose, PosePolicy::KeepLocalPose);

    object.advance_to(6, Tick(6)).expect("the detach tick");
    let states = object.states();
    let detached = states[&cargo].attachment().expect("the detach is recorded");
    assert_eq!(detached.parent, None, "detached means no parent");
    assert_eq!(detached.pose, PosePolicy::KeepWorldPose);
}

/// `Interpolation::Linear` moves the semantic pose between authored keys at
/// integer ticks; `Step` holds the previous key.
#[test]
fn accept_f20_a_linear_channel_interpolates_at_whole_ticks() {
    let node = scene_node("synthetic.lift");
    let rise = |height: f64| {
        PoseSample::try_new(Quaternion::IDENTITY, [0.0, height, 0.0], [1.0, 1.0, 1.0])
            .expect("finite pose")
    };
    let clip = AnimatedClip::try_new(
        content_id(ContentKind::AnimationTrack, "synthetic.lift"),
        10,
        LoopMode::Once,
        vec![NodeChannel::Transform {
            target: node.clone(),
            interpolation: Interpolation::Linear,
            keys: vec![
                TransformKey {
                    tick: 0,
                    pose: rise(0.0),
                },
                TransformKey {
                    tick: 10,
                    pose: rise(2.0),
                },
            ],
        }],
        Vec::new(),
    )
    .expect("valid clip");

    let mut object = AnimatedObject::new(clip, 1, 1);
    object.advance_to(5, Tick(5)).expect("halfway");
    let states = object.states();
    let midway = states[&node].mesh_pose().expect("a pose is sampled");
    assert_eq!(
        midway.translation_m(),
        [0.0, 1.0, 0.0],
        "half the clip time is half the lift"
    );
}

/// The failure cases: backwards playback is refused by name, and malformed
/// clips fail construction instead of reaching the evaluator.
#[test]
fn accept_f20_a_regression_and_malformed_clips_are_refused() {
    let mut object = AnimatedObject::new(synthetic_door_clip(), 1, 1);
    object.advance_to(4, Tick(4)).expect("forward");
    assert_eq!(
        object.advance_to(2, Tick(5)),
        Err(AnimationError::Regression { from: 4, to: 2 }),
        "reversing a playing clip is refused"
    );
    assert_eq!(object.time(), 4, "a refused advance changes nothing");

    let track = || content_id(ContentKind::AnimationTrack, "synthetic.bad");
    let node = || scene_node("synthetic.node");
    let one_key = || NodeChannel::Transform {
        target: node(),
        interpolation: Interpolation::Step,
        keys: vec![TransformKey {
            tick: 0,
            pose: PoseSample::IDENTITY,
        }],
    };

    assert_eq!(
        AnimatedClip::try_new(
            content_id(ContentKind::Mesh, "synthetic.not_a_track"),
            4,
            LoopMode::Once,
            vec![one_key()],
            Vec::new(),
        ),
        Err(AnimationError::NotAnAnimationTrack {
            kind: ContentKind::Mesh
        })
    );
    assert_eq!(
        AnimatedClip::try_new(track(), 0, LoopMode::Once, vec![one_key()], Vec::new()),
        Err(AnimationError::ZeroDuration { clip: track() }),
    );
    assert_eq!(
        AnimatedClip::try_new(
            track(),
            4,
            LoopMode::Once,
            vec![NodeChannel::Transform {
                target: node(),
                interpolation: Interpolation::Step,
                keys: vec![
                    TransformKey {
                        tick: 2,
                        pose: PoseSample::IDENTITY,
                    },
                    TransformKey {
                        tick: 2,
                        pose: PoseSample::IDENTITY,
                    },
                ],
            }],
            Vec::new(),
        ),
        Err(AnimationError::KeyTicks {
            target: node(),
            first: 2,
            second: 2,
        }),
    );
    assert_eq!(
        AnimatedClip::try_new(
            track(),
            4,
            LoopMode::Once,
            vec![NodeChannel::Transform {
                target: node(),
                interpolation: Interpolation::Step,
                keys: vec![TransformKey {
                    tick: 9,
                    pose: PoseSample::IDENTITY,
                }],
            }],
            Vec::new(),
        ),
        Err(AnimationError::BeyondDuration {
            tick: 9,
            duration: 4
        }),
    );
    assert_eq!(
        AnimatedClip::try_new(
            track(),
            4,
            LoopMode::Once,
            vec![NodeChannel::Visibility {
                target: node(),
                keys: Vec::new(),
            }],
            Vec::new(),
        ),
        Err(AnimationError::EmptyChannel { target: node() }),
    );
    assert_eq!(
        AnimatedClip::try_new(
            track(),
            4,
            LoopMode::Once,
            vec![NodeChannel::Material {
                target: node(),
                keys: vec![cs_sim::animated_object::MaterialKey {
                    tick: 0,
                    material: known(content_id(ContentKind::Mesh, "synthetic.mesh")),
                }],
            }],
            Vec::new(),
        ),
        Err(AnimationError::MaterialKind {
            target: node(),
            material: ContentKind::Mesh,
        }),
    );
    assert_eq!(
        AnimatedClip::try_new(
            track(),
            4,
            LoopMode::Once,
            vec![NodeChannel::Visibility {
                target: content_id(ContentKind::Mesh, "synthetic.mesh_node"),
                keys: vec![VisibilityKey {
                    tick: 0,
                    visibility: Visibility::Visible,
                }],
            }],
            Vec::new(),
        ),
        Err(AnimationError::TargetKind {
            target: content_id(ContentKind::Mesh, "synthetic.mesh_node"),
        }),
    );
    assert_eq!(
        AnimatedClip::try_new(
            track(),
            4,
            LoopMode::Once,
            vec![one_key()],
            vec![
                ClipMarker {
                    tick: 1,
                    key: "dup".to_owned(),
                    effect: known(MarkerEffect::Presentation {
                        cue: "a".to_owned()
                    }),
                },
                ClipMarker {
                    tick: 1,
                    key: "dup".to_owned(),
                    effect: known(MarkerEffect::Presentation {
                        cue: "b".to_owned()
                    }),
                },
            ],
        ),
        Err(AnimationError::DuplicateMarkerKey {
            key: "dup".to_owned()
        }),
    );
    assert_eq!(
        AnimatedClip::try_new(
            track(),
            4,
            LoopMode::Once,
            vec![one_key()],
            vec![ClipMarker {
                tick: 9,
                key: "late".to_owned(),
                effect: known(MarkerEffect::Gameplay {
                    cue: "a".to_owned()
                }),
            }],
        ),
        Err(AnimationError::BeyondDuration {
            tick: 9,
            duration: 4
        }),
    );
    assert_eq!(
        PoseSample::interpolate(&PoseSample::IDENTITY, &PoseSample::IDENTITY, 1.5),
        Err(AnimationError::AlphaOutOfRange { alpha: 1.5 }),
    );
}
