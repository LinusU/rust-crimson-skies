//! F24-C acceptance tests: loadouts, damage, instruments and profile selection
//! reach the production fixed-wing flight model.
//!
//! Spec: `specs/F24-fixed-wing-flight-engine-stall-and-arcade-assists.md`,
//! stage `### F24-C`. Task test prefix: `accept_f24_c_`. Minimum scenario:
//! "Loadout, damage, instrument and profile selection reach the production
//! flight model; sustained turn, roll, acceleration and stall recovery traces
//! remain within the declared synthetic envelopes and are reported as
//! synthetic, not original-reference, results."
//!
//! These tests drive production code only: the real [`PhysicsFixture`] world
//! with the production [`FlightForcesPlugin`], the `cs_content` declared
//! record mapped through [`declared_flight_model`], equipment bound through
//! [`FlightEquipment`], and instrument readings consumed through
//! [`FlightInstruments`]. Every value is newly authored synthetic fixture data
//! — never original game data — and no test reads `CS_GAME_DIR`.

use avian3d::prelude::{AngularVelocity, LinearVelocity, Mass, Position, Rotation};
use bevy::prelude::{Entity, Vec3};
use cs_app::physics::flight::{
    FlightEquipment, FlightEvidenceClass, FlightInstruments, FlightRefusalReason,
    FlightTuningError, declared_flight_model,
};
use cs_app::physics::{
    FixtureBodySpec, FlightAircraft, FlightAircraftError, FlightForcesPlugin, FlightSpawnSpec,
    FlightTickReport, PhysicsFixture, PhysicsSample, spawn_flight_body,
};
use cs_content::flight_tuning::{
    FIDELITY_PROFILE, IMPROVED_PROFILE, declared_synthetic_airframe,
    declared_synthetic_airframe_for, declared_synthetic_improved_airframe,
};
use cs_sim::flight::{
    DamageState, EngineState, FlightInput, FlightModel, HandlingProfile, LoadoutMass,
    synthetic_fixed_wing, synthetic_trace_envelope,
};
use cs_types::asset_id::SourceSpan;
use cs_types::content::Origin;
use cs_types::evidence::ContentHash;
use cs_types::space::{Quaternion, Radians, UnitVec3};

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

/// Spawns one aircraft through the production path with `model`.
fn aircraft_with(
    fixture: &mut PhysicsFixture,
    model: FlightModel,
    spec: &FlightSpawnSpec,
) -> Entity {
    spawn_flight_body(fixture.world_mut(), model, spec).expect("the spawn spec is valid")
}

/// Spawns one synthetic fixed-wing aircraft through the production path.
fn aircraft(fixture: &mut PhysicsFixture, spec: &FlightSpawnSpec) -> Entity {
    aircraft_with(fixture, FlightModel::new(synthetic_fixed_wing()), spec)
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

fn speed(sample: &PhysicsSample) -> f64 {
    sample
        .linear_velocity_m_s
        .iter()
        .map(|component| f64::from(*component) * f64::from(*component))
        .sum::<f64>()
        .sqrt()
}

/// The heading of the body's forward axis about world up, in radians.
fn heading(fixture: &PhysicsFixture, entity: Entity) -> f64 {
    let rotation = fixture
        .world()
        .get::<Rotation>(entity)
        .expect("a flight body has a rotation");
    let forward = rotation.0 * Vec3::new(0.0, 0.0, -1.0);
    f64::from(forward.x).atan2(-f64::from(forward.z))
}

/// Checks one measured quantity against its declared synthetic envelope.
fn assert_envelope(name: &str, value: f64) {
    let envelope = synthetic_trace_envelope(name).expect("the envelope is declared");
    if let Err(error) = envelope.check(value) {
        panic!("{error}");
    }
}

/// Asserts the runtime trace is a synthetic result, not an original-reference
/// one, and explains why in the assertion message.
fn assert_synthetic_label(model: &FlightModel) {
    let class = FlightEvidenceClass::from_origin(&model.tuning().origin);
    assert_eq!(
        class,
        FlightEvidenceClass::Synthetic,
        "a runtime trace from a {} origin is a synthetic result",
        model.tuning().origin.label()
    );
    assert!(class.is_synthetic());
    assert_eq!(class.label(), "synthetic");
}

// ---------------------------------------------------------------------------
// Profile selection and error propagation
// ---------------------------------------------------------------------------

/// Both named profiles reach the production model, the improved record's
/// differences actually land on the tuning, and the selected profile is the one
/// the model flies — never a silent fallback.
#[test]
fn accept_f24_c_declared_profiles_build_production_models() {
    let fidelity = declared_synthetic_airframe();
    let improved = declared_synthetic_improved_airframe();

    let fidelity_model =
        declared_flight_model(&fidelity, HandlingProfile::Fidelity).expect("fidelity maps");
    assert_eq!(fidelity_model.tuning().profile, HandlingProfile::Fidelity);
    assert!(!fidelity_model.tuning().assists.enabled);
    assert_eq!(fidelity_model.tuning().angular.rate_gain_per_s, 4.0);
    assert_synthetic_label(&fidelity_model);

    let improved_model =
        declared_flight_model(&improved, HandlingProfile::Improved).expect("improved maps");
    assert_eq!(improved_model.tuning().profile, HandlingProfile::Improved);
    assert!(improved_model.tuning().assists.enabled);
    assert_eq!(improved_model.tuning().angular.rate_gain_per_s, 6.0);
    assert_eq!(improved_model.tuning().angular.rate_damping_per_s, 2.0);
    assert_eq!(
        improved_model.tuning().assists.bank_level_gain_nm_per_rad,
        8_000.0
    );
    assert_synthetic_label(&improved_model);

    // Selection is by the record's own name: asking for the other profile is a
    // mismatch, not a silent re-labeling.
    assert!(declared_flight_model(&fidelity, HandlingProfile::Improved).is_err());
    assert!(declared_flight_model(&improved, HandlingProfile::Fidelity).is_err());
}

/// An unknown profile label is refused, and a record missing a field the model
/// needs is refused by name instead of read as zero.
#[test]
fn accept_f24_c_profile_and_field_errors_propagate() {
    let mut unknown = declared_synthetic_airframe();
    unknown.profile = "autogyro".to_owned();
    assert!(declared_flight_model(&unknown, HandlingProfile::Fidelity).is_err());

    let mut incomplete = declared_synthetic_airframe();
    incomplete
        .values
        .retain(|value| value.field != "engine.max_thrust_n");
    assert!(declared_flight_model(&incomplete, HandlingProfile::Fidelity).is_err());

    assert_eq!(
        declared_synthetic_airframe_for("autogyro"),
        None,
        "the content producer declares no such profile"
    );
    assert_eq!(
        declared_synthetic_airframe_for(FIDELITY_PROFILE)
            .expect("fidelity is declared")
            .profile,
        FIDELITY_PROFILE
    );
    assert_eq!(
        declared_synthetic_airframe_for(IMPROVED_PROFILE)
            .expect("improved is declared")
            .profile,
        IMPROVED_PROFILE
    );
}

/// A fidelity record that enables assists is refused: the calibrated profile
/// must fly with assist contributions of exactly zero (F24 non-negotiable
/// behavior 5), so an assist set is not silently folded into it. The same
/// assist set is accepted under the explicitly named improved profile.
#[test]
fn accept_f24_c_fidelity_profile_refuses_enabled_assists() {
    let mut smuggled = declared_synthetic_airframe();
    assert!(!smuggled.assists_enabled);
    smuggled.assists_enabled = true;
    match declared_flight_model(&smuggled, HandlingProfile::Fidelity) {
        Err(FlightTuningError::FidelityAssistsEnabled { .. }) => {}
        other => panic!("a fidelity record with assists must be refused, got {other:?}"),
    }

    let improved = declared_synthetic_improved_airframe();
    assert!(improved.assists_enabled);
    let improved_model = declared_flight_model(&improved, HandlingProfile::Improved)
        .expect("the improved profile may enable assists");
    assert!(
        improved_model.tuning().assists.enabled,
        "the improved profile's assist reaches the model"
    );
}

// ---------------------------------------------------------------------------
// Equipment (loadout + damage + boost reserve) producer
// ---------------------------------------------------------------------------

/// The declared equipment record reaches the equations: its loadout moves the
/// integrator's mass, its damage scales thrust, and its reserve funds boost.
#[test]
fn accept_f24_c_equipment_reaches_the_production_model() {
    let mut fixture = fixture();
    let plane = aircraft(
        &mut fixture,
        &FlightSpawnSpec {
            engine: EngineState::direct(1.0),
            command: FlightInput::try_new(0.0, 0.0, 0.0, 1.0, false).expect("valid input"),
            ..FlightSpawnSpec::level_at([0.0, 400.0, 0.0], [0.0, 0.0, -60.0])
        },
    );

    // The producer declares a loaded, damaged aircraft with a boost reserve.
    fixture
        .world_mut()
        .entity_mut(plane)
        .insert(FlightEquipment {
            loadout: LoadoutMass {
                fuel_kg: 300.0,
                ordnance_kg: 200.0,
                armor_kg: 50.0,
            },
            damage: DamageState {
                control_authority: 0.8,
                thrust_authority: 0.5,
                lift_scale: 0.9,
            },
            boost_capacity_units: 0.02,
        });

    fixture.step(1);
    let world = fixture.world();
    assert_eq!(
        world.get::<Mass>(plane).expect("mass").0,
        1750.0,
        "the equipment's loadout moves the integrator's mass"
    );
    let output = aircraft_record(&fixture, plane)
        .last_output()
        .expect("a driven tick leaves an output");
    assert!(
        (output.instrument_state.thrust_n - 4_500.0).abs() < 1.0,
        "damage scales thrust: {}",
        output.instrument_state.thrust_n
    );
    assert_eq!(
        aircraft_record(&fixture, plane).damage().thrust_authority,
        0.5
    );

    // A mid-flight equipment change reaches the next tick.
    fixture
        .world_mut()
        .entity_mut(plane)
        .insert(FlightEquipment {
            loadout: LoadoutMass::EMPTY,
            ..FlightEquipment::EMPTY
        });
    fixture.step(1);
    assert_eq!(
        fixture.world().get::<Mass>(plane).expect("mass").0,
        1200.0,
        "dropping the loadout moves the same mass the equations use"
    );
}

/// A declared boost reserve is a draining quantity, not a per-tick refill: with
/// an unchanged equipment record the reserve falls monotonically to zero and the
/// boost thrust stops contributing.
#[test]
fn accept_f24_c_equipment_reserve_drains_without_refilling() {
    let mut fixture = fixture();
    let plane = aircraft(
        &mut fixture,
        &FlightSpawnSpec {
            engine: EngineState::direct(1.0),
            command: FlightInput::try_new(0.0, 0.0, 0.0, 1.0, true).expect("valid input"),
            ..FlightSpawnSpec::level_at([0.0, 400.0, 0.0], [0.0, 0.0, -60.0])
        },
    );
    fixture
        .world_mut()
        .entity_mut(plane)
        .insert(FlightEquipment {
            boost_capacity_units: 0.01,
            ..FlightEquipment::EMPTY
        });

    let mut previous = f64::INFINITY;
    for _ in 0..20 {
        fixture.step(1);
        let reserve = aircraft_record(&fixture, plane).boost_capacity_units();
        assert!(
            reserve <= previous + 1e-12,
            "the reserve must not refill: {previous} -> {reserve}"
        );
        previous = reserve;
    }
    assert_eq!(previous, 0.0, "the declared reserve is fully consumed");
    let output = aircraft_record(&fixture, plane)
        .last_output()
        .expect("a driven tick leaves an output");
    assert_eq!(
        output.accepted_boost_consumption, 0.0,
        "an empty reserve accepts no boost"
    );
    assert!(
        (output.instrument_state.thrust_n - 9_000.0).abs() < 1.0,
        "an exhausted reserve contributes no thrust: {}",
        output.instrument_state.thrust_n
    );
}

/// A corrupt equipment record refuses the tick by name and changes nothing; the
/// record is retried, and once fixed the aircraft flies again. Removing the
/// producer tears it down without breaking the flight.
#[test]
fn accept_f24_c_corrupt_equipment_refuses_then_retries() {
    let mut fixture = fixture();
    let plane = aircraft(
        &mut fixture,
        &FlightSpawnSpec {
            engine: EngineState::direct(1.0),
            command: FlightInput::try_new(0.0, 0.0, 0.0, 1.0, false).expect("valid input"),
            ..FlightSpawnSpec::level_at([0.0, 400.0, 0.0], [0.0, 0.0, -60.0])
        },
    );

    fixture.step(1);
    let before = aircraft_record(&fixture, plane)
        .last_output()
        .expect("a driven tick leaves an output");
    let mass_before = fixture.world().get::<Mass>(plane).expect("mass").0;

    // A negative reserve is refused before any field is applied.
    fixture
        .world_mut()
        .entity_mut(plane)
        .insert(FlightEquipment {
            loadout: LoadoutMass {
                fuel_kg: 100.0,
                ..LoadoutMass::EMPTY
            },
            damage: DamageState::PRISTINE,
            boost_capacity_units: -1.0,
        });
    fixture.step(1);
    let tick_report = report(&fixture);
    assert_eq!(tick_report.refused, 1);
    assert_eq!(tick_report.driven, 1, "only the first tick flew");
    match &tick_report.last_refusal {
        Some(refusal) => assert!(
            matches!(
                refusal.reason,
                FlightRefusalReason::Equipment(FlightAircraftError::Negative {
                    field: "boost_capacity_units"
                })
            ),
            "the refusal names the corrupt equipment field: {:?}",
            refusal.reason
        ),
        None => panic!("a refused equipment tick must be recorded"),
    }
    let after = aircraft_record(&fixture, plane);
    assert_eq!(
        after.loadout(),
        LoadoutMass::EMPTY,
        "a refused equipment record applies nothing, not even its valid fields"
    );
    assert_eq!(
        fixture.world().get::<Mass>(plane).expect("mass").0,
        mass_before,
        "the integrator's mass did not change"
    );
    assert_eq!(
        after.last_output(),
        Some(before),
        "a refused tick keeps the last measured output"
    );

    // Fix the record: the next tick retries it and flies again.
    fixture
        .world_mut()
        .get_mut::<FlightEquipment>(plane)
        .expect("equipment")
        .boost_capacity_units = 0.0;
    fixture.step(1);
    let tick_report = report(&fixture);
    assert_eq!(tick_report.driven, 2);
    assert_eq!(
        tick_report.refused, 1,
        "the fixed record stops being refused"
    );
    assert_eq!(
        fixture.world().get::<Mass>(plane).expect("mass").0,
        1300.0,
        "the fixed loadout now reaches the integrator"
    );

    // Teardown: removing the producer leaves the aircraft flying its last bound
    // equipment without a refusal.
    fixture
        .world_mut()
        .entity_mut(plane)
        .remove::<FlightEquipment>();
    fixture.step(5);
    let tick_report = report(&fixture);
    assert_eq!(tick_report.refused, 1, "a removed producer is not an error");
    assert_eq!(tick_report.driven, 7);
}

// ---------------------------------------------------------------------------
// Instrument consumer
// ---------------------------------------------------------------------------

/// The panel consumer reads the tick the driver measured, carries the synthetic
/// evidence class, and is not refreshed by a tick that measured nothing.
#[test]
fn accept_f24_c_instruments_publish_the_measured_tick_with_synthetic_evidence() {
    let mut fixture = fixture();
    let plane = aircraft(
        &mut fixture,
        &FlightSpawnSpec {
            engine: EngineState::direct(1.0),
            command: FlightInput::try_new(0.0, 0.0, 0.0, 1.0, false).expect("valid input"),
            ..FlightSpawnSpec::level_at([0.0, 400.0, 0.0], [0.0, 0.0, -60.0])
        },
    );
    fixture
        .world_mut()
        .entity_mut(plane)
        .insert(FlightInstruments {
            state: cs_sim::flight::InstrumentState {
                airspeed_mps: -1.0,
                angle_of_attack_rad: 0.0,
                sideslip_rad: 0.0,
                dynamic_pressure_pa: 0.0,
                lift_coefficient: 0.0,
                drag_coefficient: 0.0,
                stall_scale: 0.0,
                thrust_n: 0.0,
            },
            evidence: FlightEvidenceClass::InstallationBacked,
            tick: 0,
        });

    fixture.step(60);
    let panel = fixture
        .world()
        .get::<FlightInstruments>(plane)
        .expect("the panel is present");
    let output = aircraft_record(&fixture, plane)
        .last_output()
        .expect("a driven tick leaves an output");
    assert_eq!(
        panel.state, output.instrument_state,
        "the panel shows the driver's measured state"
    );
    assert_eq!(
        panel.tick, 60,
        "the panel is stamped with the measuring tick"
    );
    assert_eq!(panel.evidence, FlightEvidenceClass::Synthetic);
    assert!(panel.state.airspeed_mps > 0.0);
    assert_eq!(report(&fixture).instrumented, 60);

    // An equipment refusal measures nothing, so the panel keeps its last
    // measured tick and the publish stays put.
    let measured_tick = panel.tick;
    let measured_state = panel.state;
    let mut bad = FlightEquipment::EMPTY;
    bad.boost_capacity_units = f64::NAN;
    fixture.world_mut().entity_mut(plane).insert(bad);
    fixture.step(1);
    let panel = fixture
        .world()
        .get::<FlightInstruments>(plane)
        .expect("the panel is present");
    assert_eq!(panel.tick, measured_tick, "a refused tick does not publish");
    assert_eq!(panel.state, measured_state);
    assert_eq!(
        report(&fixture).instrumented,
        60,
        "only measured ticks publish"
    );
}

/// The evidence class is derived from the tuning's origin: a synthetic or
/// designed tuning can never be published as original-reference.
#[test]
fn accept_f24_c_evidence_class_never_calls_a_synthetic_result_original() {
    assert_eq!(
        FlightEvidenceClass::from_origin(&Origin::SyntheticFixture),
        FlightEvidenceClass::Synthetic
    );
    assert_eq!(
        FlightEvidenceClass::from_origin(&Origin::Designed),
        FlightEvidenceClass::Synthetic
    );
    let installation = Origin::Installation {
        source: SourceSpan::new(
            ContentHash::from_bytes([0u8; 32]),
            "container.cab",
            None,
            0,
            1,
            None,
        )
        .expect("a valid span"),
    };
    assert_eq!(
        FlightEvidenceClass::from_origin(&installation),
        FlightEvidenceClass::InstallationBacked
    );
    // Even installation-backed values are not an original-reference claim.
    assert!(!FlightEvidenceClass::from_origin(&installation).is_synthetic());
    assert_eq!(FlightEvidenceClass::Synthetic.label(), "synthetic");
}

// ---------------------------------------------------------------------------
// The four AC03 maneuver traces, measured through the runtime
// ---------------------------------------------------------------------------

/// Acceleration: full throttle raises the airspeed, and the gain is inside the
/// declared synthetic envelope.
#[test]
fn accept_f24_c_acceleration_trace_stays_in_its_synthetic_envelope() {
    let mut fixture = fixture();
    let model = FlightModel::new(synthetic_fixed_wing());
    let plane = aircraft_with(
        &mut fixture,
        model.clone(),
        &FlightSpawnSpec {
            engine: EngineState::direct(1.0),
            command: FlightInput::try_new(0.0, 0.0, 0.0, 1.0, false).expect("valid"),
            ..FlightSpawnSpec::level_at([0.0, 400.0, 0.0], [0.0, 0.0, -60.0])
        },
    );

    let start = speed(&sample_of(&fixture, plane));
    fixture.step(240);
    let end = speed(&sample_of(&fixture, plane));
    let gain = end - start;
    println!("MEASURE acceleration.full_throttle_speed_gain_mps = {gain}");
    assert_envelope("acceleration.full_throttle_speed_gain_mps", gain);
    assert!(
        gain > 0.0,
        "full throttle must accelerate: {start} -> {end}"
    );
    assert_synthetic_label(&model);
}

/// Sustained turn: a held yaw command turns the aircraft, and the heading change
/// is inside the declared synthetic envelope.
#[test]
fn accept_f24_c_sustained_turn_trace_stays_in_its_synthetic_envelope() {
    let mut fixture = fixture();
    let model = FlightModel::new(synthetic_fixed_wing());
    let plane = aircraft_with(
        &mut fixture,
        model.clone(),
        &FlightSpawnSpec {
            engine: EngineState::direct(1.0),
            command: FlightInput::try_new(0.0, 0.0, 1.0, 1.0, false).expect("valid"),
            ..FlightSpawnSpec::level_at([0.0, 400.0, 0.0], [0.0, 0.0, -60.0])
        },
    );

    let start = heading(&fixture, plane);
    fixture.step(240);
    let change = (heading(&fixture, plane) - start).abs();
    println!("MEASURE turn.heading_change_rad = {change}");
    assert_envelope("turn.heading_change_rad", change);
    assert_synthetic_label(&model);
}

/// Roll: a full roll command reaches a bounded peak rate and, released, damps to
/// rest; both measurements are inside their declared synthetic envelopes.
#[test]
fn accept_f24_c_roll_trace_stays_in_its_synthetic_envelopes() {
    let mut fixture = fixture();
    let model = FlightModel::new(synthetic_fixed_wing());
    let plane = aircraft_with(
        &mut fixture,
        model.clone(),
        &FlightSpawnSpec {
            engine: EngineState::direct(1.0),
            command: FlightInput::try_new(0.0, 1.0, 0.0, 1.0, false).expect("valid"),
            ..FlightSpawnSpec::level_at([0.0, 400.0, 0.0], [0.0, 0.0, -60.0])
        },
    );

    let mut peak: f64 = 0.0;
    for _ in 0..120 {
        fixture.step(1);
        let rate = f64::from(sample_of(&fixture, plane).angular_velocity_rad_s[2]).abs();
        peak = peak.max(rate);
    }
    println!("MEASURE roll.peak_rate_radps = {peak}");
    assert_envelope("roll.peak_rate_radps", peak);

    fixture
        .world_mut()
        .get_mut::<FlightAircraft>(plane)
        .expect("flight record")
        .set_command(FlightInput::try_new(0.0, 0.0, 0.0, 1.0, false).expect("valid"))
        .expect("valid command");
    fixture.step(120);
    let settled = f64::from(sample_of(&fixture, plane).angular_velocity_rad_s[2]).abs();
    println!("MEASURE roll.released_settled_rate_radps = {settled}");
    assert_envelope("roll.released_settled_rate_radps", settled);
    assert_synthetic_label(&model);
}

/// Stall recovery: a stall entry drives the stall factor to its residual and a
/// nose-down recovery brings it back; both measurements are inside their
/// declared synthetic envelopes.
#[test]
fn accept_f24_c_stall_recovery_trace_stays_in_its_synthetic_envelopes() {
    let mut fixture = fixture();
    let model = FlightModel::new(synthetic_fixed_wing());
    let plane = aircraft_with(
        &mut fixture,
        model.clone(),
        &FlightSpawnSpec {
            engine: EngineState::direct(1.0),
            command: FlightInput::try_new(1.0, 0.0, 0.0, 1.0, false).expect("valid"),
            ..FlightSpawnSpec::level_at([0.0, 500.0, 0.0], [0.0, 0.0, -80.0])
        },
    );

    let mut minimum: f64 = 1.0;
    for _ in 0..180 {
        fixture.step(1);
        let scale = aircraft_record(&fixture, plane)
            .last_output()
            .expect("a driven tick leaves an output")
            .instrument_state
            .stall_scale;
        minimum = minimum.min(scale);
    }
    println!("MEASURE stall.minimum_stall_scale = {minimum}");
    assert_envelope("stall.minimum_stall_scale", minimum);
    assert!(minimum < 1.0, "the entry must actually stall: {minimum}");

    // Push the nose back down; the angle of attack falls out of the stall and
    // the stall factor recovers.
    fixture
        .world_mut()
        .get_mut::<FlightAircraft>(plane)
        .expect("flight record")
        .set_command(FlightInput::try_new(-1.0, 0.0, 0.0, 1.0, false).expect("valid"))
        .expect("valid command");
    let mut recovered: f64 = 0.0;
    for _ in 0..240 {
        fixture.step(1);
        let scale = aircraft_record(&fixture, plane)
            .last_output()
            .expect("a driven tick leaves an output")
            .instrument_state
            .stall_scale;
        recovered = recovered.max(scale);
    }
    println!("MEASURE stall.recovered_stall_scale = {recovered}");
    assert_envelope("stall.recovered_stall_scale", recovered);
    assert_synthetic_label(&model);
}

/// The improved profile's declared assist reaches the runtime: the same banked
/// state produces no assist torque under the fidelity profile and a bounded
/// longitudinal torque under the improved one.
#[test]
fn accept_f24_c_improved_profile_assist_reaches_the_runtime() {
    let banked = Quaternion::from_axis_angle(
        UnitVec3::try_new([0.0, 0.0, -1.0]).expect("unit axis"),
        Radians(0.4),
    )
    .expect("a valid axis and angle produce a unit quaternion");
    let spawn = FlightSpawnSpec {
        engine: EngineState::direct(1.0),
        orientation: banked,
        ..FlightSpawnSpec::level_at([0.0, 400.0, 0.0], [0.0, 0.0, -60.0])
    };

    let fidelity_model =
        declared_flight_model(&declared_synthetic_airframe(), HandlingProfile::Fidelity)
            .expect("fidelity model");
    let mut fidelity_fixture = fixture();
    let fidelity_plane = aircraft_with(&mut fidelity_fixture, fidelity_model, &spawn);
    fidelity_fixture.step(1);
    let fidelity_assist = aircraft_record(&fidelity_fixture, fidelity_plane)
        .last_output()
        .expect("output")
        .diagnostics
        .assist_torque_nm;
    assert_eq!(
        fidelity_assist,
        [0.0, 0.0, 0.0],
        "the fidelity profile disables assists"
    );

    let improved_model = declared_flight_model(
        &declared_synthetic_improved_airframe(),
        HandlingProfile::Improved,
    )
    .expect("improved model");
    let mut improved_fixture = fixture();
    let improved_plane = aircraft_with(&mut improved_fixture, improved_model, &spawn);
    improved_fixture.step(1);
    let improved_assist = aircraft_record(&improved_fixture, improved_plane)
        .last_output()
        .expect("output")
        .diagnostics
        .assist_torque_nm;
    let magnitude = improved_assist
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    println!("MEASURE improved.assist_torque_nm = {magnitude}");
    assert!(
        magnitude > 100.0,
        "the improved profile's assist must contribute: {improved_assist:?}"
    );
    assert!(
        magnitude <= 5_000.0,
        "the assist stays within its declared maximum: {improved_assist:?}"
    );
}
