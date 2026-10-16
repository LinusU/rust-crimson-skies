//! Acceptance tests for task #1434 (`M01-PLAY-SKY`): the one production path
//! that draws a sky, a sea and fog for a windowed scene, and the origin of
//! every value it draws them from.
//!
//! Spec: `specs/F19-sky-atmosphere-weather-and-visibility.md` (non-negotiable
//! 3 and 5). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`. Task test
//! prefix: `accept_m01_play_sky_`.
//!
//! Everything here drives production code: `cs_app::render::horizon` is the
//! module the original-assets playtest and the composed mission both install
//! (`crate::playtest::windowed_app`, `crate::mission_session::compose`), and
//! the retail half reads M01's own `weather.zrd` through
//! `cs_app::environment::read_mission_weather`, the reader task #636 bound.
//!
//! What is pinned:
//!
//! * every drawn value of the designed horizon is **labelled**, so a
//!   screenshot or a review can always say these colours are this build's;
//! * a value the environment record *does* carry reaches the screen verbatim
//!   with its provenance instead of being replaced by the design;
//! * the dome shows the fog colour below the horizon, the seam rule that
//!   keeps the sea's far edge from ending against a different colour;
//! * the fog saturates inside the dome and the sea reaches past the fog, so
//!   neither the dome's edge nor the sea's edge can be seen;
//! * the plugin spawns the dome and the sea, fogs the camera, and keeps the
//!   dome centred on the camera with an identity rotation.

use bevy::asset::AssetApp;
use bevy::mesh::{Mesh, VertexAttributeValues};
use bevy::prelude::{
    Camera3d, Color, DistanceFog, FogFalloff, Mesh3d, MeshMaterial3d, StandardMaterial, Transform,
    Vec3,
};
use cs_app::environment::fixture::{clear_sky_environment, storm_environment};
use cs_app::environment::{RunSeeds, read_mission_weather};
use cs_app::render::horizon::{
    DESIGNED_LABEL, HORIZON_FIELDS, HorizonDome, HorizonField, HorizonOrigin, HorizonPlugin,
    HorizonSea, HorizonSpec,
};
use cs_types::content::Resolved;
use cs_types::evidence::ClaimStatus;

const M01: &str = "zbd/c1c/m01";

/// The play camera's far plane, `crates/cs_app/src/playtest/visuals.rs`
/// (`FAR_PLANE_M`): a dome beyond it is clipped away and the scene is left
/// with no sky at all.
const FAR_PLANE_M: f32 = 40_000.0;

/// A record's colour, converted the way the spec converts one for display.
fn srgb_of(linear: [f64; 3]) -> [f32; 3] {
    let srgb = Color::linear_rgb(linear[0] as f32, linear[1] as f32, linear[2] as f32).to_srgba();
    [srgb.red, srgb.green, srgb.blue]
}

#[test]
fn accept_m01_play_sky_the_designed_horizon_labels_every_field_it_draws() {
    let spec = HorizonSpec::designed();
    assert!(spec.is_entirely_designed());
    for field in HORIZON_FIELDS {
        let origin = spec
            .origin(*field)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            origin,
            &HorizonOrigin::Designed(DESIGNED_LABEL),
            "{field:?} must be a labelled designed value, not a silent default"
        );
    }
    // The label names the task, so a screenshot or a review note can trace
    // the colours back to the build that painted them.
    assert!(DESIGNED_LABEL.contains("M01-PLAY-SKY"));
}

#[test]
fn accept_m01_play_sky_a_bound_environment_value_reaches_the_screen_verbatim() {
    // The clear-sky fixture authors a fog colour and an ambient colour: the
    // spec must take both with their provenance instead of substituting its
    // own design. It also names a sky texture, which this stage does *not*
    // draw, so that field must stay designed (checked below).
    let environment = clear_sky_environment().expect("the fixture is representable");
    let spec = HorizonSpec::from_environment(&environment);

    let Resolved::Known(fog) = environment.fog().color_linear() else {
        panic!("the clear-sky fixture authors a fog colour");
    };
    assert_eq!(spec.fog_colour(), srgb_of(fog.value));
    assert_eq!(
        spec.origin(HorizonField::FogColour)
            .unwrap_or_else(|error| panic!("{error}")),
        &HorizonOrigin::Bound(fog.provenance.clone())
    );

    let Resolved::Known(ambient) = environment.lighting().ambient_linear() else {
        panic!("the clear-sky fixture authors an ambient colour");
    };
    assert_eq!(spec.ambient_colour(), srgb_of(ambient.value));
    assert_eq!(
        spec.origin(HorizonField::AmbientColour)
            .unwrap_or_else(|error| panic!("{error}")),
        &HorizonOrigin::Bound(ambient.provenance.clone())
    );

    // The record names a sky texture, but this stage draws no authored art:
    // the zenith colour stays the labelled design, so the drawn gradient can
    // never be read as the record's own sky. A `Bound` zenith would claim art
    // reached the screen when none did.
    assert!(
        matches!(environment.sky().texture(), Resolved::Known(_)),
        "the clear-sky fixture authors a sky texture"
    );
    assert_eq!(
        spec.origin(HorizonField::SkyZenith)
            .unwrap_or_else(|error| panic!("{error}")),
        &HorizonOrigin::Designed(DESIGNED_LABEL),
        "the drawn zenith colour is this build's, not the record's sky art"
    );

    // The dome's horizon band follows the bound fog colour: one value, so
    // the sea cannot fade into a colour the dome does not show.
    assert_eq!(spec.sky_horizon(), spec.fog_colour());

    // The fields no record carries stay designed and labelled — the sea's
    // level and extent, the fog distances and the dome radius, none of which
    // the original has been measured for.
    for field in [
        HorizonField::SeaLevel,
        HorizonField::SeaExtent,
        HorizonField::FogStart,
        HorizonField::FogEnd,
        HorizonField::DomeRadius,
    ] {
        assert_eq!(
            spec.origin(field).unwrap_or_else(|error| panic!("{error}")),
            &HorizonOrigin::Designed(DESIGNED_LABEL),
            "{field:?} carries no measured value and must stay labelled"
        );
    }

    // A value bound *as designed* is still a designed value on screen: the
    // fixture's provenance says so, and the spec repeats it rather than
    // promoting it to a measurement.
    assert!(spec.is_entirely_designed());
    assert!(matches!(
        spec.origin(HorizonField::FogColour),
        Ok(HorizonOrigin::Bound(provenance)) if provenance.class == ClaimStatus::Designed
    ));
}

#[test]
fn accept_m01_play_sky_an_unknown_environment_field_falls_back_to_the_labelled_design() {
    // The storm fixture authors no ambient colour: the spec must fall back
    // to its design and say so, never colour silently.
    let environment = storm_environment().expect("the fixture is representable");
    assert!(matches!(
        environment.lighting().ambient_linear(),
        Resolved::Unknown { .. }
    ));
    let spec = HorizonSpec::from_environment(&environment);
    assert_eq!(
        spec.origin(HorizonField::AmbientColour)
            .unwrap_or_else(|error| panic!("{error}")),
        &HorizonOrigin::Designed(DESIGNED_LABEL)
    );
    assert_eq!(
        spec.ambient_colour(),
        HorizonSpec::designed().ambient_colour()
    );
    assert!(spec.is_entirely_designed());
}

#[test]
fn accept_m01_play_sky_the_dome_shows_the_fog_colour_below_the_horizon() {
    let spec = HorizonSpec::designed();
    let linear_of = |srgb: [f32; 3]| {
        let linear = Color::srgb(srgb[0], srgb[1], srgb[2]).to_linear();
        [linear.red, linear.green, linear.blue, 1.0]
    };

    // Below the horizon: the fog colour exactly, so the fogged sea fades
    // into what the dome paints under the horizon line. At the horizon
    // itself: the horizon colour. At the zenith: the sky's own colour.
    assert_eq!(spec.sky_colour_at(-1.0), linear_of(spec.fog_colour()));
    assert_eq!(spec.sky_colour_at(-0.001), linear_of(spec.fog_colour()));
    assert_eq!(spec.sky_colour_at(0.0), linear_of(spec.sky_horizon()));
    assert_eq!(spec.sky_colour_at(1.0), linear_of(spec.sky_zenith()));
    assert_eq!(spec.sky_colour_at(2.0), linear_of(spec.sky_zenith()));

    // The mesh is built from that one function: every vertex carries it.
    let mesh = spec.dome_mesh();
    let positions = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(positions)) => positions.clone(),
        other => panic!("the dome stores float3 positions, found {other:?}"),
    };
    let colours = match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
        Some(VertexAttributeValues::Float32x4(colours)) => colours.clone(),
        other => panic!("the dome carries float4 vertex colours, found {other:?}"),
    };
    assert_eq!(colours.len(), positions.len());
    assert!(!positions.is_empty());

    let lowest = positions
        .iter()
        .zip(&colours)
        .min_by(|(a, _), (b, _)| a[1].partial_cmp(&b[1]).expect("finite elevation"))
        .expect("the dome has vertices");
    let highest = positions
        .iter()
        .zip(&colours)
        .max_by(|(a, _), (b, _)| a[1].partial_cmp(&b[1]).expect("finite elevation"))
        .expect("the dome has vertices");
    assert_eq!(lowest.1, &linear_of(spec.fog_colour()));
    assert_eq!(highest.1, &linear_of(spec.sky_zenith()));
}

#[test]
fn accept_m01_play_sky_the_fog_saturates_inside_the_dome_and_the_sea_reaches_past_it() {
    let spec = HorizonSpec::designed();
    assert!(
        spec.fog_start_m() < spec.fog_end_m(),
        "the fog must start before it saturates"
    );
    assert!(
        spec.fog_end_m() < spec.dome_radius_m(),
        "the dome may only occlude world the fog has already taken to the haze \
         colour, or its edge becomes a visible line"
    );
    assert!(
        spec.dome_radius_m() < FAR_PLANE_M,
        "the dome must stay inside the camera's far plane or no sky is drawn"
    );
    assert!(
        spec.sea_extent_m() >= spec.fog_end_m(),
        "the sea must reach past the fog's far distance or its edge is visible"
    );
    assert!(
        spec.sea_level_m() < 0.0,
        "the sea is below the world the mission spawns"
    );
}

#[test]
fn accept_m01_play_sky_the_plugin_draws_dome_sea_and_fog_around_the_camera() {
    let mut app = cs_app::asset_stack::headless_app();
    // The windowed scene gets these from Bevy's render plugins; the headless
    // face registers the same asset so the production spawn runs unchanged.
    app.init_asset::<StandardMaterial>();
    let spec = HorizonSpec::designed();
    app.add_plugins(HorizonPlugin::new(spec.clone()));

    let camera = app
        .world_mut()
        .spawn((
            Camera3d::default(),
            Transform::from_xyz(1_024.0, 64.0, -2_048.0),
        ))
        .id();
    // Frame one runs `Startup` (the spawn) and `Update`; a `GlobalTransform`
    // is propagated in `PostUpdate`, so the second frame is the first one
    // that can be observed centred on the camera.
    app.update();
    app.update();

    let world = app.world_mut();
    let mut domes = world.query::<(
        &HorizonDome,
        &Transform,
        &Mesh3d,
        &MeshMaterial3d<StandardMaterial>,
    )>();
    let drawn = domes.iter(world).collect::<Vec<_>>();
    assert_eq!(drawn.len(), 1, "exactly one sky dome is drawn");
    let (_, dome_transform, _, dome_material) = drawn[0];
    let material = world
        .get_resource::<bevy::prelude::Assets<StandardMaterial>>()
        .expect("the material asset collection exists")
        .get(&dome_material.0)
        .expect("the dome's material exists");
    assert!(material.unlit, "the sky is not lit by the scene's lamps");
    assert!(
        !material.fog_enabled,
        "the dome's distance never changes, so fog would wash its gradient flat"
    );
    assert_eq!(
        material.cull_mode, None,
        "the camera is inside the dome and sees its back faces"
    );

    // F19 behaviour 3: centred on the camera's translation, world-oriented.
    let camera_translation = world
        .get::<Transform>(camera)
        .expect("the camera keeps its transform")
        .translation;
    assert_eq!(dome_transform.translation, camera_translation);
    assert_eq!(
        dome_transform.rotation,
        bevy::prelude::Quat::IDENTITY,
        "rolling the camera must not tilt the horizon"
    );
    assert_eq!(
        dome_transform.scale,
        Vec3::splat(spec.dome_radius_m()),
        "the dome is the spec's radius wide"
    );

    let mut seas = world.query::<(&HorizonSea, &Transform, &Mesh3d)>();
    let drawn = seas.iter(world).collect::<Vec<_>>();
    assert_eq!(drawn.len(), 1, "exactly one sea plane is drawn");
    let (_, sea_transform, _) = drawn[0];
    assert_eq!(sea_transform.translation.y, spec.sea_level_m());
    assert_eq!(sea_transform.translation.x, camera_translation.x);
    assert_eq!(sea_transform.translation.z, camera_translation.z);

    let fog = world
        .get::<DistanceFog>(camera)
        .expect("the camera carries the scene's fog");
    match fog.falloff {
        FogFalloff::Linear { start, end } => {
            assert_eq!(start, spec.fog_start_m());
            assert_eq!(end, spec.fog_end_m());
        }
        ref other => panic!("the scene's fog is linear, found {other:?}"),
    }
    assert_eq!(
        fog.color,
        Color::srgb(
            spec.fog_colour()[0],
            spec.fog_colour()[1],
            spec.fog_colour()[2],
        )
    );
    drop(drawn);

    // Fly: two frames later the dome is re-centred on the new position and
    // the sea has followed horizontally while keeping the spec's level.
    let world = app.world_mut();
    world
        .get_mut::<Transform>(camera)
        .expect("the camera keeps its transform")
        .translation = Vec3::new(0.0, 512.0, 0.0);
    app.update();
    app.update();
    let world = app.world_mut();
    let mut domes = world.query::<(&HorizonDome, &Transform)>();
    let dome = domes.iter(world).next().expect("the dome is still spawned");
    assert_eq!(dome.1.translation, Vec3::new(0.0, 512.0, 0.0));
    assert_eq!(dome.1.rotation, bevy::prelude::Quat::IDENTITY);
    let mut seas = world.query::<(&HorizonSea, &Transform)>();
    let sea = seas.iter(world).next().expect("the sea is still spawned");
    assert_eq!(
        sea.1.translation,
        Vec3::new(0.0, spec.sea_level_m(), 0.0),
        "the sea follows the camera horizontally and keeps its level"
    );
}

/// The retail half: M01's own `weather.zrd` through the production bind, so
/// the horizon a composed mission draws is the one the original mission's
/// record asks for — which today is a labelled design, because the original's
/// fog and ambient conventions are unmeasured (`docs/findings/
/// 2026-10-05-m01-lc-weather-bind.md`).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_sky_retail_m01_horizon_is_bound_to_its_weather_and_labelled() {
    use cs_assets::install;
    use cs_sim::time::TickRate;

    let root = std::path::PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the original installation"),
    );
    let found = install::discover(&root).expect("the installation is discoverable");
    let weather =
        read_mission_weather(&found, M01).unwrap_or_else(|error| panic!("{M01}: {error}"));
    let session = weather
        .session(TickRate::new(64).expect("64 Hz"), RunSeeds::from_root(7))
        .expect("M01's weather starts a session");

    let spec = HorizonPlugin::from_environment(session.definition())
        .spec()
        .clone();

    // The record is M01's own, not a fixture.
    assert!(session.definition().origin().is_original());
    // Task #1460 measured `FOG_COLOR`'s scale, so the shared zone colour is
    // bound; everything else M01's weather leaves unknown stays a labelled
    // design, so no unmeasured value reaches the screen.
    let Resolved::Known(fog) = session.definition().fog().color_linear() else {
        panic!("M01's zones spell one shared fog colour");
    };
    assert_eq!(spec.fog_colour(), srgb_of(fog.value));
    for field in HORIZON_FIELDS {
        let expected = match field {
            HorizonField::FogColour => HorizonOrigin::Bound(fog.provenance.clone()),
            HorizonField::SkyHorizon => {
                HorizonOrigin::Designed("the dome's horizon band follows the fog colour")
            }
            _ => HorizonOrigin::Designed(DESIGNED_LABEL),
        };
        assert_eq!(
            spec.origin(*field)
                .unwrap_or_else(|error| panic!("{error}")),
            &expected,
            "{field:?} for M01"
        );
    }
    assert!(
        !spec.is_entirely_designed(),
        "the bound fog colour must show as bound"
    );
    // The seam rule holds on the retail path too.
    assert_eq!(spec.sky_horizon(), spec.fog_colour());
}
