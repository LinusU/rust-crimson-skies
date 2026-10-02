//! The declared damage-graph schema: provenance-carrying damage rules and
//! part records (F29-A).
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-A`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! This module is the **content half** of the damage contract — the
//! normalized record a content importer produces and the catalog consumes.
//! Its runtime counterpart is `cs_sim::damage` (the graph the resolver
//! runs); the conversion boundary between them is `cs_app::damage`. The
//! split mirrors `flight_tuning` ↔ `cs_sim::flight`: this crate cannot
//! depend on `cs_sim`, so the declared record keeps its own typed
//! vocabulary — node kinds, systems, channels and the attribution rule —
//! and the boundary maps it field-wise.
//!
//! # Records
//!
//! A [`DeclaredDamageGraph`] names its `subject` — the `airframe`, `world`
//! or other catalog id it describes — carries an [`Origin`], a
//! [`GraphSubjectKind`] (aircraft, world object, capital ship share the
//! identity discipline but keep their own rules, per the deliverable), the
//! declared [`GraphRules`] and the [`DeclaredDamageNode`]s.
//!
//! Every load-bearing value is a [`Resolved`]: integrity pools, scene
//! bindings and the lethal-attribution rule are each either known with
//! [`Provenance`] or an explicit unknown with its claim id and reason —
//! never a silent default (F14 non-negotiable behavior 3). A node's
//! `scene_binding` ties the part to its visual [`SceneNodeId`] for the
//! presentation consumer; damage decisions never read it (F29
//! non-negotiable behavior 1).
//!
//! [`DamageNodeKey`] applies the `IDENTITY-CONTENT` key grammar to
//! graph-local node identity — "the same identity discipline" every graph
//! kind shares. `cs_types` owns no shared key type yet, so this module and
//! `cs_sim::damage` each validate their own copy of the grammar; the
//! boundary maps them by text.
//!
//! # Designed vocabulary, not original data
//!
//! The original damage model — its zone set, whether armor pooled or
//! scaled, its overkill propagation, kill attribution and bailout rules —
//! is unrecovered (F29 "Research boundary"; the manual and guides name no
//! damage equations, `docs/research/FINDINGS.md`). Every kind, rule name
//! and fixture value here is **newly authored project design** carrying
//! `Origin::Designed`/`Origin::SyntheticFixture` provenance, recorded in
//! `docs/findings/2026-09-30-f29-a-damage-graphs-hit-ordering-lifecycle.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_types::content::{ContentId, Origin, Provenance, Resolved};

use crate::scene::SceneNodeId;

// The graph-local node identity and its validation are owned by `cs_types`
// (task #442). The declared schema and `cs_sim`'s runtime graph name the
// same [`DamageNodeKey`], so a declared key lowers to a runtime key with no
// re-validation and the `IDENTITY-CONTENT` grammar has one implementation.
pub use cs_types::content::{DamageNodeKey, DamageNodeKeyError, MAX_NODE_KEY_LEN};

/// What kind of part a declared damage node models (F29 deliverable).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DamageNodeKind {
    /// An armor plate or zone absorbing hits before the part it guards.
    ArmorZone,
    /// Internal structure: spars, hull, frame.
    InternalStructure,
    /// An engine.
    Engine,
    /// A weapon mount.
    WeaponMount,
}

impl DamageNodeKind {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::ArmorZone => "armor_zone",
            Self::InternalStructure => "internal_structure",
            Self::Engine => "engine",
            Self::WeaponMount => "weapon_mount",
        }
    }
}

impl fmt::Display for DamageNodeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A gameplay system a destroyed part disables.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SystemKind {
    /// Thrust/propulsion from an engine node.
    Propulsion,
    /// Firing capability of a weapon mount.
    Weapon,
}

impl SystemKind {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Propulsion => "propulsion",
            Self::Weapon => "weapon",
        }
    }
}

/// Which kind of subject a [`DeclaredDamageGraph`] describes.
///
/// Every kind shares the node identity discipline — stable keys, typed
/// edges, resolved values — while its *rules* stay its own record, so a
/// world object and a capital ship never silently run aircraft rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GraphSubjectKind {
    /// A player or AI aircraft.
    Aircraft,
    /// A destructible world object.
    WorldObject,
    /// A capital ship or zeppelin-class actor.
    CapitalShip,
}

impl GraphSubjectKind {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Aircraft => "aircraft",
            Self::WorldObject => "world_object",
            Self::CapitalShip => "capital_ship",
        }
    }
}

/// The declared simultaneous-lethal attribution rule (F29 AC01).
///
/// The declared vocabulary mirrors `cs_sim::damage::AttributionRule`; the
/// boundary lowers it field-wise. When the original rule is unmeasured the
/// record holds `Resolved::Unknown` — the boundary refuses to lower it, so
/// no session resolves kills under a guessed rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AttributionRule {
    /// Credit the attacker of the first lethal-depleting hit, in resolved
    /// order.
    FirstLethalHit,
    /// Credit the attacker whose hits applied the most damage to the
    /// victim during the tick; ties credit the earliest contributor.
    GreatestDamage,
}

impl AttributionRule {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::FirstLethalHit => "first_lethal_hit",
            Self::GreatestDamage => "greatest_damage",
        }
    }
}

/// The rules a graph resolves under: the declared policies, each
/// [`Resolved`] so an unmeasured rule is recorded, not guessed.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphRules {
    /// The declared attribution rule for same-tick lethal hits.
    pub lethal_attribution: Resolved<AttributionRule>,
}

/// One declared damage node: a part of the subject's damage model.
///
/// `scene_binding` names the visual [`SceneNodeId`] the part corresponds
/// to — `None` for a part with no visual node, `Some(Resolved::Unknown)`
/// for a declared binding that cannot yet be resolved. It is for the
/// presentation consumer only; damage decisions never read it.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredDamageNode {
    /// The node's stable key within the graph.
    pub key: DamageNodeKey,
    /// The node's part kind.
    pub kind: DamageNodeKind,
    /// The visual part binding, when the node has one.
    pub scene_binding: Option<Resolved<SceneNodeId>>,
    /// The integrity pool the part absorbs, or an explicit unknown.
    pub integrity: Resolved<f64>,
    /// Whether the node's destruction destroys the actor.
    pub lethal: bool,
    /// The system the node's destruction disables, if any.
    pub disables: Option<SystemKind>,
    /// The armor node absorbing armor-channel hits aimed here first.
    pub guarded_by: Option<DamageNodeKey>,
    /// Where a hit's remainder flows once this node is depleted.
    pub overflow: Option<DamageNodeKey>,
}

/// Why a [`DeclaredDamageGraph`] was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum DamageSchemaError {
    /// A graph with no nodes models nothing.
    EmptyGraph,
    /// Two nodes share one key.
    DuplicateNode {
        /// The duplicated key.
        key: DamageNodeKey,
    },
    /// A node's known integrity was NaN or infinite.
    NonFiniteIntegrity {
        /// The offending node.
        node: DamageNodeKey,
    },
    /// A node's known integrity was negative.
    NegativeIntegrity {
        /// The offending node.
        node: DamageNodeKey,
        /// The rejected value.
        value: f64,
    },
    /// A `guarded_by` edge names no node of the graph.
    UnknownGuard {
        /// The node carrying the edge.
        node: DamageNodeKey,
        /// The dangling guard key.
        guard: DamageNodeKey,
    },
    /// A `guarded_by` edge names a non-[`DamageNodeKind::ArmorZone`] node.
    GuardNotArmor {
        /// The node carrying the edge.
        node: DamageNodeKey,
        /// The guard's actual kind.
        kind: DamageNodeKind,
    },
    /// An [`DamageNodeKind::ArmorZone`] declares `guarded_by`; armor is
    /// never armor-guarded.
    ArmorGuarded {
        /// The armor node carrying the edge.
        node: DamageNodeKey,
    },
    /// A node guards itself.
    SelfGuard {
        /// The offending node.
        node: DamageNodeKey,
    },
    /// An `overflow` edge names no node of the graph.
    UnknownOverflow {
        /// The node carrying the edge.
        node: DamageNodeKey,
        /// The dangling overflow key.
        overflow: DamageNodeKey,
    },
    /// A node overflows into itself.
    SelfOverflow {
        /// The offending node.
        node: DamageNodeKey,
    },
    /// The `overflow` edges form a cycle.
    OverflowCycle {
        /// One node on the cycle.
        node: DamageNodeKey,
    },
}

impl fmt::Display for DamageSchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyGraph => write!(f, "a damage graph must contain at least one node"),
            Self::DuplicateNode { key } => {
                write!(f, "damage node key {key:?} is used more than once")
            }
            Self::NonFiniteIntegrity { node } => {
                write!(f, "node {node} has a non-finite integrity")
            }
            Self::NegativeIntegrity { node, value } => {
                write!(f, "node {node} has negative integrity {value}")
            }
            Self::UnknownGuard { node, guard } => {
                write!(
                    f,
                    "node {node} is guarded by {guard}, which is not a graph node"
                )
            }
            Self::GuardNotArmor { node, kind } => {
                write!(
                    f,
                    "node {node} is guarded by a {kind} node, not an armor zone"
                )
            }
            Self::ArmorGuarded { node } => {
                write!(f, "armor node {node} cannot itself be armor-guarded")
            }
            Self::SelfGuard { node } => write!(f, "node {node} cannot guard itself"),
            Self::UnknownOverflow { node, overflow } => {
                write!(
                    f,
                    "node {node} overflows into {overflow}, which is not a graph node"
                )
            }
            Self::SelfOverflow { node } => {
                write!(f, "node {node} cannot overflow into itself")
            }
            Self::OverflowCycle { node } => {
                write!(f, "overflow edges form a cycle through node {node}")
            }
        }
    }
}

impl std::error::Error for DamageSchemaError {}

/// The declared damage model of one catalog subject.
///
/// `subject` is the catalog id the graph describes — an `airframe` for an
/// aircraft graph, a `world` object id for a destructible prop — so the
/// graph shares the catalog's identity discipline. `provenance` records
/// where the record itself came from, exactly as
/// [`crate::animation::AnimationClip`] does.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredDamageGraph {
    subject: ContentId,
    origin: Origin,
    subject_kind: GraphSubjectKind,
    rules: GraphRules,
    nodes: Vec<DeclaredDamageNode>,
    provenance: Provenance,
}

impl DeclaredDamageGraph {
    /// Assembles and validates a declared graph.
    ///
    /// # Errors
    ///
    /// [`DamageSchemaError`] on an empty node set, a duplicate key, a
    /// corrupt known integrity, a dangling or mistyped `guarded_by` edge,
    /// an armor-guarded armor node, a self edge or an `overflow` cycle.
    pub fn try_new(
        subject: ContentId,
        origin: Origin,
        subject_kind: GraphSubjectKind,
        rules: GraphRules,
        nodes: Vec<DeclaredDamageNode>,
        provenance: Provenance,
    ) -> Result<Self, DamageSchemaError> {
        validate_nodes(&nodes)?;
        Ok(Self {
            subject,
            origin,
            subject_kind,
            rules,
            nodes,
            provenance,
        })
    }

    /// The catalog id the graph describes.
    #[must_use]
    pub fn subject(&self) -> &ContentId {
        &self.subject
    }

    /// Where the record came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// Which kind of subject the graph describes.
    #[must_use]
    pub const fn subject_kind(&self) -> GraphSubjectKind {
        self.subject_kind
    }

    /// The declared rules the graph resolves under.
    #[must_use]
    pub const fn rules(&self) -> &GraphRules {
        &self.rules
    }

    /// The declared nodes, in authored order. Identity is the key, never
    /// the position.
    #[must_use]
    pub fn nodes(&self) -> &[DeclaredDamageNode] {
        &self.nodes
    }

    /// A node by key.
    #[must_use]
    pub fn node(&self, key: &DamageNodeKey) -> Option<&DeclaredDamageNode> {
        self.nodes.iter().find(|node| &node.key == key)
    }

    /// Where the record itself came from.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// The structural validation [`DeclaredDamageGraph::try_new`] and the
/// runtime graph agree on: unique keys, finite non-negative known
/// integrities, `guarded_by` naming an existing armor zone, `overflow`
/// naming an existing node, no self edges and no overflow cycle.
fn validate_nodes(nodes: &[DeclaredDamageNode]) -> Result<(), DamageSchemaError> {
    if nodes.is_empty() {
        return Err(DamageSchemaError::EmptyGraph);
    }
    let mut by_key = BTreeMap::new();
    for node in nodes {
        if by_key.insert(&node.key, node).is_some() {
            return Err(DamageSchemaError::DuplicateNode {
                key: node.key.clone(),
            });
        }
    }

    for node in nodes {
        if let Resolved::Known(known) = &node.integrity {
            if !known.value.is_finite() {
                return Err(DamageSchemaError::NonFiniteIntegrity {
                    node: node.key.clone(),
                });
            }
            if known.value < 0.0 {
                return Err(DamageSchemaError::NegativeIntegrity {
                    node: node.key.clone(),
                    value: known.value,
                });
            }
        }
        if let Some(guard) = &node.guarded_by {
            if guard == &node.key {
                return Err(DamageSchemaError::SelfGuard {
                    node: node.key.clone(),
                });
            }
            if node.kind == DamageNodeKind::ArmorZone {
                return Err(DamageSchemaError::ArmorGuarded {
                    node: node.key.clone(),
                });
            }
            let Some(target) = by_key.get(guard) else {
                return Err(DamageSchemaError::UnknownGuard {
                    node: node.key.clone(),
                    guard: guard.clone(),
                });
            };
            if target.kind != DamageNodeKind::ArmorZone {
                return Err(DamageSchemaError::GuardNotArmor {
                    node: node.key.clone(),
                    kind: target.kind,
                });
            }
        }
        if let Some(overflow) = &node.overflow {
            if overflow == &node.key {
                return Err(DamageSchemaError::SelfOverflow {
                    node: node.key.clone(),
                });
            }
            if !by_key.contains_key(overflow) {
                return Err(DamageSchemaError::UnknownOverflow {
                    node: node.key.clone(),
                    overflow: overflow.clone(),
                });
            }
        }
    }

    for start in by_key.keys() {
        let mut seen = BTreeSet::new();
        let mut current = *start;
        loop {
            if !seen.insert(current) {
                return Err(DamageSchemaError::OverflowCycle {
                    node: current.clone(),
                });
            }
            let Some(next) = &by_key[current].overflow else {
                break;
            };
            current = next;
        }
    }
    Ok(())
}

// ----------------------------------------------------------- fixture ------

fn scene_node(key: &str) -> Resolved<SceneNodeId> {
    Resolved::Known(cs_types::content::Known::new(
        SceneNodeId::from_content_id(
            ContentId::from_source(cs_types::content::ContentKind::SceneNode, key)
                .expect("fixture scene node id is valid"),
        )
        .expect("fixture id names a scene node"),
        Provenance::designed(
            cs_types::evidence::ClaimId::new("f29a.synthetic-devastator")
                .expect("fixture claim id is valid"),
        ),
    ))
}

/// The minimal synthetic fixture in declared form: the same airframe
/// damage graph `cs_sim::damage::synthetic_airframe_graph` defines, with
/// declared provenance, scene bindings and rules.
///
/// The subject is `airframe/synthetic.devastator` and every identity lives
/// under the `synthetic` key; the record carries
/// [`Origin::SyntheticFixture`] and designed provenance — it can never be
/// mistaken for retail content and cannot stand in for it.
#[must_use]
pub fn declared_synthetic_airframe_damage() -> DeclaredDamageGraph {
    let claim = || {
        cs_types::evidence::ClaimId::new("f29a.synthetic-devastator")
            .expect("fixture claim id is valid")
    };
    let designed_integrity = |value: f64| {
        Resolved::Known(cs_types::content::Known::new(
            value,
            Provenance::designed(claim()),
        ))
    };
    let key = |name: &str| DamageNodeKey::new(name).expect("fixture node keys are valid");
    let scene = |name: &str| scene_node(&format!("synthetic.devastator.{name}"));

    DeclaredDamageGraph::try_new(
        ContentId::from_source(
            cs_types::content::ContentKind::Airframe,
            "synthetic.devastator",
        )
        .expect("fixture subject id is valid"),
        Origin::SyntheticFixture,
        GraphSubjectKind::Aircraft,
        GraphRules {
            lethal_attribution: Resolved::Known(cs_types::content::Known::new(
                AttributionRule::FirstLethalHit,
                Provenance::designed(claim()),
            )),
        },
        vec![
            DeclaredDamageNode {
                key: key("nose_armor"),
                kind: DamageNodeKind::ArmorZone,
                scene_binding: Some(scene("nose_armor")),
                integrity: designed_integrity(20.0),
                lethal: false,
                disables: None,
                guarded_by: None,
                overflow: Some(key("hull")),
            },
            DeclaredDamageNode {
                key: key("hull"),
                kind: DamageNodeKind::InternalStructure,
                scene_binding: Some(scene("hull")),
                integrity: designed_integrity(40.0),
                lethal: true,
                disables: None,
                guarded_by: Some(key("nose_armor")),
                overflow: None,
            },
            DeclaredDamageNode {
                key: key("engine_1"),
                kind: DamageNodeKind::Engine,
                scene_binding: Some(scene("engine_1")),
                integrity: designed_integrity(15.0),
                lethal: false,
                disables: Some(SystemKind::Propulsion),
                guarded_by: None,
                overflow: None,
            },
            DeclaredDamageNode {
                key: key("gun_mount_1"),
                kind: DamageNodeKind::WeaponMount,
                scene_binding: Some(scene("gun_mount_1")),
                integrity: designed_integrity(10.0),
                lethal: false,
                disables: Some(SystemKind::Weapon),
                guarded_by: None,
                overflow: None,
            },
        ],
        Provenance::designed(claim()),
    )
    .expect("the declared synthetic airframe damage fixture is valid")
}
