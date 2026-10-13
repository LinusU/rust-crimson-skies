//! F43-V1 — Declaration-time structural validation for campaign graphs.
//!
//! F43-B kept the campaign *walk* safe in the face of malformed declared
//! graphs by refusing them dynamically — an interlude chain that dead-ends or
//! loops gets `InterludeDeadEnd` / `InterludeLoop` instead of a hung run. But
//! three structural defects could sit in a declared campaign forever, and none
//! of them is a runtime decision: nothing ever walks an unreachable node, a
//! campaign whose victory route can never reach an `Ending` can never be
//! finished, and a victory-edge cycle is an authoring mistake an importer is
//! most likely to emit. F43-V1 makes `CampaignDefinition::try_new` — the
//! declaration boundary every importer must pass — refuse all three, while the
//! runtime graph deliberately keeps admitting traversal shapes: refusing to
//! walk them is the runtime's own answer, and refusing to *declare* them here
//! must not remove the vocabulary those refusals are tested against. These are
//! engineered policies, not original-game rules; the game's real campaign
//! cannot be declared yet and its cycle tolerance is unmeasured.
//!
//! * **Reachable or refused.** `try_new` computes the reach set over every
//!   declared edge (any `EdgeCondition`) from the entry; a node outside it can
//!   never be selected, crossed, or gated on, so it is
//!   `CampaignError::UnreachableNode`, not a warning — silent dead weight is
//!   exactly what an importer emits when its offset math is wrong.
//! * **Victory must route to an ending.** Every non-`Ending` node — missions
//!   *and* beats — must declare a `Victory` edge, and the directed graph of
//!   only those edges must be acyclic. Following `Victory` from the entry then
//!   necessarily reaches an `Ending` — which is what "the campaign can be
//!   won" means. A victory-only interlude chain that loops (`beat_a ⇄ beat_b`)
//!   is `CampaignError::VictoryCycle`, not a hang waiting to happen.
//! * **Alternatives and replays stay legal.** `Defeat`/`Abort` branches are
//!   not part of the finishability route — a mission that defeats back to an
//!   earlier node is a retry, and retrying is how missions are replayed. They
//!   still count toward reachability, so a defeat-only branch that exists is
//!   reachable, but one that is not is refused as unreachable like anything
//!   else.
//! * **The runtime graph stays permissive.** `CampaignGraph` mirrors only the
//!   reachability half of the new rules — an unreachable node is a defect at
//!   any layer. Victory cycles and victory-free nodes it admits, because
//!   `advance_interludes`'s `InterludeLoop`/`InterludeDeadEnd` refusals are the
//!   tested dynamic answers to those shapes, and stripping the vocabulary that
//!   holds them would strip the evidence the walk refuses them.

use cs_app::campaign::lower_campaign;
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
    Provenance::designed(ClaimId::new("f43v1.fixture").expect("valid"))
}

fn node(id: &str) -> CampaignNodeId {
    CampaignNodeId::new(id).expect("valid node id")
}

fn key(id: &str) -> CampaignNodeKey {
    CampaignNodeKey::new(id).expect("valid node key")
}

fn mission_content(id: &str) -> ContentId {
    ContentId::from_source(ContentKind::Mission, id).expect("valid content id")
}

fn edge(on: EdgeCondition, to: &str) -> CampaignEdge {
    CampaignEdge {
        on,
        to: node(to),
        grant: None,
        provenance: designed(),
    }
}

fn mission(id: &str, edges: Vec<CampaignEdge>) -> CampaignNode {
    CampaignNode {
        id: node(id),
        kind: NodeKind::Mission {
            mission: Resolved::Known(Known::new(mission_content(id), designed())),
        },
        edges,
        provenance: designed(),
    }
}

fn beat(id: &str, edges: Vec<CampaignEdge>) -> CampaignNode {
    CampaignNode {
        id: node(id),
        kind: NodeKind::Interlude {
            asset: Resolved::Unknown {
                claim_id: ClaimId::new("f43v1.beat_asset").expect("valid claim"),
                reason: "unsurveyed".to_owned(),
            },
        },
        edges,
        provenance: designed(),
    }
}

fn ending(id: &str) -> CampaignNode {
    CampaignNode {
        id: node(id),
        kind: NodeKind::Ending,
        edges: Vec::new(),
        provenance: designed(),
    }
}

fn draft(nodes: Vec<CampaignNode>) -> CampaignDraft {
    CampaignDraft {
        nodes,
        entry: node("m01"),
        roster: Vec::new(),
        provenance: designed(),
    }
}

fn runtime_edge(on: Outcome, to: &str) -> RuntimeEdge {
    RuntimeEdge {
        on,
        to: key(to),
        grant: Reward::default(),
    }
}

fn runtime_beat(id: &str, edges: Vec<RuntimeEdge>) -> RuntimeNode {
    RuntimeNode {
        id: key(id),
        kind: RuntimeNodeKind::Interlude,
        edges,
    }
}

// ---------------------------------------------------------------------------
// Rejections at the declared boundary.
// ---------------------------------------------------------------------------

/// The canonical case F43-B could only refuse dynamically: a beat chain that
/// loops back on itself. Declared through the schema, it is now refused
/// outright — and the error names the beat the chain came back to, matching
/// the runtime refusal's contract.
#[test]
fn accept_f43_v1_a_victory_beat_cycle_is_refused() {
    let err = CampaignDefinition::try_new(draft(vec![
        mission("m01", vec![edge(EdgeCondition::Victory, "beat_a")]),
        beat("beat_a", vec![edge(EdgeCondition::Victory, "beat_b")]),
        // The defeat route keeps `end` inside the reach set without giving
        // the victory path an exit — the refusal under test is the cycle.
        beat(
            "beat_b",
            vec![
                edge(EdgeCondition::Victory, "beat_a"),
                edge(EdgeCondition::Defeat, "end"),
            ],
        ),
        ending("end"),
    ]))
    .expect_err("a victory-edge cycle is not a finishable campaign");
    assert_eq!(
        err,
        CampaignError::VictoryCycle {
            node: node("beat_a")
        }
    );
    assert!(
        err.to_string().contains("beat_a"),
        "the display names the beat the walk came back to"
    );
}

/// A cycle of *missions* is equally unfinishable — the route never leaves the
/// loop — and equally refused.
#[test]
fn accept_f43_v1_a_victory_mission_cycle_is_refused() {
    let err = CampaignDefinition::try_new(draft(vec![
        mission(
            "m01",
            vec![
                edge(EdgeCondition::Victory, "m02"),
                // Keeps `end` reachable without giving victory an exit.
                edge(EdgeCondition::Defeat, "end"),
            ],
        ),
        mission("m02", vec![edge(EdgeCondition::Victory, "m01")]),
        ending("end"),
    ]))
    .expect_err("a victory-edge cycle of missions is not finishable");
    assert!(
        matches!(err, CampaignError::VictoryCycle { .. }),
        "expected VictoryCycle, got {err}"
    );
}

/// A beat that declares only a `Defeat` edge can never be walked *past* — the
/// dead end F43-B refused dynamically is refused at declaration now.
#[test]
fn accept_f43_v1_a_beat_with_no_victory_edge_is_refused() {
    let err = CampaignDefinition::try_new(draft(vec![
        mission(
            "m01",
            vec![
                edge(EdgeCondition::Victory, "beat"),
                // Keeps `end` reachable; the beat's missing victory edge is
                // the refusal under test.
                edge(EdgeCondition::Defeat, "end"),
            ],
        ),
        beat("beat", vec![edge(EdgeCondition::Defeat, "beat")]),
        ending("end"),
    ]))
    .expect_err("a beat with no victory edge dead-ends the campaign");
    assert_eq!(err, CampaignError::Unfinishable { node: node("beat") });
    assert!(
        err.to_string().contains("beat"),
        "the display names the node that cannot be walked past"
    );
}

/// A mission with no `Victory` edge is refused by the same rule — it cannot
/// hand control onward on a win.
#[test]
fn accept_f43_v1_a_mission_with_no_victory_edge_is_refused() {
    let err = CampaignDefinition::try_new(draft(vec![
        mission("m01", vec![edge(EdgeCondition::Defeat, "end")]),
        ending("end"),
    ]))
    .expect_err("a mission with no victory edge dead-ends the campaign");
    assert_eq!(err, CampaignError::Unfinishable { node: node("m01") });
}

/// A declared node that no edge path from the entry reaches is refused — a
/// warning would leave silent dead weight an importer almost certainly did
/// not mean to write.
#[test]
fn accept_f43_v1_an_unreachable_node_is_refused() {
    let err = CampaignDefinition::try_new(draft(vec![
        mission("m01", vec![edge(EdgeCondition::Victory, "end")]),
        mission("m02", vec![edge(EdgeCondition::Victory, "end")]),
        ending("end"),
    ]))
    .expect_err("a node outside the entry's reach set is refused");
    assert_eq!(err, CampaignError::UnreachableNode { node: node("m02") });
    assert!(
        err.to_string().contains("m02"),
        "the display names the stranded node"
    );
}

/// Reachability is computed over *every* declared edge condition — a node
/// held only by a `Defeat` branch is still reachable and must not be refused
/// as stranded. Here `beat` is reachable only through `m01`'s defeat, and
/// every victory route still ends at `end`.
#[test]
fn accept_f43_v1_a_defeat_only_branch_still_counts_as_reachable() {
    let declared = CampaignDefinition::try_new(draft(vec![
        mission(
            "m01",
            vec![
                edge(EdgeCondition::Victory, "end"),
                edge(EdgeCondition::Defeat, "beat"),
            ],
        ),
        beat("beat", vec![edge(EdgeCondition::Victory, "end")]),
        ending("end"),
    ]))
    .expect("a defeat-only route is a route");
    let graph = lower_campaign(&declared).expect("it lowers");
    assert!(graph.node(&key("beat")).is_some());
}

/// A mission whose *defeat* routes back to itself is a retry — replays are
/// how missions are replayed — and retries must not trip the victory-cycle
/// check, which walks `Victory` edges only.
#[test]
fn accept_f43_v1_a_defeat_self_loop_is_a_replay_not_a_cycle() {
    let declared = CampaignDefinition::try_new(draft(vec![
        mission(
            "m01",
            vec![
                edge(EdgeCondition::Victory, "end"),
                edge(EdgeCondition::Defeat, "m01"),
            ],
        ),
        ending("end"),
    ]))
    .expect("a defeat self-loop is a retry, not a victory cycle");
    let graph = lower_campaign(&declared).expect("it lowers");
    assert!(graph.node(&key("m01")).is_some());
}

/// A mission whose *victory* routes back to itself is a cycle the victory
/// path can never leave: a run that keeps winning replays the node forever
/// and no victory edge reaches an ending. A defeat or abort self-loop is a
/// retry, but a victory self-loop is refused.
#[test]
fn accept_f43_v1_a_victory_self_loop_is_still_a_cycle() {
    let err = CampaignDefinition::try_new(draft(vec![
        mission(
            "m01",
            vec![
                edge(EdgeCondition::Victory, "m01"),
                // Keeps `end` inside the entry's reach set, so the refusal
                // under test is the cycle, not an unreachable ending.
                edge(EdgeCondition::Defeat, "end"),
            ],
        ),
        ending("end"),
    ]))
    .expect_err("a victory self-loop is a cycle: victory never leaves the node");
    assert_eq!(err, CampaignError::VictoryCycle { node: node("m01") });
}

// ---------------------------------------------------------------------------
// The runtime graph mirrors only what runtime must enforce.
// ---------------------------------------------------------------------------

/// Reachability is a defect at any layer: an unreachable runtime node is
/// refused by `CampaignGraph` too, and names the stranded node.
#[test]
fn accept_f43_v1_a_runtime_graph_refuses_an_unreachable_node() {
    let mission = mission_content("m01");
    let err = CampaignGraph::try_new(
        vec![
            RuntimeNode {
                id: key("m01"),
                kind: RuntimeNodeKind::Mission { mission },
                edges: vec![runtime_edge(Outcome::Succeeded, "end")],
            },
            runtime_beat("beat", vec![runtime_edge(Outcome::Succeeded, "end")]),
            RuntimeNode {
                id: key("end"),
                kind: RuntimeNodeKind::Ending,
                edges: vec![],
            },
        ],
        key("m01"),
        Vec::new(),
    )
    .expect_err("the runtime layer refuses a node outside the entry's reach");
    assert_eq!(err, GraphError::UnreachableNode { node: key("beat") });
}

/// A runtime graph may still *hold* a victory loop and a victory-free beat:
/// `advance_interludes`'s refusals are the dynamic answers to those shapes,
/// and keeping the vocabulary that constructs them keeps that refusal
/// testable — which is the deliberate asymmetry F43-V1 introduces between the
/// two layers.
#[test]
fn accept_f43_v1_a_runtime_graph_still_holds_traversal_shapes() {
    // Victory-free beat: the declared layer refuses this as `Unfinishable`.
    let dead_end = CampaignGraph::try_new(
        vec![
            RuntimeNode {
                id: key("m01"),
                kind: RuntimeNodeKind::Mission {
                    mission: mission_content("m01"),
                },
                edges: vec![runtime_edge(Outcome::Succeeded, "beat")],
            },
            runtime_beat("beat", vec![runtime_edge(Outcome::Failed, "beat")]),
        ],
        key("m01"),
        Vec::new(),
    );
    assert!(
        dead_end.is_ok(),
        "a runtime beat with no success edge is held for the walk to refuse"
    );

    // Victory loop: the declared layer refuses this as `VictoryCycle`.
    let cycle = CampaignGraph::try_new(
        vec![
            RuntimeNode {
                id: key("m01"),
                kind: RuntimeNodeKind::Mission {
                    mission: mission_content("m01"),
                },
                edges: vec![runtime_edge(Outcome::Succeeded, "beat_a")],
            },
            runtime_beat("beat_a", vec![runtime_edge(Outcome::Succeeded, "beat_b")]),
            runtime_beat("beat_b", vec![runtime_edge(Outcome::Succeeded, "beat_a")]),
        ],
        key("m01"),
        Vec::new(),
    );
    assert!(
        cycle.is_ok(),
        "a runtime beat cycle is held for the walk to refuse"
    );
}
