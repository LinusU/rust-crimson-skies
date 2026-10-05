//! The declared campaign graph: nodes, conditional transitions, rewards and
//! roster availability with provenance (F43-A).
//!
//! Spec: `specs/F43-campaign-progression-outcomes-and-economy-rules.md`,
//! stage `### F43-A`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! This module is the **content half** of the campaign contract — the
//! normalized, provenance-carrying record an importer produces from the
//! original campaign layout (`cs_content::campaign_bindings` already records
//! the observed `ZBD/<chapter><variant>/<mission>` structure, F50-A). Its
//! runtime counterpart is `cs_sim::campaign` — the [`CampaignState`]
//! transaction and its exactly-once outcome ledger — and the conversion
//! boundary is `cs_app::campaign`. The split mirrors `audio` ↔
//! `cs_app::audio`: this crate cannot depend on `cs_sim`, so the declared
//! record keeps its own typed fields.
//!
//! [`CampaignState`]: https://docs.rs/cs_sim (runtime half; named here for
//! orientation only)
//!
//! # The graph
//!
//! A [`CampaignDefinition`] is a directed graph of [`CampaignNode`]s keyed by
//! [`CampaignNodeId`]: [`NodeKind::Mission`] nodes (bound to their mission
//! content id — [`Resolved::Unknown`] while the binding is unsurveyed),
//! [`NodeKind::Interlude`] narrative beats (briefings and cutscenes) and
//! [`NodeKind::Ending`] terminal states. Every [`CampaignEdge`] leaves a node
//! on a declared [`EdgeCondition`] (`Victory`, `Defeat`, `Abort`) and may
//! carry a [`RewardSpec`]. `try_new` refuses duplicate node ids, dangling
//! edge targets, a missing entry node, edges out of an ending and a
//! mission node whose resolved binding is not a `ContentKind::Mission` —
//! the invariants a consumer needs so it never has to guess.
//!
//! **What an edge means is declared, never invented** (spec F43
//! non-negotiable behavior 4): whether defeat retries in place or skips
//! forward is whichever edge the record declares, and no edge means *no*
//! transition — there is no hidden "three failures and skip" rule in this
//! schema.
//!
//! # Rewards and roster
//!
//! A [`RewardSpec`] grants currency (in **minor units**, the integer the
//! contract's economy transaction moves — display conversion lives at the
//! UI) and `ContentId` unlocks when its edge fires. [`RosterEntry`] declares
//! when an item (an airframe blueprint, an equipment choice) becomes
//! available: the node whose first completion opens it. Currency amounts are
//! [`Resolved`]: an unmeasured reward is an explicit unknown and refuses at
//! the lowering boundary rather than paying a guessed amount.
//!
//! # What is designed and what is unknown
//!
//! The original campaign's *ordering evidence* is the observed
//! chapter/mission directory layout; the *rules* — exact reward amounts,
//! defeat/retry/skip behavior, roster pricing — are unmeasured until F43-D's
//! retail playthrough. Every rule here carries `Designed` or
//! `SyntheticFixture` provenance, and the findings record
//! (`docs/findings/2026-10-01-f43-a-campaign-graph-and-outcome-schema.md`)
//! lists what later stages still owe.

use std::collections::BTreeMap;
use std::fmt;

use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;

/// The longest accepted [`CampaignNodeId`].
pub const MAX_NODE_ID_LENGTH: usize = 64;

// -------------------------------------------------------------- node ids ---

/// A node id inside a campaign graph: nonempty `[a-z0-9_.-]` text.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CampaignNodeId(String);

/// Why a [`CampaignNodeId`] was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeIdError {
    /// The id was empty.
    Empty,
    /// The id was longer than [`MAX_NODE_ID_LENGTH`].
    TooLong {
        /// The rejected length.
        length: usize,
    },
    /// The id held a character outside `[a-z0-9_.-]`.
    InvalidCharacter {
        /// The offending character.
        ch: char,
        /// Its byte position.
        position: usize,
    },
}

impl fmt::Display for NodeIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "a campaign node id cannot be empty"),
            Self::TooLong { length } => write!(
                f,
                "a campaign node id cannot exceed {MAX_NODE_ID_LENGTH} characters ({length} given)"
            ),
            Self::InvalidCharacter { ch, position } => write!(
                f,
                "campaign node id holds {ch:?} at {position}; only [a-z0-9_.-] is allowed"
            ),
        }
    }
}

impl std::error::Error for NodeIdError {}

impl CampaignNodeId {
    /// Validates an id.
    ///
    /// # Errors
    ///
    /// [`NodeIdError`].
    pub fn new(text: &str) -> Result<Self, NodeIdError> {
        if text.is_empty() {
            return Err(NodeIdError::Empty);
        }
        if text.len() > MAX_NODE_ID_LENGTH {
            return Err(NodeIdError::TooLong { length: text.len() });
        }
        for (position, ch) in text.char_indices() {
            if !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | '.' | '-')) {
                return Err(NodeIdError::InvalidCharacter { ch, position });
            }
        }
        Ok(Self(text.to_owned()))
    }

    /// The id text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CampaignNodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ------------------------------------------------------------------- nodes ---

/// What a campaign node is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeKind {
    /// A playable mission; `mission` is its catalog binding, [`Resolved`] —
    /// unknown while the original mission's content id is unsurveyed.
    Mission {
        /// The mission content id (`ContentKind::Mission`).
        mission: Resolved<ContentId>,
    },
    /// A narrative beat between missions (briefing, cutscene, interlude):
    /// it has no outcome of its own and transitions unconditionally.
    Interlude {
        /// The media/asset the beat plays, when bound.
        asset: Resolved<ContentId>,
    },
    /// A terminal campaign state. An ending has no outgoing edges.
    Ending,
}

/// One node of a [`CampaignDefinition`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CampaignNode {
    /// The node's id.
    pub id: CampaignNodeId,
    /// What it is.
    pub kind: NodeKind,
    /// The ordered conditional transitions out of it.
    pub edges: Vec<CampaignEdge>,
    /// Where the record came from.
    pub provenance: Provenance,
}

// ------------------------------------------------------------------- edges ---

/// The outcome an edge fires on.
///
/// The vocabulary is the designed projection of the mission runtime's
/// terminal states (`Succeeded`, `Failed`, `Aborted`); which edges the
/// *original* campaign declared is unmeasured until F43-D.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EdgeCondition {
    /// The mission ended in success.
    Victory,
    /// The mission ended in failure.
    Defeat,
    /// The mission was aborted (quit); the campaign may route this
    /// differently from a defeat.
    Abort,
}

impl EdgeCondition {
    /// Every condition, in a stable order.
    pub const ALL: &'static [EdgeCondition] = &[Self::Victory, Self::Defeat, Self::Abort];

    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Victory => "victory",
            Self::Defeat => "defeat",
            Self::Abort => "abort",
        }
    }
}

/// What a fired edge grants, in minor units and content ids.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RewardSpec {
    /// Cash granted, in minor currency units. [`Resolved::Unknown`] while the
    /// original amount is unmeasured — the lowering boundary refuses it
    /// rather than paying a guess (spec F43 non-negotiable behavior 4).
    pub currency: Resolved<u64>,
    /// Content the grant unlocks (aircraft, equipment, media).
    pub unlocks: Vec<ContentId>,
}

/// A conditional transition: on `condition`, move to `to`, granting `grant`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CampaignEdge {
    /// The outcome that fires this edge.
    pub on: EdgeCondition,
    /// The target node.
    pub to: CampaignNodeId,
    /// The grant the transition applies (empty spec = none).
    pub grant: Option<RewardSpec>,
    /// Where this transition's rule came from — a designed edge and a
    /// measured one are never indistinguishable.
    pub provenance: Provenance,
}

/// An item the campaign roster opens when a node completes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RosterEntry {
    /// The purchasable/flyable content.
    pub item: ContentId,
    /// The node whose first completion makes it available.
    pub available_from: CampaignNodeId,
    /// Where the record came from.
    pub provenance: Provenance,
}

// -------------------------------------------------------------- definition ---

/// Why a [`CampaignDefinition`] was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CampaignError {
    /// Two nodes share an id.
    DuplicateNode {
        /// The repeated id.
        node: CampaignNodeId,
    },
    /// The named entry node is not declared.
    MissingEntry {
        /// The undeclared id.
        node: CampaignNodeId,
    },
    /// The entry node is an interlude. Designed engine rule (not measured from
    /// original data): a run begins on a node that can report an outcome, so a
    /// narrative beat cannot be the entry.
    InterludeEntry {
        /// The entry node.
        node: CampaignNodeId,
    },
    /// An edge names a node that is not declared.
    DanglingEdge {
        /// The edge's source node.
        from: CampaignNodeId,
        /// The undeclared target.
        to: CampaignNodeId,
    },
    /// One node declares two edges for the same condition.
    DuplicateCondition {
        /// The node.
        node: CampaignNodeId,
        /// The repeated condition.
        on: EdgeCondition,
    },
    /// An ending node declares outgoing edges — endings are terminal.
    EdgeFromEnding {
        /// The offending node.
        node: CampaignNodeId,
    },
    /// A non-ending node has no way onward — a campaign that can dead-end is
    /// a definition defect, not a runtime surprise.
    DeadEnd {
        /// The node with no outgoing edges.
        node: CampaignNodeId,
    },
    /// A roster entry keys availability to an undeclared node.
    UnknownRosterGate {
        /// The item.
        item: ContentId,
        /// The undeclared gate node.
        node: CampaignNodeId,
    },
    /// A mission node's resolved binding is not a [`ContentKind::Mission`]
    /// — a node that says "mission" must name a mission. An
    /// unsurveyed ([`Resolved::Unknown`]) binding stays legal: it refuses at
    /// the lowering boundary instead.
    MissionBindingKind {
        /// The node.
        node: CampaignNodeId,
        /// The binding's actual kind.
        kind: ContentKind,
    },
    /// A campaign needs at least one node.
    Empty,
}

impl fmt::Display for CampaignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateNode { node } => write!(f, "campaign node {node} is declared twice"),
            Self::MissingEntry { node } => {
                write!(f, "the campaign's entry node {node} is not declared")
            }
            Self::InterludeEntry { node } => {
                write!(f, "the campaign's entry node {node} is an interlude")
            }
            Self::DanglingEdge { from, to } => {
                write!(f, "node {from} edges to undeclared node {to}")
            }
            Self::DuplicateCondition { node, on } => {
                write!(f, "node {node} declares two {} edges", on.label())
            }
            Self::EdgeFromEnding { node } => {
                write!(f, "ending node {node} declares outgoing edges")
            }
            Self::DeadEnd { node } => {
                write!(f, "node {node} has no way onward and is not an ending")
            }
            Self::UnknownRosterGate { item, node } => {
                write!(f, "roster item {item} is gated on undeclared node {node}")
            }
            Self::MissionBindingKind { node, kind } => {
                write!(f, "mission node {node} is bound to a {kind:?} content id")
            }
            Self::Empty => write!(f, "a campaign needs at least one node"),
        }
    }
}

impl std::error::Error for CampaignError {}

/// The declared campaign: an ordered, conditional graph with a designated
/// entry and roster availability gates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CampaignDefinition {
    nodes: BTreeMap<CampaignNodeId, CampaignNode>,
    entry: CampaignNodeId,
    roster: Vec<RosterEntry>,
    provenance: Provenance,
}

/// The inputs of a [`CampaignDefinition`].
#[derive(Clone, Debug)]
pub struct CampaignDraft {
    /// Every node, including the entry.
    pub nodes: Vec<CampaignNode>,
    /// The node a fresh campaign run starts on.
    pub entry: CampaignNodeId,
    /// Roster availability gates.
    pub roster: Vec<RosterEntry>,
    /// Where the campaign record came from.
    pub provenance: Provenance,
}

impl CampaignDefinition {
    /// Validates the graph: unique node ids, a declared entry, no dangling
    /// edge targets, at most one edge per condition per node, no edges out
    /// of an ending, no non-ending dead ends, roster gates that name
    /// declared nodes, and mission nodes whose resolved bindings are
    /// actually [`ContentKind::Mission`] ids.
    ///
    /// # Errors
    ///
    /// [`CampaignError`].
    pub fn try_new(draft: CampaignDraft) -> Result<Self, CampaignError> {
        if draft.nodes.is_empty() {
            return Err(CampaignError::Empty);
        }
        let mut nodes = BTreeMap::new();
        for node in draft.nodes {
            let id = node.id.clone();
            if nodes.insert(id.clone(), node).is_some() {
                return Err(CampaignError::DuplicateNode { node: id });
            }
        }
        if !nodes.contains_key(&draft.entry) {
            return Err(CampaignError::MissingEntry { node: draft.entry });
        }
        if matches!(nodes[&draft.entry].kind, NodeKind::Interlude { .. }) {
            return Err(CampaignError::InterludeEntry { node: draft.entry });
        }
        for node in nodes.values() {
            if let NodeKind::Mission {
                mission: Resolved::Known(known),
            } = &node.kind
                && known.value.kind() != ContentKind::Mission
            {
                return Err(CampaignError::MissionBindingKind {
                    node: node.id.clone(),
                    kind: known.value.kind(),
                });
            }
            if node.kind == NodeKind::Ending && !node.edges.is_empty() {
                return Err(CampaignError::EdgeFromEnding {
                    node: node.id.clone(),
                });
            }
            if node.kind != NodeKind::Ending && node.edges.is_empty() {
                return Err(CampaignError::DeadEnd {
                    node: node.id.clone(),
                });
            }
            let mut conditions = std::collections::BTreeSet::new();
            for edge in &node.edges {
                if !conditions.insert(edge.on) {
                    return Err(CampaignError::DuplicateCondition {
                        node: node.id.clone(),
                        on: edge.on,
                    });
                }
                if !nodes.contains_key(&edge.to) {
                    return Err(CampaignError::DanglingEdge {
                        from: node.id.clone(),
                        to: edge.to.clone(),
                    });
                }
            }
        }
        for entry in &draft.roster {
            if !nodes.contains_key(&entry.available_from) {
                return Err(CampaignError::UnknownRosterGate {
                    item: entry.item.clone(),
                    node: entry.available_from.clone(),
                });
            }
        }
        Ok(Self {
            nodes,
            entry: draft.entry,
            roster: draft.roster,
            provenance: draft.provenance,
        })
    }

    /// The designated entry node.
    pub fn entry(&self) -> &CampaignNodeId {
        &self.entry
    }

    /// A node by id.
    pub fn node(&self, id: &CampaignNodeId) -> Option<&CampaignNode> {
        self.nodes.get(id)
    }

    /// Every node, in id order.
    pub fn nodes(&self) -> impl Iterator<Item = &CampaignNode> {
        self.nodes.values()
    }

    /// The roster gates.
    pub fn roster(&self) -> &[RosterEntry] {
        &self.roster
    }

    /// Where the record came from.
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

// -------------------------------------------------------------- fixture ---

/// The synthetic campaign used by the acceptance tests: three nodes —
/// mission `m01`, mission `m02`, ending — wired victory-forward and
/// defeat-in-place (a retry loop edge is *declared*, not implied), plus one
/// roster gate. Every value is authored fixture data carrying `Designed`
/// provenance; nothing asserts original campaign rules.
#[must_use]
pub fn declared_synthetic_campaign() -> CampaignDefinition {
    fn designed() -> Provenance {
        Provenance::designed(ClaimId::new("f43a.synthetic-campaign").expect("valid"))
    }
    fn known<T>(value: T) -> Resolved<T> {
        Resolved::Known(Known::new(value, designed()))
    }
    fn node(id: &str) -> CampaignNodeId {
        CampaignNodeId::new(id).expect("fixture node id is valid")
    }
    let mission = |id: &str, key: &str, edges: Vec<CampaignEdge>| CampaignNode {
        id: node(id),
        kind: NodeKind::Mission {
            mission: known(
                ContentId::from_source(ContentKind::Mission, key).expect("fixture id is valid"),
            ),
        },
        edges,
        provenance: designed(),
    };
    let edge = |on: EdgeCondition, to: &str, grant: Option<RewardSpec>| CampaignEdge {
        on,
        to: node(to),
        grant,
        provenance: designed(),
    };
    let grant = |currency: u64, unlocks: Vec<ContentId>| {
        Some(RewardSpec {
            currency: known(currency),
            unlocks,
        })
    };

    let unlock_a = ContentId::from_source(ContentKind::Blueprint, "synthetic.devastator")
        .expect("fixture id is valid");
    CampaignDefinition::try_new(CampaignDraft {
        nodes: vec![
            mission(
                "m01",
                "synthetic.m01",
                vec![
                    edge(
                        EdgeCondition::Victory,
                        "m02",
                        grant(500, vec![unlock_a.clone()]),
                    ),
                    edge(EdgeCondition::Defeat, "m01", None),
                ],
            ),
            mission(
                "m02",
                "synthetic.m02",
                vec![
                    edge(EdgeCondition::Victory, "ending", grant(1500, vec![])),
                    edge(EdgeCondition::Defeat, "m02", None),
                ],
            ),
            CampaignNode {
                id: node("ending"),
                kind: NodeKind::Ending,
                edges: vec![],
                provenance: designed(),
            },
        ],
        entry: node("m01"),
        roster: vec![RosterEntry {
            item: unlock_a,
            available_from: node("m02"),
            provenance: designed(),
        }],
        provenance: designed(),
    })
    .expect("the synthetic campaign is a valid graph")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(id: &str) -> ClaimId {
        ClaimId::new(id).expect("valid claim id")
    }

    fn designed() -> Provenance {
        Provenance::designed(claim("f43a.unit"))
    }

    fn known<T>(value: T) -> Resolved<T> {
        Resolved::Known(Known::new(value, designed()))
    }

    fn node(id: &str) -> CampaignNodeId {
        CampaignNodeId::new(id).expect("valid node id")
    }

    fn mission(id: &str, edges: Vec<CampaignEdge>) -> CampaignNode {
        CampaignNode {
            id: node(id),
            kind: NodeKind::Mission {
                mission: known(
                    ContentId::from_source(ContentKind::Mission, "synthetic.m").expect("valid id"),
                ),
            },
            edges,
            provenance: designed(),
        }
    }

    fn edge(on: EdgeCondition, to: &str) -> CampaignEdge {
        CampaignEdge {
            on,
            to: node(to),
            grant: None,
            provenance: designed(),
        }
    }

    /// Graph validation: duplicates, dangling targets, missing entry, edges
    /// out of endings and non-ending dead ends all refuse with named causes.
    #[test]
    fn accept_f43_a_the_definition_validates_its_shape() {
        let ending = || CampaignNode {
            id: node("end"),
            kind: NodeKind::Ending,
            edges: vec![],
            provenance: designed(),
        };
        let draft = |nodes: Vec<CampaignNode>, entry: &str, roster: Vec<RosterEntry>| {
            CampaignDefinition::try_new(CampaignDraft {
                nodes,
                entry: node(entry),
                roster,
                provenance: designed(),
            })
        };

        assert_eq!(draft(vec![], "m", vec![]), Err(CampaignError::Empty));
        let dup = vec![
            mission("m", vec![edge(EdgeCondition::Victory, "end")]),
            mission("m", vec![edge(EdgeCondition::Victory, "end")]),
            ending(),
        ];
        assert!(matches!(
            draft(dup, "m", vec![]),
            Err(CampaignError::DuplicateNode { .. })
        ));
        assert!(matches!(
            draft(
                vec![
                    mission("m", vec![edge(EdgeCondition::Victory, "end")]),
                    ending()
                ],
                "x",
                vec![]
            ),
            Err(CampaignError::MissingEntry { .. })
        ));
        assert!(matches!(
            draft(
                vec![
                    mission("m", vec![edge(EdgeCondition::Victory, "nope")]),
                    ending()
                ],
                "m",
                vec![]
            ),
            Err(CampaignError::DanglingEdge { .. })
        ));
        let mut e = ending();
        e.edges = vec![edge(EdgeCondition::Victory, "m")];
        assert!(matches!(
            draft(
                vec![mission("m", vec![edge(EdgeCondition::Victory, "end")]), e],
                "m",
                vec![]
            ),
            Err(CampaignError::EdgeFromEnding { .. })
        ));
        assert!(matches!(
            draft(vec![mission("m", vec![]), ending()], "m", vec![]),
            Err(CampaignError::DeadEnd { .. })
        ));
        assert!(matches!(
            draft(
                vec![
                    mission("m", vec![edge(EdgeCondition::Victory, "end")]),
                    ending()
                ],
                "m",
                vec![RosterEntry {
                    item: ContentId::from_source(ContentKind::Blueprint, "synthetic.x")
                        .expect("valid"),
                    available_from: node("nope"),
                    provenance: designed(),
                }],
            ),
            Err(CampaignError::UnknownRosterGate { .. })
        ));
        // A node with two Victory edges is ambiguous and refused.
        assert!(matches!(
            draft(
                vec![
                    mission(
                        "m",
                        vec![
                            edge(EdgeCondition::Victory, "end"),
                            edge(EdgeCondition::Victory, "end"),
                        ],
                    ),
                    ending(),
                ],
                "m",
                vec![],
            ),
            Err(CampaignError::DuplicateCondition { .. })
        ));
        // A mission node bound to a non-mission content id is refused —
        // the kind check matches every other declared schema's binding
        // rule (airframe_roles, animation, audio).
        let mut wrong_kind = mission("m", vec![edge(EdgeCondition::Victory, "end")]);
        wrong_kind.kind = NodeKind::Mission {
            mission: known(
                ContentId::from_source(ContentKind::Blueprint, "synthetic.not-a-mission")
                    .expect("valid id"),
            ),
        };
        assert!(matches!(
            draft(vec![wrong_kind, ending()], "m", vec![]),
            Err(CampaignError::MissionBindingKind {
                kind: ContentKind::Blueprint,
                ..
            })
        ));
        // An unsurveyed binding stays legal at this layer — the lowering
        // boundary is where an unknown refuses.
        let unsurveyed = CampaignNode {
            id: node("m"),
            kind: NodeKind::Mission {
                mission: Resolved::unknown(
                    claim("f43a.unit.unsurveyed"),
                    "the original mission id is unsurveyed",
                )
                .expect("a reason is present"),
            },
            edges: vec![edge(EdgeCondition::Victory, "end")],
            provenance: designed(),
        };
        assert!(draft(vec![unsurveyed, ending()], "m", vec![]).is_ok());
    }

    /// The declared fixture is synthetic throughout and structurally sound.
    #[test]
    fn accept_f43_a_the_fixture_is_synthetic_and_valid() {
        let campaign = declared_synthetic_campaign();
        assert_eq!(campaign.entry().as_str(), "m01");
        assert_eq!(campaign.nodes().count(), 3);
        for node in campaign.nodes() {
            assert_eq!(
                node.provenance.class,
                cs_types::evidence::ClaimStatus::Designed
            );
            for edge in &node.edges {
                assert_eq!(
                    edge.provenance.class,
                    cs_types::evidence::ClaimStatus::Designed
                );
            }
        }
        let m01 = campaign.node(&node("m01")).expect("declared");
        assert!(matches!(m01.kind, NodeKind::Mission { .. }));
        assert_eq!(campaign.roster().len(), 1);
        assert_eq!(campaign.roster()[0].available_from.as_str(), "m02");
    }
}
