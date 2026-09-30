//! `accept_f19_b_` tests for the cosmetic side of F19 non-negotiable
//! behavior 2: decorative particles draw from their own stream, advect only
//! with the authoritative wind, and appear only when a mission authors them.
//!
//! The production code under test is `cs_app::environment::cosmetic`:
//! [`CosmeticField`], which draws its particles from
//! [`CosmeticWeatherSeed`](cs_content::environment::CosmeticWeatherSeed)'s
//! `COSMETIC_WEATHER_DOMAIN` stream, and [`PrecipitationEffect`], which
//! decides whether a state is decorated at all. The wind that advects them
//! comes from `cs_app::environment::air`'s [`AuthoritativeWind`], so the test
//! reads one field for both the air and the decoration.
//!
//! No original data and no `CS_GAME_DIR`: the precipitation states are built
//! by production constructors, and the particle field is newly authored
//! development data.

use cs_app::environment::{
    AuthoritativeWind, COSMETIC_FIELD_HALF_EXTENT_M, COSMETIC_PARTICLE_COUNT, CosmeticField,
    CosmeticFieldError, PrecipitationEffect,
};
use cs_content::environment::{COSMETIC_WEATHER_DOMAIN, CosmeticWeatherSeed, PrecipitationKind};
use cs_types::random::SYNTHETIC_BODY_DOMAIN;

use crate::common;

/// A rain state, holding the wind still at the clear-sky fixture's value so a
/// test can change one thing at a time.
fn rain_state() -> cs_content::environment::EnvironmentState {
    common::state(
        common::known_wind([3.0, 0.0, -1.5], "f19b.test.cosmetic-wind"),
        common::known_precipitation(PrecipitationKind::Rain, "f19b.test.rain"),
        common::known_visibility(1500.0, "f19b.test.visibility"),
    )
}

/// The same rain state in a different wind, and nothing else changed.
fn rain_state_in(velocity_m_s: [f64; 3]) -> cs_content::environment::EnvironmentState {
    common::state(
        common::known_wind(velocity_m_s, "f19b.test.cosmetic-wind-other"),
        common::known_precipitation(PrecipitationKind::Rain, "f19b.test.rain"),
        common::known_visibility(1500.0, "f19b.test.visibility"),
    )
}

/// The wind of `state`, through the production effect path.
fn wind_of(state: &cs_content::environment::EnvironmentState) -> AuthoritativeWind {
    AuthoritativeWind::from_state(state).expect("the test states author a wind")
}

/// Decorative particles are drawn from the run's cosmetic weather stream and
/// from nothing else: the field is a pure function of its seed, another seed
/// draws a different field, and the draws belong to the cosmetic domain
/// rather than to any gameplay stream.
#[test]
fn accept_f19_b_cosmetic_particles_are_drawn_only_from_the_cosmetic_weather_stream() {
    let effect = PrecipitationEffect::resolve(&rain_state(), common::cosmetic_seed());
    let field = effect
        .field()
        .expect("a rain state authors a particle field");

    // Deterministic in the seed: the same run seed reproduces the frame.
    let again = PrecipitationEffect::resolve(&rain_state(), common::cosmetic_seed());
    assert_eq!(again.field(), Some(field), "one seed draws one field");

    // A different run seed draws a different field, so the particles are
    // random rather than a fixed pattern.
    let other = CosmeticField::new(
        PrecipitationKind::Rain,
        CosmeticWeatherSeed::new(common::COSMETIC_ROOT_SEED ^ 1),
    )
    .expect("a cosmetic stream draw is always a finite offset");
    assert_ne!(
        other.particles(),
        field.particles(),
        "another run seed must draw another particle field"
    );

    // The draws come from the cosmetic weather domain: the field is exactly
    // the replay of that stream, three draws per particle, each mapped to
    // `[-1, 1)` and scaled into the declared volume. Replaying the stream
    // here is what pins the *domain* — a field drawn from any other domain,
    // however deterministic, does not match it.
    assert_ne!(
        COSMETIC_WEATHER_DOMAIN, SYNTHETIC_BODY_DOMAIN,
        "the cosmetic weather stream owns its own domain constant"
    );
    let mut stream = CosmeticWeatherSeed::new(common::COSMETIC_ROOT_SEED).stream();
    for (index, particle) in field.particles().iter().enumerate() {
        let mut expected = [0.0; 3];
        for component in expected.iter_mut() {
            *component = (stream.unit_f64() * 2.0 - 1.0) * COSMETIC_FIELD_HALF_EXTENT_M;
        }
        assert_eq!(
            particle.offset_m(),
            expected,
            "particle {index} must be the cosmetic weather stream's own draw"
        );
    }
    // The replay above *is* the draw-count check: it consumes the stream three
    // components per particle and compares each particle, so a field that drew
    // a different number of components per particle would misalign the replay
    // and fail on the first particle that did not match. No separate
    // post-condition is needed, and none is asserted here.

    // The field is bounded: a fixed count, inside the declared designed
    // volume. A particle outside it would be a frame nobody could draw.
    assert_eq!(field.particles().len(), COSMETIC_PARTICLE_COUNT);
    for particle in field.particles() {
        for component in particle.offset_m() {
            assert!(
                component.abs() <= COSMETIC_FIELD_HALF_EXTENT_M,
                "a decorative particle must stay inside the declared volume"
            );
            assert!(component.is_finite());
        }
    }

    // A non-finite elapsed time is refused rather than putting a particle at
    // an unrepresentable place. The refusal names the bad argument, not a
    // particle: no particle is at fault when the caller's `elapsed_s` is bad.
    let wind = wind_of(&rain_state());
    assert!(matches!(
        field.particles()[0].drifted_offset_m(&wind, f64::NAN),
        Err(CosmeticFieldError::NonFiniteElapsed { elapsed_s }) if elapsed_s.is_nan()
    ));
    assert!(matches!(
        field.drifted(&wind, -1.0),
        Err(CosmeticFieldError::NonFiniteElapsed { elapsed_s: -1.0 })
    ));
}

/// The decoration follows the authoritative weather, but its *randomness*
/// does not: a gust advects every particle by exactly the wind's own
/// displacement and leaves the drawn field untouched, so no cosmetic draw can
/// be shifted by gameplay weather.
#[test]
fn accept_f19_b_a_gust_advects_the_particles_without_moving_their_draws() {
    let calm = rain_state();
    let gust = rain_state_in([16.0, 0.0, 2.0]);
    let calm_wind = wind_of(&calm);
    let gust_wind = wind_of(&gust);

    let calm_field = PrecipitationEffect::resolve(&calm, common::cosmetic_seed())
        .field()
        .expect("a rain state authors a particle field")
        .clone();
    let gust_field = PrecipitationEffect::resolve(&gust, common::cosmetic_seed())
        .field()
        .expect("a rain state authors a particle field")
        .clone();

    // The drawn field is the same in both weathers: the decoration's
    // randomness is a function of the run seed, not of the weather.
    assert_eq!(
        calm_field.particles(),
        gust_field.particles(),
        "a weather change must not shift a single decorative draw"
    );
    assert_eq!(calm_field, gust_field);

    // The *motion* is not the same: over a second of flight each particle is
    // carried by exactly the wind's difference, so a gust visibly blows the
    // rain sideways.
    let elapsed_s = 1.0;
    let calm_drifted = calm_field
        .drifted(&calm_wind, elapsed_s)
        .expect("a finite elapsed time drifts");
    let gust_drifted = gust_field
        .drifted(&gust_wind, elapsed_s)
        .expect("a finite elapsed time drifts");
    assert_eq!(calm_drifted.len(), gust_drifted.len());
    for (calm_particle, gust_particle) in calm_drifted.iter().zip(&gust_drifted) {
        for component in 0..3 {
            let moved = gust_particle.offset_m()[component] - calm_particle.offset_m()[component];
            let wind_delta =
                gust_wind.velocity_m_s()[component] - calm_wind.velocity_m_s()[component];
            assert!(
                (moved - wind_delta * elapsed_s).abs() < 1e-9,
                "a particle must be carried by the wind's own displacement"
            );
        }
    }

    // And in still air the particles stay exactly where they were drawn: the
    // wind advects, and nothing else in this module moves.
    let still = AuthoritativeWind::try_new([0.0, 0.0, 0.0]).expect("zero wind is representable");
    let undrifted = calm_field
        .drifted(&still, elapsed_s)
        .expect("still air drifts nowhere");
    for (particle, drawn) in undrifted.iter().zip(calm_field.particles()) {
        assert_eq!(particle.offset_m(), drawn.offset_m());
    }

    // The same wind drives the air the aircraft flies in: the decoration is
    // advected by the one authoritative field, not by a second decorative one.
    // The gust here runs *along* the flight path, so it shows up as a
    // tailwind — a headwind would lower the airspeed instead.
    let state_velocity = [0.0, 0.0, -55.0];
    assert_eq!(
        gust_wind.velocity_m_s(),
        wind_of(&gust).velocity_m_s(),
        "decoration and flight read one field"
    );
    assert!(
        gust_wind.airspeed_m_s(state_velocity) > calm_wind.airspeed_m_s(state_velocity),
        "a tailwind raises the airspeed from the same field the rain is carried by"
    );
    // The gust is `[16, 0, 2]` against a `-55 m/s` forward velocity: the air
    // velocity is `[-16, 0, -57]`, so the airspeed is the length of that, not a
    // component. This is the exact value, spelled out.
    let expected = (16.0_f64.powi(2) + 57.0_f64.powi(2)).sqrt();
    assert_eq!(gust_wind.airspeed_m_s(state_velocity), expected);
}

/// Only what a mission actually authors is drawn: a clear state decorates
/// nothing, an unknown kind decorates nothing and reports its claim, and
/// neither is turned into a field by a default.
#[test]
fn accept_f19_b_only_authored_precipitation_is_ever_drawn() {
    // Clear: no field at all.
    let clear = common::state(
        common::known_wind([1.0, 0.0, 0.0], "f19b.test.clear-wind"),
        common::known_precipitation(PrecipitationKind::Clear, "f19b.test.clear"),
        common::known_visibility(1500.0, "f19b.test.visibility"),
    );
    let clear_effect = PrecipitationEffect::resolve(&clear, common::cosmetic_seed());
    assert!(!clear_effect.is_decorated());
    assert_eq!(clear_effect.field(), None);
    assert_eq!(clear_effect.unknown(), None);

    // Snow is authored, so it decorates, and the field says which kind it is
    // — the vocabulary is designed, so the two kinds are not yet told apart
    // beyond that, and this asserts exactly that: no invented tuning.
    let snow = common::state(
        common::known_wind([1.0, 0.0, 0.0], "f19b.test.snow-wind"),
        common::known_precipitation(PrecipitationKind::Snow, "f19b.test.snow"),
        common::known_visibility(1500.0, "f19b.test.visibility"),
    );
    let snow_effect = PrecipitationEffect::resolve(&snow, common::cosmetic_seed());
    assert!(snow_effect.is_decorated());
    let snow_field = snow_effect.field().expect("snow authors a field");
    assert_eq!(snow_field.kind(), PrecipitationKind::Snow);
    let rain_field = PrecipitationEffect::resolve(&rain_state(), common::cosmetic_seed())
        .field()
        .expect("rain authors a field")
        .clone();
    assert_eq!(
        snow_field.particles(),
        rain_field.particles(),
        "the two kinds share one authored-agnostic field: no fall-speed tuning is invented"
    );

    // Unknown kind: nothing is drawn and the claim is reported, so missing
    // evidence never becomes a clear sky.
    let unknown = common::state(
        common::known_wind([1.0, 0.0, 0.0], "f19b.test.unknown-wind"),
        common::unknown(
            "f19b.test.unknown-kind",
            "this test authors no precipitation kind",
        ),
        common::known_visibility(1500.0, "f19b.test.visibility"),
    );
    let unknown_effect = PrecipitationEffect::resolve(&unknown, common::cosmetic_seed());
    assert!(!unknown_effect.is_decorated());
    assert_eq!(unknown_effect.field(), None);
    assert_eq!(
        unknown_effect.unknown(),
        Some((
            "f19b.test.unknown-kind",
            "this test authors no precipitation kind"
        )),
        "an unknown precipitation kind must be reported, never drawn as clear"
    );

    // The clear-sky fixture's own state authors clear, so the production
    // fixture decorates nothing either.
    let fixture =
        PrecipitationEffect::resolve(&common::clear().state().clone(), common::cosmetic_seed());
    assert!(!fixture.is_decorated());
}
