//! The declared-clip → runtime-clip conversion boundary (F20-A).
//!
//! A [`cs_content::animation::AnimationClip`] is the normalized,
//! provenance-carrying record; [`lower_clip`] produces the
//! [`cs_sim::animated_object::AnimatedClip`] the fixed-tick evaluator plays.
//! The mapping is structural: node ids shed their [`SceneNodeId`] wrapper
//! into plain `scene_node` [`ContentId`]s, samples and vocabularies map
//! field-wise, and every [`Resolved::Unknown`] is carried through verbatim —
//! an unknown material, attachment parent or marker effect must reach the
//! runtime still unknown so it blocks the transition it gates instead of
//! being repaired at the boundary (F20 non-negotiable behavior 2).
//!
//! The runtime re-validates the assembled record
//! ([`AnimatedClip::try_new`]), so a declared clip that somehow survived its
//! own boundary with a defect is still refused here.

use cs_content::animation::{
    AnimationChannel, AnimationClip, AttachmentOp, Interpolation, LoopMode, MarkerEffect,
    PosePolicy,
};
use cs_content::scene::{NodeVisibility, SceneNodeId};
use cs_sim::animated_object::{
    AnimatedClip, AnimationError, AttachmentKey as RuntimeAttachmentKey,
    AttachmentOp as RuntimeAttachmentOp, ClipMarker, Interpolation as RuntimeInterpolation,
    LoopMode as RuntimeLoopMode, MarkerEffect as RuntimeMarkerEffect, MaterialKey, NodeChannel,
    PosePolicy as RuntimePosePolicy, PoseSample, TransformKey, Visibility, VisibilityKey,
};
use cs_types::content::{ContentId, Known, Resolved};
use cs_types::space::SpaceError;

/// Why a declared clip could not be lowered to the runtime record.
#[derive(Clone, Debug, PartialEq)]
pub enum LowerError {
    /// A declared transform sample failed the runtime's finite check —
    /// unreachable for a clip that passed [`AnimationClip::try_new`], kept
    /// so the boundary stays honest if the declared record ever loosens.
    Pose(SpaceError),
    /// The runtime refused the assembled record.
    Runtime(AnimationError),
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pose(source) => write!(f, "invalid declared pose sample: {source}"),
            Self::Runtime(source) => write!(f, "the runtime refused the lowered clip: {source}"),
        }
    }
}

impl std::error::Error for LowerError {}

/// Lowers one declared clip into the runtime record the evaluator plays.
///
/// Unknown references are preserved as [`Resolved::Unknown`]; nothing is
/// resolved, guessed or repaired at this boundary.
///
/// # Errors
///
/// [`LowerError::Pose`] when a transform sample fails the runtime's
/// validation, [`LowerError::Runtime`] when [`AnimatedClip::try_new`]
/// refuses the assembled record.
pub fn lower_clip(clip: &AnimationClip) -> Result<AnimatedClip, LowerError> {
    let channels = clip
        .channels()
        .iter()
        .map(lower_channel)
        .collect::<Result<Vec<_>, _>>()?;
    let markers = clip
        .markers()
        .iter()
        .map(|marker| ClipMarker {
            tick: marker.tick,
            key: marker.key.clone(),
            effect: lower_effect(&marker.effect),
        })
        .collect();
    AnimatedClip::try_new(
        clip.id().clone(),
        clip.duration_ticks(),
        lower_loop_mode(clip.loop_mode()),
        channels,
        markers,
    )
    .map_err(LowerError::Runtime)
}

/// Maps a declared channel to its runtime form.
fn lower_channel(channel: &AnimationChannel) -> Result<NodeChannel, LowerError> {
    Ok(match channel {
        AnimationChannel::Transform(channel) => NodeChannel::Transform {
            target: node_id(&channel.target),
            interpolation: match channel.interpolation {
                Interpolation::Step => RuntimeInterpolation::Step,
                Interpolation::Linear => RuntimeInterpolation::Linear,
            },
            keys: channel
                .keys
                .iter()
                .map(|key| {
                    Ok(TransformKey {
                        tick: key.tick,
                        pose: PoseSample::try_new(
                            key.pose.rotation(),
                            key.pose.translation_m(),
                            key.pose.scale(),
                        )
                        .map_err(LowerError::Pose)?,
                    })
                })
                .collect::<Result<Vec<_>, LowerError>>()?,
        },
        AnimationChannel::Visibility(channel) => NodeChannel::Visibility {
            target: node_id(&channel.target),
            keys: channel
                .keys
                .iter()
                .map(|key| VisibilityKey {
                    tick: key.tick,
                    visibility: match key.visibility {
                        NodeVisibility::Visible => Visibility::Visible,
                        NodeVisibility::Hidden => Visibility::Hidden,
                    },
                })
                .collect(),
        },
        AnimationChannel::Material(channel) => NodeChannel::Material {
            target: node_id(&channel.target),
            keys: channel
                .keys
                .iter()
                .map(|key| MaterialKey {
                    tick: key.tick,
                    material: key.material.clone(),
                })
                .collect(),
        },
        AnimationChannel::Attachment(channel) => NodeChannel::Attachment {
            target: node_id(&channel.target),
            keys: channel
                .keys
                .iter()
                .map(|key| RuntimeAttachmentKey {
                    tick: key.tick,
                    op: match &key.op {
                        AttachmentOp::Attach { parent, pose } => RuntimeAttachmentOp::Attach {
                            parent: Box::new(lower_node_ref(parent)),
                            pose: lower_pose_policy(*pose),
                        },
                        AttachmentOp::Detach { pose } => RuntimeAttachmentOp::Detach {
                            pose: lower_pose_policy(*pose),
                        },
                    },
                })
                .collect(),
        },
    })
}

/// The runtime pose policy of a declared one.
fn lower_pose_policy(pose: PosePolicy) -> RuntimePosePolicy {
    match pose {
        PosePolicy::KeepWorldPose => RuntimePosePolicy::KeepWorldPose,
        PosePolicy::KeepLocalPose => RuntimePosePolicy::KeepLocalPose,
    }
}

/// The runtime loop mode of a declared one.
fn lower_loop_mode(mode: LoopMode) -> RuntimeLoopMode {
    match mode {
        LoopMode::Once => RuntimeLoopMode::Once,
        LoopMode::Loop => RuntimeLoopMode::Loop,
    }
}

/// A node reference without its [`SceneNodeId`] wrapper, keeping the
/// resolved state — unknown stays unknown with its claim and reason.
fn lower_node_ref(resolved: &Resolved<SceneNodeId>) -> Resolved<ContentId> {
    match resolved {
        Resolved::Known(known) => Resolved::Known(Known::new(
            known.value.as_content_id().clone(),
            known.provenance.clone(),
        )),
        Resolved::Unknown { claim_id, reason } => Resolved::Unknown {
            claim_id: claim_id.clone(),
            reason: reason.clone(),
        },
    }
}

/// The runtime effect of a declared one, keeping the resolved state.
fn lower_effect(effect: &Resolved<MarkerEffect>) -> Resolved<RuntimeMarkerEffect> {
    match effect {
        Resolved::Known(known) => Resolved::Known(Known::new(
            match &known.value {
                MarkerEffect::Gameplay { cue } => {
                    RuntimeMarkerEffect::Gameplay { cue: cue.clone() }
                }
                MarkerEffect::Presentation { cue } => {
                    RuntimeMarkerEffect::Presentation { cue: cue.clone() }
                }
            },
            known.provenance.clone(),
        )),
        Resolved::Unknown { claim_id, reason } => Resolved::Unknown {
            claim_id: claim_id.clone(),
            reason: reason.clone(),
        },
    }
}

/// A scene node's stable id as a plain content id.
fn node_id(node: &SceneNodeId) -> ContentId {
    node.as_content_id().clone()
}
