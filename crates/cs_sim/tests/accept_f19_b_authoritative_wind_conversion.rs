//! `accept_f19_b_` acceptance tests for the one air-relative conversion the
//! simulation owns (task #434 `F19-WIND-CONVERSION-OWNER`).
//!
//! F19 non-negotiable behavior 2 asks that wind be *the same authoritative
//! field* for flight and projectiles; acceptance case AC02 asks that a wind
//! change "affect aircraft airspeed and projectile-relative velocity
//! consistently". F19-B implemented that conversion above the simulation, in
//! `cs_app::environment::air`, where `cs_sim` could not reach it, and recorded
//! the seam. This task moved the conversion into `cs_sim::environment`, beside
//! the code that applies it, and made both sides call it.
//!
//! The production code under test here is real, not a test-side formula:
//!
//! * [`cs_sim::environment::air_relative_velocity_m_s`] and its exact inverse
//!   [`cs_sim::environment::world_velocity_from_air_m_s`] — the single
//!   implementation this file pins;
//! * [`FlightEnvironment::air_relative_velocity_m_s`], the accessor every
//!   consumer inside `cs_sim` reaches it through;
//! * the real [`FlightModel`] and the real [`ExceptionalControlLaw`], whose
//!   reported airspeed must equal
//!   [`cs_sim::environment::airspeed_m_s`] of the same world velocity and wind,
//!   and whose airspeed must move the way the convention says when the wind
//!   changes — so no consumer can hold a *different* conversion (a flipped
//!   sign, a different field, a per-axis shortcut) and still pass.
//!
//! What these tests cannot decide is *where* the conversion lives: a consumer
//! that inlined a correct private copy of the subtraction would produce the
//! same numbers. That the workspace has one implementation is a structural
//! property — the models delegate through
//! [`FlightEnvironment::air_relative_velocity_m_s`] and
//! `cs_app::environment::air` re-exports the functions — so it is checked by
//! reading those call sites and by review, not by an assertion here.
//!
//! The projectile record itself (`cs_app::environment::air::ProjectileMotion`)
//! is above this crate and is pinned by the F19-B acceptance tests there; what
//! is pinned here is the conversion that record and the weapons path both call.
//!
//! No original data and no `CS_GAME_DIR`: the winds are designed fixtures and
//! the tuning is the declared synthetic one. Nothing here is an original
//! measurement; a projectile's own dynamics remain unmeasured and are recorded
//! as unknown in
//! `docs/findings/2026-09-30-f19-b-sky-fog-light-and-weather-effects.md`.

use cs_sim::environment::{air_relative_velocity_m_s, airspeed_m_s, world_velocity_from_air_m_s};
use cs_sim::flight::{
    DamageState, EngineState, ExceptionalControlLaw, FlightEnvironment, FlightInput, FlightModel,
    FlightState, LoadoutMass, SYNTHETIC_TICK_DT_S, synthetic_exceptional_profile,
    synthetic_exceptional_tuning, synthetic_fixed_wing, synthetic_rotor_drive,
};
use cs_types::Tick;
use cs_types::space::Quaternion;

/// A cruise world velocity in m/s, along the body's forward axis (`-Z` per the
/// coordinate convention), far above the airspeed at which any control
/// authority is granted.
const CRUISE_WORLD_MPS: [f64; 3] = [0.0, 0.0, -55.0];

/// A measured still-air field: a real wind, not an absence.
const CALM: [f64; 3] = [0.0, 0.0, 0.0];
/// Air moving towards +Z, against an aircraft travelling towards -Z.
const HEADWIND: [f64; 3] = [0.0, 0.0, 8.0];
/// The same strength the other way.
const TAILWIND: [f64; 3] = [0.0, 0.0, -8.0];
/// A wind perpendicular to the flight. This is the case a test-side shortcut
/// gets wrong: an along-axis wind cannot distinguish a law that converts from
/// one that uses the ground track for the axes it does not touch.
const CROSSWIND: [f64; 3] = [0.0, 6.0, 0.0];
/// The timeline gust shape F19-B's storm fixture installs.
const GUST: [f64; 3] = [16.0, 0.0, 2.0];

/// Every wind the conversion is checked against.
const WINDS: [[f64; 3]; 5] = [CALM, HEADWIND, TAILWIND, CROSSWIND, GUST];

/// The cruise state: moving at [`CRUISE_WORLD_MPS`] with a spooled engine,
/// level and unrotated.
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

/// The sea-level environment sea level declares, carrying `wind_velocity_mps`.
fn environment(wind_velocity_mps: [f64; 3]) -> FlightEnvironment {
    FlightEnvironment {
        wind_velocity_mps,
        ..FlightEnvironment::SEA_LEVEL
    }
}

/// The airspeed the real fixed-wing model reports for the cruise state in
/// `wind`.
fn reported_airspeed_m_s(wind: [f64; 3]) -> f64 {
    FlightModel::new(synthetic_fixed_wing())
        .compute(
            &environment(wind),
            &LoadoutMass::EMPTY,
            &DamageState::PRISTINE,
            &cruise_state(),
            &cruise_input(),
            1.0 / 64.0,
        )
        .expect("a finite state, wind and tuning always compute")
        .instrument_state
        .airspeed_mps
}

/// The airspeed the real exceptional law reports for the same state and wind.
///
/// A stopped rotor is enough: the air-relative velocity and the reported
/// airspeed are settled before any rotor force, so the assertion is about the
/// conversion both consumers share and not about rotor tuning.
fn exceptional_airspeed_m_s(wind: [f64; 3]) -> f64 {
    let law = ExceptionalControlLaw::new(
        synthetic_exceptional_tuning(),
        synthetic_exceptional_profile(),
    )
    .expect("the declared synthetic exceptional law is valid");
    let mut rotor = synthetic_rotor_drive();
    law.compute(
        &environment(wind),
        &LoadoutMass::EMPTY,
        &DamageState::PRISTINE,
        &cruise_state(),
        &cruise_input(),
        SYNTHETIC_TICK_DT_S,
        Tick(1),
        &mut rotor,
    )
    .expect("an exceptional tick computes for a finite state and wind")
    .output
    .instrument_state
    .airspeed_mps
}

/// The airspeed the environment's own conversion produces for a held world
/// velocity, through the accessor a `cs_sim` consumer is meant to call.
fn converted_airspeed_m_s(wind: [f64; 3]) -> f64 {
    let air = environment(wind).air_relative_velocity_m_s(CRUISE_WORLD_MPS);
    assert_eq!(
        air,
        air_relative_velocity_m_s(CRUISE_WORLD_MPS, wind),
        "the environment's accessor delegates to the one conversion"
    );
    assert_eq!(airspeed_m_s(CRUISE_WORLD_MPS, wind), magnitude(air));
    airspeed_m_s(CRUISE_WORLD_MPS, wind)
}

/// AC02 through the simulation's own path: the airspeed both real models
/// report is exactly the airspeed the one shared conversion computes, for
/// every wind — including a headwind, a tailwind and a crosswind, where a
/// wrong sign or a per-axis shortcut would show.
#[test]
fn accept_f19_b_both_flight_models_report_the_shared_conversion_s_airspeed() {
    for wind in WINDS {
        let converted = converted_airspeed_m_s(wind);
        assert_eq!(
            reported_airspeed_m_s(wind),
            converted,
            "the fixed wing's airspeed must be the shared conversion's, not a copy of it"
        );
        assert_eq!(
            exceptional_airspeed_m_s(wind),
            converted,
            "the exceptional law must reach the same airspeed through the same conversion"
        );
        assert!(
            converted.is_finite(),
            "a finite wind and state never produce a non-finite airspeed: {wind:?}"
        );
    }

    // The sign of the conversion, which a headwind and a tailwind fix from both
    // directions: air moving against the aircraft raises its airspeed, air
    // moving with it lowers it.
    let calm = converted_airspeed_m_s(CALM);
    assert!(
        converted_airspeed_m_s(HEADWIND) > calm,
        "a headwind raises the airspeed of an aircraft flying at 55 m/s"
    );
    assert!(
        converted_airspeed_m_s(TAILWIND) < calm,
        "a tailwind lowers it"
    );
    assert!(
        converted_airspeed_m_s(CROSSWIND) > calm,
        "a crosswind adds to the speed without changing the axis of flight"
    );
}

/// The regression this task exists for: when the wind changes, an aircraft's
/// air-relative velocity and a projectile's world velocity move by exactly the
/// wind's own difference — the same amount, by the same conversion — so the
/// two kinds of body cannot be flying different airs (AC02).
#[test]
fn accept_f19_b_a_wind_change_moves_aircraft_and_projectile_by_the_wind_difference() {
    // Aircraft: a held world velocity, so its air-relative velocity is exactly
    // the world velocity minus the wind.
    let calm_air = air_relative_velocity_m_s(CRUISE_WORLD_MPS, CALM);
    let gust_air = air_relative_velocity_m_s(CRUISE_WORLD_MPS, GUST);
    assert_ne!(calm_air, gust_air, "the two fields really are two fields");
    assert_eq!(
        difference(gust_air, calm_air),
        [-GUST[0], -GUST[1], -GUST[2]],
        "an aircraft held at one world velocity is pushed *against* the new air, \
         by the wind's own difference"
    );

    // The real model sees the same change, because it reads the same field.
    assert_ne!(
        reported_airspeed_m_s(CALM),
        reported_airspeed_m_s(GUST),
        "the real model's airspeed changes when the wind changes"
    );
    assert_eq!(
        reported_airspeed_m_s(GUST),
        converted_airspeed_m_s(GUST),
        "and it changes to the shared conversion's number"
    );

    // Projectile: a muzzle velocity authored in air-relative terms keeps its
    // airspeed and is carried by exactly the same difference in world space.
    let shot = [0.0, 0.0, -120.0];
    let calm_carried = world_velocity_from_air_m_s(shot, CALM);
    let gust_carried = world_velocity_from_air_m_s(shot, GUST);
    assert_eq!(
        difference(gust_carried, calm_carried),
        GUST,
        "the projectile's world velocity shifts by the wind's own difference"
    );
    assert_eq!(
        airspeed_m_s(gust_carried, GUST),
        airspeed_m_s(calm_carried, CALM),
        "a projectile's airspeed is its own velocity, not the wind's"
    );

    // And the two agree with each other: the world velocity of a projectile
    // authored from the aircraft's own air-relative velocity is the
    // aircraft's world velocity, because both went through one conversion.
    assert_eq!(
        world_velocity_from_air_m_s(gust_air, GUST),
        CRUISE_WORLD_MPS,
        "a shot authored with the aircraft's air-relative velocity flies at the \
         aircraft's world velocity"
    );
}

/// A velocity handed through the conversion twice is unchanged: the inverse is
/// exact, so no consumer can round-trip a velocity through the field and lose
/// or gain a component.
#[test]
fn accept_f19_b_the_shared_conversion_round_trips_a_velocity_exactly() {
    for wind in WINDS {
        let air = air_relative_velocity_m_s(CRUISE_WORLD_MPS, wind);
        assert_eq!(world_velocity_from_air_m_s(air, wind), CRUISE_WORLD_MPS);
        assert_eq!(
            air_relative_velocity_m_s(world_velocity_from_air_m_s(air, wind), wind),
            air
        );

        // A body drifting with the air mass has no airspeed: the wind is what a
        // velocity is measured against, not what is added to it.
        assert_eq!(airspeed_m_s(wind, wind), 0.0);
    }
}

/// Euclidean length of a three-component vector, spelled out so the
/// assertions read as the identities they check.
fn magnitude(value: [f64; 3]) -> f64 {
    (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt()
}

/// Component-wise difference.
fn difference(lhs: [f64; 3], rhs: [f64; 3]) -> [f64; 3] {
    [lhs[0] - rhs[0], lhs[1] - rhs[1], lhs[2] - rhs[2]]
}
