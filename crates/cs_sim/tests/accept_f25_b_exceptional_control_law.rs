//! F25-B: the declared exceptional control law, driven end to end through the
//! public `cs_sim::flight` surface.
//!
//! Spec: `specs/F25-hoplite-autogyro-and-exceptional-flight-configurations.md`,
//! section `### F25-B`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! These tests call the production law, the production rotor drive and the
//! production telemetry frame. Nothing here re-implements an equation: the
//! closed-form values they compare against are the profile's own declared
//! functions, evaluated by hand, which is what makes the assertions fail if the
//! law stops answering its profile.

use cs_sim::flight::{
    DamageState, EngineState, ExceptionalControlLaw, ExceptionalTick, FlightEnvironment,
    FlightInput, FlightModel, FlightState, FlightTelemetry, LoadoutMass, ModelKind, RotorDrive,
    RotorSpeedMapping, RotorVisualSample, SYNTHETIC_TICK_DT_S, SharedTelemetry, TelemetryFrame,
    synthetic_exceptional_profile, synthetic_exceptional_tuning, synthetic_fixed_wing,
    synthetic_rotor_drive, synthetic_rotor_mapping,
};
use cs_types::Tick;
use cs_types::space::Quaternion;

/// Ticks that let the declared first-order rotor response settle on its
/// command; see the unit test of the same name in `autogyro.rs`.
const SETTLE_TICKS: u64 = 2400;

fn law() -> ExceptionalControlLaw {
    ExceptionalControlLaw::new(
        synthetic_exceptional_tuning(),
        synthetic_exceptional_profile(),
    )
    .expect("the declared synthetic exceptional law is valid")
}

fn level(speed_mps: f64, spool: Option<f64>) -> FlightState {
    FlightState {
        linear_velocity_mps: [0.0, 0.0, -speed_mps],
        engine: spool.map_or(EngineState::STOPPED, EngineState::direct),
        ..FlightState::at_rest(Quaternion::IDENTITY)
    }
}

fn stick(pitch: f64, roll: f64, yaw: f64, throttle: f64) -> FlightInput {
    FlightInput::try_new(pitch, roll, yaw, throttle, false)
        .expect("the probe input is inside its declared range")
}

fn step(
    law: &ExceptionalControlLaw,
    state: &FlightState,
    input: &FlightInput,
    tick: u64,
    rotor: &mut RotorDrive,
) -> ExceptionalTick {
    law.compute(
        &FlightEnvironment::SEA_LEVEL,
        &LoadoutMass::EMPTY,
        &DamageState::PRISTINE,
        state,
        input,
        SYNTHETIC_TICK_DT_S,
        Tick(tick),
        rotor,
    )
    .expect("the probe tick is a legal exceptional tick")
}

/// AC02 end to end: a closed loop of many ticks at low speed and with the engine
/// off stays finite all the way through, the rotor is the only thing that
/// produces vertical support, and the airframe's own response is the profile's
/// declared one rather than a plausible number.
#[test]
fn accept_f25_b_a_closed_low_speed_and_engine_off_loop_stays_finite() {
    let law = law();
    let profile = law.profile().clone();
    let input = stick(0.2, 0.1, 0.0, 1.0);

    // Phase 1: a spin-up and run-up at 4 m/s — below the wing's authority ramp
    // and inside the rotor's own control band.
    let mut rotor = synthetic_rotor_drive();
    let mut state = level(4.0, Some(0.6));
    let mut highest_authority = 0.0_f64;
    for tick in 1..=600_u64 {
        let computed = step(&law, &state, &input, tick, &mut rotor);
        computed
            .validate()
            .expect("the tick is finite and accounted");
        highest_authority = highest_authority.max(computed.diagnostics.control_authority);
        assert!(
            computed.diagnostics.rotor_lift_n > 0.0,
            "a spinning rotor lifts"
        );
        state = FlightState {
            linear_velocity_mps: [
                state.linear_velocity_mps[0],
                state.linear_velocity_mps[1]
                    + computed.output.world_force_n[1] / 1200.0 * SYNTHETIC_TICK_DT_S,
                state.linear_velocity_mps[2],
            ],
            ..state
        };
        assert!(state.linear_velocity_mps[1].is_finite());
    }
    // The declared support band hands the stick to the rotor at this airspeed.
    assert!(
        highest_authority > law.tuning().angular.control_airspeed_full_mps.recip() * 4.0,
        "the rotor's declared band gives more authority than the wing's airspeed ramp"
    );

    // Phase 2: the engine stops at the same low airspeed. The airframe must keep
    // answering, not freeze and not diverge: gravity is the only thing left, and
    // the rotor's own drag pulls against the residual airflow.
    let state = level(4.0, None);
    let mut previous_rate = rotor.physical_speed_radps();
    for tick in 601..=1200_u64 {
        let computed = step(&law, &state, &input, tick, &mut rotor);
        computed
            .validate()
            .expect("an engine-off tick is finite and accounted");
        assert_eq!(computed.output.diagnostics.thrust_n, 0.0);
        let commanded = profile.rotor_drive_radps_per_mps * 4.0;
        assert_eq!(computed.diagnostics.commanded_rotor_radps, commanded);
        assert!(
            computed.rotor.physical_speed_radps() >= commanded - 1e-12,
            "with the engine off the rotor settles down onto the airflow's command"
        );
        assert!(computed.rotor.physical_speed_radps() <= previous_rate + 1e-12);
        previous_rate = computed.rotor.physical_speed_radps();
        assert!(computed.output.world_force_n[1].is_finite());
        assert!(computed.output.world_torque_nm[0].is_finite());
    }

    // The final reading is the profile's own steady answer for that airspeed.
    let commanded = profile.rotor_drive_radps_per_mps * 4.0;
    assert!((previous_rate - commanded).abs() < 1.0);
    assert!(
        previous_rate < profile.rotor_drive_radps_per_mps * 600.0,
        "the rotor is bounded by the airflow, not by the pre-spin"
    );
}

/// The shared telemetry interface is fed by the real exceptional law now, so a
/// consumer holding `&dyn FlightTelemetry` reads the same shared numbers for
/// both kinds and the exceptional frame carries the law's own rotor rate
/// through the explicit visual mapping.
#[test]
fn accept_f25_b_the_real_law_feeds_the_shared_telemetry_channel() {
    let law = law();
    let speed = 25.0;
    let state = level(speed, Some(0.7));
    let input = stick(0.0, 0.0, 0.0, 0.7);
    let mut rotor = synthetic_rotor_drive();
    for tick in 1..=SETTLE_TICKS {
        let _ = step(&law, &state, &input, tick, &mut rotor);
    }
    let computed = step(&law, &state, &input, SETTLE_TICKS + 1, &mut rotor);

    let mapping: RotorSpeedMapping = synthetic_rotor_mapping();
    let frame: TelemetryFrame = computed
        .telemetry(&state, Tick(SETTLE_TICKS + 1), Some(&mapping))
        .expect("the exceptional law produces a valid telemetry frame");

    // A consumer takes the interface, not the concrete frame, and learns nothing
    // about which law produced the numbers.
    let consumer: &dyn FlightTelemetry = &frame;
    assert_eq!(consumer.model_kind(), ModelKind::Exceptional);
    let shared: &SharedTelemetry = consumer.shared();
    assert_eq!(
        shared.airspeed_mps,
        computed.output.instrument_state.airspeed_mps
    );
    assert_eq!(shared.thrust_n, computed.output.diagnostics.thrust_n);
    assert_eq!(
        shared.control_authority, computed.diagnostics.control_authority,
        "the shared channel reports the exceptional law's authority, not the wing's"
    );
    assert!(
        (shared.control_authority - 1.0).abs() < 1e-12,
        "at 25 m/s the rotor's declared band is saturated, so the law commands full authority"
    );

    // The exceptional channel carries the law's authoritative rate and the drawn
    // rate the explicit mapping produced — and no drawn rate at all without one.
    let channel = consumer
        .rotor()
        .expect("an exceptional frame carries a rotor channel");
    assert_eq!(channel.physical_speed_radps, rotor.physical_speed_radps());
    assert_eq!(
        channel.physical_speed_radps,
        computed.rotor.physical_speed_radps()
    );
    let drawn = channel
        .visual_speed_radps()
        .expect("a declared mapping produces a drawn rate");
    assert!((drawn - channel.physical_speed_radps * 1.5).abs() < 1e-9);
    assert!(
        computed
            .telemetry(&state, Tick(SETTLE_TICKS + 1), None)
            .expect("an unmapped frame is still valid")
            .rotor()
            .expect("an exceptional frame carries a rotor channel")
            .visual_speed_radps()
            .is_none()
    );

    // Sampling the drawn rotor at several render rates cannot reach the
    // simulation: the authoritative rate is identical afterwards.
    let mut sample = RotorVisualSample::at_rest();
    for _ in 0..600 {
        sample = rotor
            .visual_sample(Some(&mapping), sample, SYNTHETIC_TICK_DT_S / 60.0)
            .expect("the drawn sample is finite");
    }
    assert_eq!(
        rotor.physical_speed_radps(),
        computed.rotor.physical_speed_radps()
    );

    // The same state under the fixed-wing law produces the same shared airspeed,
    // so a HUD reading the interface is model-agnostic.
    let fixed_output = FlightModel::new(synthetic_fixed_wing())
        .compute(
            &FlightEnvironment::SEA_LEVEL,
            &LoadoutMass::EMPTY,
            &DamageState::PRISTINE,
            &state,
            &input,
            SYNTHETIC_TICK_DT_S,
        )
        .expect("the fixed-wing tick is legal");
    let fixed_frame =
        TelemetryFrame::standard(ModelKind::FixedWing, &state, &fixed_output, Tick(0))
            .expect("a fixed-wing frame is valid");
    assert_eq!(fixed_frame.shared().airspeed_mps, shared.airspeed_mps);
    assert!(
        (fixed_frame.shared().control_authority - shared.control_authority).abs() > 1e-6,
        "the two laws answer with different authority, which is the exceptional difference"
    );
    assert!(
        fixed_frame.shared().control_authority < shared.control_authority,
        "the rotor gives more authority than the same airframe's wing at this speed"
    );
}

/// Every force this law produces is the sum of the contributions it recorded,
/// and every torque it applies is inside the tuning's declared bound — the two
/// invariants a calibration probe needs before any of these numbers may be
/// compared against a reference trace.
#[test]
fn accept_f25_b_no_unaccounted_force_and_no_unbounded_torque() {
    let law = law();
    let max_torque = law.tuning().angular.max_torque_nm;
    let mut checked = 0;

    for speed in [0.0, 0.5, 2.0, 6.0, 18.0, 45.0] {
        for spool in [Some(0.0), Some(1.0), None] {
            for input in [
                FlightInput::NEUTRAL,
                stick(1.0, 0.0, 0.0, 0.0),
                stick(-1.0, 0.0, 0.0, 0.0),
                stick(0.0, 1.0, 0.0, 1.0),
                stick(0.0, 0.0, 1.0, 0.0),
            ] {
                let state = FlightState {
                    angular_velocity_radps: [0.2, -0.15, 0.35],
                    ..level(speed, spool)
                };
                let mut rotor = synthetic_rotor_drive();
                for tick in 1..=30_u64 {
                    let computed = step(&law, &state, &input, tick, &mut rotor);
                    // The total force is exactly the recorded contributions, so
                    // no term is hiding inside it.
                    let total = computed.diagnostics.total_force_n();
                    for (axis, force) in computed.output.world_force_n.iter().enumerate() {
                        assert!(
                            (total[axis] - force).abs() < 1e-9,
                            "component {axis} is unaccounted at {speed} m/s"
                        );
                    }
                    // The recorded contributions are the whole law: the shared
                    // boundary plus the two rotor force terms and nothing else.
                    let base = FlightModel::new(law.tuning().clone())
                        .compute(
                            &FlightEnvironment::SEA_LEVEL,
                            &LoadoutMass::EMPTY,
                            &DamageState::PRISTINE,
                            &state,
                            &input,
                            SYNTHETIC_TICK_DT_S,
                        )
                        .expect("the shared boundary's own tick is legal");
                    assert_eq!(
                        computed.diagnostics.base_force_n, base.world_force_n,
                        "the exceptional law adds rotor terms to the shared boundary, it does not reimplement it"
                    );
                    assert!(computed.output.diagnostics.lift_n >= computed.diagnostics.base.lift_n);
                    assert!(computed.output.diagnostics.drag_n >= computed.diagnostics.base.drag_n);
                    // The torque is bounded per axis.
                    for (axis, limit) in max_torque.iter().enumerate() {
                        assert!(
                            computed.diagnostics.control_axis_torque_nm[axis].abs() <= limit + 1e-9
                        );
                    }
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 6 * 3 * 5 * 30, "the whole grid ran");
}
