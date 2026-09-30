//! Acceptance tests for the F20-A declared animation IR: the
//! provenance-carrying `AnimationClip` record and its validation boundary.
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-A`. Task test prefix: `accept_f20_a_`.
//!
//! These tests use only [`cs_content`]'s public API: the declared clip, its
//! channel/marker records and the synthetic door fixture. They fail to
//! compile if the record is removed, and they fail at run time if the
//! validation boundary stops refusing malformed clips or starts dropping
//! unknown references.
//!
//! Every value is newly authored fixture data; the fixture asserts its
//! synthetic origin so it can never masquerade as retail content.

use cs_content::animation::{
    AnimationChannel, AnimationClip, AttachmentChannel, AttachmentKey, AttachmentOp, ClipError,
    EventMarker, Interpolation, LoopMode, MarkerEffect, MaterialChannel, MaterialKey, PosePolicy,
    SYNTHETIC_DOOR_MARKER, SYNTHETIC_DOOR_OPEN_TICK, TransformChannel, TransformKey,
    TransformSample, VisibilityChannel, VisibilityKey, declared_synthetic_door_clip,
};
use cs_content::scene::{NodeVisibility, SceneNodeId};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::{Quaternion, SpaceError};

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("a valid claim id")
}

fn designed() -> Provenance {
    Provenance::designed(claim("f20a.test"))
}

fn content_id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("valid content id")
}

fn scene_node(key: &str) -> SceneNodeId {
    SceneNodeId::from_content_id(content_id(ContentKind::SceneNode, key)).expect("a scene_node id")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, designed()))
}

fn unknown<T>(reason: &str) -> Resolved<T> {
    Resolved::unknown(claim("f20a.test.unknown"), reason).expect("a nonempty reason")
}

fn track() -> ContentId {
    content_id(ContentKind::AnimationTrack, "synthetic.test_track")
}

fn one_key_channel(node: &SceneNodeId) -> AnimationChannel {
    AnimationChannel::Transform(TransformChannel {
        target: node.clone(),
        interpolation: Interpolation::Step,
        keys: vec![TransformKey {
            tick: 0,
            pose: TransformSample::IDENTITY,
        }],
    })
}

/// The declared fixture validates, names its channel and marker, and is
/// explicitly synthetic — provenance can never promote it to original data.
#[test]
fn accept_f20_a_declared_door_clip_is_valid_and_synthetic() {
    let clip = declared_synthetic_door_clip();
    assert_eq!(
        clip.id(),
        &content_id(ContentKind::AnimationTrack, "synthetic.door_open")
    );
    assert_eq!(clip.origin(), &Origin::SyntheticFixture);
    assert!(!clip.origin().is_original());
    assert_eq!(clip.duration_ticks(), 30);
    assert_eq!(clip.loop_mode(), LoopMode::Once);
    assert_eq!(
        clip.provenance().class,
        cs_types::evidence::ClaimStatus::Designed
    );

    let AnimationChannel::Transform(channel) = &clip.channels()[0] else {
        panic!("the door fixture's channel is a transform channel")
    };
    assert_eq!(channel.target.key(), "synthetic.hangar.door");
    assert_eq!(channel.keys.len(), 2);
    assert_eq!(channel.keys[1].tick, SYNTHETIC_DOOR_OPEN_TICK);
    assert_ne!(
        channel.keys[1].pose.rotation().components(),
        Quaternion::IDENTITY.components(),
        "the open key rotates the door"
    );

    assert_eq!(clip.markers().len(), 1);
    let marker = &clip.markers()[0];
    assert_eq!(marker.tick, SYNTHETIC_DOOR_OPEN_TICK);
    assert_eq!(marker.key, SYNTHETIC_DOOR_MARKER);
    let Resolved::Known(effect) = &marker.effect else {
        panic!("the door marker is a known effect")
    };
    assert!(effect.value.is_gameplay());
}

/// The full channel inventory is representable: transform, visibility,
/// material and attachment all bind to scene nodes and carry their values.
#[test]
fn accept_f20_a_all_four_channel_kinds_bind_scene_nodes() {
    let node = scene_node("synthetic.rig.node");
    let parent = scene_node("synthetic.rig.mount");
    let clip = AnimationClip::try_new(
        track(),
        Origin::SyntheticFixture,
        8,
        LoopMode::Loop,
        vec![
            one_key_channel(&node),
            AnimationChannel::Visibility(VisibilityChannel {
                target: node.clone(),
                keys: vec![VisibilityKey {
                    tick: 0,
                    visibility: NodeVisibility::Visible,
                }],
            }),
            AnimationChannel::Material(MaterialChannel {
                target: node.clone(),
                keys: vec![MaterialKey {
                    tick: 0,
                    material: known(content_id(ContentKind::Material, "synthetic.paint")),
                }],
            }),
            AnimationChannel::Attachment(AttachmentChannel {
                target: node.clone(),
                keys: vec![AttachmentKey {
                    tick: 2,
                    op: AttachmentOp::Attach {
                        parent: Box::new(known(parent)),
                        pose: PosePolicy::KeepWorldPose,
                    },
                }],
            }),
        ],
        Vec::new(),
        designed(),
    )
    .expect("a clip with all four channel kinds validates");
    assert_eq!(clip.channels().len(), 4);
    assert!(
        clip.channels()
            .iter()
            .all(|channel| channel.target().key() == "synthetic.rig.node")
    );
}

/// Unknown references are carried, not refused or dropped: an unknown
/// material, an unknown attachment parent and an unknown marker effect all
/// survive validation with their claim id intact.
#[test]
fn accept_f20_a_unknown_references_are_retained_not_dropped() {
    let node = scene_node("synthetic.rig.node");
    let clip = AnimationClip::try_new(
        track(),
        Origin::SyntheticFixture,
        8,
        LoopMode::Once,
        vec![
            AnimationChannel::Material(MaterialChannel {
                target: node.clone(),
                keys: vec![MaterialKey {
                    tick: 0,
                    material: unknown("the material index is undecoded"),
                }],
            }),
            AnimationChannel::Attachment(AttachmentChannel {
                target: node.clone(),
                keys: vec![AttachmentKey {
                    tick: 0,
                    op: AttachmentOp::Attach {
                        parent: Box::new(unknown("the parent field is undecoded")),
                        pose: PosePolicy::KeepWorldPose,
                    },
                }],
            }),
        ],
        vec![EventMarker {
            tick: 4,
            key: "undecoded".to_owned(),
            effect: unknown("the marker opcode is undecoded"),
        }],
        designed(),
    )
    .expect("unknowns are carried, not refused");

    let AnimationChannel::Material(material) = &clip.channels()[0] else {
        panic!("channel 0 is material")
    };
    let Resolved::Unknown { claim_id, reason } = &material.keys[0].material else {
        panic!("the unknown material is retained")
    };
    assert_eq!(*claim_id, claim("f20a.test.unknown"));
    assert_eq!(reason, "the material index is undecoded");

    let AnimationChannel::Attachment(attachment) = &clip.channels()[1] else {
        panic!("channel 1 is attachment")
    };
    let AttachmentOp::Attach { parent, .. } = &attachment.keys[0].op else {
        panic!("key 0 is an attach")
    };
    assert!(matches!(parent.as_ref(), Resolved::Unknown { .. }));

    let Resolved::Unknown { reason, .. } = &clip.markers()[0].effect else {
        panic!("the unknown marker effect is retained")
    };
    assert_eq!(reason, "the marker opcode is undecoded");
}

/// The validation boundary refuses malformed clips by name instead of
/// repairing them.
#[test]
fn accept_f20_a_malformed_clips_are_refused() {
    let node = scene_node("synthetic.rig.node");
    let build = |duration, channels, markers| {
        AnimationClip::try_new(
            track(),
            Origin::SyntheticFixture,
            duration,
            LoopMode::Once,
            channels,
            markers,
            designed(),
        )
    };

    assert_eq!(
        AnimationClip::try_new(
            content_id(ContentKind::Script, "synthetic.not_a_track"),
            Origin::SyntheticFixture,
            4,
            LoopMode::Once,
            vec![one_key_channel(&node)],
            Vec::new(),
            designed(),
        ),
        Err(ClipError::NotAnAnimationTrack {
            kind: ContentKind::Script
        })
    );
    assert_eq!(
        build(0, vec![one_key_channel(&node)], Vec::new()),
        Err(ClipError::ZeroDuration)
    );
    assert_eq!(
        build(
            4,
            vec![AnimationChannel::Visibility(VisibilityChannel {
                target: node.clone(),
                keys: Vec::new(),
            })],
            Vec::new(),
        ),
        Err(ClipError::EmptyChannel {
            target: "synthetic.rig.node".to_owned()
        })
    );
    assert_eq!(
        build(
            4,
            vec![AnimationChannel::Transform(TransformChannel {
                target: node.clone(),
                interpolation: Interpolation::Step,
                keys: vec![
                    TransformKey {
                        tick: 3,
                        pose: TransformSample::IDENTITY,
                    },
                    TransformKey {
                        tick: 1,
                        pose: TransformSample::IDENTITY,
                    },
                ],
            })],
            Vec::new(),
        ),
        Err(ClipError::KeyTicks {
            target: "synthetic.rig.node".to_owned(),
            first: 3,
            second: 1,
        })
    );
    assert_eq!(
        build(
            4,
            vec![AnimationChannel::Transform(TransformChannel {
                target: node.clone(),
                interpolation: Interpolation::Step,
                keys: vec![TransformKey {
                    tick: 5,
                    pose: TransformSample::IDENTITY,
                }],
            })],
            Vec::new(),
        ),
        Err(ClipError::BeyondDuration {
            tick: 5,
            duration: 4
        })
    );
    assert_eq!(
        build(
            4,
            vec![AnimationChannel::Material(MaterialChannel {
                target: node.clone(),
                keys: vec![MaterialKey {
                    tick: 0,
                    material: known(content_id(ContentKind::Mesh, "synthetic.mesh")),
                }],
            })],
            Vec::new(),
        ),
        Err(ClipError::MaterialKind {
            target: "synthetic.rig.node".to_owned(),
            material: ContentKind::Mesh,
        })
    );
    assert_eq!(
        build(
            4,
            vec![one_key_channel(&node)],
            vec![
                EventMarker {
                    tick: 1,
                    key: "same".to_owned(),
                    effect: known(MarkerEffect::Gameplay {
                        cue: "a".to_owned()
                    }),
                },
                EventMarker {
                    tick: 1,
                    key: "same".to_owned(),
                    effect: known(MarkerEffect::Presentation {
                        cue: "b".to_owned()
                    }),
                },
            ],
        ),
        Err(ClipError::DuplicateMarkerKey {
            key: "same".to_owned()
        })
    );
    assert_eq!(
        build(
            4,
            vec![one_key_channel(&node)],
            vec![EventMarker {
                tick: 5,
                key: "late".to_owned(),
                effect: known(MarkerEffect::Gameplay {
                    cue: "a".to_owned()
                }),
            }],
        ),
        Err(ClipError::BeyondDuration {
            tick: 5,
            duration: 4
        })
    );
    assert_eq!(
        build(
            4,
            vec![one_key_channel(&node)],
            vec![
                EventMarker {
                    tick: 2,
                    key: "second".to_owned(),
                    effect: known(MarkerEffect::Gameplay {
                        cue: "a".to_owned()
                    }),
                },
                EventMarker {
                    tick: 1,
                    key: "first".to_owned(),
                    effect: known(MarkerEffect::Gameplay {
                        cue: "b".to_owned()
                    }),
                },
            ],
        ),
        Err(ClipError::MarkerTicks { tick: 1 })
    );
    assert_eq!(
        build(
            4,
            vec![one_key_channel(&node)],
            vec![EventMarker {
                tick: 1,
                key: String::new(),
                effect: known(MarkerEffect::Gameplay {
                    cue: "a".to_owned()
                }),
            }],
        ),
        Err(ClipError::EmptyMarkerKey)
    );
    assert_eq!(
        build(
            4,
            vec![one_key_channel(&node)],
            vec![EventMarker {
                tick: 1,
                key: "mute".to_owned(),
                effect: known(MarkerEffect::Gameplay { cue: String::new() }),
            }],
        ),
        Err(ClipError::EmptyMarkerCue {
            key: "mute".to_owned()
        })
    );
    assert_eq!(
        TransformSample::try_new(Quaternion::IDENTITY, [f64::NAN, 0.0, 0.0], [1.0, 1.0, 1.0]),
        Err(SpaceError::NonFinite {
            field: "translation_m[0]"
        })
    );
}
