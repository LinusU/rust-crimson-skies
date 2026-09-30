//! F23-D acceptance tests: the convergence, stability and high-speed contact
//! evidence.
//!
//! Spec: `specs/F23-avian-integration-collision-and-fixed-step-authority.md`,
//! stage `### F23-D`. Task test prefix: `accept_f23_d_`. Minimum scenario:
//! "Run 60/120/240 Hz convergence probes before freezing tolerances."
//!
//! Every test here drives the production probes in
//! [`cs_app::physics::evidence`], which in turn drive the production session,
//! the production force queue, the production body-creation path, the
//! production contact reporter and the production F24 flight driver. There is
//! no test-only physics: removing the adapter's force drain, the declared
//! substep policy, the swept spawn response or the flight driver makes one of
//! these fail.
//!
//! No original data and no `CS_GAME_DIR` access: a convergence probe on our own
//! integrator proves the interface and the schedule, never the original game.
//! Every value is newly authored synthetic fixture data.

use cs_app::physics::{
    BASELINE_FIXED_HZ, CONTACT_FACE_TOLERANCE_M, ContactScenario, ConvergenceScenario,
    DECLARED_SUBSTEP_COUNT, FROZEN_CONVERGENCE_BUDGETS, FROZEN_STABILITY_BUDGET, MAX_PROBE_TICKS,
    PROBE_RATES_HZ, PROBE_SPEEDS_M_S, ProbeError, SENSOR_MIN_SPEED_FRACTION,
    SPAWN_IN_HOLE_TRAVEL_TICKS, StabilityScenario, contact_probe, contact_sweep,
    convergence_evidence, convergence_probe, rate_spread_m_s, stability_probe,
};

/// Formats a failure list so a reviewer sees every defect, not just the first.
fn report<T: std::fmt::Display>(what: &str, found: &[T]) -> String {
    format!(
        "{what} ({}):\n  - {}",
        found.len(),
        found
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n  - ")
    )
}

/// AC04: the 60/120/240 Hz convergence probes run against the frozen
/// tolerances, and the position error halves as the rate doubles.
///
/// Observable failure if the implementation is removed: a force that is not
/// applied once per tick leaves the velocity error at `20 m/s` and the position
/// error at 60 m, both far outside the frozen budget; a force applied twice
/// doubles them; a wrong timestep stops the error from shrinking with the
/// rate and the observed order drops below 0.9.
#[test]
fn accept_f23_d_convergence_probes_meet_the_frozen_tolerances() {
    let evidence = convergence_evidence().expect("the declared scenario is runnable");

    assert_eq!(
        evidence.probes.len(),
        PROBE_RATES_HZ.len(),
        "AC04 requires a probe at every declared rate: {evidence:?}"
    );

    let mut found = Vec::new();
    for budget in FROZEN_CONVERGENCE_BUDGETS {
        let probe = evidence
            .probe(budget.fixed_hz)
            .unwrap_or_else(|| panic!("{budget:?} names a rate the evidence did not probe"));
        found.extend(budget.violations(probe));
        if budget.min_observed_order.is_some() {
            let measured = evidence.observed_order(budget.fixed_hz);
            assert!(
                measured.is_some(),
                "the convergence order from {} Hz is unmeasurable: {evidence:?}",
                budget.fixed_hz
            );
            found.extend(budget.order_violation(budget.fixed_hz, measured));
        }
    }
    assert!(
        found.is_empty(),
        "{}",
        report("the frozen convergence budgets were missed", &found)
    );

    // The measured table, so a failure names the numbers instead of a
    // predicate: position error falls by ~2x per doubling of the rate, the
    // velocity error stays inside f32 rounding noise, and every run is exactly
    // the symplectic solution to within a micrometre.
    for probe in &evidence.probes {
        println!(
            "convergence {} Hz: position error {:.6} m, velocity error {:.3e} m/s, \
             symplectic residual {:.3e} m, {} ticks, {} substeps",
            probe.fixed_hz,
            probe.position_error_m,
            probe.velocity_error_m_s,
            probe.symplectic_error_m,
            probe.ticks,
            probe.accounting.substeps
        );
    }
    for pair in PROBE_RATES_HZ.windows(2) {
        let order = evidence
            .observed_order(pair[0])
            .unwrap_or_else(|| panic!("no order from {} Hz", pair[0]));
        assert!(
            (0.95..=1.05).contains(&order),
            "the position error must halve from {} Hz to {} Hz, measured an order of {order}",
            pair[0],
            pair[1]
        );
    }
}

/// AC04's baseline: the designed 120 Hz rate is the one the frozen table was
/// written around, and its declared rate still converges at first order.
///
/// Observable failure if the baseline is changed or the tolerances are moved:
/// the budget lookup for 120 Hz fails to resolve, or the measured error at the
/// new baseline leaves the frozen band.
#[test]
fn accept_f23_d_the_baseline_rate_converges_inside_its_frozen_budget() {
    let evidence = convergence_evidence().expect("the declared scenario is runnable");
    let budget = FROZEN_CONVERGENCE_BUDGETS
        .iter()
        .find(|budget| budget.fixed_hz == BASELINE_FIXED_HZ)
        .expect("the frozen table covers the designed baseline rate");
    let probe = evidence
        .baseline()
        .expect("the evidence probed the designed baseline rate");

    let found = budget.violations(probe);
    assert!(
        found.is_empty(),
        "{}",
        report("the baseline rate missed its frozen budget", &found)
    );
    assert_eq!(
        probe.accounting.substeps, DECLARED_SUBSTEP_COUNT,
        "the product schedule must run the declared substep count: {probe:?}"
    );
    assert!(
        probe.position_error_m > probe.symplectic_error_m * 100.0,
        "the baseline's first-order position error must be the measured one, not noise: \
         position {:.6} m, symplectic residual {:.3e} m",
        probe.position_error_m,
        probe.symplectic_error_m
    );
}

/// The frozen table covers exactly the rates AC04 names, so the tolerances
/// cannot silently drift away from the rates they were measured at.
///
/// Observable failure if a rate is added to `PROBE_RATES_HZ` without measuring
/// it, or removed while its budget stays: the two lists no longer describe the
/// same set.
#[test]
fn accept_f23_d_every_probed_rate_has_a_frozen_budget() {
    let probed: Vec<u32> = PROBE_RATES_HZ.to_vec();
    let budgeted: Vec<u32> = FROZEN_CONVERGENCE_BUDGETS
        .iter()
        .map(|budget| budget.fixed_hz)
        .collect();
    assert_eq!(
        budgeted, probed,
        "the frozen budgets must cover exactly the probed rates, in order"
    );
    for budget in FROZEN_CONVERGENCE_BUDGETS {
        assert!(
            budget.max_position_error_m.is_finite() && budget.max_position_error_m > 0.0,
            "a frozen position tolerance must be a positive number: {budget:?}"
        );
        assert!(
            budget.max_velocity_error_m_s.is_finite() && budget.max_velocity_error_m_s > 0.0,
            "a frozen velocity tolerance must be a positive number: {budget:?}"
        );
    }
}

/// High-speed contact: across the whole rate/speed/geometry matrix, a
/// projectile crossing a thin obstacle is reported exactly once and never passes
/// it.
///
/// Observable failure if the swept path is removed: at 300 m/s and 120 Hz the
/// projectile travels 2.5 m per tick through a 2 cm wall and the crossing is
/// reported once while the body sails on past `x = +0.2` — a reported hit that
/// physically did not happen.
#[test]
fn accept_f23_d_every_high_speed_crossing_is_reported_once_and_never_tunnels() {
    let sweep = contact_sweep().expect("the declared matrix is runnable");
    assert_eq!(
        sweep.probes.len(),
        // rate x speed x (two spawn distances for a solid obstacle, one for a
        // sensor — see `contact_sweep`'s doc for the measured asymmetry).
        PROBE_RATES_HZ.len() * PROBE_SPEEDS_M_S.len() * 3,
        "the sweep must cover rate x speed x spawn distance x obstacle kind"
    );

    let found = sweep.violations();
    assert!(
        found.is_empty(),
        "{}",
        report(
            "the high-speed contact sweep missed the frozen rules",
            &found
        )
    );
    assert_eq!(
        sweep.deepest_solid_penetration_m(),
        0.0,
        "no solid crossing may reach an obstacle's near face at all: {sweep:?}"
    );
    assert!(
        CONTACT_FACE_TOLERANCE_M
            < f64::from(ContactScenario::solid_wall().obstacle_half_thickness_m),
        "the frozen face tolerance must be smaller than the thinnest obstacle the sweep uses, \
         or it would admit real penetration"
    );
}

/// The declared substep count is what closes the tunneling defect, and the
/// single-step schedule the F23-A/B stages shipped with still fails.
///
/// Observable failure if `DECLARED_SUBSTEP_COUNT` is removed or lowered: the
/// one-substep probe tunnels at every probed speed above one metre of travel
/// per tick, which is the regression this stage repaired. If the *product*
/// policy were weakened without the substep count, both halves of this test
/// would move together and the single-step run would stop failing.
#[test]
fn accept_f23_d_a_single_solver_step_tunnels_where_the_declared_policy_does_not() {
    // The product schedule, through the production session.
    let product = contact_probe(BASELINE_FIXED_HZ, ContactScenario::timed(300.0, 4.0))
        .expect("the declared scenario is runnable");
    assert_eq!(
        product.accounting.substeps, DECLARED_SUBSTEP_COUNT,
        "the session must install the declared substep policy: {product:?}"
    );
    assert!(
        !product.tunnelled(),
        "the declared policy must not tunnel: {product:?}"
    );

    // The same crossing with the pre-F23-D single solver step, driven through
    // the same production session: the defect this stage found, measured rather
    // than remembered.
    let scenario = ContactScenario::timed(300.0, 4.0);
    let mut single = cs_app::physics::PhysicsSession::new(BASELINE_FIXED_HZ);
    single
        .world_mut()
        .expect("a fresh session is active")
        .insert_resource(avian3d::prelude::SubstepCount(1));
    single
        .spawn(&cs_app::physics::BodySpec {
            layer: cs_sim::collision::CollisionLayer::StaticWorld,
            shape: cs_sim::collision::ShapeClass::Solid,
            mode: cs_app::physics::BodyMode::Static,
            mass_kg: 1.0,
            half_extents_m: [scenario.obstacle_half_thickness_m, 2.0, 2.0],
            position_m: [0.0, 0.0, 0.0],
            linear_velocity_m_s: [0.0; 3],
        })
        .expect("the obstacle spec is valid");
    // Four ticks of travel short of the obstacle, exactly as the sweep uses.
    let projectile = single
        .spawn(&cs_app::physics::BodySpec {
            layer: cs_sim::collision::CollisionLayer::Projectile,
            shape: cs_sim::collision::ShapeClass::Solid,
            mode: cs_app::physics::BodyMode::Dynamic,
            mass_kg: 1.0,
            half_extents_m: [scenario.projectile_half_extent_m; 3],
            position_m: [-10.0, 0.0, 0.0],
            linear_velocity_m_s: [scenario.speed_m_s, 0.0, 0.0],
        })
        .expect("the projectile spec is valid")
        .entity;

    let mut deepest_x = f64::MIN;
    for _ in 0..scenario.observed_ticks {
        single.step(1).expect("the session is active");
        if let Some(pose) = single.pose(projectile) {
            deepest_x = deepest_x.max(f64::from(pose.position_m[0]));
        }
    }
    assert!(
        deepest_x > scenario.obstacle_face_m() + CONTACT_FACE_TOLERANCE_M,
        "one solver step per tick is expected to push a 2.5 m-per-tick projectile through a 2 cm \
         wall; if it no longer does, the regression this stage repaired is gone and the frozen \
         substep policy must be re-derived (deepest x was {deepest_x:.4})"
    );
}

/// The spawn inside the first-tick hole is caught by the production preflight:
/// clamped onto the contact, held there, and reported once.
///
/// Two parts of the swept spawn response carry this, and each one has its own
/// observable failure (both measured, both recorded in the finding):
///
/// * the declared `SPAWN_CONTACT_OVERLAP_M` on the clamp. The swept layers run
///   with `SpeculativeMargin::ZERO`, so a body stopped exactly at the time of
///   impact only *touches* and generates no contact: with the overlap removed,
///   9 of the 12 probed rate/speed combinations sail through the wall
///   (`x = +2.9` to `+29.9`) and report nothing at all;
/// * the removal of the velocity component pointing into the obstacle, which
///   leaves the projectile at rest on the contact from the following tick
///   instead of rebounding off it at 1–17 m/s.
#[test]
fn accept_f23_d_a_spawn_inside_the_first_tick_hole_is_stopped_and_reported() {
    for fixed_hz in PROBE_RATES_HZ {
        for speed in PROBE_SPEEDS_M_S {
            let probe = contact_probe(
                fixed_hz,
                ContactScenario::timed(speed, SPAWN_IN_HOLE_TRAVEL_TICKS),
            )
            .expect("the declared scenario is runnable");
            assert!(
                probe.preflight_clamped,
                "a spawn {} ticks of travel from a solid obstacle must preflight: {probe:?}",
                SPAWN_IN_HOLE_TRAVEL_TICKS
            );
            assert_eq!(
                probe.episodes, 1,
                "the clamped spawn must be reported exactly once: {probe:?}"
            );
            assert_eq!(
                probe.contact_kind,
                Some(cs_sim::collision::ContactKind::SolidContact),
                "a solid obstacle classifies as a solid contact: {probe:?}"
            );
            assert!(
                !probe.tunnelled(),
                "a clamped spawn must never pass the obstacle: {probe:?}"
            );
            assert!(
                probe.final_speed_m_s < 0.01,
                "a clamped spawn ends at rest on the contact, not rebounding off it: {probe:?}"
            );
        }
    }
}

/// The one case the swept spawn repair deliberately does not close: a trigger
/// crossed entirely within the spawn tick.
///
/// Observable failure if this is "fixed" by stopping or delaying the spawn —
/// which is what F23-C's `accept_f23_c_preflight_never_stops_on_a_sensor`
/// forbids: the projectile would no longer fly through at its fired speed. The
/// test pins the F23-C behaviour and the F23-D measurement that the crossing is
/// nonetheless recorded on the preflight channel and never reported as a
/// contact, so the limitation cannot be forgotten.
#[test]
fn accept_f23_d_a_trigger_crossed_inside_the_spawn_tick_is_recorded_but_never_reported() {
    for fixed_hz in PROBE_RATES_HZ {
        for speed in PROBE_SPEEDS_M_S {
            let mut scenario = ContactScenario::sensor_trigger();
            scenario.speed_m_s = speed;
            scenario.start_travel_ticks = SPAWN_IN_HOLE_TRAVEL_TICKS;
            let probe = contact_probe(fixed_hz, scenario).expect("the scenario is runnable");

            assert!(
                !probe.preflight_clamped,
                "a sensor must never clamp or stop a spawn (F23-C): {probe:?}"
            );
            assert!(
                probe.final_x_m > probe.cleared_x_m,
                "the projectile must fly through the trigger untouched: {probe:?}"
            );
            assert!(
                probe.final_speed_m_s > SENSOR_MIN_SPEED_FRACTION * f64::from(speed),
                "the trigger must not slow the projectile: {probe:?}"
            );
            // Whether the engine reports the overlap depends on where the body
            // lands at the end of the spawn tick: if the whole crossing fits
            // inside that tick, the body has already left the trigger and no
            // event exists (F23-D measured 0 episodes at 60 and 120 Hz, where
            // a tick is 1 m and 0.5 m of travel); if the body lands *inside*
            // the trigger, the next tick reports it (1 episode at 240 Hz, where
            // a tick is 0.25 m). What never varies is that the crossing is
            // reported at most once and is always recorded.
            assert!(
                probe.episodes <= 1,
                "a trigger crossing is reported at most once: {probe:?}"
            );
            if probe.episodes == 1 {
                assert_eq!(
                    probe.first_report_tick,
                    Some(2),
                    "a reported spawn-tick overlap arrives on the following tick: {probe:?}"
                );
            }
            println!(
                "sensor in the spawn tick: {} Hz at {} m/s -> {} episodes, first at {:?}, \
                 preflight recorded {} at {:.4} m",
                probe.fixed_hz,
                probe.speed_m_s,
                probe.episodes,
                probe.first_report_tick,
                probe.preflight_passed.is_some(),
                probe.preflight_passed_distance_m.unwrap_or(f64::NAN)
            );
            assert!(
                probe.preflight_hit.is_none(),
                "no solid obstacle is involved, so the clamp cast must find none: {probe:?}"
            );
            assert!(
                probe.preflight_passed.is_some(),
                "the preflight's second cast must record the sensor the spawn crossed, or the \
                 crossing leaves no trace anywhere: {probe:?}"
            );
            assert!(
                probe
                    .preflight_passed_distance_m
                    .is_some_and(|distance| distance > 0.0),
                "the recorded crossing must name how far the body could travel: {probe:?}"
            );
        }
    }
}

/// A sensor is not an obstacle: every trigger crossing is reported once as an
/// overlap, and the projectile leaves at its fired speed.
///
/// Observable failure if the sensor binding or the swept spawn predicate is
/// dropped: the trigger resolves as a solid obstacle (the projectile is stopped
/// or slowed) or the overlap is never reported.
#[test]
fn accept_f23_d_a_sensor_crossing_is_reported_once_and_never_slows_the_projectile() {
    for fixed_hz in PROBE_RATES_HZ {
        for speed in PROBE_SPEEDS_M_S {
            let mut scenario = ContactScenario::sensor_trigger();
            scenario.speed_m_s = speed;
            let probe = contact_probe(fixed_hz, scenario).expect("the scenario is runnable");
            assert_eq!(
                probe.episodes, 1,
                "a trigger crossing is reported exactly once: {probe:?}"
            );
            assert_eq!(
                probe.contact_kind,
                Some(cs_sim::collision::ContactKind::SensorOverlap),
                "a sensor overlap is never a solid contact: {probe:?}"
            );
            assert!(
                probe.cleared(),
                "a sensor must not stop the projectile at its face: {probe:?}"
            );
            assert!(
                !probe.preflight_clamped,
                "a sensor must never clamp a spawn (F23-C): {probe:?}"
            );
            assert!(
                probe.preflight_hit.is_none(),
                "the preflight's clamp cast must not treat a sensor as a solid contact: {probe:?}"
            );
            assert!(
                probe.preflight_passed.is_none(),
                "a spawn with a clear approach never reaches the sensor inside its own tick: \
                 {probe:?}"
            );
        }
    }
}

/// Stability: ten simulated seconds of production flight at every probed rate
/// stay finite, bounded, and fully accounted for.
///
/// Observable failure if the driver, the model, the mass binding or the schedule
/// breaks: a refused or skipped driver tick, a lost force request, a tick
/// without an integration, a non-finite state, or a body rate the bounded
/// controller was never supposed to reach.
#[test]
fn accept_f23_d_ten_seconds_of_production_flight_stays_stable_at_every_rate() {
    let scenario = StabilityScenario::level_cruise();
    for fixed_hz in PROBE_RATES_HZ {
        let probe = stability_probe(&scenario, fixed_hz).expect("the scenario is flyable");
        assert!(
            probe.violations.is_empty(),
            "{}",
            report("the stability run broke a frozen rule", &probe.violations)
        );
        assert_eq!(
            probe.driver.driven, probe.driver.ticks,
            "every fixed tick must produce exactly one applied force request: {probe:?}"
        );
        assert_eq!(
            probe.driver.parked, 0,
            "a dynamic aircraft is never parked: {probe:?}"
        );
        println!(
            "stability {} Hz: {} ticks, peak {:.4} m/s, peak rate {:.4} rad/s, altitude \
             {:.3}..{:.3} m, {} requests applied",
            probe.fixed_hz,
            probe.ticks,
            probe.max_speed_m_s,
            probe.max_body_rate_rad_s,
            probe.altitude_range_m.0,
            probe.altitude_range_m.1,
            probe.accounting.applied_requests
        );
    }
}

/// The flight path's own convergence: the peak speed a ten-second cruise
/// reaches must not depend on the rate, which is what a rate-independent
/// integrator looks like from the outside.
///
/// Observable failure if the force path were rate-dependent — a timestep applied
/// twice, a substep count leaking into the velocity integration, or a force
/// applied once per *frame* instead of once per tick: the three rates would
/// disagree by far more than the frozen spread.
#[test]
fn accept_f23_d_the_flight_peak_speed_agrees_across_rates() {
    let scenario = StabilityScenario::level_cruise();
    let probes: Vec<_> = PROBE_RATES_HZ
        .iter()
        .map(|fixed_hz| stability_probe(&scenario, *fixed_hz).expect("the scenario is flyable"))
        .collect();
    let spread = rate_spread_m_s(&probes);
    assert!(
        spread <= FROZEN_STABILITY_BUDGET.max_rate_spread_m_s,
        "the cruise peak speed spread across {PROBE_RATES_HZ:?} Hz is {spread:.6} m/s, above the \
         frozen {:.6} m/s",
        FROZEN_STABILITY_BUDGET.max_rate_spread_m_s
    );
    for probe in &probes {
        println!(
            "flight convergence {} Hz: peak {:.6} m/s",
            probe.fixed_hz, probe.max_speed_m_s
        );
    }
}

/// A duration that is not a whole number of ticks is judged on the time the run
/// actually simulated, not on the duration that was asked for.
///
/// 10.51 s at 60 Hz is 631 whole ticks, i.e. 10.5167 s: six milliseconds more
/// than requested, which at the scenario's terminal 220 m/s is 1.47 m of
/// travel. Charging that to the integrator would report the *difference* of the
/// two first-order terms (0.88 m − 1.47 m = −0.59 m) as the position error
/// instead of the term itself.
///
/// Observable failure if the comparison reverts to the requested duration: the
/// measured error becomes 0.59 m rather than 0.88 m, and the ratio this test
/// requires drops from 1.00 to 0.68.
#[test]
fn accept_f23_d_a_fractional_tick_duration_is_judged_on_the_time_it_simulated() {
    let mut scenario = ConvergenceScenario::constant_thrust();
    scenario.duration_s = 10.51;
    let probe = convergence_probe(&scenario, 60).expect("the scenario is runnable");

    assert_eq!(
        probe.ticks, 631,
        "60 Hz can only simulate whole ticks: 10.51 s is 630.6 of them"
    );
    assert!(
        (probe.simulated_seconds - f64::from(scenario.duration_s)).abs() > 0.005,
        "this scenario must not divide into whole ticks, or the test proves nothing: \
         simulated {} s",
        probe.simulated_seconds
    );

    // The first-order term of symplectic Euler at the time actually simulated.
    let dt = 1.0 / (f64::from(probe.fixed_hz) * f64::from(probe.accounting.substeps));
    let first_order_m = 0.5 * 20.0 * probe.simulated_seconds * dt;
    assert!(
        probe.position_error_m > 0.9 * first_order_m,
        "the position error must be the first-order term for the simulated time \
         ({first_order_m:.6} m), not the difference of two terms: measured {:.6} m",
        probe.position_error_m
    );
    assert!(
        probe.symplectic_error_m < 1.0e-2,
        "and what is left must be f32 accumulation, not a time mismatch: {:.3e} m",
        probe.symplectic_error_m
    );
}

/// A spawn that crosses a trigger and is then stopped by a wall in the same
/// tick still records the trigger crossing.
///
/// The spawn preflight's sensor cast is not conditional on the solid cast: a
/// body that crossed a trigger crossed it, whether or not something solid
/// stopped it on the same tick, and the record is the only trace that crossing
/// leaves (the engine reports nothing for a crossing that fits inside the
/// spawn tick). `passed_distance_m` and the clamp's `distance_m` are measured
/// from the same spawn position, so the trigger reads closer than the wall.
///
/// Observable failure if the sensor cast is skipped when a solid obstacle
/// stops the spawn: `passed` comes back `None` and the crossing leaves no
/// trace at all, even though the body demonstrably flew through the trigger.
#[test]
fn accept_f23_d_a_spawn_records_a_sensor_it_crossed_before_a_solid_stops_it() {
    use cs_app::physics::{BodyMode, BodySpec, PhysicsSession};
    use cs_sim::collision::{CollisionLayer, ContactKind, ShapeClass};

    // 120 Hz and 60 m/s: 0.5 m of travel in the spawn tick. The trigger sits at
    // x = -0.10 and the wall at x = +0.10, so the projectile meets the trigger
    // 0.24 m into the tick and the wall 0.44 m into it.
    let travel_per_tick_m = 0.5_f32;
    let mut session = PhysicsSession::new(BASELINE_FIXED_HZ);
    let trigger = session
        .spawn(&BodySpec {
            layer: CollisionLayer::Trigger,
            shape: ShapeClass::Sensor,
            mode: BodyMode::Static,
            mass_kg: 1.0,
            half_extents_m: [0.01, 1.0, 1.0],
            position_m: [-0.10, 0.0, 0.0],
            linear_velocity_m_s: [0.0; 3],
        })
        .expect("the trigger is valid")
        .entity;
    let wall = session
        .spawn(&BodySpec {
            layer: CollisionLayer::StaticWorld,
            shape: ShapeClass::Solid,
            mode: BodyMode::Static,
            mass_kg: 1.0,
            half_extents_m: [0.01, 2.0, 2.0],
            position_m: [0.10, 0.0, 0.0],
            linear_velocity_m_s: [0.0; 3],
        })
        .expect("the wall is valid")
        .entity;
    let projectile = session
        .spawn(&BodySpec {
            layer: CollisionLayer::Projectile,
            shape: ShapeClass::Solid,
            mode: BodyMode::Dynamic,
            mass_kg: 1.0,
            half_extents_m: [0.05; 3],
            position_m: [-0.40, 0.0, 0.0],
            linear_velocity_m_s: [
                travel_per_tick_m * f64::from(BASELINE_FIXED_HZ) as f32,
                0.0,
                0.0,
            ],
        })
        .expect("the projectile is valid")
        .entity;

    let mut events = Vec::new();
    let mut solid_reports = 0_usize;
    for _ in 0..6 {
        let frame = session.step(1).expect("the session is active");
        for event in &frame.spawn_events {
            if event.body == projectile {
                events.push(*event);
            }
        }
        for report in &frame.reports {
            if report.bodies.contains(&projectile) && report.bodies.contains(&wall) {
                assert_eq!(report.kind, ContactKind::SolidContact);
                solid_reports += 1;
            }
        }
    }

    let event = events
        .first()
        .unwrap_or_else(|| panic!("the moving swept-layer spawn must preflight: {events:?}"));
    assert!(event.clamped, "the wall stops the spawn: {event:?}");
    assert!(
        event.stopped,
        "and stops its motion into the wall: {event:?}"
    );
    assert_eq!(
        event.passed,
        Some(trigger),
        "the trigger the spawn crossed is recorded even though a solid obstacle \
         stopped the same tick: {event:?}"
    );
    let passed = event
        .passed_distance_m
        .expect("a recorded crossing names its distance");
    let clamped = event.distance_m.expect("a clamp names its distance");
    assert!(
        passed > 0.0 && passed < clamped,
        "the trigger is met before the wall, measured from the same spawn \
         position: trigger at {passed:.4} m, wall at {clamped:.4} m"
    );
    assert!(
        clamped < travel_per_tick_m,
        "both are inside the tick's travel of {travel_per_tick_m} m, or the \
         scenario is not the one this test names: wall at {clamped:.4} m"
    );
    assert_eq!(
        solid_reports, 1,
        "the clamped spawn is reported against the wall exactly once"
    );
    let pose = session
        .pose(projectile)
        .expect("the body is still in the world");
    // The wall's near face is at x = 0.09 and the projectile's half extent is
    // 0.05, so resting on the face is x = 0.04; `CONTACT_FACE_TOLERANCE_M`
    // absorbs the declared overlap the solver has not yet pushed out.
    let face_rest_x = 0.09 - 0.05 + CONTACT_FACE_TOLERANCE_M as f32;
    assert!(
        pose.position_m[0] < face_rest_x,
        "and it ends at rest against the wall's near face (x < {face_rest_x}): {pose:?}"
    );
    assert!(
        pose.linear_velocity_m_s[0].abs() < 0.01,
        "not rebounding off it: {pose:?}"
    );
}

/// A probe refuses a scenario it cannot run instead of producing a number that
/// looks like evidence.
///
/// Observable failure if validation is removed: a zero-mass or non-finite
/// scenario would build a world, and the resulting "measurement" would be
/// indistinguishable from a real one.
#[test]
fn accept_f23_d_a_probe_refuses_a_scenario_it_cannot_run() {
    let mut scenario = ConvergenceScenario::constant_thrust();
    scenario.mass_kg = 0.0;
    assert_eq!(
        convergence_evidence_run(scenario),
        Err(ProbeError::Field {
            field: "mass_kg",
            reason: "is outside the probe's declared range",
        }),
        "a zero mass is not a runnable convergence scenario"
    );

    let mut scenario = ConvergenceScenario::constant_thrust();
    scenario.duration_s = f32::NAN;
    assert!(
        convergence_evidence_run(scenario).is_err(),
        "NaN is not a duration"
    );

    let mut scenario = ConvergenceScenario::constant_thrust();
    scenario.duration_s = 0.001;
    assert_eq!(
        convergence_probe(&scenario, BASELINE_FIXED_HZ),
        Err(ProbeError::Field {
            field: "duration_s",
            reason: "is shorter than one tick at this rate",
        }),
        "a run shorter than one tick is not a run"
    );

    let mut scenario = ConvergenceScenario::constant_thrust();
    scenario.duration_s = 1.0e9;
    assert_eq!(
        convergence_probe(&scenario, BASELINE_FIXED_HZ),
        Err(ProbeError::Field {
            field: "duration_s",
            reason: "is longer than the probe's declared tick budget",
        }),
        "a probe run is bounded by MAX_PROBE_TICKS ({MAX_PROBE_TICKS})"
    );

    let mut scenario = ContactScenario::solid_wall();
    scenario.speed_m_s = 0.0;
    assert_eq!(
        contact_probe(BASELINE_FIXED_HZ, scenario),
        Err(ProbeError::Field {
            field: "speed_m_s",
            reason: "is outside the probe's declared range",
        }),
        "a stationary projectile cannot probe a crossing"
    );

    let mut scenario = StabilityScenario::level_cruise();
    scenario.duration_s = 0.0;
    assert!(
        stability_probe(&scenario, BASELINE_FIXED_HZ).is_err(),
        "a zero-length run is not a stability probe"
    );
}

/// Runs one scenario through every probed rate, so the refusal is a whole-run
/// refusal rather than one rate's.
fn convergence_evidence_run(
    scenario: cs_app::physics::ConvergenceScenario,
) -> Result<(), ProbeError> {
    for fixed_hz in PROBE_RATES_HZ {
        cs_app::physics::convergence_probe(&scenario, fixed_hz)?;
    }
    Ok(())
}
