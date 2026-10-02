//! F25-E acceptance tests: the production flight driver dispatches on the
//! declared `ModelKind`, so an exceptional airframe is flown on the same Avian
//! schedule as a fixed wing.
//!
//! Spec: `specs/F25-hoplite-autogyro-and-exceptional-flight-configurations.md`,
//! task `#414` ("Dispatch the flight driver on `ModelKind` so an exceptional
//! airframe can be flown"). Task test prefix: `accept_f25_e_`.
//!
//! These tests drive production code only: the real [`PhysicsFixture`] world
//! with the production [`FlightForcesPlugin`], bodies entering through
//! [`spawn_exceptional_flight_body`], the exceptional tick computed by
//! `cs_sim`'s F25-B `ExceptionalControlLaw` through [`FlightAircraft`] and its
//! F25-A rotor drive/telemetry. Every value is newly authored synthetic fixture
//! data — never original game data — and no test reads `CS_GAME_DIR`.
//!
//! The dispatch itself is what these tests prove: remove the `ModelKind` arm in
//! `FlightAircraft::compute_tick` and the exceptional airframe no longer
//! produces a force request on each tick, so the schedule and telemetry
//! assertions fail.

use avian3d::prelude::{AngularVelocity, Gravity, LinearVelocity, Position};
use bevy::prelude::{Entity, Vec3};
use cs_app::physics::{
    FixtureBodySpec, FlightAircraft, FlightAircraftError, FlightForcesPlugin, FlightRefusalReason,
    FlightSpawnError, FlightSpawnSpec, FlightTickReport, PhysicsFixture, PhysicsSample,
    spawn_exceptional_flight_body, spawn_flight_body,
};
use cs_sim::flight::{
    EngineState, ExceptionalLawError, FlightEnvironment, FlightInput, FlightModel, FlightState,
    FlightTelemetry, ModelKind, SYNTHETIC_TICK_DT_S, synthetic_exceptional_profile,
    synthetic_exceptional_tuning, synthetic_fixed_wing, synthetic_rotor_mapping,
};
use cs_types::Tick;
use cs_types::space::Quaternion;

/// The number of fixed ticks the exceptional airframe is driven for.
const DRIVE_TICKS: u64 = 120;
/// A generous ceiling for the exceptional force trace, in newtons. It is far
/// above every declared term (gravity 11.8 kN, thrust 9 kN, the 6 kN rotor lift
/// cap and the strongest rotor drag at cruise), so it only fails on a runaway or
/// non-finite trace; the exact bounded quantity is the torque, clamped per axis
/// to the tuning's declared `angular.max_torque_nm`.
const FORCE_CEILING_N: f64 = 1.0e6;
/// The norm of the declared principal torque maxima `(roll, pitch, yaw)` =
/// `(20_000, 30_000, 8_000)` N·m is ~37 kN·m; a rotation preserves it, so a
/// world torque component can never exceed this by construction.
const TORQUE_CEILING_NM: f64 = 40_000.0;

/// A flight world: the real physics fixture with the production
/// [`FlightForcesPlugin`]; the fixture's own body is parked far away so the
/// spawned aircraft own the sky.
fn fixture() -> PhysicsFixture {
    PhysicsFixture::builder(FixtureBodySpec {
        mass_kg: 1.0,
        half_extents_m: [0.05, 0.05, 0.05],
        position_m: [-10_000.0, 0.0, 0.0],
        linear_velocity_m_s: [0.0; 3],
    })
    .configure(|app| {
        app.add_plugins(FlightForcesPlugin);
    })
    .build()
    .expect("the fixture spec is valid")
}

/// Spawns one exceptional aircraft through the production path with the
/// synthetic F25 tuning/profile and the F25-A synthetic rotor mapping.
fn exceptional_aircraft(fixture: &mut PhysicsFixture, spec: &FlightSpawnSpec) -> Entity {
    spawn_exceptional_flight_body(
        fixture.world_mut(),
        synthetic_exceptional_tuning(),
        synthetic_exceptional_profile(),
        spec,
        Some(synthetic_rotor_mapping()),
    )
    .expect("the exceptional spawn spec is valid")
}

/// Reads the [`FlightAircraft`] record of a spawned body.
fn aircraft_record(fixture: &PhysicsFixture, entity: Entity) -> FlightAircraft {
    fixture
        .world()
        .get::<FlightAircraft>(entity)
        .expect("a spawned flight body carries FlightAircraft")
        .clone()
}

/// Reads one body's authoritative pose and velocities.
fn sample_of(fixture: &PhysicsFixture, entity: Entity) -> PhysicsSample {
    let world = fixture.world();
    let position = world
        .get::<Position>(entity)
        .expect("a body has a position");
    let linear = world
        .get::<LinearVelocity>(entity)
        .expect("a body has a velocity");
    let angular = world
        .get::<AngularVelocity>(entity)
        .expect("a body has an angular velocity");
    PhysicsSample {
        position_m: position.0.to_array(),
        linear_velocity_m_s: linear.0.to_array(),
        angular_velocity_rad_s: angular.0.to_array(),
    }
}

/// Reads the flight driver's tick accounting.
fn report(fixture: &PhysicsFixture) -> FlightTickReport {
    fixture.world().resource::<FlightTickReport>().clone()
}

/// The production [`FlightState`] a telemetry consumer would pass for a body's
/// authoritative sample.
///
/// The fixture readback carries no orientation, and the telemetry's shared
/// channel takes its quantities from the model output, so a level orientation
/// is enough: the only state-derived shared value is the world-vertical speed,
/// which comes from the sample's own world velocity.
fn state_of(sample: &PhysicsSample, engine: EngineState) -> FlightState {
    FlightState {
        orientation: Quaternion::IDENTITY,
        linear_velocity_mps: sample.linear_velocity_m_s.map(f64::from),
        angular_velocity_radps: [0.0; 3],
        engine,
        boost_available: false,
    }
}

/// The exceptional airframe is driven on the real Avian schedule for a run of
/// fixed ticks, producing exactly one applied force request per tick and a
/// visible, bounded, finite force/torque trace; the F25-A telemetry reports the
/// rotor the airflow spun up and its explicitly mapped visual rate.
#[test]
fn accept_f25_e_exceptional_airframe_is_flown_on_the_avian_schedule() {
    let mut fixture = fixture();
    let plane = exceptional_aircraft(
        &mut fixture,
        &FlightSpawnSpec {
            engine: EngineState::direct(0.8),
            command: FlightInput::try_new(0.2, 0.1, 0.0, 0.8, false).expect("a legal command"),
            ..FlightSpawnSpec::level_at([0.0, 300.0, 0.0], [0.0, 0.0, -55.0])
        },
    );

    assert_eq!(
        aircraft_record(&fixture, plane).model_kind(),
        ModelKind::Exceptional,
        "the spawned record carries the declared kind"
    );

    let mut max_force = 0.0_f64;
    let mut max_torque = 0.0_f64;
    let mut traced_ticks = 0_u64;
    for _ in 0..DRIVE_TICKS {
        fixture.step(1);
        traced_ticks += 1;
        let output = aircraft_record(&fixture, plane)
            .last_output()
            .expect("every driven exceptional tick leaves an output");
        for value in output.world_force_n {
            assert!(value.is_finite(), "the force trace must stay finite");
            max_force = max_force.max(value.abs());
        }
        for value in output.world_torque_nm {
            assert!(value.is_finite(), "the torque trace must stay finite");
            max_torque = max_torque.max(value.abs());
        }
    }

    assert_eq!(traced_ticks, DRIVE_TICKS);
    assert!(
        max_force > 0.0 && max_force < FORCE_CEILING_N,
        "the exceptional force trace is visible and bounded: {max_force} N"
    );
    assert!(
        max_torque > 0.0 && max_torque < TORQUE_CEILING_NM,
        "the exceptional torque trace is bounded by the declared envelope: {max_torque} N·m"
    );

    let report = report(&fixture);
    assert_eq!(
        report.ticks, DRIVE_TICKS,
        "the driver runs on every fixed tick"
    );
    assert_eq!(
        report.driven, DRIVE_TICKS,
        "one applied force request per fixed tick; a second rotor advance in a tick would be refused"
    );
    assert_eq!(
        report.refused, 0,
        "the exceptional tick is never refused: {:?}",
        report.last_refusal
    );
    assert_eq!(
        fixture.ledger().total_applied_requests,
        DRIVE_TICKS,
        "every exceptional request reaches the integrator on its own tick"
    );

    // The body actually moved under the integrated forces: it is not parked.
    let sample = sample_of(&fixture, plane);
    assert!(
        sample.position_m.iter().all(|value| value.is_finite()),
        "the integrated pose stays finite: {:?}",
        sample.position_m
    );

    // F25-A telemetry: the shared channel is model-agnostic and the exceptional
    // frame carries the rotor channel, mapped through the declared ratio.
    let record = aircraft_record(&fixture, plane);
    let frame = record
        .telemetry(
            &state_of(&sample, EngineState::direct(0.8)),
            Tick(DRIVE_TICKS),
        )
        .expect("the telemetry boundary accepts the measured tick")
        .expect("a driven tick has a measured frame");
    assert_eq!(frame.model_kind(), ModelKind::Exceptional);
    let shared = frame.shared();
    assert!(
        shared.airspeed_mps.is_finite() && shared.airspeed_mps > 0.0,
        "the airflow the rotor sees is the measured airspeed: {}",
        shared.airspeed_mps
    );
    let rotor = frame
        .rotor()
        .expect("an exceptional frame carries a rotor channel");
    assert!(
        rotor.physical_speed_radps > 0.0,
        "the airflow spun the rotor up: {} rad/s",
        rotor.physical_speed_radps
    );
    let ratio = synthetic_rotor_mapping().visual_radps_per_physical_radps();
    let visual = rotor
        .visual_speed_radps()
        .expect("the declared mapping produces a visual rate");
    assert!(
        (visual - rotor.physical_speed_radps * ratio).abs() < 1e-9,
        "the visual rate is the explicit mapping of the physical rate"
    );
}

/// The dispatch refuses the wrong kind by name: an exceptional tuning handed to
/// the fixed-wing-only constructor (the undeclared path, no profile supplied)
/// and a fixed-wing tuning handed to the exceptional constructor both leave no
/// half-spawned body behind.
#[test]
fn accept_f25_e_wrong_model_kind_is_refused_by_name_and_spawns_nothing() {
    let mut fixture = fixture();
    let entities_before = fixture.world().entities().len();

    assert_eq!(
        FlightAircraft::new(
            FlightModel::new(synthetic_exceptional_tuning()),
            FlightEnvironment::SEA_LEVEL,
        )
        .err(),
        Some(FlightAircraftError::UnsupportedModelKind {
            found: ModelKind::Exceptional,
        }),
        "an exceptional tuning needs a declared law, not the fixed-wing constructor"
    );

    assert!(matches!(
        spawn_flight_body(
            fixture.world_mut(),
            FlightModel::new(synthetic_exceptional_tuning()),
            &FlightSpawnSpec::level_at([0.0, 100.0, 0.0], [0.0; 3]),
        ),
        Err(FlightSpawnError::Aircraft(
            FlightAircraftError::UnsupportedModelKind {
                found: ModelKind::Exceptional
            }
        ))
    ));

    // The declared exceptional entry point refuses a fixed-wing tuning by name,
    // so a body can never be flown by the wrong law.
    assert!(matches!(
        spawn_exceptional_flight_body(
            fixture.world_mut(),
            synthetic_fixed_wing(),
            synthetic_exceptional_profile(),
            &FlightSpawnSpec::level_at([0.0, 100.0, 0.0], [0.0; 3]),
            None,
        ),
        Err(FlightSpawnError::Aircraft(
            FlightAircraftError::Exceptional(ExceptionalLawError::NotAnExceptionalAirframe {
                declared: ModelKind::FixedWing
            })
        ))
    ));

    assert_eq!(
        fixture.world().entities().len(),
        entities_before,
        "a refused exceptional spawn must leave no half body behind"
    );
}

/// The F24-B one-gravity invariant survives the dispatch: with a non-zero
/// Avian `Gravity` the exceptional tick is refused and counted, and the body
/// falls under the world's gravity alone — the law never runs, so no rotor is
/// advanced and no output is fabricated.
#[test]
fn accept_f25_e_gravity_conflict_refuses_the_exceptional_tick() {
    let mut fixture = fixture();
    fixture
        .world_mut()
        .insert_resource(Gravity(Vec3::new(0.0, -9.806_65, 0.0)));
    let plane = exceptional_aircraft(
        &mut fixture,
        &FlightSpawnSpec::level_at([0.0, 300.0, 0.0], [0.0, 0.0, -40.0]),
    );

    fixture.step(120);
    let report = report(&fixture);
    assert_eq!(
        report.driven, 0,
        "no exceptional tick may run under double gravity"
    );
    assert_eq!(report.refused, 120, "every conflicting tick is refused");
    assert!(
        matches!(
            report.last_refusal.as_ref().map(|refusal| &refusal.reason),
            Some(FlightRefusalReason::GravityConflict { .. })
        ),
        "the refusal names the conflict: {:?}",
        report.last_refusal
    );

    let record = aircraft_record(&fixture, plane);
    assert!(
        record.last_output().is_none(),
        "a refused exceptional tick never fabricates an output"
    );
    assert!(
        record
            .telemetry(&FlightState::at_rest(Quaternion::IDENTITY), Tick(120))
            .expect("the telemetry boundary is reachable")
            .is_none(),
        "a refused tick has no measured frame"
    );
    let sample = sample_of(&fixture, plane);
    assert!(
        (sample.linear_velocity_m_s[1] + 9.806_65).abs() < 0.5,
        "the body falls under the world's gravity alone, not twice: {:?}",
        sample.linear_velocity_m_s
    );
}

/// The synthetic fixture advances at the fixed timestep the driver uses, so the
/// exceptional law's `dt` guard and the driver agree; a sanity check that the
/// fixture constant did not drift from the pinned fixed rate.
#[test]
fn accept_f25_e_synthetic_tick_matches_the_pinned_fixed_rate() {
    use cs_app::physics::BASELINE_FIXED_HZ;
    assert!(
        (SYNTHETIC_TICK_DT_S - 1.0 / f64::from(BASELINE_FIXED_HZ)).abs() < 1e-12,
        "the synthetic exceptional tick dt matches the pinned fixed rate"
    );
}
