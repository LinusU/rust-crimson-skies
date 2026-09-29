//! Canonical render mesh: GameZ vertices split per corner attribute, and the
//! dependency audit a GameZ mesh's material records imply
//! (`specs/F10-gamez-mesh-topology-and-material-records.md`, stages
//! `### F10-C`, slices F10-C.01 and F10-C.02; shared contract
//! `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! [`RenderMesh`] is the Bevy-free handoff between a GameZ mesh reader
//! (F10-B's [`RawMesh`] plus its [`MeshTopology`]) and the upload adapter.
//! It is built with [`RenderMesh::build`], which computes the topology, or
//! with [`RenderMesh::from_parts`], which takes a topology the caller
//! already has. No Bevy or Avian type appears here.
//!
//! [`MeshDependencyAudit`] is the other half of that handoff: it resolves every
//! stored material index a mesh carries to a material record, every material
//! record to the texture name it stores, and that name to exactly one stored
//! texture in one named archive — or records why it could not.
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
//! The design decisions, the recorded unknowns (front-face winding is still
//! unknown) and the test inventory are in
//! `docs/findings/2026-09-29-f10-c-01-render-vertex-splitting.md` and
//! `docs/findings/2026-09-29-f10-c-02-gamez-material-records.md`.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use cs_assets::install::sha256;
use cs_assets::vfs::ContentSession;
use cs_formats::gamez::materials::{GameZMaterials, MaterialKind, RawMaterial};
use cs_formats::gamez::{FaceStatus, GameZMeshes, MeshTopology, RawMesh};
use cs_types::asset_id::{AssetKey, SourceSpan};
use cs_types::evidence::ContentHash;
use cs_types::install::ParseState;

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
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderVertex {
    /// Position resolved from [`Self::position_index`].
    pub position: [f32; 3],
    /// Normal resolved from [`Self::normal_index`], unchanged and unnormalized.
    pub normal: Option<[f32; 3]>,
    /// Texture coordinate as stored.
    pub uv: Option<[f32; 2]>,
    /// Corner color as stored.
    pub color: Option<[f32; 3]>,
    /// Raw material index of the polygon this vertex was split for.
    pub material: u32,
    /// Stored position index (part of the vertex key).
    pub position_index: u32,
    /// Stored normal index (part of the vertex key).
    pub normal_index: Option<u32>,
    /// First source corner that produced this vertex.
    pub source: SourceCorner,
}

/// One render triangle: three vertex indices and where it came from.
///
/// Degenerate triangles are kept; [`Self::degenerate`] records the stored
/// position-index degeneracy so a consumer can skip them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderTriangle {
    /// Indices into [`RenderMesh::vertices`], in drawing order.
    pub vertices: [u32; 3],
    /// Source triangle in the stored topology.
    pub source: SourceTriangle,
    /// Raw material index (the group this triangle belongs to).
    pub material: u32,
    /// Two of the three stored position indices are equal.
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
        }
    }
}

impl std::error::Error for RenderMeshError {}

/// A canonical, Bevy-free render mesh built from a [`RawMesh`] and its
/// [`MeshTopology`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderMesh {
    vertices: Vec<RenderVertex>,
    triangles: Vec<RenderTriangle>,
    groups: Vec<RenderGroup>,
    source_faces: usize,
    degenerate_triangles: usize,
}

impl RenderMesh {
    /// Builds the render mesh from `mesh`, computing its topology first.
    ///
    /// # Errors
    ///
    /// Any [`RenderMeshError`] [`Self::from_parts`] returns.
    pub fn build(mesh: &RawMesh) -> Result<Self, RenderMeshError> {
        Self::from_parts(mesh, &mesh.topology())
    }

    /// Builds the render mesh from `mesh` and a topology the caller already
    /// has.
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
                let key = VertexKey {
                    position: raw.position,
                    normal: raw.normal,
                    uv: raw.uv.map(|uv| uv.map(f32::to_bits)),
                    color: raw.color.map(|color| color.map(f32::to_bits)),
                    material: polygon.material,
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
                            uv: raw.uv,
                            color: raw.color,
                            material: polygon.material,
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
            triangles.push(RenderTriangle {
                vertices: indices,
                source: SourceTriangle {
                    polygon: triangle.polygon,
                    step: triangle.step,
                },
                material: polygon.material,
                degenerate: triangle.is_degenerate(),
            });
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
        })
    }

    /// Every render vertex, in first-encounter order.
    #[must_use]
    pub fn vertices(&self) -> &[RenderVertex] {
        &self.vertices
    }

    /// Every render triangle, in topology order.
    #[must_use]
    pub fn triangles(&self) -> &[RenderTriangle] {
        &self.triangles
    }

    /// Triangles grouped by raw material index, groups in ascending material
    /// order.
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
    #[must_use]
    pub fn source_triangles(&self) -> usize {
        self.triangles.len()
    }

    /// Triangles with two equal stored position indices. They are kept; a
    /// consumer that draws may skip them.
    #[must_use]
    pub fn degenerate_triangles(&self) -> usize {
        self.degenerate_triangles
    }
}

/// The bit-exact identity of one render vertex.
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
    /// Stable codes of everything that keeps the row from being ready.
    pub unsupported_reasons: Vec<String>,
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
    /// Rows that did not, and why they are still present.
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
                blocked.push(format!("material {index}: {}", row.state));
                for reason in &row.unsupported_reasons {
                    blocked.push(format!("material {index}: {reason}"));
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
    let mut reasons: Vec<String> = Vec::new();
    for finding in materials
        .findings
        .iter()
        .filter(|finding| finding.material() == index)
    {
        reasons.push(format!("{}:{}", finding.code(), finding));
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
        return finish(row, false, state, reasons);
    };
    let bits = material.record.unknown_flag_bits();
    if bits != 0 {
        return finish(row, true, MaterialState::UnknownField { bits }, reasons);
    }
    if material.kind() == MaterialKind::Colored {
        return finish(row, true, MaterialState::Untextured, reasons);
    }
    let Some(texture) = materials.texture(material.record.texture_index) else {
        let state = MaterialState::TextureIndexOutOfRange {
            index: material.record.texture_index,
            available: materials.textures.len() as u32,
        };
        return finish(row, true, state, reasons);
    };
    if let Some(indices) = duplicated.get(&texture.name) {
        reasons.push(format!(
            "container_texture_name_duplicated: {} table indices {indices:?}",
            texture.name
        ));
    }
    if let Some(state) = archive_state {
        return finish(row, true, state.clone(), reasons);
    }
    let reference = TextureRef::new(context.archive.clone(), &texture.name);
    match context.catalog.resolve(context.session, &reference) {
        Ok(resolved) => {
            let state = MaterialState::Resolved {
                texture: resolved.id().clone(),
            };
            finish(row, true, state, reasons)
        }
        Err(error) => {
            reasons.push(error.code().to_owned());
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
            finish(row, true, state, reasons)
        }
    }
}

/// Fills in the four fields that follow from the state, and returns the row.
///
/// `has_record` says whether the material table holds a record for this index at
/// all: a stored index outside the table is a **failed parse**, while every other
/// state is a record that was read and whose dependency did or did not reach an
/// origin.
fn finish(
    mut row: MaterialRow,
    has_record: bool,
    state: MaterialState,
    mut reasons: Vec<String>,
) -> MaterialRow {
    reasons.dedup();
    if !state.is_complete() && reasons.is_empty() {
        reasons.push(state.code().to_owned());
    }
    let ready = state.is_complete() && reasons.is_empty();
    let diagnostic = reasons
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
    row.unsupported_reasons = reasons;
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
    use cs_assets::vfs::{ContentSession, SessionBuilder, WORLD_NAMESPACE};
    use cs_formats::gamez::materials::{
        GameZMaterials, GameZTextureName, MATERIAL_FLAG_ALWAYS, MATERIAL_FLAG_TEXTURED,
        MaterialInfo, RawMaterial, RawMaterialRecord, TextureNameEncoding,
    };
    use cs_formats::gamez::reader::{MeshIndex, RawMaterialGroup, RawMeshInfo};
    use cs_formats::gamez::{GameZHeader, GameZMesh, GameZMeshes, RawMeshMaterialInfo};
    use cs_formats::texture::zbd::{
        FLAG_BYTES_PER_PIXEL2, FLAG_NO_ALPHA, ZBD_TEXTURE_HEADER_BYTES,
    };
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
        // caller can see that no single container entry owns this name.
        assert!(
            row.unsupported_reasons
                .iter()
                .any(|reason| reason.starts_with("container_texture_name_duplicated:")),
            "{:?}",
            row.unsupported_reasons
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
}
