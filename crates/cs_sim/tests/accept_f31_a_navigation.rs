//! Acceptance scenarios F31-A for the navigation contract: the route graph,
//! the maneuver envelope and the bounded route follower.
//!
//! Spec: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, stage
//! `### F31-A`. Task test prefix: `accept_f31_a_`.
//!
//! These tests drive production code only: [`cs_sim::ai::navigation`]'s
//! [`Navigator::decide`], the synthetic arch probe, the swept arrival and
//! blocker primitives and the typed envelope/route validation. Removing the
//! follower, letting a step cross a blocker, making arrival a position
//! equality, dropping the envelope bound or letting a corrupt route through
//! each makes one of them fail.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use cs_sim::ai::navigation::{
    AvoidanceState, Blocker, ManeuverEnvelope, ManeuverEnvelopeError, NavState, NavigationCadence,
    NavigationError, NavigationRequest, Navigator, ReferenceFrameSample, RouteGraph,
    RouteGraphError, RouteNode, RouteNodeId, RouteProgress, SyntheticArchProbe,
    forward_from_heading, heading_from_direction, synthetic_arch_blockers, synthetic_arch_route,
    synthetic_arch_start, synthetic_maneuver_envelope, wrap_pi,
};
use cs_types::Tick;

const DT_S: f64 = 1.0 / 60.0;

fn navigator() -> Navigator {
    Navigator::new(
        synthetic_maneuver_envelope(),
        NavigationCadence::designed_default(),
    )
    .expect("the synthetic envelope and cadence are valid")
}

fn single_node_route(position_m: [f64; 3], arrival_radius_m: f64) -> RouteGraph {
    RouteGraph {
        frame: cs_sim::ai::navigation::RouteFrame::World,
        clearance_m: 0.0,
        nodes: vec![RouteNode {
            id: RouteNodeId(0),
            sequence: 0,
            mandatory: true,
            position_m,
            arrival_radius_m,
        }],
    }
}

/// AC01 minimum scenario: the route through the narrow arch is followed from
/// the start to the goal without crossing the blocked wall, and no step is
/// ever a `Blocked` hold.
#[test]
fn accept_f31_a_route_through_narrow_arch_is_followed_without_crossing_the_wall() {
    let mut probe = SyntheticArchProbe::new().expect("the synthetic fixture is valid");
    let traversal = probe.run(4000).expect("every request is valid");

    assert!(
        traversal.complete,
        "the follower must complete the arch route, reached {} of {} nodes",
        traversal.reached, traversal.total_nodes
    );
    assert_eq!(traversal.reached, traversal.total_nodes);
    assert_eq!(traversal.blocked_at, None, "no step may hold as blocked");
    assert_eq!(
        traversal.crossed_blocker, None,
        "no committed segment may cross a blocker"
    );
    assert!(
        traversal.decisions.len() > 100,
        "the route is long enough to be a real traversal, got {} decisions",
        traversal.decisions.len()
    );

    // Progress is monotonic and never skips: it either holds or advances by
    // exactly one node per decision.
    let mut previous = RouteProgress::reached_nodes(1).reached();
    for decision in &traversal.decisions {
        let reached = decision.progress.reached();
        assert!(
            reached == previous || reached == previous + 1,
            "progress jumped from {previous} to {reached}"
        );
        assert!(
            !matches!(decision.avoidance, AvoidanceState::Blocked),
            "the arch route must never require a blocked hold"
        );
        previous = reached;
    }

    // The mandatory arch marker was reached in sequence.
    assert!(traversal.reached >= 4, "the mandatory markers were reached");
}

/// The arch is a real obstacle: a naive direct pursuit from the start to the
/// goal sweeps through the wall, so the previous test is not vacuous. The
/// production follower never did so.
#[test]
fn accept_f31_a_direct_goal_pursuit_would_cross_the_wall() {
    let start = synthetic_arch_start();
    let route = synthetic_arch_route();
    let goal = route.nodes.last().expect("the goal node exists").position_m;
    let blockers = synthetic_arch_blockers();

    let naive_crossings = blockers
        .iter()
        .filter(|blocker| blocker.segment_intersects(start.position_m, goal))
        .count();
    assert_eq!(
        naive_crossings, 1,
        "the straight start-to-goal line must cross the wall exactly once"
    );

    let mut probe = SyntheticArchProbe::new().expect("the synthetic fixture is valid");
    let traversal = probe.run(4000).expect("every request is valid");
    assert_eq!(traversal.crossed_blocker, None);
    assert!(traversal.complete);
}

/// A mandatory marker cannot be skipped: it is targeted before its successor
/// and reached only by a swept arrival.
#[test]
fn accept_f31_a_mandatory_marker_is_targeted_before_its_successor() {
    let route = RouteGraph {
        frame: cs_sim::ai::navigation::RouteFrame::World,
        clearance_m: 0.0,
        nodes: vec![
            RouteNode {
                id: RouteNodeId(0),
                sequence: 0,
                mandatory: true,
                position_m: [0.0, 0.0, -50.0],
                arrival_radius_m: 4.0,
            },
            RouteNode {
                id: RouteNodeId(1),
                sequence: 1,
                mandatory: true,
                position_m: [0.0, 0.0, -100.0],
                arrival_radius_m: 4.0,
            },
        ],
    };
    let mut state = NavState {
        position_m: [0.0, 0.0, 0.0],
        heading_rad: heading_from_direction(0.0, -1.0),
        speed_mps: 40.0,
        climb_mps: 0.0,
    };
    let navigator = navigator();

    let mut progress = RouteProgress::start();
    let mut targets = Vec::new();
    for tick in 0..2000 {
        let request = NavigationRequest {
            tick: Tick(tick),
            generation: 1,
            state,
            route: &route,
            progress,
            frame: ReferenceFrameSample::IDENTITY,
            blockers: &[],
            dt_s: DT_S,
        };
        let decision = navigator.decide(&request).expect("valid request");
        targets.push(decision.target);
        progress = decision.progress;
        state.position_m = decision.step.to_m;
        state.heading_rad = decision.step.heading_rad;
        state.speed_mps = decision.step.speed_mps;
        state.climb_mps = decision.step.climb_mps;
        if progress.is_complete(&route) {
            break;
        }
    }
    assert!(
        progress.is_complete(&route),
        "the route completes, reached {}",
        progress.reached()
    );

    let first_marker = targets
        .iter()
        .position(|target| *target == Some(RouteNodeId(0)))
        .expect("the first marker is targeted");
    let second_marker = targets
        .iter()
        .position(|target| *target == Some(RouteNodeId(1)))
        .expect("the second marker is targeted");
    assert!(
        first_marker < second_marker,
        "marker 0 must be targeted before marker 1"
    );
}

/// Arrival is swept, not an equality of float positions: a fast step whose
/// endpoints are both outside a node still arrives, and a step that does not
/// reach it does not.
#[test]
fn accept_f31_a_arrival_is_swept_not_position_equality() {
    let navigator = navigator();
    let fast = NavState {
        position_m: [0.0, 0.0, 0.0],
        heading_rad: 0.0, // forward -Z
        speed_mps: 3000.0,
        climb_mps: 0.0,
    };

    let reaches = single_node_route([0.0, 0.0, -10.0], 2.0);
    let decision = navigator
        .decide(&NavigationRequest {
            tick: Tick(0),
            generation: 1,
            state: fast,
            route: &reaches,
            progress: RouteProgress::start(),
            frame: ReferenceFrameSample::IDENTITY,
            blockers: &[],
            dt_s: DT_S,
        })
        .expect("valid request");
    assert_ne!(decision.step.to_m, reaches.nodes[0].position_m);
    assert_ne!(decision.step.from_m, reaches.nodes[0].position_m);
    assert_eq!(
        decision.progress.reached(),
        1,
        "a step that sweeps through the arrival sphere arrives"
    );

    let misses = single_node_route([0.0, 0.0, -500.0], 2.0);
    let decision = navigator
        .decide(&NavigationRequest {
            tick: Tick(0),
            generation: 1,
            state: fast,
            route: &misses,
            progress: RouteProgress::start(),
            frame: ReferenceFrameSample::IDENTITY,
            blockers: &[],
            dt_s: DT_S,
        })
        .expect("valid request");
    assert_eq!(
        decision.progress.reached(),
        0,
        "a step that does not reach the node does not arrive"
    );
}

/// A step that would cross a blocker is never issued: the follower holds
/// position, commands neutral, keeps progress and reports `Blocked` rather
/// than crossing or teleporting.
#[test]
fn accept_f31_a_blocked_step_holds_position_without_crossing() {
    let route = single_node_route([100.0, 0.0, 0.0], 5.0);
    // A wall just ahead, far wider than one tick's bounded deviation can clear,
    // but not containing the start position itself.
    let wall = Blocker::axis_aligned_box([10.0, 0.0, 0.0], [9.9, 50.0, 50.0]);
    let blockers = [wall];
    let state = NavState {
        position_m: [0.0, 0.0, 0.0],
        heading_rad: heading_from_direction(1.0, 0.0),
        speed_mps: 40.0,
        climb_mps: 0.0,
    };
    let navigator = navigator();

    let decision = navigator
        .decide(&NavigationRequest {
            tick: Tick(0),
            generation: 1,
            state,
            route: &route,
            progress: RouteProgress::start(),
            frame: ReferenceFrameSample::IDENTITY,
            blockers: &blockers,
            dt_s: DT_S,
        })
        .expect("valid request");

    assert_eq!(decision.avoidance, AvoidanceState::Blocked);
    assert_eq!(decision.step.from_m, decision.step.to_m, "no motion issued");
    assert_eq!(decision.progress.reached(), 0, "progress is unchanged");
    assert_eq!(decision.command, cs_sim::flight::FlightInput::NEUTRAL);
    assert!(!wall.segment_intersects(decision.step.from_m, decision.step.to_m));
    assert!(
        !blockers
            .iter()
            .any(|blocker| blocker.segment_intersects(decision.step.from_m, decision.step.to_m)),
        "the held position must not be inside the wall"
    );
}

/// When the direct step would clip a small blocker, the follower issues a
/// bounded deviation inside one tick's yaw step rather than holding.
#[test]
fn accept_f31_a_blocked_direct_step_deviates_within_the_envelope() {
    let route = single_node_route([0.0, 0.0, -1000.0], 5.0);
    // A small sphere just ahead: the direct step sweeps through it, but a
    // single bounded yaw step to either side clears it.
    let small = Blocker::sphere([0.0, 0.0, -0.3], 0.004);
    let blockers = [small];
    let state = NavState {
        position_m: [0.0, 0.0, 0.0],
        heading_rad: 0.0, // forward -Z, straight at the sphere
        speed_mps: 40.0,
        climb_mps: 0.0,
    };
    let navigator = navigator();

    assert!(
        small.segment_intersects(state.position_m, [0.0, 0.0, -40.0 * DT_S]),
        "the direct step must sweep through the blocker for this case to bite"
    );

    let decision = navigator
        .decide(&NavigationRequest {
            tick: Tick(0),
            generation: 1,
            state,
            route: &route,
            progress: RouteProgress::start(),
            frame: ReferenceFrameSample::IDENTITY,
            blockers: &blockers,
            dt_s: DT_S,
        })
        .expect("valid request");

    assert_eq!(decision.avoidance, AvoidanceState::Deviating);
    assert_ne!(
        decision.step.from_m, decision.step.to_m,
        "a deviation still moves the aircraft"
    );
    assert!(
        !small.segment_intersects(decision.step.from_m, decision.step.to_m),
        "the committed deviation must clear the blocker"
    );
    let applied = wrap_pi(decision.step.heading_rad - state.heading_rad);
    let max_step = synthetic_maneuver_envelope().max_yaw_rate_radps * DT_S;
    assert!(
        applied.abs() <= max_step + 1e-12,
        "the deviation {applied} exceeds the envelope step {max_step}"
    );
}

/// A completed route yields the `Arrived` state, a neutral command, no target
/// and a held position, without reading past the last node.
#[test]
fn accept_f31_a_completed_route_reports_arrived_and_holds() {
    let route = single_node_route([0.0, 0.0, -10.0], 2.0);
    let navigator = navigator();
    let state = NavState {
        position_m: [0.0, 0.0, -10.0],
        heading_rad: 0.0,
        speed_mps: 40.0,
        climb_mps: 0.0,
    };

    let decision = navigator
        .decide(&NavigationRequest {
            tick: Tick(9),
            generation: 2,
            state,
            route: &route,
            progress: RouteProgress::reached_nodes(1),
            frame: ReferenceFrameSample::IDENTITY,
            blockers: &[],
            dt_s: DT_S,
        })
        .expect("valid request");

    assert_eq!(decision.avoidance, AvoidanceState::Arrived);
    assert_eq!(decision.target, None);
    assert_eq!(decision.command, cs_sim::flight::FlightInput::NEUTRAL);
    assert_eq!(decision.step.from_m, decision.step.to_m, "arrival holds");
    assert!(decision.progress.is_complete(&route));
}

/// The same request with the blockers reordered produces the identical
/// decision, because the follower's only dependence on them is `any`.
#[test]
fn accept_f31_a_decision_is_independent_of_blocker_order() {
    let route = synthetic_arch_route();
    let mut blockers = synthetic_arch_blockers();
    let state = synthetic_arch_start();
    let navigator = navigator();
    let progress = RouteProgress::reached_nodes(1);

    fn build<'a>(
        route: &'a RouteGraph,
        state: NavState,
        progress: RouteProgress,
        blockers: &'a [Blocker],
    ) -> NavigationRequest<'a> {
        NavigationRequest {
            tick: Tick(7),
            generation: 3,
            state,
            route,
            progress,
            frame: ReferenceFrameSample::IDENTITY,
            blockers,
            dt_s: DT_S,
        }
    }
    let forward = navigator
        .decide(&build(&route, state, progress, &blockers))
        .expect("valid request");
    blockers.reverse();
    let reversed = navigator
        .decide(&build(&route, state, progress, &blockers))
        .expect("valid request");
    assert_eq!(forward, reversed);
}

/// A route authored in a moving frame is reached by translating the state and
/// the frame origin together: an origin shift moves both endpoints and changes
/// no relative geometry or command.
#[test]
fn accept_f31_a_origin_shift_translates_state_and_frame_together() {
    let route = single_node_route([60.0, 5.0, -4.0], 5.0);
    let navigator = navigator();
    let progress = RouteProgress::start();

    let local = NavState {
        position_m: [0.0, 0.0, -30.0],
        heading_rad: heading_from_direction(60.0, 26.0),
        speed_mps: 40.0,
        climb_mps: 0.0,
    };
    let frame = ReferenceFrameSample {
        origin_m: [1000.0, -200.0, 3000.0],
        yaw_rad: 0.0,
    };
    let shift = [500.0, 50.0, -750.0];

    fn request<'a>(
        route: &'a RouteGraph,
        progress: RouteProgress,
        state: NavState,
        frame: ReferenceFrameSample,
    ) -> NavigationRequest<'a> {
        NavigationRequest {
            tick: Tick(0),
            generation: 1,
            state,
            route,
            progress,
            frame,
            blockers: &[],
            dt_s: DT_S,
        }
    }
    let base = navigator
        .decide(&request(&route, progress, local, frame))
        .expect("valid request");

    let shifted_state = NavState {
        position_m: [
            local.position_m[0] + shift[0],
            local.position_m[1] + shift[1],
            local.position_m[2] + shift[2],
        ],
        ..local
    };
    let shifted_frame = ReferenceFrameSample {
        origin_m: [
            frame.origin_m[0] + shift[0],
            frame.origin_m[1] + shift[1],
            frame.origin_m[2] + shift[2],
        ],
        yaw_rad: frame.yaw_rad,
    };
    let shifted = navigator
        .decide(&request(&route, progress, shifted_state, shifted_frame))
        .expect("valid request");

    for (axis, offset) in shift.iter().enumerate() {
        assert!(
            (base.step.to_m[axis] + offset - shifted.step.to_m[axis]).abs() < 1e-9,
            "the origin shift must translate both endpoints, axis {axis}"
        );
    }
    assert!((base.step.heading_rad - shifted.step.heading_rad).abs() < 1e-12);
    // The command is the same up to the floating-point rounding a large origin
    // introduces; it is not bit-identical, so compare each range-valued field.
    for (base_value, shifted_value) in [
        (base.command.pitch, shifted.command.pitch),
        (base.command.roll, shifted.command.roll),
        (base.command.yaw, shifted.command.yaw),
        (base.command.throttle, shifted.command.throttle),
    ] {
        assert!(
            (base_value - shifted_value).abs() < 1e-9,
            "the shifted command must match the base one, got {base_value} vs {shifted_value}"
        );
    }
    assert_eq!(base.command.boost, shifted.command.boost);
    assert_eq!(base.avoidance, shifted.avoidance);
}

/// Every heading step is bounded by the envelope's turn rate, and the emitted
/// command stays inside the flight-input ranges.
#[test]
fn accept_f31_a_heading_step_is_bounded_by_the_envelope() {
    let route = single_node_route([50.0, 0.0, 0.0], 5.0);
    let state = NavState {
        position_m: [0.0, 0.0, 0.0],
        heading_rad: 0.0, // forward -Z; the target is 90 degrees to the left
        speed_mps: 40.0,
        climb_mps: 0.0,
    };
    let navigator = navigator();
    let envelope = synthetic_maneuver_envelope();

    let decision = navigator
        .decide(&NavigationRequest {
            tick: Tick(0),
            generation: 1,
            state,
            route: &route,
            progress: RouteProgress::start(),
            frame: ReferenceFrameSample::IDENTITY,
            blockers: &[],
            dt_s: DT_S,
        })
        .expect("valid request");

    let applied = wrap_pi(decision.step.heading_rad - state.heading_rad);
    let max_step = envelope.max_yaw_rate_radps * DT_S;
    assert!(
        applied.abs() <= max_step + 1e-12,
        "the applied turn {applied} exceeds the envelope step {max_step}"
    );
    assert!(decision.command.roll.abs() <= 1.0);
    assert!(decision.command.pitch.abs() <= 1.0);
    assert!((0.0..=1.0).contains(&decision.command.throttle));
    assert!(
        forward_from_heading(decision.step.heading_rad)
            .iter()
            .all(|component| component.is_finite())
    );
}

/// The navigator refuses a corrupt envelope and a corrupt cadence by name.
#[test]
fn accept_f31_a_navigator_refuses_corrupt_envelope_and_cadence() {
    assert_eq!(
        NavigationCadence::new(0),
        Err(NavigationError::NonPositive {
            field: "cadence.ticks_per_decision",
            value: 0.0,
        })
    );

    let mut envelope = ManeuverEnvelope {
        max_yaw_rate_radps: -1.0,
        ..synthetic_maneuver_envelope()
    };
    assert_eq!(
        Navigator::new(envelope, NavigationCadence::designed_default()),
        Err(NavigationError::Envelope(
            ManeuverEnvelopeError::NotPositive {
                field: "max_yaw_rate_radps",
                value: -1.0,
            }
        ))
    );
    envelope.max_yaw_rate_radps = 0.0;
    assert!(Navigator::new(envelope, NavigationCadence::designed_default()).is_err());

    assert_eq!(
        single_node_route([0.0, 0.0, -10.0], 0.0).validate(),
        Err(RouteGraphError::NonPositive {
            field: "arrival_radius_m",
            value: 0.0,
        })
    );
}
