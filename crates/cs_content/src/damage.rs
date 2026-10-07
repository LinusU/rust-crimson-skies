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
//!
//! # The measured airframe damage vocabulary (F29-D.1)
//!
//! One thing in this module is **measured** rather than designed:
//! [`AirframeDamageVocabulary`], read from the installation's
//! `ZBD/planes.zbd` by [`observe_airframe_damage_vocabulary`] through
//! `cs_formats`' production readers, records the four damage-region node
//! names the container stores (once per airframe group, eleven groups) and
//! the eleven `<prefix>_damage` materials its airframe subtrees bind, each
//! with the stored record it was read from and an `observed_tool`
//! provenance.
//!
//! It is vocabulary and shape only. Every one of those nodes stores
//! `zone_id` `255`, so nothing measured assigns a zone; that a named node
//! *is* a damage zone, its topology, its parent's role and how a hit routes
//! to one are all still unknown, as is every integrity, armor and multiplier
//! value. [`DeclaredDamageGraph::declare_airframe_regions`] is the declared
//! lowering of that measurement — four region part slots and at most one
//! wreck presentation slot per airframe, with no number invented and no
//! name promoted to a role — and it refuses a graph whose declared count
//! does not match the measurement, by name.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use cs_assets::install;
use cs_formats::ParseContext;
use cs_formats::gamez::{
    GameZError, GameZMaterialError, GameZMaterials, GameZMeshes, GameZNodeError, GameZNodes,
    NODE_SLOT_BYTES, TEXTURE_INFO_BYTES, read_gamez_materials, read_gamez_meshes, read_gamez_nodes,
};
use cs_types::asset_id::{SourceSpan, SourceSpanError};
use cs_types::content::{ContentId, ContentKind, Origin, Provenance, ProvenanceError, Resolved};
use cs_types::evidence::{ClaimId, ClaimIdError, ClaimStatus, ContentHash};

use crate::catalog::baseline::install_file_key;
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
    /// An airframe graph declared a different number of region part slots
    /// than its measured vocabulary holds.
    RegionCountMismatch {
        /// The graph's subject, named so the refusal identifies the airframe.
        subject: ContentId,
        /// How many distinct region slots the graph declared.
        declared: usize,
        /// How many distinct damage-region names the measurement holds.
        observed: usize,
    },
    /// An airframe graph declared more wreck presentation slots than the
    /// measurement allows for one airframe.
    WreckSlotOverflow {
        /// The graph's subject, named so the refusal identifies the airframe.
        subject: ContentId,
        /// How many distinct wreck slots the graph declared.
        declared: usize,
        /// The largest number of distinct wreck materials one measured
        /// airframe binds.
        observed: usize,
    },
    /// A declared region or wreck slot names the same key twice, so the slot
    /// count would not be the number of distinct slots it claims.
    DuplicateShapeSlot {
        /// The graph's subject.
        subject: ContentId,
        /// The repeated key.
        key: DamageNodeKey,
    },
    /// The airframe region shape was offered to a graph whose subject is not
    /// an aircraft, so the shape would describe something the measurement
    /// never covered.
    RegionShapeOnNonAircraft {
        /// The graph's subject.
        subject: ContentId,
        /// What the graph actually describes.
        kind: GraphSubjectKind,
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
            Self::RegionCountMismatch {
                subject,
                declared,
                observed,
            } => write!(
                f,
                "airframe {subject} declares {declared} damage-region slots, but its measured \
                 vocabulary holds {observed}"
            ),
            Self::WreckSlotOverflow {
                subject,
                declared,
                observed,
            } => write!(
                f,
                "airframe {subject} declares {declared} wreck presentation slots, but one \
                 measured airframe binds at most {observed}"
            ),
            Self::DuplicateShapeSlot { subject, key } => write!(
                f,
                "airframe {subject} declares the shape slot {key} more than once"
            ),
            Self::RegionShapeOnNonAircraft { subject, kind } => write!(
                f,
                "the airframe region shape cannot be declared for {subject}, which describes a \
                 {}",
                kind.label()
            ),
        }
    }
}

impl std::error::Error for DamageSchemaError {}

/// The declared airframe damage-region shape of one graph
/// (`specs/F29-damage-zones-armor-destruction-and-bailout.md`, task F29-D.1).
///
/// The shape is the **lowering** of a measured [`AirframeDamageVocabulary`]
/// into the F29 damage graph, and it is shape only: four region part slots
/// and at most one wreck presentation slot per airframe, with no kind, no
/// integrity, no edge and no routing attached to either. What the original
/// does with a name is unknown, so nothing here decides it.
///
/// A slot is a [`DamageNodeKey`] rather than a bare count so a later binding
/// is by identity and never by array position (`IDENTITY-CONTENT`); the keys
/// are the graph's own declarations, and the *number* of them is what the
/// measured vocabulary constrains.
#[derive(Clone, Debug, PartialEq)]
pub struct AirframeRegionShape {
    /// The graph's region part slots, in declared order.
    regions: Vec<DamageNodeKey>,
    /// The graph's wreck presentation slots: at most one per airframe.
    wreck: Vec<DamageNodeKey>,
    /// The provenance of the measurement the shape was validated against.
    provenance: Provenance,
}

impl AirframeRegionShape {
    /// The declared region part slots.
    #[must_use]
    pub fn regions(&self) -> &[DamageNodeKey] {
        &self.regions
    }

    /// The declared wreck presentation slots: at most one.
    #[must_use]
    pub fn wreck_slots(&self) -> &[DamageNodeKey] {
        &self.wreck
    }

    /// The provenance of the measurement this shape was validated against.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

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
    airframe_regions: Option<AirframeRegionShape>,
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
            airframe_regions: None,
        })
    }

    /// Declares the airframe damage-region shape of this graph against a
    /// measured [`AirframeDamageVocabulary`], returning the graph that holds
    /// it.
    ///
    /// This is the task F29-D.1 lowering: the observation says how many
    /// region part slots an airframe declares and how many wreck
    /// presentation slots one airframe can bind, and this method accepts a
    /// declaration only when the graph's own slots agree with those
    /// measured numbers. The counts come from `observation`, never from a
    /// constant, so a vocabulary measuring a different number changes what
    /// is accepted.
    ///
    /// The slots themselves are the caller's declarations — keys, so the
    /// binding that follows is by identity — and no kind, integrity or edge
    /// is derived from a measured *name*: a name is not a role.
    ///
    /// # Errors
    ///
    /// [`DamageSchemaError::RegionShapeOnNonAircraft`] when the graph does
    /// not describe an aircraft,
    /// [`DamageSchemaError::RegionCountMismatch`] when the graph declares a
    /// different number of distinct region slots than the vocabulary
    /// measures, [`DamageSchemaError::WreckSlotOverflow`] when it declares
    /// more wreck slots than one measured airframe binds,
    /// [`DamageSchemaError::DuplicateShapeSlot`] when a slot key is repeated
    /// (so the declared count is never inflated by a duplicate).
    pub fn declare_airframe_regions(
        &self,
        regions: Vec<DamageNodeKey>,
        wreck: Vec<DamageNodeKey>,
        observation: &AirframeDamageVocabulary,
    ) -> Result<Self, DamageSchemaError> {
        if self.subject_kind != GraphSubjectKind::Aircraft {
            return Err(DamageSchemaError::RegionShapeOnNonAircraft {
                subject: self.subject.clone(),
                kind: self.subject_kind,
            });
        }
        let mut seen = BTreeSet::new();
        for key in regions.iter().chain(&wreck) {
            if !seen.insert(key) {
                return Err(DamageSchemaError::DuplicateShapeSlot {
                    subject: self.subject.clone(),
                    key: key.clone(),
                });
            }
        }
        let declared = regions.len();
        let observed = observation.region_count();
        if declared != observed {
            return Err(DamageSchemaError::RegionCountMismatch {
                subject: self.subject.clone(),
                declared,
                observed,
            });
        }
        let declared_wreck = wreck.len();
        let observed_wreck = observation.max_wreck_materials_per_airframe();
        if declared_wreck > observed_wreck {
            return Err(DamageSchemaError::WreckSlotOverflow {
                subject: self.subject.clone(),
                declared: declared_wreck,
                observed: observed_wreck,
            });
        }
        let mut graph = self.clone();
        graph.airframe_regions = Some(AirframeRegionShape {
            regions,
            wreck,
            provenance: observation.provenance().clone(),
        });
        Ok(graph)
    }

    /// The declared airframe damage-region shape, once
    /// [`Self::declare_airframe_regions`] accepted one.
    #[must_use]
    pub const fn airframe_regions(&self) -> Option<&AirframeRegionShape> {
        self.airframe_regions.as_ref()
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

// ------------------------------------------------- measured vocabulary ----

/// The installation-relative spelling of the container the airframe damage
/// vocabulary is measured in.
///
/// Its catalog key is `install_file/zbd_2f_planes.zbd` — the
/// [`install_file_key`] of this spelling, the same container F11-D2's airframe
/// roster discovery reads.
pub const AIRFRAME_DAMAGE_CONTAINER: &str = "ZBD/planes.zbd";

/// The suffix a stored **node name** must carry to be selected as one of the
/// container's damage regions.
///
/// This is the observation's disclosed selection rule, not a claim about the
/// original: `ZBD/planes.zbd` stores seven distinct node names containing
/// `damage`, and the four that *end* with it are the four measured region
/// names. The three that do not are kept in
/// [`AirframeDamageVocabulary::discarded`] rather than silently dropped, so
/// the rule's boundary is part of the record and can be checked against the
/// container.
const REGION_SUFFIX: &str = "damage";

/// The suffix a **texture stem** must carry to be selected as a wreck
/// material — the measured `<prefix>_damage` names.
const WRECK_STEM_SUFFIX: &str = "_damage";

/// The marker every name of interest carries, selected or not.
const DAMAGE_MARKER: &str = "damage";

/// Case-insensitive ASCII `ends_with`, byte-wise so a name holding a
/// non-ASCII byte can never panic the selection rule.
fn ends_with_ascii_folded(name: &str, suffix: &str) -> bool {
    name.len() >= suffix.len()
        && name.as_bytes()[name.len() - suffix.len()..].eq_ignore_ascii_case(suffix.as_bytes())
}

/// Case-insensitive ASCII containment, for the same reason.
fn contains_ascii_folded(name: &str, needle: &str) -> bool {
    name.len() >= needle.len()
        && name
            .as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

/// One of the two name tables the observation reads its names from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NameTable {
    /// The container's node array, read by [`read_gamez_nodes`].
    Nodes,
    /// The container's texture-name table, which a material record reaches
    /// through, read by [`read_gamez_materials`].
    Textures,
}

impl NameTable {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Nodes => "nodes",
            Self::Textures => "textures",
        }
    }
}

impl fmt::Display for NameTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Why a measured airframe damage vocabulary could not be assembled.
///
/// Every variant is a fact about the *measurement*, never a repair: the
/// reader reports what it found instead of reshaping it into the shape the
/// record expected.
#[derive(Clone, Debug, PartialEq)]
pub enum AirframeDamageVocabularyError {
    /// Nothing of the container was selected, so there is no vocabulary to
    /// record (an empty record would read as "no damage regions", which is a
    /// different claim).
    NoRegions,
    /// A selected region node has no parent, so it belongs to no airframe
    /// group and the per-airframe reading cannot be supported.
    RegionWithoutParent {
        /// The stored name of that node.
        name: String,
        /// Its slot in the info array.
        node_index: u32,
    },
    /// A parent slot names a node outside the array, so the record the
    /// measurement wanted to anchor on does not exist.
    MissingParent {
        /// The node whose parent slot is out of range.
        node_index: u32,
        /// The parent slot it stores.
        parent: u32,
    },
    /// The ancestry walk revisited a node instead of reaching a root.
    AncestryCycle {
        /// The node the walk reached twice.
        node_index: u32,
    },
    /// A group holds no region record, so it would record an airframe with no
    /// measured regions.
    GroupWithoutRegions {
        /// The group's parent slot.
        parent: u32,
    },
    /// A group has no ancestry, so the node its regions hang under is unnamed.
    GroupWithoutAncestry {
        /// The first region of the group, which names the group instead.
        first_region: String,
    },
    /// A span could not be built for a record the measurement located.
    Span(SourceSpanError),
    /// The record's claim id or provenance was rejected.
    Provenance {
        /// The rejected claim id, when the claim id itself was the problem.
        claim: Option<ClaimIdError>,
        /// The rejected provenance, when the provenance wrapper was.
        provenance: Option<ProvenanceError>,
    },
    /// The container's catalog id was refused by the id grammar.
    ContainerId(String),
}

impl fmt::Display for AirframeDamageVocabularyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRegions => {
                write!(f, "no damage-region node was selected: nothing to record")
            }
            Self::RegionWithoutParent { name, node_index } => write!(
                f,
                "node {node_index} ({name:?}) is a damage region with no parent, so it belongs to \
                 no airframe group"
            ),
            Self::MissingParent { node_index, parent } => write!(
                f,
                "node {node_index} stores parent slot {parent}, which is outside the node array"
            ),
            Self::AncestryCycle { node_index } => {
                write!(f, "the ancestry of node {node_index} loops at {node_index}")
            }
            Self::GroupWithoutRegions { parent } => {
                write!(f, "the group under node {parent} holds no region record")
            }
            Self::GroupWithoutAncestry { first_region } => write!(
                f,
                "the group holding {first_region:?} has no ancestry, so its parent is unnamed"
            ),
            Self::Span(error) => write!(f, "a source span was refused: {error}"),
            Self::ContainerId(reason) => {
                write!(f, "the container's catalog id was refused: {reason}")
            }
            Self::Provenance { claim, provenance } => match (claim, provenance) {
                (Some(claim), _) => write!(f, "the claim id was refused: {claim}"),
                (_, Some(provenance)) => write!(f, "the provenance was refused: {provenance}"),
                (None, None) => write!(f, "the record's provenance was refused"),
            },
        }
    }
}

impl std::error::Error for AirframeDamageVocabularyError {}

/// Why the airframe damage vocabulary could not be read from an
/// installation.
#[derive(Clone, Debug, PartialEq)]
pub enum AirframeDamageObservationError {
    /// Production discovery could not read the installation.
    Discovery(String),
    /// The installation inventory holds no row for the container, so its
    /// bytes are not part of the fingerprinted installation.
    ContainerNotInventoried {
        /// The installation-relative spelling that is missing.
        spelling: &'static str,
    },
    /// The container's bytes could not be read.
    Read {
        /// The spelling that was read.
        spelling: &'static str,
        /// The operating system's reason.
        reason: String,
    },
    /// The bytes read do not hash to the inventoried file's own digest, so
    /// they are not the installation's file (the file changed underneath, or
    /// the path resolved to something else).
    DigestMismatch {
        /// The digest the inventory recorded.
        expected: ContentHash,
        /// The digest of the bytes actually read.
        found: ContentHash,
    },
    /// The node array reader refused the container.
    Nodes(GameZNodeError),
    /// The material section reader refused the container.
    Materials(GameZMaterialError),
    /// The mesh section reader refused the container.
    Meshes(GameZError),
    /// The measurement does not assemble into a record.
    Vocabulary(AirframeDamageVocabularyError),
    /// The container's catalog id was refused by the id grammar, so the
    /// record could not name what it measured.
    ContainerId(String),
}

impl fmt::Display for AirframeDamageObservationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(reason) => write!(f, "installation discovery failed: {reason}"),
            Self::ContainerNotInventoried { spelling } => {
                write!(f, "the inventory holds no row for {spelling}")
            }
            Self::Read { spelling, reason } => {
                write!(f, "cannot read {spelling}: {reason}")
            }
            Self::DigestMismatch { expected, found } => write!(
                f,
                "the bytes read hash to {found}, but the inventory recorded {expected} for this \
                 file"
            ),
            Self::Nodes(error) => write!(f, "the node array reader refused the container: {error}"),
            Self::Materials(error) => {
                write!(
                    f,
                    "the material section reader refused the container: {error}"
                )
            }
            Self::Meshes(error) => {
                write!(f, "the mesh section reader refused the container: {error}")
            }
            Self::Vocabulary(error) => write!(f, "the measurement does not record: {error}"),
            Self::ContainerId(reason) => {
                write!(f, "the container's catalog id was refused: {reason}")
            }
        }
    }
}

impl std::error::Error for AirframeDamageObservationError {}

/// Where one airframe damage vocabulary's bytes came from, and the
/// [`Origin`]/[`Provenance`] the record inherits from that.
///
/// The identity and the provenance are built together so they cannot
/// disagree: an installation-backed vocabulary always carries
/// [`Origin::Installation`] over the whole container with an
/// [`ClaimStatus::ObservedTool`] provenance naming the same span, and a
/// fixture vocabulary always carries [`Origin::SyntheticFixture`] with
/// designed provenance, so it can never be mistaken for retail measurement
/// (the rule `docs/contracts/IDENTITY-CONTENT.md` states for every record).
#[derive(Clone, Debug, PartialEq)]
pub struct AirframeContainerSource {
    container: ContentId,
    container_path: String,
    install_sha256: ContentHash,
    container_sha256: ContentHash,
    container_len: u64,
    origin: Origin,
    provenance: Provenance,
}

impl AirframeContainerSource {
    /// The installation-backed source of `container_path`'s bytes.
    ///
    /// # Errors
    ///
    /// [`AirframeDamageVocabularyError::Span`] or
    /// [`AirframeDamageVocabularyError::Provenance`] when the whole-container
    /// span or its provenance is refused.
    pub fn installation(
        container: ContentId,
        container_path: &str,
        install_sha256: ContentHash,
        container_sha256: ContentHash,
        container_len: u64,
    ) -> Result<Self, AirframeDamageVocabularyError> {
        let span = SourceSpan::new(
            install_sha256,
            container_path,
            None,
            0,
            container_len,
            Some(container_sha256),
        )
        .map_err(AirframeDamageVocabularyError::Span)?;
        let provenance = Provenance::new(
            Self::claim_id()?,
            ClaimStatus::ObservedTool,
            Some(span.clone()),
        )
        .map_err(|provenance| AirframeDamageVocabularyError::Provenance {
            claim: None,
            provenance: Some(provenance),
        })?;
        Ok(Self {
            container,
            container_path: container_path.to_owned(),
            install_sha256,
            container_sha256,
            container_len,
            origin: Origin::Installation { source: span },
            provenance,
        })
    }

    /// The authored fixture source: `Origin::SyntheticFixture` and designed
    /// provenance over a deliberately fabricated digest, so nothing built
    /// from it can stand in for installation data.
    ///
    /// # Errors
    ///
    /// [`AirframeDamageVocabularyError::Provenance`] when the claim id is
    /// refused.
    pub fn synthetic() -> Result<Self, AirframeDamageVocabularyError> {
        let fabricated = ContentHash::from_bytes([0x5a; 32]);
        let container =
            ContentId::from_source(ContentKind::InstallFile, "synthetic.airframe-damage.zbd")
                .map_err(|error| AirframeDamageVocabularyError::ContainerId(error.to_string()))?;
        let provenance =
            Provenance::designed(ClaimId::new("f29d1.synthetic-airframe-vocabulary").map_err(
                |claim| AirframeDamageVocabularyError::Provenance {
                    claim: Some(claim),
                    provenance: None,
                },
            )?);
        Ok(Self {
            container,
            container_path: "synthetic/airframe-damage.zbd".to_owned(),
            install_sha256: fabricated,
            container_sha256: fabricated,
            container_len: 0,
            origin: Origin::SyntheticFixture,
            provenance,
        })
    }

    /// The one claim id both constructors record.
    fn claim_id() -> Result<ClaimId, AirframeDamageVocabularyError> {
        ClaimId::new("f29d1.airframe-damage-vocabulary").map_err(|claim| {
            AirframeDamageVocabularyError::Provenance {
                claim: Some(claim),
                provenance: None,
            }
        })
    }

    /// A span into this source's own container, for a record the measurement
    /// located there.
    ///
    /// # Errors
    ///
    /// [`AirframeDamageVocabularyError::Span`] when the range is refused.
    pub fn span(
        &self,
        offset: u64,
        length: u64,
    ) -> Result<SourceSpan, AirframeDamageVocabularyError> {
        SourceSpan::new(
            self.install_sha256,
            &self.container_path,
            None,
            offset,
            length,
            Some(self.container_sha256),
        )
        .map_err(AirframeDamageVocabularyError::Span)
    }

    /// The container's catalog id.
    #[must_use]
    pub fn container(&self) -> &ContentId {
        &self.container
    }

    /// The installation digest the record is bound to.
    #[must_use]
    pub const fn install_sha256(&self) -> ContentHash {
        self.install_sha256
    }

    /// The SHA-256 of the container's own bytes.
    #[must_use]
    pub const fn container_sha256(&self) -> ContentHash {
        self.container_sha256
    }

    /// The container's length in bytes.
    #[must_use]
    pub const fn container_len(&self) -> u64 {
        self.container_len
    }

    /// Where the record came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// Where the record itself came from.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// One stored name that carries the damage marker and the selection rule did
/// not take, with how often the container stores it.
///
/// Keeping these is what makes the selection rule auditable: the record shows
/// every damage-flavoured name the container holds, not only the ones it
/// classified as regions or wreck materials.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscardedDamageName {
    /// The stored name, exactly as the table spells it.
    pub name: String,
    /// How many entries of that table store it.
    pub occurrences: usize,
    /// Which table it was read from.
    pub table: NameTable,
}

/// One node record selected as an airframe damage region, with the stored
/// record its name was read from.
///
/// `zone_id` is carried raw and uninterpreted (spec F10 non-negotiable #5):
/// the field's domain beyond the default is unmeasured, so nothing here reads
/// it as a zone assignment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeasuredDamageRegion {
    /// The authored node name, exactly as the container stores it.
    pub name: String,
    /// The node's slot in the container's info array.
    pub node_index: u32,
    /// The parent slot the group this region belongs to hangs under.
    pub parent: u32,
    /// The stored `zone_id` word, uninterpreted.
    pub zone_id: u32,
    /// The stored record the name was read from: the whole info-array slot
    /// (`NODE_SLOT_BYTES`), because `cs_formats` publishes the record sizes
    /// and table starts but not the name field's inner offset. The
    /// acceptance test reads the name back out of this span, so a layout or
    /// arithmetic drift fails loudly instead of pointing at the wrong byte.
    pub span: SourceSpan,
}

/// One node of a group's ancestry: its slot and stored name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AncestryNode {
    /// The node's slot in the info array.
    pub index: u32,
    /// The authored name, exactly as stored.
    pub name: String,
}

/// One airframe's measured damage-region group: the sibling nodes that share
/// one parent, each of the container's damage regions appearing once.
///
/// The grouping is structural, never nominal: a group is "the nodes sharing a
/// parent slot", and the ancestry records — as stored names — how that parent
/// reaches the top of the hierarchy. Nothing here decides which of those
/// ancestors *is* the airframe; that reading is checked by the acceptance
/// test against F11-D2's independently measured eleven-airframe roster.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AirframeRegionGroup {
    /// The shared parent first, then its ancestors up to the topmost one.
    pub ancestry: Vec<AncestryNode>,
    /// The regions sharing that parent, in stored order.
    pub regions: Vec<MeasuredDamageRegion>,
}

impl AirframeRegionGroup {
    /// Assembles and validates one group.
    ///
    /// # Errors
    ///
    /// [`AirframeDamageVocabularyError::GroupWithoutAncestry`] when no parent
    /// names the group, or [`AirframeDamageVocabularyError::GroupWithoutRegions`]
    /// when the group measures no region.
    pub fn try_new(
        ancestry: Vec<AncestryNode>,
        regions: Vec<MeasuredDamageRegion>,
    ) -> Result<Self, AirframeDamageVocabularyError> {
        let first_region = regions
            .first()
            .map(|region| region.name.clone())
            .ok_or_else(|| AirframeDamageVocabularyError::GroupWithoutRegions {
                parent: ancestry.first().map_or(u32::MAX, |node| node.index),
            })?;
        if ancestry.is_empty() {
            return Err(AirframeDamageVocabularyError::GroupWithoutAncestry { first_region });
        }
        Ok(Self { ancestry, regions })
    }

    /// The group's parent slot (the node every region of the group hangs on).
    #[must_use]
    pub fn parent(&self) -> u32 {
        self.ancestry[0].index
    }

    /// The distinct region names of this group, in stored order.
    #[must_use]
    pub fn region_names(&self) -> Vec<&str> {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut names = Vec::new();
        for region in &self.regions {
            if seen.insert(region.name.as_str()) {
                names.push(region.name.as_str());
            }
        }
        names
    }
}

/// Where one `<prefix>_damage` material is bound: a mesh inside one airframe
/// group's subtree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WreckBinding {
    /// The parent slot of the group whose subtree binds the material.
    pub group_parent: u32,
    /// The node whose mesh binds it.
    pub node_index: u32,
    /// The mesh slot that node stores.
    pub mesh_index: i32,
}

/// One container material whose texture stem carries the measured
/// `<prefix>_damage` suffix.
///
/// The material *record* stores no name: it stores an index into the
/// container's texture-name table, and [`GameZMaterials::texture_of`] is the
/// lookup this record was built from. `material_indices` are the present
/// material records that name the texture (measured: exactly one each);
/// `bindings` are the airframe subtrees whose meshes reference it (measured:
/// exactly one each, so the container's eleven materials land one per
/// airframe group).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeasuredWreckMaterial {
    /// The texture stem as stored (`whawk_damage`).
    pub stem: String,
    /// The decoded texture name (`whawk_damage.tif`).
    pub texture_name: String,
    /// The entry's slot in the texture-name table.
    pub texture_index: u32,
    /// The present material records naming that texture, in stored order.
    pub material_indices: Vec<u32>,
    /// The airframe subtrees binding it, in group order.
    pub bindings: Vec<WreckBinding>,
    /// The stored record the texture name was read from: the whole 44-byte
    /// texture-name record, for the same reason as
    /// [`MeasuredDamageRegion::span`] — the inner offset is not published by
    /// `cs_formats`, and the acceptance test reads the stem back out of this
    /// span against the container's bytes.
    pub span: SourceSpan,
}

/// The measured airframe damage vocabulary of one container
/// (`specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
/// `### F29-D`, task F29-D.1).
///
/// This is the **measured** half of `cs_content::damage`: every name in it
/// was read out of `ZBD/planes.zbd` by [`observe_airframe_damage_vocabulary`]
/// using `cs_formats`' production readers, and every one carries the span it
/// was read from under a [`ClaimStatus::ObservedTool`] provenance.
///
/// What the record states is *names and shape only*:
///
/// * the container stores four distinct damage-region node names, each once
///   per airframe group, and the groups are the container's eleven
///   airframe-sized subtrees;
/// * the container stores eleven distinct `<prefix>_damage` materials, each
///   named by one present material record and bound by exactly one airframe
///   group's meshes;
/// * every one of those nodes stores `zone_id` `255`, the stored default, so
///   nothing measured here assigns a zone id to any of them.
///
/// What it does **not** state, because nothing measured supports it: that a
/// named node *is* a damage zone, its topology or its parent's role, how a
/// hit routes to it, or any integrity, armor or multiplier value. Those stay
/// unknown (F29 "Research boundary"); this record is vocabulary and counts.
#[derive(Clone, Debug, PartialEq)]
pub struct AirframeDamageVocabulary {
    source: AirframeContainerSource,
    groups: Vec<AirframeRegionGroup>,
    wreck_materials: Vec<MeasuredWreckMaterial>,
    discarded: Vec<DiscardedDamageName>,
}

impl AirframeDamageVocabulary {
    /// Assembles and validates the measured record.
    ///
    /// # Errors
    ///
    /// [`AirframeDamageVocabularyError::NoRegions`] when nothing was
    /// selected; the group variants surface from
    /// [`AirframeRegionGroup::try_new`] for the groups handed in.
    pub fn try_new(
        source: AirframeContainerSource,
        groups: Vec<AirframeRegionGroup>,
        wreck_materials: Vec<MeasuredWreckMaterial>,
        discarded: Vec<DiscardedDamageName>,
    ) -> Result<Self, AirframeDamageVocabularyError> {
        if groups.is_empty() {
            return Err(AirframeDamageVocabularyError::NoRegions);
        }
        Ok(Self {
            source,
            groups,
            wreck_materials,
            discarded,
        })
    }

    /// Where the measured bytes came from.
    #[must_use]
    pub const fn source(&self) -> &AirframeContainerSource {
        &self.source
    }

    /// The container's catalog id (`install_file/zbd_2f_planes.zbd`).
    #[must_use]
    pub fn container(&self) -> &ContentId {
        self.source.container()
    }

    /// The SHA-256 of the container's own bytes.
    #[must_use]
    pub const fn container_sha256(&self) -> ContentHash {
        self.source.container_sha256()
    }

    /// The installation digest the measurement is bound to.
    #[must_use]
    pub const fn install_sha256(&self) -> ContentHash {
        self.source.install_sha256()
    }

    /// Where the record came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        self.source.origin()
    }

    /// Where the record itself came from.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        self.source.provenance()
    }

    /// Every measured airframe region group, in stored order.
    #[must_use]
    pub fn groups(&self) -> &[AirframeRegionGroup] {
        &self.groups
    }

    /// Every measured `<prefix>_damage` material, in texture-table order.
    #[must_use]
    pub fn wreck_materials(&self) -> &[MeasuredWreckMaterial] {
        &self.wreck_materials
    }

    /// Every damage-flavoured name the selection rule did not take, ordered
    /// by table then name.
    #[must_use]
    pub fn discarded(&self) -> &[DiscardedDamageName] {
        &self.discarded
    }

    /// The distinct damage-region names the container stores, sorted — the
    /// measured region vocabulary itself.
    #[must_use]
    pub fn region_names(&self) -> Vec<String> {
        let mut names: BTreeSet<String> = BTreeSet::new();
        for group in &self.groups {
            for name in group.region_names() {
                names.insert(name.to_owned());
            }
        }
        names.into_iter().collect()
    }

    /// How many distinct damage-region names the container stores.
    ///
    /// This is the number the declared graph shape is checked against; it is
    /// *read from the measurement*, never a constant in the check.
    #[must_use]
    pub fn region_count(&self) -> usize {
        self.groups
            .iter()
            .flat_map(|group| group.region_names())
            .collect::<BTreeSet<&str>>()
            .len()
    }

    /// The distinct `<prefix>_damage` stems the container stores, sorted.
    #[must_use]
    pub fn wreck_material_stems(&self) -> Vec<String> {
        let mut stems: BTreeSet<String> = BTreeSet::new();
        for material in &self.wreck_materials {
            stems.insert(material.stem.clone());
        }
        stems.into_iter().collect()
    }

    /// The largest number of distinct wreck materials one airframe group
    /// binds — the measured ceiling the declared shape's "at most one wreck
    /// presentation slot per airframe" is read from.
    #[must_use]
    pub fn max_wreck_materials_per_airframe(&self) -> usize {
        let mut per_group: BTreeMap<u32, BTreeSet<&str>> = BTreeMap::new();
        for material in &self.wreck_materials {
            for binding in &material.bindings {
                per_group
                    .entry(binding.group_parent)
                    .or_default()
                    .insert(material.stem.as_str());
            }
        }
        per_group.values().map(BTreeSet::len).max().unwrap_or(0)
    }
}

/// Reads the measured airframe damage vocabulary out of an installation's
/// `ZBD/planes.zbd`, with production readers only.
///
/// The byte source is checked against the installation inventory: the
/// container's row supplies the digest, and the bytes read must hash to it,
/// so the vocabulary can never come from a file that is not the
/// fingerprinted installation's. The three sections are then read by
/// `cs_formats`' own readers — [`read_gamez_nodes`], [`read_gamez_materials`]
/// and [`read_gamez_meshes`] — each proving its own boundary, and the names
/// are collected from what they decoded. Nothing is scanned with a side
/// channel, and no name is looked up from a table this module carries.
///
/// # Errors
///
/// [`AirframeDamageObservationError`] naming the stage that failed: discovery,
/// the inventory row, the byte read, the digest check, one of the three
/// readers, or the measurement itself.
pub fn observe_airframe_damage_vocabulary(
    root: &Path,
) -> Result<AirframeDamageVocabulary, AirframeDamageObservationError> {
    let found = install::discover(root)
        .map_err(|error| AirframeDamageObservationError::Discovery(error.to_string()))?;
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| {
            record
                .relative_spelling
                .as_str()
                .eq_ignore_ascii_case(AIRFRAME_DAMAGE_CONTAINER)
        })
        .ok_or(AirframeDamageObservationError::ContainerNotInventoried {
            spelling: AIRFRAME_DAMAGE_CONTAINER,
        })?;
    let container_sha256 = record.sha256;
    let inventoried_len = record.size_bytes;
    let bytes = std::fs::read(root.join(AIRFRAME_DAMAGE_CONTAINER)).map_err(|error| {
        AirframeDamageObservationError::Read {
            spelling: AIRFRAME_DAMAGE_CONTAINER,
            reason: error.to_string(),
        }
    })?;
    let found_sha256 = install::sha256(&bytes);
    if found_sha256 != container_sha256 || bytes.len() as u64 != inventoried_len {
        return Err(AirframeDamageObservationError::DigestMismatch {
            expected: container_sha256,
            found: found_sha256,
        });
    }

    let container = ContentId::from_source(
        ContentKind::InstallFile,
        &install_file_key(AIRFRAME_DAMAGE_CONTAINER),
    )
    .map_err(|error| AirframeDamageObservationError::ContainerId(error.to_string()))?;
    let source = AirframeContainerSource::installation(
        container,
        AIRFRAME_DAMAGE_CONTAINER,
        install::fingerprint(&found.manifest),
        container_sha256,
        bytes.len() as u64,
    )
    .map_err(AirframeDamageObservationError::Vocabulary)?;

    // One parse context for all three sections, as the readers are meant to
    // be used: each proves its own boundary on the same bytes, and a failed
    // attempt leaves the ledger untouched.
    let label = AIRFRAME_DAMAGE_CONTAINER.to_owned();
    let mut parse = ParseContext::with_defaults(label.clone());
    let nodes =
        read_gamez_nodes(&mut parse, &bytes).map_err(AirframeDamageObservationError::Nodes)?;
    let materials = read_gamez_materials(&mut parse, &label, &bytes)
        .map_err(AirframeDamageObservationError::Materials)?;
    let meshes = read_gamez_meshes(&mut parse, &label, &bytes)
        .map_err(AirframeDamageObservationError::Meshes)?;

    measure_vocabulary(&source, &nodes, &materials, &meshes)
        .map_err(AirframeDamageObservationError::Vocabulary)
}

/// The measurement itself, separated from byte access so every input is what
/// a production reader already validated.
fn measure_vocabulary(
    source: &AirframeContainerSource,
    nodes: &GameZNodes,
    materials: &GameZMaterials,
    meshes: &GameZMeshes,
) -> Result<AirframeDamageVocabulary, AirframeDamageVocabularyError> {
    // The groups: selected region nodes, bucketed by the parent slot they
    // store. Bucketing runs in stored order, so groups appear in the order
    // the container stores them.
    let mut buckets: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for node in &nodes.nodes {
        if !ends_with_ascii_folded(&node.name, REGION_SUFFIX) {
            continue;
        }
        let Some(parent) = node.parent else {
            return Err(AirframeDamageVocabularyError::RegionWithoutParent {
                name: node.name.clone(),
                node_index: node.index,
            });
        };
        if nodes.get(parent).is_none() {
            return Err(AirframeDamageVocabularyError::MissingParent {
                node_index: node.index,
                parent,
            });
        }
        buckets.entry(parent).or_default().push(node.index);
    }

    let mut groups = Vec::with_capacity(buckets.len());
    for (parent, region_indices) in buckets {
        let ancestry = ancestry_of(nodes, parent)?;
        let mut regions = Vec::with_capacity(region_indices.len());
        for index in region_indices {
            let node = nodes
                .get(index)
                .ok_or(AirframeDamageVocabularyError::MissingParent {
                    node_index: index,
                    parent,
                })?;
            regions.push(MeasuredDamageRegion {
                name: node.name.clone(),
                node_index: node.index,
                parent,
                zone_id: node.info.zone_id,
                span: source.span(
                    nodes.info_offset + u64::from(node.index) * NODE_SLOT_BYTES,
                    NODE_SLOT_BYTES,
                )?,
            });
        }
        groups.push(AirframeRegionGroup::try_new(ancestry, regions)?);
    }

    // The wreck materials: every texture-table entry whose stem carries the
    // measured suffix, with the present material records naming it.
    let mut wreck_materials = Vec::new();
    for texture in &materials.textures {
        if !ends_with_ascii_folded(&texture.stem, WRECK_STEM_SUFFIX) {
            continue;
        }
        let material_indices: Vec<u32> = materials
            .materials
            .iter()
            .filter(|material| {
                materials
                    .texture_of(material)
                    .is_some_and(|named| named.index == texture.index)
            })
            .map(|material| material.index)
            .collect();
        wreck_materials.push(MeasuredWreckMaterial {
            stem: texture.stem.clone(),
            texture_name: texture.name.clone(),
            texture_index: texture.index,
            material_indices,
            bindings: Vec::new(),
            span: source.span(
                materials.textures_offset + u64::from(texture.index) * TEXTURE_INFO_BYTES,
                TEXTURE_INFO_BYTES,
            )?,
        });
    }

    // The bindings: which airframe subtree's meshes reference each of those
    // materials. A group's subtree is walked from its parent through the
    // stored child slots, once per node, so a malformed cycle can never loop.
    for group in &groups {
        for node_index in subtree_of(nodes, group.parent()) {
            let Some(node) = nodes.get(node_index) else {
                continue;
            };
            if node.info.mesh_index < 0 {
                continue;
            }
            let Ok(mesh_slot) = usize::try_from(node.info.mesh_index) else {
                continue;
            };
            let Some(mesh) = meshes.meshes.get(mesh_slot).and_then(Option::as_ref) else {
                continue;
            };
            let mut referenced: BTreeSet<u32> = BTreeSet::new();
            for polygon_groups in &mesh.material_groups {
                for material in polygon_groups {
                    referenced.insert(material.material);
                }
            }
            for material_index in referenced {
                let Some(material) = materials.material(material_index) else {
                    continue;
                };
                let Some(texture) = materials.texture_of(material) else {
                    continue;
                };
                if !ends_with_ascii_folded(&texture.stem, WRECK_STEM_SUFFIX) {
                    continue;
                }
                if let Some(record) = wreck_materials
                    .iter_mut()
                    .find(|record| record.texture_index == texture.index)
                {
                    record.bindings.push(WreckBinding {
                        group_parent: group.parent(),
                        node_index: node.index,
                        mesh_index: node.info.mesh_index,
                    });
                }
            }
        }
    }

    // What the rule did not take: every name carrying the marker, so the
    // boundary of the selection is part of the record.
    let mut discarded: Vec<DiscardedDamageName> = Vec::new();
    let mut node_names: BTreeMap<String, usize> = BTreeMap::new();
    for node in &nodes.nodes {
        if contains_ascii_folded(&node.name, DAMAGE_MARKER) {
            *node_names.entry(node.name.clone()).or_default() += 1;
        }
    }
    for (name, occurrences) in node_names {
        if !ends_with_ascii_folded(&name, REGION_SUFFIX) {
            discarded.push(DiscardedDamageName {
                name,
                occurrences,
                table: NameTable::Nodes,
            });
        }
    }
    let mut texture_names: BTreeMap<String, usize> = BTreeMap::new();
    for texture in &materials.textures {
        if contains_ascii_folded(&texture.name, DAMAGE_MARKER)
            && !ends_with_ascii_folded(&texture.stem, WRECK_STEM_SUFFIX)
        {
            *texture_names.entry(texture.name.clone()).or_default() += 1;
        }
    }
    for (name, occurrences) in texture_names {
        discarded.push(DiscardedDamageName {
            name,
            occurrences,
            table: NameTable::Textures,
        });
    }
    discarded.sort_by(|left, right| (left.table, &left.name).cmp(&(right.table, &right.name)));

    AirframeDamageVocabulary::try_new(source.clone(), groups, wreck_materials, discarded)
}

/// The stored ancestry of `start`: the node itself first, then each parent up
/// to the topmost one, with the stored names the walk read.
fn ancestry_of(
    nodes: &GameZNodes,
    start: u32,
) -> Result<Vec<AncestryNode>, AirframeDamageVocabularyError> {
    let mut chain = Vec::new();
    let mut seen = BTreeSet::new();
    let mut current = Some(start);
    let mut from = start;
    while let Some(index) = current {
        if !seen.insert(index) {
            return Err(AirframeDamageVocabularyError::AncestryCycle { node_index: index });
        }
        let node = nodes
            .get(index)
            .ok_or(AirframeDamageVocabularyError::MissingParent {
                node_index: from,
                parent: index,
            })?;
        chain.push(AncestryNode {
            index: node.index,
            name: node.name.clone(),
        });
        from = node.index;
        current = node.parent;
    }
    Ok(chain)
}

/// Every node reachable from `root` through the stored child slots,
/// including `root`, each once. A malformed cycle is visited once rather
/// than looped over.
fn subtree_of(nodes: &GameZNodes, root: u32) -> Vec<u32> {
    let mut visited = BTreeSet::new();
    let mut stack = vec![root];
    let mut order = Vec::new();
    while let Some(index) = stack.pop() {
        if !visited.insert(index) {
            continue;
        }
        order.push(index);
        let Some(node) = nodes.get(index) else {
            continue;
        };
        for child in &node.children {
            stack.push(*child);
        }
    }
    order
}

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

/// A newly authored airframe damage vocabulary for the unignored synthetic
/// regression: one airframe group holding `region_names`, plus at most one
/// wreck material, under [`Origin::SyntheticFixture`] with designed
/// provenance.
///
/// The count is whatever the caller names, which is the point: a fixture
/// measuring three regions makes the lowering accept three and refuse four,
/// so a check that quietly used a constant instead of reading its
/// observation cannot pass.
///
/// Nothing here can stand in for installation data, and nothing measured is
/// copied into it.
#[must_use]
pub fn synthetic_airframe_damage_vocabulary(
    region_names: &[&str],
    wreck_stem: Option<&str>,
) -> AirframeDamageVocabulary {
    let source =
        AirframeContainerSource::synthetic().expect("the fixture source builds its provenance");
    let parent = AncestryNode {
        index: 1,
        name: "synthetic.airframe".to_owned(),
    };
    let regions = region_names
        .iter()
        .enumerate()
        .map(|(position, name)| MeasuredDamageRegion {
            name: (*name).to_owned(),
            node_index: u32::try_from(position).expect("a fixture region index fits a u32") + 2,
            parent: parent.index,
            zone_id: 255,
            span: source
                .span(
                    u64::try_from(position).expect("a fixture offset fits a u64") * 212,
                    212,
                )
                .expect("the fixture span is inside its own container"),
        })
        .collect();
    let group = AirframeRegionGroup::try_new(vec![parent.clone()], regions)
        .expect("the fixture group has a parent and its regions");
    let wreck_materials = match wreck_stem {
        Some(stem) => vec![MeasuredWreckMaterial {
            stem: stem.to_owned(),
            texture_name: format!("{stem}.tif"),
            texture_index: 0,
            material_indices: vec![0],
            bindings: vec![WreckBinding {
                group_parent: parent.index,
                node_index: 2,
                mesh_index: 0,
            }],
            span: source
                .span(0, 44)
                .expect("the fixture span is inside its own container"),
        }],
        None => Vec::new(),
    };
    AirframeDamageVocabulary::try_new(source, vec![group], wreck_materials, Vec::new())
        .expect("the fixture vocabulary is well formed")
}
