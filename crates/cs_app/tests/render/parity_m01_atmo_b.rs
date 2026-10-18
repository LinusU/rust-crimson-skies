//! Acceptance for PARITY-M01-ATMO-B (#1649): the drawn horizon's fog colour,
//! fog distances and ambient colour come from the mission's own
//! `weather.zrd` zone, not from the designed palette. Task test prefix:
//! `accept_parity_m01_atmo_b_`.
//!
//! Facts: `docs/findings/2026-10-18-parity-m01-atmo-a-weather-units-and-sky-
//! consumers.md` §2 and §7. The synthetic half builds a record with M01's
//! numbers by hand; the retail half reads M01 (`zbd/c3/m01`) through the
//! production reader and needs `$CS_GAME_DIR`.

use cs_app::environment::RetailWeather;
use cs_app::render::horizon::{
    DESIGNED_LABEL, HorizonField, HorizonOrigin, HorizonPlugin, HorizonSpec,
};
use cs_assets::install;
use cs_content::weather::{
    CloudCoverRecord, ViewingRangeTier, WeatherDocument, WeatherScalar as S, WindRecord,
    ZoneRecord, bind_weather,
};
use cs_types::asset_id::SourceSpan;

const M01: &str = "zbd/c3/m01";

fn zone(name: &str, colour: [f32; 3], ranges: [f32; 2], white_ambient: bool) -> ZoneRecord {
    let ambient = if white_ambient {
        [S::Float(1.0); 3]
    } else {
        [S::Float(0.5); 3]
    };
    ZoneRecord {
        name: name.to_owned(),
        fog_color: colour.map(S::Float),
        fog_ranges: ranges.map(S::Float),
        fog_altitude: Some([S::Float(9000.0), S::Float(10000.0)]),
        clip_ranges: [S::Float(5.0), S::Float(4800.0)],
        sunlight_active: S::Int(1),
        sunlight_orientation: [S::Float(-25.0), S::Float(135.0), S::Float(0.0)],
        sunlight_diffuse: S::Float(1.5),
        sunlight_ambient: S::Float(0.3),
        sunlight_color_diffuse: [S::Float(1.0); 3],
        sunlight_color_ambient: ambient,
        sunlight_static: S::Int(1),
        sunlight_bicolored: S::Int(0),
    }
}

fn m01_like_weather(fog_scale: f32) -> RetailWeather {
    let document = WeatherDocument {
        viewing_range: vec![ViewingRangeTier {
            name: "HIGH".to_owned(),
            clip_scale: S::Float(1.0),
            fog_scale: S::Float(fog_scale),
        }],
        wind: WindRecord {
            static_velocity: [S::Int(0), S::Int(2), S::Int(0)],
            random_max_speed: S::Int(10),
            random_accel: S::Int(5),
            random_ang_vel: S::Int(5),
        },
        cloud_cover: CloudCoverRecord {
            top: S::Int(11000),
            bottom: S::Int(10000),
            thickness: S::Int(50),
            top_color: None,
            bottom_color: None,
        },
        zones: vec![
            zone("ZONE1", [0.79; 3], [1000.0, 4500.0], true),
            zone("ZONE2", [0.06275, 0.09412, 0.1882], [1000.0, 4500.0], false),
        ],
        shadow_angles: [S::Int(0); 3],
        precipitation: None,
    };
    let span = SourceSpan::new(
        install::sha256(b"install"),
        "zbd/c3/m01/zrdr.zbd",
        Some("weather.zrd"),
        0,
        10,
        None,
    )
    .expect("the synthetic span is recordable");
    let binding = bind_weather(M01, &document, &span).expect("the document binds");
    RetailWeather {
        mission_key: M01.to_owned(),
        span,
        member_bytes: 10,
        document,
        binding,
    }
}

#[test]
fn accept_parity_m01_atmo_b_zone1_fog_replaces_the_designed_values() {
    let weather = m01_like_weather(1.0);
    let designed = HorizonSpec::designed();
    let mut spec = designed.clone();
    spec.bind_weather(&weather).expect("ZONE1 binds");

    assert_eq!(
        spec.fog_colour(),
        [0.79; 3],
        "ZONE1's grey, not ZONE2's blue"
    );
    assert!((spec.fog_start_m() - 1000.0).abs() < f32::EPSILON);
    assert!((spec.fog_end_m() - 4500.0).abs() < f32::EPSILON);
    assert_ne!(spec.fog_end_m(), designed.fog_end_m());
    assert_eq!(spec.ambient_colour(), [1.0; 3]);
    // The dome's horizon band and its under-horizon colour are the fog.
    assert_eq!(spec.sky_horizon(), spec.fog_colour());
    // The fog ends inside the dome, so distant terrain fades into the haze
    // before the dome can occlude it.
    assert!(spec.fog_end_m() < spec.dome_radius_m());

    for field in [
        HorizonField::FogColour,
        HorizonField::FogStart,
        HorizonField::FogEnd,
        HorizonField::AmbientColour,
    ] {
        assert!(
            matches!(spec.origin(field), Ok(HorizonOrigin::Bound(_))),
            "{field:?} is bound"
        );
    }
    // What nothing measured stays labelled designed.
    for field in [
        HorizonField::SkyZenith,
        HorizonField::SeaColour,
        HorizonField::AmbientBrightness,
    ] {
        assert_eq!(
            spec.origin(field).expect("origin"),
            &HorizonOrigin::Designed(DESIGNED_LABEL)
        );
    }
    assert!(!spec.is_entirely_designed());
}

#[test]
fn accept_parity_m01_atmo_b_the_viewing_tier_scales_the_fog_ranges_only() {
    let mut spec = HorizonSpec::designed();
    spec.bind_weather(&m01_like_weather(0.85)).expect("binds");
    assert!((spec.fog_start_m() - 850.0).abs() < 1e-3);
    assert!((spec.fog_end_m() - 3825.0).abs() < 1e-3);
}

#[test]
fn accept_parity_m01_atmo_b_a_record_without_the_flight_zone_is_refused() {
    let mut weather = m01_like_weather(1.0);
    weather.document.zones.retain(|zone| zone.name != "ZONE1");
    let mut spec = HorizonSpec::designed();
    assert!(spec.bind_weather(&weather).is_err());
    assert_eq!(spec, HorizonSpec::designed(), "a refusal changes nothing");
}

#[test]
fn accept_parity_m01_atmo_b_the_plugin_hands_the_bound_fog_to_the_scene() {
    use bevy::prelude::{Camera3d, DistanceFog, FogFalloff};
    let plugin = HorizonPlugin::designed()
        .with_weather(&m01_like_weather(1.0))
        .expect("binds");
    assert!((plugin.spec().fog_end_m() - 4500.0).abs() < f32::EPSILON);

    let mut app = bevy::app::App::new();
    app.add_plugins(bevy::MinimalPlugins);
    app.init_resource::<bevy::prelude::Assets<bevy::mesh::Mesh>>();
    app.init_resource::<bevy::prelude::Assets<bevy::prelude::StandardMaterial>>();
    app.add_plugins(plugin);
    let camera = app.world_mut().spawn(Camera3d::default()).id();
    app.update();
    let fog = app
        .world()
        .entity(camera)
        .get::<DistanceFog>()
        .expect("the camera is fogged");
    assert!(matches!(
        fog.falloff,
        FogFalloff::Linear { start, end }
            if (start - 1000.0).abs() < f32::EPSILON && (end - 4500.0).abs() < f32::EPSILON
    ));
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_atmo_b_retail_m01_weather_drives_the_horizon() {
    let root = std::path::PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the original installation"),
    );
    let found = install::discover(&root).expect("the installation is discoverable");
    let weather = cs_app::environment::read_mission_weather(&found, M01)
        .unwrap_or_else(|error| panic!("{M01}: {error}"));
    let mut spec = HorizonSpec::designed();
    spec.bind_weather(&weather).expect("M01's ZONE1 binds");
    for (got, want) in spec.fog_colour().iter().zip([0.79_f32; 3]) {
        assert!((got - want).abs() < 1e-6);
    }
    assert!((spec.fog_start_m() - 1000.0).abs() < 1e-3);
    assert!((spec.fog_end_m() - 4500.0).abs() < 1e-3);
}
