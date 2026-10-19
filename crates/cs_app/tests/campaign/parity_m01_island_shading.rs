//! PARITY-M01-ISLAND-SHADING (Rally #1706): M01's island tiles draw their
//! stored corner colours, their bound textures and the world's shading
//! binding — the bindings #1697's first-control comparison found missing.
//!
//! `docs/findings/2026-10-19-parity-m01-island-shading.md` holds the
//! measurement chain: `ZBD/C3`'s terrain tiles store a complete per-corner
//! shading channel while `playtest_textures::slice_groups` rebuilt the
//! drawn per-group parts with every stored attribute except the colours.
//! The authored colours are the surface's own shading input — a
//! vertex-coloured world part draws `texture × colour` unlit and fogged
//! ([`stored_shading_materials`]) — and the drawn colour is the stored
//! triple under the engine's own `>1 → ×1/255` stored-colour rule
//! (`0x4bc350`, `data/owner-static/engine-facts.toml` `colour_byte_scale`).
//!
//! The retail member drives the production path end to end —
//! `plan_mission_launch` → `stage_for` → `build_headless` →
//! `MissionWorldParts` → `attach_world_parts` — on the real `ZBD/C3`
//! container, M01's stage and `rtexture12.zbd`, and asserts on mesh 781's
//! drawn parts:
//!
//! * **material**: each stored material group reached a `PlaytestPart`
//!   parented as a `Mesh3d` + `MeshMaterial3d` child of its world node,
//!   with the stored material table still naming it;
//! * **texture**: a textured part binds a `base_color_texture` and the
//!   archive key of its image, and the island's terrain textures
//!   (`forest1`, `cliff1`, `sand128`) are among them;
//! * **lighting**: each vertex-coloured part's material is `unlit` and
//!   `fog_enabled`, and its `Mesh::ATTRIBUTE_COLOR` carries exactly the
//!   colours the production render mesh gives that material's triangles —
//!   byte-rule normalized, one value per position.
//!
//! `#[ignore = "requires CS_GAME_DIR"]`: no original data in CI. Nothing
//! here is `verified_original`: how the original *applied* the corner
//! colours (modulate, replace or ignore) and its lighting model are
//! `MaterialUnknown::VertexColorMeaning`; the unlit draw is this build's
//! declared presentation, not a measured original rule.

use bevy::asset::Assets;
use bevy::mesh::{Mesh, VertexAttributeValues};
use bevy::prelude::{ChildOf, Children, Entity, MeshMaterial3d, StandardMaterial};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{MissionWorldParts, build_headless, stage_for};
use cs_app::playtest_retail::read_world_sources;
use cs_app::world::contacts::WorldObjectBinding;
use cs_content::mesh::RenderMesh;
use cs_content::world::WorldObjectId;
use cs_types::content::Resolved;

use crate::campaign::common::label;
use crate::campaign::vs_m01_rt_window::game_dir;

/// `ZBD/C3` mesh-array slot 781 — `g28614` (node-2976), the nearest
/// substantial island tile in M01's loaded residency: stored bounds
/// `(-6144, 0, -4096)..(-5120, 342, -3072)`, about 4.7 km from the
/// first-control camera at `(-1426, 250, -1813)`. It stores 197 corners,
/// every one coloured and UV'd, three quarters of them storing a normal —
/// the same island-tile shape as `g28982`/mesh 762 (the tile the finding's
/// measurement took), which M01's residency does not load.
const ISLAND_MESH_SLOT: u32 = 781;

/// The world's mesh key for `ZBD/C3`'s container: the escaped container
/// path plus `.` and the stored mesh-array index.
const MESH_KEY_PREFIX: &str = "zbd_2f_c3_2f_gamez.zbd.";

/// The stored mesh-array index a world mesh key names, or `None`.
fn mesh_slot_of(key: &str) -> Option<u32> {
    key.strip_prefix(MESH_KEY_PREFIX)?.parse().ok()
}

/// The byte rule the original's own colour readers apply to a stored colour
/// triple (`0x4bc350`): a channel above `1.0` is scaled `1/255`, at or below
/// `1.0` it is already a unit channel. Kept private so the test reads the
/// drawn buffer against the *stored* triples, not against the
/// implementation's own function.
fn engine_stored_channel(value: f32) -> f32 {
    if value > 1.0 { value / 255.0 } else { value }
}

/// `drawn`'s RGB equals `stored` under the byte rule, within a float's
/// noise floor.
fn same_colour(drawn: [f32; 4], stored: [f32; 3]) -> bool {
    (0..3).all(|channel| (drawn[channel] - engine_stored_channel(stored[channel])).abs() < 1e-5)
}

/// **M01's nearest island tile draws every stored material group as a
/// textured, vertex-coloured, unlit, fogged part.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_island_shading_retail_tile_binds_texture_colour_and_shading() {
    let root = game_dir();
    let sources = read_world_sources(&root, "C3").expect("C3's world sources read");
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();

    // The subject resolves to the measured stored mesh slot. The world's
    // mesh keys spell the escaped container path plus the stored index, so
    // the object is found by that index.
    let mut candidates: Vec<&WorldObjectId> = stage
        .world
        .objects()
        .iter()
        .filter(|object| match object.mesh() {
            Resolved::Known(known) => mesh_slot_of(known.value.key()) == Some(ISLAND_MESH_SLOT),
            _ => false,
        })
        .map(|object| object.id())
        .collect();
    candidates.sort_by_key(|id| id.as_str().to_string());
    let [id] = candidates.as_slice() else {
        panic!(
            "mesh slot {ISLAND_MESH_SLOT} resolves to exactly one M01 world object, found {}: {:?}",
            candidates.len(),
            candidates.iter().map(|id| id.as_str()).collect::<Vec<_>>()
        );
    };
    let id = (*id).clone();
    let object = stage.world.object(&id).expect("the object is there");
    let Resolved::Known(mesh) = object.mesh() else {
        panic!("{} names a stored mesh", id.as_str());
    };
    let gamez = sources
        .world()
        .meshes()
        .get(ISLAND_MESH_SLOT)
        .expect("the stored mesh reads");

    // Every drawn part is a material-bound child of the island node — the
    // whole-mesh entity itself stays material-less for the collider.
    let drawn_count = app
        .world()
        .resource::<MissionWorldParts>()
        .for_mesh(&mesh.value)
        .expect("the island tile has drawn parts")
        .len();
    let world = app.world_mut();
    let mut bindings = world.query::<(Entity, &WorldObjectBinding)>();
    let node = bindings
        .iter(world)
        .find(|(_, binding)| binding.object() == &id)
        .map(|(entity, _)| entity)
        .unwrap_or_else(|| panic!("{} is spawned", id.as_str()));
    let children = world
        .get::<Children>(node)
        .unwrap_or_else(|| panic!("{} has drawn children", id.as_str()));
    let bound = children
        .iter()
        .filter(|&&child| {
            world
                .get::<MeshMaterial3d<StandardMaterial>>(child)
                .is_some()
                && world
                    .get::<ChildOf>(child)
                    .is_some_and(|c| c.parent() == node)
        })
        .count();
    assert_eq!(
        bound,
        drawn_count,
        "every drawn part is a material-bound child of {}",
        id.as_str()
    );

    // The colours the production render mesh gives each stored material's
    // triangles — the upload's own input, so culled faces and compacted
    // vertices are already accounted for.
    let render = RenderMesh::from_stored_groups(&gamez.mesh, &gamez.material_groups)
        .expect("the stored mesh decodes through the production reader");
    let mut stored_materials: Vec<u32> =
        render.groups().iter().map(|group| group.material).collect();
    stored_materials.sort_unstable();
    stored_materials.dedup();
    let stored_colours = |material: u32| -> Vec<[f32; 3]> {
        let mut colours: Vec<[f32; 3]> = render
            .triangles()
            .iter()
            .filter(|triangle| triangle.material == material)
            .flat_map(|triangle| triangle.vertices)
            .filter_map(|vertex| render.vertices()[vertex as usize].color)
            .collect();
        colours.sort_by(|a, b| a.partial_cmp(b).expect("stored colours are finite"));
        colours.dedup();
        colours
    };

    let drawn = app
        .world()
        .resource::<MissionWorldParts>()
        .for_mesh(&mesh.value)
        .expect("the island tile has drawn parts");
    for material in &stored_materials {
        assert!(
            drawn.iter().any(|part| part.stored_material == *material),
            "stored material {material} has a drawn part"
        );
    }

    let meshes = app.world().resource::<Assets<Mesh>>();
    let materials = app.world().resource::<Assets<StandardMaterial>>();
    let stored = sources.world().materials();
    let mut bound_textures = Vec::new();
    let mut drawn_colours: std::collections::BTreeMap<u32, Vec<[f32; 4]>> =
        std::collections::BTreeMap::new();
    for (index, part) in drawn.iter().enumerate() {
        // The material binding: the stored index still names a material
        // record, and the record's texture is the one bound.
        let stored_material = stored
            .material(part.stored_material)
            .unwrap_or_else(|| panic!("part {index}'s stored material exists"));
        let material = materials
            .get(&part.material)
            .unwrap_or_else(|| panic!("part {index}'s material asset exists"));

        // The texture binding: a stored material that names a texture draws
        // with an image, and the island's terrain names are among them.
        if let Some(texture) = stored.texture_of(stored_material) {
            assert!(
                part.image.is_some() && material.base_color_texture.is_some(),
                "part {index} (material {}, texture {}) binds its image",
                part.stored_material,
                texture.name
            );
            bound_textures.push(texture.name.as_str());
        }
        // The lighting binding: a vertex-coloured world part draws
        // texture × stored colour — `unlit`, with the weather's fog still on.
        assert!(
            material.unlit,
            "part {index} (material {}) draws its stored shading unlit",
            part.stored_material
        );
        assert!(
            material.fog_enabled,
            "part {index} keeps the weather's fog binding",
        );
        assert_eq!(material.metallic, 0.0);
        assert_eq!(material.perceptual_roughness, 1.0);
        assert!(material.cull_mode.is_none());

        // The stored shading: the drawn corner colours are exactly the
        // colours the production render mesh gives this material's
        // triangles, byte-rule normalized, one per drawn vertex.
        let mesh_asset = meshes
            .get(&part.mesh)
            .unwrap_or_else(|| panic!("part {index}'s mesh asset exists"));
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh_asset.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("part {index} carries positions");
        };
        let Some(VertexAttributeValues::Float32x4(colours)) =
            mesh_asset.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("part {index} carries the stored corner colours");
        };
        assert_eq!(
            colours.len(),
            positions.len(),
            "one colour per drawn vertex"
        );
        let stored_colours = stored_colours(part.stored_material);
        assert!(
            !stored_colours.is_empty(),
            "material {}'s stored polygons colour their corners",
            part.stored_material
        );
        for (vertex, drawn) in colours.iter().enumerate() {
            assert_eq!(drawn[3], 1.0, "the upload wrote an opaque colour");
            assert!(
                stored_colours
                    .iter()
                    .any(|stored| same_colour(*drawn, *stored)),
                "part {index} vertex {vertex} drew {:?}, none of material {}'s stored colours",
                drawn,
                part.stored_material
            );
        }
        drawn_colours
            .entry(part.stored_material)
            .or_default()
            .extend(colours.iter().copied());
    }

    // Nothing stored is lost: every stored colour of a material's triangles
    // reaches the material's drawn parts — unioned, because a partial-normal
    // group splits its colours across a normal-bearing and a normal-free
    // part.
    for (material, union) in &drawn_colours {
        for stored in stored_colours(*material) {
            assert!(
                union.iter().any(|drawn| same_colour(*drawn, stored)),
                "material {material}'s stored colour {stored:?} reached its drawn parts"
            );
        }
    }

    // The island's own terrain textures made it onto the tile.
    for name in ["forest1", "cliff1", "sand128"] {
        assert!(
            bound_textures.iter().any(|bound| bound.contains(name)),
            "the island tile binds a {name} texture; bound {bound_textures:?}"
        );
    }
}
