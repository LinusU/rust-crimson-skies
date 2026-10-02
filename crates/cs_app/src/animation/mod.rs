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
//!
//! Stage `### F20-C.02` adds [`schedule`], the **producer wiring**: the
//! committed session tick arrives as the
//! [`schedule::CommittedSessionTick`] resource the session driver writes, and
//! [`schedule::advance_animation_on_session_tick`] (installed by
//! [`schedule::AnimationSchedulePlugin`], in `FixedPostUpdate` after the
//! physics step) advances the playback **once per committed tick change** —
//! nothing at all without that stamp, and nothing for a repeated one.
//! [`schedule::release_superseded_instances`] is the teardown half: a scene
//! load that superseded an instance's generation releases what that instance
//! applied, for its own entities only.

use std::fmt;

use bevy::ecs::component::Component;
use cs_types::content::ContentId;

use crate::scene::SceneGeneration;

pub mod attachment;
pub mod lower;
pub mod playback;
pub mod presentation;
pub mod schedule;

pub use attachment::{
    AppliedAttachment, AttachmentRecord, AttachmentRefusalReason, RefusedAttachment,
    VelocitySkipReason, apply_attachment_transitions, release_animated_attachment,
    release_attachments_before_despawn,
};
pub use playback::{
    AnimationLog, AnimationPlayError, AnimationPlayback, AnimationRefusal, BlockedTrack,
    InstanceKey, NodeAnimatedAttachment, NodeAnimatedMaterial, NodeAnimatedPose, TrackKind,
    advance_animation, play_animation, stop_animation,
};
pub use schedule::{
    AnimationSchedulePlugin, CommittedSessionTick, advance_animation_on_session_tick,
    release_superseded_instances,
};

/// The identity of one live instance of an `animation_track`.
///
/// Several entities may play **one** track as separate instances — two
/// aircraft each spin a propeller hub with the same authored clip — so the
/// track id alone does not name a playback instance, and neither does an
/// entity id (the spawn wiring, not the playback, owns entity identity). The
/// spawn wiring assigns one instance per animated node it spawns and every
/// [`AnimatedNodeBinding`] it writes names that instance, so
/// [`AnimationPlayback`](playback::AnimationPlayback) can key its live map by
/// (track, instance) and give each instance its own evaluator, its own applied
/// state and its own event ids.
///
/// A validated nonzero number, like the `SessionId`/`PeerId` of
/// `docs/contracts/IDENTITY-CONTENT.md`: zero never names a live instance, so a
/// default-constructed identity cannot alias one. **Designed** — no original
/// data carries an instance identity; see
/// `docs/findings/2026-10-02-f20-c-02-fixed-tick-instances-and-teardown.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnimationInstance(u32);

impl AnimationInstance {
    /// Wraps an assigned instance number; zero is refused.
    #[must_use]
    pub const fn new(value: u32) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    /// The assigned number.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for AnimationInstance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "instance {}", self.0)
    }
}

/// Component: marks an entity as presenting one node of one playing clip.
///
/// `node` is the channel target's stable `scene_node` id, `clip` the
/// `animation_track` it is driven by and `instance` which live instance of
/// that track it belongs to; `generation` is the scene generation the binding
/// was spawned under, so a reload stamps new bindings and stale ones are
/// identified by mismatch rather than surviving pointers (F11/F20
/// session-generation ownership).
///
/// The instance is what keeps one track's instances apart: two entities bound
/// to the same track under different instances each receive their own
/// evaluated state, and a teardown of one instance leaves the other untouched.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct AnimatedNodeBinding {
    /// The playing clip (`animation_track` id).
    pub clip: ContentId,
    /// The animated node (`scene_node` id).
    pub node: ContentId,
    /// Which live instance of `clip` this entity presents.
    pub instance: AnimationInstance,
    /// The scene generation that spawned the binding.
    pub generation: SceneGeneration,
}
