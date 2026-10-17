//! F24-B acceptance tests: the fixed-wing force production path on the real
//! Avian schedule.
//!
//! Spec: `specs/F24-fixed-wing-flight-engine-stall-and-arcade-assists.md`,
//! stage `### F24-B`. Task test prefix: `accept_f24_b_`. Minimum scenario:
//! "Power-off climb loses energy; a dive converts altitude into speed."
//!
//! These tests drive production code only: the world is the real
//! [`PhysicsFixture`] (pinned Bevy/Avian plugin group, F23-A adapter, one
//! integration per fixed tick), bodies enter it through
//! [`spawn_flight_body`], forces are computed by `cs_sim`'s production
//! [`FlightModel`] and applied through the production [`ForceRequests`]
//! one-tick queue. Deleting the driver, dropping the gravity term or
//! integrating a second time each makes one of them fail.
//!
//! Every value here is newly authored synthetic fixture data — the synthetic
//! airframe of `cs_sim::flight::synthetic_fixed_wing`, never original game
//! data — and no test reads `CS_GAME_DIR`.

use avian3d::prelude::{
    AngularInertia, AngularVelocity, ComputedMass, Gravity, LinearVelocity, Mass, Position,
    Rotation,
};
use bevy::prelude::{Entity, Vec3};
use cs_app::physics::{
    BodyMode, BodySpec, FixtureBodySpec, FlightAircraft, FlightAircraftError, FlightForcesPlugin,
    FlightRefusalReason, FlightSpawnError, FlightSpawnSpec, FlightTickReport, PhysicsFixture,
    PhysicsSample, spawn_body, spawn_flight_body,
};
use cs_sim::collision::{CollisionLayer, ShapeClass};
use cs_sim::flight::{
    EngineState, FlightEnvironment, FlightInput, FlightModel, LoadoutMass, ModelKind,
    synthetic_fixed_wing,
};
use cs_types::space::{Quaternion, Radians, UnitVec3};

/// The synthetic airframe's declared empty mass, in kg.
const AIRFRAME_MASS_KG: f64 = 1200.0;
/// The sea-level gravity the model applies, in m/s².
const GRAVITY_MPS2: f64 = 9.806_65;

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

/// Spawns one synthetic fixed-wing aircraft through the production path.
fn aircraft(fixture: &mut PhysicsFixture, spec: &FlightSpawnSpec) -> Entity {
    spawn_flight_body(
        fixture.world_mut(),
        FlightModel::new(synthetic_fixed_wing()),
        spec,
    )
    .expect("the spawn spec is valid")
}

/// Reads the [`FlightAircraft`] record of a spawned body.
fn aircraft_record(fixture: &PhysicsFixture, entity: Entity) -> FlightAircraft {
    fixture
        .world()
        .get::<FlightAircraft>(entity)
        .expect("a spawned flight body carries FlightAircraft")
        .clone()
}

/// Reads one body's authoritative pose and velocities, the same components
/// the production driver reads.
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

/// Translational kinetic plus gravitational potential energy, in joules.
fn mechanical_energy(sample: &PhysicsSample, total_mass_kg: f64) -> f64 {
    let speed_sq = sample
        .linear_velocity_m_s
        .iter()
        .map(|component| f64::from(*component) * f64::from(*component))
        .sum::<f64>();
    0.5 * total_mass_kg * speed_sq + total_mass_kg * GRAVITY_MPS2 * f64::from(sample.position_m[1])
}

fn speed(sample: &PhysicsSample) -> f64 {
    sample
        .linear_velocity_m_s
        .iter()
        .map(|component| f64::from(*component) * f64::from(*component))
        .sum::<f64>()
        .sqrt()
}

/// A pitched orientation: `angle` about the lateral (+X) axis; negative is
/// nose-down because body forward is -Z.
fn pitched(angle_rad: f64) -> Quaternion {
    Quaternion::from_axis_angle(
        UnitVec3::try_new([1.0, 0.0, 0.0]).expect("unit axis"),
        Radians(angle_rad),
    )
    .expect("a valid axis and angle produce a unit quaternion")
}

/// AC02 minimum scenario, first half: a powered-off climb bleeds total
/// mechanical energy every tick — drag does the only non-conservative work —
/// while altitude rises and airspeed falls. Removing the force path leaves a
/// body drifting at constant speed with constant energy, and dropping the
/// drag term leaves a conservative system whose energy stays level: both
/// fail here.
#[test]
fn accept_f24_b_power_off_climb_loses_energy() {
    let mut fixture = fixture();
    let plane = aircraft(
        &mut fixture,
        &FlightSpawnSpec::level_at([0.0, 300.0, 0.0], [0.0, 15.0, -50.0]),
    );

    let start = sample_of(&fixture, plane);
    let initial_energy = mechanical_energy(&start, AIRFRAME_MASS_KG);
    let mut previous_energy = initial_energy;
    for _ in 0..60 {
        fixture.step(1);
        let sample = sample_of(&fixture, plane);
        let energy = mechanical_energy(&sample, AIRFRAME_MASS_KG);
        assert!(
            energy < previous_energy,
            "a power-off aircraft must bleed energy every tick ({energy} !< {previous_energy})"
        );
        previous_energy = energy;
    }
    let end = sample_of(&fixture, plane);
    assert!(
        end.position_m[1] > 303.0,
        "the climb must gain altitude: {:?}",
        end.position_m
    );
    assert!(
        speed(&end) < speed(&start) - 1.0,
        "climbing without power bleeds speed: {} -> {}",
        speed(&start),
        speed(&end)
    );
    assert!(
        mechanical_energy(&end, AIRFRAME_MASS_KG) < initial_energy - 10_000.0,
        "drag must remove a measurable amount of energy"
    );
    // The climb ran through the production tick: one request per tick.
    let report = report(&fixture);
    assert_eq!(report.driven, 60);
    assert_eq!(report.refused, 0);
    assert_eq!(fixture.ledger().total_applied_requests, 60);
}

/// AC02 minimum scenario, second half: a powered-off aircraft held in a dive
/// converts altitude into airspeed — the nose stays down, gravity outruns
/// drag along the flight path, and the speed climbs while the altitude
/// falls. A path without gravity can only slow the diving body; a path
/// without the driver leaves it drifting level: both fail here.
#[test]
fn accept_f24_b_dive_converts_altitude_into_speed() {
    let mut fixture = fixture();
    let plane = aircraft(
        &mut fixture,
        &FlightSpawnSpec {
            orientation: pitched(-0.9),
            command: FlightInput::try_new(-0.3, 0.0, 0.0, 0.0, false).expect("valid dive input"),
            ..FlightSpawnSpec::level_at([0.0, 500.0, 0.0], [0.0, 0.0, -60.0])
        },
    );

    let start = sample_of(&fixture, plane);
    fixture.step(240);
    let end = sample_of(&fixture, plane);

    assert!(
        end.position_m[1] < start.position_m[1] - 50.0,
        "the dive must lose altitude: {} -> {}",
        start.position_m[1],
        end.position_m[1]
    );
    assert!(
        speed(&end) > speed(&start) + 5.0,
        "the dive must convert altitude into speed: {} -> {}",
        speed(&start),
        speed(&end)
    );
    assert!(
        mechanical_energy(&end, AIRFRAME_MASS_KG) < mechanical_energy(&start, AIRFRAME_MASS_KG),
        "even a diving aircraft loses total energy to drag"
    );
    // The held -0.3 pitch command is a rate command: the nose is still down
    // because the bounded controller kept driving the attitude.
    let rotation = fixture
        .world()
        .get::<Rotation>(plane)
        .expect("a flight body has a rotation");
    let nose_down = (rotation.0 * Vec3::new(0.0, 0.0, -1.0)).y;
    assert!(
        nose_down < -0.5,
        "a held nose-down command keeps the nose down: {nose_down}"
    );
}

/// AC01 through the runtime: a flight body at rest with a dead engine falls —
/// every readback stays finite and the only force is the model's world-space
/// gravity, so it descends without drift.
#[test]
fn accept_f24_b_zero_airspeed_falls_under_model_gravity() {
    let mut fixture = fixture();
    let plane = aircraft(
        &mut fixture,
        &FlightSpawnSpec::level_at([0.0, 50.0, 0.0], [0.0, 0.0, 0.0]),
    );

    fixture.step(60);
    let sample = sample_of(&fixture, plane);
    for value in sample
        .position_m
        .into_iter()
        .chain(sample.linear_velocity_m_s)
        .chain(sample.angular_velocity_rad_s)
    {
        assert!(value.is_finite(), "every readback must be finite");
    }
    assert!(
        sample.linear_velocity_m_s[1] < -4.0,
        "one half second of model gravity must accelerate the parked aircraft down: {:?}",
        sample.linear_velocity_m_s
    );
    assert!(
        sample.position_m[1] < 49.0,
        "the aircraft must have fallen: {:?}",
        sample.position_m
    );
    assert!(
        sample.linear_velocity_m_s[0].abs() < 0.01 && sample.linear_velocity_m_s[2].abs() < 0.01,
        "a zero-airspeed aircraft must fall straight down: {:?}",
        sample.linear_velocity_m_s
    );
}

/// The engine path: a throttle command spins the spool up at the tuning's
/// response rate, so thrust ramps over the first ticks instead of jumping to
/// full, and the aircraft accelerates along its nose.
#[test]
fn accept_f24_b_throttle_spools_then_accelerates() {
    let mut fixture = fixture();
    let plane = aircraft(
        &mut fixture,
        &FlightSpawnSpec {
            engine: EngineState::direct(0.0),
            command: FlightInput::try_new(0.0, 0.0, 0.0, 1.0, false).expect("valid input"),
            ..FlightSpawnSpec::level_at([0.0, 300.0, 0.0], [0.0, 0.0, -55.0])
        },
    );

    fixture.step(6);
    let early_thrust = aircraft_record(&fixture, plane)
        .last_output()
        .expect("a driven tick leaves an output")
        .instrument_state
        .thrust_n;
    fixture.step(114);
    let aircraft_late = aircraft_record(&fixture, plane);
    let late = aircraft_late
        .last_output()
        .expect("a driven tick leaves an output");
    let sample = sample_of(&fixture, plane);

    assert!(
        early_thrust > 400.0 && early_thrust < 5_000.0,
        "the spool must lag the throttle command: early thrust {early_thrust}"
    );
    assert!(
        (late.instrument_state.thrust_n - 9_000.0).abs() < 1.0,
        "a full throttle reaches the declared maximum after spool-up: {}",
        late.instrument_state.thrust_n
    );
    assert!(
        speed(&sample) > 56.0,
        "full thrust must accelerate the aircraft: {}",
        speed(&sample)
    );
    // And it accelerates along the nose, not sideways: Δv is mostly -Z.
    assert!(
        sample.linear_velocity_m_s[2] < -56.0,
        "thrust acts along body forward: {:?}",
        sample.linear_velocity_m_s
    );
}

/// The bounded arcade controller: a full roll command converges to at most
/// the declared maximum roll rate, about the longitudinal axis only, and
/// releasing the command damps the rate back to zero instead of letting it
/// persist.
#[test]
fn accept_f24_b_roll_command_produces_a_bounded_body_rate() {
    let mut fixture = fixture();
    let plane = aircraft(
        &mut fixture,
        &FlightSpawnSpec {
            engine: EngineState::direct(1.0),
            command: FlightInput::try_new(0.0, 1.0, 0.0, 1.0, false).expect("valid input"),
            ..FlightSpawnSpec::level_at([0.0, 400.0, 0.0], [0.0, 0.0, -60.0])
        },
    );

    fixture.step(120);
    let rolling = sample_of(&fixture, plane);
    let max_rate = synthetic_fixed_wing().angular.max_rate_radps[0];
    let measured = f64::from(rolling.angular_velocity_rad_s[2]);
    assert!(
        measured < 0.0 && measured.abs() <= max_rate + 0.05,
        "a full roll command must not exceed the declared rate {max_rate}: {measured}"
    );
    assert!(
        measured.abs() > 1.0,
        "a full roll command must actually roll: {measured}"
    );
    assert!(
        rolling.angular_velocity_rad_s[0].abs() < 0.2
            && rolling.angular_velocity_rad_s[1].abs() < 0.2,
        "a pure roll command must not command pitch or yaw: {:?}",
        rolling.angular_velocity_rad_s
    );
    // The bank is real: the wings are no longer level.
    let rotation = fixture.world().get::<Rotation>(plane).expect("rotation");
    let up = rotation.0 * Vec3::new(0.0, 1.0, 0.0);
    assert!(up.y < 0.6, "a sustained roll banks the aircraft: up {up}");

    // Releasing the command lets the bounded damping bring the rate back.
    fixture
        .world_mut()
        .get_mut::<FlightAircraft>(plane)
        .expect("flight record")
        .set_command(FlightInput::try_new(0.0, 0.0, 0.0, 1.0, false).expect("valid"))
        .expect("valid command");
    fixture.step(120);
    let settled = sample_of(&fixture, plane);
    assert!(
        settled.angular_velocity_rad_s[2].abs() < 0.05,
        "a released command must damp the roll rate to zero: {:?}",
        settled.angular_velocity_rad_s
    );
}

/// AC04's runtime half: one fixed tick is one force computation and one
/// applied request — render-frame shape never enters the count, and the
/// request the model produces is applied to the tick that produced it.
#[test]
fn accept_f24_b_each_fixed_tick_produces_exactly_one_applied_request() {
    let mut fixture = fixture();
    aircraft(
        &mut fixture,
        &FlightSpawnSpec {
            engine: EngineState::direct(0.5),
            command: FlightInput::try_new(0.0, 0.0, 0.0, 0.5, false).expect("valid input"),
            ..FlightSpawnSpec::level_at([0.0, 400.0, 0.0], [0.0, 0.0, -55.0])
        },
    );

    fixture.step(240);
    let report = report(&fixture);
    let ledger = fixture.ledger();
    assert_eq!(report.ticks, 240);
    assert_eq!(ledger.ticks, 240, "the driver ticks with the fixed clock");
    assert_eq!(ledger.integrations, 240, "one integration per fixed tick");
    assert_eq!(
        report.driven, 240,
        "one force computation per aircraft tick"
    );
    assert_eq!(
        ledger.total_applied_requests, 240,
        "every computed tick reached the integrator on its own tick"
    );
    assert_eq!(ledger.total_dropped_requests, 0);
    assert_eq!(report.refused, 0);
}

/// A held boost drains the declared reserve and stops contributing when it is
/// empty: `accepted_boost_consumption` is zero after exhaustion and the
/// reserve never goes negative — a press while empty consumes nothing.
#[test]
fn accept_f24_b_boost_drains_the_reserve_then_stops() {
    let mut fixture = fixture();
    let plane = aircraft(
        &mut fixture,
        &FlightSpawnSpec {
            engine: EngineState::direct(1.0),
            boost_capacity_units: 0.02,
            command: FlightInput::try_new(0.0, 0.0, 0.0, 1.0, true).expect("valid input"),
            ..FlightSpawnSpec::level_at([0.0, 400.0, 0.0], [0.0, 0.0, -55.0])
        },
    );

    fixture.step(1);
    let boosted = aircraft_record(&fixture, plane)
        .last_output()
        .expect("a driven tick leaves an output");
    assert!(boosted.accepted_boost_consumption > 0.0);
    assert!(boosted.instrument_state.thrust_n > 9_000.0);
    assert!(aircraft_record(&fixture, plane).boost_capacity_units() < 0.02);

    fixture.step(60);
    let aircraft_state = aircraft_record(&fixture, plane);
    let exhausted = aircraft_state
        .last_output()
        .expect("a driven tick leaves an output");
    assert_eq!(aircraft_state.boost_capacity_units(), 0.0);
    assert_eq!(
        exhausted.accepted_boost_consumption, 0.0,
        "an empty reserve accepts no boost and consumes nothing"
    );
    assert!(
        (exhausted.instrument_state.thrust_n - 9_000.0).abs() < 1.0,
        "an exhausted boost contributes no thrust: {}",
        exhausted.instrument_state.thrust_n
    );
}

/// Bodies that are not flying stay out of the force path: a kinematic
/// aircraft is parked (counted, not flown) and a plain dynamic body never
/// receives a flight force.
#[test]
fn accept_f24_b_parked_and_plain_bodies_are_not_flown() {
    let mut fixture = fixture();
    // A scripted kinematic aircraft: carries the flight record but is not
    // dynamic, so the driver parks it instead of forcing it.
    let parked = spawn_body(
        fixture.world_mut(),
        &BodySpec {
            layer: CollisionLayer::Aircraft,
            shape: ShapeClass::Solid,
            mode: BodyMode::Kinematic,
            mass_kg: 1200.0,
            half_extents_m: [0.5, 0.5, 0.5],
            position_m: [0.0, 100.0, 0.0],
            linear_velocity_m_s: [0.0, 0.0, -10.0],
        },
    )
    .expect("valid body");
    fixture.world_mut().entity_mut(parked).insert(
        FlightAircraft::new(
            FlightModel::new(synthetic_fixed_wing()),
            FlightEnvironment::SEA_LEVEL,
        )
        .expect("valid aircraft"),
    );
    // And one real flying aircraft for contrast.
    let flying = aircraft(
        &mut fixture,
        &FlightSpawnSpec::level_at([0.0, 300.0, 0.0], [0.0, 0.0, -40.0]),
    );

    let parked_start = sample_of(&fixture, parked);
    fixture.step(60);
    let parked_end = sample_of(&fixture, parked);
    let report = report(&fixture);
    let ledger = fixture.ledger();

    assert_eq!(
        report.parked, 60,
        "a kinematic aircraft is parked, not flown"
    );
    assert_eq!(report.driven, 60, "only the dynamic aircraft is driven");
    assert_eq!(
        ledger.total_applied_requests, 60,
        "the parked aircraft must not consume force requests"
    );
    // The kinematic body drifts at exactly its scripted velocity — no
    // gravity, no forces — which is what "scripted actor" means.
    assert_eq!(
        parked_end.linear_velocity_m_s,
        parked_start.linear_velocity_m_s
    );
    assert!(
        (parked_end.position_m[2] - (parked_start.position_m[2] - 5.0)).abs() < 0.5,
        "kinematic motion is unaccelerated: {:?}",
        parked_end.position_m
    );
    assert!(
        speed(&sample_of(&fixture, flying)) > 0.0,
        "the other aircraft still flies"
    );
}

/// The contract's one-gravity rule is enforced loudly: with a non-zero Avian
/// `Gravity` the flight tick is refused and counted, and the body falls under
/// the world's gravity alone — never twice.
#[test]
fn accept_f24_b_gravity_conflict_refuses_the_tick() {
    let mut fixture = fixture();
    // Deliberately violate the flight-world rule: the model applies its own
    // gravity, so Avian's must stay zero.
    fixture
        .world_mut()
        .insert_resource(Gravity(Vec3::new(0.0, -9.806_65, 0.0)));
    let plane = aircraft(
        &mut fixture,
        &FlightSpawnSpec::level_at([0.0, 100.0, 0.0], [0.0, 0.0, 0.0]),
    );

    fixture.step(120);
    let report = report(&fixture);
    assert_eq!(
        report.driven, 0,
        "no flight tick may run under double gravity"
    );
    assert_eq!(report.refused, 120);
    match &report.last_refusal {
        Some(refusal) => {
            assert_eq!(refusal.entity, plane);
            assert!(
                matches!(refusal.reason, FlightRefusalReason::GravityConflict { .. }),
                "the refusal names the conflict: {:?}",
                refusal.reason
            );
        }
        None => panic!("a refused tick must be recorded"),
    }

    let sample = sample_of(&fixture, plane);
    assert!(
        (sample.linear_velocity_m_s[1] + 9.806_65).abs() < 0.5,
        "the body falls under the world's gravity alone, not twice: {:?}",
        sample.linear_velocity_m_s
    );
}

/// The other refusal boundary: an entity that carries the flight record but
/// no `RigidBody` can never take a force, so it is refused by name on every
/// tick — while the aircraft spawned through the production path keeps
/// flying. One bad body never silences the rest of the squadron, and a
/// refused aircraft never fabricates an output.
#[test]
fn accept_f24_b_missing_body_is_refused_by_name_without_stopping_the_others() {
    let mut fixture = fixture();
    let orphan = fixture
        .world_mut()
        .spawn(
            FlightAircraft::new(
                FlightModel::new(synthetic_fixed_wing()),
                FlightEnvironment::SEA_LEVEL,
            )
            .expect("a valid aircraft record"),
        )
        .id();
    let flying = aircraft(
        &mut fixture,
        &FlightSpawnSpec::level_at([0.0, 300.0, 0.0], [0.0, 0.0, -40.0]),
    );

    fixture.step(10);
    let report = report(&fixture);
    assert_eq!(report.ticks, 10, "the driver runs on every fixed tick");
    assert_eq!(
        report.refused, 10,
        "an aircraft with no rigid body is refused every tick"
    );
    assert_eq!(
        report.driven, 10,
        "one refused aircraft must not stop the healthy one"
    );
    assert_eq!(
        fixture.ledger().total_applied_requests,
        10,
        "the healthy aircraft still reaches the integrator"
    );
    match &report.last_refusal {
        Some(refusal) => {
            assert_eq!(refusal.entity, orphan);
            assert_eq!(refusal.tick, 10, "the refusal carries its tick");
            assert!(
                matches!(refusal.reason, FlightRefusalReason::MissingBody),
                "the refusal names the missing body: {:?}",
                refusal.reason
            );
        }
        None => panic!("a refused tick must be recorded"),
    }
    assert!(
        aircraft_record(&fixture, orphan).last_output().is_none(),
        "a refused aircraft never fabricates an output"
    );
    assert!(
        speed(&sample_of(&fixture, flying)) > 0.0,
        "the healthy aircraft still flies"
    );
}

/// `spawn_flight_body` binds the declared mass properties, not the collider's
/// derived ones: the body's mass is the tuning's airframe mass plus the
/// loadout (the same number the model uses), and the principal inertia lands
/// on the physical axes it is declared against.
#[test]
fn accept_f24_b_spawn_binds_declared_mass_and_inertia() {
    let mut fixture = fixture();
    let plane = aircraft(
        &mut fixture,
        &FlightSpawnSpec {
            loadout: LoadoutMass {
                fuel_kg: 200.0,
                ordnance_kg: 100.0,
                armor_kg: 50.0,
            },
            orientation: pitched(-0.4),
            ..FlightSpawnSpec::level_at([0.0, 300.0, 0.0], [0.0, 0.0, -40.0])
        },
    );

    let mass = fixture.world().get::<Mass>(plane).expect("declared mass");
    assert_eq!(mass.0, 1550.0, "mass = airframe + loadout, exactly");

    let inertia = fixture
        .world()
        .get::<AngularInertia>(plane)
        .expect("declared inertia");
    // Tuning order is (roll, pitch, yaw) = (about +Z, +X, +Y); Avian's
    // principal vector is (about X, Y, Z).
    assert_eq!(
        inertia.principal,
        Vec3::new(2100.0, 2600.0, 1400.0),
        "principal inertia lands on (pitch=X, yaw=Y, roll=Z)"
    );

    let rotation = fixture.world().get::<Rotation>(plane).expect("rotation");
    let forward = rotation.0 * Vec3::new(0.0, 0.0, -1.0);
    assert!(
        forward.y < -0.3,
        "a nose-down spawn must actually face down: {forward}"
    );

    // After a tick the recomputed mass must not have grown behind the
    // declared value's back: the collider's own contribution is excluded by
    // `NoAutoMass`.
    fixture.step(1);
    let computed = fixture
        .world()
        .get::<ComputedMass>(plane)
        .expect("a computed mass");
    assert!(
        (computed.value() - 1550.0).abs() < 0.5,
        "the collider must not inflate the declared mass: {}",
        computed.value()
    );
}

/// The same mass everywhere: a loadout change mid-flight moves the body's
/// integrated `Mass` to the new airframe + loadout total, so the force the
/// model computes and the mass the integrator divides by are the same number
/// (non-negotiable behavior 4).
#[test]
fn accept_f24_b_loadout_change_moves_the_integrated_mass() {
    let mut fixture = fixture();
    let plane = aircraft(
        &mut fixture,
        &FlightSpawnSpec {
            loadout: LoadoutMass {
                fuel_kg: 0.0,
                ordnance_kg: 200.0,
                armor_kg: 0.0,
            },
            ..FlightSpawnSpec::level_at([0.0, 300.0, 0.0], [0.0, 0.0, -40.0])
        },
    );
    fixture.step(1);
    assert_eq!(
        fixture.world().get::<Mass>(plane).expect("mass").0,
        1400.0,
        "spawn binds airframe + ordnance"
    );

    // Drop the ordnance: the next tick must move the integrator's mass too.
    fixture
        .world_mut()
        .get_mut::<FlightAircraft>(plane)
        .expect("flight record")
        .set_loadout(LoadoutMass::EMPTY)
        .expect("valid loadout");
    fixture.step(1);
    let world = fixture.world();
    assert_eq!(
        world.get::<Mass>(plane).expect("mass").0,
        1200.0,
        "the integrator's mass follows the loadout"
    );
    assert!(
        (world
            .get::<ComputedMass>(plane)
            .expect("computed mass")
            .value()
            - 1200.0)
            .abs()
            < 0.5,
        "the recomputed mass follows without collider inflation"
    );
}

/// Instruments are the measured state, not the commanded one: the reported
/// airspeed is the body's true speed through the air and the dynamic pressure
/// follows it.
#[test]
fn accept_f24_b_instruments_report_the_measured_state() {
    let mut fixture = fixture();
    let plane = aircraft(
        &mut fixture,
        &FlightSpawnSpec {
            engine: EngineState::direct(0.75),
            command: FlightInput::try_new(0.0, 0.0, 0.0, 0.75, false).expect("valid input"),
            ..FlightSpawnSpec::level_at([0.0, 300.0, 0.0], [0.0, 0.0, -55.0])
        },
    );

    fixture.step(60);
    let output = aircraft_record(&fixture, plane)
        .last_output()
        .expect("a driven tick leaves an output");
    let measured_speed = speed(&sample_of(&fixture, plane));
    assert!(
        (output.instrument_state.airspeed_mps - measured_speed).abs() < 0.5,
        "the airspeed instrument reads the body's true speed: instrument {} vs body {}",
        output.instrument_state.airspeed_mps,
        measured_speed
    );
    let expected_q =
        0.5 * FlightEnvironment::SEA_LEVEL.air_density_kg_m3 * measured_speed * measured_speed;
    assert!(
        (output.instrument_state.dynamic_pressure_pa - expected_q).abs() / expected_q < 0.02,
        "q follows the measured airspeed"
    );
    assert!(output.instrument_state.stall_scale > 0.9);
    assert_eq!(output.diagnostics.assist_force_n, [0.0; 3]);
    assert_eq!(output.diagnostics.assist_torque_nm, [0.0; 3]);
}

/// The spawn boundary refuses a corrupt spec by name and leaves nothing
/// behind: no half-spawned body, no ghost entity.
#[test]
fn accept_f24_b_spawn_and_setters_refuse_corrupt_values() {
    let mut fixture = fixture();
    let model = || FlightModel::new(synthetic_fixed_wing());
    let entities_before = fixture.world().entities().len();

    let mut bad_position = FlightSpawnSpec::level_at([0.0, 100.0, 0.0], [0.0; 3]);
    bad_position.position_m[0] = f32::NAN;
    assert!(matches!(
        spawn_flight_body(fixture.world_mut(), model(), &bad_position),
        Err(FlightSpawnError::Body(_))
    ));

    let mut bad_command = FlightSpawnSpec::level_at([0.0, 100.0, 0.0], [0.0; 3]);
    bad_command.command = FlightInput {
        yaw: -1.5,
        ..FlightInput::NEUTRAL
    };
    assert!(matches!(
        spawn_flight_body(fixture.world_mut(), model(), &bad_command),
        Err(FlightSpawnError::Aircraft(FlightAircraftError::Input(_)))
    ));

    let mut bad_boost = FlightSpawnSpec::level_at([0.0, 100.0, 0.0], [0.0; 3]);
    bad_boost.boost_capacity_units = -1.0;
    assert!(matches!(
        spawn_flight_body(fixture.world_mut(), model(), &bad_boost),
        Err(FlightSpawnError::Aircraft(FlightAircraftError::Negative {
            field: "boost_capacity_units"
        }))
    ));

    let mut exceptional_tuning = synthetic_fixed_wing();
    exceptional_tuning.model_kind = ModelKind::Exceptional;
    assert!(matches!(
        spawn_flight_body(
            fixture.world_mut(),
            FlightModel::new(exceptional_tuning),
            &FlightSpawnSpec::level_at([0.0, 100.0, 0.0], [0.0; 3]),
        ),
        Err(FlightSpawnError::Aircraft(
            FlightAircraftError::UnsupportedModelKind { .. }
        ))
    ));

    assert_eq!(
        fixture.world().entities().len(),
        entities_before,
        "a refused spawn must leave no half body behind"
    );
}
