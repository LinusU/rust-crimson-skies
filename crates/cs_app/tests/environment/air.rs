//! `accept_f19_b_` tests for the authoritative wind field's effect on flight
//! and projectiles: F19 non-negotiable behavior 2 and acceptance case AC02 —
//! *wind changes affect aircraft airspeed and projectile-relative velocity
//! consistently*.
//!
//! The production code under test is `cs_app::environment::air`:
//! [`AuthoritativeWind`], which lifts the one wind field out of a state and
//! performs the `FLIGHT-PHYSICS` conversion `v_air = v_world - wind_world`,
//! and [`ProjectileMotion`], which carries a projectile's constant
//! air-relative velocity through that same field.
//!
//! The aircraft half is measured by the real
//! [`FlightModel`](cs_sim::flight::FlightModel) reading the
//! [`FlightEnvironment`] this module builds: the test does not compute an
//! airspeed of its own, it compares the model's instrument against the
//! effect's own number and against the projectile's.
//!
//! No original data and no `CS_GAME_DIR`: the winds come from the production
//! [`clear_sky_environment`] / [`storm_environment`] fixtures and from states
//! the tests assemble through [`common::state`].
//!
//! [`clear_sky_environment`]: cs_app::environment::clear_sky_environment
//! [`storm_environment`]: cs_app::environment::storm_environment

use cs_app::environment::{
    AuthoritativeWind, MAX_AIR_VELOCITY_MPS, ProjectileError, ProjectileMotion, WindUnavailable,
};
use cs_content::environment::{EnvironmentState, PrecipitationKind};
use cs_sim::flight::{
    DamageState, EngineState, FlightEnvironment, FlightInput, FlightModel, FlightState,
    LoadoutMass, synthetic_fixed_wing,
};
use cs_types::space::Quaternion;

use crate::common;

/// A cruise world velocity in meters per second: 55 m/s along the body's
/// forward axis (`-Z` per the coordinate convention), far above the
/// airspeed at which any control authority is granted.
const CRUISE_WORLD_MPS: [f64; 3] = [0.0, 0.0, -55.0];

/// The production flight model over the declared synthetic fixed-wing tuning.
fn model() -> FlightModel {
    FlightModel::new(synthetic_fixed_wing())
}

/// The cruise aircraft state: moving at [`CRUISE_WORLD_MPS`] with a spooled
/// engine, level and unrotated.
fn cruise_state() -> FlightState {
    FlightState {
        linear_velocity_mps: CRUISE_WORLD_MPS,
        engine: EngineState::direct(0.75),
        ..FlightState::at_rest(Quaternion::IDENTITY)
    }
}

/// A steady cruise command.
fn cruise_input() -> FlightInput {
    FlightInput::try_new(0.0, 0.0, 0.0, 0.75, false).expect("a cruise input is in range")
}

/// The true airspeed the real flight model reports for `state` in `wind`.
///
/// This is production measurement, not a test-side formula: the model
/// subtracts the environment's own wind internally, so this number is
/// independent evidence that the environment's field reached the aircraft.
fn model_airspeed_m_s(wind: &AuthoritativeWind, state: &FlightState) -> f64 {
    let environment = wind.flight_environment(&FlightEnvironment::SEA_LEVEL);
    let output = model()
        .compute(
            &environment,
            &LoadoutMass::EMPTY,
            &DamageState::PRISTINE,
            state,
            &cruise_input(),
            1.0 / 64.0,
        )
        .expect("a finite state, wind and tuning always compute");
    output.instrument_state.airspeed_mps
}

/// The clear-sky fixture's wind field, through the effect path.
fn clear_wind() -> AuthoritativeWind {
    AuthoritativeWind::from_state(&common::clear().state().clone())
        .expect("the clear-sky fixture authors a wind")
}

/// A state carrying exactly the wind `velocity_m_s` and nothing else that
/// the wind path reads.
fn state_with_wind(velocity_m_s: [f64; 3], claim_id: &str) -> EnvironmentState {
    common::state(
        common::known_wind(velocity_m_s, claim_id),
        common::known_precipitation(PrecipitationKind::Clear, "f19b.test.precipitation"),
        common::known_visibility(1500.0, "f19b.test.visibility"),
    )
}

/// AC02: the same aircraft, at the same world velocity, in three different
/// winds, is seen at three different airspeeds — and a projectile shot through
/// the same air moves with it by exactly the wind's own difference, so the
/// two consumers cannot disagree about the field they are flying in.
#[test]
fn accept_f19_b_wind_changes_aircraft_airspeed_and_projectile_velocity_consistently() {
    let clear = clear_wind();
    let storm = AuthoritativeWind::from_state(&common::storm().state().clone())
        .expect("the storm fixture authors an initial wind");
    let gust = state_with_wind([16.0, 0.0, 2.0], "f19b.test.gust");
    let gust = AuthoritativeWind::from_state(&gust).expect("the gust state authors a wind");

    // The three fields really are three fields.
    assert_eq!(clear.velocity_m_s(), [3.0, 0.0, -1.5]);
    assert_eq!(storm.velocity_m_s(), [8.0, 0.0, 0.0]);
    assert_eq!(gust.velocity_m_s(), [16.0, 0.0, 2.0]);

    // The aircraft's airspeed changes with the wind — measured by the real
    // model, not by a test formula, and equal to the effect's own number in
    // every case.
    let state = cruise_state();
    let airspeeds: Vec<f64> = [&clear, &storm, &gust]
        .iter()
        .map(|wind| model_airspeed_m_s(wind, &state))
        .collect();
    for (wind, airspeed) in [&clear, &storm, &gust].iter().zip(&airspeeds) {
        assert_eq!(
            *airspeed,
            wind.airspeed_m_s(CRUISE_WORLD_MPS),
            "the model's airspeed and the effect's airspeed must be the same number"
        );
        assert!(airspeed.is_finite());
    }

    // All three fixture winds push the air in the aircraft's direction of
    // travel, so each one is a tailwind and the true airspeed *rises* with the
    // wind. A headwind (`+Z`) would lower it instead; the sign is what the
    // `air_relative` conversion fixes, and both directions follow from it.
    assert!(
        airspeeds[0] < airspeeds[1] && airspeeds[1] < airspeeds[2],
        "a stronger tailwind must raise the airspeed: {airspeeds:?}"
    );

    // The projectile reads the same field: its air-relative speed is
    // unchanged by the weather — it is a property of the shot — while its
    // *world* velocity moves by exactly the wind's own difference.
    let shot = ProjectileMotion::try_new([0.0, 0.0, -120.0])
        .expect("a 120 m/s air-relative muzzle speed is representable");
    for wind in [&clear, &storm, &gust] {
        assert_eq!(
            shot.airspeed_m_s(),
            120.0,
            "a projectile's airspeed is its own velocity, not the wind's"
        );
        assert_eq!(
            shot.world_velocity_m_s(wind),
            wind.world_velocity([0.0, 0.0, -120.0]),
            "the projectile's world velocity is its air velocity plus the same wind field"
        );
    }
    let world_delta = [
        shot.world_velocity_m_s(&gust)[0] - shot.world_velocity_m_s(&clear)[0],
        shot.world_velocity_m_s(&gust)[1] - shot.world_velocity_m_s(&clear)[1],
        shot.world_velocity_m_s(&gust)[2] - shot.world_velocity_m_s(&clear)[2],
    ];
    let wind_delta = [
        gust.velocity_m_s()[0] - clear.velocity_m_s()[0],
        gust.velocity_m_s()[1] - clear.velocity_m_s()[1],
        gust.velocity_m_s()[2] - clear.velocity_m_s()[2],
    ];
    assert_eq!(
        world_delta, wind_delta,
        "a projectile must be carried by exactly the wind's difference, not by a second estimate of it"
    );

    // The two consumers are consistent with each other: a projectile shot
    // with the aircraft's own air-relative velocity ends up at the aircraft's
    // world velocity and is measured by the same field, so the two kinds of
    // body cannot be flying different airs.
    for wind in [&clear, &storm, &gust] {
        let matching = ProjectileMotion::try_new(wind.air_relative(CRUISE_WORLD_MPS))
            .expect("the aircraft's air-relative velocity is a representable shot");
        assert_eq!(
            matching.world_velocity_m_s(wind),
            CRUISE_WORLD_MPS,
            "a shot authored with the aircraft's air-relative velocity flies at the aircraft's world velocity"
        );
        assert_eq!(
            wind.airspeed_m_s(matching.world_velocity_m_s(wind)),
            wind.airspeed_m_s(CRUISE_WORLD_MPS),
            "aircraft and projectile are measured by one subtraction of one field"
        );
    }

    // A relative quantity inside one air does not move when the air moves: the
    // same two bodies flying through the same air close at the same rate in
    // every wind, because their air-relative velocities are unchanged. The
    // closing speed takes no wind at all — that is the property.
    let target_air_m_s = [0.0, 0.0, -55.0];
    assert_eq!(shot.closing_speed_m_s(target_air_m_s), 65.0);

    // Hold both bodies' *air* velocities fixed — the aircraft at the airspeed it
    // has in the calm field, the target at its own — and let the wind decide
    // their world velocities. Every world velocity moves by the wind's own
    // difference, and every difference between them stays the same.
    let aircraft_air_m_s = clear.air_relative(CRUISE_WORLD_MPS);
    let calm_aircraft_world = clear.world_velocity(aircraft_air_m_s);
    let gust_aircraft_world = gust.world_velocity(aircraft_air_m_s);
    let calm_target_world = clear.world_velocity(target_air_m_s);
    let gust_target_world = gust.world_velocity(target_air_m_s);
    assert_ne!(
        calm_target_world, gust_target_world,
        "the gust really does carry the target at a different world velocity"
    );
    assert_ne!(
        calm_aircraft_world, gust_aircraft_world,
        "the gust really does carry the aircraft at a different world velocity"
    );
    assert_eq!(
        clear.relative_air_velocity(calm_aircraft_world, calm_target_world),
        gust.relative_air_velocity(gust_aircraft_world, gust_target_world),
        "a relative velocity between two bodies in the same air is the same in every wind"
    );
    assert_eq!(
        clear.relative_air_velocity(calm_aircraft_world, calm_target_world),
        sub3(target_air_m_s, aircraft_air_m_s),
        "a relative velocity is the difference of air-relative velocities, with the wind subtracted once"
    );
    assert_eq!(
        sub3(calm_target_world, calm_aircraft_world),
        sub3(gust_target_world, gust_aircraft_world),
        "the wind shifts both world velocities by the same amount, so the difference between them is untouched"
    );

    // World-space closure against a *held* world velocity does move with the
    // gust, and that is correct rather than convenient: the target is being
    // blown at a different rate relative to the shot.
    let still = AuthoritativeWind::try_new([0.0, 0.0, 0.0]).expect("zero wind is representable");
    assert_eq!(
        shot.world_closing_speed_m_s(&still, CRUISE_WORLD_MPS),
        65.0,
        "in still air a 120 m/s shot behind a 55 m/s aircraft closes at 65 m/s"
    );
    assert_eq!(
        shot.world_closing_speed_m_s(&still, CRUISE_WORLD_MPS),
        shot.closing_speed_m_s(target_air_m_s),
        "in still air the world-space and air-space closures are one number"
    );
    assert_ne!(
        shot.world_closing_speed_m_s(&clear, CRUISE_WORLD_MPS),
        shot.world_closing_speed_m_s(&gust, CRUISE_WORLD_MPS),
        "a held world velocity really is a different closure when the air moves"
    );
}

/// Component-wise difference, spelled out so the assertion above reads as the
/// identity it checks.
fn sub3(lhs: [f64; 3], rhs: [f64; 3]) -> [f64; 3] {
    [lhs[0] - rhs[0], lhs[1] - rhs[1], lhs[2] - rhs[2]]
}

/// The conversion itself: `v_air = v_world - wind_world` and its exact
/// inverse, so no consumer can round-trip a velocity through the field and
/// lose or gain a component.
#[test]
fn accept_f19_b_air_relative_velocity_is_one_lossless_field_conversion() {
    let wind = clear_wind();

    // The air-relative velocity is the world velocity minus the wind, per
    // component — the FLIGHT-PHYSICS convention, not a norm or a scaled
    // copy.
    assert_eq!(
        wind.air_relative(CRUISE_WORLD_MPS),
        [
            CRUISE_WORLD_MPS[0] - 3.0,
            CRUISE_WORLD_MPS[1] - 0.0,
            CRUISE_WORLD_MPS[2] - (-1.5),
        ]
    );

    // Round trip: world → air → world is the identity, so a velocity handed
    // through the field twice is unchanged.
    let once = wind.air_relative(CRUISE_WORLD_MPS);
    assert_eq!(wind.world_velocity(once), CRUISE_WORLD_MPS);

    // A body drifting *with* the wind has no airspeed: the field is what the
    // aircraft's airspeed is measured against, not what it is added to.
    assert_eq!(wind.airspeed_m_s(wind.velocity_m_s()), 0.0);

    // The projectile carries its air-relative velocity through the wind, and
    // its airspeed never depends on which field it flies through.
    let shot = ProjectileMotion::try_new([30.0, 0.0, -40.0]).expect("a representable shot");
    let carried = shot.world_velocity_m_s(&wind);
    assert_eq!(wind.air_relative(carried), shot.air_velocity_m_s());
    assert_eq!(shot.airspeed_m_s(), 50.0, "a 3-4-5 shot flies at 50 m/s");

    // The projectile's `travel_m` is the world velocity times the elapsed
    // ticks — the same field again, integrated.
    let travel = shot
        .travel_m(&wind, 64, 1.0 / 64.0)
        .expect("a finite dt integrates");
    assert_eq!(travel[0], carried[0]);
    assert_eq!(travel[2], carried[2]);

    // A non-finite timestep is refused rather than producing a NaN position.
    assert!(matches!(
        shot.travel_m(&wind, 64, f64::NAN),
        Err(ProjectileError::NonFiniteExtent { field: "dt_s" })
    ));

    // Out-of-range velocities are refused by name instead of being flown.
    match ProjectileMotion::try_new([f64::NAN, 0.0, 0.0]) {
        Err(ProjectileError::OutOfRange {
            component: 0,
            value,
        }) => assert!(value.is_nan()),
        other => panic!("a non-finite shot must be refused, got {other:?}"),
    }
    assert!(
        ProjectileMotion::try_new([MAX_AIR_VELOCITY_MPS * 2.0, 0.0, 0.0]).is_err(),
        "a velocity beyond the representability bound is refused"
    );
}

/// F19 non-negotiable behavior 1 and the failure case of behavior 2: a wind
/// nobody measured is an explicit unknown, and the effect path refuses it.
/// There is no still-air default anywhere in the chain, and the refusal
/// names the claim so a diagnostic can act on it.
#[test]
fn accept_f19_b_an_unknown_wind_is_refused_and_never_becomes_still_air() {
    // A state whose wind is unknown — a real record shape, because
    // `EnvironmentState` accepts an unknown field.
    let unknown = common::state(
        common::unknown(
            "f19b.test.unknown-wind",
            "this test authors no wind measurement",
        ),
        common::known_precipitation(PrecipitationKind::Rain, "f19b.test.precipitation"),
        common::known_visibility(1500.0, "f19b.test.visibility"),
    );

    match AuthoritativeWind::from_state(&unknown) {
        Err(WindUnavailable::Unknown { claim_id, reason }) => {
            assert_eq!(claim_id.as_str(), "f19b.test.unknown-wind");
            assert_eq!(reason, "this test authors no wind measurement");
        }
        other => panic!("an unknown wind must be refused, got {other:?}"),
    }

    // The refusal is not a fallback to zero: nothing in the API can produce a
    // wind from an unknown field, so the only answer is the error. The
    // contrast with a *known* zero wind is the point.
    let calm = state_with_wind([0.0, 0.0, 0.0], "f19b.test.calm");
    let calm = AuthoritativeWind::from_state(&calm).expect("a known zero wind is a real field");
    assert_eq!(calm.velocity_m_s(), [0.0, 0.0, 0.0]);
    assert_ne!(
        calm.airspeed_m_s(CRUISE_WORLD_MPS),
        match AuthoritativeWind::from_state(&unknown) {
            Ok(wind) => wind.airspeed_m_s(CRUISE_WORLD_MPS),
            Err(_) => {
                // There is no value to compare: an unknown wind cannot be
                // quietly read as calm air, which is what a default would do.
                f64::NAN
            }
        },
        "an unknown wind must never produce the calm-air airspeed"
    );

    // Non-finite and out-of-range velocities are refused by name.
    assert!(matches!(
        AuthoritativeWind::try_new([f64::INFINITY, 0.0, 0.0]),
        Err(WindUnavailable::OutOfRange { component: 0, .. })
    ));
    assert!(
        AuthoritativeWind::try_new([MAX_AIR_VELOCITY_MPS * 2.0, 0.0, 0.0]).is_err(),
        "a wind beyond the representability bound is refused"
    );
}

/// The wind the environment's clock installs at its authored tick is the
/// wind flight and projectiles read, so the field is not a definition's
/// stale copy: the gust reaches both consumers at the same tick.
#[test]
fn accept_f19_b_the_wind_the_timeline_installed_is_the_wind_consumers_read() {
    let storm = common::storm();
    let mut clock = cs_app::environment::EnvironmentClock::new(&storm, common::tick_rate())
        .expect("the storm fixture's schedule is strictly increasing");

    // Before the authored gust: the initial field.
    let before = AuthoritativeWind::from_state(clock.state()).expect("the initial wind is known");
    assert_eq!(before.velocity_m_s(), [8.0, 0.0, 0.0]);
    let airspeed_before = before.airspeed_m_s(CRUISE_WORLD_MPS);

    // Advance to exactly the authored gust tick.
    for _ in 0..cs_app::environment::WIND_SHIFT_TICK {
        clock
            .advance_frame(common::FRAME)
            .expect("the clock advances");
    }
    let after = AuthoritativeWind::from_state(clock.state()).expect("the gust wind is known");
    assert_eq!(
        after.velocity_m_s(),
        [16.0, 0.0, 2.0],
        "the installed event's wind is the field consumers read"
    );

    let state = cruise_state();
    assert_ne!(
        before.airspeed_m_s(CRUISE_WORLD_MPS),
        after.airspeed_m_s(CRUISE_WORLD_MPS),
        "the installed gust changes the aircraft's airspeed"
    );
    assert_eq!(
        before.airspeed_m_s(CRUISE_WORLD_MPS),
        airspeed_before,
        "the earlier field is unchanged by a later event"
    );
    assert_eq!(
        after.airspeed_m_s(CRUISE_WORLD_MPS),
        model_airspeed_m_s(&after, &state),
        "the gust reaches the real model through the same field"
    );
    assert_ne!(
        model_airspeed_m_s(&before, &state),
        model_airspeed_m_s(&after, &state),
        "the real model's airspeed changes at the authored tick"
    );
}
