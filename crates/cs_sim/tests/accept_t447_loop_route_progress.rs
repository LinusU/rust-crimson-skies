//! Acceptance scenarios for task #447: the production follower follows a
//! loop-terminated route with monotonic, re-armed progress.
//!
//! Spec: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md` (stage
//! `### F31-C`, spec non-negotiable behaviors 1 and 3). Task test prefix:
//! `accept_t447_`.
//!
//! These tests call production code only: [`follow_route`], [`RouteGraph`],
//! [`RouteProgress`] and the [`NavigationSet`] beneath them. Removing the loop
//! re-arm (so a loop behaves like an end), making the wrap skip a mandatory
//! marker, or testing the wrap edge with a radius other than node 0's own
//! authored radius each makes one of them fail.
//!
//! Whether the original 2000 route encoding expresses a loop at all is
//! **unmeasured** (F13; F31-D owns retail coverage). These tests pin the
//! runtime's *designed* loop semantics, not an original-data claim.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use cs_sim::ai::navigation::{
    AvoidanceState, FollowPlan, MIN_LOOP_NODES, NavState, NavigationCadence, NavigationSet,
    Navigator, PursuitRequest, ReferenceFrameSample, RouteFrame, RouteGraph, RouteGraphError,
    RouteNode, RouteNodeId, RouteTermination, SYNTHETIC_PURSUIT_DT_S, SYNTHETIC_PURSUIT_SEED,
    SYNTHETIC_PURSUIT_SESSION, follow_route, synthetic_loop_route, synthetic_maneuver_envelope,
    synthetic_pursuit_actor,
};
use cs_sim::damage::ActorId;
use cs_types::Tick;

fn actor(serial: u64) -> ActorId {
    synthetic_pursuit_actor(serial)
}

/// An empty set, so a test chooses how its actor registers.
fn empty_set() -> NavigationSet {
    let navigator = Navigator::new(
        synthetic_maneuver_envelope(),
        NavigationCadence::designed_default(),
    )
    .expect("the synthetic envelope and cadence are valid");
    NavigationSet::new(SYNTHETIC_PURSUIT_SESSION, SYNTHETIC_PURSUIT_SEED, navigator)
}

/// A `NavigationSet` whose actor registers at the route's start (node 0 is the
/// live target), so a loop is followed from its very first node.
fn loop_set(actor: ActorId) -> NavigationSet {
    let mut set = empty_set();
    set.register(actor).expect("the fixture actor registers");
    set
}

fn start_at_origin() -> NavState {
    NavState {
        position_m: [0.0, 0.0, 0.0],
        heading_rad: 0.0,
        speed_mps: 40.0,
        climb_mps: 0.0,
    }
}

/// The core scenario: the production driver flies a declared loop for several
/// laps. Progress is monotonic (it never rewinds), the target re-arms to node 0
/// at the wrap, and the run never reports the loop route complete.
#[test]
fn accept_t447_loop_route_is_followed_with_monotonic_rearmed_progress() {
    let route = synthetic_loop_route();
    assert_eq!(route.termination(), RouteTermination::Loop);
    assert!(route.node_count() >= MIN_LOOP_NODES);

    let actor = actor(1);
    let mut set = loop_set(actor);
    let outcome = follow_route(
        &mut set,
        actor,
        FollowPlan {
            route: &route,
            blockers: &[],
            dt_s: SYNTHETIC_PURSUIT_DT_S,
            start: start_at_origin(),
            max_ticks: 20_000,
        },
        |_| ReferenceFrameSample::IDENTITY,
    )
    .expect("the loop follow request is valid");

    assert!(
        !outcome.complete,
        "a loop route re-arms instead of completing"
    );
    assert_eq!(outcome.blocked_at, None, "the fixture is unobstructed");
    assert!(
        outcome.progress.laps() >= 3,
        "the follower must complete several laps, got {}",
        outcome.progress.laps()
    );
    assert!(
        outcome.progress.reached() >= route.node_count() * 3,
        "the monotonic count must keep climbing across laps, got {}",
        outcome.progress.reached()
    );

    // The live target really does re-arm: `next_index` wraps back to 0 while
    // the monotonic count keeps rising. A loop followed as an end would drive
    // `next_index` past the list instead and never reach a second lap.
    let mut wraps = 0u32;
    let mut previous_reached = 0;
    let mut previous_next = 0;
    let mut previous_laps = 0;
    for decision in &outcome.decisions {
        let progress = decision.decision.progress;
        assert!(
            progress.reached() == previous_reached || progress.reached() == previous_reached + 1,
            "progress jumped from {previous_reached} to {}",
            progress.reached()
        );
        if progress.next_index() < previous_next {
            assert_eq!(
                progress.next_index(),
                0,
                "only the wrap edge may re-target node 0"
            );
            assert_eq!(
                progress.laps(),
                previous_laps + 1,
                "each wrap records exactly one more lap"
            );
            wraps += 1;
        }
        previous_reached = progress.reached();
        previous_next = progress.next_index();
        previous_laps = progress.laps();
    }
    assert!(
        wraps >= 3,
        "the run must wrap at least three times, got {wraps}"
    );
}

/// Every node, mandatory ones included, is re-targeted on every lap, and the
/// run visits each authored node id at least twice. A loop that remembered
/// "already fired" instead of re-arming would visit each id once.
#[test]
fn accept_t447_loop_route_rearms_every_node_including_mandatory_markers() {
    let route = synthetic_loop_route();
    let actor = actor(1);
    let mut set = loop_set(actor);
    let outcome = follow_route(
        &mut set,
        actor,
        FollowPlan {
            route: &route,
            blockers: &[],
            dt_s: SYNTHETIC_PURSUIT_DT_S,
            start: start_at_origin(),
            max_ticks: 20_000,
        },
        |_| ReferenceFrameSample::IDENTITY,
    )
    .expect("the loop follow request is valid");

    let mut visits = vec![0usize; route.node_count()];
    for decision in &outcome.decisions {
        if let Some(target) = decision.decision.target {
            let index = route
                .nodes
                .iter()
                .position(|node| node.id == target)
                .expect("a decision target is a node of this route");
            visits[index] += 1;
        }
    }
    for (index, count) in visits.iter().enumerate() {
        assert!(
            *count >= 2,
            "route node {index} ({:?}) was targeted {count} times across the laps, expected a re-arm per lap",
            route.nodes[index].id
        );
    }
    // Every mandatory marker really was reached on a later lap, which is what
    // "re-armed" means for a marker that a lap may not skip.
    assert!(
        route.nodes.iter().any(|node| node.mandatory),
        "the loop fixture must exercise a mandatory marker"
    );
    let mandatory_id = route
        .nodes
        .iter()
        .find(|node| node.mandatory)
        .expect("the fixture has a mandatory node")
        .id;
    let mandatory_targets = outcome
        .decisions
        .iter()
        .filter(|decision| decision.decision.target == Some(mandatory_id))
        .count();
    assert!(
        mandatory_targets >= 2,
        "the mandatory marker must be targeted again on a later lap, got {mandatory_targets}"
    );
}

/// The wrap edge is the ordinary last-to-first edge: arrival at node 0 after a
/// wrap is tested against node 0's own authored `arrival_radius_m` and nothing
/// else. This builds a loop whose first node has a wide radius while the rest
/// are tight, then shows the wrap edge is entered with that wide radius in
/// force — and that the same route with a tight node 0 is followed identically
/// in *target* terms, so the wrap edge is not a special-cased shortcut.
#[test]
fn accept_t447_loop_wrap_edge_uses_the_first_nodes_own_arrival_radius() {
    let route = synthetic_loop_route();
    let node0_radius = route.nodes[0].arrival_radius_m;
    assert!(
        node0_radius > route.nodes[1].arrival_radius_m,
        "the fixture's first node must declare the widest radius for this probe to discriminate"
    );

    let actor = actor(1);
    let mut set = loop_set(actor);
    let outcome = follow_route(
        &mut set,
        actor,
        FollowPlan {
            route: &route,
            blockers: &[],
            dt_s: SYNTHETIC_PURSUIT_DT_S,
            start: start_at_origin(),
            max_ticks: 20_000,
        },
        |_| ReferenceFrameSample::IDENTITY,
    )
    .expect("the loop follow request is valid");

    // The wrap edge is the ordinary last-to-first edge, so re-arriving at node 0
    // after a wrap is the same swept test as arriving at any other node: the
    // committed segment crosses node 0's *own* authored radius.
    //
    // This discriminates which radius the wrap edge is tested against. A wrap
    // tested against node 1's or node 2's tighter radius (6-8 m) would not fire
    // until the follower was already well inside it; a wrap that invented its
    // own volume, or one that teleported onto the node, would fire from far
    // away or without flying the leg at all.
    let mut arrivals = Vec::new();
    let mut previous_reached = 0;
    for decision in &outcome.decisions {
        let progress = decision.decision.progress;
        let arrived_at_node_0 = decision.decision.target == Some(RouteNodeId(0))
            && progress.reached() > previous_reached;
        if arrived_at_node_0 && progress.laps() > 0 {
            let step = &decision.decision.step;
            let distance_from = ((step.from_m[0] * step.from_m[0])
                + (step.from_m[1] * step.from_m[1])
                + (step.from_m[2] * step.from_m[2]))
                .sqrt();
            let distance_to = ((step.to_m[0] * step.to_m[0])
                + (step.to_m[1] * step.to_m[1])
                + (step.to_m[2] * step.to_m[2]))
                .sqrt();
            arrivals.push((distance_from, distance_to));
        }
        previous_reached = progress.reached();
    }
    assert!(
        arrivals.len() >= 2,
        "node 0 must be arrived at again on later laps, got {} arrivals",
        arrivals.len()
    );
    for (distance_from, distance_to) in &arrivals {
        // The arrival fired because *this* segment entered node 0's radius.
        assert!(
            *distance_to < node0_radius,
            "arrival at node 0 after a wrap must be a swept crossing of node 0's \
             own {node0_radius} m radius, but the committed segment ended \
             {distance_to} m out"
        );
        // ...and it fired on this tick, so the step began just outside the
        // radius. The slack is one tick of travel, and is still far below the
        // ~177 m the follower is from node 0 on the wrap leg.
        assert!(
            *distance_from > node0_radius - 1.0 && *distance_from < node0_radius + 2.0,
            "arrival at node 0 must fire on the first tick whose swept segment \
             crosses node 0's own {node0_radius} m radius, so the step must start \
             just outside it (within one tick of travel); it started \
             {distance_from} m out"
        );
        assert!(
            *distance_from > 0.5,
            "the wrap edge must be flown in from off-node, not assumed on arrival; \
             the step started {distance_from} m out"
        );
    }

    // The wrap itself is a real leg: the decision that re-armed the target to
    // node 0 was taken at the last node the follower reached, not at node 0 and
    // not somewhere in between.
    let wrap = outcome
        .decisions
        .iter()
        .find(|decision| {
            decision.decision.progress.laps() > 0 && decision.decision.progress.next_index() == 0
        })
        .expect("the run contains a decision whose target re-armed to node 0");
    let corner = route.nodes[route.node_count() - 1].position_m;
    let wrap_step = &wrap.decision.step;
    let distance_from_corner = ((wrap_step.from_m[0] - corner[0]).powi(2)
        + (wrap_step.from_m[1] - corner[1]).powi(2)
        + (wrap_step.from_m[2] - corner[2]).powi(2))
    .sqrt();
    assert!(
        distance_from_corner < 20.0,
        "the wrap must be recorded at the last node the follower reached, not \
         elsewhere; it was {distance_from_corner} m from the last node"
    );
}

/// A loop route the runtime cannot honestly follow is refused by name at the
/// graph boundary rather than producing a follower that can never advance.
#[test]
fn accept_t447_a_one_node_loop_is_refused_by_name() {
    let one_node = || {
        vec![RouteNode {
            id: RouteNodeId(0),
            sequence: 0,
            mandatory: true,
            position_m: [0.0, 0.0, 0.0],
            arrival_radius_m: 4.0,
        }]
    };
    assert_eq!(
        RouteGraph::try_new_terminated(RouteFrame::World, RouteTermination::Loop, 0.0, one_node()),
        Err(RouteGraphError::LoopNeedsMultipleNodes { nodes: 1 })
    );
    // The same node count on an ending route is fine: the rule is about
    // looping, not about one node.
    assert!(
        RouteGraph::try_new(RouteFrame::World, 0.0, one_node()).is_ok(),
        "an ending route may legitimately declare a single node"
    );
}

/// A resume count beyond the node list is a caller error, and the runtime must
/// survive it on a loop exactly as it does on an ending route: hold station,
/// report no target, and never index off the node list.
///
/// [`NavigationSet::register_resuming`] is public and takes no route, so it
/// cannot check the count against the node list. This is the one place a loop
/// could run its target off the end (`next_index` past the last node), because a
/// followed loop wraps before it can: the bound has to be checked in
/// [`RouteProgress::is_complete`] for every termination, not only for
/// [`RouteTermination::End`]. A `loop` that short-circuited that bound to
/// `false` panics here with `index out of bounds` instead of holding station.
#[test]
fn accept_t447_a_resume_past_the_end_of_a_loop_holds_station_instead_of_panicking() {
    let route = synthetic_loop_route();
    let actor = actor(1);
    // `register_resuming` is the production seeding API and validates nothing
    // against the route, so an over-counted resume is reachable.
    let mut set = empty_set();
    set.register_resuming(actor, route.node_count())
        .expect("a resume count is not refused at registration");

    let decision = set
        .decide(&PursuitRequest {
            actor,
            tick: Tick(0),
            generation: set.session(),
            state: start_at_origin(),
            route: &route,
            frame: ReferenceFrameSample::IDENTITY,
            blockers: &[],
            dt_s: SYNTHETIC_PURSUIT_DT_S,
        })
        .expect("an over-counted resume is a decision, not a panic");
    assert_eq!(
        decision.decision.avoidance,
        AvoidanceState::Arrived,
        "a progress seeded past the node list holds station"
    );
    assert_eq!(
        decision.decision.target, None,
        "there is no node left to target, so none is named"
    );
    assert!(
        decision.state.is_complete(&route),
        "the same bound answers completion on an ending route, so it answers it here"
    );
    // The same seeded state is still monotone and untouched: holding station is
    // not a rewind and not a teleport (spec non-negotiable behavior 4).
    assert_eq!(decision.decision.progress.reached(), route.node_count());
    assert_eq!(decision.decision.progress.laps(), 0);
}
