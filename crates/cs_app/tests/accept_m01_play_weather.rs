//! Acceptance tests for task #1440 (`M01-PLAY-WEATHER`): the one production
//! path that draws a mission's cloud deck and haze for a windowed scene, and
//! the origin of every value it draws them from.
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
//! * every drawn value of the designed weather is **labelled**, so a
//!   screenshot or a review can always say these clouds are this build's;
//! * a cloud layer an environment record *does* author reaches the screen at
//!   its own altitude and with its own coverage, with its provenance;
//! * a record that authors no layer (and M01's record binds none) gets the
//!   labelled design instead of an invented layout;
//! * the layers sit above the water and inside the horizon's haze, so neither
//!   a deck's edge nor the haze's edge can be seen against a different colour;
//! * the plugin spawns one entity per layer and keeps every one of them
//!   horizontally under the camera at its own world altitude.

use bevy::asset::{AssetApp, Assets};
use bevy::mesh::{Mesh, Mesh3d, VertexAttributeValues};
use bevy::prelude::{
    AlphaMode, Camera3d, Color, MeshMaterial3d, Quat, StandardMaterial, Transform, Vec3,
};
use cs_app::environment::fixture::{clear_sky_environment, storm_environment};
use cs_app::environment::{RunSeeds, read_mission_weather};
use cs_app::render::horizon::HorizonSpec;
use cs_app::render::weather::{
    DESIGNED_CLOUD_ALTITUDE_M, DESIGNED_HAZE_ALTITUDE_M, DESIGNED_LABEL, WEATHER_FIELDS,
    WeatherField, WeatherLayerKind, WeatherLayerSpec, WeatherOrigin, WeatherPlugin, WeatherSpec,
};
use cs_types::content::Resolved;

const M01: &str = "zbd/c1c/m01";

/// A record's colour, converted the way the spec converts one for display.
fn srgb_of(linear: [f64; 3]) -> [f32; 3] {
    let srgb = Color::linear_rgb(linear[0] as f32, linear[1] as f32, linear[2] as f32).to_srgba();
    [srgb.red, srgb.green, srgb.blue]
}

#[test]
fn accept_m01_play_weather_the_designed_weather_labels_every_value_it_draws() {
    let spec = WeatherSpec::designed();
    assert!(spec.is_entirely_designed());
    assert_eq!(spec.layers().len(), 2, "a cloud deck and the haze");
    assert_eq!(
        spec.layers()
            .iter()
            .map(|layer| layer.kind())
            .collect::<Vec<_>>(),
        vec![WeatherLayerKind::CloudDeck, WeatherLayerKind::Haze]
    );
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
    // No two layers claim the same altitude: the haze is under the deck.
    assert_eq!(spec.layers()[0].altitude_m(), DESIGNED_CLOUD_ALTITUDE_M);
    assert_eq!(spec.layers()[1].altitude_m(), DESIGNED_HAZE_ALTITUDE_M);
}

#[test]
fn accept_m01_play_weather_an_authored_cloud_layer_reaches_the_drawn_deck() {
    // The clear-sky fixture authors one cloud layer: 1 500 m at a 0.25
    // coverage. The drawn deck must be that layer, not the design.
    let environment = clear_sky_environment().expect("the fixture is representable");
    let spec = WeatherSpec::from_environment(&environment);

    let decks = spec.cloud_layers().collect::<Vec<_>>();
    assert_eq!(decks.len(), 1, "one authored layer, one drawn deck");
    assert_eq!(decks[0].kind(), WeatherLayerKind::CloudDeck);
    assert_eq!(decks[0].altitude_m(), 1_500.0);
    assert_eq!(decks[0].coverage(), 0.25);

    // The altitude comes from the record that stated the layer; the coverage
    // carries its own provenance.
    assert_eq!(
        spec.origin(0, WeatherField::Altitude)
            .unwrap_or_else(|error| panic!("{error}")),
        &WeatherOrigin::Bound(environment.provenance().clone())
    );
    let authored = environment
        .cloud_layers()
        .first()
        .expect("the fixture authors one layer");
    let Resolved::Known(coverage) = authored.coverage() else {
        panic!("the clear-sky fixture authors a known coverage");
    };
    assert_eq!(
        spec.origin(0, WeatherField::Coverage)
            .unwrap_or_else(|error| panic!("{error}")),
        &WeatherOrigin::Bound(coverage.provenance.clone())
    );

    // A `CloudLayer` carries no colour and no extent — the record's
    // `TOP_COLOR`/`BOTTOM_COLOR` are unbound (task #636) — so those two stay
    // the labelled design.
    for field in [WeatherField::Colour, WeatherField::Extent] {
        assert_eq!(
            spec.origin(0, field)
                .unwrap_or_else(|error| panic!("{error}")),
            &WeatherOrigin::Designed(DESIGNED_LABEL),
            "{field:?} has no authored value and must stay labelled"
        );
    }

    // The haze follows the record's own fog colour, so the veil, the fog and
    // the sky the dome paints below the horizon are one value.
    let Resolved::Known(fog) = environment.fog().color_linear() else {
        panic!("the clear-sky fixture authors a fog colour");
    };
    let haze = spec.haze();
    assert_eq!(haze.colour(), srgb_of(fog.value));
    assert_eq!(
        spec.origin(spec.layers().len() - 1, WeatherField::Colour)
            .unwrap_or_else(|error| panic!("{error}")),
        &WeatherOrigin::Bound(fog.provenance.clone())
    );
    // Nothing here is a measurement: the fixture's provenance says designed,
    // so the whole spec stays a design end to end.
    assert!(spec.is_entirely_designed());
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

    let decks = spec.cloud_layers().collect::<Vec<_>>();
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
    // The haze still follows the record's fog colour, which the storm fixture
    // does author.
    let Resolved::Known(fog) = environment.fog().color_linear() else {
        panic!("the storm fixture authors a fog colour");
    };
    assert_eq!(spec.haze().colour(), srgb_of(fog.value));
    assert!(spec.is_entirely_designed());
}

#[test]
fn accept_m01_play_weather_the_layers_sit_above_the_water_and_inside_the_haze() {
    let spec = WeatherSpec::designed();
    let horizon = HorizonSpec::designed();
    let haze = spec.haze();
    let deck = spec
        .cloud_layers()
        .next()
        .expect("the designed spec draws one deck");

    assert!(
        haze.altitude_m() < deck.altitude_m(),
        "the haze veil is under the cloud deck"
    );
    assert!(
        haze.altitude_m() > horizon.sea_level_m(),
        "the haze must be above the water or it is drawn inside the sea"
    );
    assert!(
        deck.altitude_m() < horizon.dome_radius_m(),
        "a deck above the dome's radius would be occluded by the sky itself"
    );
    for layer in spec.layers() {
        assert!(
            layer.half_extent_m() >= horizon.fog_end_m(),
            "a layer must reach past the fog's far distance or its edge is visible"
        );
        assert!(
            layer.coverage() > 0.0 && layer.coverage() <= 1.0,
            "a coverage fraction is the layer's alpha and must be in (0, 1]"
        );
    }
    // The seam rule: the designed haze is the very colour the designed dome
    // paints below the horizon, so the veil fades into the fog rather than
    // into a different colour.
    assert_eq!(haze.colour(), horizon.fog_colour());
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
            (
                drawn.layer,
                drawn.kind,
                *transform,
                mesh.0.clone(),
                material.0.clone(),
            )
        })
        .collect::<Vec<_>>();
    drawn.sort_by_key(|(index, _, _, _, _)| *index);
    assert_eq!(drawn.len(), spec.layers().len(), "one entity per layer");

    let materials = world
        .get_resource::<Assets<StandardMaterial>>()
        .expect("the material asset collection exists");
    let meshes = world
        .get_resource::<Assets<Mesh>>()
        .expect("the mesh asset collection exists");

    for (index, kind, transform, mesh, material) in &drawn {
        let layer = &spec.layers()[*index];
        assert_eq!(*kind, layer.kind());
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
    use cs_app::render::weather::{HAZE_OVERHEAD_FRACTION, LAYER_MESH_CELLS};

    // A deck is a broken layer, not a sheet: the share of its own grid the
    // designed pattern paints is the layer's coverage, and the gaps between
    // the clouds are clear, so the horizon's sky stays visible through them.
    let designed = WeatherSpec::designed();
    let environment = clear_sky_environment().expect("the fixture is representable");
    let bound = WeatherSpec::from_environment(&environment);
    let decks = [
        designed
            .cloud_layers()
            .next()
            .expect("the designed spec draws a deck"),
        bound
            .cloud_layers()
            .next()
            .expect("the fixture's authored layer is drawn"),
    ];
    for deck in decks {
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

    // The haze is a profile, not a sheet either: thinnest overhead, at the
    // coverage towards the horizon, and never above it.
    let haze = designed.haze();
    let alphas = mesh_alphas(&haze.mesh());
    let highest = alphas.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let lowest = alphas.iter().copied().fold(f32::INFINITY, f32::min);
    assert!((highest - haze.coverage()).abs() < 1e-6);
    assert!((lowest - haze.coverage() * HAZE_OVERHEAD_FRACTION).abs() < 1e-6);
}

/// The retail half: M01's own `weather.zrd` through the production bind, so
/// the weather a composed mission draws is the one the original mission's
/// record asks for — the cloud floor at `CLOUD_COVER.BOTTOM` metres and the
/// shared zone `FOG_COLOR`, bound where task #1460 measured their units
/// (`docs/findings/2026-10-16-f19-weather-units-measure.md`), labelled where
/// the record still carries nothing a measurement reached.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_weather_retail_m01_weather_is_bound_to_its_record_and_labelled() {
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
    let definition = session.definition();

    // The record is M01's own, not a fixture, and it *does* describe cloud
    // cover: `BOTTOM 1055` binds the deck's floor altitude in metres and the
    // shared zone colour `(0.69, 0.69, 0.69)` the haze's colour.
    assert!(definition.origin().is_original());
    let layers = definition.cloud_layers();
    assert_eq!(layers.len(), 1, "CLOUD_COVER binds one cloud layer");
    assert_eq!(layers[0].altitude_m(), 1055.0, "BOTTOM in metres");
    let Resolved::Known(fog) = definition.fog().color_linear() else {
        panic!("M01's zones spell one shared fog colour");
    };
    // The record spells 0.69f: the channels are at or below 1.0, so the
    // measured rule takes them verbatim — f32 widened to f64.
    assert_eq!(fog.value, [f64::from(0.69f32); 3]);
    assert!(!definition.fog().density_per_m().is_known());

    // `TOP`/`THICKNESS` and the fog ranges/bands still have no contract
    // fields, so they stay named unbound rather than silently dropped.
    for key in ["CLOUD_COVER.TOP/THICKNESS", "ZONEn.FOG_RANGES/FOG_ALTITUDE"] {
        assert!(
            weather.binding.unbound.iter().any(|field| field.key == key),
            "{key} must stay named as unbound"
        );
    }
    assert!(
        !weather
            .binding
            .unbound
            .iter()
            .any(|field| field.key == "ZONEn.FOG_COLOR"),
        "FOG_COLOR binds when the zones agree"
    );

    let spec = WeatherPlugin::from_environment(definition).spec().clone();
    assert_eq!(spec.layers().len(), 2);
    let deck = spec
        .cloud_layers()
        .next()
        .expect("M01's bound layer is the drawn deck");
    assert_eq!(deck.altitude_m(), 1055.0);
    assert_eq!(
        spec.origin(0, WeatherField::Altitude)
            .unwrap_or_else(|error| panic!("{error}")),
        &WeatherOrigin::Bound(definition.provenance().clone()),
        "the deck's altitude is the record's own"
    );
    assert_eq!(
        spec.origin(0, WeatherField::Coverage)
            .unwrap_or_else(|error| panic!("{error}")),
        &WeatherOrigin::Designed("the record authors no coverage fraction for this layer"),
        "the record spells no coverage fraction, so the drawn coverage is labelled"
    );
    for field in [WeatherField::Colour, WeatherField::Extent] {
        assert_eq!(
            spec.origin(0, field)
                .unwrap_or_else(|error| panic!("{error}")),
            &WeatherOrigin::Designed(DESIGNED_LABEL),
            "{field:?} of the deck has no authored value and stays labelled"
        );
    }
    let haze = spec.haze();
    assert_eq!(haze.colour(), srgb_of(fog.value));
    assert_eq!(
        spec.origin(1, WeatherField::Colour)
            .unwrap_or_else(|error| panic!("{error}")),
        &WeatherOrigin::Bound(fog.provenance.clone()),
        "the haze's colour is the record's own fog colour"
    );
    for field in [
        WeatherField::Altitude,
        WeatherField::Coverage,
        WeatherField::Extent,
    ] {
        assert_eq!(
            spec.origin(1, field)
                .unwrap_or_else(|error| panic!("{error}")),
            &WeatherOrigin::Designed(DESIGNED_LABEL),
            "{field:?} of the haze has no authored value and stays labelled"
        );
    }
    assert!(
        !spec.is_entirely_designed(),
        "the bound altitude and fog colour must show as bound"
    );
}
