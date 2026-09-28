//! GameZ mesh topology: the lossless raw mesh IR, strip decoding and n-gon
//! triangulation (`specs/F10-gamez-mesh-topology-and-material-records.md`,
//! stages `### F10-A` and `### F10-B`).
//!
//! These modules define the typed input and output every GameZ mesh reader
//! shares and turn it into triangles. No `gamez.zbd` / `planes.zbd` bytes
//! are parsed yet: the stored layout is not established in the research
//! pack, and no installation bytes were read (ordinary build/test
//! capability only):
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
//!
//! Design decisions and recorded unknowns are in
//! `docs/findings/2026-09-28-f10-a-lossless-mesh-ir-and-strip-fixtures.md`
//! and `docs/findings/2026-09-29-f10-b-validated-ngon-triangulation.md`.
//! The fixtures exercised by `crates/cs_formats/tests/gamez/` are newly
//! authored synthetic values; nothing here is derived from original game
//! data.

pub mod mesh;
pub mod polygon;
pub mod strip;

pub use mesh::{
    FaceIssue, FaceStatus, MeshTopology, MeshTriangle, PrimitiveKind, RawCorner, RawMesh,
    RawPolygon,
};
pub use polygon::{NgonIssue, triangulate_polygon};
pub use strip::{MIN_STRIP_INDICES, StripError, StripTriangle, decode_strip};
