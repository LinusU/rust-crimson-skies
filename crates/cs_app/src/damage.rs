//! The damage application boundary (F29-A/B/C).
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stages
//! `### F29-A`, `### F29-B` and `### F29-C`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! This module sits between the declared damage schema
//! ([`cs_content::damage`]) and the session resolver
//! ([`cs_sim::damage`]), which cannot see each other — `cs_sim` must not
//! depend on `cs_content` (`docs/01-ARCHITECTURE.md`). It contains:
//!
//! * [`lower_graph`] — the conversion boundary: a validated
//!   [`cs_content::damage::DeclaredDamageGraph`] becomes the runtime
//!   [`cs_sim::damage::DamageGraph`], with every [`Resolved::Unknown`]
//!   carried through so a hit routed at an unresolved integrity still
//!   blocks visibly instead of being repaired at the boundary;
//! * [`lower_policy`] — the declared-rules boundary: the graph's
//!   [`cs_content::damage::AttributionRule`] becomes the
//!   [`cs_sim::damage::DamagePolicy`] the actor registers under. An
//!   `Unknown` attribution **refuses** rather than guessing: no session
//!   resolves kills under an unstated rule;
//! * [`DamageActorBinding`] — the ECS record tying an entity to its
//!   session-qualified [`cs_sim::damage::ActorId`] and damage-graph
//!   subject, generation-stamped like
//!   [`crate::scene::SceneNodeBinding`] so a reload can never leave a
//!   stale binding looking live.
//! * [`apply_world_initial_damage`] — starts a registered actor in the
//!   damage state a mission's [`WorldInstance`] authors, reporting every
//!   object or statement it could not resolve (task #515);
//! * [`apply_damage_state`] — the F29-C consumer seam: it reads the
//!   resolver's authoritative part state for one actor and rewrites the
//!   weapon firing gate ([`FireResolver`]'s disabled-mount set) and the
//!   visual damage record ([`AirframeDamageState`]) to agree with it. A
//!   destroyed weapon mount stops firing and its part is presented
//!   destroyed; a repair brings both back. Every refusal is named in the
//!   returned [`DamageConsumerLog`].
//! * [`apply_propulsion_state`] — the F29-C.1 consumer seam for
//!   [`SystemKind::Propulsion`]: it reads the resolver's authoritative system
//!   state and rewrites the thrust authority of the actor's
//!   [`FlightAircraft`] gate, so a destroyed engine stops producing thrust and
//!   a repair gives it back.
//!
//! Nothing here owns damage state: pools, lifecycle records and event
//! sequences are the resolver's; these are the conversion and binding
//! records the ECS wiring consumes (F29-B/C).
//!
//! # The damage side of the collider decision
//!
//! F20-C.04 ([`crate::physics::collider`]) is the *application* of a damage
//! removal: it records that a decision took a node's collider out of the
//! simulation and keeps it terminal against the animation layer. It carries
//! **no gameplay rule**, because *whether* a destroyed part loses its collider
//! is F29's. This module is that caller.
//!
//! The decision is read from the damage zone's own record, never from the
//! animation layer's: [`DamageZoneBinding`] ties one collider-managed entity to
//! the session-qualified damage zone it is, and [`apply_damage_events`] walks a
//! resolution's [`DamageEvent`]s, decides per [`PartState`] transition and
//! calls
//! [`remove_collider_for_damage`](crate::physics::remove_collider_for_damage)
//! / [`restore_collider_after_repair`](crate::physics::restore_collider_after_repair).
//! [`repair_damage_zone`] is the repair path's entry. Both report a refusal
//! through [`DamageColliderLog`] rather than swallowing it; a zone the spawn
//! path never bound is reported too, because a damage decision that silently
//! disappears is worse than one that is refused.
//!
//! **Designed rule, not original data.** Which original parts lose collision
//! when destroyed is unmeasured (F29-D/F20-D keep the gate); the rule here — a
//! destroyed zone under the collision policy loses its collider, and a repair
//! gives it back unless a clip still hides the node — is this engine's
//! designed behavior, recorded in
//! `docs/findings/2026-10-02-f29-damage-zone-collider-call.md`.

use std::collections::BTreeMap;

use bevy::ecs::component::Component;
use bevy::prelude::{Entity, Resource, World};
use cs_content::damage::{
    AttributionRule as DeclaredAttributionRule, DamageNodeKind as DeclaredNodeKind,
    DeclaredDamageGraph, DeclaredDamageNode, SystemKind as DeclaredSystemKind,
};
use cs_content::scene::SceneNodeId;
use cs_content::world::{WorldInstance, WorldObjectId};
use cs_sim::damage::{
    ActorId, AttributionRule, DamageEvent, DamageEventKind, DamageGraph, DamageGraphError,
    DamageNode, DamageNodeKey, DamageNodeKind, DamagePolicy, DamageResolver, InitialDamage,
    InitialDamageReport, NodeKeyError, PartState, SystemKind, SystemState,
};
use cs_sim::flight::DamageState;
use cs_sim::weapons::FireResolver;
use cs_types::content::{ContentId, Known, Resolved};
use cs_types::evidence::ClaimId;

use crate::physics::{
    ColliderDecisionError, FlightAircraft, remove_collider_for_damage,
    restore_collider_after_repair,
};
use crate::scene::{AirframeDamageState, SceneGeneration};

/// Why a declared graph could not be lowered to the runtime records.
#[derive(Clone, Debug, PartialEq)]
pub enum DamageLowerError {
    /// A declared node key could not form a runtime key. Retained for
    /// signature compatibility: declared and runtime keys are now the one
    /// shared [`cs_types::content::DamageNodeKey`], so this is unreachable.
    NodeKey {
        /// The declared key text.
        key: String,
        /// Why the runtime refused it.
        source: NodeKeyError,
    },
    /// The runtime refused the assembled graph.
    Graph(DamageGraphError),
    /// The graph's lethal-attribution rule is `Resolved::Unknown`: no
    /// session may resolve kills under a guessed rule (F29 AC01's
    /// "declared attribution rule" is mandatory).
    UnknownAttribution {
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the rule is unknown.
        reason: String,
    },
}

impl std::fmt::Display for DamageLowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NodeKey { key, source } => {
                write!(f, "damage node key {key:?} cannot be lowered: {source}")
            }
            Self::Graph(source) => write!(f, "the runtime refused the lowered graph: {source}"),
            Self::UnknownAttribution { claim_id, reason } => write!(
                f,
                "the lethal attribution rule is unknown ({}: {reason}) and cannot be lowered",
                claim_id.as_str()
            ),
        }
    }
}

impl std::error::Error for DamageLowerError {}

/// Lowers a declared damage graph into the runtime graph the resolver
/// registers.
///
/// Node keys map by text, kinds and edges field-wise, `scene_binding`s
/// shed their [`SceneNodeId`] wrapper into `scene_node` [`ContentId`]s and
/// every [`Resolved::Unknown`] is carried through verbatim — nothing is
/// resolved, guessed or repaired at this boundary.
///
/// # Errors
///
/// [`DamageLowerError::Graph`] when
/// [`cs_sim::damage::DamageGraph::try_new`] refuses the assembled record.
/// Node keys are no longer re-validated here: declared and runtime node keys
/// are the one shared [`cs_types::content::DamageNodeKey`].
pub fn lower_graph(graph: &DeclaredDamageGraph) -> Result<DamageGraph, DamageLowerError> {
    let nodes = graph
        .nodes()
        .iter()
        .map(lower_node)
        .collect::<Result<Vec<_>, _>>()?;
    DamageGraph::try_new(graph.subject().clone(), nodes).map_err(DamageLowerError::Graph)
}

/// Lowers the declared rules into the [`DamagePolicy`] the actor registers
/// under — its own graph's rules, so different subject kinds resolve side
/// by side under their own.
///
/// An unknown `lethal_attribution` is refused with its claim — a session
/// may not resolve kills under a guessed rule.
///
/// # Errors
///
/// [`DamageLowerError::UnknownAttribution`] when the declared rule is
/// `Resolved::Unknown`.
pub fn lower_policy(graph: &DeclaredDamageGraph) -> Result<DamagePolicy, DamageLowerError> {
    match &graph.rules().lethal_attribution {
        Resolved::Known(known) => Ok(DamagePolicy {
            attribution: lower_attribution(known.value),
        }),
        Resolved::Unknown { claim_id, reason } => Err(DamageLowerError::UnknownAttribution {
            claim_id: claim_id.clone(),
            reason: reason.clone(),
        }),
    }
}

fn lower_attribution(rule: DeclaredAttributionRule) -> AttributionRule {
    match rule {
        DeclaredAttributionRule::FirstLethalHit => AttributionRule::FirstLethalHit,
        DeclaredAttributionRule::GreatestDamage => AttributionRule::GreatestDamage,
    }
}

fn lower_node(node: &DeclaredDamageNode) -> Result<DamageNode, DamageLowerError> {
    let key = lower_key(&node.key)?;
    let mut lowered = DamageNode::new(
        key,
        match node.kind {
            DeclaredNodeKind::ArmorZone => DamageNodeKind::ArmorZone,
            DeclaredNodeKind::InternalStructure => DamageNodeKind::InternalStructure,
            DeclaredNodeKind::Engine => DamageNodeKind::Engine,
            DeclaredNodeKind::WeaponMount => DamageNodeKind::WeaponMount,
        },
        node.integrity.clone(),
    )
    .with_lethal(node.lethal);
    if let Some(system) = node.disables {
        lowered = lowered.with_disables(match system {
            DeclaredSystemKind::Propulsion => SystemKind::Propulsion,
            DeclaredSystemKind::Weapon => SystemKind::Weapon,
        });
    }
    if let Some(guard) = &node.guarded_by {
        lowered = lowered.with_guard(lower_key(guard)?);
    }
    if let Some(overflow) = &node.overflow {
        lowered = lowered.with_overflow(lower_key(overflow)?);
    }
    if let Some(binding) = &node.scene_binding {
        lowered = lowered.with_scene_binding(lower_scene_binding(binding));
    }
    Ok(lowered)
}

fn lower_key(key: &cs_content::damage::DamageNodeKey) -> Result<DamageNodeKey, DamageLowerError> {
    // The declared and runtime node keys are now one shared
    // `cs_types::content::DamageNodeKey`, so lowering is an identity map.
    // The `Result` is kept for the boundary's existing signatures; this
    // cannot fail.
    Ok(key.clone())
}

/// A scene binding without its [`SceneNodeId`] wrapper, keeping the
/// resolved state — unknown stays unknown with its claim and reason.
fn lower_scene_binding(binding: &Resolved<SceneNodeId>) -> Resolved<ContentId> {
    match binding {
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

/// Component: marks an entity as the visual face of one damage actor.
///
/// `actor` is the session-qualified [`ActorId`] the resolver registered
/// (its `session` is the session generation), `graph` the catalog subject
/// the actor's damage graph was lowered from, and `generation` the scene
/// generation the binding was spawned under — so a reload stamps new
/// bindings and stale ones are identified by mismatch, never by surviving
/// pointers (the `STATE-TRANSACTIONS` session-generation discipline; the
/// same rule [`crate::scene::SceneNodeBinding`] follows).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct DamageActorBinding {
    /// The resolver actor this entity presents.
    pub actor: ActorId,
    /// The damage graph's catalog subject.
    pub graph: ContentId,
    /// The scene generation that spawned the binding.
    pub generation: SceneGeneration,
}

// ------------------------------------------------ the damage → collider seam ---

/// Component: the damage zone one collider-managed entity is.
///
/// This is the damage side's **own** record of *what* an entity's collider is:
/// the session-qualified damage graph node whose destruction decides that
/// entity's collider. [`apply_damage_events`] and [`repair_damage_zone`] read
/// it, so the collider decision never comes from the animation layer's
/// visibility record or from F11-C's `NodeDisabled` presentation marker — those
/// are other stages' records and F29 does not re-decide from them.
///
/// The entity it is put on is the one the collision policy manages — the same
/// entity that carries
/// [`NodeColliderPresence`](crate::physics::NodeColliderPresence) and Avian's
/// [`Collider`](avian3d::prelude::Collider) — because
/// [`remove_collider_for_damage`](crate::physics::remove_collider_for_damage)
/// moves one entity's marker and nothing else. The spawn path (F29-B) inserts
/// it for a part whose authored collider the policy manages; a node without it
/// is unbound, and a damage transition naming that zone is reported as
/// [`DamageColliderEvent::UnboundZone`] rather than applied to some other
/// entity.
///
/// A `(actor, node)` pair identifies at most one bound entity. The resolver's
/// node keys are graph-local, so the session-qualified [`ActorId`] is part of
/// the identity.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct DamageZoneBinding {
    /// The session-qualified actor the zone belongs to.
    pub actor: ActorId,
    /// The damage graph node the entity's collider answers to.
    pub node: DamageNodeKey,
}

impl DamageZoneBinding {
    /// Binds one collider-managed entity to the damage zone it is.
    #[must_use]
    pub fn new(actor: ActorId, node: DamageNodeKey) -> Self {
        Self { actor, node }
    }
}

/// What the damage side decided a zone's collider must become.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneColliderDecision {
    /// The zone was destroyed, so its collider leaves the simulation.
    Remove,
    /// The zone stopped being destroyed, so its collider returns.
    Restore,
}

/// The designed rule: which collider change a zone's part transition implies.
///
/// A zone that becomes [`PartState::Destroyed`] loses its collider; a zone that
/// stops being destroyed gets it back. Every other transition — `Intact` ↔
/// `Damaged`, or any unresolved state — changes nothing, so a part that is only
/// scratched keeps colliding and an unknown is never guessed into a decision.
///
/// Pure, and the whole rule. **Designed, not measured**: whether the original
/// removes a destroyed part's collider at all is unknown (the module doc and
/// `docs/findings/2026-10-02-f29-damage-zone-collider-call.md` record it);
/// F29-D/F20-D keep the original-family gate.
#[must_use]
pub const fn zone_collider_decision(
    from: PartState,
    to: PartState,
) -> Option<ZoneColliderDecision> {
    match (from, to) {
        (PartState::Intact | PartState::Damaged, PartState::Destroyed) => {
            Some(ZoneColliderDecision::Remove)
        }
        (PartState::Destroyed, PartState::Intact | PartState::Damaged) => {
            Some(ZoneColliderDecision::Restore)
        }
        _ => None,
    }
}

/// One entry in a [`DamageColliderLog`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DamageColliderEvent {
    /// A destroyed zone's damage removal was applied to its collider policy.
    ///
    /// The managed entity's Avian [`Collider`](avian3d::prelude::Collider) is
    /// disabled, not despawned, so a repair re-enables the same one.
    Removed {
        /// The zone's actor.
        actor: ActorId,
        /// The destroyed zone.
        node: DamageNodeKey,
        /// The collider-managed entity.
        entity: Entity,
    },
    /// A repaired zone's damage removal was lifted.
    ///
    /// This records the damage side's decision only. The collider itself is
    /// **not** necessarily engaged afterwards: a repair under a clip that still
    /// hides the node lands on `HiddenByAnimation`, and the collider returns
    /// when the clip shows the node or stops hiding it.
    Restored {
        /// The zone's actor.
        actor: ActorId,
        /// The repaired zone.
        node: DamageNodeKey,
        /// The collider-managed entity.
        entity: Entity,
    },
    /// A damage transition named a zone no entity is bound to.
    ///
    /// The damage record knows a part the spawn path never gave a
    /// [`DamageZoneBinding`]: the decision cannot reach a collider, and is
    /// reported instead of dropped.
    UnboundZone {
        /// The zone's actor.
        actor: ActorId,
        /// The unbound zone.
        node: DamageNodeKey,
    },
    /// The collision seam refused because the bound entity carries no
    /// [`NodeColliderPresence`](crate::physics::NodeColliderPresence).
    ///
    /// This is a **wiring gap**, not a gameplay case: the spawner never put
    /// this node's collider under the policy. Reported the way F11-C reports
    /// `SceneEvent::UnknownDamage`, because a damage decision that silently
    /// disappears is worse than one that is refused. It is also the signal that
    /// the spawn-wiring follow-up is incomplete.
    UnmanagedNode {
        /// The zone's actor.
        actor: ActorId,
        /// The zone.
        node: DamageNodeKey,
        /// The bound entity, which is not under the policy.
        entity: Entity,
    },
    /// The bound entity was already gone when the decision arrived.
    ///
    /// A no-op (the removal changed nothing), recorded so the release is
    /// visible rather than silent.
    ///
    /// Defensive: the bridge resolves a zone from a *live*
    /// [`DamageZoneBinding`], so a despawned zone is currently
    /// indistinguishable from one the spawn path never bound and surfaces as
    /// [`Self::UnboundZone`]. The arm keeps the match exhaustive against the
    /// seam's own error type and is the record a future caller that holds an
    /// entity handle would produce.
    ReleasedNode {
        /// The zone's actor.
        actor: ActorId,
        /// The zone.
        node: DamageNodeKey,
        /// The entity that is no longer in the world.
        entity: Entity,
    },
}

/// Resource: the append-only record of what the damage side did to colliders,
/// oldest first.
///
/// This is the error-propagation channel F11-C's `AirframeSceneLog` is for the
/// scene: a refusal and a zone no entity is bound to are both visible here
/// instead of being logged away or defaulted. It grows with the number of
/// *decisions* that changed state or were refused, never with frames.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct DamageColliderLog {
    events: Vec<DamageColliderEvent>,
}

impl DamageColliderLog {
    /// An empty log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A log holding one event.
    #[must_use]
    pub fn with(event: DamageColliderEvent) -> Self {
        Self {
            events: vec![event],
        }
    }

    /// Appends one event.
    pub fn push(&mut self, event: DamageColliderEvent) {
        self.events.push(event);
    }

    /// Every event, oldest first.
    #[must_use]
    pub fn events(&self) -> &[DamageColliderEvent] {
        &self.events
    }

    /// The most recent event.
    #[must_use]
    pub fn last(&self) -> Option<&DamageColliderEvent> {
        self.events.last()
    }

    /// How many events the log holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the log is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// What one damage → collider application changed or refused.
///
/// `removed` and `restored` count applied decisions; `unbound`, `unmanaged` and
/// `released` count the three reasons nothing was applied. The counters are
/// how a caller observes idempotence: applying the same transition twice
/// reports zero on the second call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DamageColliderReport {
    /// Destroyed zones whose damage removal was applied.
    pub removed: u32,
    /// Repaired zones whose damage removal was lifted. The collider is not
    /// necessarily engaged afterwards: a repair under a clip that still hides
    /// the node lands on `HiddenByAnimation`.
    pub restored: u32,
    /// Transitions that named a zone no entity is bound to.
    pub unbound: u32,
    /// Refusals: the bound entity is not under the collision policy.
    pub unmanaged: u32,
    /// Decisions whose bound entity was already gone.
    pub released: u32,
}

impl DamageColliderReport {
    /// Whether the application changed nothing and refused nothing.
    #[must_use]
    pub const fn is_noop(&self) -> bool {
        self.removed == 0
            && self.restored == 0
            && self.unbound == 0
            && self.unmanaged == 0
            && self.released == 0
    }

    /// Counts one application outcome.
    fn record(&mut self, outcome: ZoneOutcome) {
        match outcome {
            ZoneOutcome::Removed => self.removed += 1,
            ZoneOutcome::Restored => self.restored += 1,
            ZoneOutcome::Unbound => self.unbound += 1,
            ZoneOutcome::Unmanaged => self.unmanaged += 1,
            ZoneOutcome::Released => self.released += 1,
            ZoneOutcome::Unchanged => {}
        }
    }
}

/// The outcome of one zone's application attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ZoneOutcome {
    Removed,
    Restored,
    Unchanged,
    Unbound,
    Unmanaged,
    Released,
}

/// The entity bound to `(actor, node)`, when one carries the record.
fn bound_entity(world: &World, actor: ActorId, node: &DamageNodeKey) -> Option<Entity> {
    world.iter_entities().find_map(|entity| {
        let binding = entity.get::<DamageZoneBinding>()?;
        (binding.actor == actor && &binding.node == node).then_some(entity.id())
    })
}

/// Appends one event to the log, creating the resource if it is absent.
fn log_damage_collider_event(world: &mut World, event: DamageColliderEvent) {
    let mut log = world
        .remove_resource::<DamageColliderLog>()
        .unwrap_or_default();
    log.push(event);
    world.insert_resource(log);
}

/// Applies one zone's collider decision through the F20-C.04 seam and records
/// what happened.
///
/// The entity is resolved from the zone's own [`DamageZoneBinding`]; a
/// [`ColliderDecisionError`] is handled explicitly rather than propagated,
/// because the two arms mean different things: an
/// [`UnknownEntity`](ColliderDecisionError::UnknownEntity) is a released node
/// and a no-op, an
/// [`UnmanagedNode`](ColliderDecisionError::UnmanagedNode) is a wiring gap and
/// is reported. Every other refusal is likewise logged, never swallowed.
///
/// The `UnknownEntity` arm is defensive: `bound_entity` only yields entities
/// that are in the world, so a zone whose entity the teardown released is
/// reported as [`DamageColliderEvent::UnboundZone`] rather than
/// [`DamageColliderEvent::ReleasedNode`]. The arm stays because the seam's
/// error type can carry it and the match must stay exhaustive.
fn apply_zone_decision(
    world: &mut World,
    actor: ActorId,
    node: &DamageNodeKey,
    decision: Option<ZoneColliderDecision>,
) -> ZoneOutcome {
    let Some(decision) = decision else {
        return ZoneOutcome::Unchanged;
    };
    let Some(entity) = bound_entity(world, actor, node) else {
        log_damage_collider_event(
            world,
            DamageColliderEvent::UnboundZone {
                actor,
                node: node.clone(),
            },
        );
        return ZoneOutcome::Unbound;
    };
    let result = match decision {
        ZoneColliderDecision::Remove => remove_collider_for_damage(world, entity),
        ZoneColliderDecision::Restore => restore_collider_after_repair(world, entity),
    };
    match result {
        Ok(false) => ZoneOutcome::Unchanged,
        Ok(true) => {
            let event = match decision {
                ZoneColliderDecision::Remove => DamageColliderEvent::Removed {
                    actor,
                    node: node.clone(),
                    entity,
                },
                ZoneColliderDecision::Restore => DamageColliderEvent::Restored {
                    actor,
                    node: node.clone(),
                    entity,
                },
            };
            log_damage_collider_event(world, event);
            match decision {
                ZoneColliderDecision::Remove => ZoneOutcome::Removed,
                ZoneColliderDecision::Restore => ZoneOutcome::Restored,
            }
        }
        Err(ColliderDecisionError::UnmanagedNode(_)) => {
            log_damage_collider_event(
                world,
                DamageColliderEvent::UnmanagedNode {
                    actor,
                    node: node.clone(),
                    entity,
                },
            );
            ZoneOutcome::Unmanaged
        }
        Err(ColliderDecisionError::UnknownEntity(_)) => {
            log_damage_collider_event(
                world,
                DamageColliderEvent::ReleasedNode {
                    actor,
                    node: node.clone(),
                    entity,
                },
            );
            ZoneOutcome::Released
        }
    }
}

/// Applies one actor's damage resolution to the collider policy: every
/// [`PartTransition`](DamageEventKind::PartTransition) is decided through
/// [`zone_collider_decision`] and applied to the zone's own entity.
///
/// `events` is the resolution the session's [`DamageResolver`](cs_sim::damage::DamageResolver)
/// produced. The resolver resolves hits for many actors in one batch, while a
/// `PartTransition` names only the node, not its actor, so the caller passes
/// the victim `actor` the batch belongs to — the same actor it registered with
/// the resolver. Events that are not part transitions are ignored.
///
/// This is the production caller F20-C.04's seam was waiting for: without it a
/// destroyed part's collider state is whatever the spawner and the clip say.
/// The decision comes from the damage record alone. The seam is terminal for
/// the animation pass, so a clip that re-shows a destroyed part cannot restore
/// it — `apply_damage_events` never needs to re-assert the removal, and a
/// later clip verdict leaves it in place.
#[must_use]
pub fn apply_damage_events(
    world: &mut World,
    actor: ActorId,
    events: &[DamageEvent],
) -> DamageColliderReport {
    let mut report = DamageColliderReport::default();
    for event in events {
        let DamageEventKind::PartTransition { node, from, to } = &event.kind else {
            continue;
        };
        let outcome = apply_zone_decision(world, actor, node, zone_collider_decision(*from, *to));
        report.record(outcome);
    }
    report
}

/// The repair path's entry: lifts one zone's damage removal and restores its
/// collider.
///
/// The restoration goes through
/// [`restore_collider_after_repair`](crate::physics::restore_collider_after_repair),
/// which re-merges the clip's **current** verdict — so a repair under a clip
/// that still hides the node leaves the collider off, and the collider returns
/// when the clip shows the node again. Writing `Live` unconditionally would
/// expose a node the animation still hides, which is the invisible obstacle
/// from the other side.
///
/// The entry resolves the zone from its own [`DamageZoneBinding`] and reports
/// an unbound zone or a refusal through [`DamageColliderLog`] exactly as
/// [`apply_damage_events`] does.
#[must_use]
pub fn repair_damage_zone(
    world: &mut World,
    actor: ActorId,
    node: &DamageNodeKey,
) -> DamageColliderReport {
    let mut report = DamageColliderReport::default();
    let outcome = apply_zone_decision(world, actor, node, Some(ZoneColliderDecision::Restore));
    report.record(outcome);
    report
}

// ------------------------------------------ the damage → consumers seam ---
//
// F29-C. The resolver (F29-A/F29-B) is the *producer*: it owns the
// authoritative part and system state and emits the ordered events. These are
// the consumers the sheet names for this stage's minimum scenario — the weapon
// firing gate and the visual damage record. The firing gate is
// [`FireResolver`]'s per-mount disabled set, which `fire_one_mount` checks
// before a round is ever consumed (so a disabled mount cannot fire, and the
// refusal is `FireDenialReason::MountDisabled`); the visual record is
// [`AirframeDamageState`], whose single owner `scene::apply_airframe_damage`
// projects it onto the `NodeDisabled` marker and the render/LOD pass.
//
// The pass is **state-driven**, not event-replayed: it reads the resolver's
// [`part_state`](DamageResolver::part_state) and rewrites the consumers to
// agree with it. That is F29 non-negotiable 1 ("Damaged visuals consume
// authoritative state") and it makes the pass convergent — running it twice, or
// after a reload, changes nothing the second time. Every refusal (a foreign
// actor, an unarmed actor, a carrier with no gun, an unresolved pool, a binding
// that is not a scene node) is named in the returned [`DamageConsumerLog`]
// instead of being swallowed, the way [`apply_damage_events`] reports its
// collider refusals.
//
// [`apply_damage_state`] wires [`SystemKind::Weapon`];
// [`apply_propulsion_state`] wires [`SystemKind::Propulsion`] to the
// flight-authority gate (F29-C.1). Scoring and the bailout mission
// transition need consumers that do not exist on this branch yet; they are
// recorded as follow-ups, never guessed. Debris is no longer one of them: the
// state-driven pass in [`crate::debris`] reads the same authoritative
// `part_state` and spawns/despawns the authored debris instance itself, so
// this pass stays the gate + visual consumer it is. Nothing in this pass
// *decides* damage — it only reflects the resolver's state onto the two
// consumers.

/// Why the damage → consumer pass could not update a consumer.
///
/// A refusal is a returned record, not a dropped update: the pass stays
/// deterministic and says exactly which consumer it could not reach and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DamageConsumerRefusal {
    /// The actor's session is not the resolver's. A restarted or swapped actor
    /// is a new generation; nothing from the old one is applied.
    ForeignSession {
        /// The resolver's session generation.
        expected: u64,
        /// The session the named actor carried.
        found: u64,
    },
    /// The actor is not registered with the resolver.
    UnknownActor {
        /// The unknown actor.
        actor: ActorId,
    },
    /// The actor has a weapon carrier in its damage graph but no weapon state
    /// in the firing gate, so the carrier's mount cannot be named there at
    /// all. Reported whether the carrier is destroyed or intact: the gap is
    /// the missing gate registration, not the part's state.
    UnarmedActor {
        /// The actor with no registered weapons.
        actor: ActorId,
    },
    /// The firing gate has no gun on the carrier's mount, so disabling it
    /// would name a weapon that does not exist.
    UnmountedMount {
        /// The actor.
        actor: ActorId,
        /// The carrier's mount, which carries no gun.
        mount: DamageNodeKey,
    },
    /// A part declares a visual binding that is unresolved: its scene node
    /// cannot be named, so neither destroying nor repairing it is guessed.
    UnresolvedVisual {
        /// The actor.
        actor: ActorId,
        /// The part with the unresolved binding.
        node: DamageNodeKey,
        /// The claim the unknown binding is recorded under.
        claim_id: ClaimId,
        /// Why the binding is unknown.
        reason: String,
    },
    /// A part's scene binding does not name a `scene_node`, so it cannot be a
    /// visual part of the live scene.
    NonSceneVisual {
        /// The actor.
        actor: ActorId,
        /// The part with the non-scene binding.
        node: DamageNodeKey,
        /// The binding's actual catalog id.
        id: ContentId,
    },
    /// A part's integrity is unresolved, so whether it is destroyed cannot be
    /// asserted; the consumer is left exactly as it is.
    UnresolvedIntegrity {
        /// The actor.
        actor: ActorId,
        /// The part with the unresolved pool.
        node: DamageNodeKey,
        /// The claim the unknown pool is recorded under.
        claim_id: ClaimId,
        /// Why the pool is unknown.
        reason: String,
    },
}

/// One update the damage → consumer pass applied, or one refusal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DamageConsumerEvent {
    /// A destroyed weapon carrier's mount was disabled in the firing gate.
    MountDisabled {
        /// The actor.
        actor: ActorId,
        /// The disabled mount.
        mount: DamageNodeKey,
    },
    /// A mount the state says is not destroyed was re-enabled — a repair, or
    /// the convergence that clears a stale disable.
    MountEnabled {
        /// The actor.
        actor: ActorId,
        /// The re-enabled mount.
        mount: DamageNodeKey,
    },
    /// A destroyed part was recorded as destroyed in the visual state.
    VisualDestroyed {
        /// The actor.
        actor: ActorId,
        /// The destroyed part.
        node: DamageNodeKey,
        /// The visual part it presents as.
        scene_node: SceneNodeId,
    },
    /// A part the state says is not destroyed was recorded as repaired in the
    /// visual state.
    VisualRepaired {
        /// The actor.
        actor: ActorId,
        /// The repaired part.
        node: DamageNodeKey,
        /// The visual part it presents as.
        scene_node: SceneNodeId,
    },
    /// A disabled propulsion system cut the actor's thrust authority to zero.
    ThrustCut {
        /// The actor.
        actor: ActorId,
    },
    /// An enabled propulsion system lifted the gate's own cut: the thrust
    /// authority is back at full — a repair, or the convergence that clears a
    /// stale cut.
    ThrustRestored {
        /// The actor.
        actor: ActorId,
    },
    /// The update could not be applied; see [`DamageConsumerRefusal`].
    Refused(DamageConsumerRefusal),
}

/// The append-only record of what the damage → consumer pass changed, oldest
/// first.
///
/// A log entry is written only for a real change or a refusal, never for a
/// consumer that already agreed with the state. Re-running over an unchanged
/// state logs nothing more for a consumer that has converged; a refusal is
/// reported again on every pass, because the gap it names is still unresolved
/// and [`apply_damage_state`] holds no "already reported" set of its own.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct DamageConsumerLog {
    events: Vec<DamageConsumerEvent>,
}

impl DamageConsumerLog {
    /// An empty log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends one event.
    pub fn push(&mut self, event: DamageConsumerEvent) {
        self.events.push(event);
    }

    /// Every event, oldest first.
    #[must_use]
    pub fn events(&self) -> &[DamageConsumerEvent] {
        &self.events
    }

    /// The most recent event.
    #[must_use]
    pub fn last(&self) -> Option<&DamageConsumerEvent> {
        self.events.last()
    }

    /// How many events the log holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the log is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// What one damage → consumer pass changed.
///
/// The counters are how a caller observes convergence: the same state applied
/// twice reports zero the second time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DamageConsumerReport {
    /// Destroyed carriers whose mount was disabled.
    pub mounts_disabled: u32,
    /// Not-destroyed carriers whose mount was re-enabled.
    pub mounts_enabled: u32,
    /// Parts newly recorded destroyed in the visual state.
    pub visuals_destroyed: u32,
    /// Parts newly recorded repaired in the visual state.
    pub visuals_repaired: u32,
    /// Flight gates whose thrust authority a disabled propulsion system cut.
    pub thrust_cut: u32,
    /// Flight gates whose thrust authority an enabled propulsion system
    /// restored.
    pub thrust_restored: u32,
    /// Updates that could not be applied.
    pub refused: u32,
}

impl DamageConsumerReport {
    /// Whether the pass changed nothing and refused nothing.
    #[must_use]
    pub const fn is_noop(&self) -> bool {
        self.mounts_disabled == 0
            && self.mounts_enabled == 0
            && self.visuals_destroyed == 0
            && self.visuals_repaired == 0
            && self.thrust_cut == 0
            && self.thrust_restored == 0
            && self.refused == 0
    }
}

/// One damage → consumer pass's report and its log.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DamageConsumerOutcome {
    /// What the pass changed.
    pub report: DamageConsumerReport,
    /// Every change and refusal, oldest first.
    pub log: DamageConsumerLog,
}

/// Applies one actor's authoritative damage state to the consumers it drives:
/// the weapon firing gate and the visual damage record.
///
/// For every node of the actor's registered graph this pass:
///
/// * records the part as destroyed in `visuals` when its
///   [`PartState`] is [`Destroyed`](PartState::Destroyed), and as repaired
///   when it is `Intact` or `Damaged` — a part with no visual binding is
///   simply skipped, and a declared-but-unresolved binding is refused rather
///   than guessed;
/// * disables the mount on a destroyed [`SystemKind::Weapon`] carrier in
///   `fire`, and re-enables it when the state says it is not destroyed, so the
///   gate stops firing exactly while the mount is a wreck and comes back on a
///   repair.
///
/// `log`-free callers read the returned [`DamageConsumerOutcome`]; an
/// `Unknown` integrity asserts neither direction and is refused by name.
#[must_use]
pub fn apply_damage_state(
    resolver: &DamageResolver,
    actor: ActorId,
    fire: &mut FireResolver,
    visuals: &mut AirframeDamageState,
) -> DamageConsumerOutcome {
    let mut outcome = DamageConsumerOutcome::default();

    if actor.session != resolver.session() {
        refusals::foreign_session(&mut outcome, resolver.session().get(), actor.session.get());
        return outcome;
    }
    let Some(graph) = resolver.graph(&actor) else {
        refusals::unknown_actor(&mut outcome, actor);
        return outcome;
    };

    // An unarmed actor is reported once, not once per carrier.
    let mut unarmed_reported = false;
    for node in graph.nodes() {
        let node_key = node.key();
        let destroyed = match resolver.part_state(&actor, node_key) {
            Some(PartState::Destroyed) => Some(true),
            Some(PartState::Intact | PartState::Damaged) => Some(false),
            // Unresolved, or a node the resolver does not know: assert nothing.
            Some(PartState::Unknown) | None => None,
        };

        if destroyed.is_none() {
            refusals::unresolved_integrity(&mut outcome, actor, node);
        } else {
            apply_visual(&mut outcome, actor, node, destroyed == Some(true), visuals);
        }

        if node.disables() == Some(SystemKind::Weapon) {
            if fire.state(&actor).is_none() {
                if !unarmed_reported {
                    unarmed_reported = true;
                    refusals::unarmed_actor(&mut outcome, actor);
                }
                continue;
            }
            if fire.definition(&actor, node_key).is_none() {
                refusals::unmounted_mount(&mut outcome, actor, node_key.clone());
                continue;
            }
            let Some(destroyed) = destroyed else {
                // Already reported as an unresolved integrity above.
                continue;
            };
            apply_mount(&mut outcome, actor, node_key, destroyed, fire);
        }
    }

    outcome
}

/// Records one part's destroyed/repaired state in the visual record, or
/// refuses a binding that cannot name a scene node.
fn apply_visual(
    outcome: &mut DamageConsumerOutcome,
    actor: ActorId,
    node: &DamageNode,
    destroyed: bool,
    visuals: &mut AirframeDamageState,
) {
    let scene_node = match node.scene_binding() {
        None => return,
        Some(Resolved::Known(known)) => match SceneNodeId::from_content_id(known.value.clone()) {
            Ok(scene_node) => scene_node,
            Err(_) => {
                outcome.report.refused += 1;
                outcome.log.push(DamageConsumerEvent::Refused(
                    DamageConsumerRefusal::NonSceneVisual {
                        actor,
                        node: node.key().clone(),
                        id: known.value.clone(),
                    },
                ));
                return;
            }
        },
        Some(Resolved::Unknown { claim_id, reason }) => {
            outcome.report.refused += 1;
            outcome.log.push(DamageConsumerEvent::Refused(
                DamageConsumerRefusal::UnresolvedVisual {
                    actor,
                    node: node.key().clone(),
                    claim_id: claim_id.clone(),
                    reason: reason.clone(),
                },
            ));
            return;
        }
    };

    if destroyed {
        let was = visuals.is_destroyed(&scene_node);
        visuals.destroy(scene_node.clone());
        if !was {
            outcome.report.visuals_destroyed += 1;
            outcome.log.push(DamageConsumerEvent::VisualDestroyed {
                actor,
                node: node.key().clone(),
                scene_node,
            });
        }
    } else if visuals.repair(&scene_node) {
        outcome.report.visuals_repaired += 1;
        outcome.log.push(DamageConsumerEvent::VisualRepaired {
            actor,
            node: node.key().clone(),
            scene_node,
        });
    }
}

/// Disables or re-enables one mount on the firing gate, logging a real change.
fn apply_mount(
    outcome: &mut DamageConsumerOutcome,
    actor: ActorId,
    mount: &DamageNodeKey,
    destroyed: bool,
    fire: &mut FireResolver,
) {
    let state = fire
        .state_mut(&actor)
        .expect("the caller checked the actor has weapon state");
    let was_disabled = state.is_disabled(mount);
    if destroyed {
        state.disable(mount);
        if !was_disabled {
            outcome.report.mounts_disabled += 1;
            outcome.log.push(DamageConsumerEvent::MountDisabled {
                actor,
                mount: mount.clone(),
            });
        }
    } else {
        state.enable(mount);
        if was_disabled {
            outcome.report.mounts_enabled += 1;
            outcome.log.push(DamageConsumerEvent::MountEnabled {
                actor,
                mount: mount.clone(),
            });
        }
    }
}

/// Authored initial damage for a world object: the damage node it is and the
/// integrity it starts without.
///
/// Neither half is derivable from
/// [`WorldInstance::initially_damaged`] (a bare set of object ids), so the
/// caller supplies them from authored data; an object without an entry is
/// reported, never given a guessed node or amount (task #515).
pub type WorldDamageMapping = BTreeMap<WorldObjectId, InitialDamage>;

/// What [`apply_world_initial_damage`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldInitialDamageReport {
    /// The resolver's account of the mapped statements it applied or refused.
    pub resolver: InitialDamageReport,
    /// Objects the load starts damaged that the mapping says nothing about.
    pub unmapped_objects: Vec<WorldObjectId>,
}

/// Starts `actor`'s damage graph in the state `instance` authors.
///
/// Each object of [`WorldInstance::initially_damaged`] is looked up in
/// `mapping`; mapped ones go to
/// [`DamageResolver::apply_initial_damage`], unmapped ones are named in
/// [`WorldInitialDamageReport::unmapped_objects`]. Mapping entries for
/// objects the load does not start damaged are ignored.
///
/// # Errors
///
/// Any [`cs_sim::damage::DamageError`] the resolver refuses with.
pub fn apply_world_initial_damage(
    resolver: &mut DamageResolver,
    actor: &ActorId,
    instance: &WorldInstance,
    mapping: &WorldDamageMapping,
) -> Result<WorldInitialDamageReport, cs_sim::damage::DamageError> {
    let mut mapped = Vec::new();
    let mut unmapped_objects = Vec::new();
    for object in instance.initially_damaged() {
        match mapping.get(object) {
            Some(damage) => mapped.push(damage.clone()),
            None => unmapped_objects.push(object.clone()),
        }
    }
    let resolver = resolver.apply_initial_damage(actor, &mapped)?;
    Ok(WorldInitialDamageReport {
        resolver,
        unmapped_objects,
    })
}

/// Applies one actor's authoritative propulsion state to its flight-authority
/// gate: the [`DamageState::thrust_authority`] the [`FlightAircraft`]'s
/// equations scale thrust and boost by.
///
/// The pass reads [`DamageResolver::system_state`] for
/// [`SystemKind::Propulsion`] — the aggregate the resolver already owns, never
/// a replay of `SystemDisabled` events — and:
///
/// * [`SystemState::Disabled`] (any declaring part destroyed) cuts the thrust
///   authority to `0`;
/// * [`SystemState::Enabled`] lifts the gate's own cut: a thrust authority of
///   exactly `0` goes back to [`DamageState::PRISTINE`]'s `1`, and any other
///   value is left to the producer that wrote it;
/// * [`SystemState::Unknown`] asserts neither direction: every unresolved
///   declaring pool is refused by name and the gate is left as it is;
/// * an actor whose graph declares no propulsion carrier leaves the gate
///   untouched — "this actor has no engine part" is not "its engine is down".
///
/// Control authority and lift scale are not this system's and are never
/// written. A foreign session generation and an unregistered actor are
/// refused by name and change nothing.
///
/// **Designed rule, not original data.** Whether the original cut thrust on a
/// destroyed engine, and how it aggregated several engines, is unmeasured; see
/// `docs/findings/2026-10-05-f29-c1-propulsion-consumer.md`.
#[must_use]
pub fn apply_propulsion_state(
    resolver: &DamageResolver,
    actor: ActorId,
    flight: &mut FlightAircraft,
) -> DamageConsumerOutcome {
    let mut outcome = DamageConsumerOutcome::default();

    if actor.session != resolver.session() {
        refusals::foreign_session(&mut outcome, resolver.session().get(), actor.session.get());
        return outcome;
    }
    let Some(graph) = resolver.graph(&actor) else {
        refusals::unknown_actor(&mut outcome, actor);
        return outcome;
    };

    let damage = flight.damage();
    match resolver.system_state(&actor, SystemKind::Propulsion) {
        // No propulsion carrier declared: nothing to gate.
        None => {}
        Some(SystemState::Disabled) => {
            if damage.thrust_authority != 0.0 {
                set_thrust_authority(flight, damage, 0.0);
                outcome.report.thrust_cut += 1;
                outcome.log.push(DamageConsumerEvent::ThrustCut { actor });
            }
        }
        Some(SystemState::Enabled) => {
            if damage.thrust_authority == 0.0 {
                set_thrust_authority(flight, damage, DamageState::PRISTINE.thrust_authority);
                outcome.report.thrust_restored += 1;
                outcome
                    .log
                    .push(DamageConsumerEvent::ThrustRestored { actor });
            }
        }
        Some(SystemState::Unknown) => {
            for node in graph.nodes() {
                if node.disables() == Some(SystemKind::Propulsion)
                    && resolver.part_state(&actor, node.key()) == Some(PartState::Unknown)
                {
                    refusals::unresolved_integrity(&mut outcome, actor, node);
                }
            }
        }
    }

    outcome
}

/// Rewrites only the thrust authority of a validated damage state.
fn set_thrust_authority(flight: &mut FlightAircraft, damage: DamageState, thrust_authority: f64) {
    flight
        .set_damage(DamageState {
            thrust_authority,
            ..damage
        })
        .expect("a validated damage state with a thrust authority of 0 or 1 stays valid");
}

/// The refusal constructors, so [`apply_damage_state`] stays readable.
mod refusals {
    use cs_sim::damage::{ActorId, DamageNode, DamageNodeKey};
    use cs_types::content::Resolved;
    use cs_types::evidence::ClaimId;

    use super::{DamageConsumerOutcome, DamageConsumerRefusal};

    /// The actor belongs to another session generation.
    pub(super) fn foreign_session(outcome: &mut DamageConsumerOutcome, expected: u64, found: u64) {
        outcome.report.refused += 1;
        outcome.log.push(super::DamageConsumerEvent::Refused(
            DamageConsumerRefusal::ForeignSession { expected, found },
        ));
    }

    /// The actor is not registered with the resolver.
    pub(super) fn unknown_actor(outcome: &mut DamageConsumerOutcome, actor: ActorId) {
        outcome.report.refused += 1;
        outcome.log.push(super::DamageConsumerEvent::Refused(
            DamageConsumerRefusal::UnknownActor { actor },
        ));
    }

    /// A destroyed weapon carrier on an actor with no weapon state.
    pub(super) fn unarmed_actor(outcome: &mut DamageConsumerOutcome, actor: ActorId) {
        outcome.report.refused += 1;
        outcome.log.push(super::DamageConsumerEvent::Refused(
            DamageConsumerRefusal::UnarmedActor { actor },
        ));
    }

    /// The firing gate has no gun on the carrier's mount.
    pub(super) fn unmounted_mount(
        outcome: &mut DamageConsumerOutcome,
        actor: ActorId,
        mount: DamageNodeKey,
    ) {
        outcome.report.refused += 1;
        outcome.log.push(super::DamageConsumerEvent::Refused(
            DamageConsumerRefusal::UnmountedMount { actor, mount },
        ));
    }

    /// A part's integrity is unresolved; neither direction is asserted.
    pub(super) fn unresolved_integrity(
        outcome: &mut DamageConsumerOutcome,
        actor: ActorId,
        node: &DamageNode,
    ) {
        let (claim_id, reason) = match node.integrity() {
            Resolved::Unknown { claim_id, reason } => (claim_id.clone(), reason.clone()),
            // `part_state` only reports `Unknown` for an unresolved pool, so
            // this arm is unreachable; it keeps the refusal total rather than
            // unwrapping a condition the caller already knows.
            Resolved::Known(_) => (
                ClaimId::new("f29c.unknown-integrity").expect("the claim id is valid"),
                "the pool state is unknown".to_owned(),
            ),
        };
        outcome.report.refused += 1;
        outcome.log.push(super::DamageConsumerEvent::Refused(
            DamageConsumerRefusal::UnresolvedIntegrity {
                actor,
                node: node.key().clone(),
                claim_id,
                reason,
            },
        ));
    }
}
