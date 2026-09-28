//! GameZ mesh topology: the lossless raw mesh IR and strip decoding
//! (`specs/F10-gamez-mesh-topology-and-material-records.md`, stage
//! `### F10-A`).
//!
//! This stage defines the typed input and output every GameZ mesh reader
//! shares and nothing else — no `gamez.zbd` / `planes.zbd` bytes are parsed
//! yet (that is F10-B), and no installation bytes were read for this stage
//! (ordinary build/test capability only):
//!
//! * [`mesh`] is the IR: a [`RawMesh`] keeps stored positions, normals,
//!   polygons, raw flags, raw material indices and per-corner attributes,
//!   and [`RawMesh::topology`] reports source-mapped triangles plus the
//!   status of every polygon, invalid and unsupported ones included.
//! * [`strip`] decodes triangle strips with parity that degenerate steps
//!   still advance.
//!
//! Design decisions and recorded unknowns are in
//! `docs/findings/2026-09-28-f10-a-lossless-mesh-ir-and-strip-fixtures.md`.
//! The fixtures exercised by `crates/cs_formats/tests/gamez/` are newly
//! authored synthetic values; nothing here is derived from original game
//! data.

pub mod mesh;
pub mod strip;

pub use mesh::{
    FaceIssue, FaceStatus, MeshTopology, MeshTriangle, PrimitiveKind, RawCorner, RawMesh,
    RawPolygon,
};
pub use strip::{MIN_STRIP_INDICES, StripError, StripTriangle, decode_strip};
