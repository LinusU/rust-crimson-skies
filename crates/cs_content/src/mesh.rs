//! Canonical render mesh: GameZ vertices split per corner attribute, the
//! dependency audit a GameZ mesh's material records imply, and the wiring from
//! a container on a content session to the mesh upload boundary
//! (`specs/F10-gamez-mesh-topology-and-material-records.md`, stage
//! `### F10-C`, slices F10-C.01, F10-C.02 and F10-C.03; shared contract
//! `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! [`RenderMesh`] is the Bevy-free handoff between a GameZ mesh reader
//! (F10-B's [`RawMesh`] plus its [`MeshTopology`]) and the upload adapter.
//! It is built with [`RenderMesh::from_stored_groups`], which takes the stored
//! mesh and **every stored material group** of its polygons; with
//! [`RenderMesh::build`], which computes the topology, or with
//! [`RenderMesh::from_parts`], which takes a topology the caller already has.
//! No Bevy or Avian type appears here.
//!
//! [`MeshDependencyAudit`] is the other half of that handoff: it resolves every
//! stored material index a mesh carries to a material record, every material
//! record to the texture name it stores, and that name to exactly one stored
//! texture in one named archive — or records why it could not.
//!
//! [`MeshContainer`] and [`MeshCatalog`] are the path those two live on, added
//! in F10-C.03:
//!
//! * **the producer.** [`MeshContainer::open`] resolves a container key in a
//!   [`ContentSession`], opens it with [`ZbdContainer::open`], **checks the ZBD
//!   dispatch routed it to [`ZbdFamily::GameZ`]**, reads the mesh section with
//!   [`read_gamez_meshes`] and the material section with
//!   [`read_gamez_materials`] from the container's own bytes, and cross-checks
//!   that the two readers read the same 40-byte header. It builds a
//!   [`RenderMesh`] for every present stored mesh and the audit for the
//!   container. The container owns its bytes, so everything it produced outlives
//!   the session.
//! * **the catalog.** [`MeshCatalog`] is that container type over a list of
//!   keys, with the F08-C session and retry semantics: a failed container is a
//!   row, [`MeshCatalog::retry_failed`] reopens only the failed containers of
//!   the same session, and a foreign session is refused. A catalog also carries
//!   a process-local serial, which is what binds a [`ResolvedMesh`] to the
//!   catalog that produced it.
//! * **the consumer boundary.** [`MeshCatalog::prepare_upload`] hands a
//!   [`MeshUpload`]: the split render mesh **owned**, the audit rows that
//!   mesh's own stored references reach, the container's origin, the session
//!   generation, and the presentation decisions still open
//!   ([`MeshPresentationUnknown`]). The payload borrows nothing, so a catalog
//!   and its session can be dropped while it is in flight.
//!
//! # Splitting
//!
//! One [`RenderVertex`] exists per distinct
//! `(position index, normal index, uv, color, material)` tuple (F10
//! non-negotiable #3). Stored values are compared bit-exactly
//! ([`f32::to_bits`], no tolerance), so two corners that share a position
//! index but differ in UV, color, normal or material become two vertices and
//! an authored UV seam stays visible. Two corners with *different* position
//! indices are never welded, even when their coordinates are equal, because
//! the index is part of the key.
//!
//! # Stored material groups
//!
//! The CS GameZ layout does not store one material and one UV set per polygon.
//! It stores one **material group** per polygon, each with its own raw material
//! index and its own per-corner UV set, and `mat_count` is 1, 2 or 3. F10-A's
//! [`RawMesh`] mirrors only the first group onto its single-valued fields, so
//! [`RenderMesh::build`] — the single-group reading — is exact for a mesh whose
//! polygons each stored one group and is a first-group *view* of a multi-group
//! one.
//!
//! [`RenderMesh::from_stored_groups`] is the reading that loses nothing: it
//! takes [`cs_formats::gamez::GameZMesh::material_groups`] and draws **every**
//! group, one set of triangles each, carrying that group's own raw material index
//! and that group's own stored UVs. A corner two groups store differently is
//! therefore two render vertices, so a seam authored in the second group of a
//! polygon is visible rather than a value that never reaches the mesh.
//! [`MeshContainer::open`] uses it, so a catalog row and an upload payload both
//! see the whole table.
//!
//! What that does **not** settle is how the original renderer presented a
//! multi-group polygon — one group or all of them, in what order, and whether a
//! later group covered an earlier one. Drawing them all is a faithful reading,
//! not a claim about the original, so
//! [`MeshPresentationUnknown::MultiMaterialGroup`] is on every row and payload
//! whose mesh really stored such a polygon, and
//! [`MeshFaceCounts::multi_material_group_polygons`] is the count. Measured:
//! 1 006 of the installation's 128 734 stored polygons, all of them in the eight
//! world archives and none in `ZBD/planes.zbd`; the design is in
//! `docs/findings/2026-09-29-f10-e-material-groups-into-the-render-mesh.md`.
//!
//! # Source maps
//!
//! Every render vertex names the first `(polygon, corner)` that produced it
//! ([`SourceCorner`]); every render triangle names its `(polygon, step)`
//! ([`SourceTriangle`]). Deduplicated vertices keep their first corner, which
//! is enough to look the authored attributes back up.
//!
//! # Validation gate
//!
//! A mesh whose topology is not complete is refused with
//! [`RenderMeshError::IncompleteTopology`], which names every rejected
//! polygon and its [`cs_formats::gamez::FaceIssue`] code; it is never
//! silently dropped (F10 non-negotiable #4). A topology that does not have
//! one status per polygon is refused as
//! [`RenderMeshError::TopologyFaceCount`]. Stored attribute indices are
//! re-checked while resolving, so an out-of-range index is
//! [`RenderMeshError::OutOfRange`] rather than an out-of-bounds index.
//!
//! # Materials and flags
//!
//! Triangles are grouped by their raw, unresolved material index
//! ([`RenderGroup`]). Raw polygon flags are never read: two meshes that
//! differ only in a polygon's `raw_flags` build the same render mesh.
//!
//! # The dependency audit
//!
//! A GameZ material record stores a **number**, not a name: an index into its
//! container's texture-name table. [`MeshDependencyAudit`] is the only place
//! that number becomes a name, and the name becomes an origin:
//!
//! * the archive to search is the caller's, through [`DependencyContext`]. The
//!   audit never picks one, never tries a second and never substitutes a
//!   default;
//! * the name is compared exactly — no case folding, no extension stripping, no
//!   alias, per the IDENTITY-CONTENT lookup contract's "no filename guessing"
//!   rule;
//! * a material index outside the material table is **reported**, never clamped
//!   to the last record and never wrapped;
//! * every row is kept, including the failures (IDENTITY-CONTENT: "collections
//!   cannot exclude failed entries"), and each row carries the contract's
//!   `dependencies`, `parse_state`, `normalize_state`, `readiness`,
//!   `unsupported_reasons` and `fingerprint` fields.
//!
//! # Blocking reasons are codes
//!
//! `unsupported_reasons` is a field a consumer **groups rows by**, so every
//! entry in it is a code: the same cause is the same string whatever bytes
//! caused it. What the bytes were is [`MaterialRow::reason_details`], which
//! keeps one line per finding and de-duplicates nothing, and — for the polygons
//! that store more than one material group — the count on
//! [`MeshFaceCounts::multi_material_group_polygons`].
//!
//! The distinction is not cosmetic. The measured `planes.zbd` stores
//! `bldhwk_cowling..tif` at 36 texture-table positions, so a reason that
//! carried the name and the positions was 242 bytes and **differed per row**:
//! two rows refusing that one cause would not have compared equal, and a
//! container storing the name more often produced a longer "code" for the same
//! reason. The codes are the `pub const` [`CONTAINER_DUPLICATE_NAME`], the
//! [`MeshPresentationUnknown::code`]s and the plain state codes beside them, so
//! a consumer matches on a closed vocabulary.
//!
//! The design is in
//! `docs/findings/2026-09-29-f10-c-integration-and-reason-codes.md`.
//!
//! The measured consequence of the exact-name rule is in
//! `docs/findings/2026-09-29-f10-c-02-gamez-material-records.md`: a GameZ
//! container spells a texture `Sky1.tif` while the world's texture archive
//! stores `sky1`, so the row is `MissingTexture`. That is the honest answer for
//! the rule as written, and the rule is not quietly relaxed here to make the
//! number smaller.
//!
//! # Degenerate triangles
//!
//! Degenerate triangles — two equal *position indices* — are **kept**, marked
//! with [`RenderTriangle::degenerate`]. F10-A's rule is that nothing is
//! dropped, and keeping them makes the source maps and the face, triangle and
//! degenerate counts exact without extra bookkeeping. A consumer that draws
//! may skip them; the render mesh does not hide them.
//!
//! # Error propagation
//!
//! A reader failure reaches the catalog row with its own context, not flattened
//! into a message: [`MeshFailure`] keeps the container the reader's parse
//! context named, the logical field inside it (a `ParseError::field`, already
//! scoped, e.g. `gamez.meshes.polygon.color.b`) and the absolute container
//! offset the read anchored at. A check that happens *after* the bytes were read
//! — a header chain, a family, the render mesh's validation gate — reports the
//! offset as `None`, because no read failed and none is invented.
//!
//! # What this stage does not do
//!
//! There is **no** `crates/cs_app/src/mesh.rs` here. The canonical-mesh-to-Bevy
//! adapter is F17-B's (`specs/F17-…`, stage `### F17-B`, owner path
//! `crates/cs_app/src/render/`), and adding one now would pre-empt it. This
//! stage stops at the content-side upload boundary, exactly as F08-C stopped at
//! the image upload boundary, and the boundary is the whole deliverable: the
//! design is in
//! `docs/findings/2026-09-29-f10-c-03-mesh-container-catalog-and-upload.md`.
//!
//! The design decisions, the recorded unknowns (front-face winding, the UV
//! convention, the corner-colour meaning and the presentation of a multi-group
//! polygon are all still unknown) and the test inventory are in
//! `docs/findings/2026-09-29-f10-c-01-render-vertex-splitting.md`,
//! `docs/findings/2026-09-29-f10-c-02-gamez-material-records.md`,
//! `docs/findings/2026-09-29-f10-c-03-mesh-container-catalog-and-upload.md`,
//! `docs/findings/2026-09-29-f10-c-integration-and-reason-codes.md` and
//! `docs/findings/2026-09-29-f10-e-material-groups-into-the-render-mesh.md`.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use cs_assets::install::sha256;
use cs_assets::vfs::{ContentSession, ResolutionTrace, SessionGeneration};
use cs_assets::zbd::{ZbdContainer, ZbdError};
use cs_formats::ParseContext;
use cs_formats::gamez::materials::{GameZMaterialError, GameZMaterials, MaterialKind, RawMaterial};
use cs_formats::gamez::{
    FaceStatus, GameZError, GameZHeader, GameZMeshes, MeshTopology, RawMaterialGroup, RawMesh,
    RawPolygon, read_gamez_materials, read_gamez_meshes,
};
use cs_formats::zbd::ZbdFamily;
use cs_types::asset_id::{AssetKey, AssetVariant, MountId, SourceSpan};
use cs_types::evidence::ContentHash;
use cs_types::install::{ParseState, RelativePath};

use crate::textures::{TextureAttempt, TextureCatalog, TextureId, TextureRef, TextureResolveError};

/// A `(polygon, corner)` location in the stored [`RawMesh`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceCorner {
    /// Polygon in stored order.
    pub polygon: usize,
    /// Corner of that polygon in stored order.
    pub corner: usize,
}

/// A `(polygon, step)` location of one [`cs_formats::gamez::MeshTriangle`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceTriangle {
    /// Polygon in stored order.
    pub polygon: usize,
    /// Strip step, or the triangle's place in a polygon's triangulation.
    pub step: usize,
}

/// One render vertex: the values a renderer needs plus the raw indices it
/// was split on and its first source corner.
///
/// A vertex is split by material as well as by the per-corner attributes, so
/// `material` and `normal_index`/`position_index` are always consistent with
/// the tuple it was keyed on. `normal`, `uv` and `color` are the stored
/// values, unresolved: no normalization, V flip, wrap, clamp or color-space
/// change happens here.
///
/// `material` and `uv` are the ones of the stored material **group**
/// [`Self::group`] of the source polygon, which is not necessarily its first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderVertex {
    /// Position resolved from [`Self::position_index`].
    pub position: [f32; 3],
    /// Normal resolved from [`Self::normal_index`], unchanged and unnormalized.
    pub normal: Option<[f32; 3]>,
    /// Texture coordinate as stored, of material group [`Self::group`].
    pub uv: Option<[f32; 2]>,
    /// Corner color as stored. Per corner in the layout, so it is the same
    /// value in every group of the polygon.
    pub color: Option<[f32; 3]>,
    /// Raw material index of the group this vertex was split for.
    pub material: u32,
    /// Stored material group of the source polygon this vertex was split for.
    /// `0` is the polygon's first stored group, and it is the only group of
    /// every polygon in the measured corpus except 1 006 of them (all of those
    /// in the world archives).
    ///
    /// This is the group of [`Self::source`], i.e. of the **first** group that
    /// reached the vertex. The group *index* is not part of the vertex key, so
    /// two groups that store the same material and the same coordinate for a
    /// corner are one vertex, and this field then names only the first of them;
    /// [`RenderTriangle::group`] is the field that says which group a triangle
    /// is the drawing of. Nothing is lost by the sharing, because a shared
    /// vertex's `uv` and `material` are the two groups' identical values.
    pub group: usize,
    /// Stored position index (part of the vertex key).
    pub position_index: u32,
    /// Stored normal index (part of the vertex key).
    pub normal_index: Option<u32>,
    /// First source corner that produced this vertex. The group its `uv` and
    /// `material` came from is [`Self::group`], exactly as they are.
    pub source: SourceCorner,
}

/// One render triangle: three vertex indices and where it came from.
///
/// Degenerate triangles are kept; [`Self::degenerate`] records the stored
/// position-index degeneracy so a consumer can skip them.
///
/// A stored polygon that kept `n` material groups produces `n` of these per
/// topology triangle, one per group. They share their positions and their
/// corner colours and differ in the group's own material and UV set, and
/// [`Self::group`] says which group a triangle is the drawing of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderTriangle {
    /// Indices into [`RenderMesh::vertices`], in drawing order.
    pub vertices: [u32; 3],
    /// Source triangle in the stored topology. Several triangles share it when
    /// the polygon kept more than one material group; [`Self::group`] separates
    /// them.
    pub source: SourceTriangle,
    /// Stored material group of the source polygon this triangle is the
    /// drawing of. `0` is the polygon's first stored group.
    pub group: usize,
    /// Raw material index of [`Self::group`], unresolved.
    pub material: u32,
    /// Two of the three stored position indices are equal. The same in every
    /// group of the polygon: it is a property of the stored indices, not of the
    /// group.
    pub degenerate: bool,
}

/// The triangles of one raw material index, in source order.
///
/// The material index is kept raw and unresolved: the material record layout
/// is unknown (F10 non-negotiable #5), so every distinct stored index is just
/// its own group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderGroup {
    /// Stored material index.
    pub material: u32,
    /// Indices into [`RenderMesh::triangles`], ascending.
    pub triangles: Vec<usize>,
}

/// One rejected face named by the validation gate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedFace {
    /// Polygon in stored order.
    pub polygon: usize,
    /// The `FaceIssue` code of the rejection.
    pub code: &'static str,
}

/// Why a render mesh was not built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderMeshError {
    /// The supplied topology does not have one face status per polygon.
    TopologyFaceCount {
        /// Polygons in the raw mesh.
        polygons: usize,
        /// Face statuses supplied.
        faces: usize,
    },
    /// The topology is incomplete. Every rejected polygon is named by its
    /// `FaceIssue` code; none is silently dropped.
    IncompleteTopology {
        /// The rejected faces, in polygon order.
        rejected: Vec<RejectedFace>,
    },
    /// A stored index the render mesh needs is outside its array.
    OutOfRange {
        /// Polygon the index was reached through.
        polygon: usize,
        /// Corner of that polygon, when the index is a per-corner attribute.
        corner: Option<usize>,
        /// `"position"`, `"normal"`, `"polygon"` or `"corner"`.
        field: &'static str,
        /// The stored index.
        index: u64,
        /// Entries in the array.
        available: usize,
    },
    /// More render vertices than a `u32` index can address.
    TooManyVertices {
        /// Vertices already built.
        vertices: usize,
    },
    /// The material-group table does not have one entry per stored polygon, so
    /// one polygon would have no group and another one it did not store.
    GroupCount {
        /// Polygons in the raw mesh.
        polygons: usize,
        /// Group-table entries supplied.
        groups: usize,
    },
    /// A stored material group does not hold exactly one texture coordinate per
    /// corner of its polygon.
    GroupCornerCount {
        /// Polygon in stored order.
        polygon: usize,
        /// Material group of that polygon, in stored order.
        group: usize,
        /// Corners the polygon stores.
        corners: usize,
        /// Coordinates that group stores.
        uvs: usize,
    },
    /// A stored polygon kept no material group at all, so it has no material
    /// index to draw. The reference asserts `mat_count > 0`; the reader keeps
    /// the face and reports it as
    /// [`cs_formats::gamez::ParseFinding::PolygonWithoutMaterial`] instead of
    /// inventing a group, and this is where that decision costs the mesh.
    PolygonWithoutMaterialGroup {
        /// Polygon in stored order.
        polygon: usize,
    },
}

impl fmt::Display for RenderMeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TopologyFaceCount { polygons, faces } => write!(
                f,
                "topology has {faces} face statuses for {polygons} polygons"
            ),
            Self::IncompleteTopology { rejected } => {
                write!(
                    f,
                    "incomplete topology: {} rejected face(s)",
                    rejected.len()
                )?;
                for face in rejected {
                    write!(f, "; polygon {}: {}", face.polygon, face.code)?;
                }
                Ok(())
            }
            Self::OutOfRange {
                polygon,
                corner,
                field,
                index,
                available,
            } => {
                write!(f, "polygon {polygon}")?;
                if let Some(corner) = corner {
                    write!(f, " corner {corner}")?;
                }
                write!(f, " {field} index {index} out of range (0..{available})")
            }
            Self::TooManyVertices { vertices } => {
                write!(f, "{vertices} render vertices exceed the u32 index range")
            }
            Self::GroupCount { polygons, groups } => {
                write!(
                    f,
                    "{groups} material-group entries for {polygons} stored polygons"
                )
            }
            Self::GroupCornerCount {
                polygon,
                group,
                corners,
                uvs,
            } => write!(
                f,
                "polygon {polygon} material group {group} stores {uvs} coordinates for {corners} \
                 corners"
            ),
            Self::PolygonWithoutMaterialGroup { polygon } => {
                write!(f, "polygon {polygon} stored no material group")
            }
        }
    }
}

impl std::error::Error for RenderMeshError {}

/// A canonical, Bevy-free render mesh built from a [`RawMesh`], its
/// [`MeshTopology`] and the stored material groups of every polygon.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderMesh {
    vertices: Vec<RenderVertex>,
    triangles: Vec<RenderTriangle>,
    groups: Vec<RenderGroup>,
    source_faces: usize,
    degenerate_triangles: usize,
    extra_group_triangles: usize,
    extra_group_degenerate: usize,
}

/// Where a render mesh's per-corner material and texture coordinate come from.
///
/// The CS GameZ layout stores **one UV set and one material index per material
/// group** of a stored polygon, and the measured installation stores a polygon
/// with one, two or three of them. [`RawMesh`] mirrors only the first group onto
/// its single-valued fields, so these two sources differ exactly in what a
/// multi-group polygon would lose.
enum GroupSource<'a> {
    /// The IR's own single-valued fields: exactly one group per polygon, the one
    /// `read_polygon` mirrored onto [`RawPolygon::material`] and
    /// [`RawCorner::uv`]. This is the whole of the geometry for a mesh whose
    /// polygons each stored one group, and a single-material *view* of the rest.
    IrOnly,
    /// Every stored group of every stored polygon, in stored order. This is the
    /// authority, and it loses nothing.
    Stored(&'a [Vec<RawMaterialGroup>]),
}

impl RenderMesh {
    /// Builds the render mesh from `mesh`, computing its topology first.
    ///
    /// This is the **single-group** reading: every polygon is drawn once, with
    /// the material and the per-corner texture coordinate the IR carries, which
    /// `read_polygon` mirrored from the polygon's **first** stored material
    /// group. It is exact for a mesh whose polygons each stored one group. For a
    /// mesh read out of a container, use [`Self::from_stored_groups`]: this
    /// method cannot see a second group, so on a multi-group polygon it is the
    /// first-group view and nothing more.
    ///
    /// # Errors
    ///
    /// Any [`RenderMeshError`] [`Self::from_parts`] returns.
    pub fn build(mesh: &RawMesh) -> Result<Self, RenderMeshError> {
        Self::from_parts(mesh, &mesh.topology())
    }

    /// Builds the render mesh from `mesh` and a topology the caller already
    /// has, as [`Self::build`] does. The single-group reading: see
    /// [`Self::from_stored_groups_with_topology`] for the one that keeps every
    /// stored material group.
    ///
    /// `topology` must belong to `mesh`: one face status per polygon, all
    /// decoded. Stored attribute indices are re-checked while resolving, so a
    /// topology that claims a face decoded while the mesh stores an
    /// out-of-range index is refused instead of indexed.
    ///
    /// # Errors
    ///
    /// [`RenderMeshError::TopologyFaceCount`] when the statuses do not match
    /// the polygons, [`RenderMeshError::IncompleteTopology`] when a face was
    /// rejected, [`RenderMeshError::OutOfRange`] when a stored index is
    /// outside its array, and [`RenderMeshError::TooManyVertices`] when the
    /// vertex count passes the `u32` index range.
    pub fn from_parts(mesh: &RawMesh, topology: &MeshTopology) -> Result<Self, RenderMeshError> {
        Self::resolve(mesh, topology, GroupSource::IrOnly)
    }

    /// Builds the render mesh from `mesh` and **every stored material group** of
    /// its polygons, computing the topology first.
    ///
    /// `groups` is [`cs_formats::gamez::GameZMesh::material_groups`]: one entry
    /// per stored polygon, in stored order, each entry holding the polygon's
    /// stored `mat_count` groups with their own raw material index and their own
    /// UV set. Nothing is merged and nothing is dropped:
    ///
    /// * a polygon that stored `n` groups produces `n` triangles per topology
    ///   triangle, one per group, each naming its group in
    ///   [`RenderTriangle::group`] and carrying that group's own raw material
    ///   index;
    /// * a corner's [`RenderVertex::uv`] is **that group's** stored coordinate,
    ///   and the material is **that group's** raw index, so the vertex key
    ///   splits on a group's own UV set as well as on the position, normal,
    ///   colour and material;
    /// * a corner two groups store differently therefore becomes two render
    ///   vertices, so a seam authored in the second group of a polygon is a
    ///   visible seam rather than a value that never reaches the mesh.
    ///
    /// # Errors
    ///
    /// Any [`RenderMeshError`] [`Self::from_stored_groups_with_topology`]
    /// returns.
    pub fn from_stored_groups(
        mesh: &RawMesh,
        groups: &[Vec<RawMaterialGroup>],
    ) -> Result<Self, RenderMeshError> {
        Self::from_stored_groups_with_topology(mesh, &mesh.topology(), groups)
    }

    /// [`Self::from_stored_groups`] with a topology the caller already has.
    ///
    /// # Errors
    ///
    /// [`RenderMeshError::GroupCount`] when `groups` does not have one entry per
    /// stored polygon, [`RenderMeshError::PolygonWithoutMaterialGroup`] when a
    /// stored polygon kept no group, [`RenderMeshError::GroupCornerCount`] when
    /// a group does not hold one coordinate per corner, and every refusal
    /// [`Self::from_parts`] can return.
    pub fn from_stored_groups_with_topology(
        mesh: &RawMesh,
        topology: &MeshTopology,
        groups: &[Vec<RawMaterialGroup>],
    ) -> Result<Self, RenderMeshError> {
        check_group_table(mesh, groups)?;
        Self::resolve(mesh, topology, GroupSource::Stored(groups))
    }

    /// The one split, for both group sources.
    fn resolve(
        mesh: &RawMesh,
        topology: &MeshTopology,
        source: GroupSource<'_>,
    ) -> Result<Self, RenderMeshError> {
        if topology.faces.len() != mesh.polygons.len() {
            return Err(RenderMeshError::TopologyFaceCount {
                polygons: mesh.polygons.len(),
                faces: topology.faces.len(),
            });
        }
        let rejected: Vec<RejectedFace> = topology
            .faces
            .iter()
            .enumerate()
            .filter_map(|(polygon, face)| match face {
                FaceStatus::Rejected(issue) => Some(RejectedFace {
                    polygon,
                    code: issue.code(),
                }),
                FaceStatus::Decoded { .. } => None,
            })
            .collect();
        if !rejected.is_empty() {
            return Err(RenderMeshError::IncompleteTopology { rejected });
        }

        let mut vertices: Vec<RenderVertex> = Vec::new();
        let mut lookup: HashMap<VertexKey, u32> = HashMap::new();
        let mut triangles: Vec<RenderTriangle> = Vec::new();
        let mut extra_group_triangles = 0usize;
        let mut extra_group_degenerate = 0usize;

        for triangle in &topology.triangles {
            let Some(polygon) = mesh.polygons.get(triangle.polygon) else {
                return Err(RenderMeshError::OutOfRange {
                    polygon: triangle.polygon,
                    corner: None,
                    field: "polygon",
                    index: triangle.polygon as u64,
                    available: mesh.polygons.len(),
                });
            };
            // The stored groups of this polygon, already checked to be non-empty
            // and to hold one coordinate per corner by `check_group_table`, or the
            // single group the IR's own fields describe.
            let mut group = 0usize;
            while let Some(attributes) = source.group(triangle.polygon, polygon, group) {
                let mut indices = [0u32; 3];
                for (slot, &corner) in triangle.corners.iter().enumerate() {
                    let Some(raw) = polygon.corners.get(corner) else {
                        return Err(RenderMeshError::OutOfRange {
                            polygon: triangle.polygon,
                            corner: Some(corner),
                            field: "corner",
                            index: corner as u64,
                            available: polygon.corners.len(),
                        });
                    };
                    let position = stored(
                        &mesh.positions,
                        raw.position,
                        triangle.polygon,
                        corner,
                        "position",
                    )?;
                    let normal = match raw.normal {
                        Some(index) => Some(stored(
                            &mesh.normals,
                            index,
                            triangle.polygon,
                            corner,
                            "normal",
                        )?),
                        None => None,
                    };
                    // This group's own coordinate, which is the only difference
                    // between two corners of two groups of the same polygon.
                    let uv = attributes.uv(corner, raw.uv);
                    let key = VertexKey {
                        position: raw.position,
                        normal: raw.normal,
                        uv: uv.map(|uv| uv.map(f32::to_bits)),
                        color: raw.color.map(|color| color.map(f32::to_bits)),
                        material: attributes.material,
                    };
                    indices[slot] = match lookup.get(&key) {
                        Some(&index) => index,
                        None => {
                            let index = u32::try_from(vertices.len()).map_err(|_| {
                                RenderMeshError::TooManyVertices {
                                    vertices: vertices.len(),
                                }
                            })?;
                            vertices.push(RenderVertex {
                                position: *position,
                                normal: normal.copied(),
                                uv,
                                color: raw.color,
                                material: attributes.material,
                                group,
                                position_index: raw.position,
                                normal_index: raw.normal,
                                source: SourceCorner {
                                    polygon: triangle.polygon,
                                    corner,
                                },
                            });
                            lookup.insert(key, index);
                            index
                        }
                    };
                }
                if group > 0 {
                    extra_group_triangles += 1;
                    if triangle.is_degenerate() {
                        extra_group_degenerate += 1;
                    }
                }
                triangles.push(RenderTriangle {
                    vertices: indices,
                    source: SourceTriangle {
                        polygon: triangle.polygon,
                        step: triangle.step,
                    },
                    group,
                    material: attributes.material,
                    degenerate: triangle.is_degenerate(),
                });
                group += 1;
            }
        }

        let degenerate_triangles = triangles.iter().filter(|t| t.degenerate).count();
        let mut by_material: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        for (index, triangle) in triangles.iter().enumerate() {
            by_material
                .entry(triangle.material)
                .or_default()
                .push(index);
        }
        let groups = by_material
            .into_iter()
            .map(|(material, triangles)| RenderGroup {
                material,
                triangles,
            })
            .collect();

        Ok(Self {
            vertices,
            triangles,
            groups,
            source_faces: mesh.polygons.len(),
            degenerate_triangles,
            extra_group_triangles,
            extra_group_degenerate,
        })
    }

    /// Every render vertex, in first-encounter order.
    #[must_use]
    pub fn vertices(&self) -> &[RenderVertex] {
        &self.vertices
    }

    /// Every render triangle, in topology order, then in stored material-group
    /// order within a topology triangle.
    #[must_use]
    pub fn triangles(&self) -> &[RenderTriangle] {
        &self.triangles
    }

    /// Triangles grouped by raw material index, groups in ascending material
    /// order.
    ///
    /// A material index reached by two different stored groups of two different
    /// polygons is one group here, holding the triangles of both: the raw index
    /// is the only thing this grouping is by.
    #[must_use]
    pub fn groups(&self) -> &[RenderGroup] {
        &self.groups
    }

    /// Stored polygons that fed this mesh. Every one decoded (the gate
    /// refuses an incomplete topology), so this is the exact face count.
    #[must_use]
    pub fn source_faces(&self) -> usize {
        self.source_faces
    }

    /// Stored topology triangles that fed this mesh, degenerate ones included.
    ///
    /// A polygon that kept `n` material groups fed `n` render triangles per
    /// topology triangle, so this is the number of **draws**, not the number of
    /// stored triangles. The stored count is this minus
    /// [`Self::extra_group_triangles`], which is also how a catalog row's
    /// [`MeshFaceCounts::triangles`] relates to it.
    #[must_use]
    pub fn source_triangles(&self) -> usize {
        self.triangles.len()
    }

    /// Triangles drawn for a stored material group beyond the first, degenerate
    /// ones included.
    ///
    /// Zero for a mesh whose polygons each stored one group, which is every
    /// polygon of `ZBD/planes.zbd` and all but 1 006 of the installation's
    /// 128 734 stored polygons. It is the exact cost of a render mesh that
    /// keeps every group: those triangles carry the second and later authored
    /// UV sets and the second and later raw material indices, which a
    /// single-group reading has nowhere to put.
    #[must_use]
    pub fn extra_group_triangles(&self) -> usize {
        self.extra_group_triangles
    }

    /// Of [`Self::extra_group_triangles`], the degenerate ones.
    ///
    /// Degeneracy is a property of the stored position indices, so a degenerate
    /// step is degenerate in **every** group of its polygon and the two counts
    /// stand or fall together. It is stated separately because a consumer that
    /// skips degenerate draws needs to know how many of them it is skipping
    /// beyond the stored ones, not only the total.
    #[must_use]
    pub fn extra_group_degenerate_triangles(&self) -> usize {
        self.extra_group_degenerate
    }

    /// Render triangles that draw nothing, because two of their three stored
    /// position indices are equal. They are kept; a consumer that draws may skip
    /// them.
    ///
    /// A mesh that keeps every stored material group has one of these per
    /// group, so this is the stored count plus
    /// [`Self::extra_group_degenerate_triangles`].
    #[must_use]
    pub fn degenerate_triangles(&self) -> usize {
        self.degenerate_triangles
    }
}

impl GroupSource<'_> {
    /// One of a stored polygon's material groups, as the splitter sees it, or
    /// `None` when that polygon has no further group.
    ///
    /// Group `0` is always `Some`: [`Self::IrOnly`] describes exactly one group,
    /// and [`check_group_table`] has already refused a stored table with a
    /// polygon that kept none. Iterated by index rather than collected into a
    /// `Vec`, because the mesh build walks this once per stored topology
    /// triangle and the table itself is the authority on how many there are.
    fn group<'a>(
        &'a self,
        polygon: usize,
        stored: &'a RawPolygon,
        group: usize,
    ) -> Option<GroupAttributes<'a>> {
        match self {
            Self::IrOnly => (group == 0).then_some(GroupAttributes {
                material: stored.material,
                uvs: None,
            }),
            Self::Stored(table) => table[polygon].get(group).map(|attributes| GroupAttributes {
                material: attributes.material,
                uvs: Some(attributes.uvs.as_slice()),
            }),
        }
    }
}

/// One stored material group, as the splitter sees it.
struct GroupAttributes<'a> {
    /// The group's own raw material index.
    material: u32,
    /// The group's own UV set, one coordinate per corner, or `None` for the
    /// IR-only source, where the coordinate comes from the corner itself.
    uvs: Option<&'a [[f32; 2]]>,
}

impl GroupAttributes<'_> {
    /// The coordinate this group stores for `corner`, or the corner's own IR
    /// value when the source has no group table. A stored group always holds one
    /// coordinate per corner — [`check_group_table`] has said so — so a stored
    /// group never falls back to the IR's value.
    fn uv(&self, corner: usize, ir: Option<[f32; 2]>) -> Option<[f32; 2]> {
        match self.uvs {
            Some(uvs) => uvs.get(corner).copied(),
            None => ir,
        }
    }
}

/// Refuses a material-group table that does not describe the mesh's polygons.
///
/// One place, so the invariant [`GroupSource::groups`] and the indexing in
/// [`RenderMesh::resolve`] rely on is stated once: one entry per stored
/// polygon, at least one group each, and exactly one stored coordinate per
/// corner of that group. The reader satisfies all three by construction — the
/// table is built beside the polygons from the same `corners` and `mat_count` —
/// so a refusal here is a caller handing a hand-built table, and it is named
/// rather than indexed.
fn check_group_table(
    mesh: &RawMesh,
    table: &[Vec<RawMaterialGroup>],
) -> Result<(), RenderMeshError> {
    if table.len() != mesh.polygons.len() {
        return Err(RenderMeshError::GroupCount {
            polygons: mesh.polygons.len(),
            groups: table.len(),
        });
    }
    for (polygon, (stored, groups)) in mesh.polygons.iter().zip(table).enumerate() {
        if groups.is_empty() {
            return Err(RenderMeshError::PolygonWithoutMaterialGroup { polygon });
        }
        for (group, attributes) in groups.iter().enumerate() {
            if attributes.uvs.len() != stored.corners.len() {
                return Err(RenderMeshError::GroupCornerCount {
                    polygon,
                    group,
                    corners: stored.corners.len(),
                    uvs: attributes.uvs.len(),
                });
            }
        }
    }
    Ok(())
}

/// The bit-exact identity of one render vertex.
///
/// The group index is deliberately **not** part of the key: two groups that store
/// the same material index and the same UV for a corner are the same vertex, and
/// the triangles that reach it say which group they are the drawing of. What must
/// be in the key is the *resolved* material and UV, which is what `uv` and
/// `material` are — taken from the group being built, not from the polygon's
/// first group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct VertexKey {
    position: u32,
    normal: Option<u32>,
    uv: Option<[u32; 2]>,
    color: Option<[u32; 3]>,
    material: u32,
}

fn stored<'a, T>(
    array: &'a [T],
    index: u32,
    polygon: usize,
    corner: usize,
    field: &'static str,
) -> Result<&'a T, RenderMeshError> {
    array
        .get(index as usize)
        .ok_or(RenderMeshError::OutOfRange {
            polygon,
            corner: Some(corner),
            field,
            index: u64::from(index),
            available: array.len(),
        })
}

// ------------------------------------------------- the dependency audit ---

/// The consumer every audited material row feeds.
pub const MATERIAL_CONSUMER: &str = "mesh_material_binding";

/// Always `"material"`, the IDENTITY-CONTENT catalog `kind` for these rows.
pub const MATERIAL_KIND: &str = "material";

/// Whether a row's dependencies all reached exactly one origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DependencyReadiness {
    /// The material record exists, names a texture, and that name is stored
    /// exactly once in the named archive.
    Ready,
    /// At least one step did not. [`MaterialRow::unsupported_reasons`] says
    /// which; the row itself is still present.
    Blocked,
}

/// Where one stored material reference sits, so an audit row can name the exact
/// places that depend on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MaterialReference {
    /// The mesh record's own 12-byte material reference list, at this position.
    MeshRecord {
        /// Position in that list.
        position: usize,
    },
    /// One stored polygon material group.
    PolygonGroup {
        /// Polygon in stored order.
        polygon: usize,
        /// Group within that polygon.
        group: usize,
    },
}

/// One stored reference that reaches a material.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MaterialUse {
    /// Mesh array index.
    pub mesh: u32,
    /// Where in that mesh the reference is stored.
    pub reference: MaterialReference,
}

/// What happened when one material's texture dependency was followed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MaterialState {
    /// The material record exists, names a texture, and that name is stored
    /// exactly once in the named archive. This is the only state with an
    /// origin.
    Resolved {
        /// The one texture the name reached.
        texture: TextureId,
    },
    /// The material names a texture, and the named archive does not store that
    /// name. The row carries the **exact** stored name and the **exact** archive
    /// that was searched. No other archive is searched, no case is folded, no
    /// extension is stripped and no default is substituted.
    MissingTexture {
        /// The name the container stores, exactly.
        name: String,
        /// The archive that was searched, exactly.
        archive: AssetKey,
    },
    /// The named archive stores the name more than once, so there is no single
    /// origin. Both entry indices are reported rather than one being picked.
    DuplicateTexture {
        /// The name the container stores, exactly.
        name: String,
        /// The archive that was searched, exactly.
        archive: AssetKey,
        /// Every table position in that archive holding the name, ascending.
        entries: Vec<usize>,
    },
    /// The material record names a texture index the container's texture table
    /// does not have. The reference asserts this cannot happen; the record is
    /// still reported, with no name.
    TextureIndexOutOfRange {
        /// The stored index.
        index: u32,
        /// Entries in the container's texture table.
        available: u32,
    },
    /// A stored material index is outside the container's material table. It is
    /// reported and never clamped to the last record and never wrapped.
    MaterialIndexOutOfRange {
        /// The stored index.
        material: u32,
        /// Present material records.
        count: u32,
    },
    /// The material record exists and is not textured: it is a flat colour and
    /// has no texture dependency at all. Not a failure.
    Untextured,
    /// The material record has flag bits the reference's own `MaterialFlags`
    /// does not name, so even whether the record is textured at all is not
    /// established. The raw record is still on the row.
    UnknownField {
        /// The unmapped flag bits.
        bits: u8,
    },
    /// The archive the caller named is not in the catalog, or it failed to
    /// open, so no lookup was possible.
    ArchiveUnavailable {
        /// The archive the caller named.
        archive: AssetKey,
        /// The catalog's error code for it.
        code: String,
    },
}

impl MaterialState {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Resolved { .. } => "resolved",
            Self::MissingTexture { .. } => "missing_texture",
            Self::DuplicateTexture { .. } => "duplicate_texture",
            Self::TextureIndexOutOfRange { .. } => "texture_index_out_of_range",
            Self::MaterialIndexOutOfRange { .. } => "material_index_out_of_range",
            Self::Untextured => "untextured",
            Self::UnknownField { .. } => "unknown_field",
            Self::ArchiveUnavailable { .. } => "archive_unavailable",
        }
    }

    /// The texture this dependency reached, when it reached one.
    pub fn texture(&self) -> Option<&TextureId> {
        match self {
            Self::Resolved { texture } => Some(texture),
            _ => None,
        }
    }

    /// Whether the dependency reached exactly one stored texture.
    pub fn is_resolved(&self) -> bool {
        matches!(self, Self::Resolved { .. })
    }

    /// Whether the row is **complete**: the dependency reached one origin, or
    /// there was no texture to reach.
    ///
    /// An untextured material is a flat colour and names no texture, so its
    /// dependency audit is finished and its row is ready. That is different from
    /// a missing texture, which is an unmet dependency.
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Resolved { .. } | Self::Untextured)
    }
}

impl fmt::Display for MaterialState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            Self::Resolved { texture } => write!(f, "{code}: {texture}"),
            Self::MissingTexture { name, archive } => {
                write!(f, "{code}: {archive} stores no `{name}`")
            }
            Self::DuplicateTexture {
                name,
                archive,
                entries,
            } => write!(f, "{code}: {archive} stores `{name}` at {entries:?}"),
            Self::TextureIndexOutOfRange { index, available } => {
                write!(f, "{code}: texture {index} of {available}")
            }
            Self::MaterialIndexOutOfRange { material, count } => {
                write!(f, "{code}: material {material} of {count}")
            }
            Self::Untextured => write!(f, "{code}: the material is a flat colour"),
            Self::UnknownField { bits } => {
                write!(f, "{code}: the material has flag bits 0x{bits:02X}")
            }
            Self::ArchiveUnavailable { archive, code } => {
                write!(f, "archive_unavailable: {archive} is {code}")
            }
        }
    }
}

/// One row of the material dependency audit: the contract's catalog element for
/// a material a mesh needs, and the state of its texture dependency.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialRow {
    /// Stable id: the material's array index inside its container.
    pub id: String,
    /// Always [`MATERIAL_KIND`].
    pub kind: &'static str,
    /// The stored material index this row is about.
    pub material: u32,
    /// Where the container came from, when the caller knows.
    pub origin: Option<SourceSpan>,
    /// The archives this row depends on. One, the caller's: the audit never
    /// widens the search.
    pub dependencies: Vec<AssetKey>,
    /// Whether the material record was read. A material index outside the table
    /// has no record, so it is `Failed` with the diagnostic.
    pub parse_state: ParseState,
    /// Whether the dependency reached a usable origin. A row that did not is
    /// `Failed` with the same diagnostic as its state.
    pub normalize_state: ParseState,
    /// The consumers this row feeds.
    pub runtime_consumers: Vec<&'static str>,
    /// Readiness.
    pub readiness: DependencyReadiness,
    /// Stable codes of everything that keeps the row from being ready, in
    /// discovery order and without a code twice.
    ///
    /// Every entry is a **code**: a consumer matches on it, and the same cause
    /// is the same string whatever the bytes that caused it were. The text
    /// saying what those bytes were is [`Self::reason_details`], and it is
    /// never folded into a code — a code carrying a texture name and a table
    /// index list would differ per row, would not compare equal across rows
    /// that share a cause, and would grow without bound.
    pub unsupported_reasons: Vec<String>,
    /// One line per blocking **finding**, in the same discovery order as
    /// [`Self::unsupported_reasons`] but **not** de-duplicated: two findings of
    /// the same code with different stored values are two lines, because the
    /// values are the evidence.
    ///
    /// The two lists are deliberately not parallel and must not be zipped: every
    /// line here has a code in `unsupported_reasons`, one code can have many
    /// lines, and a code can have none, because a blocked state that raised no
    /// separate reason contributes [`MaterialState::code`] on its own.
    pub reason_details: Vec<String>,
    /// SHA-256 of the record's 40 stored bytes plus its two link words, in
    /// stored order. `None` when the material index is outside the table, where
    /// there are no stored bytes to hash.
    pub fingerprint: Option<ContentHash>,
    /// The state of the dependency.
    pub state: MaterialState,
    /// Every stored reference that reached this material.
    pub used_by: Vec<MaterialUse>,
    /// The record's ten stored words, raw, when the row has a record. The
    /// field the reference calls `specular` and newer classification calls soil
    /// is [`RawMaterial::record`]`::field32` and is **not** interpreted.
    pub record: Option<RawMaterial>,
}

/// The outcome of auditing one container's meshes.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshDependencyAudit {
    /// The container the audited meshes came from, as a provenance label.
    pub container: String,
    /// The archive every material's texture name was looked for in.
    pub archive: AssetKey,
    /// One row per distinct stored material index the meshes reference, in
    /// ascending index order. Failed rows are here too.
    pub rows: Vec<MaterialRow>,
    /// Every stored material reference the meshes carry, over both levels.
    pub references: usize,
    /// Rows whose dependency reached exactly one stored texture.
    pub resolved: usize,
    /// Rows that did not, and why they are still present: one line per blocked
    /// row's state and one per blocking reason, in prose rather than as codes.
    pub blocked: Vec<String>,
}

impl MeshDependencyAudit {
    /// Audits every material index `meshes` stores, resolving each through
    /// `materials` and then through `context`'s archive.
    ///
    /// Nothing here can fail: a dependency that does not resolve is a row with a
    /// state, not an error, because a caller has to be able to enumerate the
    /// whole set and see which parts are missing.
    #[must_use]
    pub fn build(
        meshes: &GameZMeshes,
        materials: &GameZMaterials,
        context: &DependencyContext<'_>,
    ) -> Self {
        // Which stored references reach which material index, in a deterministic
        // order, so two audits of the same bytes produce the same rows.
        let mut uses: BTreeMap<u32, Vec<MaterialUse>> = BTreeMap::new();
        let mut references = 0usize;
        for mesh in meshes.present() {
            for (position, info) in mesh.materials.iter().enumerate() {
                references += 1;
                uses.entry(info.material_index)
                    .or_default()
                    .push(MaterialUse {
                        mesh: mesh.index,
                        reference: MaterialReference::MeshRecord { position },
                    });
            }
            for (polygon, groups) in mesh.material_groups.iter().enumerate() {
                for (group, entry) in groups.iter().enumerate() {
                    references += 1;
                    uses.entry(entry.material).or_default().push(MaterialUse {
                        mesh: mesh.index,
                        reference: MaterialReference::PolygonGroup { polygon, group },
                    });
                }
            }
        }

        // Container-level facts that several rows share. A texture name the
        // container stores more than once is a property of the container, not of
        // one material, so it is a reason on the rows that use the name rather
        // than a second state.
        let duplicated: BTreeMap<String, Vec<u32>> =
            materials.duplicate_names().into_iter().collect();
        let archive_state = archive_state(context);

        let mut rows = Vec::with_capacity(uses.len());
        let mut resolved = 0usize;
        let mut blocked = Vec::new();
        for (index, used_by) in uses {
            let row = audit_material(
                index,
                &used_by,
                materials,
                context,
                &duplicated,
                archive_state.as_ref(),
            );
            if row.state.is_resolved() && row.readiness == DependencyReadiness::Ready {
                resolved += 1;
            } else {
                // This list is prose for a human, not a code list, so it names
                // the **detail** lines where the row has them: a bare code would
                // say only what went wrong and not which stored bytes did it.
                blocked.push(format!("material {index}: {}", row.state));
                if row.reason_details.is_empty() {
                    for reason in &row.unsupported_reasons {
                        blocked.push(format!("material {index}: {reason}"));
                    }
                } else {
                    for detail in &row.reason_details {
                        blocked.push(format!("material {index}: {detail}"));
                    }
                }
            }
            rows.push(row);
        }
        let blocked = blocked.into_iter().fold(Vec::new(), |mut kept, entry| {
            if !kept.contains(&entry) {
                kept.push(entry);
            }
            kept
        });

        Self {
            container: context.container.to_owned(),
            archive: context.archive.clone(),
            rows,
            references,
            resolved,
            blocked,
        }
    }

    /// The rows whose dependency reached a stored texture, in index order.
    pub fn resolved_rows(&self) -> impl Iterator<Item = &MaterialRow> {
        self.rows.iter().filter(|row| row.state.is_resolved())
    }

    /// The rows that did not, in index order.
    pub fn blocked_rows(&self) -> impl Iterator<Item = &MaterialRow> {
        self.rows.iter().filter(|row| !row.state.is_resolved())
    }

    /// The stored material indices the audit reports as out of range.
    pub fn out_of_range(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.rows.iter().filter_map(|row| match &row.state {
            MaterialState::MaterialIndexOutOfRange { material, count } => Some((*material, *count)),
            _ => None,
        })
    }
}

/// What the audit needs from its caller.
///
/// The archive is the caller's decision on purpose: which texture archive a
/// mission or a world uses is not established (F08-C records that as an open
/// question), and the audit must not answer it by searching. A caller with
/// several archives in mind runs the audit once per archive and compares.
#[derive(Clone, Debug)]
pub struct DependencyContext<'a> {
    /// The archive every texture name is looked for in.
    pub archive: &'a AssetKey,
    /// The session that read the catalog, for its generation check.
    pub session: &'a ContentSession,
    /// The catalog that answers the name lookups.
    pub catalog: &'a TextureCatalog,
    /// Where the container came from, when the caller knows.
    pub origin: Option<SourceSpan>,
    /// A provenance label for the container, never a path that gets joined.
    pub container: &'a str,
}

/// The catalog's own verdict on the caller's archive: `None` when it opened.
fn archive_state(context: &DependencyContext<'_>) -> Option<MaterialState> {
    let session_error = context
        .catalog
        .generation()
        .ne(&context.session.generation());
    if session_error {
        return Some(MaterialState::ArchiveUnavailable {
            archive: context.archive.clone(),
            code: "foreign_session".to_owned(),
        });
    }
    for (key, error) in context.catalog.failures() {
        if key == context.archive {
            return Some(MaterialState::ArchiveUnavailable {
                archive: context.archive.clone(),
                code: error.code().to_owned(),
            });
        }
    }
    let known = context
        .catalog
        .archives()
        .any(|archive| archive.key() == context.archive);
    if !known {
        return Some(MaterialState::ArchiveUnavailable {
            archive: context.archive.clone(),
            code: "archive_not_catalogued".to_owned(),
        });
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn audit_material(
    index: u32,
    used_by: &[MaterialUse],
    materials: &GameZMaterials,
    context: &DependencyContext<'_>,
    duplicated: &BTreeMap<String, Vec<u32>>,
    archive_state: Option<&MaterialState>,
) -> MaterialRow {
    // The codes are what a consumer groups by, so they carry no data; the lines
    // below carry the data, and both are kept so nothing measured is dropped.
    let mut reasons: Vec<String> = Vec::new();
    let mut details: Vec<String> = Vec::new();
    for finding in materials
        .findings
        .iter()
        .filter(|finding| finding.material() == index)
    {
        reasons.push(finding.code().to_owned());
        details.push(finding.to_string());
    }
    let count = materials.count();
    let record = materials.material(index).cloned();
    let row = MaterialRow {
        id: format!("gamez.materials[{index}]"),
        kind: MATERIAL_KIND,
        material: index,
        origin: context.origin.clone(),
        dependencies: vec![context.archive.clone()],
        parse_state: ParseState::Unparsed,
        normalize_state: ParseState::Unparsed,
        runtime_consumers: vec![MATERIAL_CONSUMER],
        readiness: DependencyReadiness::Blocked,
        unsupported_reasons: Vec::new(),
        reason_details: Vec::new(),
        fingerprint: record
            .as_ref()
            .map(|material| sha256(&material_bytes(material))),
        state: MaterialState::Untextured,
        used_by: used_by.to_vec(),
        record,
    };

    let Some(material) = materials.material(index) else {
        let state = MaterialState::MaterialIndexOutOfRange {
            material: index,
            count,
        };
        return finish(row, false, state, reasons, details);
    };
    let bits = material.record.unknown_flag_bits();
    if bits != 0 {
        return finish(
            row,
            true,
            MaterialState::UnknownField { bits },
            reasons,
            details,
        );
    }
    if material.kind() == MaterialKind::Colored {
        return finish(row, true, MaterialState::Untextured, reasons, details);
    }
    let Some(texture) = materials.texture(material.record.texture_index) else {
        let state = MaterialState::TextureIndexOutOfRange {
            index: material.record.texture_index,
            available: materials.textures.len() as u32,
        };
        return finish(row, true, state, reasons, details);
    };
    if let Some(indices) = duplicated.get(&texture.name) {
        // The code names the cause; the name and the table positions go in the
        // detail line, because the measured corpus stores one name at 36 table
        // positions and that list is evidence, not a reason code.
        reasons.push(CONTAINER_DUPLICATE_NAME.to_owned());
        details.push(format!(
            "the container stores `{}` at table indices {indices:?}",
            texture.name
        ));
    }
    if let Some(state) = archive_state {
        return finish(row, true, state.clone(), reasons, details);
    }
    let reference = TextureRef::new(context.archive.clone(), &texture.name);
    match context.catalog.resolve(context.session, &reference) {
        Ok(resolved) => {
            let state = MaterialState::Resolved {
                texture: resolved.id().clone(),
            };
            finish(row, true, state, reasons, details)
        }
        Err(error) => {
            reasons.push(error.code().to_owned());
            details.push(error.to_string());
            let state = match &error {
                TextureResolveError::Duplicate { attempts, .. } => {
                    let mut entries = Vec::new();
                    for attempt in attempts {
                        if let TextureAttempt::Name { entries: found, .. } = attempt {
                            entries = found.clone();
                        }
                    }
                    MaterialState::DuplicateTexture {
                        name: texture.name.clone(),
                        archive: context.archive.clone(),
                        entries,
                    }
                }
                other if other.code() == "texture_not_found" => MaterialState::MissingTexture {
                    name: texture.name.clone(),
                    archive: context.archive.clone(),
                },
                other => MaterialState::ArchiveUnavailable {
                    archive: context.archive.clone(),
                    code: other.code().to_owned(),
                },
            };
            finish(row, true, state, reasons, details)
        }
    }
}

/// Fills in the four fields that follow from the state, and returns the row.
///
/// `has_record` says whether the material table holds a record for this index at
/// all: a stored index outside the table is a **failed parse**, while every other
/// state is a record that was read and whose dependency did or did not reach an
/// origin.
///
/// `reasons` are stable codes and `details` the matching evidence lines. The
/// codes are de-duplicated **as a set**, not by collapsing neighbours: the same
/// cause reached through two findings is one code, and `Vec::dedup` would keep
/// a second copy of it whenever a different code sits between the two. Every
/// detail line is kept, because two findings of one code with different stored
/// values are two pieces of evidence.
fn finish(
    mut row: MaterialRow,
    has_record: bool,
    state: MaterialState,
    reasons: Vec<String>,
    details: Vec<String>,
) -> MaterialRow {
    let mut codes: Vec<String> = Vec::new();
    for code in reasons {
        if !codes.contains(&code) {
            codes.push(code);
        }
    }
    if !state.is_complete() && codes.is_empty() {
        codes.push(state.code().to_owned());
    }
    let ready = state.is_complete() && codes.is_empty();
    // The diagnostic prefers a detail line, because it names the stored bytes
    // rather than only naming the cause; the state's own text is the fallback
    // for a blocked state that raised no separate reason.
    let diagnostic = details
        .first()
        .cloned()
        .unwrap_or_else(|| state.to_string());
    row.parse_state = if has_record {
        ParseState::Parsed
    } else {
        ParseState::Failed {
            diagnostic: state.to_string(),
        }
    };
    row.normalize_state = if ready {
        ParseState::Parsed
    } else {
        ParseState::Failed { diagnostic }
    };
    row.readiness = if ready {
        DependencyReadiness::Ready
    } else {
        DependencyReadiness::Blocked
    };
    row.unsupported_reasons = codes;
    row.reason_details = details;
    row.state = state;
    row
}

/// The stored bytes a material's fingerprint is taken over: the record's ten
/// words in stored order, then its two link words. Exactly the bytes the layout
/// stores, so two materials that differ anywhere differ here.
fn material_bytes(material: &RawMaterial) -> Vec<u8> {
    let record = &material.record;
    let mut out = Vec::with_capacity(44);
    out.push(record.alpha);
    out.push(record.flags);
    out.extend_from_slice(&record.rgb.to_le_bytes());
    for value in record.color {
        out.extend_from_slice(&value.to_bits().to_le_bytes());
    }
    out.extend_from_slice(&record.texture_index.to_le_bytes());
    out.extend_from_slice(&record.field20.to_bits().to_le_bytes());
    out.extend_from_slice(&record.field24.to_bits().to_le_bytes());
    out.extend_from_slice(&record.field28.to_bits().to_le_bytes());
    out.extend_from_slice(&record.field32.to_bits().to_le_bytes());
    out.extend_from_slice(&record.cycle_ptr.to_le_bytes());
    out.extend_from_slice(&material.link1.to_le_bytes());
    out.extend_from_slice(&material.link2.to_le_bytes());
    debug_assert_eq!(
        out.len(),
        44,
        "a material slot is forty bytes and two words"
    );
    out
}

// ============================================ the container, catalog, upload ===

/// The code for a texture name the **container itself** stores more than once.
///
/// It is a bare code: which name, and which table positions hold it, are
/// evidence and live in [`MaterialRow::reason_details`], because the measured
/// `planes.zbd` stores `bldhwk_cowling..tif` at 36 positions — a code carrying
/// them was 242 bytes of data on every row that shares the cause, and two rows
/// sharing the cause would not have compared equal.
pub const CONTAINER_DUPLICATE_NAME: &str = "container_texture_name_duplicated";

/// Always `"render_mesh"`: the IDENTITY-CONTENT catalog `kind` for a GameZ
/// render mesh ("render meshes/materials/images").
pub const RENDER_MESH_KIND: &str = "render_mesh";

/// The consumer every render-mesh row feeds: the upload payload a renderer
/// adapter turns into a GPU mesh. F17-B owns that adapter; this slice stops at
/// the boundary, exactly as F08-C stopped at the image upload boundary.
pub const MESH_UPLOAD_CONSUMER: &str = "mesh_upload";

/// Stable identity of one stored mesh inside one GameZ container.
///
/// `index` is the array position a scene node's `mesh_index` refers to
/// (`NodeCsC.mesh_index`, offset 60 of the 208-byte node record), so this id is
/// the join key between the node array and the render mesh. Two archives that
/// both store mesh 7 give two ids that never compare equal.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MeshId {
    /// Installation-relative path of the container that stores it.
    pub container: RelativePath,
    /// The mount that served the container.
    pub mount: MountId,
    /// The variant of the key the container was resolved with.
    pub variant: AssetVariant,
    /// Position in the container's mesh array.
    pub index: u32,
}

impl fmt::Display for MeshId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}#{} ({}, variant {})",
            self.container, self.index, self.mount, self.variant
        )
    }
}

/// Which stage refused a container or a mesh, and so where a failure's
/// container, member and offset came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MeshFailureStage {
    /// The VFS resolution, the read or the ZBD dispatch failed, so no reader
    /// ever saw the bytes.
    Container,
    /// The container opened, but was routed to another ZBD family.
    Family,
    /// The 40-byte container header the two section readers read disagreed.
    Header,
    /// The mesh section ([`read_gamez_meshes`]) refused the container.
    Meshes,
    /// The material section ([`read_gamez_materials`]) refused the container.
    Materials,
    /// One mesh's stored polygons did not survive the render mesh's validation
    /// gate, so the mesh produced no render mesh at all.
    Render,
}

impl MeshFailureStage {
    /// Stable lowercase identifier, used as a catalog unsupported reason.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Container => "container_failed",
            Self::Family => "wrong_family",
            Self::Header => "header_disagreement",
            Self::Meshes => "mesh_section_failed",
            Self::Materials => "material_section_failed",
            Self::Render => "render_mesh_refused",
        }
    }
}

impl fmt::Display for MeshFailureStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// One refused read, with the reader's own context kept rather than flattened
/// into a message.
///
/// The three context fields are what F10-C.03 has to carry to the catalog row:
/// `container` is the label the reader's own parse context carries,
/// `member` is the logical field inside that container the read was reaching
/// (a `cs_formats::ParseError::field`, e.g. `mesh.0.polygons.1.uvs`), and
/// `offset` is the absolute container offset the read anchored at. A failure
/// with no offset says so with `None`; no offset is invented for a check that
/// happens after the bytes were read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeshFailure {
    /// Which stage refused.
    pub stage: MeshFailureStage,
    /// The reader's stable code, e.g. `unexpected_eof` or `parse`.
    pub code: String,
    /// The container the reader was reading, as its own parse context names it.
    pub container: String,
    /// The reader's logical field scope inside the container, when it named
    /// one. `None` for a check that is not anchored at a field.
    pub member: Option<String>,
    /// The absolute container offset the failure is anchored at, when the
    /// reader named one.
    pub offset: Option<u64>,
    /// The mesh array index, when the failure is about one stored mesh.
    pub mesh: Option<u32>,
    /// The reader's own message, offsets and scope included.
    pub diagnostic: String,
}

impl fmt::Display for MeshFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.stage.code(), self.code)?;
        if let Some(mesh) = self.mesh {
            write!(f, " (mesh {mesh})")?;
        }
        write!(f, " at {}", self.container)?;
        if let Some(member) = &self.member {
            write!(f, " field {member}")?;
        }
        if let Some(offset) = self.offset {
            write!(f, " offset {offset}")?;
        }
        Ok(())
    }
}

/// The reader-specific reason a GameZ container produced no render meshes.
#[derive(Debug)]
pub enum MeshContainerErrorKind {
    /// Resolving, reading or dispatching the container failed, so no reader
    /// saw its bytes.
    Container(ZbdError),
    /// The container is routed to another ZBD family and is not read as GameZ.
    WrongFamily {
        /// The key that was opened.
        key: AssetKey,
        /// The family the dispatch named.
        family: ZbdFamily,
    },
    /// The mesh section refused the container.
    Meshes(GameZError),
    /// The material section refused the container.
    Materials(GameZMaterialError),
    /// The two section readers read the same 40 header bytes and did not agree.
    HeaderDisagreement {
        /// Which header word differs.
        field: &'static str,
        /// The value the mesh reader read.
        meshes: u32,
        /// The value the material reader read.
        materials: u32,
    },
}

impl fmt::Display for MeshContainerErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Container(error) => write!(f, "{error}"),
            Self::WrongFamily { key, family } => write!(
                f,
                "{key} is routed to the {} family, not to GameZ containers",
                family.as_str()
            ),
            Self::Meshes(error) => write!(f, "{error}"),
            Self::Materials(error) => write!(f, "{error}"),
            Self::HeaderDisagreement {
                field,
                meshes,
                materials,
            } => write!(
                f,
                "the two readers disagree on header {field}: {meshes} and {materials}"
            ),
        }
    }
}

impl MeshContainerErrorKind {
    fn stage(&self) -> MeshFailureStage {
        match self {
            Self::Container(_) => MeshFailureStage::Container,
            Self::WrongFamily { .. } => MeshFailureStage::Family,
            Self::HeaderDisagreement { .. } => MeshFailureStage::Header,
            Self::Meshes(_) => MeshFailureStage::Meshes,
            Self::Materials(_) => MeshFailureStage::Materials,
        }
    }

    fn code(&self) -> String {
        match self {
            Self::Container(error) => error.code().to_owned(),
            Self::WrongFamily { .. } => MeshFailureStage::Family.code().to_owned(),
            Self::Meshes(error) => error.code().to_owned(),
            Self::Materials(error) => error.code().to_owned(),
            Self::HeaderDisagreement { .. } => MeshFailureStage::Header.code().to_owned(),
        }
    }
}

/// Why a GameZ container did not become a set of render meshes, with the
/// reader's own container, member field and byte offset kept.
///
/// This is one type rather than an error enum plus a separate context struct so
/// a catalog row cannot be built without the context: the context and the reason
/// are constructed together in [`MeshContainer::open`] and are inseparable
/// afterwards.
///
/// The three payloads are boxed so a `Result` carrying this error stays small
/// enough to return from [`MeshContainer::open`] by value.
#[derive(Debug)]
pub struct MeshContainerError {
    kind: Box<MeshContainerErrorKind>,
    origin: Option<Box<SourceSpan>>,
    failure: Box<MeshFailure>,
}

impl MeshContainerError {
    /// The reader-specific reason.
    pub fn kind(&self) -> &MeshContainerErrorKind {
        self.kind.as_ref()
    }

    /// The origin of the container's bytes, when the container was read. `None`
    /// only for a resolution, read or dispatch failure, where no bytes exist to
    /// point at.
    pub fn origin(&self) -> Option<&SourceSpan> {
        self.origin.as_deref()
    }

    /// The failure, with its container, member field and offset.
    pub fn failure(&self) -> &MeshFailure {
        self.failure.as_ref()
    }

    /// Stable lowercase identifier: the reader's own code, or this stage's for
    /// a family or header failure the readers never raised.
    pub fn code(&self) -> &str {
        &self.failure.code
    }
}

impl fmt::Display for MeshContainerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.failure.fmt(f)
    }
}

impl std::error::Error for MeshContainerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self.kind.as_ref() {
            MeshContainerErrorKind::Container(error) => Some(error),
            MeshContainerErrorKind::Meshes(error) => Some(error),
            MeshContainerErrorKind::Materials(error) => Some(error),
            MeshContainerErrorKind::WrongFamily { .. }
            | MeshContainerErrorKind::HeaderDisagreement { .. } => None,
        }
    }
}

/// The label a failure's container field carries.
///
/// A reader's validation variants name no container of their own — a header is
/// refused on its own words — so the VFS label of the container they were handed
/// is the fallback. `<unnamed>` would hide which container failed, and the row
/// has to name it.
fn labelled<'a>(reader: &'a str, fallback: &'a str) -> &'a str {
    if reader.is_empty() { fallback } else { reader }
}

/// The reader's own context, extracted once so both section errors report it the
/// same way.
fn reader_context(
    container: &str,
    offset: Option<u64>,
    field: Option<String>,
) -> (String, Option<u64>, Option<String>) {
    let labelled = if container.is_empty() {
        "<unnamed>".to_owned()
    } else {
        container.to_owned()
    };
    (labelled, offset, field)
}

/// Builds the container failure: the reason, the origin when the bytes exist,
/// and the reader's container, member field and offset.
fn container_error(
    kind: MeshContainerErrorKind,
    origin: Option<SourceSpan>,
    context: Option<(String, Option<u64>, Option<String>)>,
) -> MeshContainerError {
    let (container, offset, member) =
        context.unwrap_or_else(|| ("<unresolved>".to_owned(), None, None));
    let diagnostic = kind.to_string();
    let failure = MeshFailure {
        stage: kind.stage(),
        code: kind.code(),
        container,
        member,
        offset,
        mesh: None,
        diagnostic,
    };
    MeshContainerError {
        kind: Box::new(kind),
        origin: origin.map(Box::new),
        failure: Box::new(failure),
    }
}

/// The first header word the two section readers did not agree on.
///
/// Both readers read the same 40 bytes from the same container, so a
/// disagreement is not a format variant: it means one reader was handed
/// something the other was not, and the container is refused instead of having
/// one reader's view of it win.
fn header_disagreement(
    meshes: &GameZHeader,
    materials: &GameZHeader,
) -> Option<(&'static str, u32, u32)> {
    let pairs: [(&'static str, u32, u32); 10] = [
        ("signature", meshes.signature, materials.signature),
        ("version", meshes.version, materials.version),
        ("unk08", meshes.unk08, materials.unk08),
        (
            "texture_count",
            meshes.texture_count,
            materials.texture_count,
        ),
        (
            "textures_offset",
            meshes.textures_offset,
            materials.textures_offset,
        ),
        (
            "materials_offset",
            meshes.materials_offset,
            materials.materials_offset,
        ),
        (
            "meshes_offset",
            meshes.meshes_offset,
            materials.meshes_offset,
        ),
        (
            "node_array_size",
            meshes.node_array_size,
            materials.node_array_size,
        ),
        ("light_index", meshes.light_index, materials.light_index),
        ("nodes_offset", meshes.nodes_offset, materials.nodes_offset),
    ];
    pairs.into_iter().find(|(_, first, second)| first != second)
}

/// Which texture archive a mesh container's material audit searches, and the
/// catalog that answers lookups in it.
///
/// The archive is the caller's decision, for the reason
/// [`MeshDependencyAudit`] documents: which archive a world or a plane uses is
/// not established, and the audit must not answer it by searching. A catalog
/// opened with an archive the [`TextureCatalog`] does not hold is not a special
/// case — every material row becomes `archive_not_catalogued`, which is the
/// honest state.
#[derive(Clone, Copy, Debug)]
pub struct MeshDependencies<'a> {
    /// The archive every material's texture name is looked for in.
    pub archive: &'a AssetKey,
    /// The catalog that answers the name lookups.
    pub textures: &'a TextureCatalog,
}

/// One GameZ container read through a content session: the container's owned
/// bytes, the two sections the F10-B/F10-C.02 readers produced from them, and
/// the render mesh of every present stored mesh.
#[derive(Clone, Debug)]
pub struct MeshContainer {
    container: ZbdContainer,
    trace: ResolutionTrace,
    meshes: GameZMeshes,
    materials: GameZMaterials,
    render: Vec<Option<Result<RenderMesh, RenderMeshError>>>,
    topologies: Vec<Option<MeshTopology>>,
    audit: MeshDependencyAudit,
    /// Which audit rows each array slot's own stored references reach, as
    /// indices into [`Self::audit`]. Built once so a caller that wants one
    /// mesh's rows does not scan the container's whole audit for every row.
    audit_rows_by_mesh: Vec<Vec<usize>>,
}

impl MeshContainer {
    /// Resolves `key` in `session`, opens the container it names, checks the
    /// ZBD dispatch routed it to [`ZbdFamily::GameZ`], reads the mesh and
    /// material sections with the production readers and builds a render mesh
    /// for every present stored mesh.
    ///
    /// The container **owns** its bytes, so everything it produced stays usable
    /// after `session` is closed; it is stamped with the session generation that
    /// read it so a later session can refuse it.
    ///
    /// A mesh whose stored polygons do not survive [`RenderMesh`]'s validation
    /// gate does not fail the container: it is kept as
    /// `Some(Err(..))` in its own array slot and becomes a failed catalog row,
    /// because a sibling mesh of the same container may be complete.
    ///
    /// # Errors
    ///
    /// [`MeshContainerErrorKind::Container`] when the key does not resolve to
    /// exactly one origin, cannot be read or is not routed at all;
    /// [`MeshContainerErrorKind::WrongFamily`] when it is routed elsewhere;
    /// [`MeshContainerErrorKind::Meshes`] or
    /// [`MeshContainerErrorKind::Materials`] when a section reader refuses it;
    /// and [`MeshContainerErrorKind::HeaderDisagreement`] when the two readers
    /// read the same 40 header bytes and disagree.
    pub fn open(
        session: &ContentSession,
        key: &AssetKey,
        dependencies: &MeshDependencies<'_>,
    ) -> Result<Self, MeshContainerError> {
        let resolved = session.resolve(key).map_err(|error| {
            container_error(
                MeshContainerErrorKind::Container(ZbdError::from(error)),
                None,
                None,
            )
        })?;
        let trace = resolved.resolved().trace.clone();
        let container = ZbdContainer::open(session, key).map_err(|error| {
            container_error(MeshContainerErrorKind::Container(error), None, None)
        })?;
        if container.family() != ZbdFamily::GameZ {
            let key = key.clone();
            let family = container.family();
            let span = container.span().clone();
            let label = container.label().to_owned();
            return Err(container_error(
                MeshContainerErrorKind::WrongFamily { key, family },
                Some(span),
                Some(reader_context(&label, None, None)),
            ));
        }

        let label = container.label().to_owned();
        let span = container.span().clone();
        let bytes = container.bytes();
        // One parse context for both sections, as the two readers are meant to be
        // used: the label is the parse's own, and a failed attempt leaves the
        // allocation ledger untouched so the next reader starts clean.
        let mut parse = ParseContext::with_defaults(label.clone());
        let meshes = read_gamez_meshes(&mut parse, &label, bytes).map_err(|error| {
            let context = reader_context(
                labelled(error.container(), &label),
                error.offset(),
                parse_field(&error),
            );
            container_error(
                MeshContainerErrorKind::Meshes(error),
                Some(span.clone()),
                Some(context),
            )
        })?;
        let materials = read_gamez_materials(&mut parse, &label, bytes).map_err(|error| {
            let context = materials_context(&error, &label);
            container_error(
                MeshContainerErrorKind::Materials(error),
                Some(span.clone()),
                Some(context),
            )
        })?;
        if let Some((field, first, second)) = header_disagreement(&meshes.header, &materials.header)
        {
            return Err(container_error(
                MeshContainerErrorKind::HeaderDisagreement {
                    field,
                    meshes: first,
                    materials: second,
                },
                Some(span.clone()),
                Some(reader_context(&label, None, None)),
            ));
        }

        let mut render = Vec::with_capacity(meshes.meshes.len());
        let mut topologies = Vec::with_capacity(meshes.meshes.len());
        for slot in &meshes.meshes {
            match slot {
                None => {
                    render.push(None);
                    topologies.push(None);
                }
                Some(mesh) => {
                    // Every stored material group, not just the first: the CS
                    // layout stores one UV set and one material index per group,
                    // and `RawMesh` mirrors only the first onto its own fields.
                    render.push(Some(RenderMesh::from_stored_groups(
                        &mesh.mesh,
                        &mesh.material_groups,
                    )));
                    topologies.push(Some(mesh.topology()));
                }
            }
        }

        let audit = MeshDependencyAudit::build(
            &meshes,
            &materials,
            &DependencyContext {
                archive: dependencies.archive,
                session,
                catalog: dependencies.textures,
                origin: Some(span),
                container: container.path().as_str(),
            },
        );

        let mut audit_rows_by_mesh: Vec<Vec<usize>> = vec![Vec::new(); meshes.meshes.len()];
        for (position, row) in audit.rows.iter().enumerate() {
            for use_ in &row.used_by {
                if let Some(rows) = audit_rows_by_mesh.get_mut(use_.mesh as usize) {
                    rows.push(position);
                }
            }
        }
        for rows in &mut audit_rows_by_mesh {
            rows.dedup();
        }

        Ok(Self {
            container,
            trace,
            meshes,
            materials,
            render,
            topologies,
            audit,
            audit_rows_by_mesh,
        })
    }

    /// The audit rows one stored mesh's own material references reach, in
    /// ascending material index order.
    pub fn audit_rows_for(&self, index: u32) -> impl Iterator<Item = &MaterialRow> {
        self.audit_rows_by_mesh
            .get(index as usize)
            .into_iter()
            .flatten()
            .map(|&at| &self.audit.rows[at])
    }

    /// The key the container was resolved with.
    pub const fn key(&self) -> &AssetKey {
        self.container.key()
    }

    /// The installation-relative path of the container.
    pub const fn path(&self) -> &RelativePath {
        self.container.path()
    }

    /// The immutable origin of the container's bytes.
    pub const fn span(&self) -> &SourceSpan {
        self.container.span()
    }

    /// The session generation that read the container.
    pub const fn generation(&self) -> SessionGeneration {
        self.container.generation()
    }

    /// The provenance label the VFS and the readers both carry.
    pub fn label(&self) -> &str {
        self.container.label()
    }

    /// The container's own bytes, as the mount read them. A mesh's stored span
    /// is a range of these, so a caller can see exactly what was hashed.
    pub fn container_bytes(&self) -> &[u8] {
        self.container.bytes()
    }

    /// The VFS attempts that chose this container.
    pub const fn trace(&self) -> &ResolutionTrace {
        &self.trace
    }

    /// The parsed mesh section.
    pub const fn meshes(&self) -> &GameZMeshes {
        &self.meshes
    }

    /// The parsed material section.
    pub const fn materials(&self) -> &GameZMaterials {
        &self.materials
    }

    /// The dependency audit of this container's stored material references.
    pub const fn audit(&self) -> &MeshDependencyAudit {
        &self.audit
    }

    /// The identity of the mesh at array position `index`, whether or not that
    /// slot is present. The identity is a property of the container and the
    /// position, not of the render mesh, so a failed slot still has one.
    pub fn id(&self, index: u32) -> MeshId {
        MeshId {
            container: self.path().clone(),
            mount: self.container.mount().clone(),
            variant: self.key().variant().clone(),
            index,
        }
    }

    /// Every present stored mesh's identity, in array order.
    pub fn ids(&self) -> impl Iterator<Item = MeshId> + '_ {
        self.meshes.present().map(|mesh| self.id(mesh.index))
    }

    /// The render mesh of one array slot: `None` for an absent slot, `Some` of
    /// the build result for a present one.
    pub fn render(&self, index: u32) -> Option<Result<&RenderMesh, &RenderMeshError>> {
        let built = self.render.get(index as usize)?.as_ref()?;
        Some(built.as_ref())
    }

    /// The topology report of one array slot, so a refused mesh can still state
    /// its exact face, triangle and rejected-face counts.
    pub fn topology(&self, index: u32) -> Option<&MeshTopology> {
        self.topologies.get(index as usize)?.as_ref()
    }

    /// The exact face accounting of one array slot.
    pub fn faces(&self, index: u32) -> Option<MeshFaceCounts> {
        let mesh = self.meshes.get(index)?;
        let topology = self.topology(index)?;
        let multi = mesh
            .material_groups
            .iter()
            .filter(|groups| groups.len() > 1)
            .count();
        Some(MeshFaceCounts {
            faces: mesh.mesh.polygons.len(),
            triangles: topology.triangles.len(),
            rejected: topology
                .faces
                .iter()
                .filter(|face| matches!(face, FaceStatus::Rejected(_)))
                .count(),
            degenerate: topology
                .triangles
                .iter()
                .filter(|triangle| triangle.is_degenerate())
                .count(),
            multi_material_group_polygons: multi,
        })
    }
}

/// The `field` a mesh-section failure carried, when it is a parse failure.
fn parse_field(error: &GameZError) -> Option<String> {
    match error {
        GameZError::Parse(error) => Some(error.field.clone()),
        _ => None,
    }
}

fn materials_context(
    error: &GameZMaterialError,
    fallback: &str,
) -> (String, Option<u64>, Option<String>) {
    let field = match error {
        GameZMaterialError::Parse(error) => Some(error.field.clone()),
        _ => None,
    };
    reader_context(labelled(error.container(), fallback), error.offset(), field)
}

/// The exact face accounting of one stored mesh, for a row that has one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshFaceCounts {
    /// Stored polygons the mesh holds. Every one of them decoded when the
    /// render mesh was built, so this is also the exact valid face count.
    pub faces: usize,
    /// Topology triangles the stored polygons produced, degenerate ones
    /// included.
    pub triangles: usize,
    /// Stored polygons the validation gate rejected, named by their
    /// `FaceIssue` code in [`RenderMeshError::IncompleteTopology`]. Zero for a
    /// mesh that produced a render mesh.
    pub rejected: usize,
    /// Triangles with two equal stored position indices. They are kept; a
    /// consumer that draws may skip them.
    ///
    /// Measured on the installation: `ZBD/C1/gamez.zbd` stores 8 019 of 56 073
    /// (14.3 %) and `ZBD/planes.zbd` stores 10 497 of 71 645 (14.7 %), so the
    /// mark is load-bearing on world geometry and on airframes alike, and not
    /// only on a fixture. Whether the original renderer skipped them is
    /// unmeasured; F17-B's adapter decides.
    pub degenerate: usize,
    /// Stored polygons that stored more than one material group.
    ///
    /// The CS GameZ layout stores one UV set and one material index per group.
    /// [`RawMesh::polygons`] mirrors only the first onto its single-valued
    /// fields, but the render mesh is built from the whole table
    /// ([`RenderMesh::from_stored_groups`]), so **no group is lost**: each one is
    /// drawn as its own triangles with its own stored UVs and its own raw
    /// material index. This count is therefore not a loss count any more — it is
    /// the evidence for the one open question such a polygon raises,
    /// [`MeshPresentationUnknown::MultiMaterialGroup`], and it is on a row and a
    /// payload only when it is not zero.
    ///
    /// Measured on the installation, per archive: `ZBD/C1` 390, `ZBD/C5` 352,
    /// `ZBD/C2` 206, `ZBD/C3` 16, `ZBD/C4` 16, `ZBD/C1C` 10, `ZBD/C1B` 8,
    /// `ZBD/C2B` 8 — 1 006 in the eight world archives, 999 of them storing two
    /// groups and 7 storing three — and `ZBD/planes.zbd` **none**, so no airframe
    /// is affected and this count is non-zero for world geometry only. 307 of the
    /// 17 139 stored meshes hold one. See
    /// `docs/findings/2026-09-29-f10-e-material-groups-into-the-render-mesh.md`.
    pub multi_material_group_polygons: usize,
}

/// How far a render-mesh row got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderMeshReadiness {
    /// Every stored face decoded, every material a flat colour or bound to
    /// exactly one stored texture, and nothing left undecided about
    /// presenting it. The evidence for the last part is not in yet, so no row
    /// reaches this today; see [`MeshPresentationUnknown`].
    Ready,
    /// Every stored face decoded, but something is open: a material's texture
    /// dependency reached no single origin, a polygon stores more than one
    /// material group whose presentation is unmeasured, or another presentation
    /// decision is unmeasured. See [`MeshPresentationUnknown`].
    Blocked,
    /// The mesh was not read, or not turned into a render mesh. The row carries
    /// the reader's own context in [`RenderMeshRecord::failure`].
    Failed,
}

impl RenderMeshReadiness {
    /// Stable lowercase identifier, used as a catalog unsupported reason.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
        }
    }
}

/// A presentation decision the evidence has not settled for one GameZ mesh.
///
/// Each one is a fact about a value [`MeshContainer::open`] hands on untouched:
/// the render mesh resolves stored indices and splits vertices, and does
/// nothing else. The renderer adapter (F17-B) must read
/// [`MeshUpload::unknowns`] before it chooses anything, exactly as F17-B must
/// read [`crate::textures::TextureUpload::unknowns`] for an image.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MeshPresentationUnknown {
    /// Which winding faces the viewer, and therefore which winding is culled.
    /// A render triangle keeps the stored winding; the mesh does not decide
    /// handedness. Carried from F10-A/F10-B and still open in F10-C.01.
    FrontFaceWinding,
    /// The UV convention: no V flip, wrap, clamp or scale is applied to a
    /// stored texture coordinate, and the original renderer's convention is
    /// unmeasured.
    UvOrigin,
    /// What a stored corner colour means. Three raw floats are handed on; their
    /// range, whether they are intensities at all, and whether the original
    /// renderer read them, are unmeasured.
    VertexColor,
    /// How the original renderer presented a stored polygon's second and further
    /// material groups.
    ///
    /// Nothing is lost here, which is what changed: the CS GameZ layout stores
    /// one UV set and one material index per group, and the render mesh draws
    /// **every** group as its own triangles carrying that group's own stored
    /// values. What is unmeasured is what the original renderer did with a
    /// polygon that has two or three of them — whether it drew one group or all
    /// of them, in which order, and whether a later group was meant to cover an
    /// earlier one. Drawing them all is a faithful, lossless reading, not a
    /// claim that the original drew them all.
    ///
    /// On a row or payload only when the mesh really stored such a polygon;
    /// [`MeshFaceCounts::multi_material_group_polygons`] is the count and
    /// [`RenderMesh::extra_group_triangles`] the extra draws.
    MultiMaterialGroup,
}

impl MeshPresentationUnknown {
    /// Stable lowercase identifier, used as a catalog unsupported reason.
    pub const fn code(self) -> &'static str {
        match self {
            Self::FrontFaceWinding => "front_face_winding_unknown",
            Self::UvOrigin => "uv_origin_unknown",
            Self::VertexColor => "vertex_color_unknown",
            Self::MultiMaterialGroup => "multi_material_group_presentation_unknown",
        }
    }
}

impl fmt::Display for MeshPresentationUnknown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// The presentation decisions open for one GameZ mesh.
///
/// The first three are a property of the pipeline, not of one container, so they
/// are one fixed list rather than something recomputed per mesh and able to
/// drift. [`MeshPresentationUnknown::MultiMaterialGroup`] is the exception and is
/// appended only when the mesh's own stored bytes have a polygon with more than
/// one material group — a mesh that stores one group per polygon has no such
/// question, and saying so on its row would be a false claim about the corpus.
fn mesh_presentation_unknowns(faces: Option<&MeshFaceCounts>) -> Vec<MeshPresentationUnknown> {
    let mut unknowns = vec![
        MeshPresentationUnknown::FrontFaceWinding,
        MeshPresentationUnknown::UvOrigin,
        MeshPresentationUnknown::VertexColor,
    ];
    if faces.is_some_and(|faces| faces.multi_material_group_polygons > 0) {
        unknowns.push(MeshPresentationUnknown::MultiMaterialGroup);
    }
    unknowns
}

/// One row of the render-mesh catalog (IDENTITY-CONTENT "required catalog
/// collections": render meshes).
///
/// A row is about one stored mesh, or — when `mesh_index` is `None` and `id` is
/// `None` — about a container that produced no mesh at all. Failed rows are
/// rows: a container that could not be read and a mesh whose faces did not
/// validate both appear here with their reader's context, and neither is
/// dropped.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderMeshRecord {
    /// The mesh, or `None` for a container that produced none.
    pub id: Option<MeshId>,
    /// Always [`RENDER_MESH_KIND`].
    pub kind: &'static str,
    /// The container key the row came from.
    pub container_key: AssetKey,
    /// The origin of the container's bytes, when the container was read.
    pub origin: Option<SourceSpan>,
    /// The container itself, plus the one texture archive the audit searched.
    /// Two, always: a render mesh's material index is a stored number whose only
    /// origin is a texture in a named archive.
    pub dependencies: Vec<AssetKey>,
    /// Whether the container and the stored mesh were read. A mesh whose faces
    /// did not validate *was* read, so it is `Parsed` here and the failure is in
    /// [`Self::normalize_state`].
    pub parse_state: ParseState,
    /// Whether the read mesh became a usable render mesh.
    pub normalize_state: ParseState,
    /// The consumers this row feeds.
    pub runtime_consumers: Vec<&'static str>,
    /// How far the row got.
    pub readiness: RenderMeshReadiness,
    /// Stable codes of everything that keeps the row from being ready, in
    /// discovery order and without a code twice.
    ///
    /// Every entry is a **code**, never a code with the data attached: a
    /// consumer groups rows by this list, so a reason that varied with the
    /// stored bytes would split one cause across many strings. The bytes are
    /// named on the [`MaterialRow`]s the upload carries, in their own
    /// [`MaterialRow::reason_details`].
    pub unsupported_reasons: Vec<String>,
    /// SHA-256 of the mesh's stored data span inside the container, or of
    /// nothing for a container that was never read. The span is exactly the
    /// range the reader walked for that mesh, so two meshes that differ in one
    /// stored word differ here.
    pub fingerprint: Option<ContentHash>,
    /// The mesh array index, or `None` on a row that is about the container.
    pub mesh_index: Option<u32>,
    /// The exact face accounting, when the row is about one stored mesh.
    pub faces: Option<MeshFaceCounts>,
    /// The refusal that produced this row, with the reader's container, member
    /// field and byte offset.
    pub failure: Option<MeshFailure>,
}

/// The process-local serial of the next catalog. A catalog's serial is what
/// binds a [`ResolvedMesh`] to the catalog that produced it, so a resolution
/// handed back to a **sibling** catalog of the same session over the same bytes
/// is refused instead of being served a second time.
static NEXT_CATALOG_SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn next_catalog_serial() -> u64 {
    NEXT_CATALOG_SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// One container slot of a [`MeshCatalog`]: the key, and either the container
/// it read or the failure that replaced it.
#[derive(Debug)]
struct MeshSlot {
    key: AssetKey,
    state: Result<MeshContainer, MeshContainerError>,
}

/// The GameZ containers one session makes available, and the render meshes they
/// hold.
///
/// Modelled on [`crate::textures::TextureCatalog`], because the two have the
/// same shape of problem: a session-scoped set of archives whose failures must
/// stay visible, whose contents must not survive into another session, and
/// whose contents must survive the closing of their own.
///
/// * It owns every container's bytes, so closing the session does not
///   invalidate what was already resolved or uploaded.
/// * It is stamped with the [`SessionGeneration`] that read it:
///   [`Self::resolve`], [`Self::prepare_upload`] and [`Self::retry_failed`]
///   refuse any other session, and a [`ResolvedMesh`] from another catalog of
///   the same session is refused too.
/// * [`Self::retry_failed`] reopens only the failed containers of the same
///   session and keeps the ones that read. Repairing a file does not change what
///   that session mounted, so the retry fails on the mount-time digest; a
///   **remount** of a repaired tree, and a new catalog over it, is what loads
///   the fix.
#[derive(Debug)]
pub struct MeshCatalog {
    serial: u64,
    generation: SessionGeneration,
    archive: AssetKey,
    slots: Vec<MeshSlot>,
}

impl MeshCatalog {
    /// Opens every container key in `session`. A key that fails stays in the
    /// catalog as a failed row; it is never dropped. A repeated key is opened
    /// once. A container whose mesh section reads but whose material section
    /// does not is a failed container: the mesh rows depend on the audit, and
    /// there is no audit to hand them.
    pub fn open(
        session: &ContentSession,
        containers: &[AssetKey],
        dependencies: &MeshDependencies<'_>,
    ) -> Self {
        let mut slots: Vec<MeshSlot> = Vec::new();
        for key in containers {
            if slots.iter().any(|slot| &slot.key == key) {
                continue;
            }
            slots.push(MeshSlot {
                key: key.clone(),
                state: MeshContainer::open(session, key, dependencies),
            });
        }
        Self {
            serial: next_catalog_serial(),
            generation: session.generation(),
            archive: dependencies.archive.clone(),
            slots,
        }
    }

    /// This catalog's process-local serial, which a [`ResolvedMesh`] carries so a
    /// sibling catalog of the same session cannot answer for it.
    pub const fn serial(&self) -> u64 {
        self.serial
    }

    /// The session generation that read the catalog.
    pub const fn generation(&self) -> SessionGeneration {
        self.generation
    }

    /// The one texture archive every audited material was looked for in.
    pub const fn archive(&self) -> &AssetKey {
        &self.archive
    }

    /// Every container that read.
    pub fn containers(&self) -> impl Iterator<Item = &MeshContainer> {
        self.slots
            .iter()
            .filter_map(|slot| slot.state.as_ref().ok())
    }

    /// Every container that failed, with its error.
    pub fn failures(&self) -> impl Iterator<Item = (&AssetKey, &MeshContainerError)> {
        self.slots
            .iter()
            .filter_map(|slot| slot.state.as_ref().err().map(|error| (&slot.key, error)))
    }

    /// Refuses a session that did not read this catalog.
    ///
    /// # Errors
    ///
    /// [`MeshError::ForeignSession`] when the generations differ.
    pub fn require_session(&self, session: &ContentSession) -> Result<(), MeshError> {
        if session.generation() == self.generation {
            Ok(())
        } else {
            Err(MeshError::ForeignSession {
                session: session.generation(),
                catalog: self.generation,
            })
        }
    }

    /// Reopens every failed container in `session`, keeping those that read.
    /// Returns how many are still failing.
    ///
    /// The dependencies are restated because the audit is rebuilt from the
    /// repaired bytes, and the caller — not the catalog — owns which texture
    /// archive it is rebuilt against.
    ///
    /// # Errors
    ///
    /// [`MeshError::ForeignSession`] when `session` is not the one that read the
    /// catalog; a new session needs a new catalog.
    pub fn retry_failed(
        &mut self,
        session: &ContentSession,
        dependencies: &MeshDependencies<'_>,
    ) -> Result<usize, MeshError> {
        self.require_session(session)?;
        for slot in &mut self.slots {
            if slot.state.is_err() {
                slot.state = MeshContainer::open(session, &slot.key, dependencies);
            }
        }
        Ok(self.failures().count())
    }

    fn container(&self, key: &AssetKey) -> Result<&MeshContainer, MeshError> {
        let slot = self
            .slots
            .iter()
            .find(|slot| &slot.key == key)
            .ok_or_else(|| MeshError::ContainerNotCatalogued {
                container: Box::new(key.clone()),
            })?;
        slot.state
            .as_ref()
            .map_err(|error| MeshError::ContainerFailed {
                container: Box::new(key.clone()),
                code: error.failure.code.clone(),
                diagnostic: error.failure.diagnostic.clone(),
            })
    }

    /// Resolves the stored mesh at array position `index` of `key` to its exact
    /// origin, with the VFS trace that chose the container.
    ///
    /// # Errors
    ///
    /// [`MeshError::ForeignSession`] for another session,
    /// [`MeshError::ContainerNotCatalogued`] or
    /// [`MeshError::ContainerFailed`] when the container is not available,
    /// [`MeshError::MeshNotFound`] when the array slot is absent, and
    /// [`MeshError::MeshFailed`] when the slot is present but its stored faces
    /// did not survive the render mesh's validation gate.
    pub fn resolve(
        &self,
        session: &ContentSession,
        key: &AssetKey,
        index: u32,
    ) -> Result<ResolvedMesh, MeshError> {
        self.require_session(session)?;
        let container = self.container(key)?;
        let slot = container
            .render(index)
            .ok_or_else(|| MeshError::MeshNotFound {
                container: Box::new(key.clone()),
                index,
            })?;
        let resolved = ResolvedMesh {
            serial: self.serial,
            generation: self.generation,
            container_key: key.clone(),
            id: container.id(index),
            container_span: container.span().clone(),
            trace: container.trace().clone(),
        };
        match slot {
            Ok(_) => Ok(resolved),
            Err(error) => Err(MeshError::MeshFailed {
                container: Box::new(key.clone()),
                index,
                code: render_mesh_code(error).to_owned(),
                diagnostic: error.to_string(),
            }),
        }
    }

    /// Hands a resolved mesh to the upload boundary.
    ///
    /// The payload **owns** everything it carries: the split render mesh, the
    /// audit rows its stored material references reach, the container's origin
    /// and the session generation that read them. It stays usable after the
    /// catalog and the session are dropped, which is what a renderer adapter
    /// needs while the world it belongs to is being torn down or replaced.
    ///
    /// # Errors
    ///
    /// [`MeshError::ForeignSession`] for another session,
    /// [`MeshError::NotFromThisCatalog`] for a mesh another catalog resolved, or
    /// for one whose container no longer holds it, and
    /// [`MeshError::MeshFailed`] when the stored mesh is present but its faces
    /// did not survive the render gate.
    pub fn prepare_upload(
        &self,
        session: &ContentSession,
        resolved: &ResolvedMesh,
    ) -> Result<MeshUpload, MeshError> {
        self.require_session(session)?;
        let not_ours = || MeshError::NotFromThisCatalog {
            id: Box::new(resolved.id.clone()),
        };
        if resolved.generation != self.generation || resolved.serial != self.serial {
            return Err(not_ours());
        }
        // The container is looked up by the **key** the mesh was resolved with,
        // so the payload always names the container this catalog read for that
        // key, never a sibling slot whose bytes happen to hash the same. The
        // slot must still hold the mesh: a resolution cannot outlive a retry
        // that replaced its container.
        let container = self.container(&resolved.container_key)?;
        if container.span() != &resolved.container_span
            || container.id(resolved.id.index) != resolved.id
        {
            return Err(not_ours());
        }
        let built = container.render(resolved.id.index).ok_or_else(not_ours)?;
        let render = match built {
            Ok(render) => render.clone(),
            Err(error) => {
                return Err(MeshError::MeshFailed {
                    container: Box::new(container.key().clone()),
                    index: resolved.id.index,
                    code: render_mesh_code(error).to_owned(),
                    diagnostic: error.to_string(),
                });
            }
        };
        // Only the audit rows this mesh's own stored references reach: a
        // container's audit covers every mesh in it, and a payload for one mesh
        // must not imply the others' materials.
        let materials: Vec<MaterialRow> = container
            .audit()
            .rows
            .iter()
            .filter(|row| {
                row.used_by
                    .iter()
                    .any(|use_| use_.mesh == resolved.id.index)
            })
            .cloned()
            .collect();
        let faces = container.faces(resolved.id.index).ok_or_else(not_ours)?;
        Ok(MeshUpload {
            id: resolved.id.clone(),
            generation: self.generation,
            container_span: resolved.container_span.clone(),
            container_key: container.key().clone(),
            render,
            materials,
            unknowns: mesh_presentation_unknowns(Some(&faces)),
            faces,
        })
    }

    /// One row per stored mesh of every container that read, and one row per
    /// container that failed, in catalog order and then array order.
    ///
    /// The fingerprint of a mesh row is taken over exactly the stored span the
    /// reader walked for that mesh
    /// ([`cs_formats::gamez::GameZMesh::data_offset`]..
    /// [`cs_formats::gamez::GameZMesh::data_end`]), not over the whole
    /// container, so two meshes of one archive differ if and only if their own
    /// stored bytes differ.
    pub fn records(&self) -> Vec<RenderMeshRecord> {
        let mut records = Vec::new();
        for slot in &self.slots {
            let container = match &slot.state {
                Ok(container) => container,
                Err(error) => {
                    records.push(RenderMeshRecord {
                        id: None,
                        kind: RENDER_MESH_KIND,
                        container_key: slot.key.clone(),
                        origin: error.origin().cloned(),
                        dependencies: vec![slot.key.clone(), self.archive.clone()],
                        parse_state: ParseState::Failed {
                            diagnostic: error.failure.diagnostic.clone(),
                        },
                        normalize_state: ParseState::Unparsed,
                        runtime_consumers: vec![MESH_UPLOAD_CONSUMER],
                        readiness: RenderMeshReadiness::Failed,
                        unsupported_reasons: vec![
                            error.failure.stage.code().to_owned(),
                            error.failure.code.clone(),
                        ],
                        fingerprint: None,
                        mesh_index: None,
                        faces: None,
                        failure: Some(error.failure().clone()),
                    });
                    continue;
                }
            };
            for (index, built) in container.render.iter().enumerate() {
                // An absent array slot stores no mesh, so it is not a row: the
                // collection holds what the container stored, and this one
                // stored nothing there.
                let Some(built) = built else { continue };
                records.push(mesh_record(container, index as u32, built, &self.archive));
            }
        }
        records
    }
}

/// Builds the row of one stored mesh, complete or refused.
fn mesh_record(
    container: &MeshContainer,
    index: u32,
    built: &Result<RenderMesh, RenderMeshError>,
    archive: &AssetKey,
) -> RenderMeshRecord {
    let id = container.id(index);
    let mut row = RenderMeshRecord {
        id: Some(id.clone()),
        kind: RENDER_MESH_KIND,
        container_key: container.key().clone(),
        origin: Some(container.span().clone()),
        dependencies: vec![container.key().clone(), archive.clone()],
        parse_state: ParseState::Parsed,
        normalize_state: ParseState::Unparsed,
        runtime_consumers: vec![MESH_UPLOAD_CONSUMER],
        readiness: RenderMeshReadiness::Failed,
        unsupported_reasons: Vec::new(),
        fingerprint: None,
        mesh_index: Some(index),
        faces: container.faces(index),
        failure: None,
    };
    // The fingerprint is over the stored span, not the re-derived render mesh,
    // so it is available for a refused mesh too and does not depend on this
    // stage's splitting.
    if let Some(mesh) = container.meshes().get(index) {
        let bytes = &container.container_bytes()[mesh.data_offset as usize..mesh.data_end as usize];
        row.fingerprint = Some(sha256(bytes));
    }

    match built {
        Err(error) => {
            let failure = MeshFailure {
                stage: MeshFailureStage::Render,
                code: render_mesh_code(error).to_owned(),
                container: container.label().to_owned(),
                member: None,
                // A render-mesh refusal is a check over bytes that were read
                // whole, so there is no failing read offset to report and none
                // is invented.
                offset: None,
                mesh: Some(index),
                diagnostic: error.to_string(),
            };
            row.normalize_state = ParseState::Failed {
                diagnostic: failure.diagnostic.clone(),
            };
            row.unsupported_reasons = vec![failure.stage.code().to_owned(), failure.code.clone()];
            row.failure = Some(failure);
        }
        Ok(_) => {
            // The reasons are a **set**: the same code reached through two
            // different audit rows is one reason, and `Vec::dedup` would keep a
            // second copy of it whenever the two are not adjacent. Insertion
            // order is kept, so the codes still read in a stable order.
            let mut reasons: Vec<String> = Vec::new();
            let mut reason = |code: String| {
                if !reasons.contains(&code) {
                    reasons.push(code);
                }
            };
            // The audit rows this mesh's own stored references reach, so the row
            // states its own material readiness rather than the container's.
            for material in container.audit_rows_for(index) {
                for code in &material.unsupported_reasons {
                    reason(code.clone());
                }
            }
            // The presentation decisions. The multi-group one is on this row only
            // because the mesh's own stored polygons have one: nothing is lost
            // any more (every stored group is drawn with its own UV set and its
            // own material index), so what the row names is the one thing still
            // undecided about them, and the count is the evidence.
            for unknown in mesh_presentation_unknowns(row.faces.as_ref()) {
                reason(unknown.code().to_owned());
            }
            row.readiness = if reasons.is_empty() {
                RenderMeshReadiness::Ready
            } else {
                RenderMeshReadiness::Blocked
            };
            // The mesh **did** become a render mesh, and `prepare_upload` hands
            // it over. Open presentation decisions and an unresolved texture
            // make the row `Blocked`, not its normalization `Failed`: the same
            // split F08-C makes for a decoded image with open unknowns. Only the
            // `Err` arm above, where no render mesh exists at all, is a
            // normalization failure.
            row.normalize_state = ParseState::Parsed;
            row.unsupported_reasons = reasons;
        }
    }
    row
}

/// Stable lowercase code of a [`RenderMeshError`], for a catalog row's
/// `unsupported_reasons` and for [`MeshError`]'s `code`.
///
/// The code is derived here rather than added to the type so F10-C.01's
/// published error shape does not change.
fn render_mesh_code(error: &RenderMeshError) -> &'static str {
    match error {
        RenderMeshError::TopologyFaceCount { .. } => "topology_face_count",
        RenderMeshError::IncompleteTopology { .. } => "incomplete_topology",
        RenderMeshError::OutOfRange { .. } => "attribute_index_out_of_range",
        RenderMeshError::TooManyVertices { .. } => "too_many_vertices",
        RenderMeshError::GroupCount { .. } => "material_group_count",
        RenderMeshError::GroupCornerCount { .. } => "material_group_corner_count",
        RenderMeshError::PolygonWithoutMaterialGroup { .. } => "polygon_without_material_group",
    }
}

/// One mesh resolved against a catalog: its exact origin, and the catalog and
/// session generation that produced it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedMesh {
    serial: u64,
    generation: SessionGeneration,
    /// The container key the mesh was resolved with. Carried so
    /// [`MeshCatalog::prepare_upload`] serves the **same** container
    /// [`MeshCatalog::resolve`] read, and never a sibling slot whose bytes
    /// happen to hash the same.
    container_key: AssetKey,
    id: MeshId,
    container_span: SourceSpan,
    trace: ResolutionTrace,
}

impl ResolvedMesh {
    /// The mesh's identity.
    pub const fn id(&self) -> &MeshId {
        &self.id
    }

    /// The serial of the catalog that resolved it.
    pub const fn serial(&self) -> u64 {
        self.serial
    }

    /// The container key the mesh was resolved with.
    pub const fn container_key(&self) -> &AssetKey {
        &self.container_key
    }

    /// The origin of the container that stores it.
    pub const fn container_span(&self) -> &SourceSpan {
        &self.container_span
    }

    /// The VFS attempts that chose the container.
    pub const fn trace(&self) -> &ResolutionTrace {
        &self.trace
    }

    /// The session generation that resolved it.
    pub const fn generation(&self) -> SessionGeneration {
        self.generation
    }
}

/// Why a mesh lookup or a hand-off to the upload boundary did not happen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MeshError {
    /// The catalog was read by another session.
    ForeignSession {
        /// The session asking.
        session: SessionGeneration,
        /// The session that read the catalog.
        catalog: SessionGeneration,
    },
    /// The catalog was not opened with this container key.
    ContainerNotCatalogued {
        /// The container key asked for.
        container: Box<AssetKey>,
    },
    /// The container is in the catalog but failed to read.
    ContainerFailed {
        /// The container key asked for.
        container: Box<AssetKey>,
        /// The failure's stable code.
        code: String,
        /// The failure's text, with its container, member field and offset.
        diagnostic: String,
    },
    /// The container's mesh array has no present record at that position.
    MeshNotFound {
        /// The container that was asked.
        container: Box<AssetKey>,
        /// The array position asked for.
        index: u32,
    },
    /// The mesh is present but its stored faces did not survive the render
    /// mesh's validation gate. Nothing is uploaded and nothing is dropped.
    MeshFailed {
        /// The container that holds it.
        container: Box<AssetKey>,
        /// The array position.
        index: u32,
        /// The refusal's stable code.
        code: String,
        /// The refusal's text, naming every rejected face.
        diagnostic: String,
    },
    /// A resolved mesh handed back to this catalog did not come from it.
    NotFromThisCatalog {
        /// The mesh handed in.
        id: Box<MeshId>,
    },
}

impl MeshError {
    /// Stable lowercase identifier.
    pub fn code(&self) -> &str {
        match self {
            Self::ForeignSession { .. } => "foreign_session",
            Self::ContainerNotCatalogued { .. } => "container_not_catalogued",
            Self::ContainerFailed { code, .. } => code,
            Self::MeshNotFound { .. } => "mesh_not_found",
            Self::MeshFailed { code, .. } => code,
            Self::NotFromThisCatalog { .. } => "not_from_this_catalog",
        }
    }
}

impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignSession { session, catalog } => write!(
                f,
                "the mesh catalog was read by {catalog} and cannot serve {session}"
            ),
            Self::ContainerNotCatalogued { container } => {
                write!(f, "GameZ container {container} is not in this catalog")
            }
            Self::ContainerFailed {
                container,
                code,
                diagnostic,
            } => write!(
                f,
                "GameZ container {container} failed ({code}): {diagnostic}"
            ),
            Self::MeshNotFound { container, index } => {
                write!(f, "{container} stores no present mesh at index {index}")
            }
            Self::MeshFailed {
                container,
                index,
                code,
                diagnostic,
            } => write!(
                f,
                "{container} mesh {index} is not uploadable ({code}): {diagnostic}"
            ),
            Self::NotFromThisCatalog { id } => {
                write!(f, "mesh {id} was not resolved by this catalog")
            }
        }
    }
}

impl std::error::Error for MeshError {}

/// What a renderer adapter receives for one mesh: the split render mesh,
/// **owned**, the audit rows its own stored material references reached, and
/// everything still undecided about presenting it.
///
/// The payload borrows nothing. A catalog and the session that read it can both
/// be dropped, and a world switch can build its own catalog, without
/// invalidating a payload that is already on its way to the GPU.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshUpload {
    id: MeshId,
    generation: SessionGeneration,
    container_span: SourceSpan,
    container_key: AssetKey,
    render: RenderMesh,
    materials: Vec<MaterialRow>,
    faces: MeshFaceCounts,
    unknowns: Vec<MeshPresentationUnknown>,
}

impl MeshUpload {
    /// The mesh this payload is for.
    pub const fn id(&self) -> &MeshId {
        &self.id
    }

    /// The session generation that produced it.
    pub const fn generation(&self) -> SessionGeneration {
        self.generation
    }

    /// The origin of the container that stores the mesh.
    pub const fn container_span(&self) -> &SourceSpan {
        &self.container_span
    }

    /// The key the container was resolved with.
    pub const fn container_key(&self) -> &AssetKey {
        &self.container_key
    }

    /// The split render mesh: one vertex per distinct
    /// `(position index, normal index, uv, color, material)` tuple, so an
    /// authored per-corner UV seam is still there.
    pub const fn render(&self) -> &RenderMesh {
        &self.render
    }

    /// The exact face accounting of the stored mesh.
    pub const fn faces(&self) -> MeshFaceCounts {
        self.faces
    }

    /// The audit rows this mesh's own stored material references reach, in
    /// ascending material index order. A material index the container's table
    /// does not hold is a row here, not a missing one.
    pub fn materials(&self) -> &[MaterialRow] {
        &self.materials
    }

    /// The audit row of one stored material index, when this mesh references it.
    pub fn material(&self, material: u32) -> Option<&MaterialRow> {
        self.materials.iter().find(|row| row.material == material)
    }

    /// The presentation decisions still open, in a fixed order.
    pub fn unknowns(&self) -> &[MeshPresentationUnknown] {
        &self.unknowns
    }

    /// Whether nothing is left open for release presentation.
    pub fn is_release_ready(&self) -> bool {
        self.unknowns.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_formats::gamez::{MeshTriangle, PrimitiveKind, RawCorner, RawPolygon};

    /// ```text
    /// 2 (0,1) ---- 3 (1,1)
    ///   |        / |
    ///   |      /   |
    /// 0 (0,0) ---- 1 (1,0)
    /// ```
    const POSITIONS: [[f32; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ];

    const NORMALS: [[f32; 3]; 2] = [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0]];

    fn corner(position: u32) -> RawCorner {
        RawCorner {
            position,
            normal: None,
            uv: None,
            color: None,
        }
    }

    fn triangle(material: u32, corners: Vec<RawCorner>) -> RawPolygon {
        RawPolygon {
            kind: PrimitiveKind::Polygon,
            raw_flags: 0,
            material,
            corners,
        }
    }

    fn strip(material: u32, positions: &[u32]) -> RawPolygon {
        RawPolygon {
            kind: PrimitiveKind::TriangleStrip,
            raw_flags: 0,
            material,
            corners: positions.iter().copied().map(corner).collect(),
        }
    }

    fn mesh(polygons: Vec<RawPolygon>) -> RawMesh {
        RawMesh {
            positions: POSITIONS.to_vec(),
            normals: NORMALS.to_vec(),
            polygons,
        }
    }

    /// The UVs a render triangle samples, by its source polygon, in drawing
    /// order. Independent of how the vertices were split.
    fn uvs_of(render: &RenderMesh, polygon: usize) -> [[f32; 2]; 3] {
        let triangle = render
            .triangles()
            .iter()
            .find(|t| t.source.polygon == polygon)
            .expect("a triangle for that polygon");
        triangle
            .vertices
            .map(|index| render.vertices()[index as usize].uv.expect("authored uv"))
    }

    fn colors_of(render: &RenderMesh, polygon: usize) -> [[f32; 3]; 3] {
        let triangle = render
            .triangles()
            .iter()
            .find(|t| t.source.polygon == polygon)
            .expect("a triangle for that polygon");
        triangle.vertices.map(|index| {
            render.vertices()[index as usize]
                .color
                .expect("authored color")
        })
    }

    fn normals_of(render: &RenderMesh, polygon: usize) -> [Option<[f32; 3]>; 3] {
        let triangle = render
            .triangles()
            .iter()
            .find(|t| t.source.polygon == polygon)
            .expect("a triangle for that polygon");
        triangle
            .vertices
            .map(|index| render.vertices()[index as usize].normal)
    }

    #[test]
    fn accept_f10_c_01_uv_seam_keeps_two_vertices_at_one_position() {
        // AC03: a quad split into two triangles that share positions 1 and 2.
        // Position 2 is authored with different UVs on the two polygons, so
        // the seam must survive; a splitter keyed by position index alone
        // would reuse one vertex and lose it.
        let with_uv = |position, uv| RawCorner {
            position,
            normal: Some(0),
            uv: Some(uv),
            color: None,
        };
        let mesh = mesh(vec![
            triangle(
                0,
                vec![
                    with_uv(0, [0.0, 0.0]),
                    with_uv(1, [1.0, 0.0]),
                    with_uv(2, [0.0, 1.0]),
                ],
            ),
            triangle(
                0,
                vec![
                    with_uv(2, [0.5, 0.5]),
                    with_uv(1, [1.0, 0.0]),
                    with_uv(3, [1.0, 1.0]),
                ],
            ),
        ]);
        let render = RenderMesh::build(&mesh).expect("complete topology");

        assert_eq!(render.source_faces(), 2);
        assert_eq!(render.source_triangles(), 2);
        assert_eq!(render.degenerate_triangles(), 0);
        assert_eq!(render.vertices().len(), 5, "position 2 carries two UVs");

        // The shared position 1 has the same UV in both polygons and merges.
        let at_one: Vec<&RenderVertex> = render
            .vertices()
            .iter()
            .filter(|vertex| vertex.position_index == 1)
            .collect();
        assert_eq!(at_one.len(), 1);
        assert_eq!(at_one[0].uv, Some([1.0, 0.0]));

        // The shared position 2 keeps both authored UVs.
        let at_two: Vec<[f32; 2]> = render
            .vertices()
            .iter()
            .filter(|vertex| vertex.position_index == 2)
            .map(|vertex| vertex.uv.expect("authored uv"))
            .collect();
        assert_eq!(at_two.len(), 2);
        assert!(at_two.contains(&[0.0, 1.0]));
        assert!(at_two.contains(&[0.5, 0.5]));

        // Each triangle samples the UVs authored on its own polygon.
        assert_eq!(uvs_of(&render, 0), [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]);
        assert_eq!(uvs_of(&render, 1), [[0.5, 0.5], [1.0, 0.0], [1.0, 1.0]]);
    }

    #[test]
    fn accept_f10_c_01_color_difference_splits_a_shared_position() {
        let with_color = |position, color| RawCorner {
            position,
            normal: Some(0),
            uv: None,
            color: Some(color),
        };
        let mesh = mesh(vec![
            triangle(
                0,
                vec![
                    with_color(0, [1.0, 0.0, 0.0]),
                    with_color(1, [0.0, 1.0, 0.0]),
                    with_color(2, [0.0, 0.0, 1.0]),
                ],
            ),
            triangle(
                0,
                vec![
                    with_color(2, [1.0, 1.0, 1.0]),
                    with_color(1, [0.0, 1.0, 0.0]),
                    with_color(3, [0.5, 0.5, 0.5]),
                ],
            ),
        ]);
        let render = RenderMesh::build(&mesh).expect("complete topology");

        assert_eq!(render.vertices().len(), 5);
        let at_two: Vec<[f32; 3]> = render
            .vertices()
            .iter()
            .filter(|vertex| vertex.position_index == 2)
            .map(|vertex| vertex.color.expect("authored color"))
            .collect();
        assert_eq!(at_two.len(), 2);
        assert!(at_two.contains(&[0.0, 0.0, 1.0]));
        assert!(at_two.contains(&[1.0, 1.0, 1.0]));
        assert_eq!(colors_of(&render, 0)[2], [0.0, 0.0, 1.0]);
        assert_eq!(colors_of(&render, 1)[0], [1.0, 1.0, 1.0]);
    }

    #[test]
    fn accept_f10_c_01_normal_difference_splits_a_shared_position() {
        let with_normal = |position, normal| RawCorner {
            position,
            normal: Some(normal),
            uv: None,
            color: None,
        };
        let mesh = mesh(vec![
            triangle(
                0,
                vec![with_normal(0, 0), with_normal(1, 0), with_normal(2, 0)],
            ),
            triangle(
                0,
                vec![with_normal(2, 1), with_normal(1, 0), with_normal(3, 0)],
            ),
        ]);
        let render = RenderMesh::build(&mesh).expect("complete topology");

        assert_eq!(render.vertices().len(), 5);
        let at_two: Vec<Option<[f32; 3]>> = render
            .vertices()
            .iter()
            .filter(|vertex| vertex.position_index == 2)
            .map(|vertex| vertex.normal)
            .collect();
        assert_eq!(at_two.len(), 2);
        assert!(at_two.contains(&Some([0.0, 0.0, 1.0])));
        assert!(at_two.contains(&Some([0.0, 1.0, 0.0])));
        assert_eq!(normals_of(&render, 0)[2], Some([0.0, 0.0, 1.0]));
        assert_eq!(normals_of(&render, 1)[0], Some([0.0, 1.0, 0.0]));
    }

    #[test]
    fn accept_f10_c_01_material_difference_splits_every_shared_position() {
        // Material is per polygon, so two polygons with different materials
        // share no vertex even where their corner attributes agree.
        let mesh = mesh(vec![
            triangle(0, vec![corner(0), corner(1), corner(2)]),
            triangle(7, vec![corner(2), corner(1), corner(3)]),
        ]);
        let render = RenderMesh::build(&mesh).expect("complete topology");

        let materials_at = |position: u32| -> Vec<u32> {
            render
                .vertices()
                .iter()
                .filter(|vertex| vertex.position_index == position)
                .map(|vertex| vertex.material)
                .collect()
        };
        assert_eq!(materials_at(0), [0]);
        assert_eq!(materials_at(3), [7]);
        assert_eq!(materials_at(1).len(), 2);
        assert!(materials_at(1).contains(&0));
        assert!(materials_at(1).contains(&7));
        assert_eq!(materials_at(2).len(), 2);
        assert_eq!(render.vertices().len(), 6);
    }

    #[test]
    fn accept_f10_c_01_identical_corners_merge() {
        let mesh = mesh(vec![
            triangle(0, vec![corner(0), corner(1), corner(2)]),
            triangle(0, vec![corner(2), corner(1), corner(3)]),
        ]);
        let render = RenderMesh::build(&mesh).expect("complete topology");

        assert_eq!(render.vertices().len(), 4);
        let [first, second] = [render.triangles()[0], render.triangles()[1]];
        assert_eq!(first.vertices[1], second.vertices[1], "position 1 merges");
        assert_eq!(first.vertices[2], second.vertices[0], "position 2 merges");
        let positions: Vec<u32> = render
            .vertices()
            .iter()
            .map(|vertex| vertex.position_index)
            .collect();
        assert_eq!(positions, [0, 1, 2, 3]);
    }

    #[test]
    fn accept_f10_c_01_distinct_position_indices_are_never_welded() {
        // Position 3 is exactly position 0. Different indices must stay
        // different vertices even though every stored value is identical.
        let mesh = RawMesh {
            positions: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0],
            ],
            normals: NORMALS.to_vec(),
            polygons: vec![
                triangle(0, vec![corner(0), corner(1), corner(2)]),
                triangle(0, vec![corner(3), corner(1), corner(2)]),
            ],
        };
        let render = RenderMesh::build(&mesh).expect("complete topology");

        assert_eq!(render.vertices().len(), 4);
        let coincident: Vec<u32> = render
            .vertices()
            .iter()
            .filter(|vertex| vertex.position == [0.0, 0.0, 0.0])
            .map(|vertex| vertex.position_index)
            .collect();
        assert_eq!(coincident, [0, 3]);
        assert_ne!(
            render.triangles()[0].vertices[0],
            render.triangles()[1].vertices[0]
        );
    }

    #[test]
    fn accept_f10_c_01_incomplete_topology_is_refused_with_exact_face_codes() {
        let mesh = mesh(vec![
            // 0: decoded
            triangle(0, vec![corner(0), corner(1), corner(2)]),
            // 1: position index past the four positions
            strip(0, &[0, 1, 9]),
            // 2: self-intersecting bow tie, cannot be triangulated
            triangle(0, vec![corner(0), corner(1), corner(2), corner(3)]),
            // 3: two-corner polygon
            triangle(0, vec![corner(0), corner(1)]),
        ]);

        let error = RenderMesh::build(&mesh).unwrap_err();
        assert_eq!(
            error,
            RenderMeshError::IncompleteTopology {
                rejected: vec![
                    RejectedFace {
                        polygon: 1,
                        code: "position_index_out_of_range"
                    },
                    RejectedFace {
                        polygon: 2,
                        code: "unsupported_ngon"
                    },
                    RejectedFace {
                        polygon: 3,
                        code: "too_few_corners"
                    },
                ],
            }
        );
        let text = error.to_string();
        for code in [
            "position_index_out_of_range",
            "unsupported_ngon",
            "too_few_corners",
        ] {
            assert!(text.contains(code), "{text}");
        }
    }

    #[test]
    fn accept_f10_c_01_out_of_range_bounds_are_refused_not_panicked() {
        let mesh = mesh(vec![strip(0, &[0, 1, 9])]);

        // A supplied topology that claims the face decoded must still not
        // index past the stored positions.
        let forged = MeshTopology {
            triangles: vec![MeshTriangle {
                polygon: 0,
                step: 0,
                corners: [0, 1, 2],
                positions: [0, 1, 9],
            }],
            faces: vec![FaceStatus::Decoded {
                triangles: 1,
                degenerate: 0,
            }],
        };
        assert_eq!(
            RenderMesh::from_parts(&mesh, &forged),
            Err(RenderMeshError::OutOfRange {
                polygon: 0,
                corner: Some(2),
                field: "position",
                index: 9,
                available: 4,
            })
        );

        // Through the gate, the same mesh is refused with the face's code.
        assert_eq!(
            RenderMesh::build(&mesh),
            Err(RenderMeshError::IncompleteTopology {
                rejected: vec![RejectedFace {
                    polygon: 0,
                    code: "position_index_out_of_range",
                }],
            })
        );
    }

    #[test]
    fn accept_f10_c_01_material_groups_keep_raw_indices_and_ignore_flags() {
        let mut flagged = triangle(7, vec![corner(0), corner(1), corner(2)]);
        flagged.raw_flags = 0xF0F0_F0F0;
        let mut plain = flagged.clone();
        plain.raw_flags = 0;

        let build = |first: RawPolygon| {
            RenderMesh::build(&RawMesh {
                positions: POSITIONS.to_vec(),
                normals: NORMALS.to_vec(),
                polygons: vec![first, triangle(2, vec![corner(2), corner(1), corner(3)])],
            })
            .expect("complete topology")
        };
        let with_flags = build(flagged);
        let without = build(plain);

        // Raw flags are not interpreted: only the material index is read.
        assert_eq!(with_flags, without);
        let materials: Vec<u32> = with_flags
            .groups()
            .iter()
            .map(|group| group.material)
            .collect();
        assert_eq!(materials, [2, 7]);
        assert_eq!(with_flags.groups()[0].triangles, [1]);
        assert_eq!(with_flags.groups()[1].triangles, [0]);
        for group in with_flags.groups() {
            for &index in &group.triangles {
                assert_eq!(with_flags.triangles()[index].material, group.material);
            }
        }
    }

    #[test]
    fn accept_f10_c_01_source_maps_cover_vertices_and_triangles() {
        let mesh = mesh(vec![strip(0, &[0, 1, 2, 3])]);
        let render = RenderMesh::build(&mesh).expect("complete topology");

        assert_eq!(render.triangles().len(), 2);
        assert_eq!(
            render.triangles()[0].source,
            SourceTriangle {
                polygon: 0,
                step: 0
            }
        );
        assert_eq!(
            render.triangles()[1].source,
            SourceTriangle {
                polygon: 0,
                step: 1
            }
        );

        let source_of = |position: u32| {
            render
                .vertices()
                .iter()
                .find(|vertex| vertex.position_index == position)
                .expect("a vertex at that position")
                .source
        };
        assert_eq!(
            source_of(0),
            SourceCorner {
                polygon: 0,
                corner: 0
            }
        );
        assert_eq!(
            source_of(1),
            SourceCorner {
                polygon: 0,
                corner: 1
            }
        );
        // Position 3 is first drawn by step 1, whose corners are [2, 1, 3].
        assert_eq!(
            source_of(3),
            SourceCorner {
                polygon: 0,
                corner: 3
            }
        );
    }

    #[test]
    fn accept_f10_c_01_degenerate_triangles_are_kept_and_counted() {
        let mesh = mesh(vec![strip(0, &[0, 1, 2, 1, 3])]);
        let render = RenderMesh::build(&mesh).expect("complete topology");

        assert_eq!(render.source_faces(), 1, "face counts stay exact");
        assert_eq!(render.source_triangles(), 3);
        assert_eq!(render.triangles().len(), 3, "nothing is dropped");
        assert_eq!(render.degenerate_triangles(), 1);
        let degenerate: Vec<bool> = render
            .triangles()
            .iter()
            .map(|triangle| triangle.degenerate)
            .collect();
        assert_eq!(degenerate, [false, true, false]);
        assert_eq!(
            render.triangles()[1].source,
            SourceTriangle {
                polygon: 0,
                step: 1
            }
        );
    }

    // ------------------------------------------------ the audit's fixtures ---

    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use cs_assets::install;
    use cs_assets::vfs::{ContentSession, INSTALL_NAMESPACE, SessionBuilder, WORLD_NAMESPACE};
    use cs_formats::gamez::materials::{
        GameZMaterials, GameZTextureName, MATERIAL_FLAG_ALWAYS, MATERIAL_FLAG_TEXTURED,
        MaterialInfo, RawMaterial, RawMaterialRecord, TextureNameEncoding,
    };
    use cs_formats::gamez::reader::{MeshIndex, RawMaterialGroup, RawMeshInfo};
    use cs_formats::gamez::{
        CORNER_COUNT_MASK, FLAG_NORMALS, FLAG_SHIFT, FLAG_TRIANGLE_STRIP, GameZHeader, GameZMesh,
        GameZMeshes, NG_MATERIAL_SLOTS, RawMeshMaterialInfo,
    };
    use cs_formats::texture::zbd::{
        FLAG_BYTES_PER_PIXEL2, FLAG_NO_ALPHA, ZBD_TEXTURE_HEADER_BYTES,
    };
    use cs_formats::zbd::{GAMEZ_SIGNATURE, GAMEZ_VERSION};
    use cs_types::asset_id::{AssetKey, ResolveContext, WorldGroup};
    use cs_types::install::ParseState;

    use crate::textures::TextureCatalog;

    /// The texture-archive layout's own numbers, spelled here so the fixture
    /// writer does not borrow the reader's constants: a 24-byte header, then one
    /// 40-byte table entry per texture, whose first 32 bytes are the name.
    const FIXTURE_HEADER: usize = ZBD_TEXTURE_HEADER_BYTES;
    const FIXTURE_ENTRY: usize = 40;
    const FIXTURE_NAME: usize = 32;
    const FIXTURE_OPAQUE: u32 = FLAG_BYTES_PER_PIXEL2 | FLAG_NO_ALPHA;

    /// A synthetic texture package: `names`, each a 1x1 direct-colour texture
    /// whose single word is its position in the table plus one, so a decoded
    /// upload can be traced back to the entry it came from.
    ///
    /// The per-texture info block is sixteen bytes in the layout's own order —
    /// `u32` flags, `u16` width, `u16` height, `u32` zero, `u16` palette count,
    /// `u16` stretch — and a zero palette count means one RGB565 word per texel.
    fn package(names: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        for word in [0u32, 1, 0, names.len() as u32, 0, 0] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        let mut offset = FIXTURE_HEADER + names.len() * FIXTURE_ENTRY;
        let mut bodies = Vec::new();
        for (position, name) in names.iter().enumerate() {
            let mut table = vec![0u8; FIXTURE_NAME];
            table[..name.len()].copy_from_slice(name.as_bytes());
            out.extend_from_slice(&table);
            out.extend_from_slice(&(offset as u32).to_le_bytes());
            out.extend_from_slice(&(-1i32).to_le_bytes());

            let mut body = Vec::new();
            body.extend_from_slice(&FIXTURE_OPAQUE.to_le_bytes());
            body.extend_from_slice(&1u16.to_le_bytes());
            body.extend_from_slice(&1u16.to_le_bytes());
            body.extend_from_slice(&0u32.to_le_bytes());
            body.extend_from_slice(&0u16.to_le_bytes());
            body.extend_from_slice(&0u16.to_le_bytes());
            body.extend_from_slice(&((position as u16) + 1).to_le_bytes());
            offset += body.len();
            bodies.push(body);
        }
        for body in bodies {
            out.extend_from_slice(&body);
        }
        out
    }

    static NEXT_TREE: AtomicU64 = AtomicU64::new(0);

    /// A disposable fixture installation under the temporary directory, dropped
    /// with the test. No original game data is ever written here.
    struct Tree(PathBuf);

    impl Tree {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "cs-f10-c-02-{}-{}",
                std::process::id(),
                NEXT_TREE.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("fixture root is created");
            Self(root)
        }

        fn write(&self, spelling: &str, bytes: &[u8]) {
            let path = self.0.join(spelling);
            fs::create_dir_all(path.parent().expect("a parent")).expect("fixture dirs");
            fs::write(path, bytes).expect("fixture bytes are written");
        }

        /// One world whose `texture.zbd` stores exactly `names`, and a second
        /// archive of the same world holding `extra`, so a fallback search has
        /// somewhere to go and must still not happen.
        fn world(names: &[&str], extra: &[&str]) -> Self {
            let tree = Self::new();
            tree.write("ZBD/c1/texture.zbd", &package(names));
            tree.write("ZBD/c1/rtexture2.zbd", &package(extra));
            tree
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A session of the installation at `root` with the production mount layout,
    /// selecting the world group spelled `world`.
    fn world_session(root: &Path, world: &str) -> ContentSession {
        let found = install::discover(root).expect("installation is discovered");
        let group = found
            .diagnosis
            .world_groups
            .iter()
            .find(|group| group.as_str().eq_ignore_ascii_case(world))
            .unwrap_or_else(|| panic!("world group {world} is discovered"))
            .clone();
        let context = ResolveContext::new(install::fingerprint(&found.manifest))
            .with_world_group(WorldGroup::from_relative(group));
        let mut builder = SessionBuilder::new(context);
        builder
            .mount_installation(root, &found.diagnosis)
            .expect("installation mounts");
        builder.open()
    }

    fn world_key(path: &str) -> AssetKey {
        AssetKey::from_spelling(WORLD_NAMESPACE, path, "default").expect("valid key")
    }

    /// The world's `texture.zbd` key, spelled once so a test never guesses at a
    /// namespace.
    fn texture_key() -> AssetKey {
        world_key("texture.zbd")
    }

    // ---- the mesh and material tables, built from literals ----

    /// A container header with only the words the audit reads.
    fn header() -> GameZHeader {
        GameZHeader {
            signature: cs_formats::zbd::GAMEZ_SIGNATURE,
            version: cs_formats::zbd::GAMEZ_VERSION,
            unk08: 0,
            texture_count: 0,
            textures_offset: 40,
            materials_offset: 0,
            meshes_offset: 0,
            node_array_size: 0,
            light_index: 0,
            nodes_offset: 0,
        }
    }

    /// A material record with only the words the audit reads; the rest is the
    /// reference's asserted profile, so no `MaterialFinding` is raised for it.
    fn record(texture_index: u32, textured: bool) -> RawMaterialRecord {
        RawMaterialRecord {
            alpha: 0xFF,
            flags: MATERIAL_FLAG_ALWAYS | if textured { MATERIAL_FLAG_TEXTURED } else { 0 },
            rgb: if textured { 0x7FFF } else { 0 },
            color: if textured {
                [255.0, 255.0, 255.0]
            } else {
                [0.0, 0.0, 0.0]
            },
            texture_index,
            field20: 0.0,
            field24: 0.5,
            field28: 0.5,
            // The word the reference calls `specular` and newer classification
            // calls soil. The audit never looks at it; the fixture gives each
            // material a distinct value so a reader that did look would differ.
            field32: 0.25,
            cycle_ptr: 0,
        }
    }

    /// A material at `index`, with the link words the layout's own rule gives it.
    fn material(index: u32, texture_index: u32, textured: bool) -> RawMaterial {
        let count = 3u32;
        RawMaterial {
            index,
            record: record(texture_index, textured),
            link1: if index + 1 >= count {
                -1
            } else {
                (index + 1) as i16
            },
            link2: if index == 0 { -1 } else { (index - 1) as i16 },
            cycle: None,
        }
    }

    fn texture(index: u32, name: &str) -> GameZTextureName {
        let (stem, suffix) = match name.split_once('.') {
            Some((stem, suffix)) => (stem.to_owned(), Some(suffix.to_owned())),
            None => (name.to_owned(), None),
        };
        GameZTextureName {
            index,
            name: name.to_owned(),
            stem,
            suffix,
            encoding: if name.contains('.') {
                TextureNameEncoding::WithSuffix
            } else {
                TextureNameEncoding::StemOnly
            },
            field00: 0,
            field32: 2,
            field36: 0,
            field40: -1,
        }
    }

    /// The container's two tables, exactly as `read_gamez_materials` hands them
    /// over for those bytes.
    fn tables(names: &[&str], materials: Vec<RawMaterial>) -> GameZMaterials {
        let count = materials.len() as i32;
        GameZMaterials {
            header: header(),
            textures: names
                .iter()
                .enumerate()
                .map(|(index, name)| texture(index as u32, name))
                .collect(),
            info: MaterialInfo {
                array_size: count,
                count,
                index_max: count,
                index_last: count - 1,
            },
            materials,
            free_slots: 1000 - count as u32,
            findings: Vec::new(),
            textures_offset: 40,
            materials_offset: 40 + names.len() as u64 * 44,
            data_end: 0,
        }
    }

    /// A mesh record's 25 raw words, of which only `parent_count` (which marks
    /// the slot present) and the material count are ever read here.
    fn mesh_info(material_count: u32) -> RawMeshInfo {
        RawMeshInfo {
            file_ptr: 0,
            unk04: 0,
            unk08: 0,
            parent_count: 1,
            polygon_count: 0,
            vertex_count: 0,
            normal_count: 0,
            morph_count: 0,
            light_count: 0,
            unk36: 0,
            unk40: 0.0,
            unk44: 0.0,
            unk48: 0,
            polygons_ptr: 0,
            vertices_ptr: 0,
            normals_ptr: 0,
            lights_ptr: 0,
            morphs_ptr: 0,
            unk72: 0.0,
            unk76: 0.0,
            unk80: 0.0,
            unk84: 0.0,
            unk88: 0,
            material_count,
            materials_ptr: 0,
        }
    }

    /// One mesh whose stored material references are `mesh_level` and whose stored
    /// polygons carry `polygons`' groups each, in polygon order. Both levels are
    /// real: the audit counts them separately and a reader that dropped either
    /// would report fewer references.
    fn container_mesh(index: u32, mesh_level: &[u32], polygons: &[Vec<u32>]) -> GameZMesh {
        let stored: Vec<RawMeshMaterialInfo> = mesh_level
            .iter()
            .map(|&material| RawMeshMaterialInfo {
                material_index: material,
                polygon_usage_count: 1,
                unk_ptr: 0,
            })
            .collect();
        let material_groups: Vec<Vec<RawMaterialGroup>> = polygons
            .iter()
            .map(|groups| {
                groups
                    .iter()
                    .map(|&material| RawMaterialGroup {
                        material,
                        uvs: vec![[0.0, 0.0]; 3],
                    })
                    .collect()
            })
            .collect();
        let raw = RawMesh {
            positions: vec![[0.0, 0.0, 0.0]],
            normals: Vec::new(),
            polygons: polygons
                .iter()
                .map(|groups| RawPolygon {
                    kind: PrimitiveKind::Polygon,
                    raw_flags: 0,
                    material: groups.first().copied().unwrap_or(0),
                    corners: vec![corner(0), corner(0), corner(0)],
                })
                .collect(),
        };
        assert_eq!(
            material_groups.len(),
            raw.polygons.len(),
            "one group list per stored polygon"
        );
        GameZMesh {
            index,
            info: mesh_info(mesh_level.len() as u32),
            mesh: raw,
            polygon_records: Vec::new(),
            lights: Vec::new(),
            morphs: Vec::new(),
            materials: stored,
            material_groups,
            data_offset: 0,
            data_end: 0,
        }
    }

    /// The parsed mesh section, as `read_gamez_meshes` hands it over.
    fn container(meshes: Vec<GameZMesh>) -> GameZMeshes {
        let references: usize = meshes
            .iter()
            .map(|mesh| mesh.materials.len() + mesh.material_groups.iter().flatten().count())
            .sum();
        GameZMeshes {
            header: header(),
            index: MeshIndex {
                array_size: meshes.len() as i32,
                count: meshes.len() as i32,
                last_index: -1,
            },
            fixup: cs_formats::gamez::reader::Fixup::None,
            meshes: meshes.into_iter().map(Some).collect(),
            findings: Vec::new(),
            unchecked_material_references: references,
            data_offset: 0,
            data_end: 0,
        }
    }

    /// The catalog a fixture world is read through, and the session that opened
    /// it. Both are real: the package bytes are read by the production reader.
    fn catalog(tree: &Tree) -> (ContentSession, TextureCatalog, AssetKey) {
        let session = world_session(&tree.0, "ZBD/c1");
        let key = texture_key();
        let catalog = TextureCatalog::open(&session, std::slice::from_ref(&key));
        assert_eq!(catalog.failures().count(), 0, "the fixture archive opens");
        (session, catalog, key)
    }

    fn context<'a>(
        session: &'a ContentSession,
        catalog: &'a TextureCatalog,
        key: &'a AssetKey,
    ) -> DependencyContext<'a> {
        DependencyContext {
            archive: key,
            session,
            catalog,
            origin: None,
            container: "fixture",
        }
    }

    // ------------------------------------------------ the audit's tests ---

    /// A material that names a texture the world's archive stores exactly once
    /// resolves to that one stored texture, and its row is a complete catalog
    /// element. The resolved `TextureId` names the archive, the entry and the
    /// stored name, so the origin is exact rather than "some sky".
    #[test]
    fn accept_f10_c_02_audit_resolves_a_material_to_exactly_one_stored_texture() {
        let tree = Tree::world(&["sky", "ground"], &["tier"]);
        let (session, catalog, key) = catalog(&tree);
        let meshes = container(vec![container_mesh(0, &[0], &[vec![0]])]);
        let materials = tables(&["sky"], vec![material(0, 0, true)]);
        let audit =
            MeshDependencyAudit::build(&meshes, &materials, &context(&session, &catalog, &key));

        assert_eq!(
            audit.references, 2,
            "one mesh-level and one polygon reference"
        );
        assert_eq!(audit.rows.len(), 1);
        assert_eq!(audit.resolved, 1);
        assert!(audit.blocked.is_empty(), "{:?}", audit.blocked);
        let row = &audit.rows[0];
        assert_eq!(row.material, 0);
        assert_eq!(row.id, "gamez.materials[0]");
        assert_eq!(row.kind, "material");
        assert_eq!(
            row.dependencies,
            vec![key.clone()],
            "one archive, the caller's"
        );
        assert_eq!(row.parse_state, ParseState::Parsed);
        assert_eq!(row.normalize_state, ParseState::Parsed);
        assert_eq!(row.readiness, DependencyReadiness::Ready);
        assert!(
            row.unsupported_reasons.is_empty(),
            "{:?}",
            row.unsupported_reasons
        );
        assert_eq!(row.runtime_consumers, vec![MATERIAL_CONSUMER]);
        assert!(row.fingerprint.is_some(), "a read record has bytes to hash");
        assert_eq!(row.state.code(), "resolved");
        match &row.state {
            MaterialState::Resolved { texture } => {
                assert_eq!(texture.name, "sky");
                assert_eq!(texture.entry_index, 0);
                assert!(
                    texture.archive.as_str().ends_with("texture.zbd"),
                    "{texture}"
                );
            }
            other => panic!("expected a resolved texture, got {other}"),
        }
        // Both stored references are named, so a caller can see what depends on
        // this material.
        assert_eq!(
            row.used_by,
            vec![
                MaterialUse {
                    mesh: 0,
                    reference: MaterialReference::MeshRecord { position: 0 }
                },
                MaterialUse {
                    mesh: 0,
                    reference: MaterialReference::PolygonGroup {
                        polygon: 0,
                        group: 0
                    }
                },
            ]
        );
        assert_eq!(
            audit.references, meshes.unchecked_material_references,
            "the audit sees exactly the references the mesh reader counted"
        );
    }

    /// **The discriminating case.** A polygon whose material names a texture that
    /// is absent from its world's archive appears in the audit as
    /// `missing_texture`, with the exact stored name and the exact archive. It
    /// does **not** resolve to another archive of the same world, to a
    /// case-folded spelling, to an extension-stripped one, and not to a default.
    #[test]
    fn accept_f10_c_02_audit_reports_a_missing_texture_with_its_exact_name_and_archive() {
        // The world's `texture.zbd` stores `sky` and `smoke`. A **second archive
        // of the same world** stores `Sky1.tif`, `sky1` and `c2only`, so a
        // fallback search would have somewhere to go. It must not be taken, and
        // the catalog is opened over both archives to prove the audit used only
        // the one it was given.
        let tree = Tree::world(&["sky", "smoke"], &["Sky1.tif", "sky1", "c2only"]);
        let session = world_session(&tree.0, "ZBD/c1");
        let key = texture_key();
        let other = world_key("rtexture2.zbd");
        let catalog = TextureCatalog::open(&session, &[key.clone(), other.clone()]);
        assert_eq!(catalog.failures().count(), 0, "both fixture archives open");
        assert_eq!(catalog.archives().count(), 2, "and both are readable");

        let meshes = container(vec![container_mesh(0, &[0], &[vec![0]])]);
        let materials = tables(&["Sky1.tif"], vec![material(0, 0, true)]);
        let audit =
            MeshDependencyAudit::build(&meshes, &materials, &context(&session, &catalog, &key));

        assert_eq!(
            audit.resolved, 0,
            "the name is not in the archive it was given"
        );
        assert_eq!(audit.rows.len(), 1, "a failed entry stays in the audit");
        let row = &audit.rows[0];
        assert_eq!(
            row.state,
            MaterialState::MissingTexture {
                name: "Sky1.tif".to_owned(),
                archive: key.clone(),
            },
            "the exact stored name and the exact archive, nothing substituted"
        );
        assert_eq!(row.state.code(), "missing_texture");
        assert_eq!(row.dependencies, vec![key.clone()], "one archive only");
        assert_eq!(row.readiness, DependencyReadiness::Blocked);
        assert_eq!(
            row.parse_state,
            ParseState::Parsed,
            "the record itself was read"
        );
        assert!(
            matches!(row.normalize_state, ParseState::Failed { .. }),
            "the dependency did not reach an origin: {:?}",
            row.normalize_state
        );
        assert_eq!(
            row.unsupported_reasons,
            vec!["texture_not_found".to_owned()]
        );
        let text = row.state.to_string();
        assert!(text.contains("Sky1.tif"), "{text}");
        assert!(text.contains("texture.zbd"), "{text}");

        // The other archive really does hold both spellings, so the row is a
        // choice of archive and a choice of spelling, not a missing file.
        for spelling in ["Sky1.tif", "sky1"] {
            assert!(
                catalog
                    .resolve(&session, &TextureRef::new(other.clone(), spelling))
                    .is_ok(),
                "the second archive stores `{spelling}`"
            );
        }
        assert!(
            catalog
                .resolve(&session, &TextureRef::new(key.clone(), "sky1"))
                .is_err()
        );
    }

    /// The exact-name rule is not relaxed to make a number smaller: a name that
    /// differs only in case, and one that differs only by its extension, are both
    /// missing. Neither is folded, stripped or aliased.
    #[test]
    fn accept_f10_c_02_audit_neither_folds_case_nor_strips_an_extension() {
        let tree = Tree::world(&["sky1", "ground"], &["tier"]);
        let (session, catalog, key) = catalog(&tree);
        let meshes = container(vec![container_mesh(0, &[0, 1, 2], &[])]);
        let materials = tables(
            &["Sky1.tif", "sky1", "ground"],
            vec![
                material(0, 0, true),
                material(1, 1, true),
                material(2, 2, true),
            ],
        );
        let audit =
            MeshDependencyAudit::build(&meshes, &materials, &context(&session, &catalog, &key));

        let states: Vec<&str> = audit.rows.iter().map(|row| row.state.code()).collect();
        assert_eq!(
            states,
            ["missing_texture", "resolved", "resolved"],
            "`Sky1.tif` is neither folded to `sky1` nor stripped to `Sky1`"
        );
        assert_eq!(audit.resolved, 2);
        assert!(matches!(
            &audit.rows[0].state,
            MaterialState::MissingTexture { name, .. } if name == "Sky1.tif"
        ));
        assert!(matches!(
            &audit.rows[1].state,
            MaterialState::Resolved { texture } if texture.name == "sky1"
        ));
        assert!(matches!(
            &audit.rows[2].state,
            MaterialState::Resolved { texture } if texture.name == "ground"
        ));
    }

    /// A material index past the material table is **reported**, never clamped to
    /// the last record and never wrapped.
    #[test]
    fn accept_f10_c_02_audit_reports_a_material_index_past_the_table_and_never_clamps() {
        let tree = Tree::world(&["sky"], &["tier"]);
        let (session, catalog, key) = catalog(&tree);
        // Three records; the meshes store 0, 2 and 7.
        let meshes = container(vec![container_mesh(0, &[0, 2, 7], &[vec![7], vec![0, 7]])]);
        let materials = tables(
            &["sky"],
            vec![
                material(0, 0, true),
                material(1, 0, true),
                material(2, 0, true),
            ],
        );
        let audit =
            MeshDependencyAudit::build(&meshes, &materials, &context(&session, &catalog, &key));

        assert_eq!(audit.rows.len(), 3, "one row per distinct stored index");
        assert_eq!(
            audit.out_of_range().collect::<Vec<_>>(),
            vec![(7, 3)],
            "index 7 of three present records; 0 and 2 are inside"
        );
        let out_of_range = &audit.rows[2];
        assert_eq!(out_of_range.material, 7);
        assert_eq!(
            out_of_range.state,
            MaterialState::MaterialIndexOutOfRange {
                material: 7,
                count: 3
            }
        );
        assert!(
            matches!(out_of_range.parse_state, ParseState::Failed { .. }),
            "there is no record to parse"
        );
        assert_eq!(out_of_range.fingerprint, None, "no stored bytes to hash");
        assert!(out_of_range.record.is_none());
        // The last real record is material 2, and it is **not** what index 7
        // resolved to: that row has no texture at all.
        assert!(matches!(
            &audit.rows[1].state,
            MaterialState::Resolved { texture } if texture.name == "sky"
        ));
        assert!(out_of_range.state.texture().is_none());
        // The references that reached the bad index are still named.
        assert_eq!(
            out_of_range.used_by,
            vec![
                MaterialUse {
                    mesh: 0,
                    reference: MaterialReference::MeshRecord { position: 2 }
                },
                MaterialUse {
                    mesh: 0,
                    reference: MaterialReference::PolygonGroup {
                        polygon: 0,
                        group: 0
                    }
                },
                MaterialUse {
                    mesh: 0,
                    reference: MaterialReference::PolygonGroup {
                        polygon: 1,
                        group: 1
                    }
                },
            ]
        );
        assert!(
            audit
                .blocked
                .iter()
                .any(|entry| entry == "material 7: material_index_out_of_range: material 7 of 3"),
            "{:?}",
            audit.blocked
        );
    }

    /// A material with no texture and a material whose record has an unmapped flag
    /// bit: two more states, two more rows, neither dropped. An untextured
    /// material is **complete**, not blocked — it has no texture to find.
    #[test]
    fn accept_f10_c_02_audit_keeps_untextured_and_unknown_field_rows() {
        let tree = Tree::world(&["sky"], &["tier"]);
        let (session, catalog, key) = catalog(&tree);
        let meshes = container(vec![container_mesh(0, &[0, 1, 2], &[])]);
        let mut untextured = material(1, 0, false);
        untextured.record.color = [0.25, 0.5, 0.75];
        untextured.record.alpha = 0x10;
        let mut unknown = material(2, 0, true);
        unknown.record.flags |= 0x40; // a bit the reference's MaterialFlags does not name
        let materials = tables(&["sky"], vec![material(0, 0, true), untextured, unknown]);
        let audit =
            MeshDependencyAudit::build(&meshes, &materials, &context(&session, &catalog, &key));

        assert_eq!(audit.rows.len(), 3, "every distinct stored index has a row");
        assert_eq!(audit.rows[0].state.code(), "resolved");
        assert_eq!(audit.rows[1].state, MaterialState::Untextured);
        assert_eq!(
            audit.rows[1].readiness,
            DependencyReadiness::Ready,
            "an untextured material is complete: it has no texture to find"
        );
        assert_eq!(audit.rows[1].normalize_state, ParseState::Parsed);
        assert_eq!(audit.rows[1].unsupported_reasons, Vec::<String>::new());
        assert_eq!(
            audit.rows[1]
                .record
                .as_ref()
                .expect("raw record")
                .record
                .color,
            [0.25, 0.5, 0.75],
            "the flat colour is on the row, uninterpreted"
        );
        assert_eq!(
            audit.rows[2].state,
            MaterialState::UnknownField { bits: 0x40 }
        );
        assert_eq!(
            audit.rows[2].readiness,
            DependencyReadiness::Blocked,
            "even whether the record is textured is not established"
        );
        assert_eq!(audit.resolved, 1);
        assert_eq!(
            audit
                .rows
                .iter()
                .filter(|row| row.readiness == DependencyReadiness::Ready)
                .count(),
            2
        );
    }

    /// A texture index outside the container's own table, a name the archive
    /// stores twice, and an archive the catalog does not hold: three states, each
    /// reported, none resolved to a substitute.
    #[test]
    fn accept_f10_c_02_audit_reports_dangling_archive_and_duplicate_dependencies() {
        // The archive stores `twin` twice, so it has no single origin for it.
        let tree = Tree::world(&["sky", "twin", "twin"], &["tier"]);
        let (session, catalog, key) = catalog(&tree);
        // Material 0 names a texture the container's own two-entry table does not
        // have; material 1 names the duplicated name.
        let meshes = container(vec![container_mesh(0, &[0, 1], &[])]);
        let materials = tables(
            &["sky", "twin"],
            vec![material(0, 5, true), material(1, 1, true)],
        );
        let audit =
            MeshDependencyAudit::build(&meshes, &materials, &context(&session, &catalog, &key));
        assert_eq!(
            audit.rows[0].state,
            MaterialState::TextureIndexOutOfRange {
                index: 5,
                available: 2
            },
            "the container does not store that texture at all"
        );
        assert_eq!(
            audit.rows[1].state,
            MaterialState::DuplicateTexture {
                name: "twin".to_owned(),
                archive: key.clone(),
                entries: vec![1, 2],
            },
            "both entries are reported, not one of them picked"
        );
        assert_eq!(audit.resolved, 0);

        // An archive the catalog does not hold: no lookup is attempted, and the
        // container-side defect above is still reported as itself.
        let unheld = world_key("rtexture8.zbd");
        let audit =
            MeshDependencyAudit::build(&meshes, &materials, &context(&session, &catalog, &unheld));
        assert_eq!(
            audit.rows[0].state,
            MaterialState::TextureIndexOutOfRange {
                index: 5,
                available: 2
            },
            "a container-side defect is reported before any archive question"
        );
        assert_eq!(
            audit.rows[1].state,
            MaterialState::ArchiveUnavailable {
                archive: unheld.clone(),
                code: "archive_not_catalogued".to_owned()
            }
        );
        assert_eq!(audit.rows.len(), 2, "the rows are kept, not dropped");
        assert_eq!(audit.rows[1].dependencies, vec![unheld.clone()]);
    }

    /// The container stores one name several times, and a material naming it is
    /// still resolved through that one name — the duplicate is a reason on the
    /// row, not a second state and not a rewritten name.
    #[test]
    fn accept_f10_c_02_audit_reports_a_container_duplicate_as_a_reason() {
        // The measured corpus stores `bldhwk_cowling..tif` 36 times in
        // `planes.zbd`; the archive stores it without the extension.
        let tree = Tree::world(&["bldhwk_cowling"], &["tier"]);
        let (session, catalog, key) = catalog(&tree);
        let meshes = container(vec![container_mesh(0, &[0], &[])]);
        let materials = tables(
            &["bldhwk_cowling..tif", "bldhwk_cowling..tif"],
            vec![material(0, 1, true)],
        );
        assert_eq!(
            materials.duplicate_names(),
            vec![("bldhwk_cowling..tif".to_owned(), vec![0, 1])],
            "the container's own duplicate, reported by the reader"
        );
        let audit =
            MeshDependencyAudit::build(&meshes, &materials, &context(&session, &catalog, &key));

        assert_eq!(audit.rows.len(), 1);
        let row = &audit.rows[0];
        // The archive does not store that exact name, so the row is missing…
        assert!(
            matches!(&row.state, MaterialState::MissingTexture { name, .. } if name == "bldhwk_cowling..tif"),
            "{:?}",
            row.state
        );
        // …and the container-level duplicate is named as its own reason, so a
        // caller can see that no single container entry owns this name. The
        // reason is the bare code and the measured positions are on their own
        // line, because the corpus stores this one name at 36 of them.
        assert!(
            row.unsupported_reasons
                .iter()
                .any(|reason| reason == CONTAINER_DUPLICATE_NAME),
            "{:?}",
            row.unsupported_reasons
        );
        assert!(
            row.reason_details
                .iter()
                .any(|detail| detail.contains("[0, 1]")),
            "the table positions are the evidence: {:?}",
            row.reason_details
        );
    }

    /// The raw material record reaches the audit, and the fingerprint is taken
    /// over exactly the 44 stored bytes, so two materials that differ in one word
    /// have different fingerprints. The audit is a function of the bytes: the
    /// same input twice gives the same rows.
    #[test]
    fn accept_f10_c_02_audit_keeps_the_raw_record_and_hashes_exactly_its_stored_bytes() {
        let tree = Tree::world(&["sky", "ground"], &["tier"]);
        let (session, catalog, key) = catalog(&tree);
        let meshes = container(vec![container_mesh(0, &[0, 1], &[])]);
        let first = material(0, 0, true);
        let mut second = material(1, 1, true);
        second.record.field32 = 0.75; // one word different
        let materials = tables(&["sky", "ground"], vec![first.clone(), second]);
        let audit =
            MeshDependencyAudit::build(&meshes, &materials, &context(&session, &catalog, &key));

        assert_eq!(audit.rows[0].record.as_ref().expect("raw record"), &first);
        let a = audit.rows[0].fingerprint.expect("hashed");
        let b = audit.rows[1].fingerprint.expect("hashed");
        assert_ne!(a, b, "one word different, one fingerprint different");
        assert_eq!(a.to_hex().len(), 64, "a canonical lowercase hex digest");

        let again =
            MeshDependencyAudit::build(&meshes, &materials, &context(&session, &catalog, &key));
        assert_eq!(
            audit.rows, again.rows,
            "the audit is a function of the bytes"
        );
        assert_eq!(audit.references, again.references);
    }

    /// The retail half: the world's own GameZ archive and the world's own texture
    /// archive, read by the production readers and audited together. The
    /// discriminating facts are checked on real data.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f10_c_02_audit_retail_world_resolves_no_name_by_substitution() {
        use cs_formats::ParseContext;

        let game_dir = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
        let found = install::discover(&game_dir).expect("the installation is discovered");
        let group = found
            .diagnosis
            .world_groups
            .iter()
            .find(|group| group.as_str().eq_ignore_ascii_case("ZBD/c1"))
            .expect("world C1 is discovered")
            .clone();
        let context = ResolveContext::new(install::fingerprint(&found.manifest))
            .with_world_group(WorldGroup::from_relative(group));
        let mut builder = SessionBuilder::new(context);
        builder
            .mount_installation(&game_dir, &found.diagnosis)
            .expect("the installation mounts");
        let session = builder.open();

        let key = texture_key();
        let catalog = TextureCatalog::open(&session, std::slice::from_ref(&key));
        assert_eq!(
            catalog.failures().count(),
            0,
            "the world's texture archive opens"
        );

        let relative = "ZBD/C1/gamez.zbd";
        let bytes =
            fs::read(game_dir.join(relative)).expect("the world's GameZ archive is readable");
        let mut parse = ParseContext::with_defaults(relative);
        let meshes = cs_formats::gamez::read_gamez_meshes(&mut parse, relative, &bytes)
            .expect("the mesh section reads");
        let materials = cs_formats::gamez::read_gamez_materials(&mut parse, relative, &bytes)
            .expect("the material section reads");
        // Both readers proved their own section boundary from the same bytes.
        assert_eq!(meshes.data_end, u64::from(meshes.header.nodes_offset));
        assert_eq!(
            materials.data_end,
            u64::from(materials.header.meshes_offset)
        );
        assert!(meshes.findings.is_empty() && materials.findings.is_empty());

        let audit = MeshDependencyAudit::build(
            &meshes,
            &materials,
            &DependencyContext {
                archive: &key,
                session: &session,
                catalog: &catalog,
                origin: None,
                container: relative,
            },
        );

        // F10-B's deferred item 1, for this archive: no stored material index is
        // outside the material table, at either level.
        assert_eq!(
            audit.out_of_range().count(),
            0,
            "every one of the {} stored references is inside the material table",
            audit.references
        );
        assert_eq!(audit.references, meshes.unchecked_material_references);
        assert!(
            !audit.rows.is_empty(),
            "the world stores material references"
        );

        // No row resolved to anything but an exact stored name, and every
        // resolved row names the world's own archive.
        for row in audit.resolved_rows() {
            let MaterialState::Resolved { texture } = &row.state else {
                panic!("a resolved row whose state is {:?}", row.state);
            };
            assert!(
                texture.archive.as_str().ends_with("C1/texture.zbd"),
                "a resolution came from {texture}, not the world's own archive"
            );
            let stored = materials
                .texture_of(
                    materials
                        .material(row.material)
                        .expect("a resolved row has a record"),
                )
                .expect("a resolved row names a stored texture");
            assert_eq!(&texture.name, &stored.name, "the name was altered");
        }
        // The exact-name rule is visible on real data: this world spells a texture
        // `Sky1.tif` in its GameZ container and stores `sky1`.
        let differing = audit
            .rows
            .iter()
            .filter(|row| matches!(&row.state, MaterialState::MissingTexture { .. }))
            .count();
        assert!(
            differing > 0,
            "the world's names are not all stored verbatim"
        );
        let sky = audit
            .rows
            .iter()
            .find(|row| {
                matches!(&row.state, MaterialState::MissingTexture { name, .. } if name == "Sky1.tif")
            })
            .expect("the container's Sky1.tif does not resolve to the archive's sky1");
        assert_eq!(sky.dependencies, vec![key.clone()]);
        assert!(sky.fingerprint.is_some());
        // And the archive really does store the lower-case spelling, so the row
        // is a naming difference and not a missing archive.
        assert!(
            catalog
                .resolve(&session, &TextureRef::new(key.clone(), "sky1"))
                .is_ok(),
            "the world's own archive stores `sky1`"
        );
        // Nothing resolved by a name the archive does not store.
        for row in audit.resolved_rows() {
            let MaterialState::Resolved { texture } = &row.state else {
                unreachable!()
            };
            assert!(
                catalog
                    .resolve(&session, &TextureRef::new(key.clone(), &texture.name))
                    .is_ok(),
                "a row resolved to a name its archive does not store"
            );
        }
    }

    // ============================================ F10-C.03: the wiring tests ===

    /// One stored polygon to author: the position index of every corner, the
    /// material group the polygon stores, and the UV of every corner of that
    /// group.
    ///
    /// [`Self::new`] stores exactly one group; [`Self::with_groups`] stores the
    /// rest, because a polygon that keeps two or three groups is a real stored
    /// shape — the reader keeps them all on `GameZMesh::material_groups`, the
    /// render mesh draws every one of them, and the row reports how many
    /// polygons had more than one as
    /// `multi_material_group_polygons`. Most fixtures store one group, and that
    /// count is a number of stored **polygons**, not of groups.
    struct StoredPolygon {
        corners: Vec<u32>,
        material: u32,
        uvs: Vec<[f32; 2]>,
        /// Stored material groups beyond the first, each its own material index
        /// and its own UV set. The measured corpus stores one group for
        /// 127 728 polygons and two or three for 1 006, and a fixture that could
        /// only ever store one group could not reach a multi-group polygon at
        /// all.
        extra_groups: Vec<(u32, Vec<[f32; 2]>)>,
        /// The flag byte says `FLAG_TRIANGLE_STRIP` rather than one outline.
        ///
        /// A strip is the only corner topology that can repeat a position index
        /// inside one polygon and still decode: `triangulate_polygon` refuses
        /// coincident corners, and the corpus authors its repeated positions in
        /// strips, which go through `decode_strip` instead.
        strip: bool,
        /// The polygon stores no material group at all (`mat_count == 0`), which
        /// the reference asserts against and the measured corpus never does. The
        /// reader keeps the face and reports
        /// `ParseFinding::PolygonWithoutMaterial`; the render gate is where that
        /// costs the mesh.
        no_group: bool,
    }

    impl StoredPolygon {
        /// A polygon whose corners are `corners` and whose UVs are `uvs`, one per
        /// corner, in the same order. It stores exactly one material group.
        fn new(corners: &[u32], material: u32, uvs: &[[f32; 2]]) -> Self {
            assert_eq!(corners.len(), uvs.len(), "one uv per corner");
            Self {
                corners: corners.to_vec(),
                material,
                uvs: uvs.to_vec(),
                extra_groups: Vec::new(),
                strip: false,
                no_group: false,
            }
        }

        /// The same polygon as a triangle **strip** of those corners. A strip
        /// decodes through `decode_strip`, so it may repeat a position index —
        /// which is how the corpus authors a seam inside one polygon.
        fn stored_as_strip(mut self) -> Self {
            self.strip = true;
            self
        }

        /// The same polygon storing `mat_count == 0`: no material index and no
        /// UV set at all, which the layout can still walk.
        fn without_group(mut self) -> Self {
            self.no_group = true;
            self
        }

        /// The same polygon plus `groups` further stored material groups. The
        /// first is the one the reader mirrors onto the IR's single-valued
        /// fields, so a fixture can reproduce a polygon whose second and third
        /// authored UV sets are only reachable through the stored group table.
        fn with_groups(mut self, groups: &[(u32, &[[f32; 2]])]) -> Self {
            self.extra_groups = groups
                .iter()
                .map(|&(material, uvs)| {
                    assert_eq!(uvs.len(), self.corners.len(), "one uv per corner");
                    (material, uvs.to_vec())
                })
                .collect();
            self
        }

        /// How many material groups this polygon stores.
        fn group_count(&self) -> u32 {
            if self.no_group {
                0
            } else {
                1 + self.extra_groups.len() as u32
            }
        }

        /// The packed `vertex_info` word: the corner count in the low nine bits,
        /// the flag byte shifted up by eight. `FLAG_NORMALS` is always set, so
        /// the polygon stores one normal index per corner and no bit outside the
        /// layout's own flag field is set; [`Self::stored_as_strip`] adds the strip bit.
        fn vertex_info(&self) -> u32 {
            let mut flags = FLAG_NORMALS;
            if self.strip {
                flags |= FLAG_TRIANGLE_STRIP;
            }
            (self.corners.len() as u32 & CORNER_COUNT_MASK) | (flags << FLAG_SHIFT)
        }

        /// The ten words of the 40-byte polygon record. The five `*_ptr` values
        /// and the three `unk` words carry a distinct sentinel per field and per
        /// polygon, so a reader that reordered or shifted them is caught instead
        /// of quietly agreeing.
        fn record_bytes(&self, polygon: u32) -> Vec<u8> {
            let mut out = Vec::new();
            for word in [
                self.vertex_info(),
                0,
                0xAAAA_0000 | polygon,
                0xAAAA_1000 | polygon,
                self.group_count(),
                0xAAAA_2000 | polygon,
                0xAAAA_3000 | polygon,
                0xAAAA_4000 | polygon,
                0xAAAA_5000 | polygon,
                0xAAAA_6000 | polygon,
            ] {
                out.extend_from_slice(&word.to_le_bytes());
            }
            assert_eq!(out.len(), 40, "a polygon record is ten 4-byte fields");
            out
        }

        /// The five corner arrays the layout stores after the records: the
        /// position indices, the normal indices (the flag byte says so), the
        /// stored material indices — one per group, `mat_count` of them — then
        /// each group's own UVs, then one colour per corner.
        fn corner_bytes(&self) -> Vec<u8> {
            let mut out = Vec::new();
            for index in &self.corners {
                out.extend_from_slice(&index.to_le_bytes());
            }
            for (corner, _) in self.corners.iter().enumerate() {
                out.extend_from_slice(&(corner as u32 % 2).to_le_bytes());
            }
            // A polygon that stores no group writes neither a material index nor
            // a UV set; the reader then leaves every corner's `uv` unset and
            // mirrors a `0` the bytes never said onto `RawPolygon::material`.
            if !self.no_group {
                out.extend_from_slice(&self.material.to_le_bytes());
            }
            for (material, _) in &self.extra_groups {
                out.extend_from_slice(&material.to_le_bytes());
            }
            // The first group's UVs, then each further group's own set: the
            // reader reads `mat_count` UV sets back to back, so a writer that
            // interleaved them differently would desynchronise the next polygon.
            if !self.no_group {
                for uv in &self.uvs {
                    out.extend_from_slice(&uv[0].to_le_bytes());
                    out.extend_from_slice(&uv[1].to_le_bytes());
                }
            }
            for (_, uvs) in &self.extra_groups {
                for uv in uvs {
                    out.extend_from_slice(&uv[0].to_le_bytes());
                    out.extend_from_slice(&uv[1].to_le_bytes());
                }
            }
            // One distinct colour per corner, so a reader that took the colours
            // from a neighbouring corner is visible on the render vertices.
            for corner in 0..self.corners.len() {
                let value = 16.0 * (corner + 1) as f32;
                for channel in [value, value * 2.0, value * 3.0] {
                    out.extend_from_slice(&channel.to_le_bytes());
                }
            }
            out
        }
    }

    /// One stored mesh to author: its positions, its normals, its polygons, the
    /// 12-byte material references that follow them, and the polygon count its
    /// 100-byte record **declares**.
    ///
    /// The declared count is separate from the data on purpose: a fixture that
    /// declares more polygons than it stores is a mesh whose stored array is
    /// truncated, which is one of the two ways the reader is made to fail with
    /// a byte offset.
    struct StoredMesh {
        positions: Vec<[f32; 3]>,
        normals: Vec<[f32; 3]>,
        polygons: Vec<StoredPolygon>,
        material_refs: Vec<u32>,
        declared_polygons: u32,
    }

    impl StoredMesh {
        /// A present mesh record with the raw scalars one carries, and nothing
        /// declared past its own data.
        fn new(
            positions: Vec<[f32; 3]>,
            normals: Vec<[f32; 3]>,
            polygons: Vec<StoredPolygon>,
        ) -> Self {
            let material_refs = polygons.iter().map(|polygon| polygon.material).collect();
            Self {
                declared_polygons: polygons.len() as u32,
                positions,
                normals,
                polygons,
                material_refs,
            }
        }

        /// The 25 stored words of the 100-byte record, in the layout's order.
        /// Every count but `polygon_count` is derived from the data it composes,
        /// so a fixture cannot contradict itself by accident.
        fn record_words(&self) -> [u32; 25] {
            [
                1, // file_ptr: the reference asserts 0 or 1
                0, // unk04
                0, // unk08
                1, // parent_count: non-zero marks a present record
                self.declared_polygons,
                self.positions.len() as u32,
                self.normals.len() as u32,
                0, // morph_count
                0, // light_count
                0, // unk36
                0.0f32.to_bits(),
                0.0f32.to_bits(),
                0, // unk48
                0, // polygons_ptr
                0, // vertices_ptr
                0, // normals_ptr
                0, // lights_ptr
                0, // morphs_ptr
                0.0f32.to_bits(),
                0.0f32.to_bits(),
                0.0f32.to_bits(),
                0.0f32.to_bits(),
                0, // unk88
                self.material_refs.len() as u32,
                0, // materials_ptr
            ]
        }

        /// The mesh's data, composed from its parts in the layout's order:
        /// positions, normals, morphs, every light header, every light's trailing
        /// vectors, every polygon record, then per polygon its corner arrays, then
        /// the mesh material references. The two two-pass structures are the
        /// layout's and are where an interleaving writer would desynchronise.
        fn data(&self) -> Vec<u8> {
            let mut out = Vec::new();
            for vector in self.positions.iter().chain(&self.normals) {
                for channel in vector {
                    out.extend_from_slice(&channel.to_le_bytes());
                }
            }
            for (polygon, _) in self.polygons.iter().enumerate() {
                out.extend_from_slice(&self.polygons[polygon].record_bytes(polygon as u32));
            }
            for polygon in &self.polygons {
                out.extend_from_slice(&polygon.corner_bytes());
            }
            for material in &self.material_refs {
                for word in [*material, 1, 0] {
                    out.extend_from_slice(&word.to_le_bytes());
                }
            }
            out
        }
    }

    /// One stored texture-name record: three `u32` words, the 20-byte name and
    /// three more words, exactly as `read_gamez_materials` reads them.
    fn texture_name(name: &str) -> Vec<u8> {
        assert!(name.len() < 20, "a fixture name fits the 20-byte field");
        let mut out = Vec::new();
        for word in [0u32, 0, 0] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        let mut field = [0u8; 20];
        field[..name.len()].copy_from_slice(name.as_bytes());
        // The layout's one established encoding rule: the stored NUL stands where
        // the `.` of an extension was, and a name with no suffix stores a NUL
        // there and another after it.
        field[name.len()] = 0;
        out.extend_from_slice(&field);
        for word in [2u32, 0, -1i32 as u32] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        assert_eq!(out.len(), 44, "a texture-name record is 44 bytes");
        out
    }

    /// One present material record plus its two link words: the 40 stored bytes
    /// and the 4 that follow them.
    ///
    /// The values are the reference's asserted profile, so the reader raises no
    /// `MaterialFinding` for them, and `field32` — the word the reference calls
    /// `specular` and newer classification calls soil — is left at a distinct
    /// value per material.
    fn material_slot(index: u32, count: u32, texture_index: u32) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(0xFF); // alpha
        out.push(MATERIAL_FLAG_ALWAYS | MATERIAL_FLAG_TEXTURED); // flags
        out.extend_from_slice(&0x7FFFu16.to_le_bytes()); // rgb
        for _ in 0..3 {
            out.extend_from_slice(&255.0f32.to_le_bytes()); // color
        }
        out.extend_from_slice(&texture_index.to_le_bytes());
        for value in [0.0f32, 0.5, 0.5, 0.25 + index as f32] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&0u32.to_le_bytes()); // cycle_ptr: not cycled
        assert_eq!(out.len(), 40, "a material record is forty bytes");
        let link1: i16 = if index + 1 >= count {
            -1
        } else {
            (index + 1) as i16
        };
        let link2: i16 = if index == 0 { -1 } else { (index - 1) as i16 };
        out.extend_from_slice(&link1.to_le_bytes());
        out.extend_from_slice(&link2.to_le_bytes());
        assert_eq!(
            out.len(),
            44,
            "a material slot is forty bytes and two words"
        );
        out
    }

    /// One zero material slot, of which a container stores
    /// `1000 - count` of them. Their link words are the reference's own rule,
    /// which is the other way round from a present slot's.
    fn zero_material_slot(index: u32, count: u32) -> Vec<u8> {
        let mut out = vec![0u8; 40];
        let link1: i16 = if index == count {
            -1
        } else {
            (index - 1) as i16
        };
        let link2: i16 = if index + 1 >= NG_MATERIAL_SLOTS {
            -1
        } else {
            (index + 1) as i16
        };
        out.extend_from_slice(&link1.to_le_bytes());
        out.extend_from_slice(&link2.to_le_bytes());
        out
    }

    /// A whole CS GameZ container, authored here from the layout recorded in
    /// `docs/findings/2026-09-29-f10-b-gamez-mesh-layout.md` and
    /// `docs/findings/2026-09-29-f10-c-02-gamez-material-records.md`.
    ///
    /// `textures` are the container's stored texture names, `materials` how many
    /// present material records there are (material `i` names texture `i`), and
    /// `meshes` the stored mesh records in array order — an `Err(())` is an
    /// absent stub slot, which stores the expected index of the next present
    /// mesh instead of a data offset.
    ///
    /// The writer lays the sections out in the order the layout requires and
    /// fills in every offset, so no fixture depends on a hand-computed byte
    /// count, and the file ends exactly at `nodes_offset` with an empty node
    /// array. The writer shares no code with either reader.
    fn gamez_container(
        textures: &[&str],
        materials: u32,
        meshes: &[Result<StoredMesh, ()>],
    ) -> Vec<u8> {
        let texture_of: Vec<u32> = (0..materials).collect();
        gamez_container_with(textures, &texture_of, meshes)
    }

    /// [`gamez_container`] with the container's material table spelled out:
    /// `texture_of[i]` is the texture index the present material record `i`
    /// stores, so a fixture can point two materials at the same name, or at an
    /// index the container's texture table does not have.
    fn gamez_container_with(
        textures: &[&str],
        texture_of: &[u32],
        meshes: &[Result<StoredMesh, ()>],
    ) -> Vec<u8> {
        let materials = texture_of.len() as u32;
        let header_bytes = 40usize;
        assert_eq!(textures_offset(), header_bytes as u32);
        let textures_offset = header_bytes;
        let materials_offset = textures_offset + textures.len() * 44;
        let material_section = 16 + NG_MATERIAL_SLOTS as usize * 44;
        let meshes_offset = materials_offset + material_section;
        let record_bytes = 100 + 4;
        let index_bytes = 12 + meshes.len() * record_bytes;
        let data: Vec<Vec<u8>> = meshes
            .iter()
            .map(|slot| match slot {
                Ok(mesh) => mesh.data(),
                Err(()) => Vec::new(),
            })
            .collect();
        let data_len: usize = data.iter().map(Vec::len).sum();
        let nodes_offset = meshes_offset + index_bytes + data_len;
        assert!(
            nodes_offset <= u32::MAX as usize,
            "a fixture this large is not a fixture"
        );

        let present: Vec<u32> = meshes
            .iter()
            .enumerate()
            .filter_map(|(slot, mesh)| mesh.as_ref().ok().map(|_| slot as u32))
            .collect();
        let array_size = meshes.len() as i32;
        let count = present.len() as i32;
        let last_index = present.last().copied().map_or(-1, |slot| {
            let next = slot + 1;
            if next as i32 == array_size {
                -1
            } else {
                next as i32
            }
        });

        let mut out = Vec::new();
        for word in [
            GAMEZ_SIGNATURE,
            GAMEZ_VERSION,
            1_234_567_890, // unk08: neither measured fixup table, so `Fixup::None`
            textures.len() as u32,
            textures_offset as u32,
            materials_offset as u32,
            meshes_offset as u32,
            0, // node_array_size
            0, // light_index
            nodes_offset as u32,
        ] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        assert_eq!(out.len(), header_bytes, "the header is ten 4-byte fields");

        for name in textures {
            out.extend_from_slice(&texture_name(name));
        }
        for word in [
            materials as i32,
            materials as i32,
            materials as i32,
            materials as i32 - 1,
        ] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        for (index, texture_index) in texture_of.iter().enumerate() {
            out.extend_from_slice(&material_slot(index as u32, materials, *texture_index));
        }
        for index in materials..NG_MATERIAL_SLOTS {
            out.extend_from_slice(&zero_material_slot(index, materials));
        }
        assert_eq!(
            out.len(),
            meshes_offset,
            "the material section ends where the mesh index starts"
        );

        for word in [array_size, count, last_index] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        // The record array, each present record's data offset filled in from the
        // composed data that follows the whole array. An absent slot's trailing
        // word is the expected index of the next present mesh, not an offset.
        let mut offset = (out.len() + meshes.len() * record_bytes) as u32;
        for (slot, mesh) in meshes.iter().enumerate() {
            match mesh {
                Ok(mesh) => {
                    for word in mesh.record_words() {
                        out.extend_from_slice(&word.to_le_bytes());
                    }
                    out.extend_from_slice(&offset.to_le_bytes());
                    offset += data[slot].len() as u32;
                }
                Err(()) => {
                    out.extend_from_slice(&[0u8; 100]);
                    let next = slot as i32 + 1;
                    let expected = if next == array_size { -1 } else { next };
                    out.extend_from_slice(&expected.to_le_bytes());
                }
            }
        }
        for body in &data {
            out.extend_from_slice(body);
        }
        assert_eq!(
            out.len(),
            nodes_offset,
            "the mesh data ends at nodes_offset"
        );
        out
    }

    fn textures_offset() -> u32 {
        cs_formats::gamez::GAMEZ_HEADER_BYTES as u32
    }

    /// `count` distinct `Vec3`s from one **named block**, so the positions and
    /// the normals of the same mesh never hold the same numbers. The two arrays
    /// are adjacent and equally sized, so a reader that swapped them would
    /// consume exactly the right bytes, walk to `nodes_offset` and satisfy every
    /// count — it would only be wrong.
    fn block(base: f32, count: usize) -> Vec<[f32; 3]> {
        (0..count)
            .map(|index| {
                let value = (index + 1) as f32;
                [base + value, base + value * 2.0, base + value * 3.0]
            })
            .collect()
    }

    /// The quad of the seam fixture, in stored order: two triangles that share
    /// positions 1 and 2, where position 2 is authored with a different UV on
    /// each polygon.
    ///
    /// ```text
    /// 2 (0,1) ---- 3 (1,1)
    ///   |        / |
    ///   |      /   |
    /// 0 (0,0) ---- 1 (1,0)
    /// ```
    fn seam_mesh() -> StoredMesh {
        StoredMesh::new(
            block(0.0, 4),
            block(1000.0, 4),
            vec![
                StoredPolygon::new(&[0, 1, 2], 0, &[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]),
                StoredPolygon::new(&[2, 1, 3], 0, &[[0.5, 0.5], [1.0, 0.0], [1.0, 1.0]]),
            ],
        )
    }

    /// A one-triangle mesh whose single material names the second texture name,
    /// which the world's archive does not store.
    fn missing_material_mesh() -> StoredMesh {
        StoredMesh::new(
            block(2000.0, 3),
            block(3000.0, 3),
            vec![StoredPolygon::new(
                &[0, 1, 2],
                1,
                &[[0.0, 0.0], [0.5, 0.0], [0.25, 0.25]],
            )],
        )
    }

    /// The world's `gamez.zbd`: two present meshes and one absent stub slot.
    ///
    /// Material 0 names texture 0 (`sky`, which the world's texture archive
    /// stores); material 1 names texture 1 (`Sky1.tif`, which it does not).
    fn seam_container() -> Vec<u8> {
        gamez_container(
            &["sky", "Sky1.tif"],
            2,
            &[
                Ok(seam_mesh()),
                Ok(missing_material_mesh()),
                Err(()),
                Ok(StoredMesh::new(block(0.0, 1), Vec::new(), vec![])),
            ],
        )
    }

    /// The world's `gamez.zbd` when one mesh reaches **three** materials whose
    /// blocking reasons are the same code, the same other code, the same code
    /// again: material 0 and material 2 both name a texture the world's archive
    /// does not store, and material 1 stores a texture index the container's own
    /// texture table does not have.
    fn repeated_reason_container() -> Vec<u8> {
        let polygons = |material: u32| {
            StoredPolygon::new(
                &[0, 1, 2],
                material,
                &[[0.0, 0.0], [0.5, 0.0], [0.25, 0.25]],
            )
        };
        let mesh = StoredMesh::new(
            block(0.0, 3),
            block(1000.0, 3),
            vec![polygons(0), polygons(1), polygons(2)],
        );
        gamez_container_with(
            &["Sky1.tif", "sky", "Sky2.tif"],
            // Material 1's index is past the three stored names on purpose: it
            // is the different reason that separates the two missing textures.
            &[0, 9, 2],
            &[Ok(mesh)],
        )
    }

    /// The world's `gamez.zbd` when one mesh stores **two** material groups on
    /// each of its two polygons. The CS layout stores one UV set per group, so a
    /// second group is a second UV set and a second raw material index for the
    /// same corners. Both are kept: the render mesh draws each group, so this
    /// container is what `accept_f10_e_` reaches the group code through.
    fn multi_group_container() -> Vec<u8> {
        let group_uvs = [[0.75, 0.75], [0.5, 0.5], [0.25, 0.25]];
        let mesh = StoredMesh::new(
            block(0.0, 4),
            block(1000.0, 4),
            vec![
                StoredPolygon::new(&[0, 1, 2], 0, &[[0.0, 0.0], [0.5, 0.0], [0.25, 0.25]])
                    .with_groups(&[(1, &group_uvs)]),
                StoredPolygon::new(&[2, 1, 3], 0, &[[0.5, 0.5], [1.0, 0.0], [1.0, 1.0]])
                    .with_groups(&[(1, &group_uvs)]),
            ],
        );
        gamez_container_with(&["sky"], &[0], &[Ok(mesh)])
    }

    // ================================================ F10-E: the group fixtures ===

    /// The seven corners of the F10-E strip, in stored order. Positions 1, 2 and
    /// 3 each appear twice, which is how the corpus authors a seam inside one
    /// polygon: a strip may repeat a position index and still decode, and an
    /// outline may not.
    const STRIP_CORNERS: [u32; 7] = [0, 1, 2, 3, 2, 1, 3];

    /// The strip's first-group UV set: position 2 is `[0.0, 1.0]` at both of its
    /// corners, position 1 is `[1.0, 0.0]` at both of its corners and position 3
    /// is `[1.0, 1.0]` at both of its corners. **No seam**: every repeated
    /// position carries the same coordinate in this group.
    const STRIP_GROUP_ZERO: [[f32; 2]; 7] = [
        [0.0, 0.0],
        [1.0, 0.0],
        [0.0, 1.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [1.0, 0.0],
        [1.0, 1.0],
    ];

    /// The same strip's **second**-group UV set. Positions 1 and 3 still repeat
    /// without a seam, but position 2 is `[0.0, 1.0]` at corner 2 and
    /// `[0.2, 0.9]` at corner 4: a seam that exists **only** in this group, and
    /// that the first group alone would have hidden.
    const STRIP_GROUP_ONE: [[f32; 2]; 7] = [
        [0.0, 0.1],
        [1.0, 0.0],
        [0.0, 1.0],
        [1.0, 1.0],
        [0.2, 0.9],
        [1.0, 0.0],
        [1.0, 1.0],
    ];

    /// The F10-E group mesh, mesh `0` of [`group_tree`]'s container.
    ///
    /// Three polygons over four positions, chosen so that the three ways a group
    /// can differ from another are all present:
    ///
    /// * **polygon 0** is a seven-corner strip with **two** groups. Its second
    ///   group authors a UV seam at position 2 that its first group does not, so
    ///   AC03 has to be re-asserted for the second group here. Its last strip
    ///   step is degenerate, so the per-group degeneracy has to stay exact.
    /// * **polygon 1** is a triangle with **three** groups whose first two store
    ///   the *same* UV set under different raw material indices: those two groups
    ///   must still split, on the material alone.
    /// * **polygon 2** is a triangle with **one** group, so a single-group polygon
    ///   shares the mesh and a row for it can have no multi-group question.
    fn group_mesh() -> StoredMesh {
        StoredMesh::new(
            block(0.0, 4),
            block(1000.0, 4),
            vec![
                StoredPolygon::new(&STRIP_CORNERS, 0, &STRIP_GROUP_ZERO)
                    .stored_as_strip()
                    .with_groups(&[(1, &STRIP_GROUP_ONE)]),
                StoredPolygon::new(&[0, 1, 2], 0, &[[0.0, 0.0], [0.5, 0.0], [0.25, 0.25]])
                    .with_groups(&[
                        (1, &[[0.0, 0.0], [0.5, 0.0], [0.25, 0.25]]),
                        (2, &[[0.0, 0.5], [0.5, 0.5], [0.25, 0.75]]),
                    ]),
                StoredPolygon::new(&[1, 3, 2], 3, &[[0.0, 0.0], [1.0, 0.0], [0.5, 0.5]]),
            ],
        )
    }

    /// The world's `gamez.zbd` holding two present meshes: the group mesh and the
    /// single-group seam mesh, so one catalog carries a row that has a multi-group
    /// question beside a row that has none.
    ///
    /// All four texture names are stored and every material resolves, so the only
    /// open reason on either row is a presentation one.
    fn group_container() -> Vec<u8> {
        gamez_container_with(
            &["sky", "ground", "tier", "stone"],
            &[0, 1, 2, 3],
            &[Ok(group_mesh()), Ok(seam_mesh())],
        )
    }

    /// [`group_container`] over a world's own texture archive, with the archive
    /// the audit must **not** search holding a decoy name.
    fn group_tree() -> Tree {
        let tree = Tree::world(&["sky", "ground", "tier", "stone"], &["decoy"]);
        tree.write("ZBD/c1/gamez.zbd", &group_container());
        tree
    }

    /// The world's `gamez.zbd` when one polygon stores `mat_count == 0`: the
    /// reference asserts against it and the measured corpus never does, so this
    /// is the only way to reach the render gate's
    /// `PolygonWithoutMaterialGroup` through the production reader.
    fn no_group_container() -> Vec<u8> {
        let broken = StoredMesh::new(
            block(0.0, 3),
            block(1000.0, 3),
            vec![
                StoredPolygon::new(&[0, 1, 2], 0, &[[0.0, 0.0], [0.5, 0.0], [0.25, 0.25]])
                    .without_group(),
                StoredPolygon::new(&[2, 1, 0], 1, &[[0.0, 0.0], [0.5, 0.0], [0.25, 0.25]]),
            ],
        );
        gamez_container_with(&["sky", "ground"], &[0, 1], &[Ok(broken)])
    }

    /// The render triangles one polygon and one stored group produced, in
    /// drawing order. The `group` field is part of the lookup so a test cannot
    /// accidentally read another group's triangles.
    fn group_triangles(render: &RenderMesh, polygon: usize, group: usize) -> Vec<&RenderTriangle> {
        render
            .triangles()
            .iter()
            .filter(|t| t.source.polygon == polygon && t.group == group)
            .collect()
    }

    /// The render vertices one polygon and one stored group produced, in
    /// first-encounter order.
    ///
    /// A vertex two corners agree on bit-exactly is shared, whether they belong
    /// to two groups of one polygon or to two polygons, so the set is not a
    /// partition. [`group_vertex_slots`] is the reading that keeps the corner a
    /// vertex was reached for.
    fn group_vertices(render: &RenderMesh, polygon: usize, group: usize) -> Vec<&RenderVertex> {
        let mut out: Vec<&RenderVertex> = Vec::new();
        for triangle in group_triangles(render, polygon, group) {
            for &index in &triangle.vertices {
                let vertex = &render.vertices()[index as usize];
                if !out.iter().any(|kept| std::ptr::eq(*kept, vertex)) {
                    out.push(vertex);
                }
            }
        }
        out
    }

    /// Every `(stored corner, render vertex)` one polygon and one stored group
    /// produced, paired from the stored topology's own source map.
    ///
    /// A vertex may be shared with another corner, another group or another
    /// polygon when they agree bit-exactly, so the corner is the *address* the
    /// vertex was reached for and the only thing a test can compare a stored
    /// coordinate against. The render triangles of one `(polygon, group)` are
    /// the stored topology triangles of that polygon in order, which is what
    /// makes the pairing a statement rather than a coincidence.
    fn group_vertex_slots<'a>(
        render: &'a RenderMesh,
        topology: &MeshTopology,
        polygon: usize,
        group: usize,
    ) -> Vec<(usize, &'a RenderVertex)> {
        let sources: Vec<MeshTriangle> = topology
            .triangles
            .iter()
            .filter(|t| t.polygon == polygon)
            .copied()
            .collect();
        let drawn = group_triangles(render, polygon, group);
        assert_eq!(
            drawn.len(),
            sources.len(),
            "polygon {polygon} group {group}: one render triangle per stored topology triangle"
        );
        let mut out = Vec::new();
        for (triangle, source) in drawn.iter().zip(&sources) {
            assert_eq!(triangle.source.step, source.step);
            for (slot, &index) in triangle.vertices.iter().enumerate() {
                out.push((source.corners[slot], &render.vertices()[index as usize]));
            }
        }
        out
    }

    /// Fails unless every corner one polygon and one stored group draws with
    /// samples **that** group's own coordinate, and the vertex it lands on
    /// carries that group's own raw material index.
    fn assert_group_samples_its_own_uvs(
        render: &RenderMesh,
        stored: &GameZMesh,
        topology: &MeshTopology,
        polygon: usize,
        group: usize,
    ) {
        let attributes = &stored.groups(polygon).expect("groups")[group];
        for (corner, vertex) in group_vertex_slots(render, topology, polygon, group) {
            let uv = vertex
                .uv
                .expect("a stored group always stores a coordinate");
            assert_eq!(
                uv, attributes.uvs[corner],
                "polygon {polygon} group {group}: corner {corner} samples this group's own \
                 coordinate"
            );
            assert_eq!(
                vertex.material, attributes.material,
                "polygon {polygon} group {group}: the vertex carries this group's material"
            );
        }
    }

    /// The distinct stored texture coordinates one polygon and one group
    /// produced at one position index, bit-exact and sorted.
    fn uvs_at(
        render: &RenderMesh,
        topology: &MeshTopology,
        polygon: usize,
        group: usize,
        position: u32,
    ) -> Vec<[u32; 2]> {
        let mut bits: Vec<[u32; 2]> = group_vertex_slots(render, topology, polygon, group)
            .into_iter()
            .filter(|(_, vertex)| vertex.position_index == position)
            .map(|(_, vertex)| {
                let uv = vertex
                    .uv
                    .expect("a stored group always stores a coordinate");
                [uv[0].to_bits(), uv[1].to_bits()]
            })
            .collect();
        bits.sort_unstable();
        bits.dedup();
        bits
    }

    fn gamez_key() -> AssetKey {
        world_key("gamez.zbd")
    }

    /// The world's `gamez.zbd` plus the world's own `texture.zbd`, so the audit
    /// has a real archive to look `sky` up in and a real `rtexture2.zbd` it must
    /// not look in.
    fn seam_tree() -> Tree {
        let tree = Tree::world(&["sky", "ground"], &["Sky1.tif", "tier"]);
        tree.write("ZBD/c1/gamez.zbd", &seam_container());
        tree
    }

    fn seam_dependencies<'a>(
        textures: &'a TextureCatalog,
        archive: &'a AssetKey,
    ) -> MeshDependencies<'a> {
        MeshDependencies { archive, textures }
    }

    /// The UVs a render triangle samples, by its source polygon, in drawing
    /// order, taken from an **upload payload** rather than from the render mesh.
    fn uploaded_uvs(upload: &MeshUpload, polygon: usize) -> [[f32; 2]; 3] {
        let render = upload.render();
        let triangle = render
            .triangles()
            .iter()
            .find(|triangle| triangle.source.polygon == polygon)
            .expect("a triangle for that polygon");
        triangle
            .vertices
            .map(|index| render.vertices()[index as usize].uv.expect("authored uv"))
    }

    /// The render vertices sitting at one stored position index, in first-
    /// encounter order.
    fn vertices_at(upload: &MeshUpload, position: u32) -> Vec<&RenderVertex> {
        upload
            .render()
            .vertices()
            .iter()
            .filter(|vertex| vertex.position_index == position)
            .collect()
    }

    /// **AC03 end to end.** A quad read by the production reader out of a
    /// container the production VFS, the production ZBD dispatch and the
    /// production GameZ reader each opened reaches the upload payload with its
    /// authored per-corner UV seam intact: position 2 keeps both UVs, each
    /// triangle samples its own polygon's, and nothing welded them on the way.
    ///
    /// A parallel test-only path would pass this if the splitter were correct
    /// and the wiring were absent, so every step here is the production one:
    /// `AssetKey` → `ContentSession` → `ZbdContainer` → [`read_gamez_meshes`] →
    /// [`RenderMesh`] → [`MeshCatalog::prepare_upload`].
    #[test]
    fn accept_f10_c_03_uv_seam_survives_the_container_to_upload_boundary() {
        let tree = seam_tree();
        let session = world_session(&tree.0, "ZBD/c1");
        let textures = TextureCatalog::open(&session, std::slice::from_ref(&texture_key()));
        assert_eq!(textures.failures().count(), 0, "the fixture archive opens");
        let archive = texture_key();
        let dependencies = seam_dependencies(&textures, &archive);
        let catalog = MeshCatalog::open(&session, &[gamez_key()], &dependencies);
        assert_eq!(catalog.failures().count(), 0, "the fixture container opens");
        let container = catalog.containers().next().expect("one container");
        assert_eq!(
            container.meshes().present_count(),
            3,
            "three present meshes"
        );

        // -- the catalog rows, with the contract's fields ---------------------
        let records = catalog.records();
        assert_eq!(records.len(), 3, "one row per stored mesh, not per slot");
        let seam = &records[0];
        assert_eq!(seam.kind, "render_mesh");
        assert_eq!(seam.mesh_index, Some(0));
        let id = seam.id.as_ref().expect("a stored mesh has an id");
        assert_eq!(id.index, 0);
        assert_eq!(id.container.as_str(), "ZBD/c1/gamez.zbd");
        assert!(seam.origin.is_some(), "the origin is the container's span");
        assert_eq!(
            seam.dependencies,
            vec![gamez_key(), archive.clone()],
            "the container and the one archive the audit searched"
        );
        assert_eq!(seam.runtime_consumers, vec![MESH_UPLOAD_CONSUMER]);
        assert_eq!(seam.parse_state, ParseState::Parsed);
        assert_eq!(seam.faces.expect("face counts").faces, 2);
        assert_eq!(seam.faces.expect("face counts").triangles, 2);
        assert_eq!(seam.faces.expect("face counts").rejected, 0);
        assert_eq!(seam.faces.expect("face counts").degenerate, 0);
        // The fingerprint is over the stored span the reader walked for this
        // mesh, so it is a function of the bytes and not of this stage's
        // splitting: a second catalog over the same bytes agrees.
        let stored = container.meshes().get(0).expect("mesh 0");
        let span =
            &container.container_bytes()[stored.data_offset as usize..stored.data_end as usize];
        assert_eq!(seam.fingerprint, Some(sha256(span)));
        // `sky` resolved and every presentation decision is still open, so the
        // row is Blocked with exactly the three codes and no material code.
        assert_eq!(seam.readiness, RenderMeshReadiness::Blocked);
        assert_eq!(
            seam.unsupported_reasons,
            vec![
                "front_face_winding_unknown".to_owned(),
                "uv_origin_unknown".to_owned(),
                "vertex_color_unknown".to_owned(),
            ]
        );
        // The mesh was built, so its normalization did not fail: what is open is
        // presentation, which is `readiness`, not a parse or a normalize
        // failure. The same split F08-C makes for a decoded image with open
        // unknowns.
        assert_eq!(
            seam.normalize_state,
            ParseState::Parsed,
            "a render mesh that uploads is not a normalization failure"
        );
        assert!(seam.failure.is_none(), "a complete mesh has no failure");

        // The second mesh's material names `Sky1.tif`, which the archive does
        // not store, so its row names that and its own upload carries the
        // refused row rather than a substituted texture. An unresolved
        // dependency is a readiness reason, not a normalization failure.
        let missing = &records[1];
        assert_eq!(missing.mesh_index, Some(1));
        assert_eq!(
            missing.unsupported_reasons,
            vec![
                "texture_not_found".to_owned(),
                "front_face_winding_unknown".to_owned(),
                "uv_origin_unknown".to_owned(),
                "vertex_color_unknown".to_owned(),
            ]
        );
        assert_eq!(missing.readiness, RenderMeshReadiness::Blocked);
        assert_eq!(
            missing.normalize_state,
            ParseState::Parsed,
            "an unresolved texture blocks the row; it does not fail it"
        );
        assert_eq!(
            records[2].mesh_index,
            Some(3),
            "the absent slot at 2 is not a row, and slot 3 is"
        );
        assert_eq!(catalog.generation(), session.generation());
        assert_eq!(catalog.archive(), &archive);

        // -- the seam, at the upload boundary -------------------------------
        let resolved = catalog
            .resolve(&session, &gamez_key(), 0)
            .expect("mesh 0 resolves");
        assert_eq!(resolved.id(), id);
        assert_eq!(resolved.container_key(), &gamez_key());
        assert_eq!(resolved.generation(), catalog.generation());
        assert!(
            !resolved.trace().attempts.is_empty(),
            "the VFS trace travels with it"
        );
        let upload = catalog
            .prepare_upload(&session, &resolved)
            .expect("mesh 0 uploads");

        assert_eq!(
            upload.render().vertices().len(),
            5,
            "position 2 carries two UVs"
        );
        assert_eq!(upload.render().source_faces(), 2);
        assert_eq!(upload.render().source_triangles(), 2);
        assert_eq!(upload.render().degenerate_triangles(), 0);
        let at_one: Vec<&RenderVertex> = vertices_at(&upload, 1);
        assert_eq!(at_one.len(), 1, "the shared position 1 merges");
        assert_eq!(at_one[0].uv, Some([1.0, 0.0]));
        let at_two: Vec<[f32; 2]> = vertices_at(&upload, 2)
            .into_iter()
            .map(|vertex| vertex.uv.expect("authored uv"))
            .collect();
        assert_eq!(at_two.len(), 2, "the seam survived the whole path");
        assert!(at_two.contains(&[0.0, 1.0]));
        assert!(at_two.contains(&[0.5, 0.5]));
        assert_eq!(
            uploaded_uvs(&upload, 0),
            [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]
        );
        assert_eq!(
            uploaded_uvs(&upload, 1),
            [[0.5, 0.5], [1.0, 0.0], [1.0, 1.0]]
        );

        // The payload carries the mesh's own audit row and nothing else, so a
        // payload for mesh 0 cannot imply mesh 1's missing texture.
        assert_eq!(upload.materials().len(), 1);
        let material = upload.material(0).expect("material 0 is audited");
        assert_eq!(material.material, 0);
        assert_eq!(material.id, "gamez.materials[0]");
        assert_eq!(material.state.code(), "resolved");
        assert!(material.fingerprint.is_some());
        assert!(material.used_by.iter().all(|use_| use_.mesh == 0));
        assert_eq!(upload.material(1), None);
        assert_eq!(upload.faces().faces, 2);
        assert_eq!(upload.container_key(), &gamez_key());
        assert!(
            upload.container_span().length() > 0,
            "the origin has a length"
        );
        assert_eq!(
            upload.unknowns(),
            &[
                MeshPresentationUnknown::FrontFaceWinding,
                MeshPresentationUnknown::UvOrigin,
                MeshPresentationUnknown::VertexColor,
            ]
        );
        assert!(!upload.is_release_ready(), "the open decisions are named");

        // Mesh 1's payload carries the refused row, with the exact name and the
        // exact archive, and resolves to no texture at all.
        let second = catalog.resolve(&session, &gamez_key(), 1).expect("mesh 1");
        let second = catalog
            .prepare_upload(&session, &second)
            .expect("mesh 1 uploads its own row");
        assert_eq!(second.materials().len(), 1);
        assert_eq!(
            second.material(1).expect("material 1 is audited").state,
            MaterialState::MissingTexture {
                name: "Sky1.tif".to_owned(),
                archive: archive.clone(),
            }
        );
        // The payload names the container its own resolution was made with.
        assert_eq!(second.container_key(), &gamez_key());
        assert!(
            second
                .material(1)
                .expect("audited")
                .state
                .texture()
                .is_none()
        );
    }

    /// A container whose file is truncated becomes a failed catalog row that
    /// keeps the reader's own offset and container, is retried in place, is
    /// **not** recovered by repairing the file under a session that already
    /// mounted it, and loads on a remount. The same container is refused through
    /// a stale session, and an absent mesh slot is refused rather than guessed.
    #[test]
    fn accept_f10_c_03_truncated_container_is_a_failed_row_and_recovers_after_remount() {
        let tree = seam_tree();
        let full = seam_container();
        // Cut the file inside the mesh data. The header's section chain still
        // describes the whole container, so the reader names the offset where the
        // mesh data claims to end as the offset it ran out at.
        let mut truncated = full.clone();
        truncated.truncate(full.len() - 8);
        tree.write("ZBD/c1/gamez.zbd", &truncated);
        let nodes_offset =
            u32::from_le_bytes(full[36..40].try_into().expect("four bytes")) as usize;
        assert!(
            nodes_offset > truncated.len(),
            "the cut is inside the section"
        );

        let session = world_session(&tree.0, "ZBD/c1");
        let textures = TextureCatalog::open(&session, std::slice::from_ref(&texture_key()));
        let archive = texture_key();
        let dependencies = seam_dependencies(&textures, &archive);
        let mut catalog = MeshCatalog::open(&session, &[gamez_key()], &dependencies);
        assert_eq!(catalog.containers().count(), 0, "nothing read");

        let records = catalog.records();
        assert_eq!(records.len(), 1, "the failed container is a row");
        let row = &records[0];
        assert_eq!(row.kind, "render_mesh");
        assert_eq!(row.id, None, "a container that read no mesh has no id");
        assert_eq!(row.mesh_index, None);
        assert_eq!(row.container_key, gamez_key());
        assert!(row.origin.is_some(), "the container's bytes did exist");
        assert_eq!(row.fingerprint, None, "no stored span was walked");
        assert_eq!(row.readiness, RenderMeshReadiness::Failed);
        assert_eq!(row.runtime_consumers, vec![MESH_UPLOAD_CONSUMER]);
        assert_eq!(row.normalize_state, ParseState::Unparsed);
        let failure = row.failure.as_ref().expect("the reader's own context");
        assert_eq!(failure.stage, MeshFailureStage::Meshes);
        assert_eq!(failure.code, "section_out_of_bounds");
        assert_eq!(
            failure.offset,
            Some(nodes_offset as u64),
            "the reader's offset"
        );
        assert_eq!(failure.mesh, None, "the container refused, not one mesh");
        assert!(
            failure.container.contains("gamez.zbd"),
            "the container is named: {}",
            failure.container
        );
        assert!(
            row.unsupported_reasons
                .contains(&"mesh_section_failed".to_owned())
        );
        assert!(
            row.unsupported_reasons
                .contains(&"section_out_of_bounds".to_owned())
        );
        // The reader's own message, offsets included, is the row's diagnostic
        // and the failure's, not a reworded summary.
        let ParseState::Failed { diagnostic } = &row.parse_state else {
            panic!("a refused container is a failed parse");
        };
        assert_eq!(diagnostic, &failure.diagnostic);
        assert!(
            diagnostic.contains(&nodes_offset.to_string()),
            "{diagnostic}"
        );
        assert!(
            failure.to_string().contains(&nodes_offset.to_string()),
            "the failure's own text carries the offset: {failure}"
        );

        // A stale session cannot retry this catalog, resolve through it, or be
        // served by it. A second session over the same tree is a new generation,
        // which is exactly what makes the first catalog stale to it.
        let other = world_session(&tree.0, "ZBD/c1");
        assert_ne!(other.generation(), session.generation());
        assert_eq!(
            catalog
                .retry_failed(&other, &dependencies)
                .expect_err("foreign")
                .code(),
            "foreign_session"
        );
        assert_eq!(
            catalog
                .resolve(&other, &gamez_key(), 0)
                .expect_err("foreign")
                .code(),
            "foreign_session"
        );
        // A failed container's code is the **reader's** code, not a generic
        // "failed", so a caller can tell a truncated file from a missing one.
        assert_eq!(
            catalog
                .resolve(&session, &gamez_key(), 0)
                .expect_err("the container failed")
                .code(),
            "section_out_of_bounds"
        );
        assert_eq!(
            catalog
                .resolve(&session, &world_key("planes.zbd"), 0)
                .expect_err("not catalogued")
                .code(),
            "container_not_catalogued"
        );

        // Retrying the same session still fails, and repairing the file does not
        // change what that session mounted: the retry fails on the mount's record
        // of the bytes, not on the new ones.
        assert_eq!(catalog.retry_failed(&session, &dependencies), Ok(1));
        tree.write("ZBD/c1/gamez.zbd", &full);
        assert_eq!(catalog.retry_failed(&session, &dependencies), Ok(1));
        assert_eq!(catalog.records().len(), 1, "still one failed row");

        // A remount of the repaired tree, and a new catalog over it, loads the
        // fix. The old catalog keeps its refusal: it belongs to the old session.
        let session = world_session(&tree.0, "ZBD/c1");
        let textures = TextureCatalog::open(&session, std::slice::from_ref(&texture_key()));
        let dependencies = seam_dependencies(&textures, &archive);
        let catalog = MeshCatalog::open(&session, &[gamez_key()], &dependencies);
        assert_eq!(catalog.failures().count(), 0);
        assert_eq!(catalog.records().len(), 3);
        let resolved = catalog
            .resolve(&session, &gamez_key(), 0)
            .expect("recovered");
        let upload = catalog
            .prepare_upload(&session, &resolved)
            .expect("the repaired mesh uploads");
        assert_eq!(upload.render().vertices().len(), 5, "the seam is back");

        // An absent array slot is refused by name, never filled from a sibling.
        assert_eq!(
            catalog
                .resolve(&session, &gamez_key(), 2)
                .expect_err("an absent slot")
                .code(),
            "mesh_not_found"
        );
        assert_eq!(
            catalog
                .resolve(&session, &gamez_key(), 99)
                .expect_err("past the array")
                .code(),
            "mesh_not_found"
        );
    }

    /// A mesh whose stored faces do not survive the validation gate is a failed
    /// **row**, its container's other meshes are untouched, and nothing is
    /// uploaded for it. A mesh whose record declares one more polygon than the
    /// section stores is a different refusal: the reader's own parse error, with
    /// its logical field and its byte offset, reaches the row.
    #[test]
    fn accept_f10_c_03_a_refused_mesh_is_a_row_and_nothing_is_uploaded_for_it() {
        // Mesh 0: a four-corner outline over the quad's four positions whose
        // stored order crosses itself, so validated triangulation refuses it. A
        // three-corner polygon would not do: a triangle needs no triangulation,
        // so it would have been accepted and the row would have been `Blocked`.
        let refused = StoredMesh::new(
            block(0.0, 4),
            block(1000.0, 4),
            vec![StoredPolygon::new(
                &[0, 1, 2, 3],
                0,
                &[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
            )],
        );
        let bytes = gamez_container(&["sky"], 1, &[Ok(refused), Ok(seam_mesh())]);
        let tree = Tree::world(&["sky", "ground"], &["tier"]);
        tree.write("ZBD/c1/gamez.zbd", &bytes);

        let session = world_session(&tree.0, "ZBD/c1");
        let textures = TextureCatalog::open(&session, std::slice::from_ref(&texture_key()));
        let archive = texture_key();
        let dependencies = seam_dependencies(&textures, &archive);
        let catalog = MeshCatalog::open(&session, &[gamez_key()], &dependencies);
        assert_eq!(catalog.failures().count(), 0, "the container itself reads");

        let records = catalog.records();
        assert_eq!(records.len(), 2);
        let refused = &records[0];
        assert_eq!(refused.mesh_index, Some(0));
        assert!(refused.id.is_some(), "the mesh is stored, so it has an id");
        assert_eq!(
            refused.parse_state,
            ParseState::Parsed,
            "the stored mesh was read whole"
        );
        assert!(matches!(refused.normalize_state, ParseState::Failed { .. }));
        assert_eq!(refused.readiness, RenderMeshReadiness::Failed);
        assert!(refused.fingerprint.is_some(), "its stored span is known");
        let faces = refused.faces.expect("face counts are exact either way");
        assert_eq!(faces.faces, 1);
        assert_eq!(faces.rejected, 1, "the face is counted, not dropped");
        let failure = refused.failure.as_ref().expect("the gate's own refusal");
        assert_eq!(failure.stage, MeshFailureStage::Render);
        assert_eq!(failure.code, "incomplete_topology");
        assert_eq!(failure.mesh, Some(0));
        assert_eq!(
            failure.offset, None,
            "no read failed, so no offset is invented"
        );
        assert!(
            refused
                .unsupported_reasons
                .contains(&"render_mesh_refused".to_owned())
        );
        assert!(
            refused
                .unsupported_reasons
                .contains(&"incomplete_topology".to_owned())
        );
        assert!(
            failure.diagnostic.contains("rejected face")
                && failure.diagnostic.contains("unsupported_ngon"),
            "{failure}"
        );

        // Nothing is uploaded for it, and the reason names the gate.
        assert_eq!(
            catalog
                .resolve(&session, &gamez_key(), 0)
                .expect_err("not uploadable")
                .code(),
            "incomplete_topology"
        );
        // Its sibling is complete and unaffected.
        let sibling = catalog.resolve(&session, &gamez_key(), 1).expect("mesh 1");
        assert_eq!(sibling.id().index, 1);
        assert!(catalog.prepare_upload(&session, &sibling).is_ok());

        // -- the reader's own parse error, with its field and offset ---------
        let mut short = seam_mesh();
        // One polygon record more than the section stores: the stored polygon
        // array of the last mesh is truncated. The header's chain still describes
        // the file, so the reader walks into the mesh data and reports the exact
        // byte offset where it ran out, naming the field it was reading.
        short.declared_polygons += 1;
        let bytes = gamez_container(&["sky"], 1, &[Ok(short)]);
        let nodes_offset = u32::from_le_bytes(bytes[36..40].try_into().expect("four bytes"));
        let tree = Tree::world(&["sky", "ground"], &["tier"]);
        tree.write("ZBD/c1/gamez.zbd", &bytes);
        let session = world_session(&tree.0, "ZBD/c1");
        let textures = TextureCatalog::open(&session, std::slice::from_ref(&texture_key()));
        let dependencies = seam_dependencies(&textures, &archive);
        let catalog = MeshCatalog::open(&session, &[gamez_key()], &dependencies);
        let row = &catalog.records()[0];
        assert_eq!(row.readiness, RenderMeshReadiness::Failed);
        assert_eq!(row.id, None, "the container read no mesh at all");
        let failure = row.failure.as_ref().expect("the reader's own context");
        assert_eq!(failure.stage, MeshFailureStage::Meshes);
        assert_eq!(failure.code, "parse");
        assert_eq!(failure.offset, Some(u64::from(nodes_offset)));
        // The reader's own scope and field, not a reworded summary of them: the
        // walk consumed the stored polygons' arrays and ran out reading the one
        // polygon its record declared but the section did not store.
        let member = failure.member.as_deref().expect("the reader named a field");
        assert!(
            member.starts_with("gamez.meshes.polygon."),
            "the reader's own scope: {member}"
        );
        assert!(
            failure.container.contains("gamez.zbd"),
            "the container is named: {}",
            failure.container
        );
        assert!(failure.diagnostic.contains(member), "{failure}");
    }

    /// Stale state: a catalog serves its own session only, refuses a mesh another
    /// catalog resolved, and a payload that has already been prepared still
    /// answers after its catalog and its session are gone.
    #[test]
    fn accept_f10_c_03_a_payload_owns_its_data_and_survives_its_session() {
        let tree = seam_tree();
        let session = world_session(&tree.0, "ZBD/c1");
        let textures = TextureCatalog::open(&session, std::slice::from_ref(&texture_key()));
        let archive = texture_key();
        let dependencies = seam_dependencies(&textures, &archive);
        let first = MeshCatalog::open(&session, &[gamez_key()], &dependencies);
        let resolved = first.resolve(&session, &gamez_key(), 0).expect("mesh 0");
        let upload = first
            .prepare_upload(&session, &resolved)
            .expect("mesh 0 uploads");
        let expected = upload.clone();

        // A second catalog of the **same** session, over the same bytes, does not
        // accept the first one's resolution: the payload is bound to the catalog
        // that produced it, even when both would answer identically.
        let second = MeshCatalog::open(&session, &[gamez_key()], &dependencies);
        assert_eq!(
            second
                .prepare_upload(&session, &resolved)
                .expect_err("not this catalog's")
                .code(),
            "not_from_this_catalog"
        );
        // Its own resolution of the same mesh is accepted, and the payloads are
        // equal because the path is a function of the bytes.
        let own = second.resolve(&session, &gamez_key(), 0).expect("mesh 0");
        let own = second
            .prepare_upload(&session, &own)
            .expect("its own resolution uploads");
        assert_eq!(own, upload, "the payload is a function of the bytes");

        // A catalog of another session refuses a resolution of this one, and
        // refuses to be asked by it.
        drop(first);
        let other = world_session(&tree.0, "ZBD/c1");
        let other_textures = TextureCatalog::open(&other, std::slice::from_ref(&archive));
        let other_dependencies = seam_dependencies(&other_textures, &archive);
        let other_catalog = MeshCatalog::open(&other, &[gamez_key()], &other_dependencies);
        // A resolution made by another session's catalog is refused here as
        // `not_from_this_catalog`, not as `foreign_session`: the session asking
        // is this catalog's own, and the actionable fault is that the mesh did
        // not come from this catalog. Either way nothing is uploaded.
        assert_eq!(
            other_catalog
                .prepare_upload(&other, &resolved)
                .expect_err("another session's resolution")
                .code(),
            "not_from_this_catalog"
        );
        assert_eq!(
            second
                .require_session(&other)
                .expect_err("another session")
                .code(),
            "foreign_session"
        );

        // The payload owns everything: dropping the catalog and closing the
        // sessions leaves it intact, seam included.
        drop(second);
        let _first_teardown = session.close();
        let _other_teardown = other.close();
        assert_eq!(upload, expected);
        assert_eq!(upload.render().vertices().len(), 5);
        assert_eq!(
            uploaded_uvs(&upload, 0),
            [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]
        );
        assert_eq!(
            uploaded_uvs(&upload, 1),
            [[0.5, 0.5], [1.0, 0.0], [1.0, 1.0]]
        );
        assert_eq!(upload.generation(), expected.generation());
        assert!(upload.material(0).is_some());
    }

    /// The family check is real. A container whose key routes to another ZBD
    /// family is never handed to a GameZ reader, in either of the two ways that
    /// can happen: the dispatch routes it elsewhere and this stage refuses it as
    /// `wrong_family`, or the dispatch itself refuses the two keys' disagreement
    /// before any reader runs.
    #[test]
    fn accept_f10_c_03_a_container_of_another_family_is_never_read_as_gamez() {
        let tree = seam_tree();
        // The texture family documents no header signature, so a valid texture
        // package under a texture key routes there and this stage refuses it.
        tree.write("ZBD/c1/rtexture2.zbd", &package(&["Sky1.tif", "tier"]));
        // The same role with GameZ bytes in it: the dispatch sees another
        // family's documented signature and refuses the two keys' disagreement
        // before a reader is chosen at all.
        tree.write("ZBD/c1/rtexture3.zbd", &seam_container());
        let session = world_session(&tree.0, "ZBD/c1");
        let textures = TextureCatalog::open(&session, std::slice::from_ref(&texture_key()));
        let archive = texture_key();
        let dependencies = seam_dependencies(&textures, &archive);
        let routed = world_key("rtexture2.zbd");
        let conflicting = world_key("rtexture3.zbd");
        let catalog = MeshCatalog::open(
            &session,
            &[routed.clone(), conflicting.clone()],
            &dependencies,
        );

        let failures: Vec<(String, &str)> = catalog
            .failures()
            .map(|(key, error)| (key.to_string(), error.code()))
            .collect();
        assert_eq!(failures.len(), 2, "{failures:?}");
        assert_eq!(failures[0].1, "wrong_family");
        assert_eq!(failures[1].1, "dispatch");

        let (_, refused) = catalog.failures().next().expect("refused");
        let MeshContainerErrorKind::WrongFamily { family, .. } = refused.kind() else {
            panic!("expected a family refusal, got {}", refused.kind());
        };
        assert_eq!(*family, ZbdFamily::Texture);
        assert_eq!(refused.failure().stage, MeshFailureStage::Family);
        assert!(refused.origin().is_some(), "the bytes did exist");
        assert!(
            refused.failure().container.contains("rtexture2.zbd"),
            "the container is named: {}",
            refused.failure().container
        );

        let records = catalog.records();
        assert_eq!(records.len(), 2, "both refusals are rows");
        for (row, key) in records.iter().zip([&routed, &conflicting]) {
            assert_eq!(row.readiness, RenderMeshReadiness::Failed);
            assert_eq!(row.id, None);
            assert_eq!(row.mesh_index, None);
            assert_eq!(row.container_key, *key);
            assert_eq!(row.fingerprint, None, "no stored span was walked");
            assert!(row.failure.is_some());
        }
        assert_eq!(
            records[0].failure.as_ref().expect("context").code,
            "wrong_family"
        );
        assert_eq!(
            records[1].failure.as_ref().expect("context").stage,
            MeshFailureStage::Container,
            "the dispatch refused it, so no reader ever saw the bytes"
        );
        assert_eq!(catalog.containers().count(), 0, "nothing was read as GameZ");
    }

    /// A row's `unsupported_reasons` is a **set** of codes, not a tally: two of
    /// this mesh's three materials are refused for the same reason, and the row
    /// names that reason once. The two copies are not adjacent — a different
    /// reason sits between them — so this fails for a de-duplication that only
    /// collapses neighbouring entries.
    #[test]
    fn accept_f10_c_03_a_row_names_each_blocking_reason_once() {
        let tree = Tree::world(&["sky", "ground"], &["tier"]);
        tree.write("ZBD/c1/gamez.zbd", &repeated_reason_container());
        let session = world_session(&tree.0, "ZBD/c1");
        let textures = TextureCatalog::open(&session, std::slice::from_ref(&texture_key()));
        let archive = texture_key();
        let dependencies = seam_dependencies(&textures, &archive);
        let catalog = MeshCatalog::open(&session, &[gamez_key()], &dependencies);
        assert_eq!(catalog.failures().count(), 0, "the fixture container opens");

        let row = &catalog.records()[0];
        assert_eq!(row.mesh_index, Some(0));
        assert_eq!(row.readiness, RenderMeshReadiness::Blocked);
        // Two materials name a texture the archive does not store and one
        // stores a texture index past the container's own table, so the row
        // carries both codes.
        assert_eq!(
            row.unsupported_reasons
                .iter()
                .filter(|reason| reason.as_str() == "texture_not_found")
                .count(),
            1,
            "the same reason reached through two materials is one reason: {:?}",
            row.unsupported_reasons
        );
        assert!(
            row.unsupported_reasons
                .iter()
                .any(|reason| reason.starts_with("texture_index_out_of_range")),
            "{:?}",
            row.unsupported_reasons
        );
        // Nothing is listed twice at all, whatever the codes are.
        let mut sorted = row.unsupported_reasons.clone();
        sorted.sort();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(sorted.len(), before, "{:?}", row.unsupported_reasons);
        // The two materials really are two rows of the audit, so the two
        // refusals exist and the row is not hiding one of them.
        let container = catalog.containers().next().expect("one container");
        assert_eq!(container.audit().rows.len(), 3);
        assert_eq!(
            container
                .audit()
                .rows
                .iter()
                .filter(|material| !material.state.is_complete())
                .count(),
            3
        );
    }

    /// The retail half: the world's own `gamez.zbd`, read by the production VFS,
    /// dispatch and both production readers, reaches the upload payload. The
    /// discriminating facts are checked on real data: every stored face decodes,
    /// the seam survives wherever the original authored one, and no stored mesh
    /// disappears between the container and the rows.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f10_c_03_retail_world_meshes_reach_the_upload_payload() {
        let game_dir = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
        let found = install::discover(&game_dir).expect("the installation is discovered");
        let group = found
            .diagnosis
            .world_groups
            .iter()
            .find(|group| group.as_str().eq_ignore_ascii_case("ZBD/c1"))
            .expect("world C1 is discovered")
            .clone();
        let context = ResolveContext::new(install::fingerprint(&found.manifest))
            .with_world_group(WorldGroup::from_relative(group));
        let mut builder = SessionBuilder::new(context);
        builder
            .mount_installation(&game_dir, &found.diagnosis)
            .expect("the installation mounts");
        let session = builder.open();

        let archive = texture_key();
        let textures = TextureCatalog::open(&session, std::slice::from_ref(&archive));
        assert_eq!(
            textures.failures().count(),
            0,
            "the world's texture archive opens"
        );
        let dependencies = seam_dependencies(&textures, &archive);
        let key = gamez_key();
        let catalog = MeshCatalog::open(&session, std::slice::from_ref(&key), &dependencies);
        assert_eq!(
            catalog.failures().count(),
            0,
            "the world's own GameZ container opens and both readers accept it"
        );

        let container = catalog.containers().next().expect("one container");
        // The two readers proved their own section boundary from the same bytes,
        // and this stage additionally checked that they read the same header.
        assert_eq!(
            container.meshes().data_end,
            u64::from(container.meshes().header.nodes_offset)
        );
        assert_eq!(
            container.materials().data_end,
            u64::from(container.materials().header.meshes_offset)
        );

        // One row per present stored mesh, and a row is never lost.
        let records = catalog.records();
        assert_eq!(records.len(), container.meshes().present_count());
        assert!(!records.is_empty());
        for row in &records {
            assert_eq!(row.kind, RENDER_MESH_KIND);
            assert!(row.id.is_some());
            assert!(row.origin.is_some());
            assert_eq!(row.dependencies, vec![key.clone(), archive.clone()]);
            assert!(row.fingerprint.is_some(), "every stored mesh has bytes");
            assert_eq!(row.parse_state, ParseState::Parsed);
            assert_eq!(row.runtime_consumers, vec![MESH_UPLOAD_CONSUMER]);
            assert!(row.faces.is_some());
        }

        // Every row is uploaded, the payload agrees with the row it came from,
        // and the split is exactly the authored one: no two render vertices of an
        // upload share a key, and every extra vertex at one position index
        // differs in one of the four attributes the key holds — including the
        // material, which is per polygon and splits a shared position on its
        // own. That is what keeps an authored seam visible.
        let mut uploaded = 0usize;
        let mut split_positions = 0usize;
        let mut uv_seams = 0usize;
        for row in &records {
            let index = row.mesh_index.expect("every row is about a mesh");
            let resolved = catalog
                .resolve(&session, &key, index)
                .expect("a decoded row resolves");
            let upload = catalog
                .prepare_upload(&session, &resolved)
                .expect("a decoded mesh uploads");
            let counts = row.faces.expect("face counts");
            assert_eq!(upload.render().source_faces(), counts.faces);
            // The render mesh draws every stored material group of a polygon, so
            // its triangle count is the stored topology count **plus** the draws
            // the later groups add. On this container that is 1 042 extra over
            // 390 multi-group polygons; on `planes.zbd`, which stores none, the
            // two are equal.
            assert_eq!(
                upload.render().source_triangles(),
                counts.triangles + upload.render().extra_group_triangles(),
                "the draws beyond the first group are exactly the groups' own"
            );
            assert_eq!(
                upload.render().degenerate_triangles(),
                counts.degenerate + upload.render().extra_group_degenerate_triangles(),
                "a degenerate stored step is degenerate in every group of its polygon"
            );
            assert_eq!(upload.id(), row.id.as_ref().expect("an id"));
            assert_eq!(upload.render().vertices().len() >= 3, counts.triangles > 0);

            let render = upload.render();
            let mut at_index: BTreeMap<u32, Vec<&RenderVertex>> = BTreeMap::new();
            for vertex in render.vertices() {
                at_index
                    .entry(vertex.position_index)
                    .or_default()
                    .push(vertex);
            }
            for vertices in at_index.values() {
                if vertices.len() < 2 {
                    continue;
                }
                split_positions += 1;
                for (a, b) in vertices.iter().zip(vertices.iter().skip(1)) {
                    assert!(
                        a.normal_index != b.normal_index
                            || a.uv != b.uv
                            || a.color != b.color
                            || a.material != b.material,
                        "a split at position {} must differ in a keyed attribute",
                        a.position_index
                    );
                }
                // A split is a **UV seam** when two vertices at one position
                // index carry two different authored texture coordinates. That
                // is the case AC03 names, and it is the one a position-keyed
                // splitter would lose.
                let mut uvs: Vec<[u32; 2]> = vertices
                    .iter()
                    .filter_map(|vertex| vertex.uv)
                    .map(|uv| [uv[0].to_bits(), uv[1].to_bits()])
                    .collect();
                let before = uvs.len();
                uvs.sort_unstable();
                uvs.dedup();
                if uvs.len() < before {
                    uv_seams += 1;
                }
            }
            uploaded += 1;
        }
        assert_eq!(uploaded, records.len(), "every row uploaded");
        assert!(
            split_positions > 0,
            "the private corpus authors per-corner attribute splits"
        );
        assert!(
            uv_seams > 0,
            "at least one split is a UV seam, which is the case AC03 names"
        );
    }

    // ================================================== F10-C (the parent) ===

    /// A blocking reason is a **code**, and the evidence that produced it is a
    /// separate line. This is the integration contract the three slices have to
    /// agree on: F10-C.02 builds the material reasons, F10-C.03 copies them onto
    /// the render-mesh row, and F10-C is where the two meet, so it is F10-C that
    /// pins what a code is.
    ///
    /// It matters because a code is what a consumer groups rows by. The measured
    /// corpus stores `bldhwk_cowling..tif` at 36 table positions, so a reason
    /// that carried the name and the positions was 242 bytes and **differed per
    /// row**: two rows refusing the same cause would not have compared equal,
    /// and a container that stored the name more often would have produced a
    /// longer string for the same reason. Both are the "reason code" contract
    /// broken, on data measured in the original installation.
    #[test]
    fn accept_f10_c_blocking_reasons_are_stable_codes_and_keep_their_evidence() {
        let tree = Tree::world(&["bldhwk_cowling"], &["tier"]);
        let (session, catalog, key) = catalog(&tree);
        // The container stores one texture name twice, exactly as the measured
        // `planes.zbd` stores `bldhwk_cowling..tif` 36 times.
        let meshes = container(vec![container_mesh(0, &[0], &[])]);
        let materials = tables(
            &["bldhwk_cowling..tif", "bldhwk_cowling..tif"],
            vec![material(0, 1, true)],
        );
        let audit =
            MeshDependencyAudit::build(&meshes, &materials, &context(&session, &catalog, &key));
        let row = &audit.rows[0];

        // The code is the code, with nothing appended to it.
        assert!(
            row.unsupported_reasons
                .iter()
                .any(|reason| reason == CONTAINER_DUPLICATE_NAME),
            "the container-level duplicate is a code: {:?}",
            row.unsupported_reasons
        );
        for reason in &row.unsupported_reasons {
            assert!(
                !reason.contains("bldhwk_cowling"),
                "a code carries no stored name: {reason:?}"
            );
            assert!(
                !reason.contains("table indices"),
                "a code carries no table position list: {reason:?}"
            );
        }
        // The evidence is not dropped: the name and the positions it is stored
        // at are on the row, on their own line.
        assert!(
            row.reason_details
                .iter()
                .any(|detail| detail.contains("bldhwk_cowling..tif") && detail.contains("[0, 1]")),
            "the measured bytes are named on their own line: {:?}",
            row.reason_details
        );
    }

    /// The same code is one entry however many materials reach it, and the count
    /// of the affected polygons is the count on the row rather than a suffix on
    /// the code. This is the seam between the audit's reasons and the render
    /// mesh's own: both have to be sets, or a consumer cannot group rows.
    #[test]
    fn accept_f10_c_a_render_mesh_row_names_each_reason_once_as_a_bare_code() {
        let tree = Tree::world(&["sky", "ground"], &["tier"]);
        tree.write("ZBD/c1/gamez.zbd", &repeated_reason_container());
        let session = world_session(&tree.0, "ZBD/c1");
        let textures = TextureCatalog::open(&session, std::slice::from_ref(&texture_key()));
        let archive = texture_key();
        let dependencies = seam_dependencies(&textures, &archive);
        let catalog = MeshCatalog::open(&session, &[gamez_key()], &dependencies);
        assert_eq!(catalog.failures().count(), 0, "the fixture container opens");

        let records = catalog.records();
        let row = &records[0];
        // Every entry is a code: a fixed, closed vocabulary, so a consumer can
        // match on it. Nothing here carries a material index, a texture name or
        // a polygon count.
        let vocabulary = [
            CONTAINER_DUPLICATE_NAME,
            "texture_not_found",
            "texture_index_out_of_range",
            "material_index_out_of_range",
            "unknown_field",
            "duplicate_texture_name",
            "archive_unavailable",
            MeshPresentationUnknown::FrontFaceWinding.code(),
            MeshPresentationUnknown::UvOrigin.code(),
            MeshPresentationUnknown::VertexColor.code(),
            MeshPresentationUnknown::MultiMaterialGroup.code(),
        ];
        for reason in &row.unsupported_reasons {
            assert!(
                vocabulary.contains(&reason.as_str()),
                "{reason:?} is not a code from the closed vocabulary"
            );
        }
        // Two of this mesh's three materials are refused for the same cause, so
        // the row names that cause once.
        assert_eq!(
            row.unsupported_reasons
                .iter()
                .filter(|reason| reason.as_str() == "texture_not_found")
                .count(),
            1,
            "{:?}",
            row.unsupported_reasons
        );
        // And the render mesh's own reason about multi-group polygons is a bare
        // code as well, with the count kept as a number on the row. This is
        // checked on a container that really stores two groups per polygon, so
        // the reason is reached through the production reader rather than
        // constructed. What the code now says is that the *presentation* of
        // those groups is unmeasured — nothing is lost any more, F10-E draws
        // every group — so the loss code of F10-C is gone and this one names the
        // question that is left.
        let multi_tree = Tree::world(&["sky"], &["tier"]);
        multi_tree.write("ZBD/c1/gamez.zbd", &multi_group_container());
        let multi_session = world_session(&multi_tree.0, "ZBD/c1");
        let multi_textures =
            TextureCatalog::open(&multi_session, std::slice::from_ref(&texture_key()));
        let multi_archive = texture_key();
        let multi_catalog = MeshCatalog::open(
            &multi_session,
            &[gamez_key()],
            &seam_dependencies(&multi_textures, &multi_archive),
        );
        assert_eq!(multi_catalog.failures().count(), 0, "the fixture opens");
        let multi_records = multi_catalog.records();
        let multi = multi_records
            .iter()
            .find(|r| r.mesh_index == Some(0))
            .expect("the multi-group mesh");
        let counts = multi.faces.expect("face counts");
        assert_eq!(
            counts.multi_material_group_polygons, 2,
            "both stored polygons keep two material groups"
        );
        let presentation = MeshPresentationUnknown::MultiMaterialGroup.code();
        assert!(
            multi.unsupported_reasons.iter().any(|r| r == presentation),
            "{:?}",
            multi.unsupported_reasons
        );
        for reason in &multi.unsupported_reasons {
            assert!(
                !reason.starts_with(&format!("{presentation}:")),
                "the count is not part of the code: {reason:?}"
            );
        }
    }

    /// The airframe producer, which the sheet names beside the world producer
    /// ("world geometry and PLANES.ZBD meshes"), reaches the same upload
    /// boundary through the same production path. It is a **different mount** —
    /// `install`, not a world group — so the integration has to work across
    /// namespaces, not only inside the one the world fixture happens to use.
    ///
    /// Both halves of this stage's deliverable are checked here, because this is
    /// the only test that reaches them on retail data: the mesh IR arrives with
    /// its face accounting intact and AC03's seam still split, and the material
    /// audit arrives with **codes** as reasons — which is what the airframe
    /// corpus, with its 36-times-duplicated texture name, is here to prove.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f10_c_retail_airframe_meshes_reach_the_upload_payload() {
        let game_dir = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
        let found = install::discover(&game_dir).expect("the installation is discovered");
        let group = found
            .diagnosis
            .world_groups
            .iter()
            .find(|group| group.as_str().eq_ignore_ascii_case("ZBD/c1"))
            .expect("world C1 is discovered")
            .clone();
        let context = ResolveContext::new(install::fingerprint(&found.manifest))
            .with_world_group(WorldGroup::from_relative(group));
        let mut builder = SessionBuilder::new(context);
        builder
            .mount_installation(&game_dir, &found.diagnosis)
            .expect("the installation mounts");
        let session = builder.open();

        // The airframe container is mounted at the install root, not inside a
        // world group, so it resolves through a different namespace and mount.
        let key = AssetKey::from_spelling(INSTALL_NAMESPACE, "ZBD/planes.zbd", "default")
            .expect("a valid key");
        let archive = texture_key();
        let textures = TextureCatalog::open(&session, std::slice::from_ref(&archive));
        assert_eq!(textures.failures().count(), 0, "the archive opens");
        let dependencies = seam_dependencies(&textures, &archive);
        let catalog = MeshCatalog::open(&session, std::slice::from_ref(&key), &dependencies);
        assert_eq!(
            catalog.failures().count(),
            0,
            "the airframe GameZ container opens and both readers accept it"
        );

        let container = catalog.containers().next().expect("one container");
        // It is the airframe archive, not a world one, that was read.
        assert!(
            container.label().starts_with("install"),
            "{}",
            container.label()
        );
        let records = catalog.records();
        assert_eq!(records.len(), container.meshes().present_count());
        assert!(!records.is_empty(), "the airframes store meshes");

        // Every stored mesh of the airframes reaches the upload payload, and
        // every row's exact face accounting survives to it.
        let mut uploaded = 0usize;
        let mut faces = 0usize;
        let mut uv_seams = 0usize;
        let mut coded_reasons = 0usize;
        for row in &records {
            let index = row.mesh_index.expect("every row is about a mesh");
            let resolved = catalog
                .resolve(&session, &key, index)
                .expect("every stored airframe mesh resolves");
            let upload = catalog
                .prepare_upload(&session, &resolved)
                .expect("every stored airframe mesh uploads");
            let counts = row.faces.expect("face counts");
            assert_eq!(upload.render().source_faces(), counts.faces);
            // The render mesh draws every stored material group of a polygon.
            // `ZBD/planes.zbd` stores no multi-group polygon at all, so on the
            // airframe corpus the draws beyond the first are none and the two
            // counts are equal; the assertion states the general relation so a
            // future airframe that stores one would still have to hold.
            assert_eq!(
                upload.render().source_triangles(),
                counts.triangles + upload.render().extra_group_triangles(),
                "the draws beyond the first group are exactly the groups' own"
            );
            assert_eq!(
                upload.render().degenerate_triangles(),
                counts.degenerate + upload.render().extra_group_degenerate_triangles()
            );
            assert_eq!(
                upload.faces().multi_material_group_polygons,
                0,
                "the measured airframe corpus stores no multi-group polygon"
            );
            // A mesh that became a render mesh had no polygon rejected, so the
            // faces the render mesh saw are all of the stored ones.
            assert_eq!(counts.rejected, 0, "a mesh that uploaded rejected none");

            // The material half of the audit reaches the same payload, and its
            // reasons are codes here too: the airframe corpus stores one texture
            // name at 36 table positions, which is exactly the reason that used
            // to carry the name and every position. A code is lowercase
            // snake_case, so it holds none of a space, a `:` or a `[`.
            for reason in row.unsupported_reasons.iter().chain(
                upload
                    .materials()
                    .iter()
                    .flat_map(|m| &m.unsupported_reasons),
            ) {
                assert!(
                    !reason.contains([' ', ':', '[']),
                    "a blocking reason carries no stored data: {reason:?}"
                );
                coded_reasons += 1;
            }

            faces += counts.faces;

            // AC03 on the airframes too: a shared position with different
            // per-corner UVs is still two vertices here. A vertex with no stored
            // UV is left out rather than counted as `(0.0, 0.0)`, so an authored
            // zero is never mistaken for an absent one.
            let mut at: BTreeMap<u32, Vec<[u32; 2]>> = BTreeMap::new();
            for vertex in upload.render().vertices() {
                if let Some(uv) = vertex.uv {
                    at.entry(vertex.position_index)
                        .or_default()
                        .push([uv[0].to_bits(), uv[1].to_bits()]);
                }
            }
            for uvs in at.values() {
                if uvs.len() < 2 {
                    continue;
                }
                let before = uvs.len();
                let mut sorted = uvs.clone();
                sorted.sort_unstable();
                sorted.dedup();
                if sorted.len() < before {
                    uv_seams += 1;
                }
            }
            uploaded += 1;
        }
        assert_eq!(uploaded, records.len(), "every airframe row uploaded");
        assert!(faces > 0, "the airframes store faces");
        assert!(
            coded_reasons > 0,
            "the airframe corpus blocks its meshes, so the reasons are there"
        );
        assert!(
            uv_seams > 0,
            "the airframe corpus authors UV seams, which is the case AC03 names"
        );
    }

    // ================================================ F10-E: every stored group ===

    /// **The whole point of F10-E, end to end.** A GameZ polygon stores one
    /// material group per `mat_count`, each with its own raw material index and
    /// its own UV set. F10-A's IR mirrors only the first onto its single-valued
    /// fields, and a render mesh built from that alone leaves every later group
    /// with nowhere to go. This walks the production path — a synthetic
    /// container through the real reader, the real [`MeshContainer`], the real
    /// catalog and the real upload boundary — and states that **every** stored
    /// group of **every** stored polygon is there, carrying its own values.
    ///
    /// The discriminating step is per group, not per mesh: a splitter that drew
    /// only the first group would produce a mesh with the right triangle count
    /// for the single-group polygon and half the right one everywhere else, and
    /// a splitter that drew a group's geometry with the *first* group's material
    /// would still have the right triangle count and the wrong payload.
    #[test]
    fn accept_f10_e_every_stored_material_group_reaches_the_upload_payload() {
        let tree = group_tree();
        let session = world_session(&tree.0, "ZBD/c1");
        let archive = texture_key();
        let textures = TextureCatalog::open(&session, std::slice::from_ref(&archive));
        assert_eq!(textures.failures().count(), 0, "the archive opens");
        let catalog = MeshCatalog::open(
            &session,
            &[gamez_key()],
            &seam_dependencies(&textures, &archive),
        );
        assert_eq!(catalog.failures().count(), 0, "the fixture container opens");

        let container = catalog.containers().next().expect("one container");
        let stored = container
            .meshes()
            .get(0)
            .expect("mesh 0 is present")
            .groups_are_complete();
        assert!(stored, "every stored polygon has its own group list");

        let resolved = catalog
            .resolve(&session, &gamez_key(), 0)
            .expect("the group mesh resolves");
        let upload = catalog
            .prepare_upload(&session, &resolved)
            .expect("the group mesh uploads");
        let render = upload.render();

        // The face accounting still describes the **stored** mesh, and the count
        // of polygons that kept more than one group is the evidence for the one
        // question such a polygon raises.
        let faces = upload.faces();
        assert_eq!(faces.faces, 3, "three stored polygons");
        assert_eq!(faces.triangles, 7, "5 strip steps, 2 triangle outlines");
        assert_eq!(faces.rejected, 0, "no polygon was rejected");
        assert_eq!(faces.degenerate, 1, "one degenerate strip step");
        assert_eq!(
            faces.multi_material_group_polygons, 2,
            "the strip and the three-group triangle keep more than one group"
        );

        // Per polygon and per group: the triangles are the product of the
        // topology triangles and the stored groups, and each carries **that
        // group's** raw material index. A build that read the groups but paired
        // them with the IR's first-group material fails here.
        let stored_mesh = container.meshes().get(0).expect("mesh 0");
        let topology = container.topology(0).expect("the stored topology");
        let mut expected_triangles = 0usize;
        let mut expected_extra = 0usize;
        let mut expected_degenerate = 0usize;
        for (polygon, face) in stored_mesh.mesh.polygons.iter().enumerate() {
            let groups = stored_mesh
                .groups(polygon)
                .expect("a stored polygon has its groups");
            let steps = if face.kind == PrimitiveKind::TriangleStrip {
                face.corners.len().saturating_sub(2)
            } else {
                1
            };
            let drawn: Vec<usize> = (0..groups.len())
                .map(|group| group_triangles(render, polygon, group).len())
                .collect();
            assert_eq!(
                drawn,
                vec![steps; groups.len()],
                "polygon {polygon}: every stored group is drawn once per topology triangle"
            );
            // Degeneracy is the stored position indices' own, so the same steps
            // are marked in every group of a polygon and no others.
            let degenerate: Vec<usize> = topology
                .triangles
                .iter()
                .filter(|t| t.polygon == polygon && t.is_degenerate())
                .map(|t| t.step)
                .collect();
            for (group, attributes) in groups.iter().enumerate() {
                let triangles = group_triangles(render, polygon, group);
                for triangle in &triangles {
                    assert_eq!(
                        triangle.material, attributes.material,
                        "polygon {polygon} group {group} is drawn with its own material"
                    );
                }
                // And the vertices of that group sample that group's own UV set,
                // corner for corner, through the production source map.
                assert_group_samples_its_own_uvs(render, stored_mesh, topology, polygon, group);
                let marked: Vec<usize> = triangles
                    .iter()
                    .filter(|t| t.degenerate)
                    .map(|t| t.source.step)
                    .collect();
                assert_eq!(
                    marked, degenerate,
                    "polygon {polygon} group {group}: degeneracy is the stored indices' own"
                );
                expected_degenerate += marked.len();
                expected_triangles += triangles.len();
                if group > 0 {
                    expected_extra += triangles.len();
                }
            }
        }
        assert_eq!(render.source_triangles(), expected_triangles);
        assert_eq!(render.extra_group_triangles(), expected_extra);
        assert_eq!(render.degenerate_triangles(), expected_degenerate);
        assert_eq!(render.source_triangles(), 14, "10 + 3 + 1");
        assert_eq!(render.extra_group_triangles(), 7, "5 + 2 + 0");
        assert_eq!(
            render.degenerate_triangles(),
            2,
            "the one strip step, twice"
        );
        assert_eq!(
            render.extra_group_degenerate_triangles(),
            1,
            "the one degenerate strip step, once more in the second group"
        );
        // The counts a catalog row carries are the stored topology's; the render
        // mesh's are those plus the later groups' own draws. Every degenerate
        // step of a polygon is degenerate in every group, so the two relations
        // hold together.
        assert_eq!(faces.triangles + render.extra_group_triangles(), 14);
        assert_eq!(
            faces.degenerate + render.extra_group_degenerate_triangles(),
            2
        );

        // Every raw material index the stored groups name is a material of the
        // payload's own audit rows, so nothing was dropped on the way there
        // either.
        let audited: Vec<u32> = upload.materials().iter().map(|row| row.material).collect();
        for group_material in 0..4u32 {
            assert!(
                audited.contains(&group_material),
                "material {group_material} is audited on the payload: {audited:?}"
            );
        }
    }

    /// **AC03, re-asserted for the second group of a multi-group polygon.**
    ///
    /// The strip of [`group_mesh`] repeats position index 2 at two of its seven
    /// corners. Its first group stores `[0.0, 1.0]` for both, so the first group
    /// has no seam there; its second group stores `[0.0, 1.0]` and `[0.2, 0.9]`,
    /// so the second group authors one. The seam therefore exists **only** in a
    /// group the first-group mirroring cannot reach, and it has to be visible in
    /// the upload payload.
    ///
    /// A splitter keyed on the position index alone, or one that welded the two
    /// groups' corners together, would find one coordinate at position 2 in the
    /// second group and fail.
    #[test]
    fn accept_f10_e_second_group_uv_seam_stays_a_visible_seam() {
        let tree = group_tree();
        let session = world_session(&tree.0, "ZBD/c1");
        let archive = texture_key();
        let textures = TextureCatalog::open(&session, std::slice::from_ref(&archive));
        let catalog = MeshCatalog::open(
            &session,
            &[gamez_key()],
            &seam_dependencies(&textures, &archive),
        );
        let container = catalog.containers().next().expect("one container");
        let topology = container.topology(0).expect("the stored topology");
        let resolved = catalog
            .resolve(&session, &gamez_key(), 0)
            .expect("the group mesh resolves");
        let upload = catalog
            .prepare_upload(&session, &resolved)
            .expect("the group mesh uploads");
        let render = upload.render();

        // The first group really has no seam at position 2, so the second
        // group's seam cannot be an accident of the fixture.
        assert_eq!(
            uvs_at(render, topology, 0, 0, 2),
            vec![[0.0f32.to_bits(), 1.0f32.to_bits()]],
            "the first group stores one coordinate for both corners at position 2"
        );
        let mut expected = vec![
            [0.0f32.to_bits(), 1.0f32.to_bits()],
            [0.2f32.to_bits(), 0.9f32.to_bits()],
        ];
        expected.sort_unstable();
        assert_eq!(
            uvs_at(render, topology, 0, 1, 2),
            expected,
            "the second group authors two coordinates at one position, and the seam survives"
        );

        // The other two repeated positions are authored identically in both
        // groups, so they stay a single coordinate each: the seam above is the
        // authored one, not a general un-welding of a strip.
        for position in [1u32, 3] {
            let only = [1.0f32.to_bits(), 0.0f32.to_bits()];
            let same = [1.0f32.to_bits(), 1.0f32.to_bits()];
            let want = if position == 1 { only } else { same };
            assert_eq!(
                uvs_at(render, topology, 0, 0, position),
                vec![want],
                "position {position} in the first group"
            );
            assert_eq!(
                uvs_at(render, topology, 0, 1, position),
                vec![want],
                "position {position} in the second group"
            );
        }

        // The two groups never share a vertex. Their stored UVs are equal at two
        // of the three repeated positions, so a splitter that keyed on position
        // and UV alone would weld them there; the group's own raw material index
        // is in the key, so the two groups stay two sets of seven. This is the
        // split the task asks for where a group's UV *differs*, and the one that
        // keeps two authored materials apart where it does not.
        let first = group_vertices(render, 0, 0);
        let second = group_vertices(render, 0, 1);
        assert_eq!(first.len(), 7, "seven distinct corners in the first group");
        assert_eq!(
            second.len(),
            7,
            "seven distinct corners in the second group"
        );
        assert!(
            first.iter().all(|vertex| vertex.material == 0),
            "the first group's corners are all material 0"
        );
        assert!(
            second.iter().all(|vertex| vertex.material == 1),
            "the second group's corners are all material 1: {:?}",
            second.iter().map(|v| v.material).collect::<Vec<_>>()
        );

        // And a group whose UV set is *identical* to another's still has to be a
        // separate draw, because its material index differs: polygon 1 stores
        // groups 0 and 1 with the same three coordinates under materials 0 and 1.
        let shared_uvs = |group: usize| {
            let mut bits: Vec<[u32; 2]> = group_vertices(render, 1, group)
                .iter()
                .map(|vertex| {
                    let uv = vertex.uv.expect("a stored coordinate");
                    [uv[0].to_bits(), uv[1].to_bits()]
                })
                .collect();
            bits.sort_unstable();
            bits.dedup();
            bits
        };
        assert_eq!(
            shared_uvs(0),
            shared_uvs(1),
            "the two groups really do store the same coordinates"
        );
        let materials: Vec<u32> = group_vertices(render, 1, 1)
            .iter()
            .map(|vertex| vertex.material)
            .collect();
        assert_eq!(materials, [1, 1, 1], "the second group is material 1");
        assert_ne!(
            group_vertices(render, 1, 0)[0].material,
            group_vertices(render, 1, 1)[0].material,
            "identical coordinates under different materials are two vertices"
        );
        // Group 2 differs in its coordinates, so it is a third draw.
        assert_ne!(shared_uvs(2), shared_uvs(0));
        assert_eq!(group_triangles(render, 1, 2).len(), 1);
    }

    /// **The reason contract.** F10-C put `multi_material_group_polygons` on
    /// every row whose mesh stored a multi-group polygon, because that code named
    /// a **loss**: the extra groups had no UV set in the render mesh. Nothing is
    /// lost now, so that code is emitted nowhere; what is left to say is that
    /// the original renderer's *presentation* of those groups is unmeasured, and
    /// that is [`MeshPresentationUnknown::MultiMaterialGroup`].
    ///
    /// The row for a mesh that stores one group per polygon must carry neither,
    /// or the corpus claim would be false: `ZBD/planes.zbd` stores no multi-group
    /// polygon at all.
    #[test]
    fn accept_f10_e_a_multi_group_row_names_the_open_question_and_a_single_group_row_does_not() {
        let tree = group_tree();
        let session = world_session(&tree.0, "ZBD/c1");
        let archive = texture_key();
        let textures = TextureCatalog::open(&session, std::slice::from_ref(&archive));
        let catalog = MeshCatalog::open(
            &session,
            &[gamez_key()],
            &seam_dependencies(&textures, &archive),
        );
        let records = catalog.records();
        assert_eq!(records.len(), 2, "one row per present stored mesh");

        let presentation = MeshPresentationUnknown::MultiMaterialGroup.code();
        for row in &records {
            let index = row.mesh_index.expect("a mesh row");
            let faces = row.faces.expect("face counts");
            let resolved = catalog
                .resolve(&session, &gamez_key(), index)
                .expect("both stored meshes resolve");
            let upload = catalog
                .prepare_upload(&session, &resolved)
                .expect("both stored meshes upload");
            let has_question = faces.multi_material_group_polygons > 0;

            assert_eq!(
                row.unsupported_reasons.contains(&presentation.to_owned()),
                has_question,
                "mesh {index}: the presentation question is on the row only when the mesh stored \
                 a multi-group polygon; got {:?}",
                row.unsupported_reasons
            );
            assert_eq!(
                upload
                    .unknowns()
                    .contains(&MeshPresentationUnknown::MultiMaterialGroup),
                has_question,
                "mesh {index}: the payload carries the same question"
            );
            // The loss code of F10-C is gone everywhere. It named a state that
            // no longer exists, and a consumer matching on it would be matching
            // on a defect.
            for reason in row
                .unsupported_reasons
                .iter()
                .map(String::as_str)
                .chain(upload.unknowns().iter().map(|unknown| unknown.code()))
            {
                assert_ne!(
                    reason, "multi_material_group_polygons",
                    "mesh {index}: the retired loss code is emitted nowhere"
                );
            }
            // Every reason is still a bare code, and a multi-group row still
            // carries the count as a number rather than inside the code.
            for reason in &row.unsupported_reasons {
                assert!(
                    !reason.contains([' ', ':', '[']),
                    "mesh {index}: a reason carries no stored data: {reason:?}"
                );
            }
            assert_eq!(row.readiness, RenderMeshReadiness::Blocked);
            assert!(!upload.is_release_ready());
        }

        // The count is on the row and the extra draws are in the payload: the
        // two pieces of evidence the code no longer has to carry.
        let group_row = records
            .iter()
            .find(|row| row.mesh_index == Some(0))
            .expect("the group mesh row");
        assert_eq!(
            group_row
                .faces
                .expect("counts")
                .multi_material_group_polygons,
            2
        );
        let resolved = catalog
            .resolve(&session, &gamez_key(), 0)
            .expect("resolves");
        assert_eq!(
            catalog
                .prepare_upload(&session, &resolved)
                .expect("uploads")
                .render()
                .extra_group_triangles(),
            7
        );
        // The three pipeline-wide presentation questions are on every payload,
        // in their fixed order, with the conditional one appended.
        let upload = catalog
            .prepare_upload(&session, &resolved)
            .expect("uploads");
        assert_eq!(
            upload.unknowns(),
            [
                MeshPresentationUnknown::FrontFaceWinding,
                MeshPresentationUnknown::UvOrigin,
                MeshPresentationUnknown::VertexColor,
                MeshPresentationUnknown::MultiMaterialGroup,
            ]
        );
        let single = catalog
            .resolve(&session, &gamez_key(), 1)
            .expect("the single-group mesh resolves");
        let single = catalog
            .prepare_upload(&session, &single)
            .expect("the single-group mesh uploads");
        assert_eq!(
            single.unknowns(),
            [
                MeshPresentationUnknown::FrontFaceWinding,
                MeshPresentationUnknown::UvOrigin,
                MeshPresentationUnknown::VertexColor,
            ],
            "a mesh with one group per polygon has no multi-group question"
        );
        assert_eq!(single.render().extra_group_triangles(), 0);
    }

    /// **F10-C.01's source maps and its degenerate accounting survive every
    /// group.** A second UV set is a new attribute to key on; it must not become a
    /// new way to lose a map. Every triangle still names the stored topology
    /// triangle it came from and the group it is the drawing of, every vertex
    /// still names its first `(polygon, corner)`, degeneracy is still a property
    /// of the stored indices, and the material grouping still covers every
    /// triangle exactly once.
    #[test]
    fn accept_f10_e_source_maps_and_degenerates_survive_every_group() {
        let tree = group_tree();
        let session = world_session(&tree.0, "ZBD/c1");
        let archive = texture_key();
        let textures = TextureCatalog::open(&session, std::slice::from_ref(&archive));
        let catalog = MeshCatalog::open(
            &session,
            &[gamez_key()],
            &seam_dependencies(&textures, &archive),
        );
        let container = catalog.containers().next().expect("one container");
        let stored = container.meshes().get(0).expect("mesh 0");
        let resolved = catalog
            .resolve(&session, &gamez_key(), 0)
            .expect("resolves");
        let upload = catalog
            .prepare_upload(&session, &resolved)
            .expect("uploads");
        let render = upload.render();
        let topology = container
            .topology(0)
            .expect("the stored topology is kept for the row");

        // Every `(polygon, step)` of the stored topology, times every stored group
        // of that polygon, and nothing else. This is F10-C.01's triangle map
        // multiplied out by the groups.
        let mut want: Vec<(usize, usize, usize)> = Vec::new();
        for triangle in &topology.triangles {
            let groups = stored.groups(triangle.polygon).expect("groups");
            for group in 0..groups.len() {
                want.push((triangle.polygon, triangle.step, group));
            }
        }
        let got: Vec<(usize, usize, usize)> = render
            .triangles()
            .iter()
            .map(|t| (t.source.polygon, t.source.step, t.group))
            .collect();
        assert_eq!(got, want, "the triangle source map covers every group once");

        // Every vertex names a real corner of the polygon that produced it, and
        // its own group.
        for (index, vertex) in render.vertices().iter().enumerate() {
            let polygon = stored
                .mesh
                .polygons
                .get(vertex.source.polygon)
                .expect("a stored polygon");
            assert!(
                vertex.source.corner < polygon.corners.len(),
                "vertex {index} names a corner the polygon does not have"
            );
            assert!(
                vertex.group < stored.groups(vertex.source.polygon).expect("groups").len(),
                "vertex {index} names a group the polygon does not have"
            );
        }

        // Degeneracy is the stored position indices' own: the same steps in every
        // group of a polygon, and no others.
        for polygon in 0..stored.mesh.polygons.len() {
            let degenerate: Vec<usize> = topology
                .triangles
                .iter()
                .filter(|t| t.polygon == polygon && t.is_degenerate())
                .map(|t| t.step)
                .collect();
            let groups = stored.groups(polygon).expect("groups");
            for group in 0..groups.len() {
                let marked: Vec<usize> = group_triangles(render, polygon, group)
                    .iter()
                    .filter(|t| t.degenerate)
                    .map(|t| t.source.step)
                    .collect();
                assert_eq!(marked, degenerate, "polygon {polygon} group {group}");
            }
        }

        // The material grouping is still a partition: every triangle in exactly
        // one group, ascending, and the groups are the stored material indices in
        // ascending order.
        let mut covered = vec![false; render.triangles().len()];
        for group in render.groups() {
            assert!(
                group.triangles.windows(2).all(|pair| pair[0] < pair[1]),
                "material {} holds its triangles in ascending order",
                group.material
            );
            for &index in &group.triangles {
                assert!(
                    !covered[index],
                    "triangle {index} is in two material groups"
                );
                covered[index] = true;
                assert_eq!(render.triangles()[index].material, group.material);
            }
        }
        assert!(
            covered.iter().all(|seen| *seen),
            "every triangle is grouped"
        );
        let materials: Vec<u32> = render.groups().iter().map(|g| g.material).collect();
        let mut sorted = materials.clone();
        sorted.sort_unstable();
        assert_eq!(materials, sorted, "material groups in ascending order");
        // A group's material reaches a material group of its own.
        for attributes in stored.material_groups.iter().flatten() {
            assert!(
                materials.contains(&attributes.material),
                "stored material {} is a render material group: {materials:?}",
                attributes.material
            );
        }
    }

    /// **A polygon with no stored group is refused by name, not invented.** The
    /// reference asserts `mat_count > 0` and the measured corpus never stores
    /// `0`, so this is the only way to reach the refusal through the production
    /// reader. Two ways of quietly getting on with it are both worse than a
    /// refusal: drawing the face with a material the bytes never named, and
    /// dropping the face.
    ///
    /// The refusal is a **row** with its code, its diagnostic and its exact face
    /// counts, nothing is uploaded for the mesh, and the sibling face's own
    /// polygon is named in the diagnostic so the cause is locatable.
    #[test]
    fn accept_f10_e_a_polygon_without_a_stored_group_is_refused_by_name() {
        let tree = Tree::world(&["sky", "ground"], &["decoy"]);
        tree.write("ZBD/c1/gamez.zbd", &no_group_container());
        let session = world_session(&tree.0, "ZBD/c1");
        let archive = texture_key();
        let textures = TextureCatalog::open(&session, std::slice::from_ref(&archive));
        let catalog = MeshCatalog::open(
            &session,
            &[gamez_key()],
            &seam_dependencies(&textures, &archive),
        );
        assert_eq!(
            catalog.failures().count(),
            0,
            "the container reads: the layout can walk a polygon with no group"
        );

        let records = catalog.records();
        assert_eq!(records.len(), 1, "the one present stored mesh is a row");
        let row = &records[0];
        assert_eq!(row.readiness, RenderMeshReadiness::Failed);
        assert_eq!(
            row.unsupported_reasons,
            ["render_mesh_refused", "polygon_without_material_group"],
            "the refusal is named on the row"
        );
        let failure = row.failure.as_ref().expect("the reader's context");
        assert_eq!(failure.stage, MeshFailureStage::Render);
        assert_eq!(failure.mesh, Some(0));
        assert_eq!(
            failure.offset, None,
            "a check over bytes that were read whole invents no offset"
        );
        assert!(
            failure
                .diagnostic
                .contains("polygon 0 stored no material group"),
            "{}",
            failure.diagnostic
        );
        // The face accounting is exact even for a mesh nothing draws: two stored
        // polygons, both decoded, so the refusal is not a face count in disguise.
        let faces = row.faces.expect("face counts");
        assert_eq!((faces.faces, faces.triangles, faces.rejected), (2, 2, 0));
        assert_eq!(faces.multi_material_group_polygons, 0);

        // And the lookup refuses with the same stable code rather than handing
        // over a render mesh that invents a material.
        let error = catalog
            .resolve(&session, &gamez_key(), 0)
            .expect_err("the mesh is not uploadable");
        assert_eq!(error.code(), "polygon_without_material_group");
        assert!(error.to_string().contains("not uploadable"), "{error}");

        // The two remaining refusals are guard clauses on a **public** entry
        // point: the reader builds the table beside the polygons from the same
        // two stored counts, so a table that does not describe them can only come
        // from a caller. They are named, not indexed, and measured on the corpus
        // as never firing.
        let one = RawPolygon {
            kind: PrimitiveKind::Polygon,
            raw_flags: 0,
            material: 0,
            corners: vec![corner(0), corner(1), corner(2)],
        };
        let mesh = RawMesh {
            positions: POSITIONS.to_vec(),
            normals: NORMALS.to_vec(),
            polygons: vec![one.clone()],
        };
        let group = |material: u32, uvs: &[[f32; 2]]| {
            vec![RawMaterialGroup {
                material,
                uvs: uvs.to_vec(),
            }]
        };
        assert_eq!(
            RenderMesh::from_stored_groups(&mesh, &[]),
            Err(RenderMeshError::GroupCount {
                polygons: 1,
                groups: 0
            })
        );
        assert_eq!(
            RenderMesh::from_stored_groups(&mesh, &[Vec::new()]),
            Err(RenderMeshError::PolygonWithoutMaterialGroup { polygon: 0 })
        );
        assert_eq!(
            RenderMesh::from_stored_groups(&mesh, &[group(3, &[[0.0, 0.0]])]),
            Err(RenderMeshError::GroupCornerCount {
                polygon: 0,
                group: 0,
                corners: 3,
                uvs: 1
            })
        );
        // The same three polygons with a table that does describe them build, so
        // the refusals above are about the table and not about the mesh.
        assert!(
            RenderMesh::from_stored_groups(
                &mesh,
                &[group(3, &[[0.0, 0.0], [0.5, 0.0], [1.0, 1.0]])]
            )
            .is_ok()
        );
        assert_eq!(
            one.corners.len(),
            3,
            "the IR fixture the refusals above ran against is the one the reader would build"
        );
    }

    /// **The documented decision that the group index is not in the vertex key.**
    ///
    /// F10-C.01's key is the *bit-exact identity* of a render vertex — position,
    /// normal, UV, colour and material — and it stays that shape here. Two
    /// material groups of one polygon that store the **same** material index and
    /// the **same** coordinate for a corner are therefore one vertex, not two,
    /// and [`RenderVertex::group`] names the first group that reached it. This is
    /// stated on both fields' documentation, so it is pinned here rather than
    /// left to a later reader to infer: putting the group index back into the
    /// key would duplicate every vertex of such a polygon and this fails.
    ///
    /// Both groups are still **drawn** — one triangle per topology triangle
    /// each — because a draw belongs to a polygon and a group, not to a vertex,
    /// and that is what [`RenderTriangle::group`] is for.
    #[test]
    fn accept_f10_e_groups_that_agree_bit_exactly_share_one_vertex() {
        let uvs = [[0.0, 0.0], [0.5, 0.0], [0.25, 0.25]];
        let polygon = RawPolygon {
            kind: PrimitiveKind::Polygon,
            raw_flags: 0,
            // The IR mirrors group 0, which is what the reader would have written.
            material: 7,
            corners: vec![corner(0), corner(1), corner(2)],
        };
        let mesh = RawMesh {
            positions: POSITIONS.to_vec(),
            normals: NORMALS.to_vec(),
            polygons: vec![polygon],
        };
        // Two groups, the same raw material index and the same three coordinates.
        let table = vec![vec![
            RawMaterialGroup {
                material: 7,
                uvs: uvs.to_vec(),
            },
            RawMaterialGroup {
                material: 7,
                uvs: uvs.to_vec(),
            },
        ]];
        let render = RenderMesh::from_stored_groups(&mesh, &table).expect("the table describes it");

        assert_eq!(
            render.source_triangles(),
            2,
            "one triangle per stored group"
        );
        assert_eq!(
            render.extra_group_triangles(),
            1,
            "the second group is one draw beyond the first"
        );
        assert_eq!(
            render.vertices().len(),
            3,
            "the two groups store identical keys, so the corners are one vertex each"
        );
        assert!(
            render.vertices().iter().all(|vertex| vertex.group == 0),
            "the shared vertices name the first group that reached them"
        );
        // Both draws still name their own group and carry its material, so the
        // sharing costs no group anything.
        for (group, triangle) in render.triangles().iter().enumerate() {
            assert_eq!(triangle.group, group, "one draw per stored group");
            assert_eq!(triangle.material, 7, "both groups store material 7");
            assert_eq!(triangle.source.polygon, 0);
            for &index in &triangle.vertices {
                assert_eq!(
                    render.vertices()[index as usize].uv,
                    Some(uvs[render.vertices()[index as usize].source.corner]),
                    "every draw samples the coordinate of the corner it was reached for"
                );
            }
        }
        // The single-group reading of the same mesh is one draw of the same three
        // vertices, which is the difference the group path exists to make.
        let single = RenderMesh::build(&mesh).expect("complete topology");
        assert_eq!(single.source_triangles(), 1);
        assert_eq!(single.vertices().len(), 3);
        assert_eq!(single.extra_group_triangles(), 0);
    }

    /// **The corpus, through the whole production path.** Every GameZ polygon of
    /// `ZBD/C1/gamez.zbd` stores one, two or three material groups, and this
    /// states on the original installation that
    ///
    /// * every row's draws are its stored topology triangles plus exactly the
    ///   draws the later groups add — the arithmetic that only holds when no
    ///   group is dropped and none is invented;
    /// * for every multi-group polygon of every row, each stored group's own raw
    ///   material index and its own per-corner UV set are on the payload, corner
    ///   for corner, through the production source map;
    /// * a seam that exists **only** in a group beyond the first is visible,
    ///   which is AC03 for the second group on real data and not only on the
    ///   fixture;
    /// * the presentation question is on the rows that stored such a polygon and
    ///   on no other, and the retired loss code is on none.
    ///
    /// It asserts the *invariants*, not this owner's exact numbers; the measured
    /// figures are in
    /// `docs/findings/2026-09-29-f10-e-material-groups-into-the-render-mesh.md`
    /// and the per-archive counts are F10-B's
    /// `accept_f10_b_gamez_retail_flags_groups_and_seams_over_the_whole_corpus`.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f10_e_retail_world_multi_group_polygons_keep_every_stored_group() {
        let game_dir = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
        let found = install::discover(&game_dir).expect("the installation is discovered");
        let group = found
            .diagnosis
            .world_groups
            .iter()
            .find(|group| group.as_str().eq_ignore_ascii_case("ZBD/c1"))
            .expect("world C1 is discovered")
            .clone();
        let context = ResolveContext::new(install::fingerprint(&found.manifest))
            .with_world_group(WorldGroup::from_relative(group));
        let mut builder = SessionBuilder::new(context);
        builder
            .mount_installation(&game_dir, &found.diagnosis)
            .expect("the installation mounts");
        let session = builder.open();

        let archive = texture_key();
        let textures = TextureCatalog::open(&session, std::slice::from_ref(&archive));
        assert_eq!(textures.failures().count(), 0, "the archive opens");
        let key = gamez_key();
        let catalog = MeshCatalog::open(
            &session,
            std::slice::from_ref(&key),
            &seam_dependencies(&textures, &archive),
        );
        assert_eq!(
            catalog.failures().count(),
            0,
            "the world's own GameZ container opens and both readers accept it"
        );
        let container = catalog.containers().next().expect("one container");

        let presentation = MeshPresentationUnknown::MultiMaterialGroup.code();
        let mut rows = 0usize;
        let mut multi_group_polygons = 0usize;
        let mut meshes_with_multi_group = 0usize;
        let mut extra_draws = 0usize;
        let mut three_group_polygons = 0usize;
        // Seams that only exist in a group beyond the first, per the corpus's own
        // stored bytes rather than per the render mesh.
        let mut second_group_seams = 0usize;
        let mut identical_uv_groups = 0usize;

        for row in catalog.records() {
            let index = row.mesh_index.expect("every row is about a mesh");
            let counts = row.faces.expect("face counts");
            let resolved = catalog
                .resolve(&session, &key, index)
                .expect("every stored world mesh resolves");
            let upload = catalog
                .prepare_upload(&session, &resolved)
                .expect("every stored world mesh uploads");
            let render = upload.render();
            rows += 1;

            assert_eq!(render.source_faces(), counts.faces);
            assert_eq!(
                render.source_triangles(),
                counts.triangles + render.extra_group_triangles(),
                "mesh {index}: the draws are the stored triangles plus each polygon's own groups"
            );
            assert_eq!(
                render.degenerate_triangles(),
                counts.degenerate + render.extra_group_degenerate_triangles()
            );
            assert_eq!(
                row.unsupported_reasons.contains(&presentation.to_owned()),
                counts.multi_material_group_polygons > 0,
                "mesh {index}: the presentation question is on the row only where it is real"
            );
            assert_eq!(
                upload
                    .unknowns()
                    .contains(&MeshPresentationUnknown::MultiMaterialGroup),
                counts.multi_material_group_polygons > 0
            );
            for reason in &row.unsupported_reasons {
                assert_ne!(reason, "multi_material_group_polygons");
            }
            if counts.multi_material_group_polygons == 0 {
                assert_eq!(render.extra_group_triangles(), 0, "mesh {index}");
                continue;
            }
            meshes_with_multi_group += 1;
            multi_group_polygons += counts.multi_material_group_polygons;
            extra_draws += render.extra_group_triangles();

            let stored = container.meshes().get(index).expect("the stored mesh");
            let topology = container.topology(index).expect("the stored topology");
            assert!(
                stored.groups_are_complete(),
                "mesh {index}: every stored polygon has its own group list"
            );
            for (polygon, face) in stored.mesh.polygons.iter().enumerate() {
                let groups = stored.groups(polygon).expect("groups");
                if groups.len() == 1 {
                    continue;
                }
                if groups.len() == 3 {
                    three_group_polygons += 1;
                }
                // Every stored group holds one coordinate per corner of its
                // polygon — the reader's own arithmetic, checked here against
                // the IR it produced.
                for attributes in groups {
                    assert_eq!(
                        attributes.uvs.len(),
                        face.corners.len(),
                        "mesh {index} polygon {polygon}: one stored coordinate per corner"
                    );
                }
                if groups.iter().skip(1).all(|g| g.uvs == groups[0].uvs) {
                    identical_uv_groups += 1;
                }
                // Every group is drawn, with its own material and its own UVs.
                for (group, attributes) in groups.iter().enumerate() {
                    assert_group_samples_its_own_uvs(render, stored, topology, polygon, group);
                    for triangle in group_triangles(render, polygon, group) {
                        assert_eq!(triangle.material, attributes.material);
                    }
                }
                // And the corpus's shared positions: a position two or more of
                // this polygon's corners use. For every group, the render mesh
                // must hold exactly the coordinates that group authors there,
                // over **all** of the polygon's corners at that position.
                let mut at: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
                for (corner, raw) in face.corners.iter().enumerate() {
                    at.entry(raw.position).or_default().push(corner);
                }
                for (position, corners) in at {
                    if corners.len() < 2 {
                        continue;
                    }
                    let authored = |group: usize| {
                        let mut bits: Vec<[u32; 2]> = corners
                            .iter()
                            .map(|&corner| {
                                let uv = groups[group].uvs[corner];
                                [uv[0].to_bits(), uv[1].to_bits()]
                            })
                            .collect();
                        bits.sort_unstable();
                        bits.dedup();
                        bits
                    };
                    for group in 0..groups.len() {
                        assert_eq!(
                            uvs_at(render, topology, polygon, group, position),
                            authored(group),
                            "mesh {index} polygon {polygon} group {group}: position {position} \
                             keeps every coordinate that group authors there"
                        );
                    }
                    // A seam the first-group mirroring could not show: the first
                    // group authors one coordinate for the whole position while a
                    // group beyond it authors more than one. This is AC03 for the
                    // second group on real data.
                    if authored(0).len() == 1 && (1..groups.len()).any(|g| authored(g).len() > 1) {
                        second_group_seams += 1;
                    }
                }
            }
        }

        // The measured figures for this installation. They are stated as a
        // floor rather than a checksum: the invariants above are what must hold
        // for any owner copy, and these say this one really exercises the path.
        assert!(
            rows > 2_000,
            "the world stores thousands of meshes, got {rows}"
        );
        assert!(
            multi_group_polygons > 0,
            "the world stores multi-group polygons"
        );
        assert!(
            meshes_with_multi_group > 0,
            "on stored meshes, not a whole archive"
        );
        assert!(three_group_polygons > 0, "three-group polygons occur");
        assert!(
            extra_draws > multi_group_polygons,
            "the later groups really add draws: {extra_draws} over {multi_group_polygons}"
        );
        assert!(
            second_group_seams > 0,
            "the corpus authors a seam that only a group beyond the first can show, which is \
             AC03 for the second group on real data"
        );
        // Some polygons store the same coordinates in every group, so the split
        // there is on the material index alone. Recorded because the measured
        // corpus has them (17 of 1 006) and the code is only right if both cases
        // are real.
        assert!(
            identical_uv_groups > 0,
            "the corpus stores polygons whose groups agree on every coordinate"
        );
        println!(
            "C1: {rows} rows, {meshes_with_multi_group} with a multi-group polygon, \
             {multi_group_polygons} such polygons ({three_group_polygons} of three groups), \
             {extra_draws} extra draws, {second_group_seams} second-group-only seams, \
             {identical_uv_groups} polygons whose groups agree on every coordinate"
        );
    }
}
