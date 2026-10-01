//! The declared route graph: authored nodes, trigger volumes and reference
//! frames with provenance (F31-A).
//!
//! Spec: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, stage
//! `### F31-A`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! This module is the **content half** of the navigation contract — the
//! normalized, provenance-carrying route record a route importer produces and
//! the catalog consumes. Its runtime counterpart is `cs_sim::ai::navigation`
//! (the typed navigation input/output, the maneuver envelope and the bounded
//! route follower). The split mirrors `flight_tuning` ↔ `cs_sim::flight`:
//! this crate cannot depend on `cs_sim`, so the declared record keeps its own
//! typed fields and re-validates them at its boundary.
//!
//! # Records
//!
//! A [`RouteDefinition`] carries a stable `route` [`ContentId`], an
//! [`Origin`], the [`ReferenceFrame`] its positions live in (world or a moving
//! carrier/train/escort), a [`RouteTermination`], the minimum `clearance_m`
//! the follower must keep from blockers, the authored [`RouteNode`]s in
//! sequence order and the [`RouteEdge`]s that connect them.
//!
//! Every node keeps a stable [`RouteNodeId`] **and** an authored `sequence`
//! (F31 non-negotiable behavior 1): the sequence is what preserves mission
//! ordering, so progress can never be derived from an array index that a
//! later reorder could change. A node's position and its [`TriggerVolume`] are
//! each a [`Resolved`], so an unmeasured position or volume stays an explicit
//! unknown with a claim id and a reason instead of a silently invented zero.
//!
//! # Designed vocabulary, not original data
//!
//! The original 2000 PC game's route encoding is **not decoded**: F13 locates
//! mission programs but recovers no route-node layout, unit or trigger shape,
//! so which file stores routes, how a node is spelled and how a trigger fires
//! are all unknown (`docs/findings/2026-09-30-f31-a-route-graph-and-maneuver-envelope.md`).
//! Every id grammar, frame kind, termination kind, trigger shape and fixture
//! value here is therefore **newly authored project design** carrying
//! `Origin::Designed` / `Origin::SyntheticFixture` provenance. Nothing in this
//! module is a measurement of the original game.

use std::collections::HashSet;
use std::fmt;

use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

/// Maximum byte length of a [`RouteNodeId`] or [`TriggerVolumeId`] key.
pub const MAX_ROUTE_KEY_LEN: usize = 128;

// ------------------------------------------------- F31-D: mission types ---

/// The mission type a retail mission directory declares (F31-D).
///
/// The type is a classification of the *directory name* the installation
/// uses — `M<digits>` for a campaign mission, `IA<digits>` for an Instant
/// Action scenario, `MP<digits>` for a multiplayer scenario — exactly the
/// mission-directory shape F13-B's `mission_scope` and F14-E's campaign layout
/// already walk. It says nothing about the mission's contents: in particular
/// it makes no claim about whether the mission carries a route, where a route
/// would be stored or how it is encoded (that stays unmeasured; see
/// [`classify_mission_type`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MissionType {
    /// A campaign mission, `ZBD/<group>/M<digits>/`.
    Campaign,
    /// An Instant Action scenario, `ZBD/<group>/IA<digits>/`.
    InstantAction,
    /// A multiplayer scenario, `ZBD/<group>/MP<digits>/`.
    Multiplayer,
    /// A `ZBD/<group>/<name>` directory that is neither of the three above.
    Other,
}

impl MissionType {
    /// The stable label this type carries in a report.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Campaign => "campaign",
            Self::InstantAction => "instant_action",
            Self::Multiplayer => "multiplayer",
            Self::Other => "other",
        }
    }
}

/// Classifies a mission directory name by the mission type it declares
/// (F31-D).
///
/// The rule is the directory-name grammar only, matched without regard to
/// case: `M<digits>` is [`MissionType::Campaign`], `IA<digits>` is
/// [`MissionType::InstantAction`], `MP<digits>` is [`MissionType::Multiplayer`]
/// and anything else is [`MissionType::Other`]. A name that has the prefix but
/// no digits after it (`M`, `IA`) is [`MissionType::Other`], never a silent
/// campaign/Instant Action entry.
///
/// This is a **name classification, not a decode**: it is the same level of
/// evidence F13-B records for a mission program's member name. Whether a
/// mission of any type actually carries a route and how that route is encoded
/// is not measured here.
#[must_use]
pub fn classify_mission_type(name: &str) -> MissionType {
    if prefixed_digits(name, "IA").is_some() {
        MissionType::InstantAction
    } else if prefixed_digits(name, "MP").is_some() {
        MissionType::Multiplayer
    } else if prefixed_digits(name, "M").is_some() {
        MissionType::Campaign
    } else {
        MissionType::Other
    }
}

/// Returns the all-digits rest of `name` after a case-insensitive `prefix`, or
/// `None` when `name` does not carry that prefix followed by at least one
/// digit.
fn prefixed_digits<'a>(name: &'a str, prefix: &str) -> Option<&'a str> {
    let head = name.get(..prefix.len())?;
    if !head.eq_ignore_ascii_case(prefix) {
        return None;
    }
    let rest = name.get(prefix.len()..)?;
    if rest.is_empty() || !rest.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(rest)
}

// ------------------------------------------------------------------ ids ---

/// A stable identity for one node of a route.
///
/// The key is authored and preserved across re-parses; it is never an array
/// index, so reordering the node list cannot silently rename an authored
/// mission marker (F31 non-negotiable behavior 1).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RouteNodeId(String);

impl RouteNodeId {
    /// Validates a node key: non-empty, at most [`MAX_ROUTE_KEY_LEN`] bytes
    /// and free of control characters.
    ///
    /// # Errors
    ///
    /// [`RouteError::EmptyNodeId`], [`RouteError::NodeIdTooLong`] or
    /// [`RouteError::NodeIdControlCharacter`].
    pub fn try_new(key: &str) -> Result<Self, RouteError> {
        validate_key(
            key,
            RouteError::EmptyNodeId,
            RouteError::NodeIdTooLong,
            RouteError::NodeIdControlCharacter,
        )?;
        Ok(Self(key.to_owned()))
    }

    /// The authored key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RouteNodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A stable identity for one authored trigger volume.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TriggerVolumeId(String);

impl TriggerVolumeId {
    /// Validates a volume key; the same grammar as [`RouteNodeId::try_new`].
    ///
    /// # Errors
    ///
    /// [`RouteError::EmptyTriggerId`], [`RouteError::TriggerIdTooLong`] or
    /// [`RouteError::TriggerIdControlCharacter`].
    pub fn try_new(key: &str) -> Result<Self, RouteError> {
        validate_key(
            key,
            RouteError::EmptyTriggerId,
            RouteError::TriggerIdTooLong,
            RouteError::TriggerIdControlCharacter,
        )?;
        Ok(Self(key.to_owned()))
    }

    /// The authored key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TriggerVolumeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn validate_key(
    key: &str,
    empty: RouteError,
    too_long: RouteError,
    control: fn(char) -> RouteError,
) -> Result<(), RouteError> {
    if key.is_empty() {
        return Err(empty);
    }
    if key.len() > MAX_ROUTE_KEY_LEN {
        return Err(too_long);
    }
    if let Some(ch) = key.chars().find(|ch| ch.is_control()) {
        return Err(control(ch));
    }
    Ok(())
}

// ------------------------------------------------------------- geometry ---

/// The authored shape of a trigger volume, in the frame's canonical meters.
///
/// The original trigger shapes are unmeasured (see the module docs); these are
/// the declared project vocabulary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TriggerShape {
    /// A sphere centred on the node.
    Sphere {
        /// Radius in meters.
        radius_m: f64,
    },
    /// An axis-aligned box centred on the node.
    AxisAlignedBox {
        /// Half extents in meters, per axis.
        half_extents_m: [f64; 3],
    },
}

impl TriggerShape {
    /// Whether the shape is finite and strictly positive on every extent.
    #[must_use]
    pub fn is_valid(self) -> bool {
        match self {
            Self::Sphere { radius_m } => radius_m.is_finite() && radius_m > 0.0,
            Self::AxisAlignedBox { half_extents_m } => half_extents_m
                .into_iter()
                .all(|extent| extent.is_finite() && extent > 0.0),
        }
    }
}

/// One authored trigger volume with its own stable id.
///
/// A volume keeps its id so a mission event bound to it survives a re-parse
/// (F31 non-negotiable behavior 1).
#[derive(Clone, Debug, PartialEq)]
pub struct TriggerVolume {
    /// The stable volume id.
    pub id: TriggerVolumeId,
    /// The authored shape.
    pub shape: TriggerShape,
}

/// One authored route node.
///
/// `sequence` is the authored order that preserves mission progression; it is
/// separate from `id` so a node can be renamed without moving, and moved
/// without being renamed. The node's position, its arrival radius, its trigger
/// volume and its arrival relationship to the route are resolved values: an
/// unmeasured field is an explicit unknown, never a guessed origin.
///
/// The `arrival_radius_m` is the navigation **swept arrival volume** the
/// follower tests a step against; it is deliberately separate from `trigger`,
/// which is the authored mission **event** volume. A node can fire an event
/// without being a navigation marker and vice versa, so the two are resolved
/// independently and F31-C's projection never substitutes one for the other.
#[derive(Clone, Debug, PartialEq)]
pub struct RouteNode {
    /// The stable authored node id.
    pub id: RouteNodeId,
    /// The authored sequence number.
    pub sequence: u32,
    /// Whether skipping this node is forbidden (a mandatory marker).
    pub mandatory: bool,
    /// The node's position in the route's [`ReferenceFrame`], or an unknown.
    pub position_m: Resolved<[f64; 3]>,
    /// The radius of the navigation swept arrival volume, in meters, or an
    /// unknown. Distinct from the mission [`TriggerVolume`] in `trigger`.
    pub arrival_radius_m: Resolved<f64>,
    /// The trigger volume that fires at this node: `Known(Some)` for an
    /// authored volume, `Known(None)` for a node that deliberately has none,
    /// and `Unknown` for a node whose trigger was not measured.
    pub trigger: Resolved<Option<TriggerVolume>>,
}

/// One authored directed edge between two nodes.
///
/// An edge is only allowed between the nodes whose sequences are adjacent, so
/// an authored graph can never encode a shortcut that bypasses a mandatory
/// marker (F31 non-negotiable behavior 1).
#[derive(Clone, Debug, PartialEq)]
pub struct RouteEdge {
    /// The edge's source node.
    pub from: RouteNodeId,
    /// The edge's destination node.
    pub to: RouteNodeId,
}

/// Where a route's positions live.
#[derive(Clone, Debug, PartialEq)]
pub enum ReferenceFrame {
    /// World-anchored positions.
    World,
    /// Positions relative to a moving anchor (a carrier, train or escort).
    Moving(MovingAnchor),
}

/// A moving anchor a route is authored against.
#[derive(Clone, Debug, PartialEq)]
pub struct MovingAnchor {
    /// The anchor's stable content id.
    pub anchor: ContentId,
    /// The declared anchor kind.
    pub kind: AnchorKind,
}

/// The fewest nodes a [`RouteTermination::Loop`] route may declare.
///
/// A loop needs a real last-to-first edge; a one-node loop has none, so
/// re-arming it could never move the follower. This mirrors the runtime
/// follower's own constant, so producer and consumer agree on what a loop is.
pub const MIN_LOOP_NODES: usize = 2;

/// The declared kinds of moving anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorKind {
    /// A landable carrier.
    Carrier,
    /// A train.
    Train,
    /// An escorted aircraft.
    Escort,
    /// Any other moving anchor.
    Other,
}

/// What a route does after its last node.
///
/// Whether the original 2000 route encoding expresses a loop at all is
/// **unmeasured**: F13 recovers no route layout and F31-D owns retail route
/// coverage. Declaring the variant here is a project design choice so a record
/// that says `Loop` is followed as a loop rather than refused or silently
/// followed as an end; it is not a claim about original content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteTermination {
    /// The route ends at its last node.
    End,
    /// The route returns to its first node (a patrol), re-arming every node —
    /// mandatory markers included — in authored sequence order.
    Loop,
}

// --------------------------------------------------------------- route ---

/// One declared route: the normalized record the catalog stores.
///
/// Validation ([`RouteDefinition::try_new`]) refuses what the runtime cannot
/// honestly follow: an id outside the `route` namespace, no nodes, a duplicate
/// node or volume id, a non-increasing sequence, a non-finite known position,
/// a non-positive known trigger shape, a loop route with fewer than
/// [`MIN_LOOP_NODES`] nodes and an edge that is not between adjacent
/// sequences. Unknown references are not refused — they are data a later stage
/// must block on — but a *known* value of the wrong kind or the wrong sign is
/// an authoring error, not content.
#[derive(Clone, Debug, PartialEq)]
pub struct RouteDefinition {
    id: ContentId,
    origin: Origin,
    frame: ReferenceFrame,
    termination: RouteTermination,
    clearance_m: Resolved<f64>,
    nodes: Vec<RouteNode>,
    edges: Vec<RouteEdge>,
    provenance: Provenance,
}

/// The raw parts of a [`RouteDefinition`], collected so the validating
/// constructor takes one record instead of a long positional argument list.
#[derive(Clone, Debug, PartialEq)]
pub struct RouteDraft {
    /// The `route` content id.
    pub id: ContentId,
    /// Where the record came from.
    pub origin: Origin,
    /// The frame the node positions are authored in.
    pub frame: ReferenceFrame,
    /// What the route does at its last node.
    pub termination: RouteTermination,
    /// The minimum clearance from blockers, or an explicit unknown.
    pub clearance_m: Resolved<f64>,
    /// The nodes, in authored sequence order.
    pub nodes: Vec<RouteNode>,
    /// The declared edges.
    pub edges: Vec<RouteEdge>,
    /// The provenance of the record itself.
    pub provenance: Provenance,
}

impl RouteDefinition {
    /// Validates and assembles a declared route.
    ///
    /// # Errors
    ///
    /// [`RouteError`] for every rule the record must satisfy.
    pub fn try_new(draft: RouteDraft) -> Result<Self, RouteError> {
        let RouteDraft {
            id,
            origin,
            frame,
            termination,
            clearance_m,
            nodes,
            edges,
            provenance,
        } = draft;
        if id.kind() != ContentKind::Route {
            return Err(RouteError::NotARoute { kind: id.kind() });
        }
        if nodes.is_empty() {
            return Err(RouteError::EmptyNodes);
        }
        // A one-node loop has no wrap edge, so re-arming it would re-target the
        // node the follower already occupies and progress could never advance.
        // The rule mirrors the runtime follower's own
        // `cs_sim::ai::navigation::MIN_LOOP_NODES`, so a record that validates
        // here is one the runtime can honestly follow.
        if matches!(termination, RouteTermination::Loop) && nodes.len() < MIN_LOOP_NODES {
            return Err(RouteError::LoopNeedsMultipleNodes { nodes: nodes.len() });
        }

        let mut node_ids: HashSet<&str> = HashSet::new();
        let mut volume_ids: HashSet<&str> = HashSet::new();
        for (index, node) in nodes.iter().enumerate() {
            if !node_ids.insert(node.id.as_str()) {
                return Err(RouteError::DuplicateNodeId {
                    id: node.id.as_str().to_owned(),
                });
            }
            if let Some(previous) = index.checked_sub(1).map(|i| nodes[i].sequence)
                && node.sequence <= previous
            {
                return Err(RouteError::SequenceNotIncreasing {
                    index,
                    previous,
                    current: node.sequence,
                });
            }
            if let Resolved::Known(known) = &node.position_m {
                for (component, value) in known.value.into_iter().enumerate() {
                    if !value.is_finite() {
                        return Err(RouteError::NonFinitePosition {
                            node: node.id.as_str().to_owned(),
                            component,
                        });
                    }
                }
            }
            if let Resolved::Known(known) = &node.arrival_radius_m {
                if !known.value.is_finite() {
                    return Err(RouteError::NonFiniteArrivalRadius {
                        node: node.id.as_str().to_owned(),
                    });
                }
                if known.value <= 0.0 {
                    return Err(RouteError::NonPositiveArrivalRadius {
                        node: node.id.as_str().to_owned(),
                        value: known.value,
                    });
                }
            }
            if let Resolved::Known(known) = &node.trigger
                && let Some(volume) = &known.value
            {
                if !volume_ids.insert(volume.id.as_str()) {
                    return Err(RouteError::DuplicateTriggerVolume {
                        id: volume.id.as_str().to_owned(),
                    });
                }
                if !volume.shape.is_valid() {
                    return Err(RouteError::NonPositiveTrigger {
                        node: node.id.as_str().to_owned(),
                    });
                }
            }
        }

        if let Resolved::Known(known) = &clearance_m
            && !known.value.is_finite()
        {
            return Err(RouteError::NonFiniteClearance);
        }
        if let Resolved::Known(known) = &clearance_m
            && known.value < 0.0
        {
            return Err(RouteError::NegativeClearance { value: known.value });
        }

        let sequence_of = |id: &RouteNodeId| -> Option<u32> {
            nodes
                .iter()
                .find(|node| &node.id == id)
                .map(|node| node.sequence)
        };
        let mut edge_keys: HashSet<(&str, &str)> = HashSet::new();
        for edge in &edges {
            let Some(from) = sequence_of(&edge.from) else {
                return Err(RouteError::UnknownEdgeNode {
                    id: edge.from.as_str().to_owned(),
                });
            };
            let Some(to) = sequence_of(&edge.to) else {
                return Err(RouteError::UnknownEdgeNode {
                    id: edge.to.as_str().to_owned(),
                });
            };
            if from.checked_add(1) != Some(to) {
                return Err(RouteError::EdgeNotAdjacent {
                    from: edge.from.as_str().to_owned(),
                    to: edge.to.as_str().to_owned(),
                });
            }
            if !edge_keys.insert((edge.from.as_str(), edge.to.as_str())) {
                return Err(RouteError::DuplicateEdge {
                    from: edge.from.as_str().to_owned(),
                    to: edge.to.as_str().to_owned(),
                });
            }
        }

        Ok(Self {
            id,
            origin,
            frame,
            termination,
            clearance_m,
            nodes,
            edges,
            provenance,
        })
    }

    /// The route's `route` id.
    #[must_use]
    pub fn id(&self) -> &ContentId {
        &self.id
    }

    /// Where this route's bytes came from.
    #[must_use]
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The frame the node positions are authored in.
    #[must_use]
    pub fn frame(&self) -> &ReferenceFrame {
        &self.frame
    }

    /// What the route does at its last node.
    #[must_use]
    pub const fn termination(&self) -> RouteTermination {
        self.termination
    }

    /// The minimum clearance from blockers, or an explicit unknown.
    #[must_use]
    pub fn clearance_m(&self) -> &Resolved<f64> {
        &self.clearance_m
    }

    /// The nodes, in authored sequence order.
    #[must_use]
    pub fn nodes(&self) -> &[RouteNode] {
        &self.nodes
    }

    /// The declared edges.
    #[must_use]
    pub fn edges(&self) -> &[RouteEdge] {
        &self.edges
    }

    /// The provenance of the route record itself.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// How many nodes are mandatory markers.
    #[must_use]
    pub fn mandatory_node_count(&self) -> usize {
        self.nodes.iter().filter(|node| node.mandatory).count()
    }
}

/// Why a declared route was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum RouteError {
    /// The id is not in the `route` namespace.
    NotARoute {
        /// The kind it actually names.
        kind: ContentKind,
    },
    /// A route with no nodes has nothing to follow.
    EmptyNodes,
    /// A node key was empty.
    EmptyNodeId,
    /// A node key exceeded [`MAX_ROUTE_KEY_LEN`].
    NodeIdTooLong,
    /// A node key contained a control character.
    NodeIdControlCharacter(char),
    /// A trigger volume key was empty.
    EmptyTriggerId,
    /// A trigger volume key exceeded [`MAX_ROUTE_KEY_LEN`].
    TriggerIdTooLong,
    /// A trigger volume key contained a control character.
    TriggerIdControlCharacter(char),
    /// Two nodes share one id.
    DuplicateNodeId {
        /// The duplicated key.
        id: String,
    },
    /// Two trigger volumes share one id.
    DuplicateTriggerVolume {
        /// The duplicated key.
        id: String,
    },
    /// Node sequences were not strictly increasing.
    SequenceNotIncreasing {
        /// The offending node's list index.
        index: usize,
        /// The previous node's sequence.
        previous: u32,
        /// The offending node's sequence.
        current: u32,
    },
    /// A known node position was not finite.
    NonFinitePosition {
        /// The node whose position is corrupt.
        node: String,
        /// The offending component.
        component: usize,
    },
    /// A known arrival radius was not finite.
    NonFiniteArrivalRadius {
        /// The node whose arrival radius is corrupt.
        node: String,
    },
    /// A known arrival radius was not strictly positive.
    NonPositiveArrivalRadius {
        /// The node whose arrival radius is corrupt.
        node: String,
        /// The offending value.
        value: f64,
    },
    /// A known trigger volume was not strictly positive.
    NonPositiveTrigger {
        /// The node whose trigger is corrupt.
        node: String,
    },
    /// A known clearance was not finite.
    NonFiniteClearance,
    /// A known clearance was negative.
    NegativeClearance {
        /// The offending value.
        value: f64,
    },
    /// An edge named a node that does not exist.
    UnknownEdgeNode {
        /// The unmatched key.
        id: String,
    },
    /// An edge was not between adjacent sequences.
    EdgeNotAdjacent {
        /// The source key.
        from: String,
        /// The destination key.
        to: String,
    },
    /// An edge was declared more than once.
    DuplicateEdge {
        /// The source key.
        from: String,
        /// The destination key.
        to: String,
    },
    /// A loop route declared fewer nodes than it needs to wrap.
    LoopNeedsMultipleNodes {
        /// How many nodes the loop route declared.
        nodes: usize,
    },
}

impl fmt::Display for RouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotARoute { kind } => write!(f, "route id names a {kind}, not a route"),
            Self::EmptyNodes => write!(f, "a route must declare at least one node"),
            Self::EmptyNodeId => write!(f, "a route node id must not be empty"),
            Self::NodeIdTooLong => {
                write!(
                    f,
                    "a route node id is longer than {MAX_ROUTE_KEY_LEN} bytes"
                )
            }
            Self::NodeIdControlCharacter(ch) => {
                write!(f, "a route node id contains control character {ch:?}")
            }
            Self::EmptyTriggerId => write!(f, "a trigger volume id must not be empty"),
            Self::TriggerIdTooLong => {
                write!(
                    f,
                    "a trigger volume id is longer than {MAX_ROUTE_KEY_LEN} bytes"
                )
            }
            Self::TriggerIdControlCharacter(ch) => {
                write!(f, "a trigger volume id contains control character {ch:?}")
            }
            Self::DuplicateNodeId { id } => {
                write!(f, "route node id {id:?} is used more than once")
            }
            Self::DuplicateTriggerVolume { id } => {
                write!(f, "trigger volume id {id:?} is used more than once")
            }
            Self::SequenceNotIncreasing {
                index,
                previous,
                current,
            } => write!(
                f,
                "route node {index} has sequence {current}, not greater than the previous {previous}"
            ),
            Self::NonFinitePosition { node, component } => write!(
                f,
                "route node {node:?} position component {component} is not finite"
            ),
            Self::NonFiniteArrivalRadius { node } => {
                write!(f, "route node {node:?} arrival radius is not finite")
            }
            Self::NonPositiveArrivalRadius { node, value } => write!(
                f,
                "route node {node:?} arrival radius {value} is not strictly positive"
            ),
            Self::NonPositiveTrigger { node } => write!(
                f,
                "route node {node:?} trigger volume is not strictly positive"
            ),
            Self::NonFiniteClearance => write!(f, "route clearance_m is not finite"),
            Self::NegativeClearance { value } => {
                write!(f, "route clearance_m {value} is negative")
            }
            Self::UnknownEdgeNode { id } => {
                write!(f, "route edge names unknown node {id:?}")
            }
            Self::EdgeNotAdjacent { from, to } => write!(
                f,
                "route edge {from:?} -> {to:?} does not connect adjacent sequences"
            ),
            Self::DuplicateEdge { from, to } => {
                write!(
                    f,
                    "route edge {from:?} -> {to:?} is declared more than once"
                )
            }
            Self::LoopNeedsMultipleNodes { nodes } => write!(
                f,
                "a loop route needs at least {MIN_LOOP_NODES} nodes to wrap, got {nodes}"
            ),
        }
    }
}

impl std::error::Error for RouteError {}

// ------------------------------------------------------- resolved route ----

/// One route node with every navigation field resolved to a known value (F31-C).
///
/// It is the producer half of the [F31-C] conversion boundary: a consumer
/// crate (`cs_inspect`, and later `cs_app`) maps it into the runtime
/// `cs_sim::ai::navigation::RouteGraph`. An unmeasured position or arrival
/// radius never reaches this type, so a consumer cannot silently follow a
/// guessed node.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedRouteNode {
    /// The stable authored node id.
    pub id: RouteNodeId,
    /// The authored sequence number.
    pub sequence: u32,
    /// Whether skipping this node is forbidden (a mandatory marker).
    pub mandatory: bool,
    /// The known node position and its provenance.
    pub position_m: Known<[f64; 3]>,
    /// The known navigation arrival radius and its provenance.
    pub arrival_radius_m: Known<f64>,
    /// The node's mission trigger volume, kept unresolved because a mission
    /// event binding is not a navigation input.
    pub trigger: Resolved<Option<TriggerVolume>>,
}

/// One declared route with every navigation field resolved to a known value
/// (F31-C).
///
/// [`RouteDefinition::resolve`] is the error-propagating boundary: an unknown
/// clearance, position or arrival radius is refused by name instead of being
/// defaulted, so the runtime follower only ever consumes measured/designed
/// values.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedRoute {
    id: ContentId,
    origin: Origin,
    frame: ReferenceFrame,
    termination: RouteTermination,
    clearance_m: Known<f64>,
    nodes: Vec<ResolvedRouteNode>,
    provenance: Provenance,
}

impl ResolvedRoute {
    /// The route's `route` id.
    #[must_use]
    pub fn id(&self) -> &ContentId {
        &self.id
    }

    /// Where this route's bytes came from.
    #[must_use]
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The frame the node positions are authored in.
    #[must_use]
    pub fn frame(&self) -> &ReferenceFrame {
        &self.frame
    }

    /// What the route does at its last node.
    #[must_use]
    pub const fn termination(&self) -> RouteTermination {
        self.termination
    }

    /// The known minimum clearance from blockers.
    #[must_use]
    pub fn clearance_m(&self) -> &Known<f64> {
        &self.clearance_m
    }

    /// The resolved nodes, in authored sequence order.
    #[must_use]
    pub fn nodes(&self) -> &[ResolvedRouteNode] {
        &self.nodes
    }

    /// The provenance of the route record itself.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// Why a declared route could not be resolved for the runtime follower (F31-C).
#[derive(Clone, Debug, PartialEq)]
pub enum RouteResolutionError {
    /// The route clearance is an explicit unknown.
    UnknownClearance {
        /// The claim the unknown belongs to.
        claim_id: ClaimId,
        /// Why it is unknown.
        reason: String,
    },
    /// A node position is an explicit unknown.
    UnknownPosition {
        /// The offending node key.
        node: String,
        /// The claim the unknown belongs to.
        claim_id: ClaimId,
        /// Why it is unknown.
        reason: String,
    },
    /// A node arrival radius is an explicit unknown.
    UnknownArrivalRadius {
        /// The offending node key.
        node: String,
        /// The claim the unknown belongs to.
        claim_id: ClaimId,
        /// Why it is unknown.
        reason: String,
    },
}

impl fmt::Display for RouteResolutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownClearance { claim_id, reason } => {
                write!(f, "route clearance_m is unknown ({claim_id}): {reason}")
            }
            Self::UnknownPosition {
                node,
                claim_id,
                reason,
            } => write!(
                f,
                "route node {node:?} position is unknown ({claim_id}): {reason}"
            ),
            Self::UnknownArrivalRadius {
                node,
                claim_id,
                reason,
            } => write!(
                f,
                "route node {node:?} arrival radius is unknown ({claim_id}): {reason}"
            ),
        }
    }
}

impl std::error::Error for RouteResolutionError {}

impl RouteDefinition {
    /// Resolves every navigation field to a known value for the runtime
    /// follower, or names the first explicit unknown (F31-C).
    ///
    /// # Errors
    ///
    /// [`RouteResolutionError`] naming the unknown clearance, position or
    /// arrival radius. Never defaults an unknown, so a consumer cannot follow a
    /// guessed node.
    pub fn resolve(&self) -> Result<ResolvedRoute, RouteResolutionError> {
        let clearance_m = match &self.clearance_m {
            Resolved::Known(known) => known.clone(),
            Resolved::Unknown { claim_id, reason } => {
                return Err(RouteResolutionError::UnknownClearance {
                    claim_id: claim_id.clone(),
                    reason: reason.clone(),
                });
            }
        };
        let mut nodes = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            let position_m = match &node.position_m {
                Resolved::Known(known) => known.clone(),
                Resolved::Unknown { claim_id, reason } => {
                    return Err(RouteResolutionError::UnknownPosition {
                        node: node.id.as_str().to_owned(),
                        claim_id: claim_id.clone(),
                        reason: reason.clone(),
                    });
                }
            };
            let arrival_radius_m = match &node.arrival_radius_m {
                Resolved::Known(known) => known.clone(),
                Resolved::Unknown { claim_id, reason } => {
                    return Err(RouteResolutionError::UnknownArrivalRadius {
                        node: node.id.as_str().to_owned(),
                        claim_id: claim_id.clone(),
                        reason: reason.clone(),
                    });
                }
            };
            nodes.push(ResolvedRouteNode {
                id: node.id.clone(),
                sequence: node.sequence,
                mandatory: node.mandatory,
                position_m,
                arrival_radius_m,
                trigger: node.trigger.clone(),
            });
        }
        Ok(ResolvedRoute {
            id: self.id.clone(),
            origin: self.origin.clone(),
            frame: self.frame.clone(),
            termination: self.termination,
            clearance_m,
            nodes,
            provenance: self.provenance.clone(),
        })
    }
}

// ------------------------------------------------------------- fixture ----

/// The declared fallback clearance of [`declared_synthetic_arch_route`], in
/// meters. A designed value, never an original measurement.
pub const SYNTHETIC_ARCH_CLEARANCE_M: f64 = 2.0;

/// The minimal declared synthetic fixture: an authored route through a narrow
/// arch, with the arch itself carrying a stable trigger volume.
///
/// It mirrors the runtime fixture `cs_sim::ai::navigation::synthetic_arch_route`
/// so F31-C can assert the producer and the consumer agree; this stage only
/// declares and validates the record. Every identity lives under the
/// `synthetic` key, the record carries [`Origin::SyntheticFixture`] and every
/// value is designed — it can never be mistaken for original content and
/// cannot stand in for it.
#[must_use]
pub fn declared_synthetic_arch_route() -> RouteDefinition {
    let designed =
        || Provenance::designed(ClaimId::new("f31a.synthetic-arch-route").expect("valid"));
    let node =
        |id: &str, sequence: u32, mandatory: bool, position: [f64; 3], arrival_radius_m: f64| {
            RouteNode {
                id: RouteNodeId::try_new(id).expect("fixture node id is valid"),
                sequence,
                mandatory,
                position_m: Resolved::Known(cs_types::content::Known::new(position, designed())),
                arrival_radius_m: Resolved::Known(cs_types::content::Known::new(
                    arrival_radius_m,
                    designed(),
                )),
                // No trigger is authored for a plain waypoint.
                trigger: Resolved::Known(cs_types::content::Known::new(None, designed())),
            }
        };
    let arch_trigger = TriggerVolume {
        id: TriggerVolumeId::try_new("synthetic.arch.opening").expect("fixture volume id is valid"),
        shape: TriggerShape::Sphere { radius_m: 5.0 },
    };

    // The arrival radii are the navigation swept volumes of the geometer's
    // authored route; they intentionally match the runtime fixture
    // `cs_sim::ai::navigation::synthetic_arch_route` so F31-C's projection can
    // assert the producer and the consumer agree.
    let mut nodes = vec![
        node("start", 0, false, [0.0, 0.0, -30.0], 3.0),
        node("funnel", 1, true, [60.0, 5.0, -4.0], 5.0),
        node("arch", 2, true, [100.0, 5.0, 0.0], 5.0),
        node("exit", 3, true, [140.0, 5.0, -4.0], 5.0),
        node("goal", 4, true, [260.0, 5.0, -30.0], 8.0),
    ];
    nodes[2].trigger = Resolved::Known(cs_types::content::Known::new(
        Some(arch_trigger),
        designed(),
    ));

    let edges = vec![
        edge("start", "funnel"),
        edge("funnel", "arch"),
        edge("arch", "exit"),
        edge("exit", "goal"),
    ];

    RouteDefinition::try_new(RouteDraft {
        id: ContentId::from_source(ContentKind::Route, "synthetic.arch")
            .expect("fixture route id is valid"),
        origin: Origin::SyntheticFixture,
        frame: ReferenceFrame::World,
        termination: RouteTermination::End,
        clearance_m: Resolved::Known(cs_types::content::Known::new(
            SYNTHETIC_ARCH_CLEARANCE_M,
            designed(),
        )),
        nodes,
        edges,
        provenance: designed(),
    })
    .expect("the declared synthetic arch route is valid")
}

fn edge(from: &str, to: &str) -> RouteEdge {
    RouteEdge {
        from: RouteNodeId::try_new(from).expect("fixture node id is valid"),
        to: RouteNodeId::try_new(to).expect("fixture node id is valid"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_types::content::Known;

    fn route_id(key: &str) -> ContentId {
        ContentId::from_source(ContentKind::Route, key).expect("valid route id")
    }

    fn claim(id: &str) -> ClaimId {
        ClaimId::new(id).expect("valid claim id")
    }

    fn draft(
        id: ContentId,
        nodes: Vec<RouteNode>,
        edges: Vec<RouteEdge>,
        provenance: Provenance,
    ) -> RouteDraft {
        RouteDraft {
            id,
            origin: Origin::SyntheticFixture,
            frame: ReferenceFrame::World,
            termination: RouteTermination::End,
            clearance_m: Resolved::Known(Known::new(1.0, provenance.clone())),
            nodes,
            edges,
            provenance,
        }
    }

    /// The declared fixture is a synthetic, world-anchored route whose
    /// mandatory arch marker and trigger volume are present and whose edges
    /// connect adjacent sequences.
    #[test]
    fn accept_f31_a_declared_arch_route_is_synthetic_with_mandatory_marker() {
        let route = declared_synthetic_arch_route();
        assert_eq!(route.id().as_str(), "route/synthetic.arch");
        assert_eq!(route.origin(), &Origin::SyntheticFixture);
        assert!(!route.origin().is_original());
        assert_eq!(route.frame(), &ReferenceFrame::World);
        assert_eq!(route.termination(), RouteTermination::End);
        assert_eq!(route.nodes().len(), 5);
        assert_eq!(route.edges().len(), 4);
        assert_eq!(route.mandatory_node_count(), 4);
        assert_eq!(route.clearance_m().clone().known(), Some(2.0));

        let arch = route
            .nodes()
            .iter()
            .find(|node| node.id.as_str() == "arch")
            .expect("the arch node exists");
        let trigger = arch
            .trigger
            .clone()
            .known()
            .expect("the arch trigger is resolved")
            .expect("the arch node has an authored trigger volume");
        assert_eq!(trigger.id.as_str(), "synthetic.arch.opening");
        assert!(trigger.shape.is_valid());

        let start = route
            .nodes()
            .iter()
            .find(|node| node.id.as_str() == "start")
            .expect("the start node exists");
        assert_eq!(
            start.trigger.clone().known(),
            Some(None),
            "a plain waypoint explicitly has no trigger"
        );
    }

    /// A duplicate node id, a non-increasing sequence and an edge that skips a
    /// sequence are each refused by name.
    #[test]
    fn accept_f31_a_route_refuses_duplicate_ids_and_sequence_shortcuts() {
        let base = declared_synthetic_arch_route();
        let designed = Provenance::designed(claim("f31a.unit"));

        let mut duplicate = base.nodes().to_vec();
        duplicate[1].id = duplicate[0].id.clone();
        assert_eq!(
            RouteDefinition::try_new(draft(
                route_id("synthetic.duplicate"),
                duplicate,
                Vec::new(),
                designed.clone(),
            )),
            Err(RouteError::DuplicateNodeId {
                id: "start".to_owned()
            })
        );

        let mut unordered = base.nodes().to_vec();
        unordered[1].sequence = 0;
        assert_eq!(
            RouteDefinition::try_new(draft(
                route_id("synthetic.unordered"),
                unordered,
                Vec::new(),
                designed.clone(),
            )),
            Err(RouteError::SequenceNotIncreasing {
                index: 1,
                previous: 0,
                current: 0,
            })
        );

        // An edge that connects non-adjacent sequences is a shortcut.
        let shortcut = vec![RouteEdge {
            from: RouteNodeId::try_new("start").expect("valid"),
            to: RouteNodeId::try_new("arch").expect("valid"),
        }];
        assert_eq!(
            RouteDefinition::try_new(draft(
                route_id("synthetic.shortcut"),
                base.nodes().to_vec(),
                shortcut,
                designed,
            )),
            Err(RouteError::EdgeNotAdjacent {
                from: "start".to_owned(),
                to: "arch".to_owned(),
            })
        );
    }

    /// A known position that is not finite is refused, and an unknown position
    /// is carried instead of being read as an origin.
    #[test]
    fn accept_f31_a_route_refuses_nonfinite_position_and_carries_unknowns() {
        let base = declared_synthetic_arch_route();
        let designed = Provenance::designed(claim("f31a.unit"));

        let mut corrupt = base.nodes().to_vec();
        corrupt[2].position_m = Resolved::Known(Known::new(
            [f64::NAN, 5.0, 0.0],
            Provenance::designed(claim("f31a.corrupt")),
        ));
        assert_eq!(
            RouteDefinition::try_new(draft(
                route_id("synthetic.corrupt"),
                corrupt,
                Vec::new(),
                designed.clone(),
            )),
            Err(RouteError::NonFinitePosition {
                node: "arch".to_owned(),
                component: 0,
            })
        );

        let mut unknown = base.nodes().to_vec();
        unknown[2].position_m = Resolved::unknown(claim("f31a.unknown-position"), "not measured")
            .expect("a reason is present");
        let route = RouteDefinition::try_new(draft(
            route_id("synthetic.unknown"),
            unknown,
            Vec::new(),
            Provenance::designed(claim("f31a.unknown-route")),
        ))
        .expect("an unknown position is content, not an authoring error");
        assert_eq!(route.nodes()[2].position_m.clone().known(), None);
        assert!(!route.nodes()[2].position_m.is_known());
    }

    /// A node at the top of the sequence range must not overflow the adjacency
    /// check: an edge from it is refused by name rather than panicking.
    #[test]
    fn accept_f31_a_route_refuses_edge_from_the_last_sequence_without_overflow() {
        let designed = Provenance::designed(claim("f31a.overflow"));
        let node = |id: &str, sequence: u32| RouteNode {
            id: RouteNodeId::try_new(id).expect("fixture node id is valid"),
            sequence,
            mandatory: true,
            position_m: Resolved::Known(Known::new([0.0, 0.0, 0.0], designed.clone())),
            arrival_radius_m: Resolved::Known(Known::new(1.0, designed.clone())),
            trigger: Resolved::Known(Known::new(None, designed.clone())),
        };

        // Sequences increase, so the top node is last; an edge from it can have
        // no adjacent successor and must be refused instead of `u32::MAX + 1`.
        let edge = RouteEdge {
            from: RouteNodeId::try_new("top").expect("fixture node id is valid"),
            to: RouteNodeId::try_new("lower").expect("fixture node id is valid"),
        };
        assert_eq!(
            RouteDefinition::try_new(draft(
                route_id("synthetic.overflow"),
                vec![node("lower", 0), node("top", u32::MAX)],
                vec![edge],
                designed,
            )),
            Err(RouteError::EdgeNotAdjacent {
                from: "top".to_owned(),
                to: "lower".to_owned(),
            })
        );
    }

    /// The F31-D mission-type rule: the three observed directory families are
    /// classified, case-insensitively, and a prefix without digits (or an
    /// unrelated name) is `Other`, never a silent campaign entry.
    #[test]
    fn accept_f31_d_mission_type_classifies_the_observed_directory_families() {
        assert_eq!(classify_mission_type("M01"), MissionType::Campaign);
        assert_eq!(classify_mission_type("m12"), MissionType::Campaign);
        assert_eq!(classify_mission_type("IA1"), MissionType::InstantAction);
        assert_eq!(classify_mission_type("ia10"), MissionType::InstantAction);
        assert_eq!(classify_mission_type("MP1"), MissionType::Multiplayer);
        assert_eq!(classify_mission_type("mp3"), MissionType::Multiplayer);

        // A prefix with no digits and an unrelated name are `Other`; `MP` is
        // not misread as the `M` campaign prefix.
        assert_eq!(classify_mission_type("M"), MissionType::Other);
        assert_eq!(classify_mission_type("IA"), MissionType::Other);
        assert_eq!(classify_mission_type("MP"), MissionType::Other);
        assert_eq!(classify_mission_type("M01B"), MissionType::Other);
        assert_eq!(classify_mission_type("INTRO"), MissionType::Other);
        assert_eq!(classify_mission_type(""), MissionType::Other);

        assert_eq!(MissionType::Campaign.label(), "campaign");
        assert_eq!(MissionType::InstantAction.label(), "instant_action");
        assert_eq!(MissionType::Multiplayer.label(), "multiplayer");
        assert_eq!(MissionType::Other.label(), "other");
    }
}
