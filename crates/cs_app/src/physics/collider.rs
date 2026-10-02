//! The collision-side consumer of a clip-hidden node: the record a hidden
//! node's collision state is written to, and who may lift it (F20-C.04).
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-C` (acceptance AC01, non-negotiable behavior 3). Shared
//! contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! # The gap this module closes
//!
//! F20-A's evaluator decides `hidden ⇒ no collider` — a visibility swap must
//! not leave an invisible obstacle behind — and F20-C.03 composed that into
//! [`ColliderVerdict::NoCollider`](crate::animation::ColliderVerdict::NoCollider),
//! read out of
//! [`composed_visibility_verdict`](crate::animation::composed_visibility_verdict).
//! **Nothing read it.** The engine had no collision-enable record at all: the
//! F20-C.03 slice recorded that `grep -rn CollisionEnabled crates/` was empty
//! and deliberately did not invent a consumer for a physics subsystem another
//! stage owns, because the original's coupling of visibility to collision is
//! **unmeasured**. This module is that consumer, and the record it writes.
//!
//! # The decision: one physics-owned record, written only here
//!
//! Three candidates were weighed. The one this module implements is the third:
//!
//! 1. **The authored `CollisionRole` on F11-C's `PartBinding`.** Rejected: a
//!    disabled part binding is a *content* change, and the authored role is
//!    the scene import's own decision (F11-C, `cs_content::scene::CollisionRole`).
//!    Writing it would re-decide another stage's record, and it has no way to
//!    say "removed by damage" as opposed to "not authored".
//! 2. **Avian's `CollisionEnabled`, written from the animation path.**
//!    Rejected, and the reason is the whole point of the task: the animation
//!    path stays a *publisher*. It writes its own fact
//!    ([`NodeAnimatedVisibility`](crate::animation::NodeAnimatedVisibility)) and
//!    nothing else, so the coupling between a clip and a physics body is never
//!    made inside the playback, and no animation module has to know Avian
//!    exists.
//! 3. **A policy record the physics adapter owns, which a consumer reads.**
//!    This is the record: [`NodeColliderPresence`], written by
//!    [`apply_collider_presence`] from the composed verdict and by the damage
//!    seam below, and projected onto the engine by inserting/removing Avian
//!    0.7's [`ColliderDisabled`] marker — the pinned engine's actual mechanism
//!    for "this collider does not participate" (Avian has **no**
//!    `CollisionEnabled` component; `ColliderDisabled` is its removal-side
//!    marker, and its `On<Add>`/`On<Remove>` observers take the collider out of
//!    and back into the broad-phase tree).
//!
//! So the animation side keeps publishing the verdict, the physics side reads
//! it, and the physics side is the only writer of the engine's marker. Two
//! consequences worth stating, because they are what the record buys:
//!
//! * **No second writer of an engine component.** Nothing in
//!   `crates/cs_app/src/animation/` names `ColliderDisabled`, and nothing in
//!   this module names `NodeAnimatedVisibility` except to read it.
//! * **The verdict cannot be wrong about the engine.** The record is the
//!   policy; the marker is its projection; a disagreement between them is
//!   impossible because one function writes both, and it re-asserts the marker
//!   from the actual component state rather than from its own memory of it, so
//!   a marker removed by anything else is restored on the next pass.
//!
//! # The rule, and why a damage removal is terminal
//!
//! [`NodeColliderPresence::merge`] is the whole merge rule, and it is pure:
//!
//! | the record says | the composed collider verdict | merged |
//! | --- | --- | --- |
//! | `Live` | `Undecided` | `Live` |
//! | `Live` | `NoCollider` | `HiddenByAnimation` |
//! | `HiddenByAnimation` | `NoCollider` | `HiddenByAnimation` |
//! | `HiddenByAnimation` | `Undecided` | `Live` |
//! | `RemovedByDamage` | `Undecided` or `NoCollider` | `RemovedByDamage` |
//!
//! `RemovedByDamage` is **terminal for this pass**: the merge can neither enter
//! nor leave it, so no animation pass can re-enable a collider a damage
//! decision removed, whatever order the two run in and however often the clip
//! loops. That is F20 non-negotiable behavior 3, and it is structural rather
//! than a priority rule that a later refactor could reorder. Only the damage
//! seam ([`remove_collider_for_damage`], [`restore_collider_after_repair`])
//! writes that state, because the **decision** that a destroyed part loses its
//! collider is F29's, not this layer's: the damage side states the decision and
//! this layer applies it.
//!
//! The two other rules are equally explicit:
//!
//! * **Only the clip's own hide removes a collider for the clip.** The merged
//!   state comes from the *clip's* fact, never from the draw half of the
//!   verdict, so a node that LOD culls keeps its collider (F11-C: "LOD is
//!   presentation state only … collision … live on the bound node regardless
//!   of which variant is active").
//! * **A node with no record is never touched.** See "Who owns the record".
//!
//! # What this module deliberately does not read
//!
//! * **`NodeDisabled`** (F11-C's damage marker). F11-C states that
//!   `NodeDisabled` decides *presentation* and that "collision, weapon origins
//!   and damage identity read their own records", so a destroyed node's
//!   collider is not this module's to infer from a presentation marker. The
//!   destruction case arrives through [`remove_collider_for_damage`], which is
//!   F29's decision to make explicit. Inferring collision from `NodeDisabled`
//!   would also put a second writer on the answer, and would make the
//!   clip-vs-damage priority a race between two readers.
//! * **`NodePresentation`** (F11-C's LOD record), for the same reason: it is
//!   rewritten every LOD pass, and collision must not move with distance.
//! * **`AirframeDamageState`.** `apply_airframe_damage` is its single owner and
//!   projects it onto the per-entity markers; reaching the resource would mean
//!   re-deciding a part identity whose owner has already decided it.
//!
//! # Who owns the record
//!
//! The record is **opt-in**: the node's spawner inserts
//! [`NodeColliderPresence::Live`] for a node whose authored
//! [`CollisionRole`](cs_content::scene::CollisionRole) is `Collider`, and
//! [`apply_collider_presence`] then manages that node's marker and nothing
//! else. That is what keeps the rule "this layer never re-enables a collider it
//! did not remove" true for the rest of the engine: a collider this policy has
//! no record for is outside this policy, and the damage seam refuses such a
//! node loudly ([`ColliderDecisionError::UnmanagedNode`]) rather than silently
//! doing nothing.
//!
//! The cost of the opt-in is stated rather than hidden: a clip-hidden node
//! whose spawner never inserted the record keeps colliding, which is the
//! invisible obstacle F20-A's rule exists to prevent. That is a wiring
//// requirement on the spawn path (F11-C's scene import, or F29's part
//! colliders — neither of which puts an Avian collider on a scene node yet),
//! recorded as a follow-up, and the acceptance test asserts the boundary
//! explicitly instead of letting it pass unnoticed.
//!
//! # Where the pass runs
//!
//! [`ColliderPresencePlugin`] installs [`apply_collider_presence_on_fixed_tick`]
//! in `FixedPostUpdate` after
//! [`PhysicsSystems::StepSimulation`](avian3d::prelude::PhysicsSystems) — the
//! same fixed-tick slot as the animation advance
//! ([`advance_animation_on_session_tick`]), and ordered **after** it, so the
//! collision layer reads the verdict the animation produced in this tick rather
//! than the previous tick's. The engine marker Avian inserts in response is
//! honoured from the **next** tick's broad phase; that one-tick offset is the
//! earliest the pinned engine can honour it, because the animation advance
//! itself runs after the step (F20-C.02's designed placement, and an unmeasured
//! original one). A late change is a late update, never a wrong one: the record
//! is the answer, and the marker follows it every tick.
//!
//! # What the original does here is still unknown
//!
//! Whether the original couples node visibility to collision at all is
//! **unmeasured** (F20-A's recorded unknown: the original animation container
//! layouts are undecoded, F13), and so is whether an original visibility swap
//! hid the node's whole **subtree**. `hidden ⇒ no collider` is this engine's
//! **designed** rule (F20-A), adopted unchanged; nothing here claims an
//! original behaviour, and F20-D keeps the validation gate. Which contacts a
//! removed part's surface should generate once it is gone, and what the
//! original did with a destroyed part's collider, are F29's measurements.
//!
//! **Designed wiring, not original data.** Every binding here follows the
//! spec's declared behaviour and Avian 0.7's measured API
//! (`ColliderDisabled`); the original's rules are unknown until the
//! compatibility work measures them (F23-D, F29).

use avian3d::prelude::{Collider, ColliderDisabled, PhysicsSystems};
use bevy::{
    ecs::{component::Component, schedule::IntoScheduleConfigs},
    prelude::{App, Entity, FixedPostUpdate, Plugin, Resource, With, World},
};

use crate::animation::{
    ColliderVerdict, advance_animation_on_session_tick, composed_visibility_verdict,
};

/// Component: the collision-presence policy this layer owns for one node.
///
/// The record is the answer ("is this node's collider engaged, and whose
/// decision is it?"); Avian's [`ColliderDisabled`] marker is its projection
/// into the engine, written by the same function that writes this component, so
/// the two cannot disagree. A node without this component is outside the
/// policy: nothing here writes, restores or removes its collider.
///
/// The spawn path inserts [`Self::Live`] for a node whose authored
/// [`CollisionRole`](cs_content::scene::CollisionRole) is `Collider`; the
/// animation path never writes it, and neither does the LOD or damage
/// presentation pass.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NodeColliderPresence {
    /// The node's authored collider is live and nothing in this policy has
    /// removed it. The state a node is spawned with.
    #[default]
    Live,
    /// A playing clip hides the node, so its collider is off (F20-A's designed
    /// `hidden ⇒ no collider`). The clip's own show lifts this and nothing
    /// else.
    HiddenByAnimation,
    /// A damage decision removed the collider. Off, and **terminal for
    /// [`apply_collider_presence`]**: the merge can neither enter nor leave
    /// this state, so no animation pass can re-enable a collider damage
    /// removed (F20 non-negotiable behavior 3). Only
    /// [`remove_collider_for_damage`] and [`restore_collider_after_repair`]
    /// write it, because the decision is F29's.
    RemovedByDamage,
}

impl NodeColliderPresence {
    /// Whether the node's collider is engaged in the simulation.
    ///
    /// The single question a physics consumer asks; the engine marker is
    /// derived from it, never the other way round.
    #[must_use]
    pub const fn collider_enabled(self) -> bool {
        matches!(self, Self::Live)
    }

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Live => "collider live",
            Self::HiddenByAnimation => "collider hidden by animation",
            Self::RemovedByDamage => "collider removed by damage",
        }
    }

    /// Merges the clip's collider verdict into this record.
    ///
    /// Pure, and the whole merge rule (see the module doc for the table):
    ///
    /// * a damage removal is **terminal** — neither entered nor left here, so
    ///   no clip verdict can re-enable a collider damage removed;
    /// * the clip's hide is the only thing that removes a collider on the
    ///   animation's account, and only the clip's own show restores it;
    /// * `Undecided` never removes anything, so the moment a clip stops hiding
    ///   the node — or stops driving it at all — the authored collider is back.
    #[must_use]
    pub const fn merge(self, verdict: ColliderVerdict) -> Self {
        match (self, verdict) {
            // Terminal: the damage side owns this state, not the clip.
            (Self::RemovedByDamage, _) => Self::RemovedByDamage,
            (_, ColliderVerdict::NoCollider) => Self::HiddenByAnimation,
            (Self::Live | Self::HiddenByAnimation, ColliderVerdict::Undecided) => Self::Live,
        }
    }
}

/// What one [`apply_collider_presence`] pass changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColliderPresenceReport {
    /// Policy records whose state this pass changed.
    pub presence_updates: u32,
    /// Engine marker writes this pass: a [`ColliderDisabled`] inserted or
    /// removed. Zero on a pass that found every managed node already in the
    /// state its record says, which is what idempotence means on the physics
    /// side.
    pub collider_writes: u32,
    /// Managed nodes that carry no Avian [`Collider`] yet: the record is
    /// maintained, the engine has nothing to apply it to. Counted rather than
    /// skipped, so a spawner that inserts the record before the collider is
    /// visible in the report instead of being a silent no-op.
    pub without_collider: u32,
}

impl ColliderPresenceReport {
    /// Whether the pass changed anything at all.
    #[must_use]
    pub const fn is_noop(&self) -> bool {
        self.presence_updates == 0 && self.collider_writes == 0
    }
}

/// Resource: what the fixed-tick presence pass has done, cumulatively.
///
/// The pass itself is idempotent by construction, so the counters are how a
/// consumer (or a test) observes that: `last.is_noop()` is the observation for
/// one tick, and the totals are the observation over a run.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColliderPresenceLedger {
    /// The most recent pass.
    pub last: ColliderPresenceReport,
    /// Policy records changed since the resource was created.
    pub total_presence_updates: u64,
    /// Engine marker writes since the resource was created.
    pub total_collider_writes: u64,
}

/// What one pass wrote: the record change, the engine marker change, and
/// whether the node carries an engine collider at all.
struct PresenceWrite {
    record: bool,
    marker: bool,
    has_collider: bool,
}

/// Writes `state` into the record and projects it onto the engine.
///
/// The marker is written from the engine's **actual** component state, not from
/// what this pass last did, so a marker removed by anything else is restored on
/// the next pass and a marker this pass already wrote is not written twice. A
/// node with no Avian [`Collider`] gets its record and no marker: there is
/// nothing in the simulation to disable yet, and the next pass projects the
/// record once the collider is there.
fn apply_state(world: &mut World, entity: Entity, state: NodeColliderPresence) -> PresenceWrite {
    let record = world.get::<NodeColliderPresence>(entity) != Some(&state);
    if record {
        world.entity_mut(entity).insert(state);
    }
    let has_collider = world.get::<Collider>(entity).is_some();
    let marker = if !has_collider {
        false
    } else {
        // The engine's marker is the *removal* side, so the state the engine
        // should already hold is "disabled exactly when the record says off".
        let disabled = world.get::<ColliderDisabled>(entity).is_some();
        if disabled == !state.collider_enabled() {
            // Already in the requested state: no second write.
            false
        } else {
            let mut target = world.entity_mut(entity);
            if state.collider_enabled() {
                target.remove::<ColliderDisabled>();
            } else {
                target.insert(ColliderDisabled);
            }
            true
        }
    };
    PresenceWrite {
        record,
        marker,
        has_collider,
    }
}

/// The one pass: reads the composed verdict of every managed node, merges it
/// into that node's record and projects the result onto the engine.
///
/// Managed means **carries a [`NodeColliderPresence`]**; every other entity in
/// the world is invisible to this pass, so a collider this policy never
/// removed can never be re-enabled here.
///
/// Idempotent: a node already in the state its record says costs no record
/// write and no engine write, and [`Self::is_noop`](ColliderPresenceReport::is_noop)
/// says so. Directly callable, and the same code
/// [`ColliderPresencePlugin`] installs.
pub fn apply_collider_presence(world: &mut World) -> ColliderPresenceReport {
    let managed: Vec<Entity> = world
        .query_filtered::<Entity, With<NodeColliderPresence>>()
        .iter(world)
        .collect();
    let mut report = ColliderPresenceReport::default();
    for entity in managed {
        // A node despawned between the query and this read has no record left
        // and nothing to decide.
        let Some(current) = world.get::<NodeColliderPresence>(entity).copied() else {
            continue;
        };
        let verdict = composed_visibility_verdict(world, entity);
        let merged = current.merge(verdict.collider());
        let write = apply_state(world, entity, merged);
        report.presence_updates += u32::from(write.record);
        report.collider_writes += u32::from(write.marker);
        report.without_collider += u32::from(!write.has_collider);
    }
    report
}

/// The fixed-tick entry: runs [`apply_collider_presence`] once per fixed tick
/// and records what it did in [`ColliderPresenceLedger`].
///
/// A world with no ledger still applies the policy; the ledger is an
/// observation, not an input.
pub fn apply_collider_presence_on_fixed_tick(world: &mut World) {
    let report = apply_collider_presence(world);
    let Some(mut ledger) = world.get_resource_mut::<ColliderPresenceLedger>() else {
        return;
    };
    ledger.last = report;
    ledger.total_presence_updates += u64::from(report.presence_updates);
    ledger.total_collider_writes += u64::from(report.collider_writes);
}

/// Why a damage-side collider decision could not be applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColliderDecisionError {
    /// The entity is not in this world (despawned, or never was).
    UnknownEntity(Entity),
    /// The node carries no [`NodeColliderPresence`], so this layer has no
    /// record to change and no marker of its own to move. The spawn path has
    /// not put the node's collider under this policy; reported instead of
    /// silently doing nothing, because a damage decision that quietly
    /// disappears is worse than one that is refused.
    UnmanagedNode(Entity),
}

impl std::fmt::Display for ColliderDecisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownEntity(entity) => write!(f, "{entity} is not in this world"),
            Self::UnmanagedNode(entity) => write!(
                f,
                "{entity} carries no NodeColliderPresence: its collider is not under this \
                 layer's policy"
            ),
        }
    }
}

impl std::error::Error for ColliderDecisionError {}

/// Requires a live, managed node; the precondition both damage entries share.
fn require_managed(world: &World, entity: Entity) -> Result<(), ColliderDecisionError> {
    if world.get_entity(entity).is_err() {
        return Err(ColliderDecisionError::UnknownEntity(entity));
    }
    if world.get::<NodeColliderPresence>(entity).is_none() {
        return Err(ColliderDecisionError::UnmanagedNode(entity));
    }
    Ok(())
}

/// Applies a damage decision that removed the node's collider.
///
/// **This layer applies the decision; it does not make it.** Whether a
/// destroyed part loses its collider is F29's call
/// (`specs/F29-damage-zones-armor-destruction-and-bailout.md`), and F11-C's
/// damage pass writes only presentation markers, so the decision arrives here
/// as a call. The result is [`NodeColliderPresence::RemovedByDamage`], which
/// [`apply_collider_presence`] can never leave: a clip that re-shows the node
/// leaves the collider off (F20 non-negotiable behavior 3).
///
/// Returns whether the physics-side state changed — the record, the engine
/// marker, or both. A repeated call on an already-removed node returns `Ok(false)`
/// and writes nothing.
///
/// # Errors
///
/// [`ColliderDecisionError::UnknownEntity`] when the entity is not in this
/// world, and [`ColliderDecisionError::UnmanagedNode`] when the node carries no
/// [`NodeColliderPresence`] record.
pub fn remove_collider_for_damage(
    world: &mut World,
    entity: Entity,
) -> Result<bool, ColliderDecisionError> {
    require_managed(world, entity)?;
    let write = apply_state(world, entity, NodeColliderPresence::RemovedByDamage);
    Ok(write.record || write.marker)
}

/// Applies a damage repair: the damage-side removal is lifted, and the node's
/// collider state is recomputed through the same merge rule the fixed pass uses.
///
/// So a repair under a clip that still hides the node leaves the collider
/// **off** ([`NodeColliderPresence::HiddenByAnimation`]) — a repair must not
/// expose a node the animation has hidden — and the collider returns when the
/// clip shows it again, or as soon as the clip stops hiding it. The repair never
/// guesses: it hands the current verdict to [`NodeColliderPresence::merge`].
///
/// Returns whether the physics-side state changed.
///
/// # Errors
///
/// [`ColliderDecisionError::UnknownEntity`] when the entity is not in this
/// world, and [`ColliderDecisionError::UnmanagedNode`] when the node carries no
/// [`NodeColliderPresence`] record.
pub fn restore_collider_after_repair(
    world: &mut World,
    entity: Entity,
) -> Result<bool, ColliderDecisionError> {
    require_managed(world, entity)?;
    let merged =
        NodeColliderPresence::Live.merge(composed_visibility_verdict(world, entity).collider());
    let write = apply_state(world, entity, merged);
    Ok(write.record || write.marker)
}

/// Installs the fixed-tick collision-presence pass.
///
/// Add it to the world that runs a session's fixed loop, beside
/// [`PhysicsAdapterPlugin`](super::PhysicsAdapterPlugin) and
/// [`PhysicsBodiesPlugin`](super::PhysicsBodiesPlugin) — the production seam is
/// [`PhysicsSessionBuilder::configure`](super::PhysicsSessionBuilder::configure).
/// The pass is installed by this plugin and nothing else: a world without it
/// keeps its colliders exactly as its spawner built them, which is the state the
/// engine is in today.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColliderPresencePlugin;

impl Plugin for ColliderPresencePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ColliderPresenceLedger>();
        app.add_systems(
            FixedPostUpdate,
            apply_collider_presence_on_fixed_tick
                .after(PhysicsSystems::StepSimulation)
                .after(advance_animation_on_session_tick),
        );
    }
}
