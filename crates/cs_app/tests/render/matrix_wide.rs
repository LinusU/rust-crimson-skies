//! `accept_f17_e_` tests for the widened comparison matrix and its textured
//! captures: `cs_app::render::matrix::resolve_wide`,
//! `cs_app::playtest_retail::read_all_playtest_sources` and
//! `cs_app::world::textured_capture`
//! (`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`,
//! stages `### F17-D` and `### F17-E`, AC04; Rally #736).
//!
//! The fast tests pin the *widened* contract: every world group offered is one
//! matrix entry, in offered order, with all five subject rows; a group whose
//! container or the shared airframe container could not be read keeps its
//! place in the set with the reader's refusal on the affected rows — a group
//! is never silently skipped. They also pin the textured half's core rule: a
//! stored material that names a texture and did not get one bound is the
//! named refusal `missing_texture`, never a neutral stand-in.
//!
//! The `#[ignore]`d tests are the stage's `retail` and `gpu` halves: they read
//! **every** discovered world group of the owner's installation through the
//! production readers, resolve all five subjects per group, and draw each
//! resolved world-side subject with the group's own texture archive through
//! the production binder. They are ignored in CI (no original data there) and
//! **fail loudly** without `CS_GAME_DIR`.

use std::path::{Path, PathBuf};

use bevy::asset::Handle;
use bevy::prelude::{Mesh, StandardMaterial};
use cs_app::playtest_textures::{PlaytestPart, UnresolvedMaterial};
use cs_app::render::capture::{
    COMPARISON_EXPOSURE, COMPARISON_GAMMA, COMPARISON_MSAA_SAMPLES, ComparisonSettings, Tonemap,
};
use cs_app::render::matrix::{
    COCKPIT_ANCHOR, ComparisonSubject, MATRIX_WORLD_GROUP, MatrixContainer, SubjectSide,
    WidenedMatrix, WorldGroupSource, resolve_wide,
};
use cs_app::world::textured_capture::{
    MissingTexture, PartOutcome, TexturedCaptureError, missing_textures, part_outcomes,
};
use cs_formats::gamez::nodes::{NODE_TYPE_OBJECT3D, NODE_TYPE_WORLD, NodeKind, RawObject3dData};
use cs_formats::gamez::reader::Fixup;
use cs_formats::gamez::{
    GameZHeader, GameZMaterials, GameZMesh, GameZMeshes, GameZNodes, GameZTextureName,
    MATERIAL_FLAG_TEXTURED, MaterialInfo, MeshIndex, PrimitiveKind, RawCorner, RawMaterial,
    RawMaterialGroup, RawMaterialRecord, RawMesh, RawMeshInfo, RawMeshMaterialInfo, RawNode,
    RawNodeInfo, RawPolygon, RawWorldData, TextureNameEncoding,
};

// ------------------------------------------------------------- synthetic ---

/// One stored node of a fixture container.
struct Node {
    name: &'static str,
    parent: Option<u32>,
    children: Vec<u32>,
    mesh_index: i32,
    world: bool,
}

/// A decoded container's three sections, owned so a [`MatrixContainer`] can
/// borrow them for a whole `resolve_wide` run.
struct Fixture {
    nodes: GameZNodes,
    meshes: GameZMeshes,
    materials: GameZMaterials,
}

impl Fixture {
    fn container<'a>(&'a self, key: &'a str) -> MatrixContainer<'a> {
        MatrixContainer::new(key, &self.nodes, &self.meshes, &self.materials)
    }
}

fn node(spec: &Node, index: u32) -> RawNode {
    RawNode {
        index,
        name: spec.name.to_owned(),
        node_index: 0x0200_0000 | index,
        info: node_info(spec),
        kind: if spec.world {
            NodeKind::World(RawWorldData {
                partition_x_count: 0,
                partition_y_count: 0,
                partition_bytes: 0,
                partition_values: 0,
            })
        } else {
            NodeKind::Object3d(RawObject3dData {
                flags: 0,
                rotation: [0.0; 3],
                scale: [1.0; 3],
                matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                translation: [0.0; 3],
            })
        },
        data_offset: 1,
        data_bytes: 148,
        parent: spec.parent,
        children: spec.children.clone(),
    }
}

fn node_info(spec: &Node) -> RawNodeInfo {
    RawNodeInfo {
        node_type: if spec.world {
            NODE_TYPE_WORLD
        } else {
            NODE_TYPE_OBJECT3D
        },
        data_ptr: 1,
        mesh_index: spec.mesh_index,
        parent_count: u16::from(spec.parent.is_some()),
        children_count: spec.children.len() as u16,
        ..zero_node_info()
    }
}

fn zero_node_info() -> RawNodeInfo {
    RawNodeInfo {
        flags: 0,
        unk040: 0,
        unk044: 0,
        zone_id: 0,
        node_type: 0,
        data_ptr: 0,
        mesh_index: -1,
        environment_data: 0,
        action_priority: 1,
        action_callback: 0,
        area_partition: [0; 4],
        parent_count: 0,
        children_count: 0,
        parent_array_ptr: 0,
        children_array_ptr: 0,
        unk096: 0,
        unk100: 0,
        unk104: 0,
        unk108: 0,
        unk112: 0,
        unk116: [[0.0; 3]; 2],
        unk140: [[0.0; 3]; 2],
        unk164: [[0.0; 3]; 2],
        unk188: 0,
        unk192: 0,
        unk196: 0,
        unk200: 0,
        unk204: 0,
    }
}

fn nodes(specs: &[Node]) -> GameZNodes {
    GameZNodes {
        header: header(specs.len() as u32),
        nodes: specs
            .iter()
            .enumerate()
            .map(|(index, spec)| node(spec, index as u32))
            .collect(),
        info_offset: 0,
        info_end: 0,
        data_offset: 0,
        data_end: 0,
        findings: Vec::new(),
    }
}

fn header(node_array_size: u32) -> GameZHeader {
    GameZHeader {
        signature: cs_formats::zbd::GAMEZ_SIGNATURE,
        version: cs_formats::zbd::GAMEZ_VERSION,
        unk08: 0,
        texture_count: 0,
        textures_offset: cs_formats::gamez::GAMEZ_HEADER_BYTES as u32,
        materials_offset: 0,
        meshes_offset: 0,
        node_array_size,
        light_index: 0,
        nodes_offset: 0,
    }
}

/// A one-triangle stored mesh binding material `material_index`.
fn mesh(index: u32, material_index: u32) -> GameZMesh {
    let uv = [0.0, 0.0];
    GameZMesh {
        index,
        info: RawMeshInfo {
            file_ptr: 1,
            unk04: 0,
            unk08: 0,
            parent_count: 1,
            polygon_count: 1,
            vertex_count: 3,
            normal_count: 0,
            morph_count: 0,
            light_count: 0,
            unk36: 0,
            unk40: 0.0,
            unk44: 0.0,
            unk48: 0,
            polygons_ptr: 1,
            vertices_ptr: 1,
            normals_ptr: 0,
            lights_ptr: 0,
            morphs_ptr: 0,
            unk72: 0.0,
            unk76: 0.0,
            unk80: 0.0,
            unk84: 0.0,
            unk88: 0,
            material_count: 1,
            materials_ptr: 1,
        },
        mesh: RawMesh {
            positions: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]],
            normals: Vec::new(),
            polygons: vec![RawPolygon {
                kind: PrimitiveKind::Polygon,
                raw_flags: 0,
                material: material_index,
                corners: (0..3_u32)
                    .map(|position| RawCorner {
                        position,
                        normal: None,
                        uv: Some(uv),
                        color: None,
                    })
                    .collect(),
            }],
        },
        polygon_records: Vec::new(),
        lights: Vec::new(),
        morphs: Vec::new(),
        materials: vec![RawMeshMaterialInfo {
            material_index,
            polygon_usage_count: 1,
            unk_ptr: 0,
        }],
        material_groups: vec![vec![RawMaterialGroup {
            material: material_index,
            uvs: vec![uv, uv, uv],
        }]],
        data_offset: 0,
        data_end: 0,
    }
}

fn meshes(count: u32) -> GameZMeshes {
    GameZMeshes {
        header: header(0),
        index: MeshIndex {
            array_size: count as i32,
            count: count as i32,
            last_index: count as i32,
        },
        fixup: Fixup::None,
        meshes: (0..count).map(|index| Some(mesh(index, 0))).collect(),
        findings: Vec::new(),
        unchecked_material_references: 0,
        data_offset: 0,
        data_end: 0,
    }
}

/// A stored texture-name table entry.
fn texture_name(index: u32, name: &str) -> GameZTextureName {
    GameZTextureName {
        index,
        name: name.to_owned(),
        stem: name.to_owned(),
        suffix: None,
        encoding: TextureNameEncoding::StemOnly,
        field00: 0,
        field32: 2,
        field36: 0,
        field40: -1,
    }
}

/// A present textured material record naming `texture_index`.
fn textured_material(index: u32, texture_index: u32) -> RawMaterial {
    RawMaterial {
        index,
        record: RawMaterialRecord {
            alpha: 0xFF,
            flags: MATERIAL_FLAG_TEXTURED,
            rgb: 0x7FFF,
            color: [255.0; 3],
            texture_index,
            field20: 0.0,
            field24: 0.5,
            field28: 0.5,
            field32: 0.0,
            cycle_ptr: 0,
        },
        link1: 0,
        link2: 0,
        cycle: None,
    }
}

/// A present flat-colour material record: the textured flag is clear.
fn colored_material(index: u32) -> RawMaterial {
    RawMaterial {
        index,
        record: RawMaterialRecord {
            alpha: 0x00,
            flags: 0,
            rgb: 0x0000,
            color: [0.0; 3],
            texture_index: 0,
            field20: 0.0,
            field24: 0.5,
            field28: 0.5,
            field32: 0.0,
            cycle_ptr: 0,
        },
        link1: 0,
        link2: 0,
        cycle: None,
    }
}

fn materials(records: Vec<RawMaterial>, textures: Vec<GameZTextureName>) -> GameZMaterials {
    let count = records.len() as i32;
    GameZMaterials {
        header: header(0),
        textures,
        info: MaterialInfo {
            array_size: count,
            count,
            index_max: count,
            index_last: count - 1,
        },
        materials: records,
        free_slots: 0,
        findings: Vec::new(),
        textures_offset: 0,
        materials_offset: 0,
        data_end: 0,
    }
}

/// A world container holding every anchor the matrix's world side looks for:
/// `horizon`, one repeated instanced family (whose name the caller chooses),
/// and the `moon`/`stars` pair.
fn world_fixture(family: &'static str) -> Fixture {
    Fixture {
        nodes: nodes(&[
            Node {
                name: "world1",
                parent: None,
                children: vec![1, 2, 3, 4, 5],
                mesh_index: -1,
                world: true,
            },
            Node {
                name: "horizon",
                parent: Some(0),
                children: Vec::new(),
                mesh_index: 0,
                world: false,
            },
            Node {
                name: family,
                parent: Some(0),
                children: Vec::new(),
                mesh_index: 1,
                world: false,
            },
            Node {
                name: family,
                parent: Some(0),
                children: Vec::new(),
                mesh_index: -1,
                world: false,
            },
            Node {
                name: "moon",
                parent: Some(0),
                children: Vec::new(),
                mesh_index: 2,
                world: false,
            },
            Node {
                name: "stars",
                parent: Some(0),
                children: Vec::new(),
                mesh_index: -1,
                world: false,
            },
        ]),
        meshes: meshes(3),
        materials: materials(
            vec![textured_material(0, 0)],
            vec![texture_name(0, "DOME.TIF")],
        ),
    }
}

/// The shared airframe container: `bloodhawk` carrying `healthy` and the
/// `cockpit1` subtree the cockpit subject anchors to.
fn aircraft_fixture() -> Fixture {
    Fixture {
        nodes: nodes(&[
            Node {
                name: "bloodhawk",
                parent: None,
                children: vec![1, 2],
                mesh_index: -1,
                world: false,
            },
            Node {
                name: "healthy",
                parent: Some(0),
                children: vec![3],
                mesh_index: -1,
                world: false,
            },
            Node {
                name: COCKPIT_ANCHOR,
                parent: Some(0),
                children: vec![4],
                mesh_index: -1,
                world: false,
            },
            Node {
                name: "fuselage",
                parent: Some(1),
                children: Vec::new(),
                mesh_index: 0,
                world: false,
            },
            Node {
                name: "panel",
                parent: Some(2),
                children: Vec::new(),
                mesh_index: 1,
                world: false,
            },
        ]),
        meshes: meshes(2),
        materials: materials(
            vec![textured_material(0, 0)],
            vec![texture_name(0, "BHAWK.TIF")],
        ),
    }
}

/// The subject codes of one group matrix's rows, in row order.
fn row_subjects(entry: &cs_app::render::matrix::GroupMatrix) -> Vec<&'static str> {
    entry
        .matrix
        .rows()
        .iter()
        .map(|row| row.subject().code())
        .collect()
}

/// Every subject the set names, in the sheet's order.
const SUBJECT_ORDER: [&str; 5] = [
    "cockpit",
    "skyline",
    "vegetation",
    "night_effects",
    "close_up_aircraft",
];

/// Every world group the matrix is offered is one [`GroupMatrix`] of the
/// result — in offered order, all five subject rows — and a group the reader
/// refused keeps its place with the refusal on its world-side rows.
///
/// This is the widened selection's contract: if the widening were removed and
/// the set collapsed back to one pinned group, `groups.len()` would be one
/// and this test fails; if a refused container were dropped instead of
/// carried, the third entry would not be here.
#[test]
fn accept_f17_e_every_offered_group_is_a_matrix_entry_in_offered_order() {
    let c1c = world_fixture("gpalm");
    let c2 = world_fixture("gtree");
    let aircraft = aircraft_fixture();

    let wide = resolve_wide(
        Ok(aircraft.container("zbd/planes.zbd")),
        vec![
            WorldGroupSource {
                group: "C1C".to_owned(),
                container: Ok(c1c.container("zbd/c1c/gamez.zbd")),
            },
            WorldGroupSource {
                group: "C2".to_owned(),
                container: Ok(c2.container("zbd/c2/gamez.zbd")),
            },
            WorldGroupSource {
                group: "C4".to_owned(),
                container: Err("fixture: c4's gamez.zbd would not decode".to_owned()),
            },
        ],
    );

    let names: Vec<&str> = wide
        .groups
        .iter()
        .map(|entry| entry.group.as_str())
        .collect();
    assert_eq!(
        names,
        ["C1C", "C2", "C4"],
        "every offered group is an entry, in offered order"
    );
    for entry in &wide.groups {
        assert_eq!(
            row_subjects(entry),
            SUBJECT_ORDER,
            "{}: the set still names all five subjects, in the sheet's order",
            entry.group
        );
    }
    assert!(
        wide.group("c1c").is_some(),
        "lookup is by the group's directory name, case-insensitive"
    );

    let first = wide.group("C1C").expect("offered");
    assert!(
        first.matrix.is_fully_resolved(),
        "a group holding every anchor resolves all five subjects; refusals: {:?}",
        first
            .matrix
            .rows()
            .iter()
            .filter_map(|row| row.reason().map(str::to_owned))
            .collect::<Vec<_>>()
    );

    let refused = wide.group("C4").expect("a refused group is still an entry");
    assert_eq!(
        refused.container_error.as_deref(),
        Some("fixture: c4's gamez.zbd would not decode"),
        "the entry carries the reader's refusal verbatim"
    );
    for subject in ComparisonSubject::ALL {
        let row = refused.matrix.row(subject).expect("every subject is a row");
        match subject.side() {
            SubjectSide::World => {
                let reason = row
                    .reason()
                    .expect("a refused container leaves its world rows unresolved");
                assert!(
                    reason.contains("zbd/c4/gamez.zbd") && reason.contains("would not decode"),
                    "{subject}: the refusal names the group's container and the reader's \
                     reason: {reason}"
                );
            }
            SubjectSide::Aircraft => assert!(
                row.resolved().is_some(),
                "{subject}: the shared airframe container still resolved — one side's \
                 refusal never masks the other"
            ),
        }
    }
    assert_eq!(
        wide.groups_with_world_subjects(),
        2,
        "two of the three offered groups resolved world-side subjects"
    );
}

/// The shared airframe container is one `Result` too: when it refuses, every
/// group's aircraft rows carry the named refusal while the world side still
/// resolves its own anchors.
#[test]
fn accept_f17_e_a_refused_airframe_container_is_reported_on_every_group() {
    let c1c = world_fixture("gpalm");
    let wide = resolve_wide(
        Err("zbd/planes.zbd: fixture read failure".to_owned()),
        vec![WorldGroupSource {
            group: "C1C".to_owned(),
            container: Ok(c1c.container("zbd/c1c/gamez.zbd")),
        }],
    );

    let entry = wide.group("C1C").expect("the group is an entry");
    for subject in ComparisonSubject::ALL {
        let row = entry.matrix.row(subject).expect("every subject is a row");
        match subject.side() {
            SubjectSide::Aircraft => {
                let reason = row
                    .reason()
                    .expect("a refused airframe leaves the aircraft rows unresolved");
                assert!(
                    reason.contains("zbd/planes.zbd") && reason.contains("fixture read failure"),
                    "{subject}: the refusal names the shared container and its reason: {reason}"
                );
            }
            SubjectSide::World => assert!(
                row.resolved().is_some(),
                "{subject}: the world side still resolved on its own container"
            ),
        }
    }
}

/// The widened matrix resolves **each** group against its own stored forest:
/// two containers that store different instanced family names produce
/// different anchors, so a per-group answer is re-derived rather than one
/// pinned group's answer being repeated.
#[test]
fn accept_f17_e_each_group_resolves_against_its_own_stored_forest() {
    let palms = world_fixture("gpalm");
    let trees = world_fixture("gtree");
    let aircraft = aircraft_fixture();

    let wide = resolve_wide(
        Ok(aircraft.container("zbd/planes.zbd")),
        vec![
            WorldGroupSource {
                group: "C1C".to_owned(),
                container: Ok(palms.container("zbd/c1c/gamez.zbd")),
            },
            WorldGroupSource {
                group: "C2".to_owned(),
                container: Ok(trees.container("zbd/c2/gamez.zbd")),
            },
        ],
    );

    let vegetation_of = |group: &str| {
        wide.group(group)
            .and_then(|entry| entry.matrix.row(ComparisonSubject::Vegetation))
            .and_then(|row| row.resolved())
            .map(|resolved| resolved.anchor_name.clone())
            .unwrap_or_else(|| panic!("{group}'s vegetation subject must resolve"))
    };
    assert_eq!(vegetation_of("C1C"), "gpalm");
    assert_eq!(vegetation_of("C2"), "gtree");

    // And a group whose stored forest holds no instanced family reports the
    // named refusal rather than borrowing another group's answer.
    let bare = world_fixture("gpalm");
    let aircraft = aircraft_fixture();
    let mut bare_nodes = bare.nodes.clone();
    bare_nodes.nodes[3].name = "unique".to_owned();
    let bare = Fixture {
        nodes: bare_nodes,
        meshes: bare.meshes,
        materials: bare.materials,
    };
    let wide = resolve_wide(
        Ok(aircraft.container("zbd/planes.zbd")),
        vec![WorldGroupSource {
            group: "C3".to_owned(),
            container: Ok(bare.container("zbd/c3/gamez.zbd")),
        }],
    );
    let entry = wide.group("C3").expect("the group is an entry");
    let row = entry
        .matrix
        .row(ComparisonSubject::Vegetation)
        .expect("the subject is a row");
    let reason = row
        .reason()
        .expect("no repeated child name means the subject stays in the set with its refusal");
    assert!(
        reason.contains("no instanced vegetation family") && reason.contains("zbd/c3/gamez.zbd"),
        "the refusal names the rule and the container: {reason}"
    );
    assert!(
        row.resolved().is_none(),
        "the refused subject is reported, not drawn with borrowed content"
    );
}

/// A stored material that names a texture and did not get one bound is the
/// named refusal `missing_texture` — the capture's core contract. The
/// classifier reports every part: textured, flat, missing-by-binder,
/// missing-with-no-record and the unsliceable sentinel, so a bound image can
/// never be swapped for a neutral stand-in without this test seeing it.
#[test]
fn accept_f17_e_a_missing_texture_is_a_refusal_never_a_fallback() {
    let table = materials(
        vec![
            textured_material(0, 0),
            colored_material(1),
            textured_material(2, 1),
        ],
        vec![texture_name(0, "DOME.TIF"), texture_name(1, "PALM.TGA")],
    );
    let part = |stored_material: u32, image: Option<&str>| PlaytestPart {
        mesh: Handle::<Mesh>::default(),
        material: Handle::<StandardMaterial>::default(),
        stored_material,
        triangles: 1,
        image: image.map(str::to_owned),
    };
    let parts = vec![
        part(0, Some("zbd/c1c/dome.tga#0")),
        part(1, None),
        part(2, None),
        part(u32::MAX, None),
    ];
    let unresolved = vec![UnresolvedMaterial {
        source_id: "zbd/c2/gamez.zbd#material-2".to_owned(),
        texture: Some("PALM.TGA".to_owned()),
        reason: "texture_not_resolved",
        meshes: 1,
    }];

    let outcomes = part_outcomes(&parts, &table, &unresolved);
    assert_eq!(outcomes.len(), parts.len());
    assert_eq!(
        outcomes[0],
        PartOutcome::Textured {
            image: "zbd/c1c/dome.tga#0".to_owned()
        },
        "a part with a bound image is the textured outcome"
    );
    assert_eq!(
        outcomes[1],
        PartOutcome::Flat,
        "a stored material that names no texture draws flat by declaration"
    );
    assert_eq!(
        outcomes[2],
        PartOutcome::MissingTexture(MissingTexture {
            stored_material: 2,
            texture: Some("PALM.TGA".to_owned()),
            reason: "texture_not_resolved".to_owned(),
        }),
        "the binder's refusal is carried with the texture's own name"
    );
    assert_eq!(
        outcomes[3],
        PartOutcome::Unsliceable,
        "the sentinel material is reported, never attributed"
    );

    let missing = missing_textures(&outcomes);
    assert_eq!(
        missing,
        vec![MissingTexture {
            stored_material: 2,
            texture: Some("PALM.TGA".to_owned()),
            reason: "texture_not_resolved".to_owned(),
        }],
        "exactly one part is missing its texture"
    );

    // The refusal is the record a `missing_texture` error carries — including
    // the case where the binder recorded nothing, e.g. the group stored no UV
    // to sample the named texture with.
    let silent = part_outcomes(&[part(2, None)], &table, &[]);
    assert_eq!(
        missing_textures(&silent)[0].reason,
        "texture_not_bound",
        "a named texture with no bound image and no binder record is still a refusal"
    );

    let error = TexturedCaptureError::MissingTexture {
        group: "C2".to_owned(),
        subject: "skyline".to_owned(),
        mesh_index: 7,
        missing,
    };
    assert_eq!(error.code(), "missing_texture");
    let message = error.to_string();
    for needle in ["C2", "skyline", "PALM.TGA", "texture_not_resolved"] {
        assert!(
            message.contains(needle),
            "the refusal names {needle:?}: {message}"
        );
    }
}

/// The textured capture's settings record is the fixed comparison set of spec
/// F17 non-negotiable 3 — and only that set qualifies: an exposure, tone
/// curve, gamma, sample count, shadow or resolution change is a different
/// presentation, not a comparison frame.
#[test]
fn accept_f17_e_the_capture_settings_are_the_fixed_comparison_set() {
    let settings = ComparisonSettings::comparison();
    assert!(settings.is_fixed());
    assert_eq!(settings.exposure(), COMPARISON_EXPOSURE);
    assert_eq!(settings.tonemap(), Tonemap::None);
    assert_eq!(settings.tonemap().code(), "none");
    assert_eq!(settings.gamma(), COMPARISON_GAMMA);
    assert_eq!(settings.msaa_samples(), COMPARISON_MSAA_SAMPLES);
    assert!(!settings.shadows());
    assert_eq!(settings.render_resolution(), None);

    for changed in [
        ComparisonSettings::with_exposure(0.5),
        ComparisonSettings::with_msaa_samples(4),
        ComparisonSettings::with_shadows(true),
        ComparisonSettings::for_presentation(Tonemap::Filmic, 4, true, None),
    ] {
        assert!(
            !changed.is_fixed(),
            "a presentation tweak is not the comparison set: {changed:?}"
        );
    }
}

// ---------------------------------------------------------------- retail ---

/// The workspace root, because cargo runs a test binary from the *package*
/// root.
fn workspace_path(relative: &str) -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .join(relative)
}

/// Reads the aircraft container and **every** discovered world group through
/// the production reader, and hands the widened matrix — plus the source
/// records, which still carry each group's container and archive outcomes —
/// to `f`.
fn with_wide_matrix<R>(
    f: impl FnOnce(&cs_app::playtest_retail::PlaytestAllSources, &WidenedMatrix) -> R,
) -> R {
    let game_dir = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set for the retail half"),
    );
    let sources = cs_app::playtest_retail::read_all_playtest_sources(&game_dir)
        .expect("production discovery must read the installation");
    let aircraft = sources
        .aircraft
        .as_ref()
        .map(|container| {
            MatrixContainer::new(
                container.container_key(),
                container.nodes(),
                container.meshes(),
                container.materials(),
            )
        })
        .map_err(|error| error.to_string());
    let groups: Vec<WorldGroupSource> = sources
        .groups
        .iter()
        .map(|source| WorldGroupSource {
            group: source.group.clone(),
            container: source
                .container
                .as_ref()
                .map(|container| {
                    MatrixContainer::new(
                        container.container_key(),
                        container.nodes(),
                        container.meshes(),
                        container.materials(),
                    )
                })
                .map_err(|error| error.to_string()),
        })
        .collect();
    let wide = resolve_wide(aircraft, groups);
    f(&sources, &wide)
}

/// The widening's acceptance scenario, measured: discovery's world-group list
/// — the eight directories the owner's installation stores — is exactly the
/// matrix's entry list, every entry carries all five subject rows, and every
/// row is either a resolved mesh or a named refusal. The pinned F17-D group
/// must still resolve in full: it is the baseline the widening was measured
/// against.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f17_e_retail_every_discovered_world_group_is_a_matrix_entry() {
    with_wide_matrix(|sources, wide| {
        let expected: Vec<String> = ["c1", "c1b", "c1c", "c2", "c2b", "c3", "c4", "c5"]
            .iter()
            .map(|group| (*group).to_owned())
            .collect();
        let discovered: Vec<String> = sources
            .groups
            .iter()
            .map(|source| source.group.to_ascii_lowercase())
            .collect();
        assert_eq!(
            discovered, expected,
            "the owner installation's world groups, in discovery order"
        );

        assert_eq!(
            wide.groups.len(),
            sources.groups.len(),
            "the matrix is exactly the discovered group list — nothing skipped"
        );
        for (source, entry) in sources.groups.iter().zip(&wide.groups) {
            assert_eq!(entry.group, source.group, "entries keep discovery order");
            assert_eq!(
                entry.matrix.rows().len(),
                ComparisonSubject::ALL.len(),
                "{}: all five subjects are rows",
                source.group
            );
            for subject in ComparisonSubject::ALL {
                let row = entry
                    .matrix
                    .row(subject)
                    .unwrap_or_else(|| panic!("{} has no {subject} row", source.group));
                assert!(
                    row.resolved().is_some() || row.reason().is_some_and(|r| !r.is_empty()),
                    "{}/{} is neither resolved nor a named refusal",
                    source.group,
                    subject.code()
                );
            }
        }

        let baseline = wide
            .group(MATRIX_WORLD_GROUP)
            .expect("the pinned F17-D group is one entry");
        assert!(
            baseline.matrix.is_fully_resolved(),
            "{MATRIX_WORLD_GROUP} is the measured baseline — all five subjects resolve; \
             refusals: {:?}",
            baseline
                .matrix
                .rows()
                .iter()
                .filter_map(|row| row.reason().map(str::to_owned))
                .collect::<Vec<_>>()
        );

        eprintln!("F17-E retail matrix ({} groups):", wide.groups.len());
        for (source, entry) in sources.groups.iter().zip(&wide.groups) {
            let archive = match &source.textures {
                Ok(_) => "archive opened".to_owned(),
                Err(error) => format!("archive refused: {error}"),
            };
            let rows: Vec<String> = entry
                .matrix
                .rows()
                .iter()
                .map(|row| {
                    if let Some(resolved) = row.resolved() {
                        format!(
                            "{}={} ({} tri)",
                            row.subject().code(),
                            resolved.chosen.name,
                            resolved.triangles
                        )
                    } else {
                        format!("{}=refused", row.subject().code())
                    }
                })
                .collect();
            eprintln!("  {} [{}]: {}", source.group, archive, rows.join(", "));
        }
    });
}

/// The `gpu` half: every resolved world-side subject of every group is drawn
/// with the materials the group's **own** texture archive binds through the
/// production `TextureBinder`/`WorldMeshes` path — or the capture is the
/// named refusal it produced, with no PNG left behind.
///
/// At least one frame must come back textured (a bound `image` on a part) —
/// that is the task's "a textured capture binds the resolved image through
/// the production upload path" — and every refusal must be a named
/// `TexturedCaptureError`, never a silent neutral fallback.
#[test]
#[ignore = "requires CS_GAME_DIR and a GPU adapter"]
fn accept_f17_e_gpu_every_resolved_world_subject_is_captured_or_refused_by_name() {
    use cs_app::world::textured_capture::{TexturedCaptureRequest, capture_subject_textured};

    let evidence_dir = workspace_path("private/evidence/F17-E-MATRIX-COVERAGE");
    std::fs::create_dir_all(&evidence_dir).expect("private/evidence is writable");

    with_wide_matrix(|sources, wide| {
        let settings = ComparisonSettings::comparison();
        let mut captured = 0_usize;
        let mut textured_frames = 0_usize;
        let mut refusals: Vec<String> = Vec::new();
        for (source, entry) in sources.groups.iter().zip(&wide.groups) {
            let group = source.group.to_ascii_lowercase();
            let Ok(container) = source.container.as_ref() else {
                refusals.push(format!(
                    "{group}: container refused — {}",
                    source
                        .container
                        .as_ref()
                        .err()
                        .map(|e| e.to_string())
                        .unwrap_or_default()
                ));
                continue;
            };
            let Ok(archive) = source.textures.as_ref() else {
                refusals.push(format!(
                    "{group}: texture archive refused — {}",
                    source
                        .textures
                        .as_ref()
                        .err()
                        .map(|e| e.to_string())
                        .unwrap_or_default()
                ));
                continue;
            };
            for row in entry.matrix.rows() {
                if row.subject().side() != SubjectSide::World {
                    continue;
                }
                let Some(resolved) = row.resolved() else {
                    refusals.push(format!(
                        "{group} {}: {}",
                        row.subject().code(),
                        row.reason().unwrap_or("unresolved")
                    ));
                    continue;
                };
                let png =
                    evidence_dir.join(format!("textured-{group}-{}.png", row.subject().code()));
                if png.exists() {
                    std::fs::remove_file(&png).expect("an old capture can be replaced");
                }
                match capture_subject_textured(&TexturedCaptureRequest {
                    group: &source.group,
                    subject: row.subject().code(),
                    container,
                    mesh_index: resolved.chosen.mesh_index,
                    render: &resolved.render,
                    unknowns: &resolved.unknowns,
                    archive,
                    png: &png,
                }) {
                    Ok(capture) => {
                        assert!(
                            capture.drew_geometry(),
                            "{group} {} came back as a blank frame: {capture:?}",
                            row.subject().code()
                        );
                        assert_eq!(
                            capture.mesh_index, resolved.chosen.mesh_index,
                            "the capture drew the mesh the matrix chose"
                        );
                        assert_eq!(
                            (
                                capture.exposure,
                                capture.tonemap,
                                capture.gamma,
                                capture.msaa_samples
                            ),
                            (
                                settings.exposure(),
                                settings.tonemap().code(),
                                settings.gamma(),
                                settings.msaa_samples()
                            ),
                            "{group} {} recorded settings are the fixed comparison set",
                            row.subject().code()
                        );
                        let written = std::fs::read(&png).expect("the PNG is on disk");
                        assert_eq!(
                            cs_assets::install::sha256(&written),
                            capture.png_sha256,
                            "{group} {}: the digest is of the file on disk",
                            row.subject().code()
                        );
                        if capture.textured_parts > 0 {
                            textured_frames += 1;
                        }
                        captured += 1;
                        eprintln!(
                            "  {} {}: {} of {} parts textured (images {:?}), {} per mille \
                             covered, adapter {}",
                            group,
                            row.subject().code(),
                            capture.textured_parts,
                            capture.parts,
                            capture.resolved_names,
                            capture.covered_permille,
                            capture.adapter,
                        );
                    }
                    Err(error) => {
                        assert!(
                            !png.exists(),
                            "{group} {} refused ({}) yet left a PNG behind",
                            row.subject().code(),
                            error.code()
                        );
                        refusals.push(format!(
                            "{group} {}: {} — {error}",
                            row.subject().code(),
                            error.code()
                        ));
                    }
                }
            }
        }
        eprintln!(
            "F17-E textured captures: {captured} drawn, {} refused:",
            refusals.len()
        );
        for refusal in &refusals {
            eprintln!("  refusal: {refusal}");
        }
        assert!(
            captured > 0,
            "the widened matrix produced no textured capture at all — the binding path was \
             not exercised: refusals {refusals:?}"
        );
        assert!(
            textured_frames > 0,
            "no capture bound an original texture through the production path — the textured \
             half of the task is unproven: {captured} flat-only frame(s)"
        );
    });
}
