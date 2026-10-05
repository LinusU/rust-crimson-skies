//! The damage → debris consumer (F29-C.2).
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-C`, task F29-C.2 "Spawn authored debris when a damage part is
//! destroyed". Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! [`crate::damage`] wired two consumers of the resolver's authoritative part
//! state — the weapon firing gate and the visual damage record — and named
//! debris as the follow-up this module answers
//! (`docs/findings/2026-10-02-f29-c-damage-consumers.md`). The producer is the
//! same one: [`DamageResolver::part_state`], never a particle, a material or a
//! replayed [`PartTransition`](cs_sim::damage::DamageEventKind::PartTransition)
//! (F29 non-negotiable 1).
//!
//! What lives here, and what deliberately does not:
//!
//! * [`PartDebrisBinding`] is the spawn path's record of *what* one part's
//!   destruction presents: a `Resolved<ContentId>` naming the authored
//!   debris object, on the entity the part is, under the scene generation that
//!   spawned it. The debris **asset** and the machinery that turns an authored
//!   object into a rendered, simulated wreck belong to another stage; this
//!   module never loads, places or renders one, and it never invents an
//!   object for a part the spawn path did not author one for.
//! * [`SpawnedDebris`] is what this pass spawns: one entity per
//!   `(actor, node)`, carrying the authored content id it was spawned from and
//!   the generation that spawned it, so a later stage resolves the object by
//!   id and a reload identifies a leftover by mismatch rather than by a
//!   surviving pointer (the `STATE-TRANSACTIONS` generation discipline, the
//!   same rule [`crate::scene::SceneNodeBinding`] and
//!   [`crate::damage::DamageZoneBinding`] follow).
//! * [`apply_debris_state`] is the pass. It is **state-driven** and
//!   convergent exactly like [`crate::damage::apply_damage_state`]: a
//!   destroyed part with a resolved binding spawns its debris once, a second
//!   pass spawns nothing, and a part the authority no longer calls destroyed
//!   has its debris despawned. A declared-but-unresolved binding is refused
//!   by name with its claim instead of guessing an object, and a part with no
//!   binding at all spawns nothing — whether the original authored debris for
//!   every part is unmeasured (see below), so absence is not an error.
//! * [`release_debris`] is the teardown entry a reload, restart or aircraft
//!   swap calls: it despawns every instance the actor owns in one call and
//!   reports how many went, so a second teardown is a no-op.
//!
//! # Designed rule, not original data
//!
//! Whether the original spawned authored debris when a damage part was
//! destroyed, which object it used, and how many instances a destruction
//! produced are **unmeasured** — F29's "Research boundary" and F29-D keep the
//! original-family gate. The rule implemented here — one debris instance per
//! destroyed part that carries an authored binding, gone again when the part
//! is not destroyed — is this engine's designed behavior, recorded in
//! `docs/findings/2026-10-05-f29-c-2-debris-spawn-consumer.md`. **Affected
//! content:** every airframe and world object whose damage graph a session
//! registers, as soon as its spawn path publishes [`PartDebrisBinding`]s.

use std::collections::BTreeMap;

use bevy::ecs::component::Component;
use bevy::prelude::{Entity, World};
use cs_sim::damage::{ActorId, DamageNodeKey, DamageResolver, PartState};
use cs_types::content::{ContentId, Resolved};
use cs_types::evidence::ClaimId;

use crate::scene::SceneGeneration;

/// Component: the authored debris one damage part presents when it is
/// destroyed.
///
/// This is the debris side's **own** record of *what* a part's destruction
/// spawns: the session-qualified damage graph node whose destruction spawns
/// the debris, and the authored object (`Resolved<ContentId>`) it spawns. The
/// spawn path (another stage's) inserts it beside the part's own binding, so
/// [`apply_debris_state`] never has to derive an object from a material, a
/// particle or a scene node that happens to be nearby.
///
/// An explicit [`Resolved::Unknown`] is a first-class value: it is refused
/// with its claim by [`apply_debris_state`], never replaced by a stand-in.
/// A part with **no** binding is simply not an authored debris part — the
/// original's per-part debris authoring is unmeasured, so absence is not an
/// error and never produces a guessed instance.
///
/// A `(actor, node)` pair identifies at most one bound entity, the same rule
/// [`crate::damage::DamageZoneBinding`] follows; the resolver's node keys are
/// graph-local, so the session-qualified [`ActorId`] is part of the identity.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct PartDebrisBinding {
    /// The session-qualified actor the part belongs to.
    pub actor: ActorId,
    /// The damage graph node whose destruction presents the debris.
    pub node: DamageNodeKey,
    /// The authored debris object, or the explicit unknown the content
    /// boundary recorded for it.
    pub debris: Resolved<ContentId>,
    /// The scene generation that spawned the binding, so a reload's bindings
    /// are distinguishable from the ones it superseded.
    pub generation: SceneGeneration,
}

impl PartDebrisBinding {
    /// Binds one entity's part to what its destruction spawns.
    #[must_use]
    pub fn new(
        actor: ActorId,
        node: DamageNodeKey,
        debris: Resolved<ContentId>,
        generation: SceneGeneration,
    ) -> Self {
        Self {
            actor,
            node,
            debris,
            generation,
        }
    }
}

/// Component: one spawned debris instance of one destroyed part.
///
/// The pass spawns at most one of these per `(actor, node)`, stamped with the
/// authored `source` it came from and the [`SceneGeneration`] that was live
/// when it spawned. It carries **no** mesh, pose, velocity or collider: which
/// authored object the `source` resolves to, where the wreckage sits and how
/// it moves are the debris presentation stage's decisions, and a
/// [`SpawnedDebris`] alone is the record this consumer owns.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct SpawnedDebris {
    /// The actor whose part spawned the debris.
    pub actor: ActorId,
    /// The destroyed part it was spawned for.
    pub node: DamageNodeKey,
    /// The authored debris object the binding resolved to.
    pub source: ContentId,
    /// The scene generation the instance was spawned under.
    pub generation: SceneGeneration,
}

/// Why the damage → debris pass could not update its consumer.
///
/// A refusal is a returned record, not a dropped update: the pass says
/// exactly which part it could not act for and why, the way
/// [`crate::damage::DamageConsumerRefusal`] does for the firing gate and the
/// visual record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DamageDebrisRefusal {
    /// The actor's session is not the resolver's. A restarted or swapped actor
    /// is a new generation; nothing from the old one is spawned.
    ForeignSession {
        /// The resolver's session generation.
        expected: u64,
        /// The session the named actor carried.
        found: u64,
    },
    /// The actor is not registered with the resolver, so nothing about its
    /// parts is authoritative.
    UnknownActor {
        /// The unknown actor.
        actor: ActorId,
    },
    /// A destroyed part declares an unresolved debris binding: the authored
    /// object cannot be named, so neither spawning nor removing it is
    /// guessed.
    UnresolvedDebris {
        /// The actor.
        actor: ActorId,
        /// The part whose binding is unresolved.
        node: DamageNodeKey,
        /// The claim the unknown binding is recorded under.
        claim_id: ClaimId,
        /// Why the binding is unknown.
        reason: String,
    },
    /// A part's integrity is unresolved, so whether it is destroyed cannot be
    /// asserted; its debris is left exactly as it is.
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

/// One update the damage → debris pass applied, or one refusal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DamageDebrisEvent {
    /// A destroyed part's authored debris was spawned.
    Spawned {
        /// The actor.
        actor: ActorId,
        /// The destroyed part.
        node: DamageNodeKey,
        /// The authored object it was spawned from.
        source: ContentId,
        /// The generation the instance was stamped with.
        generation: SceneGeneration,
        /// The instance entity.
        entity: Entity,
    },
    /// An instance left: the part is not destroyed any more, or a reload
    /// superseded the instance with one stamped under the live binding.
    Despawned {
        /// The actor.
        actor: ActorId,
        /// The part whose debris left.
        node: DamageNodeKey,
        /// The authored object it had been spawned from.
        source: ContentId,
        /// The generation it had been stamped with.
        generation: SceneGeneration,
        /// The instance entity that was despawned.
        entity: Entity,
    },
    /// The update could not be applied; see [`DamageDebrisRefusal`].
    Refused(DamageDebrisRefusal),
}

/// The append-only record of what the damage → debris pass changed, oldest
/// first.
///
/// A log entry is written only for a real change or a refusal, never for a
/// part that already agrees with the state: a re-run over an unchanged
/// authority logs nothing more, while a refusal is reported again on every
/// pass, because the gap it names is still unresolved and the pass holds no
/// "already reported" set of its own (the same rule
/// [`crate::damage::DamageConsumerLog`] documents).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DamageDebrisLog {
    events: Vec<DamageDebrisEvent>,
}

impl DamageDebrisLog {
    /// Appends one event.
    pub fn push(&mut self, event: DamageDebrisEvent) {
        self.events.push(event);
    }

    /// Every event, oldest first.
    #[must_use]
    pub fn events(&self) -> &[DamageDebrisEvent] {
        &self.events
    }

    /// The most recent event.
    #[must_use]
    pub fn last(&self) -> Option<&DamageDebrisEvent> {
        self.events.last()
    }

    /// Whether the log is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// What one damage → debris pass changed.
///
/// The counters are how a caller observes convergence: the same state applied
/// twice reports zero the second time, and `spawned` counts instances, never
/// attempts — a part that already has its instance spawns nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DamageDebrisReport {
    /// Destroyed parts whose debris instance was spawned.
    pub spawned: u32,
    /// Instances despawned: a part that is not destroyed any more, or a
    /// superseded instance replaced by the live binding's.
    pub despawned: u32,
    /// Updates that could not be applied.
    pub refused: u32,
}

impl DamageDebrisReport {
    /// Whether the pass changed nothing and refused nothing.
    #[must_use]
    pub const fn is_noop(&self) -> bool {
        self.spawned == 0 && self.despawned == 0 && self.refused == 0
    }
}

/// One damage → debris pass's report and its log.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DamageDebrisOutcome {
    /// What the pass changed.
    pub report: DamageDebrisReport,
    /// Every change and refusal, oldest first.
    pub log: DamageDebrisLog,
}

/// One existing instance, as the pass indexed it before it mutates the world.
struct DebrisInstance {
    entity: Entity,
    node: DamageNodeKey,
    source: ContentId,
    generation: SceneGeneration,
}

/// Applies one actor's authoritative damage state to its debris consumer:
/// spawns the authored debris of every destroyed part and despawns the debris
/// of every part the authority no longer calls destroyed.
///
/// For every node of the actor's registered graph this pass:
///
/// * spawns exactly one [`SpawnedDebris`] when the part is
///   [`Destroyed`](PartState::Destroyed) **and** its
///   [`PartDebrisBinding`] resolved: an instance that already matches the
///   binding's object and generation is kept, a superseded or duplicated one
///   is despawned first, so a part never carries two;
/// * despawns every instance the part owns when the state says it is not
///   destroyed — the repair/restart half of the teardown;
/// * refuses a declared-but-unresolved binding with its claim
///   ([`UnresolvedDebris`](DamageDebrisRefusal::UnresolvedDebris)) and an
///   unresolved pool with its claim
///   ([`UnresolvedIntegrity`](DamageDebrisRefusal::UnresolvedIntegrity)),
///   changing nothing in either case;
/// * skips a destroyed part with no binding silently: nothing authored
///   anything for it, and the original's per-part debris authoring is
///   unmeasured, so an absent binding is not a gap to report and never a
///   licence to guess an object.
///
/// `events` are unnecessary here: the pass reads the resolver's own
/// [`part_state`](DamageResolver::part_state), so running it twice, after a
/// reload, or against a stale world changes nothing the second time. A
/// foreign session generation or an unregistered actor is refused before any
/// part is read.
///
/// # Errors
///
/// None — a refusal is a returned record in
/// [`DamageDebrisOutcome`], not an `Err`, because the caller's job is to keep
/// the pass convergent rather than to abort the frame.
#[must_use]
pub fn apply_debris_state(
    world: &mut World,
    resolver: &DamageResolver,
    actor: ActorId,
) -> DamageDebrisOutcome {
    let mut outcome = DamageDebrisOutcome::default();

    if actor.session != resolver.session() {
        refusals::foreign_session(&mut outcome, resolver.session().get(), actor.session.get());
        return outcome;
    }
    let Some(graph) = resolver.graph(&actor) else {
        refusals::unknown_actor(&mut outcome, actor);
        return outcome;
    };

    // Both indexes are collected before the pass mutates the world, so no
    // borrow of the world survives into the spawn/despawn loop.
    let bindings = debris_bindings(world, actor);
    let mut instances = debris_instances(world, actor);

    for node in graph.nodes() {
        let node_key = node.key();
        let destroyed = match resolver.part_state(&actor, node_key) {
            Some(PartState::Destroyed) => Some(true),
            Some(PartState::Intact | PartState::Damaged) => Some(false),
            // Unresolved, or a node the resolver does not know: assert nothing.
            Some(PartState::Unknown) | None => None,
        };
        let Some(destroyed) = destroyed else {
            refusals::unresolved_integrity(&mut outcome, actor, node);
            continue;
        };

        if !destroyed {
            // The authority says the part is present again, so every instance
            // it owns goes — whether or not a binding survives, because the
            // authority, not the binding, decides what exists.
            for instance in instances.remove(node_key).unwrap_or_default() {
                despawn_instance(&mut outcome, world, actor, instance);
            }
            continue;
        }

        let Some(binding) = bindings.get(node_key) else {
            // No authored debris for this part: nothing to spawn and nothing
            // to report (see the module docs).
            continue;
        };
        let Resolved::Known(known) = &binding.debris else {
            refusals::unresolved_debris(&mut outcome, actor, node_key, &binding.debris);
            continue;
        };
        let source = known.value.clone();
        let generation = binding.generation;

        // Converge on exactly one instance with this binding's identity: the
        // first match stays, every other instance of the part (a duplicate, a
        // superseded generation, an object the binding no longer names) goes.
        let mut kept = false;
        for instance in instances.remove(node_key).unwrap_or_default() {
            if !kept && instance.source == source && instance.generation == generation {
                kept = true;
                continue;
            }
            despawn_instance(&mut outcome, world, actor, instance);
        }
        if !kept {
            spawn_instance(&mut outcome, world, actor, node_key, &source, generation);
        }
    }

    outcome
}

/// The teardown entry: despawns every debris instance `actor` owns, whatever
/// the authority currently says, and reports how many went.
///
/// This is the reload / restart / aircraft-swap half of the teardown. The
/// scene release path rebuilds entities under a new [`SceneGeneration`] and
/// cannot name the actor a leftover belongs to, so the caller that owns the
/// session's actors releases their debris beside their other bindings — the
/// same shape as [`crate::damage::repair_damage_zone`] being the repair
/// path's entry. It is convergent: a second release finds nothing and reports
/// `0`.
///
/// The state-driven half lives in [`apply_debris_state`], which despawns a
/// part's instance as soon as the authority stops calling it destroyed.
#[must_use]
pub fn release_debris(world: &mut World, actor: ActorId) -> u32 {
    let entities: Vec<Entity> = world
        .iter_entities()
        .filter_map(|entity_ref| {
            let debris = entity_ref.get::<SpawnedDebris>()?;
            (debris.actor == actor).then_some(entity_ref.id())
        })
        .collect();
    let mut released = 0;
    for entity in entities {
        world.entity_mut(entity).despawn();
        released += 1;
    }
    released
}

/// Indexes the debris bindings `actor`'s world carries, one per node: the
/// first bound entity for a `(actor, node)` pair wins, the same rule the
/// damage → collider bridge applies to colliders.
fn debris_bindings(world: &World, actor: ActorId) -> BTreeMap<DamageNodeKey, PartDebrisBinding> {
    let mut bindings = BTreeMap::new();
    for entity_ref in world.iter_entities() {
        let Some(binding) = entity_ref.get::<PartDebrisBinding>() else {
            continue;
        };
        if binding.actor != actor {
            continue;
        }
        bindings
            .entry(binding.node.clone())
            .or_insert_with(|| binding.clone());
    }
    bindings
}

/// Indexes the debris instances `actor`'s world carries, grouped by part, so
/// the pass can converge each part on exactly one instance.
fn debris_instances(world: &World, actor: ActorId) -> BTreeMap<DamageNodeKey, Vec<DebrisInstance>> {
    let mut instances: BTreeMap<DamageNodeKey, Vec<DebrisInstance>> = BTreeMap::new();
    for entity_ref in world.iter_entities() {
        let Some(debris) = entity_ref.get::<SpawnedDebris>() else {
            continue;
        };
        if debris.actor != actor {
            continue;
        }
        instances
            .entry(debris.node.clone())
            .or_default()
            .push(DebrisInstance {
                entity: entity_ref.id(),
                node: debris.node.clone(),
                source: debris.source.clone(),
                generation: debris.generation,
            });
    }
    instances
}

/// Spawns one instance and records it.
fn spawn_instance(
    outcome: &mut DamageDebrisOutcome,
    world: &mut World,
    actor: ActorId,
    node: &DamageNodeKey,
    source: &ContentId,
    generation: SceneGeneration,
) {
    let entity = world
        .spawn(SpawnedDebris {
            actor,
            node: node.clone(),
            source: source.clone(),
            generation,
        })
        .id();
    outcome.report.spawned += 1;
    outcome.log.push(DamageDebrisEvent::Spawned {
        actor,
        node: node.clone(),
        source: source.clone(),
        generation,
        entity,
    });
}

/// Despawns one existing instance and records it.
fn despawn_instance(
    outcome: &mut DamageDebrisOutcome,
    world: &mut World,
    actor: ActorId,
    instance: DebrisInstance,
) {
    world.entity_mut(instance.entity).despawn();
    outcome.report.despawned += 1;
    outcome.log.push(DamageDebrisEvent::Despawned {
        actor,
        node: instance.node,
        source: instance.source,
        generation: instance.generation,
        entity: instance.entity,
    });
}

/// The refusal constructors, so [`apply_debris_state`] stays readable.
mod refusals {
    use cs_sim::damage::{ActorId, DamageNode, DamageNodeKey};
    use cs_types::content::{ContentId, Resolved};
    use cs_types::evidence::ClaimId;

    use super::{DamageDebrisEvent, DamageDebrisOutcome, DamageDebrisRefusal};

    /// Records a refusal and counts it.
    pub(super) fn refuse(outcome: &mut DamageDebrisOutcome, refusal: DamageDebrisRefusal) {
        outcome.report.refused += 1;
        outcome.log.push(DamageDebrisEvent::Refused(refusal));
    }

    /// The actor belongs to another session generation.
    pub(super) fn foreign_session(outcome: &mut DamageDebrisOutcome, expected: u64, found: u64) {
        refuse(
            outcome,
            DamageDebrisRefusal::ForeignSession { expected, found },
        );
    }

    /// The actor is not registered with the resolver.
    pub(super) fn unknown_actor(outcome: &mut DamageDebrisOutcome, actor: ActorId) {
        refuse(outcome, DamageDebrisRefusal::UnknownActor { actor });
    }

    /// A destroyed part's authored debris binding is unresolved.
    pub(super) fn unresolved_debris(
        outcome: &mut DamageDebrisOutcome,
        actor: ActorId,
        node: &DamageNodeKey,
        binding: &Resolved<ContentId>,
    ) {
        let Resolved::Unknown { claim_id, reason } = binding else {
            // The caller only reaches this for an unknown binding.
            return;
        };
        refuse(
            outcome,
            DamageDebrisRefusal::UnresolvedDebris {
                actor,
                node: node.clone(),
                claim_id: claim_id.clone(),
                reason: reason.clone(),
            },
        );
    }

    /// A part's integrity is unresolved; neither direction is asserted.
    pub(super) fn unresolved_integrity(
        outcome: &mut DamageDebrisOutcome,
        actor: ActorId,
        node: &DamageNode,
    ) {
        let (claim_id, reason) = match node.integrity() {
            Resolved::Unknown { claim_id, reason } => (claim_id.clone(), reason.clone()),
            Resolved::Known(_) => (
                ClaimId::new("f29c2.unknown-integrity").expect("the claim id is valid"),
                "the pool state is unknown".to_owned(),
            ),
        };
        refuse(
            outcome,
            DamageDebrisRefusal::UnresolvedIntegrity {
                actor,
                node: node.key().clone(),
                claim_id,
                reason,
            },
        );
    }
}
