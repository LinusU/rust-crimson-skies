//! The animation application boundary (F20-A).
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
//! Nothing here owns animation state: clip time, dedup and node states are
//! the evaluator's; these records are outputs bound into the ECS.

use bevy::ecs::component::Component;
use cs_types::content::ContentId;

use crate::scene::SceneGeneration;

pub mod lower;
pub mod presentation;

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
