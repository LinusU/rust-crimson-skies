//! Acceptance scenarios for F43-V2: may a campaign begin on an ending?
//!
//! Task test prefix: `accept_f43_v2_`.
//!
//! **The rule is designed engine semantics, not measured.** No original
//! campaign data was read. An `Ending` cannot report an outcome, so a run
//! parked on one is finished at tick 0; both the declared schema and the
//! runtime graph refuse such an entry with a named error. Every value is
//! synthetic.

use cs_content::campaign::{
    CampaignDefinition, CampaignDraft, CampaignEdge, CampaignError, CampaignNode, CampaignNodeId,
    EdgeCondition, NodeKind,
};
use cs_sim::campaign::{
    CampaignGraph, CampaignNodeKey, GraphError, Outcome, Reward, RuntimeEdge, RuntimeNode,
    RuntimeNodeKind,
};
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;

fn designed() -> Provenance {
    Provenance::designed(ClaimId::new("f43v2.ending_entry").expect("valid"))
}

fn node(id: &str) -> CampaignNodeId {
    CampaignNodeId::new(id).expect("valid node id")
}

fn key(id: &str) -> CampaignNodeKey {
    CampaignNodeKey::new(id).expect("valid node key")
}

/// `m01 -> end`, entering at `entry`; or just `{end}` when `lone`.
fn declared(entry: &str, lone: bool) -> CampaignDraft {
    let mission = ContentId::from_source(ContentKind::Mission, "m01").expect("valid");
    let n = |id: &str, kind, edges| CampaignNode {
        id: node(id),
        kind,
        edges,
        provenance: designed(),
    };
    let mut nodes = vec![n("end", NodeKind::Ending, vec![])];
    if !lone {
        nodes.push(n(
            "m01",
            NodeKind::Mission {
                mission: Resolved::Known(Known::new(mission, designed())),
            },
            vec![CampaignEdge {
                on: EdgeCondition::Victory,
                to: node("end"),
                grant: None,
                provenance: designed(),
            }],
        ));
    }
    CampaignDraft {
        nodes,
        entry: node(entry),
        roster: Vec::new(),
        provenance: designed(),
    }
}

fn runtime_nodes(lone: bool) -> Vec<RuntimeNode> {
    let mission = ContentId::from_source(ContentKind::Mission, "m01").expect("valid");
    let mut nodes = vec![RuntimeNode {
        id: key("end"),
        kind: RuntimeNodeKind::Ending,
        edges: vec![],
    }];
    if !lone {
        nodes.push(RuntimeNode {
            id: key("m01"),
            kind: RuntimeNodeKind::Mission { mission },
            edges: vec![RuntimeEdge {
                on: Outcome::Succeeded,
                to: key("end"),
                grant: Reward::default(),
            }],
        });
    }
    nodes
}

#[test]
fn accept_f43_v2_declared_campaign_refuses_an_ending_entry() {
    assert_eq!(
        CampaignDefinition::try_new(declared("end", false)).unwrap_err(),
        CampaignError::EndingEntry { node: node("end") }
    );
    assert!(CampaignDefinition::try_new(declared("m01", false)).is_ok());
}

#[test]
fn accept_f43_v2_declared_all_endings_draft_is_not_a_legal_empty_campaign() {
    assert_eq!(
        CampaignDefinition::try_new(declared("end", true)).unwrap_err(),
        CampaignError::EndingEntry { node: node("end") }
    );
}

#[test]
fn accept_f43_v2_runtime_graph_refuses_an_ending_entry() {
    assert_eq!(
        CampaignGraph::try_new(runtime_nodes(false), key("end"), Vec::new()).unwrap_err(),
        GraphError::EndingEntry { node: key("end") }
    );
    assert_eq!(
        CampaignGraph::try_new(runtime_nodes(true), key("end"), Vec::new()).unwrap_err(),
        GraphError::EndingEntry { node: key("end") }
    );
    assert!(CampaignGraph::try_new(runtime_nodes(false), key("m01"), Vec::new()).is_ok());
}
