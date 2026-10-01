//! Acceptance scenarios for F43-A: campaign graph and the transactional
//! outcome schema.
//!
//! Spec: `specs/F43-campaign-progression-outcomes-and-economy-rules.md`,
//! stage `### F43-A`. Task test prefix: `accept_f43_a_`.
//!
//! The minimum scenario — **AC01: "Apply the same success outcome twice;
//! progression and currency change once"** — is
//! `accept_f43_a_the_same_outcome_applies_once`, run on the production path
//! (`declared_synthetic_campaign` → `lower_campaign` →
//! `CampaignState::apply_outcome`). The rest of the file covers the stage's
//! contracts: replay records latest without moving progression, foreign and
//! ineligible outcomes refuse, lowering refuses unknown bindings and
//! unmeasured rewards, and the ending is reached through the declared graph
//! alone.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use cs_app::campaign::{CampaignLowerError, lower_campaign};
use cs_content::campaign::CampaignError as DeclaredError;
use cs_content::campaign::{
    CampaignDefinition, CampaignDraft, CampaignEdge, CampaignNode, CampaignNodeId, EdgeCondition,
    NodeKind, RewardSpec, declared_synthetic_campaign,
};
use cs_sim::campaign::{
    CampaignError, CampaignNodeKey, CampaignRunId, CampaignState, DifficultyId, MissionOutcome,
    OutcomeAuthority, OutcomeId, OutcomeReceipt, ProfileId,
};
use cs_sim::campaign::{
    CampaignGraph, GraphError, Reward, RuntimeEdge, RuntimeNode, RuntimeNodeKind,
};
use cs_sim::campaign::{EventKey, SessionGeneration};
use cs_sim::campaign::{Outcome, SymbolId};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("valid claim id")
}

fn designed() -> Provenance {
    Provenance::designed(claim("f43a.boundary"))
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, designed()))
}

fn node(id: &str) -> CampaignNodeId {
    CampaignNodeId::new(id).expect("valid node id")
}

fn key(id: &str) -> CampaignNodeKey {
    CampaignNodeKey::new(id).expect("valid node key")
}

fn event(session: u32, tick: u64, sequence: u32) -> EventKey {
    EventKey {
        session: SessionGeneration(session),
        tick: Tick(tick),
        source: SymbolId(7),
        sequence,
    }
}

fn outcome(
    session: u32,
    tick: u64,
    sequence: u32,
    node: &str,
    kind: Outcome,
    score: u64,
) -> MissionOutcome {
    MissionOutcome {
        id: OutcomeId {
            profile: ProfileId::new("pilot.nathan").expect("valid"),
            run: CampaignRunId::new("run.one").expect("valid"),
            session: SessionGeneration(session),
            terminal_event: event(session, tick, sequence),
        },
        node: key(node),
        outcome: kind,
        score,
        authority: OutcomeAuthority::Authorized,
    }
}

fn fresh_run() -> (cs_sim::campaign::CampaignGraph, CampaignState) {
    let graph = lower_campaign(&declared_synthetic_campaign()).expect("the fixture lowers");
    let state = CampaignState::begin(
        ProfileId::new("pilot.nathan").expect("valid"),
        CampaignRunId::new("run.one").expect("valid"),
        DifficultyId::new("standard").expect("valid"),
        &graph,
    );
    (graph, state)
}

/// **AC01 — the stage's minimum scenario.** The same success outcome applied
/// twice changes progression and currency exactly once: the second
/// application is a ledger hit, not a second grant.
#[test]
fn accept_f43_a_the_same_outcome_applies_once() {
    let (graph, mut state) = fresh_run();
    let victory = outcome(1, 480, 0, "m01", Outcome::Succeeded, 900);

    let first = state
        .apply_outcome(&graph, &victory)
        .expect("the first application commits");
    let OutcomeReceipt::Applied(plan) = &first.receipt else {
        panic!("the first application must apply, got {first:?}")
    };
    assert_eq!(plan.currency_delta, 500);
    assert_eq!(plan.progress_to, Some(key("m02")));
    assert_eq!(plan.unlocks.len(), 1);
    assert_eq!(state.currency(), 500);
    assert_eq!(state.current(), &key("m02"));
    let revision_after_first = state.revision();

    // The identical packet — same OutcomeId — is a no-op.
    let second = state
        .apply_outcome(&graph, &victory)
        .expect("a replayed packet is still a valid call");
    assert_eq!(second.receipt, OutcomeReceipt::AlreadyApplied);
    assert_eq!(state.currency(), 500, "a replayed packet cannot pay twice");
    assert_eq!(state.current(), &key("m02"));
    assert_eq!(state.revision(), revision_after_first);
    assert_eq!(state.progress(&key("m01")).expect("recorded").victories, 1);
}

/// AC02's mechanics on the stage-A path: a replayed victory on a completed
/// node records `latest`, keeps `best` and moves nothing — the selected next
/// mission and its unclaimed grant are untouched.
#[test]
fn accept_f43_a_replay_records_latest_without_moving_progression() {
    let (graph, mut state) = fresh_run();
    state
        .apply_outcome(&graph, &outcome(1, 480, 0, "m01", Outcome::Succeeded, 900))
        .expect("victory at m01");
    assert_eq!(state.current(), &key("m02"));

    // Replay m01 in a later session with a *worse* score.
    let replay = outcome(2, 300, 0, "m01", Outcome::Succeeded, 400);
    let receipt = state.apply_outcome(&graph, &replay).expect("replays apply");
    let OutcomeReceipt::Applied(plan) = &receipt.receipt else {
        panic!("a new outcome id applies, got {receipt:?}")
    };
    let progress = state.progress(&key("m01")).expect("recorded");
    assert_eq!(progress.victories, 2);
    assert_eq!(progress.best_score, 900, "a worse replay keeps the best");
    assert_eq!(progress.latest, Some((Outcome::Succeeded, 400)));
    assert!(
        plan.progress_to.is_none(),
        "a replay never moves progression"
    );
    assert_eq!(plan.currency_delta, 0, "a replayed victory cannot re-pay");
    assert!(plan.unlocks.is_empty());
    assert_eq!(state.currency(), 500);
    assert_eq!(state.current(), &key("m02"));
}

/// A better replay raises `best` while still not paying twice or moving the
/// selection.
#[test]
fn accept_f43_a_a_better_replay_raises_best_only() {
    let (graph, mut state) = fresh_run();
    state
        .apply_outcome(&graph, &outcome(1, 480, 0, "m01", Outcome::Succeeded, 900))
        .expect("victory");
    let receipt = state
        .apply_outcome(&graph, &outcome(2, 300, 0, "m01", Outcome::Succeeded, 1200))
        .expect("replay applies");
    match receipt.receipt {
        OutcomeReceipt::Applied(plan) => {
            assert!(plan.best_score_raised);
            assert_eq!(plan.currency_delta, 0);
            assert!(plan.progress_to.is_none());
        }
        other => panic!("expected an applied plan, got {other:?}"),
    }
    assert_eq!(
        state.progress(&key("m01")).expect("recorded").best_score,
        1200
    );
}

/// Defeat records the attempt and follows the *declared* edge — the fixture
/// declares a self-loop (retry in place); nothing invents a skip rule.
#[test]
fn accept_f43_a_defeat_follows_the_declared_edge_and_pays_nothing() {
    let (graph, mut state) = fresh_run();
    let receipt = state
        .apply_outcome(&graph, &outcome(1, 480, 0, "m01", Outcome::Failed, 0))
        .expect("a defeat is a legitimate outcome");
    match receipt.receipt {
        OutcomeReceipt::Applied(plan) => {
            assert_eq!(
                plan.progress_to,
                Some(key("m01")),
                "the declared defeat edge retries in place"
            );
            assert_eq!(plan.currency_delta, 0);
        }
        other => panic!("expected an applied plan, got {other:?}"),
    }
    let progress = state.progress(&key("m01")).expect("recorded");
    assert_eq!(progress.defeats, 1);
    assert_eq!(progress.victories, 0);
    // A defeat does not complete the node for the roster gate, and does not
    // burn the victory grant: the following victory still pays in full.
    assert!(!state.has_completed(&key("m01")));
    state
        .apply_outcome(&graph, &outcome(2, 100, 0, "m01", Outcome::Succeeded, 700))
        .expect("the retried victory applies");
    assert_eq!(state.currency(), 500);
    assert!(state.has_completed(&key("m01")));
}

/// An outcome for a node the run never reached is ineligible — a future
/// mission cannot report results — and a foreign profile/run is refused.
#[test]
fn accept_f43_a_ineligible_and_foreign_outcomes_are_refused() {
    let (graph, mut state) = fresh_run();

    let future = outcome(1, 480, 0, "m02", Outcome::Succeeded, 100);
    assert!(matches!(
        state.apply_outcome(&graph, &future),
        Err(CampaignError::IneligibleNode { .. })
    ));
    let unknown = outcome(1, 480, 0, "m99", Outcome::Succeeded, 100);
    assert!(matches!(
        state.apply_outcome(&graph, &unknown),
        Err(CampaignError::UnknownNode { .. })
    ));

    let mut foreign_packet = outcome(1, 480, 0, "m01", Outcome::Succeeded, 100);
    foreign_packet.id.profile = ProfileId::new("pilot.other").expect("valid");
    assert!(matches!(
        state.apply_outcome(&graph, &foreign_packet),
        Err(CampaignError::ForeignOutcome { .. })
    ));
    assert_eq!(state.currency(), 0, "a refusal changes nothing");
    assert_eq!(state.revision(), 0);
}

/// A modified-authority outcome still records and pays — but it marks the
/// progression it touched, permanently for the run (spec F43 behavior 1).
#[test]
fn accept_f43_a_modified_evidence_marks_the_progression_it_touches() {
    let (graph, mut state) = fresh_run();
    let mut modded = outcome(1, 480, 0, "m01", Outcome::Succeeded, 900);
    modded.authority = OutcomeAuthority::Modified {
        reason: "synthetic probe run".to_owned(),
    };
    state
        .apply_outcome(&graph, &modded)
        .expect("a modified outcome applies with its mark");
    assert!(state.modified());
    assert_eq!(state.currency(), 500);
}

/// AC04's mechanics on the fixture: victories walk the declared graph to its
/// ending — no extra mission is manufactured, and an ending reports no
/// outcome.
#[test]
fn accept_f43_a_victories_walk_the_graph_to_its_ending() {
    let (graph, mut state) = fresh_run();
    assert!(!state.is_finished(&graph));
    state
        .apply_outcome(&graph, &outcome(1, 480, 0, "m01", Outcome::Succeeded, 900))
        .expect("m01 victory");
    state
        .apply_outcome(&graph, &outcome(2, 480, 0, "m02", Outcome::Succeeded, 600))
        .expect("m02 victory");
    assert!(state.is_finished(&graph));
    assert_eq!(state.currency(), 2000);
    // The roster gate opened at m02's first completion.
    let items: Vec<&ContentId> = graph.available_items(&state).collect();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind(), ContentKind::Blueprint);
    // The ending node itself can never report an outcome.
    assert!(matches!(
        state.apply_outcome(&graph, &outcome(3, 1, 0, "ending", Outcome::Succeeded, 0)),
        Err(CampaignError::NotAMission { .. })
    ));
}

/// The lowering boundary refuses a mission node with no catalog binding and
/// an edge with an unmeasured reward — no guessed identity or amount passes.
#[test]
fn accept_f43_a_lowering_refuses_unknown_bindings_and_rewards() {
    let unknown_mission = CampaignDefinition::try_new(CampaignDraft {
        nodes: vec![
            CampaignNode {
                id: node("m"),
                kind: NodeKind::Mission {
                    mission: Resolved::unknown(
                        claim("f43a.boundary.mission"),
                        "the original mission id is unsurveyed",
                    )
                    .expect("a reason is present"),
                },
                edges: vec![CampaignEdge {
                    on: EdgeCondition::Victory,
                    to: node("end"),
                    grant: None,
                    provenance: designed(),
                }],
                provenance: designed(),
            },
            CampaignNode {
                id: node("end"),
                kind: NodeKind::Ending,
                edges: vec![],
                provenance: designed(),
            },
        ],
        entry: node("m"),
        roster: vec![],
        provenance: designed(),
    })
    .expect("the declared shape is valid");
    assert!(matches!(
        lower_campaign(&unknown_mission),
        Err(CampaignLowerError::UnknownMissionBinding { .. })
    ));

    let unknown_reward = CampaignDefinition::try_new(CampaignDraft {
        nodes: vec![
            CampaignNode {
                id: node("m"),
                kind: NodeKind::Mission {
                    mission: known(
                        ContentId::from_source(ContentKind::Mission, "synthetic.m").expect("valid"),
                    ),
                },
                edges: vec![CampaignEdge {
                    on: EdgeCondition::Victory,
                    to: node("end"),
                    grant: Some(RewardSpec {
                        currency: Resolved::unknown(
                            claim("f43a.boundary.reward"),
                            "the original reward amount is unmeasured",
                        )
                        .expect("a reason is present"),
                        unlocks: vec![],
                    }),
                    provenance: designed(),
                }],
                provenance: designed(),
            },
            CampaignNode {
                id: node("end"),
                kind: NodeKind::Ending,
                edges: vec![],
                provenance: designed(),
            },
        ],
        entry: node("m"),
        roster: vec![],
        provenance: designed(),
    })
    .expect("the declared shape is valid");
    assert!(matches!(
        lower_campaign(&unknown_reward),
        Err(CampaignLowerError::UnknownReward { .. })
    ));
}

/// A mission node bound to a non-mission content id refuses — at the
/// declared schema and again in the runtime mirror, so a hand-built
/// `CampaignGraph` cannot smuggle a non-mission id into a mission node.
#[test]
fn accept_f43_a_mission_nodes_refuse_non_mission_bindings() {
    let blueprint = ContentId::from_source(ContentKind::Blueprint, "synthetic.not-a-mission")
        .expect("valid id");
    let declared = CampaignDefinition::try_new(CampaignDraft {
        nodes: vec![
            CampaignNode {
                id: node("m"),
                kind: NodeKind::Mission {
                    mission: known(blueprint.clone()),
                },
                edges: vec![CampaignEdge {
                    on: EdgeCondition::Victory,
                    to: node("end"),
                    grant: None,
                    provenance: designed(),
                }],
                provenance: designed(),
            },
            CampaignNode {
                id: node("end"),
                kind: NodeKind::Ending,
                edges: vec![],
                provenance: designed(),
            },
        ],
        entry: node("m"),
        roster: vec![],
        provenance: designed(),
    });
    assert!(matches!(
        declared,
        Err(DeclaredError::MissionBindingKind { .. })
    ));

    let runtime = CampaignGraph::try_new(
        vec![
            RuntimeNode {
                id: key("m"),
                kind: RuntimeNodeKind::Mission { mission: blueprint },
                edges: vec![RuntimeEdge {
                    on: Outcome::Succeeded,
                    to: key("end"),
                    grant: Reward::default(),
                }],
            },
            RuntimeNode {
                id: key("end"),
                kind: RuntimeNodeKind::Ending,
                edges: vec![],
            },
        ],
        key("m"),
        vec![],
    );
    assert!(matches!(
        runtime,
        Err(GraphError::MissionBindingKind { .. })
    ));
}

/// The declared fixture lowers faithfully: node keys survive verbatim and
/// the condition vocabulary maps onto the mission runtime's outcomes.
#[test]
fn accept_f43_a_the_fixture_lowers_faithfully() {
    let graph = lower_campaign(&declared_synthetic_campaign()).expect("lowers");
    assert_eq!(graph.entry().as_str(), "m01");
    assert_eq!(graph.nodes().count(), 3);
    let edge = graph
        .transition(&key("m01"), Outcome::Succeeded)
        .expect("the victory edge lowered");
    assert_eq!(edge.to.as_str(), "m02");
    assert_eq!(edge.grant.currency, 500);
    assert_eq!(edge.grant.unlocks.len(), 1);
    // No declared abort edge exists — the answer is none, not a default.
    assert!(graph.transition(&key("m01"), Outcome::Aborted).is_none());
}

/// Revision discipline: every committed application bumps the monotonic
/// revision exactly once, in order — the counter the persistence layer will
/// write atomically.
#[test]
fn accept_f43_a_revisions_count_committed_transactions() {
    let (graph, mut state) = fresh_run();
    assert_eq!(state.revision(), 0);
    let applied = state
        .apply_outcome(&graph, &outcome(1, 480, 0, "m01", Outcome::Succeeded, 900))
        .expect("applies");
    assert_eq!(applied.revision, 1);
    assert_eq!(state.revision(), 1);
    // Dedup does not consume a revision.
    state
        .apply_outcome(&graph, &outcome(1, 480, 0, "m01", Outcome::Succeeded, 900))
        .expect("dedup is still a valid call");
    assert_eq!(state.revision(), 1);
}
