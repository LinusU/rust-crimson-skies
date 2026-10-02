//! GameZ mesh topology and the CS GameZ mesh-array reader
//! (`specs/F10-gamez-mesh-topology-and-material-records.md`, stages
//! `### F10-A` and `### F10-B`).
//!
//! These modules define the typed input and output every GameZ mesh reader
//! shares, turn it into triangles, and read the Crimson Skies GameZ mesh
//! section out of real `planes.zbd` / `gamez.zbd` bytes:
//!
//! * [`mesh`] is the IR: a [`RawMesh`] keeps stored positions, normals,
//!   polygons, raw flags, raw material indices and per-corner attributes,
//!   and [`RawMesh::topology`] reports source-mapped triangles plus the
//!   status of every polygon, invalid and unsupported ones included.
//! * [`strip`] decodes triangle strips with parity that degenerate steps
//!   still advance.
//! * [`polygon`] triangulates polygon outlines with more than three corners
//!   by validated ear clipping, never a fan, and names why an outline it
//!   cannot triangulate was rejected.
//! * [`reader`] is the layout: the 40-byte container header, the
//!   non-sequential mesh index with its measured fixup tables, the 100-byte
//!   mesh record, the 40-byte polygon record with its packed corner/flag
//!   word, the per-corner index, UV and colour arrays, the 12-byte mesh
//!   material reference and the 76-byte mesh light record. The layout was
//!   read from the pinned mech3ax v0.6.0 revision and checked against the
//!   original installation; [`read_gamez_meshes`] produces [`GameZMeshes`],
//!   whose entries are [`RawMesh`] values with everything still raw.
//! * [`census`] is the report: [`FaceCensus`] accounts for every face a
//!   container declares — what the records claim, what the section held, and
//!   each face that reaches no drawable triangle — which is AC04's exact
//!   missing/invalid face count for one archive.
//! * [`materials`] is the rest of the container header's sections: the
//!   44-byte texture-name record with its NUL-suffixed name encoding, the
//!   16-byte material section header, the 40-byte material record, the
//!   1000-slot array with its zero region and the cycle data stored after
//!   it. [`read_gamez_materials`] produces [`GameZMaterials`], which is where
//!   a material index finally resolves to a texture **name** — the material
//!   record stores an index into the container's texture table, not a name.
//!
//! * [`nodes`] is the last section: the node array, which is **two passes over
//!   two sections** — `node_array_size` × (a 208-byte info record followed by a
//!   4-byte `node_index` word), then a variable-length data section holding one
//!   kind-specific record per node in node order and running to the container's
//!   end. `read_gamez_nodes` walks the data section sequentially and *checks*
//!   each record's stored `data_ptr` against the offset it reached, so the
//!   pointer is a cross-check rather than a second way to address the same
//!   bytes. All nine GameZ archives of the original installation read and every
//!   data section ends exactly at its container's end; the worksheet and the
//!   measurements are in
//!   `docs/findings/2026-10-02-gamez-node-array-layout.md`. Which archive a
//!   texture name is looked up in, and whether the lookup succeeds, is the
//!   dependency audit in `cs_content::mesh`, which is fed the values above;
//!   no reader here picks an archive, folds case or falls back to another one.
//!
//! Nothing here reads past the node array: it runs to the end of the container.
//!
//! The lossless raw mesh IR ([`RawMesh`], [`RawPolygon`], [`RawCorner`]) is stage
//! F10-A's published contract and its shape is unchanged. A stored polygon's
//! material groups — the CS layout stores one UV set per group — are therefore
//! carried beside the polygons on [`GameZMesh::material_groups`], one entry per
//! stored polygon, reachable through [`GameZMesh::groups`] and
//! [`GameZMesh::corner_uv`]. The IR's single `material` and per-corner `uv`
//! mirror the **first** group, so they are exact for a single-group asset and a
//! single-material *view* of a multi-group one; the groups themselves are the
//! authority, and F10-E's `cs_content::mesh::RenderMesh::from_stored_groups`
//! builds a render mesh out of all of them rather than out of that view.
//!
//! Design decisions and recorded unknowns are in
//! `docs/findings/2026-09-28-f10-a-lossless-mesh-ir-and-strip-fixtures.md`,
//! `docs/findings/2026-09-29-f10-b-validated-ngon-triangulation.md`,
//! `docs/findings/2026-09-29-f10-b-gamez-mesh-layout.md` and
//! `docs/findings/2026-09-29-f10-c-02-gamez-material-records.md`. The synthetic
//! fixtures exercised by `crates/cs_formats/tests/gamez/` are newly authored
//! values; only the `#[ignore]`d retail tests read original game data, and
//! nothing derived from it is committed.

pub mod census;
pub mod materials;
pub mod mesh;
pub mod nodes;
pub mod polygon;
pub mod reader;
pub mod strip;

pub use census::{FaceCensus, MissingFace, MissingFaceReason};

pub use materials::{
    CYCLE_FRAME_BYTES, CYCLE_HEADER_BYTES, GameZMaterialError, GameZMaterials, GameZTextureName,
    KNOWN_MATERIAL_FLAGS, MATERIAL_FLAG_ALWAYS, MATERIAL_FLAG_CYCLED, MATERIAL_FLAG_FREE,
    MATERIAL_FLAG_TEXTURED, MATERIAL_FLAG_UNKNOWN, MATERIAL_HEADER_BYTES, MATERIAL_LINK_BYTES,
    MATERIAL_RECORD_BYTES, MATERIAL_SLOT_BYTES, MATERIALS_ENTRYPOINT, MaterialFinding,
    MaterialInfo, MaterialKind, NG_MATERIAL_SLOTS, RawCycle, RawMaterial, RawMaterialRecord,
    TEXTURE_INFO_BYTES, TEXTURE_NAME_BYTES, TextureNameEncoding, read_gamez_materials,
};
pub use mesh::{
    FaceIssue, FaceStatus, MeshTopology, MeshTriangle, PrimitiveKind, RawCorner, RawMesh,
    RawPolygon,
};
pub use nodes::{
    CAMERA_DATA_BYTES, DISPLAY_DATA_BYTES, GameZNodeError, GameZNodes, LIGHT_DATA_BYTES,
    LOD_DATA_BYTES, MATRIX_AGREEMENT_TOLERANCE, MeshIndexBounds, NODE_INDEX_BOT_MASK,
    NODE_INDEX_BYTES, NODE_INDEX_INVALID, NODE_INDEX_TOP, NODE_INDEX_TOP_MASK, NODE_INFO_BYTES,
    NODE_NAME_BYTES, NODE_SLOT_BYTES, NODE_TYPE_CAMERA, NODE_TYPE_DISPLAY, NODE_TYPE_EMPTY,
    NODE_TYPE_LIGHT, NODE_TYPE_LOD, NODE_TYPE_OBJECT3D, NODE_TYPE_WINDOW, NODE_TYPE_WORLD,
    NODES_ENTRYPOINT, NodeFinding, NodeKind, OBJECT3D_DATA_BYTES, OBJECT3D_FLAGS_IDENTITY,
    OBJECT3D_FLAGS_TRANSFORMED, RawLodData, RawNode, RawNodeInfo, RawObject3dData, RawWorldData,
    WINDOW_DATA_BYTES, WORLD_DATA_BYTES, WORLD_PARTITION_BYTES, WORLD_PARTITION_VALUE_BYTES,
    read_gamez_nodes,
};
pub use polygon::{NgonIssue, triangulate_polygon};
pub use reader::{
    CORNER_COUNT_MASK, FLAG_MASK, FLAG_NORMALS, FLAG_SHIFT, FLAG_TRIANGLE_STRIP, FLAG_UNK2,
    FLAG_UNK3, FLAG_UNK6, GAMEZ_HEADER_BYTES, GameZError, GameZHeader, GameZMesh, GameZMeshes,
    KNOWN_POLYGON_FLAGS, MAX_POLYGON_CORNERS, MAX_POLYGON_FLAGS, MESH_INDEX_BYTES, MESH_INFO_BYTES,
    MESH_INFO_TRAILER_BYTES, MESH_LIGHT_HEADER_BYTES, MESH_MATERIAL_INFO_BYTES, MESHES_ENTRYPOINT,
    MeshIndex, POLYGON_INFO_BYTES, ParseFinding, RawMaterialGroup, RawMeshInfo, RawMeshLight,
    RawMeshLightHeader, RawMeshMaterialInfo, RawPolygonInfo, UNK08_C4, UNK08_PLANES, VEC3_BYTES,
    read_gamez_meshes,
};
pub use strip::{MIN_STRIP_INDICES, StripError, StripTriangle, decode_strip};
