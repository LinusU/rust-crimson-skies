//! Acceptance scenarios F31-B for the stateful pursuit path: the minimum
//! scenario (reorder the actors and the same seed replays each actor's local
//! decision sequence), the per-actor seeded tie-break, the remembered
//! deviation side, bounded avoidance and the retained F31-A criteria (a
//! narrow-arch route without a wall crossing, a displaced actor rejoining and
//! an origin shift that neither resets progress nor fires arrival).
//!
//! Spec: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, stage
//! `### F31-B`. Task test prefix: `accept_f31_b_`.
//!
//! Every test drives production code only: [`cs_sim::ai::navigation`]'s
//! [`NavigationSet`], [`PursuitState`], the seeded [`tie_break_draw`] and the
//! F31-A [`Navigator`] beneath them. Removing the set, dropping the per-actor
//! seed, forgetting the remembered side or making an origin shift touch
//! progress each makes one of them fail.
//!
//! The "ECS entities" of the minimum scenario are the actor observations the
//! set is handed for one tick: [`NavigationSet::decide`] names exactly one
//! actor and [`NavigationSet::decide_all`] sorts by stable actor id, so the
//! order the ECS would present them in cannot reach a decision. Every value
//! here is newly authored synthetic fixture data, never original game data.

use std::collections::{BTreeMap, BTreeSet};

use cs_sim::ai::navigation::{
    AvoidanceState, Blocker, NavState, NavigationCadence, NavigationError, NavigationSet,
    Navigator, PursuitDecision, PursuitRequest, ReferenceFrameSample, RouteFrame, RouteGraph,
    RouteNode, RouteNodeId, RouteProgress, SYNTHETIC_PURSUIT_SEED, SYNTHETIC_PURSUIT_SESSION,
    heading_from_direction, synthetic_arch_blockers, synthetic_arch_route, synthetic_arch_start,
    synthetic_maneuver_envelope, synthetic_pursuit_actor, synthetic_pursuit_route,
    synthetic_pursuit_set, synthetic_pursuit_start, synthetic_pursuit_tie_blocker, tie_break_draw,
};
use cs_sim::damage::ActorId;
use cs_types::Tick;

const DT_S: f64 = 1.0 / 60.0;

fn actor(serial: u64) -> ActorId {
    synthetic_pursuit_actor(serial)
}

fn navigator() -> Navigator {
    Navigator::new(
        synthetic_maneuver_envelope(),
        NavigationCadence::designed_default(),
    )
    .expect("the synthetic envelope and cadence are valid")
}

fn single_node_route(position_m: [f64; 3], arrival_radius_m: f64) -> RouteGraph {
    RouteGraph {
        frame: RouteFrame::World,
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

/// Runs one actor for up to `ticks` decisions, advancing its state from each
/// committed step (the kinematic closure F31-A's probe uses). Stops early when
/// the route completes.
fn run_single(
    set: &mut NavigationSet,
    actor: ActorId,
    mut state: NavState,
    route: &RouteGraph,
    frame: ReferenceFrameSample,
    blockers: &[Blocker],
    ticks: usize,
) -> Vec<PursuitDecision> {
    let mut decisions = Vec::new();
    for tick in 0..ticks {
        let request = PursuitRequest {
            actor,
            tick: Tick(tick as u64),
            generation: set.session(),
            state,
            route,
            frame,
            blockers,
            dt_s: DT_S,
        };
        let decision = set.decide(&request).expect("every request is valid");
        state.position_m = decision.decision.step.to_m;
        state.heading_rad = decision.decision.step.heading_rad;
        state.speed_mps = decision.decision.step.speed_mps;
        state.climb_mps = decision.decision.step.climb_mps;
        let complete = decision.state.is_complete(route);
        decisions.push(decision);
        if complete {
            break;
        }
    }
    decisions
}

/// One actor observation for a tick.
fn observation<'a>(
    actor: ActorId,
    tick: u64,
    state: NavState,
    route: &'a RouteGraph,
    blockers: &'a [Blocker],
) -> PursuitRequest<'a> {
    PursuitRequest {
        actor,
        tick: Tick(tick),
        generation: SYNTHETIC_PURSUIT_SESSION,
        state,
        route,
        frame: ReferenceFrameSample::IDENTITY,
        blockers,
        dt_s: DT_S,
    }
}

/// Drives a whole roster for `ticks` ticks, presenting the actors in `order`
/// each tick and returning each actor's decision sequence. The state of every
/// actor is advanced from its own decision.
fn drive(
    set: &mut NavigationSet,
    order: &[ActorId],
    route: &RouteGraph,
    blockers: &[Blocker],
    ticks: usize,
) -> BTreeMap<ActorId, Vec<PursuitDecision>> {
    let mut states: BTreeMap<ActorId, NavState> = order
        .iter()
        .map(|actor| (*actor, synthetic_pursuit_start()))
        .collect();
    let mut sequences: BTreeMap<ActorId, Vec<PursuitDecision>> = BTreeMap::new();
    for tick in 0..ticks {
        let requests: Vec<PursuitRequest<'_>> = order
            .iter()
            .map(|&actor| observation(actor, tick as u64, states[&actor], route, blockers))
            .collect();
        for decision in set.decide_all(&requests).expect("every request is valid") {
            let state = states
                .get_mut(&decision.actor)
                .expect("every actor is known");
            state.position_m = decision.decision.step.to_m;
            state.heading_rad = decision.decision.step.heading_rad;
            state.speed_mps = decision.decision.step.speed_mps;
            state.climb_mps = decision.decision.step.climb_mps;
            sequences.entry(decision.actor).or_default().push(decision);
        }
    }
    sequences
}

/// AC02 minimum scenario: the same three actors presented to the set in two
/// different orders — and registered in two different orders — produce an
/// identical decision sequence per actor. The seeded tie-break is genuinely
/// exercised: at least one actor has to deviate around the centred blocker.
#[test]
fn accept_f31_b_reordering_actors_keeps_each_actors_decision_sequence() {
    let route = synthetic_pursuit_route();
    let blockers = [synthetic_pursuit_tie_blocker()];
    let actors = [actor(1), actor(2), actor(3)];

    let mut forward_set = synthetic_pursuit_set(3);
    let forward = drive(
        &mut forward_set,
        &[actor(1), actor(2), actor(3)],
        &route,
        &blockers,
        400,
    );
    let mut reordered_set = synthetic_pursuit_set(3);
    let reordered = drive(
        &mut reordered_set,
        &[actor(3), actor(1), actor(2)],
        &route,
        &blockers,
        400,
    );

    for actor in actors {
        let expected = forward.get(&actor).expect("the actor was driven");
        let got = reordered.get(&actor).expect("the actor was driven");
        assert_eq!(
            expected, got,
            "actor {actor} must decide identically however the ECS ordered the actors"
        );
        assert!(!expected.is_empty());
    }

    assert!(
        forward
            .values()
            .flatten()
            .any(|decision| decision.decision.avoidance == AvoidanceState::Deviating),
        "the fixture must actually exercise the tie-break deviation"
    );
}

/// The set's decisions are emitted in ascending stable actor id, whatever
/// order the observations arrived in, and a repeated actor is refused rather
/// than stepped twice.
#[test]
fn accept_f31_b_decide_all_is_stable_in_actor_id_order() {
    let route = synthetic_pursuit_route();
    let blockers = [synthetic_pursuit_tie_blocker()];
    let mut set = synthetic_pursuit_set(3);

    let mut expected = vec![actor(1), actor(2), actor(3)];
    expected.sort();

    for order in [
        [actor(1), actor(2), actor(3)],
        [actor(3), actor(1), actor(2)],
        [actor(2), actor(3), actor(1)],
    ] {
        let requests: Vec<PursuitRequest<'_>> = order
            .iter()
            .map(|&actor| observation(actor, 0, synthetic_pursuit_start(), &route, &blockers))
            .collect();
        let actors: Vec<ActorId> = set
            .decide_all(&requests)
            .expect("valid requests")
            .into_iter()
            .map(|decision| decision.actor)
            .collect();
        assert_eq!(actors, expected);
    }

    let repeated = [actor(1), actor(1)];
    let requests: Vec<PursuitRequest<'_>> = repeated
        .iter()
        .map(|&actor| observation(actor, 1, synthetic_pursuit_start(), &route, &blockers))
        .collect();
    assert_eq!(
        set.decide_all(&requests),
        Err(NavigationError::DuplicateActor { actor: actor(1) })
    );
}

/// The same mission seed replays an identical sequence; the seed can move the
/// tie-break side; and the draw is a pure function of `(seed, actor, tick)`.
#[test]
fn accept_f31_b_same_seed_replays_and_a_different_seed_moves_the_tie_break() {
    let route = synthetic_pursuit_route();
    let blockers = [synthetic_pursuit_tie_blocker()];
    let actor = actor(1);

    let run = |seed: u64| {
        let mut set = NavigationSet::new(SYNTHETIC_PURSUIT_SESSION, seed, navigator());
        set.register_resuming(actor, 1)
            .expect("the actor registers");
        run_single(
            &mut set,
            actor,
            synthetic_pursuit_start(),
            &route,
            ReferenceFrameSample::IDENTITY,
            &blockers,
            4,
        )
    };

    let first = run(SYNTHETIC_PURSUIT_SEED);
    let again = run(SYNTHETIC_PURSUIT_SEED);
    assert_eq!(first, again, "the same seed must replay identically");
    assert_eq!(first[0].decision.avoidance, AvoidanceState::Deviating);
    assert_eq!(
        first[0].deviation(),
        first[0].state.deviation(),
        "the decider reports the state's committed side"
    );

    let mut sides = BTreeSet::new();
    for step in 0..16u64 {
        let seed = SYNTHETIC_PURSUIT_SEED ^ (step.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let decision = run(seed);
        assert_eq!(decision[0].decision.avoidance, AvoidanceState::Deviating);
        sides.insert(format!("{:?}", decision[0].deviation()));
    }
    assert!(
        sides.len() >= 2,
        "the mission seed must be able to choose either tie-break side, got {sides:?}"
    );

    let set = synthetic_pursuit_set(1);
    for tick in 0..32 {
        let draw = set.tie_break(actor, Tick(tick));
        assert_eq!(
            draw,
            tie_break_draw(SYNTHETIC_PURSUIT_SEED, actor, Tick(tick))
        );
        assert!((0.0..1.0).contains(&draw));
    }
}

/// The remembered deviation side holds across consecutive deviating ticks: at
/// two ticks whose seeded draws fall on opposite sides of `0.5`, the follower
/// still commits to the same side because it is remembering, not redrawing.
#[test]
fn accept_f31_b_deviation_side_persists_across_consecutive_deviation_ticks() {
    let route = synthetic_pursuit_route();
    let blockers = [synthetic_pursuit_tie_blocker()];
    let actor = actor(1);
    let mut set = synthetic_pursuit_set(1);
    let start = synthetic_pursuit_start();

    let mut low = None;
    let mut high = None;
    for tick in 0..256u64 {
        let draw = set.tie_break(actor, Tick(tick));
        if draw < 0.5 && low.is_none() {
            low = Some(tick);
        }
        if draw >= 0.5 && high.is_none() {
            high = Some(tick);
        }
        if low.is_some() && high.is_some() {
            break;
        }
    }
    let (low, high) = (
        low.expect("a below-half draw exists in the first 256 ticks"),
        high.expect("an at-or-above-half draw exists in the first 256 ticks"),
    );

    let first = set
        .decide(&observation(actor, low, start, &route, &blockers))
        .expect("valid request");
    assert_eq!(first.decision.avoidance, AvoidanceState::Deviating);
    let side = first.deviation().expect("a deviation has a side");

    // The identical geometry at a tick that would draw the other side must
    // still commit to the remembered side.
    let second = set
        .decide(&observation(actor, high, start, &route, &blockers))
        .expect("valid request");
    assert_eq!(second.decision.avoidance, AvoidanceState::Deviating);
    assert_eq!(
        second.deviation(),
        Some(side),
        "a contiguous deviation must hold its side instead of flipping"
    );
}

/// A blocked step is never issued: the set holds position, commands neutral,
/// keeps progress and counts the stall rather than crossing or teleporting.
#[test]
fn accept_f31_b_blocked_wall_holds_without_crossing_or_teleporting() {
    let route = single_node_route([100.0, 0.0, 0.0], 5.0);
    let wall = Blocker::axis_aligned_box([10.0, 0.0, 0.0], [9.9, 50.0, 50.0]);
    let blockers = [wall];
    let actor = actor(1);
    let mut set = NavigationSet::new(
        SYNTHETIC_PURSUIT_SESSION,
        SYNTHETIC_PURSUIT_SEED,
        navigator(),
    );
    set.register(actor).expect("the actor registers");
    let state = NavState {
        position_m: [0.0, 0.0, 0.0],
        heading_rad: heading_from_direction(1.0, 0.0),
        speed_mps: 40.0,
        climb_mps: 0.0,
    };

    let decision = set
        .decide(&observation(actor, 0, state, &route, &blockers))
        .expect("valid request");

    assert_eq!(decision.decision.avoidance, AvoidanceState::Blocked);
    assert_eq!(
        decision.decision.step.from_m, decision.decision.step.to_m,
        "a blocked follower holds position"
    );
    assert_eq!(decision.decision.progress.reached(), 0);
    assert_eq!(decision.state.stalled_ticks(), 1);
    assert_eq!(
        decision.decision.command,
        cs_sim::flight::FlightInput::NEUTRAL
    );
    assert!(
        !blockers.iter().any(|blocker| blocker
            .segment_intersects(decision.decision.step.from_m, decision.decision.step.to_m)),
        "the held position must not touch the wall"
    );
}

/// Parent AC01 retained through the set: the actor follows the narrow arch
/// without a single committed segment crossing the wall or a blocked hold.
#[test]
fn accept_f31_b_actor_follows_the_arch_without_crossing_the_wall() {
    let route = synthetic_arch_route();
    let blockers = synthetic_arch_blockers();
    let actor = actor(1);
    let mut set = synthetic_pursuit_set(1);

    let decisions = run_single(
        &mut set,
        actor,
        synthetic_arch_start(),
        &route,
        ReferenceFrameSample::IDENTITY,
        &blockers,
        4000,
    );

    assert!(
        decisions.len() > 100,
        "the route is long enough to be a real traversal, got {}",
        decisions.len()
    );
    let mut previous = RouteProgress::reached_nodes(1).reached();
    for decision in &decisions {
        assert!(
            !blockers.iter().any(|blocker| blocker
                .segment_intersects(decision.decision.step.from_m, decision.decision.step.to_m)),
            "tick {} crossed a blocker",
            decision.decision.tick.0
        );
        let reached = decision.decision.progress.reached();
        assert!(
            reached == previous || reached == previous + 1,
            "progress jumped from {previous} to {reached}"
        );
        previous = reached;
    }
    assert!(
        set.state(actor).expect("registered").is_complete(&route),
        "the actor completes the arch route"
    );
}

/// Parent AC03 retained through the set: an actor displaced off route and
/// pointed away rejoins the route and reaches the next mandatory marker
/// without skipping one.
#[test]
fn accept_f31_b_displaced_actor_rejoins_before_the_next_mandatory_marker() {
    let route = synthetic_pursuit_route();
    let actor = actor(1);
    let mut set = synthetic_pursuit_set(1);
    let displaced = NavState {
        position_m: [40.0, 0.0, -20.0],
        heading_rad: heading_from_direction(0.0, 1.0), // nose +Z, away from the route
        speed_mps: 40.0,
        climb_mps: 0.0,
    };

    let decisions = run_single(
        &mut set,
        actor,
        displaced,
        &route,
        ReferenceFrameSample::IDENTITY,
        &[],
        2000,
    );

    let reached = set.state(actor).expect("registered").progress().reached();
    assert!(
        reached >= 2,
        "the actor must rejoin past the first mandatory marker, reached {reached}"
    );
    let mut previous = 1;
    for decision in &decisions {
        let value = decision.decision.progress.reached();
        assert!(value == previous || value == previous + 1);
        previous = value;
    }
}

/// Parent AC04 retained through the set: rebasing the world — translating the
/// state and the frame origin by the same offset — neither resets progress nor
/// fires a false arrival, and the step translates with it.
#[test]
fn accept_f31_b_origin_shift_does_not_reset_progress_or_fire_arrival() {
    let route = synthetic_pursuit_route();
    let actor = actor(1);
    let mut set = synthetic_pursuit_set(1);

    let decisions = run_single(
        &mut set,
        actor,
        synthetic_pursuit_start(),
        &route,
        ReferenceFrameSample::IDENTITY,
        &[],
        3,
    );
    let before = decisions.last().expect("three ticks ran");
    let progress = before.decision.progress.reached();
    let state_before = NavState {
        position_m: before.decision.step.to_m,
        heading_rad: before.decision.step.heading_rad,
        speed_mps: before.decision.step.speed_mps,
        climb_mps: before.decision.step.climb_mps,
    };

    let shift = [1000.0, -250.0, 4000.0];
    let shifted_state = NavState {
        position_m: [
            state_before.position_m[0] + shift[0],
            state_before.position_m[1] + shift[1],
            state_before.position_m[2] + shift[2],
        ],
        ..state_before
    };
    let frame = ReferenceFrameSample {
        origin_m: shift,
        yaw_rad: 0.0,
    };
    let request = PursuitRequest {
        actor,
        tick: Tick(3),
        generation: SYNTHETIC_PURSUIT_SESSION,
        state: shifted_state,
        route: &route,
        frame,
        blockers: &[],
        dt_s: DT_S,
    };
    let decision = set.decide(&request).expect("valid request");

    assert_ne!(
        decision.decision.avoidance,
        AvoidanceState::Arrived,
        "an origin shift is not an arrival"
    );
    assert_eq!(
        decision.decision.progress.reached(),
        progress,
        "an origin shift must not reset or advance progress"
    );
    for (axis, offset) in shift.iter().enumerate() {
        assert!(
            (state_before.position_m[axis] + offset - decision.decision.step.from_m[axis]).abs()
                < 1e-9,
            "the shifted step must translate with the world, axis {axis}"
        );
    }
}

/// Parent AC04's moving-waypoint half: a route in a moving frame whose origin
/// jumps between ticks neither resets the progress the set owns nor fires a
/// false arrival, and the follower re-targets the next mandatory marker at its
/// new world pose.
#[test]
fn accept_f31_b_moving_waypoint_does_not_reset_progress_or_fire_false_arrival() {
    let route = RouteGraph {
        frame: RouteFrame::Moving { anchor: 7 },
        clearance_m: 0.0,
        nodes: vec![
            RouteNode {
                id: RouteNodeId(0),
                sequence: 0,
                mandatory: true,
                position_m: [0.0, 0.0, -40.0],
                arrival_radius_m: 4.0,
            },
            RouteNode {
                id: RouteNodeId(1),
                sequence: 1,
                mandatory: true,
                position_m: [0.0, 0.0, -120.0],
                arrival_radius_m: 4.0,
            },
        ],
    };
    let actor = actor(1);
    let mut set = NavigationSet::new(
        SYNTHETIC_PURSUIT_SESSION,
        SYNTHETIC_PURSUIT_SEED,
        navigator(),
    );
    set.register(actor).expect("the actor registers");
    let mut state = synthetic_pursuit_start();
    let mut frame = ReferenceFrameSample::IDENTITY;

    let step =
        |set: &mut NavigationSet, tick: u64, state: &mut NavState, frame: ReferenceFrameSample| {
            let decision = set
                .decide(&PursuitRequest {
                    actor,
                    tick: Tick(tick),
                    generation: SYNTHETIC_PURSUIT_SESSION,
                    state: *state,
                    route: &route,
                    frame,
                    blockers: &[],
                    dt_s: DT_S,
                })
                .expect("valid request");
            *state = NavState {
                position_m: decision.decision.step.to_m,
                heading_rad: decision.decision.step.heading_rad,
                speed_mps: decision.decision.step.speed_mps,
                climb_mps: decision.decision.step.climb_mps,
            };
            decision
        };

    // A frame origin that leaves the aircraft exactly at the node's *local*
    // position must not be mistaken for an arrival at the node's world pose.
    let mut local_state = synthetic_pursuit_start();
    local_state.position_m = [0.0, 0.0, -40.0];
    let displaced = step(
        &mut set,
        0,
        &mut local_state,
        ReferenceFrameSample {
            origin_m: [1000.0, 0.0, 0.0],
            yaw_rad: 0.0,
        },
    );
    assert_ne!(
        displaced.decision.avoidance,
        AvoidanceState::Arrived,
        "a displaced frame must not report arrival at the node's local position"
    );
    assert_eq!(displaced.decision.progress.reached(), 0);
    assert_eq!(displaced.decision.target, Some(RouteNodeId(0)));

    // Fly to the first mandatory marker at its initial world pose.
    let mut tick = 0;
    while set.state(actor).expect("registered").progress().reached() == 0 {
        step(&mut set, tick, &mut state, frame);
        tick += 1;
        assert!(tick < 2000, "the first marker must be reachable");
    }
    assert_eq!(
        set.state(actor).expect("registered").progress().reached(),
        1
    );

    // The moving waypoint's frame jumps 200 m down the route between ticks.
    frame = ReferenceFrameSample {
        origin_m: [0.0, 0.0, -200.0],
        yaw_rad: 0.0,
    };
    let jumped = step(&mut set, tick, &mut state, frame);
    assert_eq!(
        jumped.decision.progress.reached(),
        1,
        "a moving waypoint must not reset the set's progress"
    );
    assert_ne!(
        jumped.decision.avoidance,
        AvoidanceState::Arrived,
        "a moving waypoint must not fire a false arrival"
    );
    assert_eq!(
        jumped.decision.target,
        Some(RouteNodeId(1)),
        "the follower targets the next mandatory marker at its new pose"
    );

    // It can still reach the marker at its new world pose.
    for _ in 0..4000 {
        let decision = step(&mut set, tick, &mut state, frame);
        tick += 1;
        if decision.decision.progress.reached() >= 2 {
            break;
        }
    }
    assert_eq!(
        set.state(actor).expect("registered").progress().reached(),
        2,
        "the follower reaches the moved mandatory marker"
    );
}
