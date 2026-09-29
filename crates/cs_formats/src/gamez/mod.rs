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
//! * [`materials`] is the rest of the container header's sections: the
//!   44-byte texture-name record with its NUL-suffixed name encoding, the
//!   16-byte material section header, the 40-byte material record, the
//!   1000-slot array with its zero region and the cycle data stored after
//!   it. [`read_gamez_materials`] produces [`GameZMaterials`], which is where
//!   a material index finally resolves to a texture **name** — the material
//!   record stores an index into the container's texture table, not a name.
//!
//! What this does **not** read: the node array (F11-A) and anything past the
//! two sections above. Which archive a texture name is looked up in, and
//! whether the lookup succeeds, is the dependency audit in
//! `cs_content::mesh`, which is fed the two values above; neither reader picks
//! an archive, folds case or falls back to another one.
//!
//! The lossless raw mesh IR ([`RawMesh`], [`RawPolygon`], [`RawCorner`]) is stage
//! F10-A's published contract and this task does not change its shape. A stored
//! polygon's material groups — the CS layout stores one UV set per group — are
//! therefore carried beside the polygons on [`GameZMesh::material_groups`], one
//! entry per stored polygon, reachable through [`GameZMesh::groups`] and
//! [`GameZMesh::corner_uv`]. The IR's single `material` and per-corner `uv`
//! mirror the first group.
//!
//! Design decisions and recorded unknowns are in
//! `docs/findings/2026-09-28-f10-a-lossless-mesh-ir-and-strip-fixtures.md`,
//! `docs/findings/2026-09-29-f10-b-validated-ngon-triangulation.md`,
//! `docs/findings/2026-09-29-f10-b-gamez-mesh-layout.md` and
//! `docs/findings/2026-09-29-f10-c-02-gamez-material-records.md`. The synthetic
//! fixtures exercised by `crates/cs_formats/tests/gamez/` are newly authored
//! values; only the `#[ignore]`d retail tests read original game data, and
//! nothing derived from it is committed.

pub mod materials;
pub mod mesh;
pub mod polygon;
pub mod reader;
pub mod strip;

pub use materials::{
    CYCLE_FRAME_BYTES, CYCLE_HEADER_BYTES, GameZMaterialError, GameZMaterials,
    GameZTextureName, KNOWN_MATERIAL_FLAGS, MATERIAL_FLAG_ALWAYS, MATERIAL_FLAG_CYCLED,
    MATERIAL_FLAG_FREE, MATERIAL_FLAG_TEXTURED, MATERIAL_FLAG_UNKNOWN, MATERIALS_ENTRYPOINT,
    MATERIAL_HEADER_BYTES, MATERIAL_LINK_BYTES, MATERIAL_RECORD_BYTES, MATERIAL_SLOT_BYTES,
    MaterialFinding, MaterialInfo, MaterialKind, NG_MATERIAL_SLOTS, RawCycle, RawMaterial,
    RawMaterialRecord, TEXTURE_INFO_BYTES, TEXTURE_NAME_BYTES, TextureNameEncoding,
    read_gamez_materials,
};
pub use mesh::{
    FaceIssue, FaceStatus, MeshTopology, MeshTriangle, PrimitiveKind, RawCorner, RawMesh,
    RawPolygon,
};
pub use polygon::{NgonIssue, triangulate_polygon};
pub use reader::{
    CORNER_COUNT_MASK, FLAG_MASK, FLAG_NORMALS, FLAG_SHIFT, FLAG_TRIANGLE_STRIP, FLAG_UNK2,
    FLAG_UNK3, FLAG_UNK6, GAMEZ_HEADER_BYTES, GameZError, GameZHeader, GameZMesh, GameZMeshes,
    KNOWN_POLYGON_FLAGS, MAX_POLYGON_CORNERS, MAX_POLYGON_FLAGS, MESH_INDEX_BYTES, MESH_INFO_BYTES,
    MESH_INFO_TRAILER_BYTES, MESH_LIGHT_HEADER_BYTES, MESH_MATERIAL_INFO_BYTES, MESHES_ENTRYPOINT,
    MeshIndex, POLYGON_INFO_BYTES, ParseFinding, RawMeshInfo, RawMeshLight, RawMeshLightHeader,
    RawMeshMaterialInfo, RawPolygonInfo, UNK08_C4, UNK08_PLANES, VEC3_BYTES, read_gamez_meshes,
};
pub use strip::{MIN_STRIP_INDICES, StripError, StripTriangle, decode_strip};
