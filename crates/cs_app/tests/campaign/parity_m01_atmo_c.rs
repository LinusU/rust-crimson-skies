//! PARITY-M01-ATMO-C (Rally #1650): the M01 sea is drawn with the world's own
//! water art instead of a flat designed colour.
//!
//! * the synthetic members pin the rule (CI): a bound water art gives the sea
//!   mesh texture coordinates at the world tiles' own density, the sea follows
//!   the camera only in whole texture repeats, and the origin is labelled;
//! * the retail member measures M01's own water through the production stage,
//!   binds a spec, and asserts the art is the original `wtr0xxxx` image with a
//!   non-degenerate mapping. `#[ignore = "requires CS_GAME_DIR"]`.
//!
//! Nothing here is `verified_original`: the texture and its mapping are
//! measured from the installation's geometry; how far the original repeats it,
//! and the unit of its 16-frame cycle, are not.

use bevy::mesh::VertexAttributeValues;
use bevy::prelude::{App, Mesh, StandardMaterial};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionWorldParts, bind_sea_to_world_water, build_headless, stage_for,
};
use cs_app::render::horizon::{
    DESIGNED_LABEL, HorizonField, HorizonOrigin, HorizonSpec, WORLD_WATER_ART_LABEL,
};

use crate::campaign::common::label;
use crate::campaign::vs_m01_rt_window::game_dir;

/// A mapping of 1 repeat per 1024 m on u and 1 per 512 m on v, rotated a
/// little so the snap has to solve a full 2x2 system.
const MAPPING: [[f32; 2]; 2] = [[1.0 / 1024.0, 0.25 / 1024.0], [-0.5 / 512.0, 1.0 / 512.0]];

fn art_spec() -> HorizonSpec {
    let mut spec = HorizonSpec::designed();
    spec.bind_world_water_art(Default::default(), MAPPING, "wtr00000");
    spec
}

/// **Water art gives the sea texture coordinates at the tiles' own density.**
#[test]
fn accept_parity_m01_atmo_c_sea_mesh_carries_the_measured_mapping() {
    let plain = HorizonSpec::designed();
    assert!(plain.sea_mesh().attribute(Mesh::ATTRIBUTE_UV_0).is_some()); // Plane3d's own
    assert_eq!(plain.sea_uv_at(10.0, 10.0), None);

    let spec = art_spec();
    assert_eq!(spec.water_art_image(), Some("wtr00000"));
    let mesh = spec.sea_mesh();
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        panic!("positions");
    };
    let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else {
        panic!("uvs");
    };
    assert_eq!(positions.len(), uvs.len());
    for (p, uv) in positions.iter().zip(uvs) {
        let expected = spec.sea_uv_at(p[0], p[2]).expect("art bound");
        assert!((uv[0] - expected[0]).abs() < 1e-4 && (uv[1] - expected[1]).abs() < 1e-4);
    }
    // 20 km half extent at 1/1024 per metre: the sea repeats the art ~20 times
    // each way, not once across the whole plane.
    assert!(uvs.iter().any(|uv| uv[0].abs() > 10.0));
}

/// **The sea follows the camera in whole texture repeats, so water never slides.**
#[test]
fn accept_parity_m01_atmo_c_sea_snaps_to_whole_repeats() {
    let spec = art_spec();
    for (x, z) in [(-1426.0, -1813.0), (0.0, 0.0), (7777.7, -3141.5)] {
        let [sx, sz] = spec.sea_snap(x, z);
        let [u, v] = spec.sea_uv_at(sx, sz).expect("art bound");
        assert!((u - u.round()).abs() < 1e-3, "u {u} at {x},{z}");
        assert!((v - v.round()).abs() < 1e-3, "v {v} at {x},{z}");
        // Within one repeat of the camera.
        assert!((sx - x).abs() < 2048.0 && (sz - z).abs() < 2048.0);
    }
    // Unbound: the sea sits exactly under the camera as before.
    assert_eq!(HorizonSpec::designed().sea_snap(12.5, -3.0), [12.5, -3.0]);
}

/// **The art is labelled: original texture, designed extent.**
#[test]
fn accept_parity_m01_atmo_c_art_is_labelled_not_claimed() {
    let designed = HorizonSpec::designed();
    assert_eq!(
        designed.origin(HorizonField::SeaColour),
        Ok(&HorizonOrigin::Designed(DESIGNED_LABEL))
    );
    let spec = art_spec();
    assert_eq!(
        spec.origin(HorizonField::SeaColour),
        Ok(&HorizonOrigin::Designed(WORLD_WATER_ART_LABEL))
    );
    assert!(spec.is_entirely_designed());
    // Binding on an app without a world water measurement binds nothing.
    let mut app = App::new();
    app.insert_resource(HorizonSpec::designed());
    assert!(!bind_sea_to_world_water(
        &mut app,
        &MissionWorldParts::default()
    ));
    assert!(
        app.world()
            .resource::<HorizonSpec>()
            .water_art_image()
            .is_none()
    );
}

/// **M01's own water tiles supply the sea's art.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_atmo_c_retail_water_art_is_wtr_with_a_measured_mapping() {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();
    let parts = app.world().resource::<MissionWorldParts>().clone();
    let art = parts.water_art().expect("M01's world binds water art");
    assert!(
        art.image.to_ascii_lowercase().contains("wtr0"),
        "the original water image: {}",
        art.image
    );
    let [[a, b], [c, d]] = art.uv_from_xz;
    assert!((a * d - b * c).abs() > 1e-9, "a non-degenerate mapping");
    // The mapping repeats at tile scale, not at world scale.
    let span = 1.0 / a.abs().max(b.abs()).max(c.abs()).max(d.abs());
    assert!((1.0..100_000.0).contains(&span), "metres per repeat {span}");
    assert!(
        app.world()
            .resource::<bevy::asset::Assets<StandardMaterial>>()
            .get(&art.material)
            .is_some_and(|m| m.base_color_texture.is_some()),
        "the water material is textured"
    );

    app.insert_resource(HorizonSpec::designed());
    assert!(bind_sea_to_world_water(&mut app, &parts));
    let spec = app.world().resource::<HorizonSpec>();
    assert_eq!(spec.water_art_image(), Some(art.image.as_str()));
    assert_eq!(
        spec.origin(HorizonField::SeaColour),
        Ok(&HorizonOrigin::Designed(WORLD_WATER_ART_LABEL))
    );
}

/// PARITY2-M01-SEA-SHADING (Rally #1730): **the sea plane carries the tiles'
/// mean stored corner colour**, so it does not read as a lighter patch where
/// the tiles (drawn `texture × corner colour`) leave a gap.
#[test]
fn accept_parity2_m01_sea_shading_plane_carries_the_tiles_corner_colour() {
    let shade = [170.0 / 255.0, 170.0 / 255.0, 170.0 / 255.0];
    // Unbound art: nothing to shade, the plane is unchanged.
    let mut plain = HorizonSpec::designed();
    plain.bind_world_water_shade(shade);
    assert_eq!(plain.water_corner_shade(), None);
    assert!(plain.sea_mesh().attribute(Mesh::ATTRIBUTE_COLOR).is_none());

    let mut spec = art_spec();
    assert_eq!(spec.water_corner_shade(), Some([1.0; 3]));
    spec.bind_world_water_shade(shade);
    let mesh = spec.sea_mesh();
    let Some(VertexAttributeValues::Float32x4(colours)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
    else {
        panic!("sea corner colours");
    };
    assert!(!colours.is_empty());
    for c in colours {
        assert_eq!(*c, [shade[0], shade[1], shade[2], 1.0]);
    }
}
