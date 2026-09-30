//! `accept_f19_c_` tests for the integrated environment session: the weather
//! producer, its wind and sight-range consumers, and AC03 — weather seeds do
//! not change mission AI RNG sequences.
//!
//! The production code under test is `cs_app::environment::EnvironmentSession`
//! and `RunSeeds`, driving the real `EnvironmentClock`, `FlightModel`-facing
//! `FlightEnvironment` and `cs_types::random::SplitMix64`. No original data.

use cs_app::environment::{
    EnvironmentSession, RunSeeds, VISIBILITY_TICK, VisibilityUnavailable, WIND_SHIFT_TICK,
    WindUnavailable,
};
use cs_content::environment::{
    CosmeticWeatherSeed, EnvironmentDefinition, EnvironmentId, EnvironmentProfile,
    EnvironmentState, EnvironmentTimeline, FogDefinition, LightingDefinition,
    PrecipitationDefinition, PrecipitationKind,
};
use cs_sim::flight::FlightEnvironment;
use cs_types::content::{Origin, Provenance};
use cs_types::random::SplitMix64;

use crate::common;

const MISSION_SEED: u64 = 0x00C5_0000_0000_2024;

fn storm_session(seeds: RunSeeds) -> EnvironmentSession {
    EnvironmentSession::new(common::storm(), common::tick_rate(), seeds)
        .expect("the storm fixture's schedule is ordered")
}

fn run_ticks(session: &mut EnvironmentSession, ticks: u64) {
    for _ in 0..ticks {
        session.advance_frame(common::FRAME).expect("no overflow");
    }
}

fn draws(mut stream: SplitMix64, count: usize) -> Vec<u64> {
    (0..count).map(|_| stream.next_u64()).collect()
}

/// AC03: no weather seed, weather tick, effect resolution or particle draw
/// moves the mission AI sequence.
#[test]
fn accept_f19_c_weather_seeds_do_not_change_mission_ai_rng_sequences() {
    let reference = draws(
        SplitMix64::for_domain(MISSION_SEED, cs_sim::ai::navigation::AI_NAVIGATION_DOMAIN),
        32,
    );

    let weather_roots = [
        0,
        1,
        MISSION_SEED,
        common::COSMETIC_ROOT_SEED,
        // Chosen so the cosmetic stream collides with the AI stream.
        MISSION_SEED
            ^ cs_sim::ai::navigation::AI_NAVIGATION_DOMAIN
            ^ cs_content::environment::COSMETIC_WEATHER_DOMAIN,
        u64::MAX,
    ];
    for root in weather_roots {
        let seeds = RunSeeds::new(MISSION_SEED, CosmeticWeatherSeed::new(root));
        let mut session = storm_session(seeds);
        let mut ai = session.mission_ai_stream();
        let mut observed = Vec::new();
        // Interleave AI draws with weather ticks, effect resolution and
        // particle advection through the real producer.
        for _ in 0..32 {
            session.advance_frame(common::FRAME).expect("no overflow");
            let effects = session.effects();
            if let Some(field) = effects.precipitation().field() {
                assert!(!field.particles().is_empty());
            }
            observed.push(ai.next_u64());
        }
        assert_eq!(
            observed, reference,
            "weather seed {root:#x} moved the AI stream"
        );
    }

    // The weather seed really is live on the cosmetic side, so the
    // invariance above is not vacuous.
    let rain_at = |root: u64| {
        let mut session =
            storm_session(RunSeeds::new(MISSION_SEED, CosmeticWeatherSeed::new(root)));
        run_ticks(&mut session, WIND_SHIFT_TICK);
        session.effects().precipitation().field().cloned()
    };
    let (a, b) = (rain_at(1), rain_at(2));
    assert!(a.is_some(), "the gust authors rain");
    assert_ne!(a, b, "different weather seeds draw different particles");

    // The mission seed is what moves the AI stream.
    assert_ne!(
        RunSeeds::from_root(MISSION_SEED ^ 1).mission_ai_stream(),
        RunSeeds::from_root(MISSION_SEED).mission_ai_stream()
    );
}

/// The wind the flight model is handed follows the timeline, and the unknown
/// sight range is refused until the timeline authors it.
#[test]
fn accept_f19_c_session_installs_timeline_wind_and_sight_range_for_consumers() {
    let mut session = storm_session(RunSeeds::from_root(MISSION_SEED));
    let base = FlightEnvironment::SEA_LEVEL;

    assert_eq!(
        session.flight_environment(&base).unwrap().wind_velocity_mps,
        [8.0, 0.0, 0.0]
    );
    assert!(matches!(
        session.sight_range_m(),
        Err(VisibilityUnavailable::Unknown { .. })
    ));

    // A paused session never moves the weather.
    session.set_paused(true);
    run_ticks(&mut session, WIND_SHIFT_TICK + 1);
    assert_eq!(session.wind().unwrap().velocity_m_s(), [8.0, 0.0, 0.0]);
    session.set_paused(false);

    run_ticks(&mut session, WIND_SHIFT_TICK);
    let gusted = session.flight_environment(&base).unwrap();
    assert_eq!(gusted.wind_velocity_mps, [16.0, 0.0, 2.0]);
    assert_eq!(gusted.gravity_mps2, base.gravity_mps2);
    assert_eq!(gusted.air_density_kg_m3, base.air_density_kg_m3);

    run_ticks(&mut session, VISIBILITY_TICK - WIND_SHIFT_TICK);
    assert_eq!(session.sight_range_m(), Ok(400.0));
}

/// Restarting is the retry path: the same events fire at the same ticks, and
/// the seeds are unchanged.
#[test]
fn accept_f19_c_restart_replays_the_same_weather_from_tick_zero() {
    let mut session = storm_session(RunSeeds::from_root(MISSION_SEED));
    run_ticks(&mut session, VISIBILITY_TICK);
    let first = (session.clock().tick(), session.sight_range_m());
    session.restart().expect("rebuilding the same schedule");
    assert_eq!(session.clock().tick().0, 0);
    assert_eq!(session.wind().unwrap().velocity_m_s(), [8.0, 0.0, 0.0]);
    run_ticks(&mut session, VISIBILITY_TICK);
    assert_eq!((session.clock().tick(), session.sight_range_m()), first);
    assert_eq!(session.seeds(), &RunSeeds::from_root(MISSION_SEED));
}

/// An unknown wind reaches the caller as an error; the session never
/// substitutes still air, and it does not take the sight range from fog.
#[test]
fn accept_f19_c_unknown_wind_propagates_and_is_never_still_air() {
    let definition = EnvironmentDefinition::try_new(
        EnvironmentId::new("fixture.unknown-wind").unwrap(),
        Origin::SyntheticFixture,
        EnvironmentProfile::SyntheticDeveloper,
        common::authored_sky(),
        common::default_orientation(),
        LightingDefinition::try_new(
            common::unknown("f19c.test.sun", "no sun"),
            common::unknown("f19c.test.ambient", "no ambient"),
        )
        .unwrap(),
        FogDefinition::designed_default(1e-3, [0.7, 0.8, 0.95], "f19c.test.fog").unwrap(),
        Vec::new(),
        EnvironmentState::new(
            common::unknown("f19c.test.wind", "nobody measured the wind"),
            PrecipitationDefinition::new(common::known(
                PrecipitationKind::Clear,
                "f19c.test.precipitation",
            )),
            common::unknown("f19c.test.visibility", "nobody measured the sight range"),
        ),
        EnvironmentTimeline::default(),
        Provenance::designed(common::claim("f19c.test.record")),
    )
    .unwrap();
    let session = EnvironmentSession::new(
        definition,
        common::tick_rate(),
        RunSeeds::from_root(MISSION_SEED),
    )
    .unwrap();

    match session.flight_environment(&FlightEnvironment::SEA_LEVEL) {
        Err(WindUnavailable::Unknown { claim_id, .. }) => {
            assert_eq!(claim_id.as_str(), "f19c.test.wind");
        }
        other => panic!("an unknown wind must be refused, got {other:?}"),
    }
    match session.sight_range_m() {
        Err(VisibilityUnavailable::Unknown { claim_id, .. }) => {
            assert_eq!(claim_id.as_str(), "f19c.test.visibility");
        }
        other => panic!("an unknown sight range must be refused, got {other:?}"),
    }
}
