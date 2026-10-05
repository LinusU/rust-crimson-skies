//! The campaign boundary (F43-A): lower the declared
//! `cs_content::campaign` graph into the runtime `cs_sim::campaign` graph,
//! refusing every mandatory field that is still an explicit unknown.
//!
//! Spec: `specs/F43-campaign-progression-outcomes-and-economy-rules.md`,
//! stage `### F43-A`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! The lowering rule is the boundary contract used across the crate (see
//! `crate::audio`): a [`Resolved::Unknown`] on a mandatory field — a mission
//! node with no catalog binding, a reward with no measured amount — refuses
//! here, where a session can still decline the record, instead of paying or
//! routing on a guess. Interlude assets are *optional* data: an unbound
//! interlude lowers to a beat that plays nothing rather than refusing the
//! campaign.
//!
//! [`Resolved::Unknown`]: cs_types::content::Resolved::Unknown

use std::fmt;

use cs_content::campaign::{
    CampaignDefinition, CampaignNode, CampaignNodeId, EdgeCondition, NodeKind,
};
use cs_content::construction::{AircraftBlueprint, ConstructionRules, PriceBook};
use cs_sim::campaign::{
    CampaignGraph, CampaignNodeKey, GraphError, LoadoutWeight, Outcome, Reward, RosterGate,
    RuntimeEdge, RuntimeNode, RuntimeNodeKind,
};
use cs_types::content::Resolved;

/// Why a declared campaign record refused to lower.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CampaignLowerError {
    /// A mission node's catalog binding is unknown — the node cannot say
    /// which mission it is.
    UnknownMissionBinding {
        /// The node.
        node: CampaignNodeId,
        /// The recorded reason.
        reason: String,
    },
    /// An edge's reward amount is unknown — the transaction would pay a
    /// guess (spec F43 non-negotiable behavior 4).
    UnknownReward {
        /// The edge's source node.
        node: CampaignNodeId,
        /// The edge's condition.
        on: EdgeCondition,
        /// The recorded reason.
        reason: String,
    },
    /// A node id failed runtime-key validation — the lowered key must be a
    /// faithful copy, not a re-parse.
    BadNodeKey {
        /// The rejected id.
        node: CampaignNodeId,
    },
    /// The lowered graph failed its own validation — the runtime invariants
    /// match the declared ones, so this names a lowering defect, not a bad
    /// record.
    Graph {
        /// The runtime refusal.
        source: GraphError,
    },
}

impl fmt::Display for CampaignLowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownMissionBinding { node, reason } => {
                write!(f, "campaign node {node} has no mission binding: {reason}")
            }
            Self::UnknownReward { node, on, reason } => write!(
                f,
                "the {} edge of node {node} has no measured reward: {reason}",
                on.label()
            ),
            Self::BadNodeKey { node } => {
                write!(f, "node id {node} cannot be a runtime key")
            }
            Self::Graph { source } => write!(f, "lowered graph invalid: {source}"),
        }
    }
}

impl std::error::Error for CampaignLowerError {}

/// The declared condition vocabulary maps one-to-one onto the mission
/// runtime's terminal outcomes.
fn lower_condition(on: EdgeCondition) -> Outcome {
    match on {
        EdgeCondition::Victory => Outcome::Succeeded,
        EdgeCondition::Defeat => Outcome::Failed,
        EdgeCondition::Abort => Outcome::Aborted,
    }
}

fn lower_key(id: &CampaignNodeId) -> Result<CampaignNodeKey, CampaignLowerError> {
    CampaignNodeKey::new(id.as_str())
        .map_err(|_| CampaignLowerError::BadNodeKey { node: id.clone() })
}

fn lower_node(node: &CampaignNode) -> Result<RuntimeNode, CampaignLowerError> {
    let kind = match &node.kind {
        NodeKind::Mission { mission } => {
            let mission = match mission {
                Resolved::Known(known) => known.value.clone(),
                Resolved::Unknown { reason, .. } => {
                    return Err(CampaignLowerError::UnknownMissionBinding {
                        node: node.id.clone(),
                        reason: reason.clone(),
                    });
                }
            };
            RuntimeNodeKind::Mission { mission }
        }
        // An unbound interlude asset is optional content, not a refusal.
        NodeKind::Interlude { .. } => RuntimeNodeKind::Interlude,
        NodeKind::Ending => RuntimeNodeKind::Ending,
    };
    let mut edges = Vec::with_capacity(node.edges.len());
    for edge in &node.edges {
        let grant = match &edge.grant {
            Some(spec) => {
                let currency = match &spec.currency {
                    Resolved::Known(known) => known.value,
                    Resolved::Unknown { reason, .. } => {
                        return Err(CampaignLowerError::UnknownReward {
                            node: node.id.clone(),
                            on: edge.on,
                            reason: reason.clone(),
                        });
                    }
                };
                Reward {
                    currency,
                    unlocks: spec.unlocks.clone(),
                }
            }
            None => Reward::default(),
        };
        edges.push(RuntimeEdge {
            on: lower_condition(edge.on),
            to: lower_key(&edge.to)?,
            grant,
        });
    }
    Ok(RuntimeNode {
        id: lower_key(&node.id)?,
        kind,
        edges,
    })
}

/// Resolves the weight verdict input of a purchase draft: the exact mass of
/// `loadout` (the blueprint the purchase would produce) against the ceiling of
/// `rules`, both read from the F44-A declared records.
///
/// `cs_sim` cannot depend on `cs_content`, so the campaign transaction is
/// handed plain integers (`cs_sim::campaign::LoadoutWeight`) and this boundary
/// is where the content types are read. Any unknown — an unmeasured limit or
/// component mass, an unpriced component, a mismatched airframe or an
/// overflowing total — becomes [`LoadoutWeight::Unknown`] carrying the named
/// budget refusal, so it can never pass as a loadout that fits. The numbers
/// are only as good as the records: no original mass or ceiling is measured
/// yet (F44-D).
#[must_use]
pub fn loadout_weight(
    rules: &ConstructionRules,
    book: &PriceBook,
    loadout: &AircraftBlueprint,
) -> LoadoutWeight {
    match rules.assess(loadout, book) {
        Ok(assessment) => match rules.max_mass().clone().known() {
            Some(limit) => LoadoutWeight::Measured {
                total: assessment.totals().mass().as_units(),
                limit: limit.as_units(),
            },
            // `assess` refuses an unknown limit, so this is unreachable in
            // practice; it is still answered as unknown rather than assumed.
            None => LoadoutWeight::Unknown {
                reason: "the weight ceiling is unmeasured".to_owned(),
            },
        },
        Err(refusal) => LoadoutWeight::Unknown {
            reason: refusal.to_string(),
        },
    }
}

/// Lowers a declared campaign into the validated runtime graph. The first
/// refusal wins; a half-lowered graph is never produced.
///
/// # Errors
///
/// [`CampaignLowerError`].
pub fn lower_campaign(declared: &CampaignDefinition) -> Result<CampaignGraph, CampaignLowerError> {
    let mut nodes = Vec::with_capacity(declared.nodes().count());
    for node in declared.nodes() {
        nodes.push(lower_node(node)?);
    }
    let entry = lower_key(declared.entry())?;
    let mut roster = Vec::with_capacity(declared.roster().len());
    for gate in declared.roster() {
        roster.push(RosterGate {
            item: gate.item.clone(),
            gate: lower_key(&gate.available_from)?,
        });
    }
    CampaignGraph::try_new(nodes, entry, roster)
        .map_err(|source| CampaignLowerError::Graph { source })
}
