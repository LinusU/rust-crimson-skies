//! Task #761: the command the cascade emits is the step the follower
//! **committed**, not the straight line at the target.
//!
//! Spec: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`,
//! non-negotiable behavior 2 (avoidance bounded by the envelope, rejoining
//! without skipping markers); shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`. Task test prefix: `accept_t761_`.
//!
//! Before this task the cascade was fed the *target's* heading error: a
//! `Deviating` step bounded away from a blocker while the commanded bank still
//! chased the target — the kinematic plan deviated and the body never
//! followed — and a `Blocked` stationary step was told to hold whatever bank
//! the aircraft happened to have, so a banked aircraft kept turning while its
//! throttle decayed. These tests pin the two behaviours the task's integrated
//! acceptance case flies through the Avian loop:
//!
//! * a `Deviating` decision banks the body the way the committed step points;
//! * a `Blocked` decision commands wings-level and no climb, whatever bank and
//!   climb the state carries.
//!
//! Everything drives the production [`Navigator`] through the same
//! [`NavigationRequest`] the mission loop builds; every value is designed
//! synthetic fixture data — no original game data.

use cs_sim::ai::navigation::{
    AvoidanceState, Blocker, NavState, NavigationCadence, NavigationError, NavigationRequest,
    Navigator, ReferenceFrameSample, RouteGraph, RouteProgress, SYNTHETIC_PURSUIT_DT_S,
    synthetic_maneuver_envelope, synthetic_pursuit_route, synthetic_pursuit_start,
    synthetic_pursuit_tie_blocker, wrap_pi,
};
use cs_types::Tick;

fn navigator() -> Navigator {
    Navigator::new(
        synthetic_maneuver_envelope(),
        NavigationCadence::designed_default(),
    )
    .expect("the synthetic envelope and cadence are valid")
}

fn request<'a>(
    state: NavState,
    route: &'a RouteGraph,
    blockers: &'a [Blocker],
) -> NavigationRequest<'a> {
    NavigationRequest {
        tick: Tick(0),
        generation: 1,
        state,
        route,
        progress: RouteProgress::reached_nodes(1),
        frame: ReferenceFrameSample::IDENTITY,
        blockers,
        dt_s: SYNTHETIC_PURSUIT_DT_S,
    }
}

/// A `Deviating` decision banks the airframe the way the committed step
/// points — measured by the roll the cascade emits — so the body follows the
/// bounded dodge the plan cleared instead of the target's straight line.
///
/// The marker sits dead ahead of the fixture start, so the *target's* heading
/// error is zero: under the pre-#761 law the emitted roll would be exactly
/// `0.0` on both legs, because the target's error is all the cascade saw.
#[test]
fn accept_t761_a_deviating_decision_banks_the_body_the_committed_way() {
    let route = synthetic_pursuit_route();
    let blockers = [synthetic_pursuit_tie_blocker()];
    let state = synthetic_pursuit_start();
    assert_eq!(
        state.heading_rad, 0.0,
        "the fixture start points straight at the mandatory marker"
    );
    let navigator = navigator();

    let left = navigator
        .decide_with_tie_break(&request(state, &route, &blockers), 0.0)
        .expect("valid request");
    let right = navigator
        .decide_with_tie_break(&request(state, &route, &blockers), 1.0)
        .expect("valid request");
    assert_eq!(left.avoidance, AvoidanceState::Deviating);
    assert_eq!(right.avoidance, AvoidanceState::Deviating);

    let left_committed = wrap_pi(left.step.heading_rad - state.heading_rad);
    let right_committed = wrap_pi(right.step.heading_rad - state.heading_rad);
    assert!(
        left_committed > 0.0,
        "the draw-0.0 deviation commits nose-left: {left_committed}"
    );
    assert!(
        right_committed < 0.0,
        "the draw-1.0 deviation commits nose-right: {right_committed}"
    );

    // `FlightInput.roll` is positive right-wing-down
    // (docs/contracts/FLIGHT-PHYSICS.md): a nose-left committed step banks
    // left-wing-down, a nose-right one right-wing-down. The pre-#761 law fed
    // the cascade the target's error — zero on this fixture — so both rolls
    // came out exactly 0.0.
    assert!(
        left.command.roll < 0.0,
        "a nose-left committed step must bank the body left: roll {}",
        left.command.roll
    );
    assert!(
        right.command.roll > 0.0,
        "a nose-right committed step must bank the body right: roll {}",
        right.command.roll
    );
    assert!(left.command.roll.abs() <= 1.0);
    assert!(right.command.roll.abs() <= 1.0);
}

/// A `Blocked` hold commands the aircraft toward wings-level and no climb
/// rather than holding whatever bank and climb it happened to carry: a hold
/// is a hold, and a parked aircraft that kept its bank would keep turning
/// while its throttle decayed (the blocked behaviour this task pins).
///
/// The enclosing sphere leaves no bounded step that clears, so every
/// deviation fails and the follower holds. The banked, sinking state is what
/// the pre-#761 law answered with `roll == 0.0` and `pitch == 0.0` — holding
/// the bank and the sink exactly.
#[test]
fn accept_t761_a_blocked_hold_rolls_level_and_damps_the_climb() {
    let route = synthetic_pursuit_route();
    // A 20 m sphere centred on the aircraft swallows every bounded step a
    // ~0.33 m tick can make, so the follower can only hold.
    let blockers = [Blocker::sphere([0.0, 0.0, 0.0], 20.0)];
    let navigator = navigator();

    for (bank_rad, climb_mps) in [(-0.5, -4.0), (0.4, 3.0)] {
        let state = NavState {
            bank_rad,
            climb_mps,
            ..synthetic_pursuit_start()
        };
        let decision = navigator
            .decide(&request(state, &route, &blockers))
            .expect("valid request");
        assert_eq!(
            decision.avoidance,
            AvoidanceState::Blocked,
            "the enclosing sphere leaves nothing to commit"
        );
        assert_eq!(
            decision.step.speed_mps, 0.0,
            "the committed step is the stationary hold"
        );
        assert_eq!(
            decision.command.throttle, 0.0,
            "the hold decays the throttle"
        );
        assert_eq!(
            decision.command.roll.signum(),
            -bank_rad.signum(),
            "bank {bank_rad}: the hold must roll toward level, roll {}",
            decision.command.roll
        );
        assert_eq!(
            decision.command.pitch.signum(),
            -climb_mps.signum(),
            "climb {climb_mps}: the hold must damp the climb, pitch {}",
            decision.command.pitch
        );
    }

    // A level aircraft in the same hold stays neutral: the decision asks for
    // nothing it does not need.
    let level = navigator
        .decide(&request(synthetic_pursuit_start(), &route, &blockers))
        .expect("valid request");
    assert_eq!(level.avoidance, AvoidanceState::Blocked);
    assert_eq!(level.command.roll, 0.0);
    assert_eq!(level.command.pitch, 0.0);
    assert_eq!(level.command.throttle, 0.0);
}

/// The malformed path stays refused by name: a blocker with a non-finite
/// centre is a request error, never a decision.
#[test]
fn accept_t761_a_corrupt_blocker_is_still_refused_by_name() {
    let route = synthetic_pursuit_route();
    let blockers = [Blocker::sphere([f64::NAN, 0.0, -1.0], 1.0)];
    assert!(matches!(
        navigator().decide(&request(synthetic_pursuit_start(), &route, &blockers)),
        Err(NavigationError::Blocker { index: 0, .. })
    ));
}
