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
//!   [`cs_content::damage::AttributionRule`] becomes the resolver's
//!   [`cs_sim::damage::DamagePolicy`]. An `Unknown` attribution **refuses**
//!   rather than guessing: no session resolves kills under an unstated
//!   rule;
//! * [`DamageActorBinding`] — the ECS record tying an entity to its
//!   session-qualified [`cs_sim::damage::ActorId`] and damage-graph
//!   subject, generation-stamped like
//!   [`crate::scene::SceneNodeBinding`] so a reload can never leave a
//!   stale binding looking live.
//!
//! Nothing here owns damage state: pools, lifecycle records and event
//! sequences are the resolver's; these are the conversion and binding
//! records the ECS wiring consumes (F29-B/C).

use bevy::ecs::component::Component;
use cs_content::damage::{
    AttributionRule as DeclaredAttributionRule, DamageNodeKind as DeclaredNodeKind,
    DeclaredDamageGraph, DeclaredDamageNode, SystemKind as DeclaredSystemKind,
};
use cs_content::scene::SceneNodeId;
use cs_sim::damage::{
    ActorId, AttributionRule, DamageGraph, DamageGraphError, DamageNode, DamageNodeKey,
    DamageNodeKind, DamagePolicy, NodeKeyError, SystemKind,
};
use cs_types::content::{ContentId, Known, Resolved};
use cs_types::evidence::ClaimId;

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

/// Lowers the declared rules into the resolver's policy.
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
