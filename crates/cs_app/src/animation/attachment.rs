//! The attachment consumer: an evaluated attachment record becomes a real
//! parent change (F20-C, non-negotiable behavior 4, AC03).
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-C`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! Stage `### F20-B` published [`NodeAnimatedAttachment`] — *that* the clip
//! wants this node under that parent, with which
//! [`PosePolicy`](cs_sim::animated_object::PosePolicy) — and stopped there.
//! [`apply_attachment_transitions`] is the consumer that turns the record
//! into the hierarchy change itself; [`super::playback::advance_animation`] runs it
//! at the end of every fixed tick, so the transition is applied in the same
//! tick the evaluator reached its key:
//!
//! * **Attach** resolves the parent's `scene_node` id through
//!   [`SceneNodeBinding`] **inside the binding's scene generation** and
//!   inserts [`ChildOf`]; **detach** removes it.
//! * The authored pose policy decides which pose survives: with
//!   `KeepWorldPose` the node's composed world affine
//!   ([`NodeVisualTransform`], the one pose owner) is kept and the
//!   parent-relative pose is recomputed from it; with `KeepLocalPose` the
//!   parent-relative pose is kept and the world affine follows the new
//!   parent. Afterwards the world affine of the node's **descendants** is
//!   recomposed from their own parent-relative poses, so a parent change
//!   never leaves a child with a stale world affine (the subtree walk
//!   F20-B's findings handed to F20-C as boundary 1).
//! * A **detach** inherits the world velocity the parent had at that tick —
//!   `v + ω × r` linearly, the parent's `ω` angularly — written only onto
//!   velocity components the entity already carries, and **exactly once per
//!   detach**. The `ω × r` term is only measured when the chain really spins
//!   (it is exactly zero otherwise, however unmeasurable the offset is).
//!   Which ancestor the values come from, and what happens when none carries
//!   any, is written down in
//!   `docs/findings/2026-09-30-f20-c-01-attachment-hierarchy-and-detach-velocity.md`
//!   and implemented in the velocity step below: nothing is ever invented.
//!
//! # Verification and error propagation
//!
//! The consumer attempts a transition only for an entity whose
//! [`AnimatedNodeBinding`] verifies: a **playing** instance of that track, a
//! node that instance **drives**, and a [`SceneNodeBinding`] naming that node
//! of that generation. Everything else keeps its state silently — the record
//! of a stopped instance is a leftover its teardown releases
//! (F20-C.02, [`release_animated_attachment`]).
//!
//! What is verified but cannot be applied is **refused instead of guessed**:
//! a parent id that resolves to no live entity of that generation (or to
//! more than one), a binding the live instance no longer serves, and a node
//! whose composed world pose is missing all publish one
//! [`AttachmentRecord::Refused`] and change nothing — never a half-reparent.
//! One refusal is published **once per (state, reason)** and kept in
//! [`RefusedAttachment`], while the lookup itself is retried every tick, so
//! a parent that appears later is still found and the log never grows one
//! entry per frame.
//!
//! # Idempotence
//!
//! [`AppliedAttachment`] records the transition that was applied. The
//! evaluated state is re-published every tick, so the consumer acts only
//! while the two differ: running the same advance again performs no second
//! transition, and the inherited velocity cannot be added twice (F20
//! non-negotiable behavior 3).
//!
//! # Releasing an attachment before a despawn
//!
//! [`release_attachments_before_despawn`] is the rule non-negotiable
//! behavior 4 states — "release attachments before despawning parents" — for
//! whoever despawns a parent. It releases the animated attachments anywhere in
//! the subtree that despawn takes, keeps their composed world pose by
//! construction, and inherits the departing parent's velocity by the same rule
//! an authored detach uses.
//!
//! [`release_animated_attachment`] is the same release for **one** named
//! entity, which is what the F20-C.02 instance teardown needs: a stopped
//! instance unparents what it applied, for its own entities only, before the
//! caller despawns anything.

use std::collections::{HashMap, HashSet, VecDeque};

use avian3d::prelude::{AngularVelocity, LinearVelocity, Position};
use bevy::ecs::component::Component;
use bevy::ecs::world::World;
use bevy::math::{Affine3A, Mat4};
use bevy::prelude::{ChildOf, Entity, GlobalTransform, Vec3};
use cs_sim::animated_object::{AttachmentState, PosePolicy};
use cs_types::content::{ContentId, Resolved};

use crate::scene::{NodeVisualTransform, SceneGeneration, SceneNodeBinding};

use super::AnimatedNodeBinding;
use super::playback::{AnimationLog, AnimationPlayback, NodeAnimatedAttachment};

/// The live entities of one generation that present one `scene_node` id,
/// keyed by `(node, generation)`.
///
/// A key with more than one entity is ambiguous and is refused rather than
/// resolved by picking one (`IDENTITY-CONTENT`: multiple equal-priority
/// candidates fail visibly).
type SceneNodes = HashMap<(ContentId, SceneGeneration), Vec<Entity>>;

// ----------------------------------------------------------- bookkeeping ---

/// Component: the attachment transition the consumer applied to this entity.
///
/// It is what makes the transition **once per change**: the evaluated
/// [`AttachmentState`](cs_sim::animated_object::AttachmentState) is
/// re-published every tick, and the consumer only acts while it differs from
/// this record — so the inherited velocity of a detach is written on the
/// detach tick and never again (F20 non-negotiable behavior 3).
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct AppliedAttachment {
    /// The parent the transition left the node under; `None` after a
    /// detach.
    pub parent: Option<ContentId>,
    /// The pose policy of that transition.
    pub pose: PosePolicy,
}

/// Component: the transition the consumer refused, with the reason it
/// published.
///
/// A refusal is a *state* (a parent id that resolves to nothing lasts as
/// long as it resolves to nothing), so it is published once per
/// (state, reason) and remembered here — while the lookup itself is retried
/// every tick, which is what lets a parent that appears later still attach.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct RefusedAttachment {
    /// The parent the refused transition asked for.
    pub parent: Option<ContentId>,
    /// The pose policy it would have applied.
    pub pose: PosePolicy,
    /// Why nothing was applied.
    pub reason: AttachmentRefusalReason,
}

/// Why an attachment transition was refused instead of applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttachmentRefusalReason {
    /// The live instance of the clip serves another scene generation than
    /// the one the binding is stamped with: the record belongs to a scene
    /// load the session superseded.
    StaleBinding {
        /// The generation the live instance serves.
        serving: SceneGeneration,
    },
    /// The entity's scene binding carries another generation than its
    /// animated binding: it is not the scene node that generation drives.
    StaleScene {
        /// The generation the entity's scene binding carries.
        scene: SceneGeneration,
    },
    /// The parent's `scene_node` id names no live entity of this binding's
    /// generation.
    UnknownParent,
    /// More than one live entity of this generation presents that parent id,
    /// so no single parent can be chosen.
    AmbiguousParent {
        /// How many entities the id resolved to.
        count: usize,
    },
    /// The parent the record asked for *is* the node, or one of its own
    /// descendants, so applying it would make the node its own ancestor.
    ///
    /// Cycles in an ownership/parent hierarchy are invalid
    /// (`docs/contracts/IDENTITY-CONTENT.md`: *"cycles in ownership/parent
    /// hierarchies are invalid"*), and the subtree walk behind every pose
    /// recomposition loops over one forever — so the transition is refused
    /// instead of creating it. The same reason covers an ancestor chain that
    /// already loops: the hierarchy is already invalid and is not made worse.
    CyclicParent,
    /// The node itself carries no composed world pose, so its pose policy
    /// cannot be honored.
    NodeWorldPoseMissing,
    /// The parent the node is joining — or the one it is leaving — carries
    /// no composed world pose, so the parent-relative pose cannot be
    /// computed.
    ParentWorldPoseMissing,
}

/// Why a detach published no inherited velocity.
///
/// The detach itself still happened: the node reparented and its pose was
/// preserved. Only the velocity is missing, and it is reported instead of
/// being made up (F20 non-negotiable behavior 4, "never an invented
/// velocity").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VelocitySkipReason {
    /// The node had no parent at the detach tick, so there was nothing to
    /// inherit from.
    NoParent,
    /// No ancestor of the parent being left carries a world velocity
    /// component at all.
    NoVelocitySource,
    /// The chain carries an angular velocity but no linear one, so `ω × r`
    /// has no linear reference to be added to.
    NoLinearSource,
    /// The linear source carries no world position, so the offset `r` from
    /// its reference point to the node cannot be measured.
    NoReferencePoint,
    /// The detaching node itself carries no world position, so the other end
    /// of the offset `r` cannot be measured. Only the `ω × r` term needs it:
    /// a chain that does not spin still inherits its linear source exactly.
    NoNodeReferencePoint,
}

/// One attachment publication: a transition that changed nothing, or a
/// detach that inherited no velocity.
///
/// Appended to [`AnimationLog`] once per transition, never once per frame —
/// the log grows with publications, not with ticks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttachmentRecord {
    /// The transition was refused; nothing was applied.
    Refused {
        /// The playing clip whose record was refused.
        clip: ContentId,
        /// The animated node the record belongs to.
        node: ContentId,
        /// The parent the record asked for (`None` for a detach).
        parent: Option<ContentId>,
        /// The pose policy it would have applied.
        pose: PosePolicy,
        /// Why nothing was applied.
        reason: AttachmentRefusalReason,
    },
    /// A detach that reparented and kept its pose but inherited no velocity.
    VelocityNotInherited {
        /// The playing clip the detach belongs to.
        clip: ContentId,
        /// The animated node that detached.
        node: ContentId,
        /// Why no velocity was written.
        reason: VelocitySkipReason,
    },
}

// ------------------------------------------------------- entry point -------

/// Applies every verified attachment record that has not been applied yet.
///
/// This is the consumer half of the F20-B playback: it is called at the end
/// of [`super::playback::advance_animation`], so a transition is applied in the
/// same fixed tick the evaluator reached its key, and it is directly
/// callable for a teardown that must release an attachment by hand.
///
/// It is idempotent by construction: the record that was applied is kept on
/// the entity, so a second call over the same state performs no second
/// transition, writes no pose and adds no velocity. Everything it refuses or
/// cannot inherit is appended to the [`AnimationLog`] as one
/// [`AttachmentRecord`] per transition.
pub fn apply_attachment_transitions(world: &mut World) {
    let records = transition(world);
    if !records.is_empty() {
        let mut log = world.remove_resource::<AnimationLog>().unwrap_or_default();
        log.push_attachments(records);
        world.insert_resource(log);
    }
}

/// Releases every animated attachment `parent` carries, before it is
/// despawned (F20 non-negotiable behavior 4: "release attachments before
/// despawning parents").
///
/// The children released are the ones whose attachment this consumer
/// manages: a child carrying [`AppliedAttachment`] (the consumer
/// applied a transition for it) or [`NodeAnimatedAttachment`] (the clip
/// records an attachment for it). Each is released on its own by
/// [`release_animated_attachment`], so the composed world pose is preserved
/// by construction — it is simply not touched, so nothing jumps when a parent
/// disappears — and the departing parent's velocity is inherited exactly once
/// by the rule an authored detach uses. A child the animation never touched
/// keeps its authored `ChildOf` and stays part of the parent's subtree — that
/// link belongs to the scene graph, not to the animation.
///
/// The walk covers the **whole subtree** the despawn reaches, not one level:
/// `despawn` is recursive over `Children` (measured below), so an animated
/// attachment below a child the animation never touched would otherwise die
/// with a parent it was never attached to. The walk stops at the children it
/// releases — a released child survives together with its own subtree, so
/// unparenting anything under it would only sever a link that was never in
/// danger — and descends only through children that stay linked and will go
/// with `parent`.
///
/// Returns the entities that were released, in hierarchy order (ancestors
/// before descendants). Publishing an [`AttachmentRecord`] needs the
/// release's animated identity, so only a released entity that still carries
/// its [`AnimatedNodeBinding`] contributes one.
///
/// The Bevy `despawn` behavior this rule exists for was measured on the
/// pinned Bevy 0.19 rather than assumed; the measurement and its assertion
/// live in `crates/cs_app/tests/accept_f20_c_01_attachment_hierarchy.rs`.
pub fn release_attachments_before_despawn(world: &mut World, parent: Entity) -> Vec<Entity> {
    let mut released = Vec::new();
    let mut records = Vec::new();

    let mut pending = VecDeque::from([parent]);
    while let Some(current) = pending.pop_front() {
        let children: Vec<Entity> = {
            let mut query = world.query_filtered::<(Entity, &ChildOf), ()>();
            query
                .iter(world)
                .filter(|(_, child_of)| child_of.parent() == current)
                .map(|(entity, _)| entity)
                .collect()
        };
        for child in children {
            if !is_managed(world, child) {
                // The animation never touched this link, so it belongs to the
                // scene graph and goes down with the parent.
                pending.push_back(child);
                continue;
            }
            // A managed child that carries no link has nothing to release;
            // it is treated as an untouched scene link and the walk descends
            // through it, so nothing below it is missed.
            let Some(published) = release_animated_attachment(world, child) else {
                pending.push_back(child);
                continue;
            };
            records.extend(published);
            released.push(child);
        }
    }

    if !records.is_empty() {
        let mut log = world.remove_resource::<AnimationLog>().unwrap_or_default();
        log.push_attachments(records);
        world.insert_resource(log);
    }
    released
}

/// Releases the animated attachment this consumer applied to **one** entity,
/// by the rule an authored detach uses.
///
/// The link goes, the composed world pose stays, the departing parent's world
/// velocity is inherited, and the applied record becomes a detach so the
/// clip's own detach finds it and does not inherit twice. Returns what it
/// published, or `None` when there was no animation-managed link to release:
/// an entity with no [`ChildOf`], or one this consumer never touched, is left
/// exactly as it is and nothing is published.
///
/// This is the one-entity form the F20-C.02 instance teardown uses. A stopped
/// instance must unparent what it applied **before** the caller's despawn
/// removes the parent (non-negotiable behavior 4), and
/// `docs/findings/2026-09-30-f20-c-01-attachment-hierarchy-and-detach-velocity.md`
/// requires the release and the despawn to happen in one step — which the
/// teardown guarantees by taking the instance out of the live map first, so
/// the next advance cannot re-attach what the release unparented.
pub fn release_animated_attachment(
    world: &mut World,
    entity: Entity,
) -> Option<Vec<AttachmentRecord>> {
    if !is_managed(world, entity) {
        return None;
    }
    let parent = world
        .get::<ChildOf>(entity)
        .map(|child_of| child_of.parent())?;

    // The velocity is read from the chain the link is about to leave, so
    // before the link goes away — and it never needs the node's composed
    // world pose (the release does not move it), so a node without one is
    // released the same way and says so if its velocity could not be measured.
    let skipped = detached_velocity(world, entity, Some(parent));
    world.entity_mut(entity).remove::<ChildOf>();
    mark_released(world, entity);

    // The records carry the animated identity, so only a released entity that
    // still has its binding contributes one.
    let Some(binding) = world.get::<AnimatedNodeBinding>(entity).cloned() else {
        return Some(Vec::new());
    };
    Some(
        skipped
            .iter()
            .map(|reason| AttachmentRecord::VelocityNotInherited {
                clip: binding.clip.clone(),
                node: binding.node.clone(),
                reason: *reason,
            })
            .collect(),
    )
}

/// Whether this consumer owns the entity's hierarchy link: it applied a
/// transition for it, or the clip records an attachment for it.
fn is_managed(world: &World, entity: Entity) -> bool {
    world.get::<AppliedAttachment>(entity).is_some()
        || world.get::<NodeAnimatedAttachment>(entity).is_some()
}

/// Records a released detach: the consumer applied no pose change (the world
/// pose is preserved by construction), so only the bookkeeping moves.
fn mark_released(world: &mut World, child: Entity) {
    if let Some(applied) = world.get::<AppliedAttachment>(child) {
        let released = AppliedAttachment {
            parent: None,
            pose: PosePolicy::KeepWorldPose,
        };
        if applied != &released {
            world.entity_mut(child).insert(released);
        }
    }
}

// ------------------------------------------------------------ the step -----

/// One pass over the entities the playback published an attachment record
/// on, returning everything it refused or could not inherit.
fn transition(world: &mut World) -> Vec<AttachmentRecord> {
    let candidates: Vec<(
        Entity,
        AnimatedNodeBinding,
        AttachmentState,
        Option<AppliedAttachment>,
    )> = {
        let mut query = world.query::<(
            Entity,
            &AnimatedNodeBinding,
            &NodeAnimatedAttachment,
            Option<&AppliedAttachment>,
        )>();
        query
            .iter(world)
            .map(|(entity, binding, attachment, applied)| {
                (
                    entity,
                    binding.clone(),
                    attachment.0.clone(),
                    applied.cloned(),
                )
            })
            .collect()
    };
    if candidates.is_empty() {
        return Vec::new();
    }

    // The parent lookup, built once from the live scene bindings: a
    // `scene_node` id within one generation, never a positional guess.
    let parents: SceneNodes = {
        let mut index: SceneNodes = HashMap::new();
        let mut query = world.query::<(Entity, &SceneNodeBinding)>();
        for (entity, binding) in query.iter(world) {
            index
                .entry((binding.node.clone(), binding.generation))
                .or_default()
                .push(entity);
        }
        index
    };

    let mut records = Vec::new();
    for (entity, binding, desired, applied) in &candidates {
        records.extend(apply_one(
            world,
            &parents,
            *entity,
            binding,
            desired,
            applied.as_ref(),
        ));
    }
    records
}

/// Verifies and applies the transition of one candidate; returns what it
/// refused or could not inherit.
fn apply_one(
    world: &mut World,
    parents: &SceneNodes,
    entity: Entity,
    binding: &AnimatedNodeBinding,
    desired: &AttachmentState,
    applied: Option<&AppliedAttachment>,
) -> Vec<AttachmentRecord> {
    // 0. Idempotence: the evaluated state is re-published every tick, so an
    //    equal state is a no-op. This is what makes a transition happen
    //    once per change — and what keeps the inherited velocity of a
    //    detach written on the detach tick only (F20 non-negotiable
    //    behavior 3).
    let wanted = match &desired.parent {
        None => None,
        Some(Resolved::Known(known)) => Some(known.value.clone()),
        // An unknown parent never reaches a component (F20-B blocks and
        // reports it), so there is nothing to apply even defensively.
        Some(Resolved::Unknown { .. }) => return Vec::new(),
    };
    if applied.is_some_and(|record| record.parent == wanted && record.pose == desired.pose) {
        return Vec::new();
    }

    // 1. Verification against the playing instance. An entity that does not
    //    verify is not driven: nothing is attempted and nothing is
    //    reported, exactly like the track application it follows.
    let Some(playback) = world.get_resource::<AnimationPlayback>() else {
        return Vec::new();
    };
    let Some(serving) = playback.generation(&binding.clip, binding.instance) else {
        // That instance is not playing: the record is a leftover of a stopped
        // instance, and its teardown owns it (F20-C.02).
        return Vec::new();
    };
    if !playback.drives(&binding.clip, binding.instance, &binding.node) {
        return Vec::new();
    }

    let Some((scene_node, scene_generation)) = world
        .get::<SceneNodeBinding>(entity)
        .map(|scene| (scene.node.clone(), scene.generation))
    else {
        // Not a scene node: there is no hierarchy link for this consumer to
        // own (the F20-B records that are only binding records stay records).
        return Vec::new();
    };
    if scene_node != binding.node {
        return Vec::new();
    }
    if scene_generation != binding.generation {
        return refuse(
            world,
            entity,
            binding,
            desired,
            AttachmentRefusalReason::StaleScene {
                scene: scene_generation,
            },
        );
    }
    if serving != binding.generation {
        return refuse(
            world,
            entity,
            binding,
            desired,
            AttachmentRefusalReason::StaleBinding { serving },
        );
    }

    // 2. The parent reference (extracted in step 0): resolved inside this
    //    binding's generation, and unique — a known id that names nothing
    //    is refused, never guessed.
    // 3. The poses the policy needs. Without them the authored policy
    //    cannot be honored, so nothing is applied.
    let Some(child_world) = world.get::<NodeVisualTransform>(entity).map(|pose| pose.0) else {
        return refuse(
            world,
            entity,
            binding,
            desired,
            AttachmentRefusalReason::NodeWorldPoseMissing,
        );
    };
    let old_parent = world
        .get::<ChildOf>(entity)
        .map(|child_of| child_of.parent());

    let parent_entity = match &wanted {
        Some(parent_id) => match parents
            .get(&(parent_id.clone(), binding.generation))
            .map(Vec::as_slice)
        {
            // Exactly one live entity of this generation presents the id.
            Some([single]) => *single,
            Some(many) if many.len() > 1 => {
                return refuse(
                    world,
                    entity,
                    binding,
                    desired,
                    AttachmentRefusalReason::AmbiguousParent { count: many.len() },
                );
            }
            // No live entity of this generation carries the id: refused,
            // never resolved by guessing at another generation's node.
            _ => {
                return refuse(
                    world,
                    entity,
                    binding,
                    desired,
                    AttachmentRefusalReason::UnknownParent,
                );
            }
        },
        None => Entity::PLACEHOLDER,
    };

    // A parent that is the node itself, or one of its own descendants, would
    // make the node its own ancestor: refused before anything is written,
    // never applied and left to loop in every subtree walk.
    if wanted.is_some() && creates_cycle(world, parent_entity, entity) {
        return refuse(
            world,
            entity,
            binding,
            desired,
            AttachmentRefusalReason::CyclicParent,
        );
    }

    // The parent-relative pose is derived, never stored twice: the policy
    // that needs it validates the pose it reads before anything is written.
    let keep_local = desired.pose == PosePolicy::KeepLocalPose;
    let local = if keep_local {
        match old_parent {
            Some(old) => match world.get::<NodeVisualTransform>(old) {
                Some(pose) => pose.0.affine().inverse() * child_world.affine(),
                None => {
                    return refuse(
                        world,
                        entity,
                        binding,
                        desired,
                        AttachmentRefusalReason::ParentWorldPoseMissing,
                    );
                }
            },
            // A root's composed world pose *is* its parent-relative pose.
            None => child_world.affine(),
        }
    } else {
        Affine3A::IDENTITY
    };
    let parent_world = wanted
        .is_some()
        .then(|| {
            world
                .get::<NodeVisualTransform>(parent_entity)
                .map(|pose| pose.0)
        })
        .flatten();

    // 4. Apply the transition.
    let mut records = Vec::new();
    match (&wanted, parent_world) {
        (Some(_), Some(parent_world)) => {
            // Attach: `KeepWorldPose` keeps the composed world affine and
            // derives the parent-relative pose from it, `KeepLocalPose`
            // keeps the parent-relative pose and lets the world follow.
            let new_world = match desired.pose {
                PosePolicy::KeepWorldPose => child_world.affine(),
                PosePolicy::KeepLocalPose => parent_world.affine() * local,
            };
            world.entity_mut(entity).insert(ChildOf(parent_entity));
            write_world_pose(world, entity, child_world, new_world);
        }
        (Some(_), None) => {
            return refuse(
                world,
                entity,
                binding,
                desired,
                AttachmentRefusalReason::ParentWorldPoseMissing,
            );
        }
        (None, _) => {
            // Detach: the inherited velocity must be read from the parent
            // being left *before* the link goes away, because that chain is
            // where the velocity lives.
            for reason in detached_velocity(world, entity, old_parent) {
                records.push(AttachmentRecord::VelocityNotInherited {
                    clip: binding.clip.clone(),
                    node: binding.node.clone(),
                    reason,
                });
            }
            world.entity_mut(entity).remove::<ChildOf>();
            let new_world = match desired.pose {
                // The composed world pose is simply not touched: preserving
                // it cannot move it.
                PosePolicy::KeepWorldPose => child_world.affine(),
                // The kept local numbers become the pose of a root.
                PosePolicy::KeepLocalPose => local,
            };
            write_world_pose(world, entity, child_world, new_world);
        }
    }

    world
        .entity_mut(entity)
        .insert(AppliedAttachment {
            parent: wanted,
            pose: desired.pose,
        })
        .remove::<RefusedAttachment>();
    records
}

/// Publishes one refusal, unless the same refusal for the same state is
/// already on record — a state that lasts must not append one entry per
/// tick.
fn refuse(
    world: &mut World,
    entity: Entity,
    binding: &AnimatedNodeBinding,
    desired: &AttachmentState,
    reason: AttachmentRefusalReason,
) -> Vec<AttachmentRecord> {
    let parent = match &desired.parent {
        None => None,
        Some(Resolved::Known(known)) => Some(known.value.clone()),
        Some(Resolved::Unknown { .. }) => return Vec::new(),
    };
    let already = world
        .get::<RefusedAttachment>(entity)
        .is_some_and(|record| {
            record.parent == parent && record.pose == desired.pose && record.reason == reason
        });
    if already {
        return Vec::new();
    }
    world.entity_mut(entity).insert(RefusedAttachment {
        parent: parent.clone(),
        pose: desired.pose,
        reason: reason.clone(),
    });
    vec![AttachmentRecord::Refused {
        clip: binding.clip.clone(),
        node: binding.node.clone(),
        parent,
        pose: desired.pose,
        reason,
    }]
}

/// Whether making `parent` the parent of `node` would close a loop: `parent`
/// is `node` itself, `node` is one of `parent`'s ancestors, or `parent`'s own
/// ancestor chain already contains a loop (that hierarchy is already invalid,
/// and this walk must terminate on it either way).
fn creates_cycle(world: &World, parent: Entity, node: Entity) -> bool {
    let mut seen = HashSet::new();
    let mut cursor = Some(parent);
    while let Some(entity) = cursor {
        if entity == node || !seen.insert(entity) {
            return true;
        }
        cursor = world
            .get::<ChildOf>(entity)
            .map(|child_of| child_of.parent());
    }
    false
}

// ------------------------------------------------------------ the pose -----

/// Writes the node's new composed world pose when it changed, and recomposes
/// the world pose of every descendant behind it.
///
/// Only a change is written: a `KeepWorldPose` transition writes no floats,
/// so preserving a pose cannot introduce a rounding drift, and a stale
/// descendant affine is impossible because every descendant is recomposed
/// from its own parent-relative pose in the same pass.
fn write_world_pose(world: &mut World, entity: Entity, old: GlobalTransform, new: Affine3A) {
    if new == old.affine() {
        return;
    }
    world
        .entity_mut(entity)
        .insert(NodeVisualTransform(GlobalTransform::from(Mat4::from(new))));
    let stored = world
        .get::<NodeVisualTransform>(entity)
        .expect("the pose was just written")
        .0;
    recompose_descendants(world, entity, old.affine(), stored.affine());
}

/// Recomposes the [`NodeVisualTransform`] of a subtree under its node's new
/// world pose: each child keeps the parent-relative pose it had, so its
/// world affine follows the parent change instead of staying behind.
///
/// A child without a composed world pose is not a scene node of this
/// hierarchy and is left alone (it has no world affine of ours to stale).
fn recompose_descendants(world: &mut World, parent: Entity, old: Affine3A, new: Affine3A) {
    let children: Vec<Entity> = {
        let mut query = world.query_filtered::<(Entity, &ChildOf), ()>();
        query
            .iter(world)
            .filter(|(_, child_of)| child_of.parent() == parent)
            .map(|(entity, _)| entity)
            .collect()
    };
    for child in children {
        let Some(pose) = world.get::<NodeVisualTransform>(child).map(|pose| pose.0) else {
            continue;
        };
        let child_old = pose.affine();
        let child_new = new * (old.inverse() * child_old);
        world
            .entity_mut(child)
            .insert(NodeVisualTransform(GlobalTransform::from(Mat4::from(
                child_new,
            ))));
        recompose_descendants(world, child, child_old, child_new);
    }
}

// ----------------------------------------------------------- the velocity ---

/// Inherits the world velocity of the parent being left, at that tick.
///
/// Writes nothing that is not there to write, and reports every case where
/// an inheritance the node could have taken was not measurable. The source is
/// named, never assumed:
///
/// * the **linear source** is the nearest ancestor (the parent first) that
///   carries an avian [`LinearVelocity`];
/// * the **spin source** is the nearest ancestor that carries an avian
///   [`AngularVelocity`] — avian's angular velocity is world-space;
/// * a body's **reference point** is its avian [`Position`] when it has one
///   (physics' authority for a body's location) and otherwise its composed
///   [`NodeVisualTransform`] translation;
/// * `v_inherited = v_source + ω × r`, with `r` measured from the source's
///   reference point to the node's, and `ω_inherited = ω_spin`.
///
/// The results land **only** on components the entity already carries: a
/// node that was never simulated gains no velocity, which is not a missing
/// inheritance and is therefore not reported. Nothing is invented: a chain
/// with no velocity component, an unmeasurable `r` or a detach from a node
/// that was already a root reparented and preserved the pose anyway and
/// publishes one `VelocityNotInherited` record saying why; a chain that
/// carries only an angular source contributes the rotation alone and says so
/// (`NoLinearSource`), because `ω × r` without a linear reference point
/// would be a guess. The node's own angular velocity is left alone when no
/// ancestor spins: overwriting it with a zero would *be* an invention.
///
/// The node's own reference point is read here rather than handed in, so a
/// caller that has no composed pose for it (a release, which never moves one)
/// still inherits everything that is measurable.
fn detached_velocity(
    world: &mut World,
    node: Entity,
    parent: Option<Entity>,
) -> Vec<VelocitySkipReason> {
    let Some(parent) = parent else {
        return vec![VelocitySkipReason::NoParent];
    };

    let mut linear: Option<(Vec3, Entity)> = None;
    let mut spin: Option<Vec3> = None;
    let mut cursor = Some(parent);
    while let Some(entity) = cursor {
        if linear.is_none()
            && let Some(velocity) = world.get::<LinearVelocity>(entity)
        {
            linear = Some((velocity.0, entity));
        }
        if spin.is_none()
            && let Some(velocity) = world.get::<AngularVelocity>(entity)
        {
            spin = Some(velocity.0);
        }
        if linear.is_some() && spin.is_some() {
            break;
        }
        cursor = world
            .get::<ChildOf>(entity)
            .map(|child_of| child_of.parent());
    }

    if linear.is_none() && spin.is_none() {
        return vec![VelocitySkipReason::NoVelocitySource];
    }

    let mut skipped = Vec::new();
    if world.get::<LinearVelocity>(node).is_some() {
        match linear {
            None => skipped.push(VelocitySkipReason::NoLinearSource),
            Some((source_velocity, source)) => {
                match spin_term(world, source, node, spin.unwrap_or(Vec3::ZERO)) {
                    Ok(term) => {
                        world
                            .entity_mut(node)
                            .insert(LinearVelocity(source_velocity + term));
                    }
                    Err(reason) => skipped.push(reason),
                }
            }
        }
    }
    if world.get::<AngularVelocity>(node).is_some()
        && let Some(spin) = spin
    {
        world.entity_mut(node).insert(AngularVelocity(spin));
    }
    skipped
}

/// The `ω × r` term of the inheritance, or why it cannot be measured.
///
/// A chain that does not spin contributes **exactly** zero, however
/// unmeasurable the offset is: `ω × r = 0` for every `r`, so the linear
/// source alone is inherited and nothing is reported. Only a spin that is
/// really there needs the two reference points, and when either is missing
/// the term is refused instead of guessed.
fn spin_term(
    world: &World,
    source: Entity,
    node: Entity,
    spin: Vec3,
) -> Result<Vec3, VelocitySkipReason> {
    if spin == Vec3::ZERO {
        return Ok(Vec3::ZERO);
    }
    let from = reference_point(world, source).ok_or(VelocitySkipReason::NoReferencePoint)?;
    let to = reference_point(world, node).ok_or(VelocitySkipReason::NoNodeReferencePoint)?;
    Ok(spin.cross(to - from))
}

/// The world point a body's linear velocity is expressed at: its Avian
/// [`Position`] when it has one, otherwise its composed scene pose.
fn reference_point(world: &World, entity: Entity) -> Option<Vec3> {
    if let Some(position) = world.get::<Position>(entity) {
        return Some(position.0);
    }
    world
        .get::<NodeVisualTransform>(entity)
        .map(|pose| Vec3::from(pose.0.affine().translation))
}
