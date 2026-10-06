//! #629 (`M01-LC-WORLD-IMPORT`): an original world container becomes a
//! `WorldDefinition`, and `spawn_world` runs on it.
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`
//! (`### F18-A`, `### F18-B`), plus the first-mission path in
//! `specs/README.md`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//! Task test prefix: `accept_m01_lc_world_import_`.
//!
//! These tests drive **production code only**. The conversion is
//! `cs_content::world::import_world_container` over the production readers'
//! output; the file access is `cs_app::world::read_world_container`; the spawn is
//! the production `cs_app::world::spawn_world`. No test here carries its own
//! importer, its own sector assignment or its own collision role.
//!
//! What is pinned:
//!
//! * **the world record's partition grid is the sector index.** A cell becomes a
//!   `Sector` whose extent is the union of the stored world-space bounding boxes
//!   its members state, and an object's sector list is exactly the cells that
//!   name it. A record the world node owns but the grid does not name stays
//!   **resident**, which is what the record says rather than an absence.
//! * **the role follows the index, and everything else is named.** An indexed
//!   record is `Solid` + `FromMesh`; an unindexed record that stores no
//!   collision geometry resolves to `None` (task #677's measured split); an
//!   unindexed record whose name carries the measured `fvol` prefix resolves
//!   to `None` as a fog volume (task #716); an unindexed record that stores
//!   geometry and carries no measured prefix carries an explicit unknown with
//!   a claim id, as does every gameplay surface and the world's boundary. Nothing is silently
//!   defaulted, so a consumer can enumerate the gaps instead of discovering
//!   them in flight.
//! * **the identity is the node slot, the mesh reference is the caller's table,
//!   and a record that stores no mesh index names that fact itself.** The
//!   identity is the `node-<slot>` key, the mesh reference is the caller's entry
//!   for the stored index, a mesh index the caller's table does not hold refuses
//!   the container, and the record that stores no mesh at all carries its own
//!   claim id rather than borrowing the identity's.
//! * **the refusal is the refusal.** A grid value that names a missing record,
//!   names one record twice, or names a non-object record stops the container
//!   with a typed error carrying the counts, and the ownership statements are
//!   cross-checked rather than one assumed.
//! * **`spawn_world` runs on the imported definition**, with the geometry the
//!   definition names uploaded through the production F17-B adapter.
//!
//! The retail half is `#[ignore]`d (`requires CS_GAME_DIR`): it asserts the
//! **measured** counts of `ZBD/C1C/gamez.zbd`, that every resolved value carries
//! the import claim at `ObservedTool` with the container's own source span, and
//! runs the spawn over the real container. It is read-only and commits no derived
//! bytes.

use std::collections::{BTreeMap, BTreeSet};

use avian3d::prelude::Collider;
use cs_app::world::{
    GRID_IS_THE_SECTOR_INDEX, MESH_SETTLE_UPDATES, RETAIL_WORLD_IMPORT, RetailWorldContainer,
    SkipReason, WorldMeshes, read_world_container, spawn_world, world_app,
};
use cs_content::coordinates::SourceAdapter;
use cs_content::mesh::{MeshPresentationUnknown, RenderMesh};
use cs_content::scene::MeshSlot;
use cs_content::textures::WorldTextureLoad;
use cs_content::world::{
    INDEXED_RECORD_IS_STATIC, ImportedWorld, OBJECT_ID_IS_THE_NODE_SLOT, OBJECT_STORES_NO_MESH,
    PARTITION_GRID_IS_THE_SECTOR_INDEX, UNINDEXED_RECORD_STORES_NO_GEOMETRY,
    UNINDEXED_ROLE_UNMEASURED, WORLD_BOUNDARY_UNMEASURED, WORLD_SURFACE_UNMEASURED,
    WorldImportError, WorldPartitionGrid, import_world_container,
};
use cs_formats::gamez::{PrimitiveKind, RawCorner, RawMesh, RawPolygon, read_gamez_nodes};
use cs_formats::io::ParseContext;
use cs_types::content::{ContentId, ContentKind, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus};

// ---------------------------------------------------------------- the fixture --

/// The fixture container's 40-byte header signature.
const SIGNATURE: u32 = 0x0297_1222;
/// The fixture container's header version word.
const VERSION: u32 = 42;
/// Where the fixture's node array starts, right after the header.
const NODES_OFFSET: u32 = 52;
/// Bytes of one node's info slot: the 36-byte name plus the 172-byte record,
/// then the 4-byte `node_index` word.
const SLOT: usize = 212;

/// The fixture's node slots.
pub(crate) const WORLD: u32 = 0;
pub(crate) const TILE_A: u32 = 1;
pub(crate) const SLAB: u32 = 2;
pub(crate) const TILE_B: u32 = 3;
pub(crate) const MARKER: u32 = 4;
/// An unindexed record that **does** carry geometry: the role the container
/// leaves unmeasured (task #677's split).
pub(crate) const VOLUME: u32 = 5;

/// The fixture's grid: two cells along the second axis, one along the first.
const GRID_X: u32 = 1;
const GRID_Y: u32 = 2;

/// The mesh slots the fixture's records name.
pub(crate) const MESH_TILE_A: i32 = 7;
pub(crate) const MESH_SLAB: i32 = 6;
pub(crate) const MESH_TILE_B: i32 = 8;
/// How many slots the fixture's mesh table holds.
const MESH_SLOTS: usize = 10;

/// One object record the fixture writes.
#[derive(Clone)]
pub(crate) struct ObjectSpec {
    pub(crate) name: &'static str,
    pub(crate) mesh: i32,
    /// The stored world-space bounding box's minimum, in stored units.
    box_min: [f32; 3],
    /// The stored world-space bounding box's maximum, in stored units.
    box_max: [f32; 3],
    /// Whether the record's `parent_count` boolean is set.
    has_parent: bool,
    /// The record's stored `unk196` word.
    unk196: u32,
}

impl ObjectSpec {
    pub(crate) const fn new(name: &'static str, mesh: i32) -> Self {
        Self {
            name,
            mesh,
            box_min: [0.0; 3],
            box_max: [0.0; 3],
            has_parent: true,
            unk196: 160,
        }
    }

    pub(crate) const fn extent(mut self, min: [f32; 3], max: [f32; 3]) -> Self {
        self.box_min = min;
        self.box_max = max;
        self
    }
}

/// How the fixture's world record is arranged.
pub(crate) struct Fixture {
    pub(crate) grid: Vec<Vec<u32>>,
    pub(crate) stored_children: Vec<u32>,
    pub(crate) objects: Vec<ObjectSpec>,
    /// An extra record that names the world node but is in neither the grid nor
    /// the stored child list, which is how the ownership cross-check is broken.
    pub(crate) unlisted_extra: bool,
}

impl Default for Fixture {
    fn default() -> Self {
        Self {
            // The first cell holds **two** records with extents that reach
            // past each other, so a sector extent built from one member's box
            // differs from the union of the boxes the store states.
            grid: vec![vec![TILE_A, SLAB], vec![TILE_B]],
            stored_children: vec![MARKER, VOLUME],
            objects: vec![
                ObjectSpec::new("tile_a", MESH_TILE_A)
                    .extent([-10.0, 0.0, -10.0], [-9.0, 0.0, -9.0]),
                ObjectSpec::new("slab", MESH_SLAB).extent([-9.0, 0.0, -12.0], [-8.0, 0.0, -10.0]),
                ObjectSpec::new("tile_b", MESH_TILE_B).extent([0.0, 0.0, 0.0], [1.0, 0.0, 1.0]),
                // A world-owned record with no extent and no mesh at all: the
                // measured anchor class, resolved to `None` (task #677).
                ObjectSpec::new("marker", -1),
                // A world-owned record with a mesh **and** an extent that the
                // grid does not name and whose name carries no measured prefix:
                // the class whose role the container leaves unmeasured. It is
                // spelled `volume`, deliberately not `fvol*`: task #716's rule
                // keys on the four bytes the image compares.
                ObjectSpec::new("volume", 5).extent([2.0, 0.0, 2.0], [3.0, 1.0, 3.0]),
            ],
            unlisted_extra: false,
        }
    }
}

/// Writes one CS GameZ container holding the fixture.
///
/// The header, the 212-byte info slot and the two data records are written from
/// the format worksheet's field offsets, independently of the reader, and the
/// container ends exactly where the data section ends — which is one of the
/// checks `read_gamez_nodes` makes.
pub(crate) fn write_container(fixture: &Fixture) -> Vec<u8> {
    let mut nodes: Vec<ObjectSpec> = fixture.objects.clone();
    if fixture.unlisted_extra {
        nodes.push(ObjectSpec::new("stray", -1));
    }
    let count = 1 + nodes.len();
    let cells_len: usize = fixture.grid.iter().map(|cell| 88 + 12 * cell.len()).sum();
    let world_len = 208 + cells_len + 4 * fixture.stored_children.len();
    let mut lengths = vec![world_len];
    // An object record: 144 own bytes plus the one parent word.
    lengths.extend(std::iter::repeat_n(144 + 4, nodes.len()));
    let data_offset = NODES_OFFSET + SLOT as u32 * count as u32;
    let mut offsets = Vec::with_capacity(count);
    let mut at = data_offset;
    for length in &lengths {
        offsets.push(at);
        at += *length as u32;
    }

    let mut bytes = vec![0u8; at as usize];
    let word = |bytes: &mut Vec<u8>, at: usize, value: u32| {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };
    let half = |bytes: &mut Vec<u8>, at: usize, value: u16| {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    };
    let float = |bytes: &mut Vec<u8>, at: usize, value: f32| {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };
    for (field, value) in [
        (0usize, SIGNATURE),
        (4, VERSION),
        (8, 0x1234_5678),
        (12, 1),
        (16, 40),
        (20, 44),
        (24, 48),
        (28, count as u32),
        (32, 0),
        (36, NODES_OFFSET),
    ] {
        word(&mut bytes, field, value);
    }

    // The world record: its 36-byte name, then the words the reader reads.
    let slot = NODES_OFFSET as usize;
    bytes[slot..slot + 5].copy_from_slice(b"world");
    word(&mut bytes, slot + 36, 0x0180_0000);
    word(&mut bytes, slot + 40, 0);
    word(&mut bytes, slot + 44, 1);
    word(&mut bytes, slot + 48, 255);
    word(&mut bytes, slot + 52, 2);
    word(&mut bytes, slot + 56, offsets[0]);
    word(&mut bytes, slot + 60, u32::MAX);
    word(&mut bytes, slot + 64, 0);
    word(&mut bytes, slot + 68, 1);
    word(&mut bytes, slot + 72, 0);
    half(&mut bytes, slot + 84, 0);
    half(&mut bytes, slot + 86, fixture.stored_children.len() as u16);
    word(&mut bytes, slot + 196, 0);
    word(&mut bytes, slot + 208, 0x0200_0000);

    // Its own 204-byte record, the one child-value word, then the grid.
    let mut at = offsets[0] as usize;
    word(&mut bytes, at + 152, GRID_X);
    word(&mut bytes, at + 156, GRID_Y);
    word(&mut bytes, at + 176, 1);
    at += 208;
    for slots in &fixture.grid {
        // The cell's own 88 bytes: the flag word and the six header floats are
        // stored bytes this stage does not interpret, so the fixture writes a
        // recognisable pattern and leaves the rest zero.
        word(&mut bytes, at, 0x100);
        float(&mut bytes, at + 8, -1.0);
        half(&mut bytes, at + 58, slots.len() as u16);
        at += 88;
        for slot in slots {
            word(&mut bytes, at, *slot);
            float(&mut bytes, at + 4, 0.0);
            float(&mut bytes, at + 8, 1.0);
            at += 12;
        }
    }
    for (position, child) in fixture.stored_children.iter().enumerate() {
        word(&mut bytes, at + 4 * position, *child);
    }

    // The object records.
    for (position, object) in nodes.iter().enumerate() {
        let index = position as u32 + 1;
        let slot = NODES_OFFSET as usize + SLOT * (index as usize);
        bytes[slot..slot + object.name.len()].copy_from_slice(object.name.as_bytes());
        word(&mut bytes, slot + 36, 0x0180_0000);
        word(&mut bytes, slot + 40, 0);
        word(&mut bytes, slot + 44, 1);
        word(&mut bytes, slot + 48, 255);
        word(&mut bytes, slot + 52, 5);
        word(&mut bytes, slot + 56, offsets[index as usize]);
        word(&mut bytes, slot + 60, object.mesh as u32);
        word(&mut bytes, slot + 64, 0);
        word(&mut bytes, slot + 68, 1);
        word(&mut bytes, slot + 72, 0);
        half(&mut bytes, slot + 84, u16::from(object.has_parent));
        half(&mut bytes, slot + 86, 0);
        word(&mut bytes, slot + 196, object.unk196);
        word(&mut bytes, slot + 208, 0x0200_0000);

        // The 144-byte object record: the identity flag, the identity euler
        // triple, unit scale and the identity matrix.
        let at = offsets[index as usize] as usize;
        word(&mut bytes, at, 40);
        for axis in 0..3 {
            float(&mut bytes, at + 36 + 4 * axis, 1.0);
            float(&mut bytes, at + 48 + 4 * axis, 1.0);
            float(&mut bytes, at + 48 + 4 * (axis + 3), 1.0);
            float(&mut bytes, at + 48 + 4 * (axis + 6), 1.0);
            float(&mut bytes, at + 84 + 4 * axis, 0.0);
        }
        word(&mut bytes, at + 144, WORLD);
        // The record's own stored world-space box: `unk140`'s two `Vec3`s, at
        // 140 and 152 of the 208-byte info slot.
        for axis in 0..3 {
            float(&mut bytes, slot + 140 + 4 * axis, object.box_min[axis]);
            float(&mut bytes, slot + 152 + 4 * axis, object.box_max[axis]);
        }
    }
    bytes
}

/// The production reader's view of the fixture container.
pub(crate) fn read(bytes: &[u8]) -> cs_formats::gamez::GameZNodes {
    let mut context = ParseContext::with_defaults("fixture.world-import");
    read_gamez_nodes(&mut context, bytes).expect("the fixture container reads")
}

/// The caller's mesh-slot table: one catalog element per stored mesh slot.
pub(crate) fn mesh_slots() -> Vec<MeshSlot> {
    (0..MESH_SLOTS)
        .map(|index| {
            MeshSlot::new(
                ContentId::from_source(ContentKind::Mesh, &format!("fixture.mesh-{index}"))
                    .expect("the fixture's mesh keys are valid"),
                provenance(),
            )
            .expect("a mesh-kind id is a mesh slot")
        })
        .collect()
}

/// The provenance every fixture value carries.
pub(crate) fn provenance() -> Provenance {
    Provenance::new(
        ClaimId::new("fixture.world-import").expect("the fixture claim id is valid"),
        ClaimStatus::Designed,
        None,
    )
    .expect("a synthetic claim needs no source span")
}

/// The conversion the fixture is imported through: the canonical convention,
/// declared, with the unit evidence class its own `UnitCalibration` reports.
pub(crate) fn adapter() -> SourceAdapter {
    SourceAdapter::declared()
        .into_iter()
        .find(|adapter| adapter.source().label() == "canonical")
        .expect("the F16-A registry declares the canonical source")
}

/// The conversion the **retail** container is imported through: the measured
/// GameZ source (task #677), whose scale landmark census pins one stored unit
/// to the metre at `observed_tool` while the rest of the convention stays
/// honestly uncalibrated.
fn retail_adapter(container: &RetailWorldContainer) -> SourceAdapter {
    SourceAdapter::new(cs_content::coordinates::CoordinateSource::retail_gamez(
        container.span().clone(),
    ))
}

/// The fixture imported, through production code.
pub(crate) fn imported(bytes: &[u8]) -> ImportedWorld {
    let records = read(bytes);
    import_world_container(
        cs_content::world::WorldId::from_key("fixture").expect("the fixture world key is valid"),
        Origin::SyntheticFixture,
        &records,
        bytes,
        &mesh_slots(),
        &adapter(),
        provenance(),
    )
    .expect("the fixture container imports")
}

/// The one stored box the fixture's [`ObjectSpec::extent`] helper builds: eight
/// corners and six quad faces.
fn tile_mesh() -> RawMesh {
    let mut positions = Vec::new();
    for corner in [
        [-0.5, 0.0, -0.5],
        [0.5, 0.0, -0.5],
        [0.5, 1.0, -0.5],
        [-0.5, 1.0, -0.5],
        [-0.5, 0.0, 0.5],
        [0.5, 0.0, 0.5],
        [0.5, 1.0, 0.5],
        [-0.5, 1.0, 0.5],
    ] {
        positions.push(corner);
    }
    let polygons = [
        [0, 1, 2, 3],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ]
    .into_iter()
    .map(|face| RawPolygon {
        kind: PrimitiveKind::Polygon,
        raw_flags: 0,
        material: 0,
        corners: face
            .iter()
            .map(|corner| RawCorner {
                position: *corner,
                normal: None,
                uv: None,
                color: None,
            })
            .collect(),
    })
    .collect();
    RawMesh {
        positions,
        normals: Vec::new(),
        polygons,
    }
}

/// The mesh source the spawn consumes: one engine mesh per stored mesh slot the
/// definition names, uploaded through the production F17-B adapter.
fn fixture_meshes(world: &cs_content::world::WorldDefinition) -> WorldMeshes {
    const UNKNOWNS: [MeshPresentationUnknown; 2] = [
        MeshPresentationUnknown::FrontFaceWinding,
        MeshPresentationUnknown::UvOrigin,
    ];
    let mut meshes = WorldMeshes::new();
    let stored = tile_mesh();
    let render = RenderMesh::build(&stored).expect("the fixture tile has a decodable outline");
    for object in world.objects() {
        let Resolved::Known(known) = object.mesh() else {
            continue;
        };
        meshes
            .insert_render_mesh(known.value.clone(), &render, &UNKNOWNS)
            .expect("the fixture tile uploads through every material group");
    }
    meshes
}

// ------------------------------------------------------------ the synthetic half --

/// **The world record's partition grid is the sector index, and an object's
/// sector list is exactly the cells that name it.**
#[test]
fn accept_m01_lc_world_import_the_partition_grid_becomes_the_sector_index() {
    let bytes = write_container(&Fixture::default());
    let records = read(&bytes);
    let grid = WorldPartitionGrid::read(&records, &bytes).expect("the grid reads");

    assert_eq!(
        grid.world_node(),
        WORLD,
        "the grid belongs to the world record"
    );
    assert_eq!(grid.x_count(), GRID_X);
    assert_eq!(grid.y_count(), GRID_Y);
    assert_eq!(grid.cells().len(), 2, "one cell per grid position");
    assert_eq!(grid.value_count(), 3, "one value per member");
    assert_eq!(grid.indexed_slots(), vec![TILE_A, SLAB, TILE_B]);
    assert!(
        grid.empty_cells().is_empty(),
        "every fixture cell names a record"
    );
    // The cell's six stored header floats are carried, and **not** interpreted:
    // the fixture writes a recognisable one and the value survives untouched,
    // which is the only claim this stage makes about them.
    assert_eq!(
        grid.cell(0).expect("the first cell").header_floats()[0],
        -1.0,
        "the cell's stored header bytes survive the read"
    );
    assert!(
        !cs_content::world::WorldPartitionCell::header_floats_are_interpreted(),
        "the cell's own header floats are not read as an extent"
    );

    let imported = imported(&bytes);
    let world = imported.definition();
    let sector_keys: Vec<String> = world
        .sectors()
        .iter()
        .map(|sector| sector.id().as_str().to_owned())
        .collect();
    assert_eq!(
        sector_keys,
        vec!["partition-00-00".to_owned(), "partition-00-01".to_owned()],
        "one sector per cell, keyed by the cell's grid coordinates"
    );

    // A sector's extent is the **union** of the stored boxes its members state.
    // The fixture's first cell holds two records whose boxes reach past each
    // other, so an extent built from either one alone is a different box:
    // `tile_a` alone is `[-10,-9] x [0,0] x [-10,-9]` and `slab` alone reaches
    // `z = -12`.
    let first = world.sectors()[0].bounds();
    assert_eq!(
        first.min(),
        [-10.0, 0.0, -12.0],
        "the first cell's extent starts at its members' lowest stored corner"
    );
    assert_eq!(
        first.max(),
        [-8.0, 0.0, -9.0],
        "and ends at the other member's, not at the first one's"
    );
    let second = world.sectors()[1].bounds();
    assert_eq!(
        second.min(),
        [0.0, 0.0, 0.0],
        "the second cell holds exactly one record"
    );
    assert_eq!(second.max(), [1.0, 0.0, 1.0]);
    assert_eq!(
        world
            .objects_in_sector(&world.sectors()[0].id().clone())
            .len(),
        2,
        "both records of the first cell belong to it"
    );
    assert_eq!(
        world
            .objects_in_sector(&world.sectors()[1].id().clone())
            .len(),
        1,
        "and the second cell has its own single member"
    );

    // The records the grid does not name stay resident, which is what the store
    // says rather than the absence of a membership record.
    let resident: Vec<String> = world
        .resident_objects()
        .iter()
        .map(|object| object.id().as_str().to_owned())
        .collect();
    assert_eq!(
        resident,
        vec![format!("node-{MARKER}"), format!("node-{VOLUME}")]
    );
}

/// **The identity is the node slot, the mesh reference is the caller's table, and
/// the transform of an identity record is the identity.**
#[test]
fn accept_m01_lc_world_import_an_object_carries_its_slot_its_mesh_and_its_stored_transform() {
    let bytes = write_container(&Fixture::default());
    let imported = imported(&bytes);
    let world = imported.definition();

    let tile = world
        .object(&cs_content::world::WorldObjectId::new("node-1").expect("the key is valid"))
        .expect("an object exists for every record the world node owns");
    assert_eq!(tile.id().as_str(), "node-1");
    assert_eq!(
        tile.mesh()
            .clone()
            .known()
            .map(|id| id.key().to_owned())
            .as_deref(),
        Some("fixture.mesh-7"),
        "the mesh reference is the caller's table entry for the stored index"
    );
    assert_eq!(
        tile.transform().linear(),
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        "a record that stores the identity transform gets the identity"
    );
    assert_eq!(tile.transform().translation(), [0.0, 0.0, 0.0]);
    assert_eq!(tile.sectors().len(), 1, "one cell names it");

    let marker = world
        .object(&cs_content::world::WorldObjectId::new("node-4").expect("the key is valid"))
        .expect("the meshless world-owned record is imported too");
    let mesh = marker.mesh();
    let Resolved::Unknown { claim_id, reason } = &mesh else {
        panic!("a record that stores no mesh index resolves no mesh: {mesh:?}");
    };
    assert_eq!(
        claim_id.as_str(),
        OBJECT_STORES_NO_MESH,
        "and the claim names the record's own bytes, not its identity"
    );
    assert!(
        reason.contains("no mesh index"),
        "the reason names what the record stores: {reason}"
    );
    assert!(marker.is_resident(), "and it belongs to no cell");

    // Every object's provenance is the one the caller supplied, so a reader can
    // get from any value back to the bytes it came from.
    assert_eq!(tile.provenance(), &provenance());
}

/// **The role follows the spatial index; an unindexed record that stores no
/// geometry resolves to `None`, one that does keeps its measured unknown, and
/// every other unmeasured surface is named — nothing is defaulted.**
#[test]
fn accept_m01_lc_world_import_the_collision_role_follows_the_index_and_every_other_role_is_named() {
    let bytes = write_container(&Fixture::default());
    let imported = imported(&bytes);
    let world = imported.definition();

    let indexed = world
        .object(&cs_content::world::WorldObjectId::new("node-1").expect("the key is valid"))
        .expect("the indexed record imported");
    assert_eq!(
        indexed.known_collision(),
        Some(cs_content::world::WorldCollisionRole::Solid),
        "a record the grid names is the world's static geometry"
    );
    assert_eq!(
        indexed.known_shape(),
        Some(cs_content::world::WorldCollisionShape::FromMesh),
        "and its collider is derived from its own mesh"
    );

    // The measured anchor arm (task #677): an unindexed record that binds no
    // mesh and stores no extent has nothing a collider could be built from, so
    // its role resolves to `None` — and its *shape* stays an explicit unknown,
    // because a record with no geometry stores no shape at all.
    let anchor = world
        .object(&cs_content::world::WorldObjectId::new("node-4").expect("the key is valid"))
        .expect("the unindexed anchor record imported");
    assert_eq!(
        anchor.known_collision(),
        Some(cs_content::world::WorldCollisionRole::None),
        "a record with no mesh and no extent is presented and never blocks"
    );
    let shape = anchor.shape();
    let Resolved::Unknown { claim_id, reason } = shape else {
        panic!("a record with no geometry stores no shape: {shape:?}");
    };
    assert_eq!(claim_id.as_str(), UNINDEXED_RECORD_STORES_NO_GEOMETRY);
    assert!(
        reason.contains("no mesh"),
        "the reason names what the record stores: {reason}"
    );

    // The measured volume arm: an unindexed record that **does** bind a mesh
    // and an extent keeps the honest unknown — the container never says whether
    // the engine collided with it.
    let unindexed = world
        .object(&cs_content::world::WorldObjectId::new("node-5").expect("the key is valid"))
        .expect("the unindexed volume record imported");
    let role = unindexed.collision();
    let Resolved::Unknown { claim_id, reason } = role else {
        panic!(
            "a record the grid does not name but that stores geometry has no measured role: {role:?}"
        );
    };
    assert_eq!(claim_id.as_str(), UNINDEXED_ROLE_UNMEASURED);
    assert!(
        reason.contains("partition grid"),
        "the reason names what is missing: {reason}"
    );
    assert!(!unindexed.shape().is_known());

    // Every surface and the world's boundary are unknown too, and every one of
    // them says which measurement is missing.
    assert_eq!(world.objects().len(), 5);
    for object in world.objects() {
        let Resolved::Unknown { claim_id, .. } = object.surface() else {
            panic!("no surface is measured: {object:?}");
        };
        assert_eq!(claim_id.as_str(), WORLD_SURFACE_UNMEASURED);
    }
    let Resolved::Unknown { claim_id, .. } = world.boundary() else {
        panic!("the world's boundary is not measured");
    };
    assert_eq!(claim_id.as_str(), WORLD_BOUNDARY_UNMEASURED);
    assert!(
        world.known_boundary().is_none(),
        "and no boundary is invented"
    );

    // The report names every one of those counts, so a consumer does not have to
    // enumerate the definition to learn the shape of the gap.
    let report = imported.report();
    assert_eq!(report.world_node(), WORLD);
    assert_eq!(report.partition_cells(), 2);
    assert_eq!(report.partition_records(), 3);
    assert_eq!(report.partition_records_with_mesh(), 3);
    assert_eq!(report.stored_child_list(), 2);
    assert_eq!(report.objects(), 5);
    assert_eq!(report.objects_with_mesh(), 4);
    assert_eq!(report.objects_solid(), 3);
    assert_eq!(report.objects_unindexed_none(), 1);
    assert_eq!(report.objects_unindexed_unresolved(), 1);
    assert_eq!(report.objects_in_a_sector(), 3);
    assert_eq!(report.objects_resident(), 2);
    assert_eq!(report.sectors(), 2);
    assert_eq!(report.sectors_without_extent(), 0);
    assert_eq!(report.empty_cells(), 0);
    assert_eq!(
        report.unit_class(),
        adapter()
            .source()
            .calibration()
            .quantity_status(cs_content::coordinates::CalibratedQuantity::Scale),
        "the report carries the caller's own evidence class for the unit"
    );
}

/// **A grid that names the same record twice, or names one that is not there or
/// not an object, blocks the container with a typed refusal.**
#[test]
fn accept_m01_lc_world_import_a_grid_that_contradicts_itself_blocks_the_container() {
    // Two cells naming one record.
    let repeated = Fixture {
        grid: vec![vec![TILE_A], vec![TILE_A]],
        ..Fixture::default()
    };
    let bytes = write_container(&repeated);
    let records = read(&bytes);
    assert_eq!(
        WorldPartitionGrid::read(&records, &bytes).expect_err("a repeated slot is refused"),
        WorldImportError::PartitionSlotRepeated { slot: TILE_A },
        "a spatial index that lists a record twice is not this index"
    );

    // A value naming a slot the container does not hold.
    let absent = Fixture {
        grid: vec![vec![99], vec![TILE_B]],
        ..Fixture::default()
    };
    let bytes = write_container(&absent);
    let records = read(&bytes);
    let error = WorldPartitionGrid::read(&records, &bytes)
        .expect_err("a slot outside the array is refused");
    let WorldImportError::PartitionSlotOutOfRange { cell, slot, .. } = error else {
        panic!("the refusal must name the slot: {error:?}");
    };
    assert_eq!((cell, slot), (0, 99));

    // A value naming a record that is not an object record: the world node's own
    // slot, which a spatial index of object geometry has no reason to hold.
    let not_an_object = Fixture {
        grid: vec![vec![WORLD], vec![TILE_B]],
        ..Fixture::default()
    };
    let bytes = write_container(&not_an_object);
    let records = read(&bytes);
    assert_eq!(
        WorldPartitionGrid::read(&records, &bytes).expect_err("a non-object value is refused"),
        WorldImportError::PartitionSlotNotAnObject {
            cell: 0,
            slot: WORLD,
            kind: "world",
        },
        "an index that names a world record is not the object index this conversion reads"
    );

    // A record the world node owns that is in neither the grid nor the stored
    // child list: the three ownership statements no longer agree, and the
    // conversion is blocked rather than run under a rule its own bytes
    // contradict.
    let stray = Fixture {
        unlisted_extra: true,
        ..Fixture::default()
    };
    let bytes = write_container(&stray);
    let records = read(&bytes);
    assert_eq!(
        imported_checked(&records, &bytes).expect_err("disagreeing ownership is refused"),
        WorldImportError::OwnershipDisagreement {
            grid: 3,
            child_list: 2,
            naming: 6,
        }
    );
}

/// **A stored mesh index the caller's table does not answer blocks the container
/// rather than resolving a mesh from somewhere else.**
#[test]
fn accept_m01_lc_world_import_a_mesh_index_the_callers_table_does_not_hold_refuses_the_container() {
    let bytes = write_container(&Fixture::default());
    let records = read(&bytes);
    // `tile_a` stores mesh index 7 and `slab` 6, so a table of seven slots
    // answers one of the three fixture records and leaves the other two out.
    let mut slots = mesh_slots();
    slots.truncate(MESH_SLAB as usize + 1);
    let refused = import_world_container(
        cs_content::world::WorldId::from_key("fixture").expect("the fixture world key is valid"),
        Origin::SyntheticFixture,
        &records,
        &bytes,
        &slots,
        &adapter(),
        provenance(),
    )
    .expect_err("a mesh index outside the caller's table is refused");
    assert_eq!(
        refused,
        WorldImportError::MeshSlotMissing {
            index: MESH_TILE_A as u32,
            slots: MESH_SLAB as usize + 1,
        },
        "the refusal names the index the record stored and how many slots exist"
    );
}

/// The import with an explicit expectation about its outcome.
fn imported_checked(
    records: &cs_formats::gamez::GameZNodes,
    bytes: &[u8],
) -> Result<ImportedWorld, WorldImportError> {
    import_world_container(
        cs_content::world::WorldId::from_key("fixture").expect("the fixture world key is valid"),
        Origin::SyntheticFixture,
        records,
        bytes,
        &mesh_slots(),
        &adapter(),
        provenance(),
    )
}

/// **`spawn_world` runs on an imported definition and derives each indexed
/// object's collider from the geometry the definition names.**
#[test]
fn accept_m01_lc_world_import_spawn_world_runs_on_the_imported_definition() {
    let bytes = write_container(&Fixture::default());
    let imported = imported(&bytes);
    let world = imported.definition();
    let meshes = fixture_meshes(world);
    let mut app = world_app();

    let spawned = spawn_world(&mut app, world, &meshes).expect("the imported world spawns");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }

    assert_eq!(spawned.objects().len(), world.objects().len());
    let skipped: Vec<SkipReason> = spawned.skipped().iter().map(|entry| entry.reason).collect();
    assert_eq!(
        skipped,
        vec![SkipReason::UnknownCollisionRole],
        "the unindexed record that stores geometry is reported as exactly that"
    );
    assert_eq!(
        spawned.non_colliding(),
        vec![cs_content::world::WorldObjectId::new("node-4").expect("the key is valid")],
        "the anchor stores nothing a collider could be built from, so it is \
         presented and never blocks — a deliberate answer, not a skip"
    );
    assert_eq!(
        spawned.colliders().len(),
        3,
        "one collider per indexed record, and none for the unindexed ones"
    );
    // Every derived collider carries the twelve triangles of the one stored mesh
    // the fixture uploaded, so nothing substituted a shape on the way in.
    for object in world.objects() {
        let Some(entity) = spawned.collider_for(object.id()) else {
            continue;
        };
        let collider = app
            .world()
            .get::<Collider>(entity)
            .expect("a spawned collider is a collider");
        assert!(
            collider.shape().as_trimesh().is_some(),
            "a FromMesh record is collided by a triangle mesh"
        );
    }
}

/// **The claim ids this conversion uses are the ones its records carry.**
#[test]
fn accept_m01_lc_world_import_the_conversion_is_recorded_under_its_own_claim_ids() {
    assert_eq!(PARTITION_GRID_IS_THE_SECTOR_INDEX, GRID_IS_THE_SECTOR_INDEX);
    for id in [
        PARTITION_GRID_IS_THE_SECTOR_INDEX,
        INDEXED_RECORD_IS_STATIC,
        UNINDEXED_ROLE_UNMEASURED,
        UNINDEXED_RECORD_STORES_NO_GEOMETRY,
        WORLD_SURFACE_UNMEASURED,
        WORLD_BOUNDARY_UNMEASURED,
        OBJECT_ID_IS_THE_NODE_SLOT,
        OBJECT_STORES_NO_MESH,
        RETAIL_WORLD_IMPORT,
    ] {
        assert!(
            ClaimId::new(id).is_ok(),
            "every claim id this conversion records must be a valid one: {id}"
        );
    }
    let distinct: BTreeSet<&str> = [
        PARTITION_GRID_IS_THE_SECTOR_INDEX,
        INDEXED_RECORD_IS_STATIC,
        UNINDEXED_ROLE_UNMEASURED,
        UNINDEXED_RECORD_STORES_NO_GEOMETRY,
        WORLD_SURFACE_UNMEASURED,
        WORLD_BOUNDARY_UNMEASURED,
        OBJECT_ID_IS_THE_NODE_SLOT,
        OBJECT_STORES_NO_MESH,
    ]
    .into_iter()
    .collect();
    assert_eq!(distinct.len(), 8, "each gap has its own claim id");
}

// ------------------------------------------------------------------ the retail half --

/// `ZBD/C1C/gamez.zbd`, the container the task names.
const RETAIL_GROUP: &str = "C1C";

/// The measured shape of `ZBD/C1C/gamez.zbd`, read through the production
/// readers. Every number here is a count or a dimension of the container's own
/// records; none is a name list, a mesh or a screenshot, so nothing derived from
/// the original bytes is committed.
struct Retail {
    container: RetailWorldContainer,
    imported: ImportedWorld,
}

fn retail() -> Retail {
    let root = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set for the retail tests");
    let container = read_world_container(
        std::path::Path::new(&root),
        RETAIL_GROUP,
        &WorldTextureLoad::project_default(),
    )
    .expect("the world's geometry container reads");
    let origin = Origin::Installation {
        source: container.span().clone(),
    };
    let imported = container
        .definition(origin, &retail_adapter(&container))
        .expect("the container imports");
    Retail {
        container,
        imported,
    }
}

/// **c1c's `gamez.zbd` produces a `WorldDefinition` whose sector and object
/// counts are the container's own, and every unresolved role is named.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_world_import_retail_c1c_becomes_a_world_definition_with_every_gap_named() {
    let retail = retail();
    let report = retail.imported.report();
    let world = retail.imported.definition();

    assert_eq!(
        world.id().key(),
        "c1c",
        "the definition is the world the container is"
    );

    // The grid: 12 by 12 cells, 293 values, no cell empty, every value a
    // distinct object record.
    assert_eq!(report.partition_cells(), 144);
    assert_eq!(report.partition_records(), 293);
    assert_eq!(
        report.empty_cells(),
        0,
        "every cell names at least one record"
    );
    assert_eq!(
        report.partition_records_with_mesh(),
        292,
        "one indexed record binds no mesh, and the store says so with a zero box"
    );

    // The three ownership statements agree: the grid's records plus the world
    // node's own stored child list are exactly the records that name it.
    assert_eq!(report.stored_child_list(), 53);
    assert_eq!(report.objects(), 346);
    assert_eq!(report.objects_with_mesh(), 309);
    assert_eq!(report.objects_solid(), 293);
    assert_eq!(report.objects_in_a_sector(), 293);
    assert_eq!(report.objects_resident(), 53);
    assert_eq!(report.sectors(), 144);
    assert_eq!(report.sectors_without_extent(), 0);

    // Task #677's measured split of the 53 unindexed records, as task #716
    // resolves it: the 36 that bind no mesh and store no extent — the
    // `horizon`, the `g27816` transform groups and the zeppelin anchors —
    // resolve to `None` because the store gives them nothing a collider could
    // be built from; the 17 `fvol*` records bind a mesh and an extent and are
    // fog volumes — the image's only name-keyed consumer of that prefix is its
    // fog routine — so they are presented and never block either, while their
    // meshes stay known; and nothing else in this container stores geometry
    // without a measured prefix, so the unmeasured class is empty here.
    assert_eq!(
        report.objects_unindexed_none(),
        36,
        "the anchors, transform groups and horizon resolve to a deliberate `None`"
    );
    assert_eq!(
        report.objects_unindexed_fog(),
        17,
        "the fvol* volumes are fog volumes: presented, never blocking"
    );
    assert_eq!(
        report.objects_unindexed_unresolved(),
        0,
        "c1c holds no unindexed record that stores geometry without a measured prefix"
    );
    assert_eq!(
        report.partition_records_fog_volume(),
        4,
        "four of c1c's fvol* records are named by the partition grid as well, and the \
         disagreement between the two rules is counted rather than settled"
    );

    // Everything the container does not state is an explicit unknown, and the
    // definition's own accessors report every one of them.
    assert_eq!(report.matrix_disagreements(), 0);
    assert!(
        world.unresolved_collision().is_empty(),
        "no c1c record stores collision geometry without a measured role any more: {}",
        world.unresolved_collision().len()
    );
    assert_eq!(
        world.unresolved_surface().len(),
        346,
        "no gameplay surface is measured for any record"
    );
    assert!(world.known_boundary().is_none());
    for object in world.unresolved_collision() {
        let Resolved::Unknown { claim_id, .. } = object.collision() else {
            panic!("an unresolved role must be an explicit unknown");
        };
        assert_eq!(claim_id.as_str(), UNINDEXED_ROLE_UNMEASURED);
    }
    for object in world.unresolved_surface() {
        let Resolved::Unknown { claim_id, .. } = object.surface() else {
            panic!("an unresolved surface must be an explicit unknown");
        };
        assert_eq!(claim_id.as_str(), WORLD_SURFACE_UNMEASURED);
    }

    // The unit is named, not assumed: the report states the factor it used and
    // that factor's own evidence class — which for the measured GameZ source is
    // `observed_tool`, the strongest claim a byte census without an original
    // run can carry (task #677).
    assert_eq!(
        report.meters_per_unit(),
        1.0,
        "one stored GameZ unit is the metre, measured"
    );
    assert_eq!(
        report.unit_class(),
        ClaimStatus::ObservedTool,
        "the scale evidence is tool-observed, never verified_original"
    );

    // The container's other content is counted, not imported: these records bind
    // meshes and belong to other subsystems.
    assert_eq!(
        report.mesh_binding_records_elsewhere(),
        3045,
        "the effect hierarchies, the aircraft and the rest of the container are \
         counted here rather than imported as world objects"
    );
    assert_eq!(
        retail.container.container_key(),
        "zbd/c1c/gamez.zbd",
        "the container is the one production discovery spells"
    );
    assert!(!retail.container.container_sha256().is_empty());

    // Every resolved value points back at the bytes it was read from, and the
    // class it carries is `ObservedTool` — measured from the container by the
    // production readers, never `VerifiedOriginal`, because no original run
    // happened.
    let mut role_provenance = None;
    for object in world.objects() {
        let provenance = object.provenance();
        assert_eq!(
            provenance.claim_id.as_str(),
            RETAIL_WORLD_IMPORT,
            "a retail-derived value is filed under the import's own claim"
        );
        assert_eq!(
            provenance.class,
            ClaimStatus::ObservedTool,
            "reading container bytes is a tool observation, not an original run"
        );
        assert_eq!(
            provenance.source.as_ref(),
            Some(retail.container.span()),
            "and it names the container span a reader can go back to"
        );
        if let Resolved::Known(known) = object.collision() {
            role_provenance = Some(known.provenance.clone());
        }
    }
    // A resolved *value* carries the same provenance as the record it belongs
    // to, so a consumer reading the role alone can still get back to the bytes.
    let role = role_provenance.expect("an indexed record resolves a collision role");
    assert_eq!(role.claim_id.as_str(), RETAIL_WORLD_IMPORT);
    assert_eq!(role.source.as_ref(), Some(retail.container.span()));
}

/// **`spawn_world` runs on the definition imported from c1c's real container, and
/// the world is presented and collided from the geometry the record names.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_world_import_retail_spawn_world_runs_on_the_imported_c1c_definition() {
    let retail = retail();
    let world = retail.imported.definition();
    let meshes = retail
        .container
        .uploaded_meshes(world)
        .expect("the container's own meshes upload");
    assert!(
        meshes.len() >= 292,
        "at least every indexed record's mesh is registered: {}",
        meshes.len()
    );
    let mut app = world_app();
    let spawned = spawn_world(&mut app, world, &meshes)
        .expect("the imported definition spawns, including every shear-free affine");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }

    assert_eq!(spawned.objects().len(), world.objects().len());
    assert_eq!(
        spawned.colliders().len(),
        292,
        "every indexed record that binds a mesh collides"
    );
    let mut reasons: BTreeMap<&str, usize> = BTreeMap::new();
    for entry in spawned.skipped() {
        *reasons.entry(entry.reason.label()).or_default() += 1;
    }
    assert_eq!(
        reasons,
        BTreeMap::from([
            // The one indexed record that stores no mesh index: it is in the
            // world's spatial index and draws nothing, so it is reported rather
            // than given substitute geometry.
            ("unknown_mesh", 1),
        ]),
        "the 17 fvol* volumes are no longer a gap — they resolve as fog volumes \
         (task #716) — so the spawn reports exactly the one gap c1c's own bytes imply"
    );
    // The 36 anchors and the 17 fog volumes are a *deliberate* `None`:
    // presented, never blocking, and not in the skip list — a resolved record
    // does not read as a gap.
    assert_eq!(
        spawned.non_colliding().len(),
        53,
        "the anchors, horizon and fog volumes are presented with no collider by \
         their own answer"
    );
    assert_eq!(
        spawned.presentation_gap_count(),
        0,
        "no object reports two reasons: an anchor's own shape question stays an \
         unknown rather than reaching the mesh check"
    );
}

/// **A world object's mesh reference is an element of the shared render-mesh
/// catalog**, not a per-container name: the id is the one
/// `MeshId::content_id` (and so the retail baseline inventory) gives that stored
/// mesh, it names the bytes of the container the catalog read, and the catalog and the
/// session that read it agree on the generation.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f18_mesh_catalog_world_references_are_catalog_elements() {
    let retail = retail();
    let world = retail.imported.definition();
    let catalog = retail.container.mesh_catalog();
    let catalog_ids: BTreeSet<ContentId> = catalog
        .records()
        .iter()
        .filter_map(|record| record.id.as_ref())
        .map(|id| id.content_id().expect("a catalog mesh has an id"))
        .collect();
    assert!(!catalog_ids.is_empty(), "the catalog holds c1c's meshes");
    assert_eq!(
        catalog.generation(),
        retail.container.session().generation(),
        "the catalog was read by the session the container holds"
    );
    let container_span = catalog
        .containers()
        .next()
        .expect("the catalog opened c1c's container")
        .span()
        .clone();

    let mut referenced = BTreeSet::new();
    for object in world.objects() {
        let Resolved::Known(known) = object.mesh() else {
            continue;
        };
        assert_eq!(known.value.kind(), ContentKind::Mesh);
        assert!(
            !known.value.key().contains(".mesh-"),
            "no per-container naming survives: {}",
            known.value
        );
        assert!(
            catalog_ids.contains(&known.value),
            "{} is an element of the mesh catalog",
            known.value
        );
        // The definition files the reference under the import's own claim, with
        // the span the container's discovery spells; the catalog spells the same
        // file as a member of its group. They must name the same bytes.
        let source = known
            .provenance
            .source
            .as_ref()
            .expect("a retail reference names its bytes");
        assert_eq!(source.install_sha256(), container_span.install_sha256());
        assert_eq!(source.member_sha256(), container_span.member_sha256());
        assert_eq!(source.length(), container_span.length());
        referenced.insert(known.value.clone());
    }
    assert!(!referenced.is_empty());

    let meshes = retail
        .container
        .uploaded_meshes(world)
        .expect("the catalog's uploads fill the world's meshes");
    assert_eq!(
        meshes.len(),
        referenced.len(),
        "one engine mesh per catalog element the world names"
    );
    for id in &referenced {
        assert!(meshes.contains(id), "{id} is uploaded under its catalog id");
    }
}
