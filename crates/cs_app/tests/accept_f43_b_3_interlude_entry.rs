//! Acceptance scenarios for F43-B.3: may a campaign begin on an interlude?
//!
//! Task test prefix: `accept_f43_b_3_`.
//!
//! **The rule is designed engine semantics, not measured.** No original
//! campaign data was read; nothing measures whether the original opens on a
//! narrative beat. A campaign's entry must be a node that can report an
//! outcome, so both the declared schema and the runtime graph refuse a beat
//! entry with a named error, and `CampaignState::begin` never needs a manual
//! `advance_interludes` call to become playable. Every value is synthetic.

use cs_app::campaign::lower_campaign;
use cs_content::campaign::{
    CampaignDefinition, CampaignDraft, CampaignEdge, CampaignError, CampaignNode, CampaignNodeId,
    EdgeCondition, NodeKind,
};
use cs_sim::campaign::{
    CampaignGraph, CampaignNodeKey, CampaignState, DifficultyId, GraphError, Outcome, ProfileId,
    Reward, RuntimeEdge, RuntimeNode, RuntimeNodeKind,
};
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;

fn designed() -> Provenance {
    Provenance::designed(ClaimId::new("f43b3.interlude_entry").expect("valid"))
}

fn node(id: &str) -> CampaignNodeId {
    CampaignNodeId::new(id).expect("valid node id")
}

fn key(id: &str) -> CampaignNodeKey {
    CampaignNodeKey::new(id).expect("valid node key")
}

/// `beat -> m01 -> end`, entering at `entry`.
fn declared(entry: &str) -> CampaignDraft {
    let mission = ContentId::from_source(ContentKind::Mission, "m01").expect("valid");
    let edge = |on, to: &str| CampaignEdge {
        on,
        to: node(to),
        grant: None,
        provenance: designed(),
    };
    let n = |id: &str, kind, edges| CampaignNode {
        id: node(id),
        kind,
        edges,
        provenance: designed(),
    };
    CampaignDraft {
        nodes: vec![
            n(
                "beat",
                NodeKind::Interlude {
                    asset: Resolved::Unknown {
                        claim_id: ClaimId::new("f43b3.asset").expect("valid"),
                        reason: "unsurveyed".to_owned(),
                    },
                },
                vec![edge(EdgeCondition::Victory, "m01")],
            ),
            n(
                "m01",
                NodeKind::Mission {
                    mission: Resolved::Known(Known::new(mission, designed())),
                },
                vec![edge(EdgeCondition::Victory, "end")],
            ),
            n("end", NodeKind::Ending, vec![]),
        ],
        entry: node(entry),
        roster: Vec::new(),
        provenance: designed(),
    }
}

fn runtime_nodes() -> Vec<RuntimeNode> {
    let mission = ContentId::from_source(ContentKind::Mission, "m01").expect("valid");
    let edge = |to: &str| RuntimeEdge {
        on: Outcome::Succeeded,
        to: key(to),
        grant: Reward::default(),
    };
    vec![
        RuntimeNode {
            id: key("beat"),
            kind: RuntimeNodeKind::Interlude,
            edges: vec![edge("m01")],
        },
        RuntimeNode {
            id: key("m01"),
            kind: RuntimeNodeKind::Mission { mission },
            edges: vec![edge("end")],
        },
        RuntimeNode {
            id: key("end"),
            kind: RuntimeNodeKind::Ending,
            edges: vec![],
        },
    ]
}

#[test]
fn accept_f43_b_3_declared_campaign_refuses_an_interlude_entry() {
    assert_eq!(
        CampaignDefinition::try_new(declared("beat")).unwrap_err(),
        CampaignError::InterludeEntry { node: node("beat") }
    );
}

#[test]
fn accept_f43_b_3_declared_campaign_accepts_a_mission_entry_past_a_beat() {
    let campaign = CampaignDefinition::try_new(declared("m01")).expect("mission entry is legal");
    let graph = lower_campaign(&campaign).expect("lowers");
    let state = CampaignState::begin(
        ProfileId::new("pilot.nathan").expect("valid"),
        cs_sim::campaign::CampaignRunId::new("run.one").expect("valid"),
        DifficultyId::new("normal").expect("valid"),
        &graph,
    );
    assert_eq!(state.current(), &key("m01"));
}

#[test]
fn accept_f43_b_3_runtime_graph_refuses_an_interlude_entry() {
    assert_eq!(
        CampaignGraph::try_new(runtime_nodes(), key("beat"), Vec::new()).unwrap_err(),
        GraphError::InterludeEntry { node: key("beat") }
    );
    assert!(CampaignGraph::try_new(runtime_nodes(), key("m01"), Vec::new()).is_ok());
}
