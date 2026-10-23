//! Acceptance tests for task #1440 (`M01-PLAY-WEATHER`), as narrowed by task
//! #2001 (`BUG-M01-WHITE-WEATHER-SHEET`): the one production path that draws a
//! windowed scene's designed cloud deck, and the proof that no flat haze plane
//! and no flat deck for an authored cloud *volume* is drawn.
//!
//! Spec: `specs/F19-sky-atmosphere-weather-and-visibility.md` (non-negotiable
//! 1 and 3). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`. Task test
//! prefix: `accept_m01_play_weather_`.
//!
//! Everything here drives production code: `cs_app::render::weather` is the
//! module the original-assets playtest and the composed mission both install
//! (`crate::playtest::windowed_app`, `crate::mission_session::compose`), and
//! the retail half reads M01's own `weather.zrd` through
//! `cs_app::environment::read_mission_weather`, the reader task #636 bound.
//!
//! What is pinned:
//!
//! * every drawn value of the designed deck is **labelled**;
//! * a record that authors a cloud band (the clear-sky fixture; the retail
//!   `ZBD/C3/M01` band 10 000–11 000 m) draws **no** quad: the band is a
//!   volume, and a coplanar sheet washed the island out white (#2001);
//! * a record that authors none gets the labelled design instead of an
//!   invented layout;
//! * no layer is a haze: nothing sits at the old 300 m veil altitude;
//! * the deck sits above the water, inside the horizon's fog, and the plugin
//!   keeps it horizontally under the camera at its own altitude.

use bevy::asset::{AssetApp, Assets};
use bevy::mesh::{Mesh, Mesh3d, VertexAttributeValues};
use bevy::prelude::{
    AlphaMode, Camera3d, Color, MeshMaterial3d, Quat, StandardMaterial, Transform, Vec3,
};
use cs_app::environment::fixture::{clear_sky_environment, storm_environment};
use cs_app::environment::{RunSeeds, read_mission_weather};
use cs_app::render::horizon::HorizonSpec;
use cs_app::render::weather::{
    DESIGNED_CLOUD_ALTITUDE_M, DESIGNED_LABEL, WEATHER_FIELDS, WeatherLayerSpec, WeatherOrigin,
    WeatherPlugin, WeatherSpec,
};

/// The actual campaign M01, "The Lost Treasure". (`zbd/c1c/m01` is a
/// different archive: an earlier version of this file read it by mistake and
/// recorded its `BOTTOM 1055` as M01's.)
const M01: &str = "zbd/c3/m01";

/// The altitude of the removed haze plane, in metres. No drawn layer may sit
/// here again.
const REMOVED_HAZE_ALTITUDE_M: f32 = 300.0;

#[test]
fn accept_m01_play_weather_the_designed_weather_labels_every_value_it_draws() {
    let spec = WeatherSpec::designed();
    assert!(spec.is_entirely_designed());
    assert_eq!(spec.layers().len(), 1, "one cloud deck and no haze");
    for (index, _layer) in spec.layers().iter().enumerate() {
        for field in WEATHER_FIELDS {
            assert_eq!(
                spec.origin(index, *field)
                    .unwrap_or_else(|error| panic!("{error}")),
                &WeatherOrigin::Designed(DESIGNED_LABEL),
                "{field:?} of layer {index} must be a labelled designed value, not a silent default"
            );
        }
    }
    // The label names the task, so a screenshot or a review note can trace the
    // clouds back to the build that painted them.
    assert!(DESIGNED_LABEL.contains("M01-PLAY-WEATHER"));
    assert_eq!(spec.layers()[0].altitude_m(), DESIGNED_CLOUD_ALTITUDE_M);
}

#[test]
fn accept_m01_play_weather_an_authored_cloud_band_is_a_volume_and_draws_no_sheet() {
    // The clear-sky fixture authors one cloud layer: 1 500 m at a 0.25
    // coverage. A band is a volume in the original, so no quad is drawn for
    // it, and no haze quad either, whatever fog colour the record has.
    let environment = clear_sky_environment().expect("the fixture is representable");
    assert_eq!(environment.cloud_layers().len(), 1);
    let spec = WeatherSpec::from_environment(&environment);
    assert!(
        spec.layers().is_empty(),
        "an authored band must not become a flat sheet, found {:?}",
        spec.layers()
    );
}

#[test]
fn accept_m01_play_weather_a_record_with_no_layer_falls_back_to_the_labelled_design() {
    // The storm fixture authors no cloud layer at all. No layout may be
    // invented for it: the drawn deck is the labelled design.
    let environment = storm_environment().expect("the fixture is representable");
    assert!(
        environment.cloud_layers().is_empty(),
        "the storm fixture authors no cloud layer"
    );
    let spec = WeatherSpec::from_environment(&environment);

    let decks = spec.layers().iter().collect::<Vec<_>>();
    assert_eq!(decks.len(), 1, "one designed deck");
    let designed = WeatherSpec::designed();
    assert_eq!(decks[0].altitude_m(), designed.layers()[0].altitude_m());
    assert_eq!(decks[0].coverage(), designed.layers()[0].coverage());
    assert_eq!(decks[0].colour(), designed.layers()[0].colour());
    for field in WEATHER_FIELDS {
        assert_eq!(
            spec.origin(0, *field)
                .unwrap_or_else(|error| panic!("{error}")),
            &WeatherOrigin::Designed(DESIGNED_LABEL),
            "{field:?} carries no measured value and must stay labelled"
        );
    }
    assert_eq!(
        spec.layers().len(),
        1,
        "the deck is the only layer: no haze"
    );
    assert!(spec.is_entirely_designed());
}

#[test]
fn accept_m01_play_weather_the_deck_sits_above_the_water_and_inside_the_fog() {
    let spec = WeatherSpec::designed();
    let horizon = HorizonSpec::designed();
    let deck = spec
        .layers()
        .first()
        .expect("the designed spec draws one deck");

    assert!(
        deck.altitude_m() > horizon.sea_level_m(),
        "the deck must be above the water or it is drawn inside the sea"
    );
    assert!(
        deck.altitude_m() < horizon.dome_radius_m(),
        "a deck above the dome's radius would be occluded by the sky itself"
    );
    for layer in spec.layers() {
        assert!(
            (layer.altitude_m() - REMOVED_HAZE_ALTITUDE_M).abs() > 1.0,
            "no layer may sit at the removed haze altitude"
        );
        assert!(
            layer.half_extent_m() >= horizon.fog_end_m(),
            "a layer must reach past the fog's far distance or its edge is visible"
        );
        assert!(
            layer.coverage() > 0.0 && layer.coverage() <= 1.0,
            "a coverage fraction is the layer's alpha and must be in (0, 1]"
        );
    }
}

#[test]
fn accept_m01_play_weather_the_plugin_draws_each_layer_under_the_camera() {
    use cs_app::render::weather::WeatherLayer;
    let mut app = cs_app::asset_stack::headless_app();
    // The windowed scene gets these from Bevy's render plugins; the headless
    // face registers the same asset so the production spawn runs unchanged.
    app.init_asset::<StandardMaterial>();
    let spec = WeatherSpec::designed();
    app.add_plugins(WeatherPlugin::new(spec.clone()));

    let camera = app
        .world_mut()
        .spawn((
            Camera3d::default(),
            Transform::from_xyz(512.0, 96.0, -1_024.0),
        ))
        .id();
    // Frame one runs `Startup` (the spawn) and `Update`; a `GlobalTransform`
    // is propagated in `PostUpdate`, so the second frame is the first one
    // that can be observed centred on the camera.
    app.update();
    app.update();

    let world = app.world_mut();
    let mut query = world.query::<(
        &WeatherLayer,
        &Transform,
        &Mesh3d,
        &MeshMaterial3d<StandardMaterial>,
    )>();
    let mut drawn = query
        .iter(world)
        .map(|(drawn, transform, mesh, material)| {
            (drawn.layer, *transform, mesh.0.clone(), material.0.clone())
        })
        .collect::<Vec<_>>();
    drawn.sort_by_key(|(index, _, _, _)| *index);
    assert_eq!(drawn.len(), spec.layers().len(), "one entity per layer");

    let materials = world
        .get_resource::<Assets<StandardMaterial>>()
        .expect("the material asset collection exists");
    let meshes = world
        .get_resource::<Assets<Mesh>>()
        .expect("the mesh asset collection exists");

    for (index, transform, mesh, material) in &drawn {
        let layer = &spec.layers()[*index];
        assert_eq!(
            transform.translation,
            Vec3::new(512.0, layer.altitude_m(), -1_024.0),
            "layer {index} follows the camera horizontally and keeps its altitude"
        );
        assert_eq!(transform.rotation, bevy::prelude::Quat::IDENTITY);
        let material = materials
            .get(material)
            .expect("every drawn layer's material exists");
        assert!(material.unlit, "a layer is the air, not a lit surface");
        assert_eq!(
            material.alpha_mode,
            AlphaMode::Blend,
            "the layer's own alpha is drawn as its opacity"
        );
        assert_eq!(material.cull_mode, None, "a layer is seen from both sides");
        assert!(
            material.fog_enabled,
            "a layer's far parts must fade into the same haze as everything else"
        );
        // The colour and the alpha both travel in the mesh's own vertices, so
        // the material is white and the layer's values are not lost in it.
        assert_eq!(material.base_color, Color::WHITE);
        let mesh = meshes.get(mesh).expect("every drawn layer's mesh exists");
        assert_drawn_colour(mesh, layer);
    }
    drop(drawn);

    // Nothing may leave a layer tilted, not even a value somebody else wrote:
    // the placement re-asserts the layer's own frame on every frame.
    {
        let world = app.world_mut();
        let mut query = world.query::<(&WeatherLayer, &mut Transform)>();
        for (_, mut transform) in query.iter_mut(world) {
            transform.rotation = Quat::from_rotation_x(0.7);
        }
    }

    // Climb and roll: two frames later every layer has followed the camera
    // horizontally, still at its own altitude, and none of them has inherited
    // the camera's attitude or kept the tilt somebody else wrote (F19
    // non-negotiable 3: rolling cannot tilt a layer).
    app.world_mut()
        .get_mut::<Transform>(camera)
        .expect("the camera keeps its transform")
        .translation = Vec3::new(0.0, 4_096.0, 0.0);
    app.world_mut()
        .get_mut::<Transform>(camera)
        .expect("the camera keeps its transform")
        .rotation = Quat::from_rotation_z(1.1) * Quat::from_rotation_x(0.4);
    app.update();
    app.update();
    let world = app.world_mut();
    let mut query = world.query::<(&WeatherLayer, &Transform)>();
    let mut seen = 0;
    for (layer, transform) in query.iter(world) {
        assert_eq!(transform.translation.x, 0.0);
        assert_eq!(transform.translation.z, 0.0);
        assert_eq!(
            transform.translation.y,
            spec.layers()[layer.layer].altitude_m()
        );
        assert_eq!(
            transform.rotation,
            Quat::IDENTITY,
            "layer {} must not inherit the camera's roll or pitch",
            layer.layer
        );
        seen += 1;
    }
    assert_eq!(seen, spec.layers().len());
}

/// The drawn alpha values of one layer's mesh, in the mesh's own grid order.
fn mesh_alphas(mesh: &Mesh) -> Vec<f32> {
    match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
        Some(VertexAttributeValues::Float32x4(colours)) => {
            colours.iter().map(|colour| colour[3]).collect()
        }
        other => panic!("a drawn layer carries float4 vertex colours, found {other:?}"),
    }
}

/// Asserts that a layer's mesh paints the layer's own colour without lying
/// about it: every vertex carries that colour once and the layer's own alpha.
fn assert_drawn_colour(mesh: &Mesh, layer: &WeatherLayerSpec) {
    let linear = Color::srgb(layer.colour()[0], layer.colour()[1], layer.colour()[2]).to_linear();
    let colours = match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
        Some(VertexAttributeValues::Float32x4(colours)) => colours.clone(),
        other => panic!("a drawn layer carries float4 vertex colours, found {other:?}"),
    };
    assert!(!colours.is_empty(), "a drawn layer has vertices");
    for colour in &colours {
        assert_eq!(colour[0], linear.red);
        assert_eq!(colour[1], linear.green);
        assert_eq!(colour[2], linear.blue);
        assert!(
            (0.0..=1.0).contains(&colour[3]),
            "a drawn alpha is a fraction, found {}",
            colour[3]
        );
    }
}

/// **A fixed physics step inside the same headless stack never finds a
/// system parameter missing.** (#1471)
///
/// `App::update` never runs `Plugin::finish` — only `App::run` does — so
/// Avian's `*Diagnostics` resources, which its plugins create only in
/// `finish`, were absent in this composition's world. The flake fired only
/// when wall-clock jitter happened to advance the fixed clock enough to run
/// `PhysicsSchedule` — the first `update` always advances zero (`Time<Real>`
/// learns its `last_update` on the first call), so a second update driving a
/// whole manual second is what forces `FixedPostUpdate`'s
/// `run_physics_schedule` to step. On a stack that does not register them
/// the missing `ResMut` then panics deterministically instead of
/// intermittently.
#[test]
fn accept_m01_play_weather_the_headless_stack_answers_a_fixed_steps_resources() {
    let mut app = cs_app::asset_stack::headless_app();
    app.init_asset::<StandardMaterial>();
    app.add_plugins(WeatherPlugin::designed());
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_secs(1),
    ));
    // The first update only establishes the real clock; the second applies
    // the manual second and fires the fixed steps that read the resources.
    app.update();
    app.update();
    assert!(
        app.world()
            .contains_resource::<avian3d::collider_tree::ColliderTreeDiagnostics>(),
        "the stack registers the diagnostics resources a fixed step reads"
    );
}

#[test]
fn accept_m01_play_weather_the_deck_paints_its_coverage_and_leaves_the_sky_visible() {
    use cs_app::render::weather::LAYER_MESH_CELLS;

    // A deck is a broken layer, not a sheet: the share of its own grid the
    // designed pattern paints is the layer's coverage, and the gaps between
    // the clouds are clear, so the horizon's sky stays visible through them.
    let designed = WeatherSpec::designed();
    let deck = designed
        .layers()
        .first()
        .expect("the designed spec draws a deck");
    let alphas = mesh_alphas(&deck.mesh());
    assert_eq!(
        alphas.len(),
        (LAYER_MESH_CELLS + 1) * (LAYER_MESH_CELLS + 1)
    );
    let lowest = alphas.iter().copied().fold(f32::INFINITY, f32::min);
    let highest = alphas.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    assert!(lowest < 0.05, "a gap must stay clear, found alpha {lowest}");
    assert!(
        highest > 0.95,
        "a cloud must be opaque, found alpha {highest}"
    );
    let painted = alphas.iter().filter(|alpha| **alpha > 0.5).count() as f64;
    #[allow(clippy::cast_precision_loss)]
    let share = painted / alphas.len() as f64;
    let wanted = f64::from(deck.coverage());
    assert!(
        (share - wanted).abs() < 0.02,
        "the painted share {share} must be the layer's coverage {wanted}"
    );
}

/// The retail half: the actual campaign M01 (`ZBD/C3/M01`) through the
/// production bind. Its cloud band is 10 000–11 000 m and its zones' fog
/// colours differ (`docs/findings/2026-10-18-parity-m01-atmo-a-weather-units-
/// and-sky-consumers.md`); neither becomes a drawn quad, so there is no sheet
/// at any flight altitude (#2001).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_weather_retail_m01_draws_no_sheet_at_any_altitude() {
    use cs_assets::install;
    use cs_sim::time::TickRate;

    let root = std::path::PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the original installation"),
    );
    let found = install::discover(&root).expect("the installation is discoverable");
    let weather = read_mission_weather(&found, M01).unwrap_or_else(|error| panic!("{error}"));

    // The raw record: the C3 cloud band, and fog colours that differ by zone.
    let cloud = &weather.document.cloud_cover;
    assert_eq!(cloud.bottom.as_f64(), 10_000.0, "CLOUD_COVER.BOTTOM");
    assert_eq!(cloud.top.as_f64(), 11_000.0, "CLOUD_COVER.TOP");
    let fog = |name: &str| {
        weather
            .document
            .zones
            .iter()
            .find(|zone| zone.name == name)
            .unwrap_or_else(|| panic!("M01 has {name}"))
            .fog_color
            .map(cs_content::weather::WeatherScalar::as_f64)
    };
    assert_ne!(
        fog("ZONE1"),
        fog("ZONE2"),
        "M01's zones do not share one fog colour"
    );

    // The bound definition: one layer at the band's floor.
    let session = weather
        .session(TickRate::new(64).expect("64 Hz"), RunSeeds::from_root(7))
        .expect("M01's weather starts a session");
    let definition = session.definition();
    assert!(definition.origin().is_original());
    let layers = definition.cloud_layers();
    assert_eq!(layers.len(), 1, "CLOUD_COVER binds one cloud layer");
    assert_eq!(layers[0].altitude_m(), 10_000.0, "BOTTOM in metres");

    // The drawn weather: nothing. Not a deck at the band, not a haze.
    let spec = WeatherPlugin::from_environment(definition).spec().clone();
    assert!(
        spec.layers().is_empty(),
        "M01 must draw no weather quad, found {:?}",
        spec.layers()
            .iter()
            .map(WeatherLayerSpec::altitude_m)
            .collect::<Vec<_>>()
    );
}
