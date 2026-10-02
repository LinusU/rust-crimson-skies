//! The spawn-side producer: binding a spawned scene node to a live animation
//! instance (F20-C).
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-C`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! F20-B built the playback and F20-C.01/.03 the consumers, but **nothing in
//! the crate produced an [`AnimatedNodeBinding`]**: the scene spawn path that
//! creates a node entity had no way to say "this entity presents this node of
//! this playing clip", so a real session could play an instance and still
//! drive no entity at all. This module is that producer.
//!
//! [`bind_animated_node`] is the one entry the spawn path calls, once per
//! animated node it spawns:
//!
//! 1. the node must belong to the declared clip (a channel targets it) — a
//!    binding to a node the clip does not drive would drive nothing forever,
//!    so it is refused with [`AnimatedNodeBindError::NodeNotDriven`] instead
//!    of written;
//! 2. the `(track, instance)` must be live: the clip is started through the
//!    same [`play_animation`](super::play_animation) boundary the tests use.
//!    A second node of the same instance is the ordinary multi-node case, so
//!    `AlreadyPlaying` for the very identity being bound is not an error — the
//!    binding is simply attached to the live instance. Every other
//!    [`AnimationPlayError`] propagates;
//! 3. the live instance must serve the generation the binding names: an
//!    instance left over from a superseded scene load is
//!    [`AnimatedNodeBindError::StaleInstance`], so a stale binding is refused
//!    at the producer rather than silently ignored by the next advance.
//!
//! The generation-stamped [`AnimatedNodeBinding`] is written only after all
//! three hold. Nothing here is original data: the spawn-side contract is
//! **designed**, and F20-D keeps the original-family validation gate.
//!
//! # The mission-marker consumer is still absent
//!
//! The other half of "wire it into its actual producer and consumer" — the
//! mission/objective layer that drains [`AnimationLog`](super::AnimationLog)
//! gameplay events — has no consumer in the crate yet (the mission and
//! objective layers are F37/F39 work). This stage does **not** stub one; the
//! seam is the log's [`drain`](super::AnimationLog::drain), recorded in
//! `docs/findings/2026-10-02-f20-c-wired-session-integration.md` and filed as a
//! follow-up.

use std::fmt;

use bevy::ecs::world::World;
use bevy::prelude::Entity;
use cs_content::animation::AnimationClip;
use cs_types::Tick;
use cs_types::content::ContentId;

use crate::scene::SceneGeneration;

use super::playback::{AnimationPlayError, AnimationPlayback, play_animation};
use super::{AnimatedNodeBinding, AnimationInstance};

/// Why the spawn path could not bind an entity to an animated node.
#[derive(Clone, Debug, PartialEq)]
pub enum AnimatedNodeBindError {
    /// The entity is not part of this world (never spawned or already gone).
    UnknownEntity(Entity),
    /// The declared clip has no channel on that node, so a binding would drive
    /// nothing at all.
    NodeNotDriven {
        /// The clip that was asked to drive the node.
        clip: ContentId,
        /// The node the clip does not drive.
        node: ContentId,
    },
    /// The playback boundary refused to start the instance.
    Play(AnimationPlayError),
    /// The `(track, instance)` is live, but serves another scene generation
    /// than the binding names — a superseded instance the load path should
    /// have released first.
    StaleInstance {
        /// The `animation_track` of the instance.
        clip: ContentId,
        /// The instance identity.
        instance: AnimationInstance,
        /// The scene generation the live instance serves.
        live: SceneGeneration,
        /// The scene generation the binding names.
        requested: SceneGeneration,
    },
    /// The playback claims no instance for the identity immediately after a
    /// successful start. Unreachable in practice; kept so a future drift
    /// between the start and the lookup is refused instead of binding an
    /// entity to an instance that is not there.
    InstanceMissing {
        /// The `animation_track` of the instance.
        clip: ContentId,
        /// The instance identity.
        instance: AnimationInstance,
    },
}

impl fmt::Display for AnimatedNodeBindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownEntity(entity) => {
                write!(f, "{entity} is not part of this world")
            }
            Self::NodeNotDriven { clip, node } => {
                write!(f, "{clip} drives no channel on {node}")
            }
            Self::Play(error) => write!(f, "{error}"),
            Self::StaleInstance {
                clip,
                instance,
                live,
                requested,
            } => write!(
                f,
                "{clip} as {instance} serves generation {} but the binding names {}",
                live.0, requested.0
            ),
            Self::InstanceMissing { clip, instance } => {
                write!(f, "{clip} as {instance} is not live after being started")
            }
        }
    }
}

impl std::error::Error for AnimatedNodeBindError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Play(source) => Some(source),
            _ => None,
        }
    }
}

impl From<AnimationPlayError> for AnimatedNodeBindError {
    fn from(error: AnimationPlayError) -> Self {
        Self::Play(error)
    }
}

/// Binds the scene entity `entity` to `node` of the live `(clip, instance)`,
/// starting that instance at session tick `at` when it is not already playing.
///
/// This is the producer the scene spawn path calls for each animated node it
/// spawns. It is deliberately **not** a system: the entity is created by the
/// scene loader, which knows the node id, the generation and the instance it
/// assigned, and this entry performs the one atomic step that ties the three
/// to a playing clip.
///
/// # Errors
///
/// * [`AnimatedNodeBindError::UnknownEntity`] when `entity` is not in `world`;
/// * [`AnimatedNodeBindError::NodeNotDriven`] when `clip` has no channel on
///   `node` (nothing would ever be written);
/// * [`AnimatedNodeBindError::Play`] when the playback boundary refuses the
///   start ([`AnimationPlayError::NoSession`] with no
///   [`AnimationPlayback`] in the world, [`AnimationPlayError::Lower`] for a
///   clip that does not survive the boundary, or
///   [`AnimationPlayError::ProducerExhausted`]);
/// * [`AnimatedNodeBindError::StaleInstance`] when the identity is already
///   live under a different scene generation.
pub fn bind_animated_node(
    world: &mut World,
    clip: &AnimationClip,
    entity: Entity,
    node: &ContentId,
    instance: AnimationInstance,
    generation: SceneGeneration,
    at: Tick,
) -> Result<(), AnimatedNodeBindError> {
    if world.get_entity(entity).is_err() {
        return Err(AnimatedNodeBindError::UnknownEntity(entity));
    }

    let track = clip.id().clone();
    if !clip
        .channels()
        .iter()
        .any(|channel| channel.target().as_content_id() == node)
    {
        return Err(AnimatedNodeBindError::NodeNotDriven {
            clip: track,
            node: node.clone(),
        });
    }

    // Start the instance unless this exact identity already plays. A clip that
    // drives several nodes is bound node by node, so the second call is the
    // ordinary multi-node case and not a duplicate start.
    match play_animation(world, clip, instance, generation, at) {
        Ok(()) | Err(AnimationPlayError::AlreadyPlaying { .. }) => {}
        Err(error) => return Err(AnimatedNodeBindError::Play(error)),
    }

    // The instance is live now; it must serve the generation this binding
    // names, or the next advance would silently ignore the binding.
    match world
        .get_resource::<AnimationPlayback>()
        .and_then(|playback| playback.generation(&track, instance))
    {
        Some(live) if live == generation => {}
        Some(live) => {
            return Err(AnimatedNodeBindError::StaleInstance {
                clip: track,
                instance,
                live,
                requested: generation,
            });
        }
        None => {
            return Err(AnimatedNodeBindError::InstanceMissing {
                clip: track,
                instance,
            });
        }
    }

    world.entity_mut(entity).insert(AnimatedNodeBinding {
        clip: track,
        node: node.clone(),
        instance,
        generation,
    });
    Ok(())
}
