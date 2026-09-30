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
    BASELINE_FIXED_HZ, CONTACT_FACE_TOLERANCE_M, ContactScenario, DECLARED_SUBSTEP_COUNT,
    FROZEN_CONVERGENCE_BUDGETS, FROZEN_STABILITY_BUDGET, PROBE_RATES_HZ, PROBE_SPEEDS_M_S,
    ProbeError, SENSOR_MIN_SPEED_FRACTION, SPAWN_IN_HOLE_TRAVEL_TICKS, StabilityScenario,
    contact_probe, contact_sweep, convergence_evidence, rate_spread_m_s, stability_probe,
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
/// clamped onto the contact, stopped there, and reported once.
///
/// Observable failure if the swept spawn response is removed: the clamp lands
/// but the tick carries the projectile through the wall, and the crossing is
/// reported **zero** times — a bullet that silently passes through an aircraft.
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

/// A probe refuses a scenario it cannot run instead of producing a number that
/// looks like evidence.
///
/// Observable failure if validation is removed: a zero-mass or non-finite
/// scenario would build a world, and the resulting "measurement" would be
/// indistinguishable from a real one.
#[test]
fn accept_f23_d_a_probe_refuses_a_scenario_it_cannot_run() {
    let mut scenario = cs_app::physics::ConvergenceScenario::constant_thrust();
    scenario.mass_kg = 0.0;
    assert_eq!(
        convergence_evidence_run(scenario),
        Err(ProbeError::Field {
            field: "mass_kg",
            reason: "is outside the probe's declared range",
        }),
        "a zero mass is not a runnable convergence scenario"
    );

    let mut scenario = cs_app::physics::ConvergenceScenario::constant_thrust();
    scenario.duration_s = f32::NAN;
    assert!(
        convergence_evidence_run(scenario).is_err(),
        "NaN is not a duration"
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
