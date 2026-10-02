//! The damage application boundary (F29-A).
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-A`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
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

use bevy::ecs::component::Component;
use bevy::prelude::{Entity, Resource, World};
use cs_content::damage::{
    AttributionRule as DeclaredAttributionRule, DamageNodeKind as DeclaredNodeKind,
    DeclaredDamageGraph, DeclaredDamageNode, SystemKind as DeclaredSystemKind,
};
use cs_content::scene::SceneNodeId;
use cs_sim::damage::{
    ActorId, AttributionRule, DamageEvent, DamageEventKind, DamageGraph, DamageGraphError,
    DamageNode, DamageNodeKey, DamageNodeKind, DamagePolicy, NodeKeyError, PartState, SystemKind,
};
use cs_types::content::{ContentId, Known, Resolved};
use cs_types::evidence::ClaimId;

use crate::physics::{
    ColliderDecisionError, remove_collider_for_damage, restore_collider_after_repair,
};
use crate::scene::SceneGeneration;

/// Why a declared graph could not be lowered to the runtime records.
#[derive(Clone, Debug, PartialEq)]
pub enum DamageLowerError {
    /// A declared node key could not form a runtime key — unreachable
    /// while both crates apply the same grammar, kept so the boundary
    /// stays honest if they ever diverge.
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
/// [`DamageLowerError::NodeKey`] when a declared key cannot form a runtime
/// key, [`DamageLowerError::Graph`] when
/// [`cs_sim::damage::DamageGraph::try_new`] refuses the assembled record.
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
    DamageNodeKey::new(key.as_str()).map_err(|source| DamageLowerError::NodeKey {
        key: key.as_str().to_owned(),
        source,
    })
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
    /// A destroyed zone's collider left the simulation.
    Removed {
        /// The zone's actor.
        actor: ActorId,
        /// The destroyed zone.
        node: DamageNodeKey,
        /// The collider-managed entity.
        entity: Entity,
    },
    /// A repaired zone's collider returned.
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
    /// The bound entity was already gone when the decision arrived — the
    /// teardown released it. A no-op (the removal changed nothing), recorded so
    /// the release is visible rather than silent.
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
/// `removed` and `restored` count applied changes; `unbound`, `unmanaged` and
/// `released` count the three reasons nothing was applied. The counters are
/// how a caller observes idempotence: applying the same transition twice
/// reports zero on the second call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DamageColliderReport {
    /// Destroyed zones whose collider left the simulation.
    pub removed: u32,
    /// Repaired zones whose collider returned.
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
