//! The animation application boundary (F20-A, F20-B).
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-A`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! This module sits between the declared clip record
//! ([`cs_content::animation`]) and the fixed-tick evaluator
//! ([`cs_sim::animated_object`]), which cannot see each other — `cs_sim`
//! must not depend on `cs_content` (`docs/01-ARCHITECTURE.md`). It contains:
//!
//! * [`lower::lower_clip`] — the conversion boundary: a validated declared
//!   clip becomes a runtime [`cs_sim::animated_object::AnimatedClip`], with
//!   every [`Resolved::Unknown`] carried through so the runtime still blocks
//!   the transitions it gates (F20 non-negotiable behavior 2);
//! * [`presentation::interpolated_pose`] — the render-side pose sampler.
//!   It runs on fractional alpha between two committed tick poses and
//!   produces *no* events: interpolation changes presentation only, gameplay
//!   markers stay inside the fixed-tick evaluator (non-negotiable behavior
//!   1);
//! * [`AnimatedNodeBinding`] — the ECS binding record tying an entity to one
//!   animated node of one playing clip, generation-stamped like
//!   [`crate::scene::SceneNodeBinding`] so a reload can never leave a stale
//!   binding looking live.
//!
//! Stage `### F20-B` adds [`playback`], the fixed-tick playback that drives
//! both ends of that boundary inside a session: [`playback::play_animation`]
//! starts one lowered instance of a declared clip in the
//! [`playback::AnimationPlayback`] resource,
//! [`playback::advance_animation`] advances every instance once per
//! committed session tick, publishes its markers into the
//! [`playback::AnimationLog`] and applies the transform, material and
//! attachment tracks to the entities whose [`AnimatedNodeBinding`] verifies
//! (playing clip, live scene generation, driven node), and
//! [`playback::stop_animation`] ends an instance. The applied values are the
//! components [`playback::NodeAnimatedPose`],
//! [`playback::NodeAnimatedMaterial`] and
//! [`playback::NodeAnimatedAttachment`]; an unknown reference never becomes
//! a component — it is blocked and reported instead (F20 non-negotiable
//! behavior 2).
//!
//! The playback owns the animation state it applies; the records in this
//! module remain the ECS outputs the consumers bind.
//!
//! Stage `### F20-C` adds [`attachment`], the consumer half: the record
//! [`playback::NodeAnimatedAttachment`] becomes a real parent change —
//! [`attachment::apply_attachment_transitions`] inserts or removes `ChildOf`
//! with the authored pose policy, recomposes the world pose of the node's
//! descendants and gives a detached node the world velocity its parent had
//! at that tick, exactly once per change; and
//! [`attachment::release_attachments_before_despawn`] releases an animated
//! attachment before its parent goes away (non-negotiable behavior 4,
//! AC03).

use bevy::ecs::component::Component;
use cs_types::content::ContentId;

use crate::scene::SceneGeneration;

pub mod attachment;
pub mod lower;
pub mod playback;
pub mod presentation;

pub use attachment::{
    AppliedAttachment, AttachmentRecord, AttachmentRefusalReason, RefusedAttachment,
    VelocitySkipReason, apply_attachment_transitions, release_attachments_before_despawn,
};
pub use playback::{
    AnimationLog, AnimationPlayError, AnimationPlayback, AnimationRefusal, BlockedTrack,
    NodeAnimatedAttachment, NodeAnimatedMaterial, NodeAnimatedPose, TrackKind, advance_animation,
    play_animation, stop_animation,
};

/// Component: marks an entity as presenting one node of one playing clip.
///
/// `node` is the channel target's stable `scene_node` id and `clip` the
/// `animation_track` it is driven by; `generation` is the scene generation
/// the binding was spawned under, so a reload stamps new bindings and stale
/// ones are identified by mismatch rather than surviving pointers (F11/F20
/// session-generation ownership).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct AnimatedNodeBinding {
    /// The playing clip (`animation_track` id).
    pub clip: ContentId,
    /// The animated node (`scene_node` id).
    pub node: ContentId,
    /// The scene generation that spawned the binding.
    pub generation: SceneGeneration,
}
