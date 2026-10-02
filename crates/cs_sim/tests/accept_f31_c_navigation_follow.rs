//! Acceptance scenarios F31-C for the consumer half: the production
//! `follow_route` driver carried a displaced actor back onto the route before
//! its next mandatory marker, and the graph/driver boundaries refuse invalid
//! input by name.
//!
//! Spec: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, stage
//! `### F31-C`. Task test prefix: `accept_f31_c_`.
//!
//! These tests call production code only: [`follow_route`], [`RouteGraph`] and
//! the [`NavigationSet`] beneath them. Removing the driver or making it forget
//! the set-owned progress makes the rejoin test fail; removing the validating
//! constructor makes the boundary test fail.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use cs_sim::ai::navigation::{
    Blocker, FollowPlan, NavState, NavigationCadence, NavigationError, NavigationSet, Navigator,
    ReferenceFrameSample, RouteFrame, RouteGraph, RouteGraphError, RouteNode, RouteNodeId,
    SYNTHETIC_PURSUIT_DT_S, SYNTHETIC_PURSUIT_SEED, SYNTHETIC_PURSUIT_SESSION,
    SYNTHETIC_PURSUIT_SESSION_ID, follow_route, heading_from_direction,
    synthetic_maneuver_envelope, synthetic_pursuit_actor, synthetic_pursuit_route,
    synthetic_pursuit_set,
};
use cs_sim::damage::ActorId;

fn actor(serial: u64) -> ActorId {
    synthetic_pursuit_actor(serial)
}

/// The minimum F31-C scenario through the production follow driver: an actor
/// displaced off the route and pointed away rejoins and reaches the next
/// mandatory marker without skipping one.
#[test]
fn accept_f31_c_displaced_actor_rejoins_before_the_next_mandatory_marker() {
    let route = synthetic_pursuit_route();
    let actor = actor(1);
    let mut set = synthetic_pursuit_set(1);
    let displaced = NavState {
        position_m: [40.0, 0.0, -20.0],
        heading_rad: heading_from_direction(0.0, 1.0),
        speed_mps: 40.0,
        climb_mps: 0.0,
    };

    let outcome = follow_route(
        &mut set,
        actor,
        FollowPlan {
            route: &route,
            blockers: &[],
            dt_s: SYNTHETIC_PURSUIT_DT_S,
            start: displaced,
            max_ticks: 4000,
        },
        |_| ReferenceFrameSample::IDENTITY,
    )
    .expect("the follow request is valid");

    assert!(
        outcome.first_mandatory_reached(&route).is_some(),
        "the displaced actor must reach its first mandatory marker"
    );
    assert!(outcome.progress.reached() >= 2);
    let mut previous = 1;
    for decision in &outcome.decisions {
        let reached = decision.decision.progress.reached();
        assert!(
            reached == previous || reached == previous + 1,
            "progress jumped from {previous} to {reached}"
        );
        previous = reached;
    }
}

/// The production driver samples the moving frame on every tick, so the same
/// local node maps to its current world position: a large per-tick origin shift
/// moves the observed target, and the shifted frame neither resets the
/// set-owned progress nor fires a false arrival.
///
/// The frame origin drives 300 m down `-Z` between ticks. A follower that
/// sampled only the identity frame would keep reporting the first node at its
/// local 200 m instead of the moved ~500 m, so the second decision's target
/// distance discriminates a real sample from an ignored one.
#[test]
fn accept_f31_c_follow_route_samples_a_moving_frame_each_tick() {
    let route = RouteGraph::try_new(
        RouteFrame::Moving { anchor: 7 },
        0.0,
        vec![
            RouteNode {
                id: RouteNodeId(0),
                sequence: 0,
                mandatory: true,
                position_m: [0.0, 0.0, -200.0],
                arrival_radius_m: 6.0,
            },
            RouteNode {
                id: RouteNodeId(1),
                sequence: 1,
                mandatory: true,
                position_m: [0.0, 0.0, -500.0],
                arrival_radius_m: 6.0,
            },
        ],
    )
    .expect("the moving route is valid");

    // Register at the route start (unlike `synthetic_pursuit_set`, which
    // resumes past a spawn node) so the first node is the live target.
    let navigator = Navigator::new(
        synthetic_maneuver_envelope(),
        NavigationCadence::designed_default(),
    )
    .expect("the synthetic envelope and cadence are valid");
    let mut set = NavigationSet::new(SYNTHETIC_PURSUIT_SESSION, SYNTHETIC_PURSUIT_SEED, navigator);
    let actor = actor(1);
    set.register(actor).expect("the fixture actor registers");

    // The frame origin drives 300 m down -Z between ticks.
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
            max_ticks: 4,
        },
        |tick| ReferenceFrameSample {
            origin_m: [0.0, 0.0, -300.0 * tick.0 as f64],
            yaw_rad: 0.0,
        },
    )
    .expect("the moving follow request is valid");

    assert!(
        outcome.decisions.len() >= 2,
        "the probe runs at least two ticks"
    );
    // Tick 0: the frame is at the origin, so the first node is 200 m ahead.
    let first = outcome.decisions[0]
        .decision
        .diagnostics
        .distance_to_target_m;
    assert!(
        (first - 200.0).abs() < 1.0,
        "the first tick targets the frame-mapped first node, got {first}"
    );
    // Tick 1: the frame moved 300 m, so the target is ~500 m away. An ignored
    // frame would still report ~199 m.
    let second = outcome.decisions[1]
        .decision
        .diagnostics
        .distance_to_target_m;
    assert!(
        second > 400.0,
        "the second tick must observe the moved frame, got {second}"
    );
    // The receding target is neither a false arrival nor a progress reset.
    for decision in &outcome.decisions {
        assert_eq!(
            decision.decision.progress.reached(),
            0,
            "a moving frame must not report progress it did not earn"
        );
    }
    assert_eq!(outcome.progress.reached(), 0);
}

/// The driver refuses an actor the set does not own rather than inventing a
/// pursuit state for it.
#[test]
fn accept_f31_c_follow_route_refuses_an_unknown_actor() {
    let route = synthetic_pursuit_route();
    let mut set = synthetic_pursuit_set(1);
    let stranger = ActorId {
        session: SYNTHETIC_PURSUIT_SESSION_ID,
        serial: 42,
    };
    assert_eq!(
        follow_route(
            &mut set,
            stranger,
            FollowPlan {
                route: &route,
                blockers: &[],
                dt_s: SYNTHETIC_PURSUIT_DT_S,
                start: synthetic_pursuit_route_start(),
                max_ticks: 8,
            },
            |_| ReferenceFrameSample::IDENTITY,
        ),
        Err(NavigationError::UnknownActor { actor: stranger })
    );
}

/// A graph built through the validating constructor is refused when it is
/// empty, and a corrupt blocker is refused at the decision boundary rather
/// than silently repaired.
#[test]
fn accept_f31_c_graph_and_decision_boundaries_refuse_invalid_input() {
    assert_eq!(
        RouteGraph::try_new(RouteFrame::World, 0.0, Vec::new()),
        Err(RouteGraphError::EmptyNodes)
    );
    assert_eq!(
        RouteGraph::try_new(RouteFrame::World, -1.0, synthetic_pursuit_route().nodes),
        Err(RouteGraphError::NonPositive {
            field: "clearance_m",
            value: -1.0,
        })
    );

    let route = synthetic_pursuit_route();
    let actor = actor(1);
    let mut set = synthetic_pursuit_set(1);
    let bad = [Blocker::sphere([f64::NAN, 0.0, 0.0], 1.0)];
    assert!(matches!(
        follow_route(
            &mut set,
            actor,
            FollowPlan {
                route: &route,
                blockers: &bad,
                dt_s: SYNTHETIC_PURSUIT_DT_S,
                start: synthetic_pursuit_route_start(),
                max_ticks: 8,
            },
            |_| ReferenceFrameSample::IDENTITY,
        ),
        Err(NavigationError::Blocker { index: 0, .. })
    ));
}

fn synthetic_pursuit_route_start() -> NavState {
    NavState {
        position_m: [0.0, 0.0, 0.0],
        heading_rad: 0.0,
        speed_mps: 40.0,
        climb_mps: 0.0,
    }
}
