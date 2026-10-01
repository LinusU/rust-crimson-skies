//! Acceptance scenarios for task #447 on the producer side: a declared
//! `Loop` route projects into the runtime graph without an unexplained
//! unsupported termination, and the production driver then flies it.
//!
//! Spec: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md` (stage
//! `### F31-C`). Task test prefix: `accept_t447_`.
//!
//! These tests call production code only: [`RouteDefinition::try_new`],
//! [`RouteDefinition::resolve`], [`project_route`] and the production
//! [`follow_route`] driver beneath them. Removing the loop's declaration in
//! [`project_route`] (so a `Loop` record is refused, or defaulted to an end and
//! silently followed once) makes one of them fail.
//!
//! Whether the original 2000 route encoding expresses a loop at all is
//! **unmeasured** (F13; F31-D owns retail route coverage). These tests pin the
//! declared-content -> runtime contract, not an original-data claim.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use cs_content::routes::{
    AnchorKind, MIN_LOOP_NODES, MovingAnchor, ReferenceFrame, RouteDefinition, RouteDraft,
    RouteEdge, RouteError, RouteNode, RouteNodeId, RouteTermination,
};
use cs_inspect::routes::{RouteProjectionError, project_route};
use cs_sim::ai::navigation::{
    FollowPlan, NavState, NavigationCadence, NavigationSet, Navigator, ReferenceFrameSample,
    RouteTermination as NavRouteTermination, SYNTHETIC_PURSUIT_DT_S, SYNTHETIC_PURSUIT_SEED,
    SYNTHETIC_PURSUIT_SESSION, follow_route, synthetic_maneuver_envelope,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

fn designed() -> Provenance {
    Provenance::designed(ClaimId::new("t447.loop-fixture").expect("valid claim id"))
}

/// A three-node declared patrol in the world frame: a triangle in XZ, so the
/// wrap edge is a real leg. Node 1 is mandatory; node 0 is not, so the wrap
/// itself carries no marker requirement.
fn loop_draft() -> RouteDraft {
    let node = |id: &str, sequence: u32, mandatory: bool, position: [f64; 3], radius: f64| {
        let designed = designed();
        RouteNode {
            id: RouteNodeId::try_new(id).expect("valid node id"),
            sequence,
            mandatory,
            position_m: Resolved::Known(Known::new(position, designed.clone())),
            arrival_radius_m: Resolved::Known(Known::new(radius, designed.clone())),
            trigger: Resolved::Known(Known::new(None, designed)),
        }
    };
    let edge = |from: &str, to: &str| RouteEdge {
        from: RouteNodeId::try_new(from).expect("valid node id"),
        to: RouteNodeId::try_new(to).expect("valid node id"),
    };
    RouteDraft {
        id: ContentId::from_source(ContentKind::Route, "synthetic.loop.patrol")
            .expect("valid route id"),
        origin: Origin::SyntheticFixture,
        frame: ReferenceFrame::World,
        termination: RouteTermination::Loop,
        clearance_m: Resolved::Known(Known::new(0.0, designed())),
        nodes: vec![
            node("start", 0, false, [0.0, 0.0, 0.0], 12.0),
            node("marker", 1, true, [0.0, 0.0, -160.0], 8.0),
            node("corner", 2, true, [-160.0, 0.0, -80.0], 6.0),
        ],
        // The wrap edge (2 -> 0) is implied by `RouteTermination::Loop`; the
        // declared edges stay adjacent, as the content contract requires.
        edges: vec![edge("start", "marker"), edge("marker", "corner")],
        provenance: designed(),
    }
}

/// The producer half: a declared `Loop` route reaches the runtime graph as a
/// loop. `project_route` no longer reports an unexplained unsupported
/// termination, and it does not silently downgrade a loop to an end either.
#[test]
fn accept_t447_a_declared_loop_projects_as_a_loop_not_an_unsupported_refusal() {
    let definition =
        RouteDefinition::try_new(loop_draft()).expect("the looping route is valid content");
    let resolved = definition.resolve().expect("the looping route resolves");

    let projected = project_route(&resolved, &[]).expect(
        "a declared loop must project; the projection reports no unsupported \
         termination for it",
    );
    assert_eq!(projected.termination(), NavRouteTermination::Loop);
    assert_eq!(projected.nodes.len(), 3);

    // Silently following it as an end would make the runtime graph claim the
    // route ends at its last node; assert the mapping is a real discrimination
    // by checking the ending variant of the same record.
    let mut ending = loop_draft();
    ending.termination = RouteTermination::End;
    let ending = RouteDefinition::try_new(ending).expect("the ending route is valid content");
    let ending = ending.resolve().expect("the ending route resolves");
    let projected_end = project_route(&ending, &[]).expect("the ending route projects");
    assert_eq!(projected_end.termination(), NavRouteTermination::End);
}

/// The full producer-to-consumer path: the projected loop route is flown by the
/// production driver, re-arming across several laps with monotonic progress.
///
/// This is the end-to-end proof that the loop is *followed*, not merely
/// accepted: a projection that downgraded the loop to an end would complete
/// after one lap and fail the lap and monotonic-progress assertions.
#[test]
fn accept_t447_a_projected_loop_route_is_flown_for_several_laps() {
    let definition =
        RouteDefinition::try_new(loop_draft()).expect("the looping route is valid content");
    let resolved = definition.resolve().expect("the looping route resolves");
    let route = project_route(&resolved, &[]).expect("the looping route projects");

    let navigator = Navigator::new(
        synthetic_maneuver_envelope(),
        NavigationCadence::designed_default(),
    )
    .expect("the synthetic envelope and cadence are valid");
    let mut set = NavigationSet::new(SYNTHETIC_PURSUIT_SESSION, SYNTHETIC_PURSUIT_SEED, navigator);
    let actor = cs_sim::damage::ActorId {
        session: SYNTHETIC_PURSUIT_SESSION,
        serial: 1,
    };
    set.register(actor).expect("the fixture actor registers");

    let outcome = follow_route(
        &mut set,
        actor,
        FollowPlan {
            route: &route,
            blockers: &[],
            dt_s: SYNTHETIC_PURSUIT_DT_S,
            start: NavState {
                position_m: [0.0, 0.0, 0.0],
                heading_rad: 0.0,
                speed_mps: 40.0,
                climb_mps: 0.0,
            },
            max_ticks: 20_000,
        },
        |_| ReferenceFrameSample::IDENTITY,
    )
    .expect("the loop follow request is valid");

    assert!(
        !outcome.complete,
        "a loop route re-arms instead of reporting completion"
    );
    assert!(
        outcome.progress.laps() >= 3,
        "the projected loop must be flown for several laps, got {}",
        outcome.progress.laps()
    );
    assert!(
        outcome.progress.reached() >= route.node_count() * 3,
        "the monotonic count must keep climbing across laps, got {}",
        outcome.progress.reached()
    );
}

/// A loop route with fewer than the minimum nodes is refused by name at the
/// content boundary, before it can reach the runtime. A one-node loop has no
/// wrap edge, so re-arming it could never advance the follower.
#[test]
fn accept_t447_a_one_node_loop_is_refused_at_the_content_boundary() {
    let mut draft = loop_draft();
    draft.nodes.truncate(1);
    draft.edges.clear();

    assert_eq!(
        RouteDefinition::try_new(draft),
        Err(RouteError::LoopNeedsMultipleNodes { nodes: 1 })
    );

    // The same single-node record is fine as an ending route: the rule is about
    // looping, not about the node count.
    let mut ending = loop_draft();
    ending.termination = RouteTermination::End;
    ending.nodes.truncate(1);
    ending.edges.clear();
    assert!(RouteDefinition::try_new(ending).is_ok());
    // The content boundary and the runtime boundary must agree on the
    // threshold, or a record could validate in one and be refused in the other.
    assert_eq!(MIN_LOOP_NODES, cs_sim::ai::navigation::MIN_LOOP_NODES);
}

/// The projection boundary still refuses what it always did, so removing the
/// loop refusal did not turn it into a permissive pass-everything: a moving
/// route with no anchor binding is still a named error.
///
/// The `Graph` arm is deliberately not probed here. Every rule
/// `RouteGraph::validate` enforces is also enforced by
/// `RouteDefinition::try_new` — and since #447 the one-node-loop rule is
/// enforced on both sides with the same threshold — so no content record that
/// resolves can reach it. The runtime rules themselves are pinned by the
/// `accept_f31_a_` graph tests in `cs_sim`.
#[test]
fn accept_t447_projection_still_names_its_other_refusals() {
    let mut draft = loop_draft();
    draft.frame = ReferenceFrame::Moving(MovingAnchor {
        anchor: ContentId::from_source(ContentKind::SceneNode, "synthetic.loop.anchor")
            .expect("valid anchor id"),
        kind: AnchorKind::Carrier,
    });
    let definition =
        RouteDefinition::try_new(draft).expect("the moving loop route is valid content");
    let resolved = definition
        .resolve()
        .expect("the moving loop route resolves");

    assert!(matches!(
        project_route(&resolved, &[]),
        Err(RouteProjectionError::UnboundAnchor { .. })
    ));
}
