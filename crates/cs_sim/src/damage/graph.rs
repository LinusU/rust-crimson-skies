//! Damage graphs: typed nodes, channels and edges (F29-A).
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-A`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! A [`DamageGraph`] is the per-actor damage model one subject (an airframe,
//! a world object, a capital ship) declares: a set of [`DamageNode`]s keyed
//! by stable [`DamageNodeKey`]s — never by array position — with the kinds
//! the deliverable names (`armor zones, internal structure, engines and
//! weapon mounts are distinct where the original supports them`), the
//! integrity each node absorbs, the systems a node's destruction disables
//! and the two edge kinds damage travels:
//!
//! * `guarded_by`: an [`DamageNodeKind::ArmorZone`] node that absorbs an
//!   [`DamageChannel::Armor`] hit *before* the named node. Armor is its own
//!   integrity pool, so armor and internal channels produce distinguishable
//!   results without a multiplier ever being invented (AC02's shape);
//! * `overflow`: where a hit's remainder flows once a node is depleted —
//!   destroyed armor passes the rest of the shot into the part it guarded.
//!
//! World and capital-ship graphs use the same identity discipline and the
//! same node vocabulary; their *rules* differ and live in the declared
//! schema (`cs_content::damage`), not in special-cased node kinds.
//!
//! **Designed vocabulary, not original data.** The zone set, the armor
//! routing, the overflow rule and every fixture value are newly authored
//! project design. Which parts the original game modeled, whether its armor
//! depleted as a pool or scaled damage, and how it propagated overkill are
//! **unknown** until the compatibility work measures them; see
//! `docs/findings/2026-09-30-f29-a-damage-graphs-hit-ordering-lifecycle.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_types::content::{ContentId, Resolved};

/// Maximum byte length of a [`DamageNodeKey`], matching the
/// `IDENTITY-CONTENT` key bound.
pub const MAX_NODE_KEY_LEN: usize = 128;

/// Why a [`DamageNodeKey`] was rejected.
///
/// The grammar is the `IDENTITY-CONTENT` content-key grammar — lowercased
/// ASCII alphanumerics plus `.`, `_` and `-` with at least one
/// alphanumeric — applied to a graph-local identity instead of a catalog id:
/// the "same identity discipline" the sheet requires for every damage
/// graph. `cs_types` owns no shared key type yet (recorded in the findings
/// file), so this module validates its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeKeyError {
    /// The key was empty.
    Empty,
    /// The key exceeded [`MAX_NODE_KEY_LEN`] bytes.
    TooLong {
        /// Its length in bytes.
        len: usize,
    },
    /// The key contained a character outside `[a-z0-9._-]` (after ASCII
    /// lowercasing).
    BadCharacter {
        /// The offending character.
        ch: char,
    },
    /// The key had no ASCII alphanumeric character.
    NoAlphanumeric,
}

impl fmt::Display for NodeKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "a damage node key must not be empty"),
            Self::TooLong { len } => {
                write!(
                    f,
                    "a damage node key is {len} bytes, max is {MAX_NODE_KEY_LEN}"
                )
            }
            Self::BadCharacter { ch } => {
                write!(f, "a damage node key contains disallowed character {ch:?}")
            }
            Self::NoAlphanumeric => {
                write!(f, "a damage node key must contain an ASCII alphanumeric")
            }
        }
    }
}

impl std::error::Error for NodeKeyError {}

/// The stable identity of one node inside one [`DamageGraph`].
///
/// Keys are graph-local: `engine_1` inside one airframe's graph and
/// `engine_1` inside another's are different nodes because the owning actor
/// differs. The key is semantic — an authored part name — never the node's
/// position in any list.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DamageNodeKey(String);

impl DamageNodeKey {
    /// Validates and wraps a node key. Uppercase input is folded, matching
    /// [`ContentId`] normalization.
    ///
    /// # Errors
    ///
    /// [`NodeKeyError`] when the key is empty, too long, carries a character
    /// outside `[a-z0-9._-]` or has no alphanumeric.
    pub fn new(key: &str) -> Result<Self, NodeKeyError> {
        let key = key.to_ascii_lowercase();
        if key.is_empty() {
            return Err(NodeKeyError::Empty);
        }
        if key.len() > MAX_NODE_KEY_LEN {
            return Err(NodeKeyError::TooLong { len: key.len() });
        }
        for ch in key.chars() {
            if !ch.is_ascii_lowercase() && !ch.is_ascii_digit() && !matches!(ch, '.' | '_' | '-') {
                return Err(NodeKeyError::BadCharacter { ch });
            }
        }
        if !key.bytes().any(|byte| byte.is_ascii_alphanumeric()) {
            return Err(NodeKeyError::NoAlphanumeric);
        }
        Ok(Self(key))
    }

    /// The normalized key text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DamageNodeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What kind of part a damage node models (F29 deliverable).
///
/// The four kinds are the distinct part classes the sheet names. They are
/// labels that drive routing and transitions — never multipliers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DamageNodeKind {
    /// An armor plate or zone: its own integrity pool that absorbs
    /// [`DamageChannel::Armor`] hits before the part it guards.
    ArmorZone,
    /// Internal structure: spars, hull, frame. Usually the lethal part.
    InternalStructure,
    /// An engine; its destruction disables [`SystemKind::Propulsion`].
    Engine,
    /// A weapon mount; its destruction disables [`SystemKind::Weapon`]
    /// (AC03: the mount cannot fire and its visual state updates).
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

/// The channel a [`crate::damage::HitEvent`] routes on.
///
/// The channel chooses the entry node, never a damage factor:
/// [`DamageChannel::Armor`] enters through the target's declared armor
/// guard when one exists, so the shot depletes the armor pool before the
/// part behind it; [`DamageChannel::Internal`] enters the named node
/// directly — armor-piercing or internal-explosion semantics — so the same
/// raw damage on the two channels produces *distinguishable* results (the
/// armor pool versus the internal pool) without any invented multiplier
/// (AC02's model).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DamageChannel {
    /// Routed through the target's `guarded_by` armor node first.
    Armor,
    /// Applied to the named node directly; armor never intercepts it.
    Internal,
}

impl DamageChannel {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Armor => "armor",
            Self::Internal => "internal",
        }
    }
}

/// A gameplay system a destroyed part disables.
///
/// The node carrying the flag identifies *which* engine or mount went down
/// (AC03); this kind says which capability is lost. The actual gate — the
/// mount that stops firing, the thrust that drops — is the F29-B/C consumer
/// of the emitted transition.
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

/// The observable state of one damage node.
///
/// `Intact` means no damage applied yet, `Damaged` means integrity remains
/// but some was absorbed, `Destroyed` means the pool reached zero.
/// `Unknown` is reported — never guessed — for a node whose integrity is an
/// unresolved [`Resolved::Unknown`]: its state cannot be determined and is
/// recorded as such (AGENTS rule "unknown means unknown").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PartState {
    /// The node's integrity is unresolved; no state can be asserted.
    Unknown,
    /// Full integrity; nothing absorbed yet.
    Intact,
    /// Some integrity absorbed; the part is not destroyed.
    Damaged,
    /// Integrity reached zero.
    Destroyed,
}

impl PartState {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Intact => "intact",
            Self::Damaged => "damaged",
            Self::Destroyed => "destroyed",
        }
    }
}

/// One node of a [`DamageGraph`]: an armor zone, internal structure,
/// engine or weapon mount.
///
/// `integrity` is the part's hit-point pool as a [`Resolved`]: a known pool
/// must be finite and non-negative (checked by [`DamageGraph::try_new`]);
/// an unresolved pool stays `Resolved::Unknown` so a hit routed here blocks
/// visibly instead of being absorbed by an invented capacity.
///
/// `lethal` marks a part whose destruction destroys the whole actor.
/// `guarded_by` and `overflow` are the graph's edges; both name sibling
/// nodes by [`DamageNodeKey`].
#[derive(Clone, Debug, PartialEq)]
pub struct DamageNode {
    key: DamageNodeKey,
    kind: DamageNodeKind,
    integrity: Resolved<f64>,
    lethal: bool,
    disables: Option<SystemKind>,
    guarded_by: Option<DamageNodeKey>,
    overflow: Option<DamageNodeKey>,
    scene_binding: Option<Resolved<ContentId>>,
}

impl DamageNode {
    /// A node with the given key, kind and integrity pool.
    #[must_use]
    pub fn new(key: DamageNodeKey, kind: DamageNodeKind, integrity: Resolved<f64>) -> Self {
        Self {
            key,
            kind,
            integrity,
            lethal: false,
            disables: None,
            guarded_by: None,
            overflow: None,
            scene_binding: None,
        }
    }

    /// Marks the node lethal: its destruction destroys the actor.
    #[must_use]
    pub fn with_lethal(mut self, lethal: bool) -> Self {
        self.lethal = lethal;
        self
    }

    /// Marks the system this node's destruction disables.
    #[must_use]
    pub fn with_disables(mut self, system: SystemKind) -> Self {
        self.disables = Some(system);
        self
    }

    /// Declares the armor node that absorbs [`DamageChannel::Armor`] hits
    /// aimed at this node first.
    #[must_use]
    pub fn with_guard(mut self, guard: DamageNodeKey) -> Self {
        self.guarded_by = Some(guard);
        self
    }

    /// Declares where a hit's remainder flows once this node is depleted.
    #[must_use]
    pub fn with_overflow(mut self, target: DamageNodeKey) -> Self {
        self.overflow = Some(target);
        self
    }

    /// Binds the node to its visual scene node (a `scene_node`
    /// [`ContentId`]) for the presentation consumer. Damage never reads
    /// this to decide anything — visuals consume damage state, never the
    /// reverse (F29 non-negotiable behavior 1).
    #[must_use]
    pub fn with_scene_binding(mut self, binding: Resolved<ContentId>) -> Self {
        self.scene_binding = Some(binding);
        self
    }

    /// The node's stable key.
    #[must_use]
    pub fn key(&self) -> &DamageNodeKey {
        &self.key
    }

    /// The node's part kind.
    #[must_use]
    pub const fn kind(&self) -> DamageNodeKind {
        self.kind
    }

    /// The integrity pool, or an explicit unknown.
    #[must_use]
    pub const fn integrity(&self) -> &Resolved<f64> {
        &self.integrity
    }

    /// Whether destroying this node destroys the actor.
    #[must_use]
    pub const fn is_lethal(&self) -> bool {
        self.lethal
    }

    /// The system this node disables on destruction, if any.
    #[must_use]
    pub const fn disables(&self) -> Option<SystemKind> {
        self.disables
    }

    /// The armor node guarding this one, if any.
    #[must_use]
    pub const fn guarded_by(&self) -> Option<&DamageNodeKey> {
        self.guarded_by.as_ref()
    }

    /// The node remainder damage flows to, if any.
    #[must_use]
    pub const fn overflow(&self) -> Option<&DamageNodeKey> {
        self.overflow.as_ref()
    }

    /// The visual scene node this part binds to, if declared.
    #[must_use]
    pub const fn scene_binding(&self) -> Option<&Resolved<ContentId>> {
        self.scene_binding.as_ref()
    }
}

/// Why a [`DamageGraph`] was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum DamageGraphError {
    /// A graph with no nodes models nothing.
    EmptyGraph,
    /// Two nodes share one key; identity would be ambiguous.
    DuplicateNode {
        /// The duplicated key.
        key: DamageNodeKey,
    },
    /// A node's known integrity was NaN or infinite.
    NonFiniteIntegrity {
        /// The offending node.
        node: DamageNodeKey,
    },
    /// A node's known integrity was negative; a pool cannot start in debt.
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
    /// A `guarded_by` edge names a node that is not an
    /// [`DamageNodeKind::ArmorZone`] — only armor guards.
    GuardNotArmor {
        /// The node carrying the edge.
        node: DamageNodeKey,
        /// The guard's actual kind.
        kind: DamageNodeKind,
    },
    /// An [`DamageNodeKind::ArmorZone`] declares `guarded_by`; armor is the
    /// front of a chain and is never itself armor-guarded.
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
    /// The `overflow` edges form a cycle; remainder damage would loop
    /// forever.
    OverflowCycle {
        /// One node on the cycle.
        node: DamageNodeKey,
    },
}

impl fmt::Display for DamageGraphError {
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
            Self::SelfOverflow { node } => write!(f, "node {node} cannot overflow into itself"),
            Self::OverflowCycle { node } => {
                write!(f, "overflow edges form a cycle through node {node}")
            }
        }
    }
}

impl std::error::Error for DamageGraphError {}

/// One actor's damage model: the typed graph the [`crate::damage::DamageResolver`]
/// resolves hits against.
///
/// `subject` is the catalog id of what this graph describes (an airframe, a
/// world object, a capital ship) — the same identity discipline every
/// graph kind shares. The map is keyed by [`DamageNodeKey`], so node order
/// is canonical and identity never depends on declaration order.
#[derive(Clone, Debug, PartialEq)]
pub struct DamageGraph {
    subject: ContentId,
    nodes: BTreeMap<DamageNodeKey, DamageNode>,
}

impl DamageGraph {
    /// Assembles and validates a graph.
    ///
    /// # Errors
    ///
    /// [`DamageGraphError`] on an empty graph, a duplicate key, a corrupt
    /// known integrity, a dangling or mistyped `guarded_by` edge, an
    /// armor-guarded armor node, a self edge, or an `overflow` cycle.
    pub fn try_new(subject: ContentId, nodes: Vec<DamageNode>) -> Result<Self, DamageGraphError> {
        if nodes.is_empty() {
            return Err(DamageGraphError::EmptyGraph);
        }
        let mut by_key = BTreeMap::new();
        for node in nodes {
            let key = node.key().clone();
            if by_key.contains_key(&key) {
                return Err(DamageGraphError::DuplicateNode { key });
            }
            by_key.insert(key, node);
        }

        for node in by_key.values() {
            if let Resolved::Known(known) = node.integrity() {
                if !known.value.is_finite() {
                    return Err(DamageGraphError::NonFiniteIntegrity {
                        node: node.key().clone(),
                    });
                }
                if known.value < 0.0 {
                    return Err(DamageGraphError::NegativeIntegrity {
                        node: node.key().clone(),
                        value: known.value,
                    });
                }
            }
            if let Some(guard) = node.guarded_by() {
                if guard == node.key() {
                    return Err(DamageGraphError::SelfGuard {
                        node: node.key().clone(),
                    });
                }
                if node.kind() == DamageNodeKind::ArmorZone {
                    return Err(DamageGraphError::ArmorGuarded {
                        node: node.key().clone(),
                    });
                }
                let Some(target) = by_key.get(guard) else {
                    return Err(DamageGraphError::UnknownGuard {
                        node: node.key().clone(),
                        guard: guard.clone(),
                    });
                };
                if target.kind() != DamageNodeKind::ArmorZone {
                    return Err(DamageGraphError::GuardNotArmor {
                        node: node.key().clone(),
                        kind: target.kind(),
                    });
                }
            }
            if let Some(overflow) = node.overflow() {
                if overflow == node.key() {
                    return Err(DamageGraphError::SelfOverflow {
                        node: node.key().clone(),
                    });
                }
                if !by_key.contains_key(overflow) {
                    return Err(DamageGraphError::UnknownOverflow {
                        node: node.key().clone(),
                        overflow: overflow.clone(),
                    });
                }
            }
        }

        // Overflow edges form a partial order: walk each chain and detect a
        // revisit. The graph is finite, so a chain that exceeds the node
        // count must have cycled.
        for start in by_key.keys() {
            let mut seen = BTreeSet::new();
            let mut current = start;
            loop {
                if !seen.insert(current) {
                    return Err(DamageGraphError::OverflowCycle {
                        node: current.clone(),
                    });
                }
                let Some(next) = by_key[current].overflow() else {
                    break;
                };
                current = next;
            }
        }

        Ok(Self {
            subject,
            nodes: by_key,
        })
    }

    /// The catalog id of the subject this graph describes.
    #[must_use]
    pub fn subject(&self) -> &ContentId {
        &self.subject
    }

    /// A node by key.
    #[must_use]
    pub fn node(&self, key: &DamageNodeKey) -> Option<&DamageNode> {
        self.nodes.get(key)
    }

    /// Every node, in canonical key order.
    pub fn nodes(&self) -> impl Iterator<Item = &DamageNode> {
        self.nodes.values()
    }

    /// How many nodes the graph holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the graph holds no nodes (always false after [`Self::try_new`]).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}
