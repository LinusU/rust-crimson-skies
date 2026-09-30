//! Acceptance scenarios F20-B on the content side: the declared fixtures the
//! playback is driven with carry exactly the tracks this stage applies.
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-B`. Task test prefix: `accept_f20_b_`.
//!
//! The fixtures are production records — [`AnimationClip::try_new`] validates
//! them like any other declared clip, and the playback consumes them through
//! `cs_app::animation::lower::lower_clip`. The tests pin their identity,
//! provenance, loop semantics and every authored key so a silent change to a
//! fixture (a retimed marker, a swapped material, a dropped attachment) fails
//! instead of shifting the acceptance scenarios that depend on it.
//!
//! Every value here is newly authored fixture data, not measured original
//! game data.

use cs_content::animation::{
    AnimationChannel, AnimationClip, AttachmentOp, ClipError, Interpolation, LoopMode,
    SYNTHETIC_CARGO_ATTACH_TICK, SYNTHETIC_CARGO_BAY_NODE, SYNTHETIC_CARGO_DETACH_TICK,
    SYNTHETIC_CARGO_DURATION, SYNTHETIC_CARGO_MATERIAL, SYNTHETIC_CARGO_MATERIAL_TICK,
    SYNTHETIC_CARGO_NODE, SYNTHETIC_CARGO_SCORCH_TICK, SYNTHETIC_CARGO_SCORCHED_MATERIAL,
    SYNTHETIC_PROPELLER_DURATION, SYNTHETIC_PROPELLER_GAMEPLAY_MARKER,
    SYNTHETIC_PROPELLER_GAMEPLAY_TICK, SYNTHETIC_PROPELLER_NODE,
    SYNTHETIC_PROPELLER_PRESENTATION_MARKER, SYNTHETIC_PROPELLER_PRESENTATION_TICK,
    declared_synthetic_cargo_clip, declared_synthetic_propeller_clip,
};
use cs_types::content::{ContentId, ContentKind, Origin, Resolved};
use cs_types::space::{Quaternion, Radians, UnitVec3};

fn content_id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("valid content id")
}

fn cue_of(clip: &AnimationClip, index: usize) -> String {
    clip.markers()[index]
        .effect
        .clone()
        .known()
        .expect("the fixture marker is a known effect")
        .cue()
        .to_owned()
}

/// The propeller fixture is a looping transform track with one presentation
/// and one one-shot gameplay marker — the exact shape AC02 needs.
#[test]
fn accept_f20_b_propeller_fixture_is_a_looping_track_with_two_markers() {
    let clip = declared_synthetic_propeller_clip();

    assert_eq!(
        clip.id(),
        &content_id(ContentKind::AnimationTrack, "synthetic.propeller")
    );
    assert_eq!(clip.origin(), &Origin::SyntheticFixture);
    assert_eq!(clip.loop_mode(), LoopMode::Loop);
    assert_eq!(clip.duration_ticks(), SYNTHETIC_PROPELLER_DURATION);
    assert_eq!(
        clip.provenance().class,
        cs_types::evidence::ClaimStatus::Designed,
        "the fixture is designed content"
    );

    let [AnimationChannel::Transform(channel)] = clip.channels() else {
        panic!("the propeller drives exactly one transform channel");
    };
    assert_eq!(channel.target.key(), SYNTHETIC_PROPELLER_NODE);
    assert_eq!(channel.interpolation, Interpolation::Step);
    assert_eq!(
        channel.keys.iter().map(|key| key.tick).collect::<Vec<_>>(),
        (0..SYNTHETIC_PROPELLER_DURATION).collect::<Vec<_>>(),
        "one key per tick of the pass"
    );
    for key in &channel.keys {
        let expected = Quaternion::from_axis_angle(
            UnitVec3::FORWARD,
            Radians(f64::from(key.tick as u8) * std::f64::consts::FRAC_PI_2),
        )
        .expect("a quarter turn is unit length");
        assert_eq!(key.pose.rotation(), expected, "a quarter turn per tick");
        assert_eq!(key.pose.translation_m(), [0.0, 0.0, 0.0]);
        assert_eq!(key.pose.scale(), [1.0, 1.0, 1.0]);
    }

    assert_eq!(clip.markers().len(), 2);
    assert_eq!(
        clip.markers()[0].tick,
        SYNTHETIC_PROPELLER_PRESENTATION_TICK
    );
    assert_eq!(
        clip.markers()[0].key,
        SYNTHETIC_PROPELLER_PRESENTATION_MARKER
    );
    assert_eq!(cue_of(&clip, 0), "synthetic.plane.blade_pass");
    assert_eq!(clip.markers()[1].tick, SYNTHETIC_PROPELLER_GAMEPLAY_TICK);
    assert_eq!(clip.markers()[1].key, SYNTHETIC_PROPELLER_GAMEPLAY_MARKER);
    assert_eq!(cue_of(&clip, 1), "synthetic.plane.engine_started");
}

/// The cargo fixture carries the two `Resolved` track kinds this stage
/// applies, with the authored ticks, the authored parent and the authored
/// pose policies.
#[test]
fn accept_f20_b_cargo_fixture_carries_material_and_attachment_tracks() {
    let clip = declared_synthetic_cargo_clip();

    assert_eq!(
        clip.id(),
        &content_id(ContentKind::AnimationTrack, "synthetic.cargo")
    );
    assert_eq!(clip.origin(), &Origin::SyntheticFixture);
    assert_eq!(clip.loop_mode(), LoopMode::Once);
    assert_eq!(clip.duration_ticks(), SYNTHETIC_CARGO_DURATION);
    assert!(
        clip.markers().is_empty(),
        "this fixture only carries tracks"
    );

    let (
        Some(AnimationChannel::Material(material)),
        Some(AnimationChannel::Attachment(attachment)),
    ) = (clip.channels().first(), clip.channels().get(1))
    else {
        panic!("a material channel then an attachment channel");
    };
    assert_eq!(clip.channels().len(), 2);

    assert_eq!(material.target.key(), SYNTHETIC_CARGO_NODE);
    assert_eq!(
        material.keys.iter().map(|key| key.tick).collect::<Vec<_>>(),
        vec![SYNTHETIC_CARGO_MATERIAL_TICK, SYNTHETIC_CARGO_SCORCH_TICK]
    );
    let applied: Vec<String> = material
        .keys
        .iter()
        .map(|key| match &key.material {
            Resolved::Known(known) => {
                assert_eq!(known.value.kind(), ContentKind::Material);
                known.value.key().to_owned()
            }
            Resolved::Unknown { .. } => panic!("the fixture material is known"),
        })
        .collect();
    assert_eq!(
        applied,
        vec![
            SYNTHETIC_CARGO_MATERIAL.to_owned(),
            SYNTHETIC_CARGO_SCORCHED_MATERIAL.to_owned()
        ]
    );

    assert_eq!(attachment.target.key(), SYNTHETIC_CARGO_NODE);
    assert_eq!(
        attachment
            .keys
            .iter()
            .map(|key| key.tick)
            .collect::<Vec<_>>(),
        vec![SYNTHETIC_CARGO_ATTACH_TICK, SYNTHETIC_CARGO_DETACH_TICK]
    );
    match &attachment.keys[0].op {
        AttachmentOp::Attach { parent, pose } => {
            match parent.as_ref() {
                Resolved::Known(known) => assert_eq!(
                    known.value.key(),
                    SYNTHETIC_CARGO_BAY_NODE,
                    "the cargo attaches under the authored bay node"
                ),
                Resolved::Unknown { .. } => panic!("the fixture parent is known"),
            }
            assert_eq!(*pose, cs_content::animation::PosePolicy::KeepLocalPose);
        }
        AttachmentOp::Detach { .. } => panic!("the first key attaches"),
    }
    match &attachment.keys[1].op {
        AttachmentOp::Detach { pose } => {
            assert_eq!(*pose, cs_content::animation::PosePolicy::KeepWorldPose);
        }
        AttachmentOp::Attach { .. } => panic!("the second key detaches"),
    }
}

/// The failure case: a `material` key that names anything but a material is
/// refused at the content boundary, so a wrong-kind reference never reaches
/// lowering or the playback.
#[test]
fn accept_f20_b_a_material_key_naming_another_kind_is_refused() {
    let node = content_id(ContentKind::SceneNode, SYNTHETIC_CARGO_NODE);
    let wrong = AnimationClip::try_new(
        content_id(ContentKind::AnimationTrack, "synthetic.wrong_material"),
        Origin::SyntheticFixture,
        4,
        LoopMode::Once,
        vec![AnimationChannel::Material(
            cs_content::animation::MaterialChannel {
                target: cs_content::scene::SceneNodeId::from_content_id(node.clone())
                    .expect("the key names a scene node"),
                keys: vec![cs_content::animation::MaterialKey {
                    tick: 0,
                    material: Resolved::Known(cs_types::content::Known::new(
                        node.clone(),
                        cs_types::content::Provenance::designed(
                            cs_types::evidence::ClaimId::new("f20b.test")
                                .expect("a valid claim id"),
                        ),
                    )),
                }],
            },
        )],
        Vec::new(),
        cs_types::content::Provenance::designed(
            cs_types::evidence::ClaimId::new("f20b.test").expect("a valid claim id"),
        ),
    );

    assert_eq!(
        wrong,
        Err(ClipError::MaterialKind {
            target: SYNTHETIC_CARGO_NODE.to_owned(),
            material: ContentKind::SceneNode,
        })
    );
}
