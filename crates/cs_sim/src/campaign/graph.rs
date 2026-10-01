//! The runtime campaign graph (F43-A).
//!
//! The lowered twin of `cs_content::campaign::CampaignDefinition`: concrete
//! values only (a lowered reward's currency is a `u64`, never an unknown),
//! keyed by [`CampaignNodeKey`], and re-validated here so a caller that
//! builds a graph directly cannot hand [`CampaignState::apply_outcome`] a
//! shape that would dead-end a run.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_script::ir::Outcome;
use cs_types::content::ContentId;

use super::identity::CampaignNodeKey;
use super::state::CampaignState;

/// What a runtime node is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeNodeKind {
    /// A playable mission, bound to its mission content id.
    Mission {
        /// The mission content id.
        mission: ContentId,
    },
    /// A narrative beat (briefing, cutscene, interlude): no outcome of its
    /// own; its single edge fires on `Abort`-free flow — declared explicitly
    /// as a `Victory` edge for uniformity.
    Interlude,
    /// A terminal campaign state; no outgoing edges.
    Ending,
}

/// A concrete grant in minor currency units and content ids.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reward {
    /// Cash granted, in minor units.
    pub currency: u64,
    /// Content the grant unlocks.
    pub unlocks: Vec<ContentId>,
}

/// A conditional transition: on `on`, move to `to`, granting `grant`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeEdge {
    /// The mission outcome that fires this edge.
    pub on: Outcome,
    /// The target node.
    pub to: CampaignNodeKey,
    /// The grant the transition applies.
    pub grant: Reward,
}

/// One node of the runtime graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeNode {
    /// The node's key.
    pub id: CampaignNodeKey,
    /// What it is.
    pub kind: RuntimeNodeKind,
    /// Its conditional transitions, at most one per outcome.
    pub edges: Vec<RuntimeEdge>,
}

/// An item the roster opens when a gate node completes for the first time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RosterGate {
    /// The content that becomes available.
    pub item: ContentId,
    /// The gating node.
    pub gate: CampaignNodeKey,
}

/// Why a [`CampaignGraph`] was rejected — the runtime mirror of the declared
/// shape rules, so a hand-built graph is held to the same contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GraphError {
    /// No nodes.
    Empty,
    /// Two nodes share a key.
    DuplicateNode {
        /// The repeated key.
        node: CampaignNodeKey,
    },
    /// The entry key names no node.
    MissingEntry {
        /// The undeclared key.
        node: CampaignNodeKey,
    },
    /// An edge targets an undeclared node.
    DanglingEdge {
        /// The source node.
        from: CampaignNodeKey,
        /// The undeclared target.
        to: CampaignNodeKey,
    },
    /// One node declares two edges for one outcome.
    DuplicateCondition {
        /// The node.
        node: CampaignNodeKey,
        /// The repeated outcome.
        on: Outcome,
    },
    /// An ending declares outgoing edges.
    EdgeFromEnding {
        /// The offending node.
        node: CampaignNodeKey,
    },
    /// A non-ending node has no way onward.
    DeadEnd {
        /// The node.
        node: CampaignNodeKey,
    },
    /// A roster gate names an undeclared node.
    UnknownRosterGate {
        /// The item.
        item: ContentId,
        /// The undeclared gate.
        gate: CampaignNodeKey,
    },
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "a campaign graph needs at least one node"),
            Self::DuplicateNode { node } => write!(f, "graph node {node} is declared twice"),
            Self::MissingEntry { node } => {
                write!(f, "the graph's entry node {node} is not declared")
            }
            Self::DanglingEdge { from, to } => {
                write!(f, "node {from} edges to undeclared node {to}")
            }
            Self::DuplicateCondition { node, on } => {
                write!(f, "node {node} declares two {on:?} edges")
            }
            Self::EdgeFromEnding { node } => {
                write!(f, "ending node {node} declares outgoing edges")
            }
            Self::DeadEnd { node } => {
                write!(f, "node {node} has no way onward and is not an ending")
            }
            Self::UnknownRosterGate { item, gate } => {
                write!(f, "roster item {item} is gated on undeclared node {gate}")
            }
        }
    }
}

impl std::error::Error for GraphError {}

/// The validated runtime graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CampaignGraph {
    nodes: BTreeMap<CampaignNodeKey, RuntimeNode>,
    entry: CampaignNodeKey,
    roster: Vec<RosterGate>,
}

impl CampaignGraph {
    /// Validates the graph with the same invariants the declared schema
    /// enforces.
    ///
    /// # Errors
    ///
    /// [`GraphError`].
    pub fn try_new(
        nodes: Vec<RuntimeNode>,
        entry: CampaignNodeKey,
        roster: Vec<RosterGate>,
    ) -> Result<Self, GraphError> {
        if nodes.is_empty() {
            return Err(GraphError::Empty);
        }
        let mut map = BTreeMap::new();
        for node in nodes {
            let id = node.id.clone();
            if map.insert(id.clone(), node).is_some() {
                return Err(GraphError::DuplicateNode { node: id });
            }
        }
        if !map.contains_key(&entry) {
            return Err(GraphError::MissingEntry { node: entry });
        }
        for node in map.values() {
            if node.kind == RuntimeNodeKind::Ending && !node.edges.is_empty() {
                return Err(GraphError::EdgeFromEnding {
                    node: node.id.clone(),
                });
            }
            if node.kind != RuntimeNodeKind::Ending && node.edges.is_empty() {
                return Err(GraphError::DeadEnd {
                    node: node.id.clone(),
                });
            }
            let mut conditions = BTreeSet::new();
            for edge in &node.edges {
                if !conditions.insert(edge.on) {
                    return Err(GraphError::DuplicateCondition {
                        node: node.id.clone(),
                        on: edge.on,
                    });
                }
                if !map.contains_key(&edge.to) {
                    return Err(GraphError::DanglingEdge {
                        from: node.id.clone(),
                        to: edge.to.clone(),
                    });
                }
            }
        }
        for gate in &roster {
            if !map.contains_key(&gate.gate) {
                return Err(GraphError::UnknownRosterGate {
                    item: gate.item.clone(),
                    gate: gate.gate.clone(),
                });
            }
        }
        Ok(Self {
            nodes: map,
            entry,
            roster,
        })
    }

    /// The designated entry node.
    pub fn entry(&self) -> &CampaignNodeKey {
        &self.entry
    }

    /// A node by key.
    pub fn node(&self, key: &CampaignNodeKey) -> Option<&RuntimeNode> {
        self.nodes.get(key)
    }

    /// The transition out of `node` for `outcome`, when one was declared.
    /// None is a real answer — no declared skip exists.
    pub fn transition(&self, node: &CampaignNodeKey, outcome: Outcome) -> Option<&RuntimeEdge> {
        self.nodes
            .get(node)?
            .edges
            .iter()
            .find(|edge| edge.on == outcome)
    }

    /// The roster gates.
    pub fn roster(&self) -> &[RosterGate] {
        &self.roster
    }

    /// Every node, in key order.
    pub fn nodes(&self) -> impl Iterator<Item = &RuntimeNode> {
        self.nodes.values()
    }

    /// The items a state has unlocked through roster gates — every gate
    /// whose node the run has completed.
    pub fn available_items<'a>(
        &'a self,
        state: &'a CampaignState,
    ) -> impl Iterator<Item = &'a ContentId> {
        self.roster
            .iter()
            .filter(move |gate| state.has_completed(&gate.gate))
            .map(|gate| &gate.item)
    }
}
