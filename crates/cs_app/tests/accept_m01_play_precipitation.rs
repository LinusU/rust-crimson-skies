//! Acceptance tests for task #1459 (`M01-PLAY-PRECIPITATION`): the one
//! production path that draws a mission environment's authored precipitation
//! for a windowed scene.
//!
//! Spec: `specs/F19-sky-atmosphere-weather-and-visibility.md` (non-negotiable
//! 1 and 2, AC03). Task test prefix: `accept_m01_play_precipitation_`.
//!
//! Everything drives `cs_app::render::precipitation`, the module the composed
//! mission installs (`crate::mission_session::compose`); the retail half reads
//! M01's own `weather.zrd` through `cs_app::environment::read_mission_weather`.

use std::time::Duration;

use bevy::asset::{AssetApp, Assets};
use bevy::mesh::{Mesh, Mesh3d};
use bevy::prelude::{Camera3d, StandardMaterial, Transform, Vec3};
use cs_app::environment::fixture::{WIND_SHIFT_TICK, clear_sky_environment, storm_environment};
use cs_app::environment::{EnvironmentSession, RunSeeds};
use cs_app::render::precipitation::{
    DESIGNED_LABEL, LATTICE_PERIOD_M, PRECIPITATION_FIELDS, PrecipitationField, PrecipitationLayer,
    PrecipitationPlugin, PrecipitationSpec, Undrawn, lattice_origin,
};
use cs_app::render::weather::WeatherOrigin;
use cs_content::environment::PrecipitationKind;
use cs_sim::time::TickRate;

const RATE_HZ: u32 = 64;

fn session(
    definition: cs_content::environment::EnvironmentDefinition,
    seeds: RunSeeds,
) -> EnvironmentSession {
    EnvironmentSession::new(definition, TickRate::new(RATE_HZ).expect("64 Hz"), seeds)
        .expect("the fixture starts a session")
}

fn raining_session(seeds: RunSeeds) -> EnvironmentSession {
    let mut session = session(storm_environment().expect("storm"), seeds);
    // Frames are clamped, so run ordinary 1/60 s frames until the shift.
    while session.clock().tick().0 <= WIND_SHIFT_TICK {
        session
            .advance_frame(Duration::from_secs_f64(1.0 / 60.0))
            .expect("the timeline advances");
    }
    session
}

#[test]
fn accept_m01_play_precipitation_a_clear_record_draws_nothing() {
    let clear = session(
        clear_sky_environment().expect("clear"),
        RunSeeds::from_root(1),
    );
    let spec = PrecipitationSpec::from_session(&clear);
    assert!(!spec.is_drawn());
    assert_eq!(spec.undrawn(), Some(&Undrawn::Clear));
    assert!(spec.mesh().is_none());
    assert_eq!(spec.particle_count(), 0);
    assert!(PrecipitationSpec::none().mesh().is_none());
}

#[test]
fn accept_m01_play_precipitation_authored_rain_is_drawn_with_labelled_origins() {
    let rain = raining_session(RunSeeds::from_root(3));
    let spec = PrecipitationSpec::from_session(&rain);
    assert_eq!(spec.kind(), Some(PrecipitationKind::Rain));
    assert!(spec.particle_count() > 0);
    // Only the kind is the record's; every other value is labelled designed.
    assert!(matches!(
        spec.origin(PrecipitationField::Kind),
        Some(WeatherOrigin::Bound(_))
    ));
    for field in PRECIPITATION_FIELDS
        .iter()
        .filter(|field| **field != PrecipitationField::Kind)
    {
        assert_eq!(
            spec.origin(*field),
            Some(&WeatherOrigin::Designed(DESIGNED_LABEL)),
            "{field:?} must be labelled designed"
        );
    }
    let mesh = spec.mesh().expect("authored rain has a mesh");
    // Two crossed quads (8 vertices) per drawn particle.
    assert_eq!(mesh.count_vertices(), spec.particle_count() * 8);
}

#[test]
fn accept_m01_play_precipitation_the_weather_seed_moves_only_the_cosmetic_field() {
    let a = raining_session(RunSeeds::new(
        9,
        cs_content::environment::CosmeticWeatherSeed::new(1),
    ));
    let b = raining_session(RunSeeds::new(
        9,
        cs_content::environment::CosmeticWeatherSeed::new(2),
    ));
    assert_ne!(
        PrecipitationSpec::from_session(&a).cell_offsets_m(),
        PrecipitationSpec::from_session(&b).cell_offsets_m(),
        "the cosmetic seed decides where the particles are"
    );
    assert_eq!(
        a.mission_ai_stream().clone().next_u64(),
        b.mission_ai_stream().clone().next_u64(),
        "the weather seed must not change the mission AI stream"
    );
}

#[test]
fn accept_m01_play_precipitation_the_lattice_follows_the_camera_and_falls() {
    let camera = Vec3::new(1_000.0, 500.0, -2_000.0);
    let origin = lattice_origin(camera, 0.0, 9.0);
    let half = (LATTICE_PERIOD_M / 2.0) as f32 + 0.01;
    assert!((origin - camera).abs().max_element() <= half);
    let later = lattice_origin(camera, 1.0, 9.0);
    assert!((later - camera).abs().max_element() <= half);
    assert_ne!(origin, later, "the particles fall with the render clock");
}

#[test]
fn accept_m01_play_precipitation_the_plugin_draws_rain_and_clear_draws_no_entity() {
    let mut app = cs_app::asset_stack::headless_app();
    app.init_asset::<StandardMaterial>();
    let rain = raining_session(RunSeeds::from_root(5));
    let plugin = PrecipitationPlugin::from_session(&rain);
    let expected = plugin.spec().particle_count();
    app.add_plugins(plugin);
    app.world_mut()
        .spawn((Camera3d::default(), Transform::from_xyz(40.0, 80.0, 10.0)));
    app.update();
    app.update();

    let world = app.world_mut();
    let mut query = world.query::<(&PrecipitationLayer, &Mesh3d)>();
    let drawn: Vec<_> = query.iter(world).map(|(_, mesh)| mesh.0.clone()).collect();
    assert_eq!(drawn.len(), 1, "one entity draws the whole field");
    let meshes = world.resource::<Assets<Mesh>>();
    assert_eq!(
        meshes.get(&drawn[0]).expect("mesh stored").count_vertices(),
        expected * 8
    );

    let mut clear_app = cs_app::asset_stack::headless_app();
    clear_app.init_asset::<StandardMaterial>();
    clear_app.add_plugins(PrecipitationPlugin::none());
    clear_app.update();
    let world = clear_app.world_mut();
    let mut query = world.query::<&PrecipitationLayer>();
    assert_eq!(query.iter(world).count(), 0);
}

/// The retail half: M01's own `weather.zrd` through the production reader.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_precipitation_retail_m01_resolves_to_rain() {
    use cs_app::environment::read_mission_weather;
    use cs_assets::install;

    let root = std::path::PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the original installation"),
    );
    let found = install::discover(&root).expect("the installation is discoverable");
    let weather = read_mission_weather(&found, "zbd/c1c/m01").expect("M01's weather binds");
    let session = weather
        .session(
            TickRate::new(RATE_HZ).expect("64 Hz"),
            RunSeeds::from_root(7),
        )
        .expect("M01's weather starts a session");
    let spec = PrecipitationSpec::from_session(&session);
    assert_eq!(spec.kind(), Some(PrecipitationKind::Rain));
    assert!(spec.mesh().is_some());
    assert!(matches!(
        spec.origin(PrecipitationField::Kind),
        Some(WeatherOrigin::Bound(_))
    ));
    assert_eq!(
        spec.origin(PrecipitationField::Colour),
        Some(&WeatherOrigin::Designed(DESIGNED_LABEL)),
        "the record's COLOR has an unmeasured scale and is not drawn"
    );
}
