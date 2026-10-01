//! Acceptance scenarios F31-D for the runtime half: a moving waypoint and an
//! origin shift do not reset the set-owned progress and do not trigger a
//! false arrival.
//!
//! Spec: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, stage
//! `### F31-D` (acceptance criterion AC04, the minimum scenario). Task test
//! prefix: `accept_f31_d_`.
//!
//! These tests call production code only: [`follow_route`], [`NavigationSet`],
//! [`RouteGraph`] and [`ReferenceFrameSample`]. Removing the frame-origin
//! application from `ReferenceFrameSample::world_position` (so an ignored
//! frame reads a displaced node as arrived) makes the false-arrival assertions
//! fail; removing the set-owned progress or the per-tick frame sampling makes
//! the moving-waypoint assertions fail.
//!
//! Every route, seed, session and frame value here is newly authored synthetic
//! fixture data, never original game data.

use cs_sim::ai::navigation::{
    FollowPlan, NavState, NavigationCadence, NavigationSet, Navigator, ReferenceFrameSample,
    RouteFrame, RouteGraph, RouteNode, RouteNodeId, SYNTHETIC_PURSUIT_DT_S, SYNTHETIC_PURSUIT_SEED,
    SYNTHETIC_PURSUIT_SESSION, follow_route, synthetic_maneuver_envelope, synthetic_pursuit_actor,
};
use cs_sim::damage::ActorId;

/// A moving-frame route whose waypoints live on a carrier: their world
/// positions move with the frame between ticks.
fn moving_waypoint_route() -> RouteGraph {
    RouteGraph::try_new(
        RouteFrame::Moving { anchor: 7 },
        0.0,
        vec![
            RouteNode {
                id: RouteNodeId(0),
                sequence: 0,
                mandatory: true,
                position_m: [0.0, 0.0, -200.0],
                arrival_radius_m: 8.0,
            },
            RouteNode {
                id: RouteNodeId(1),
                sequence: 1,
                mandatory: true,
                position_m: [0.0, 0.0, -600.0],
                arrival_radius_m: 10.0,
            },
        ],
    )
    .expect("the moving route is valid")
}

/// The registered actor for one run.
fn set_with_actor() -> (NavigationSet, ActorId) {
    let navigator = Navigator::new(
        synthetic_maneuver_envelope(),
        NavigationCadence::designed_default(),
    )
    .expect("the synthetic envelope and cadence are valid");
    let mut set = NavigationSet::new(SYNTHETIC_PURSUIT_SESSION, SYNTHETIC_PURSUIT_SEED, navigator);
    let actor = synthetic_pursuit_actor(1);
    set.register(actor).expect("the fixture actor registers");
    (set, actor)
}

/// AC04's minimum scenario: the waypoint's world position moves when the frame
/// origin shifts between ticks. The shift must not reset the set-owned
/// progress and must not fire an arrival the aircraft never earned.
///
/// The aircraft starts exactly at node 0's authored *local* position while the
/// frame origin is displaced 300 m down `-Z`, so node 0's world position is
/// 300 m away. Reading the node's local position (an ignored frame) would
/// report a false arrival on tick 0; the production frame mapping does not.
/// The frame then shifts another 200 m, moving both waypoints again, and the
/// follower keeps its earned progress and reaches the moved waypoints.
#[test]
fn accept_f31_d_moving_waypoint_and_origin_shift_do_not_reset_progress_or_false_arrive() {
    let route = moving_waypoint_route();
    let (mut set, actor) = set_with_actor();

    let outcome = follow_route(
        &mut set,
        actor,
        FollowPlan {
            route: &route,
            blockers: &[],
            dt_s: SYNTHETIC_PURSUIT_DT_S,
            start: NavState {
                // Node 0's authored local position; its world position is
                // 300 m away under the displaced frame.
                position_m: [0.0, 0.0, -200.0],
                heading_rad: 0.0,
                speed_mps: 120.0,
                climb_mps: 0.0,
            },
            max_ticks: 3000,
        },
        |tick| ReferenceFrameSample {
            origin_m: if tick.0 == 0 {
                [0.0, 0.0, -300.0]
            } else {
                [0.0, 0.0, -500.0]
            },
            yaw_rad: 0.0,
        },
    )
    .expect("the moving follow request is valid");

    assert!(
        outcome.decisions.len() >= 3,
        "the probe runs several ticks, got {}",
        outcome.decisions.len()
    );

    // An ignored frame would read the node at its local position and falsely
    // arrive on tick 0; the frame mapping keeps it 300 m away.
    assert_eq!(
        outcome.decisions[0].decision.progress.reached(),
        0,
        "the displaced node is not an arrival"
    );

    // The frame shift and the moving waypoints must never reset progress, and
    // progress never advances by more than one node per tick.
    let mut previous = 0;
    for decision in &outcome.decisions {
        let reached = decision.decision.progress.reached();
        assert!(
            reached >= previous,
            "an origin shift must never reset progress: {previous} -> {reached}"
        );
        assert!(
            reached <= previous + 1,
            "progress jumped from {previous} to {reached}"
        );
        previous = reached;
    }

    // The follower still reaches both moved waypoints.
    assert_eq!(
        outcome.progress.reached(),
        2,
        "the follower reaches the moved waypoints it re-targeted"
    );
    assert!(outcome.complete, "following the moved waypoint completes");
}

/// A large origin shift after progress is earned keeps the earned prefix: the
/// progress before and after the shift agree, and no node is re-counted.
#[test]
fn accept_f31_d_a_mid_route_origin_shift_keeps_the_earned_progress() {
    let route = moving_waypoint_route();
    let (mut set, actor) = set_with_actor();

    // The frame is home for the first half of the run, then jumps far down
    // `-Z`; a re-derivation from position would lose the earned node 0.
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
                speed_mps: 120.0,
                climb_mps: 0.0,
            },
            max_ticks: 2000,
        },
        |tick| ReferenceFrameSample {
            origin_m: if tick.0 < 10 {
                [0.0, 0.0, 0.0]
            } else {
                [0.0, 0.0, -450.0]
            },
            yaw_rad: 0.0,
        },
    )
    .expect("the moving follow request is valid");

    // Progress is monotonic across the shift, and the earned node 0 is never
    // dropped even when node 1's world position jumps far away.
    let mut previous = 0;
    for decision in &outcome.decisions {
        let reached = decision.decision.progress.reached();
        assert!(
            reached >= previous,
            "the origin shift reset progress: {previous} -> {reached}"
        );
        previous = reached;
    }
    assert!(
        outcome.progress.reached() >= 1,
        "the earned node 0 survives the shift"
    );
}
