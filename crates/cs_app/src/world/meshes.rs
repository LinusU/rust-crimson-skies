//! The geometry a world's mesh references resolve to (F18-B).
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stage `### F18-B`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! An object's record names the mesh it draws in exactly one field
//! ([`cs_content::world::WorldObjectInstance::mesh`]), and its collision shape
//! names **no** mesh of its own: [`WorldCollisionShape::FromMesh`] means "the
//! geometry of that same reference". This module is where a reference meets an
//! actual upload, so there is exactly one place a mesh can be looked up, one
//! conversion to the engine's mesh type, and one fingerprint to compare.
//!
//! [`WorldMesh`] keeps that upload's **provenance**: the F17-B
//! [`GroupUpload::fingerprint`] of the exact geometry both consumers use and the
//! triangle count that geometry stored. A test can therefore assert that the
//! collider Avian derived carries the record's triangles and no substitute
//! shape, instead of trusting that it does.
//!
//! # The catalog is the source; the map is only the last hop
//!
//! F18-B's first cut took one F17-B [`GroupUpload`] and a caller-built map. That
//! is exact for a mesh whose polygons all store one material index — which is
//! every fixture it had — but F17-B's adapter groups triangles by their stored
//! **raw material index** ([`cs_content::mesh::RenderGroup`]), so a real mesh
//! carries one group per distinct material. Uploading only group `0` would
//! collide and draw only the smallest material's triangles and silently drop the
//! rest.
//!
//! [`WorldMeshes::insert_mesh_upload`] is the production source: it takes the
//! [`MeshUpload`] payload [`cs_content::mesh::MeshCatalog::prepare_upload`]
//! hands over and uploads **every** material group through the same F17-B
//! adapter, then merges them into one engine mesh. This module never resolves or
//! parses anything itself; the catalog owns bytes and lifetimes, and this is its
//! renderer-side consumer exactly as F10-C.03 designed the boundary.
//!
//! # The multi-group decision: one merged collider
//!
//! A stored mesh with several material groups has exactly one geometry: every
//! group's triangles are the *same stored topology triangles* re-drawn with that
//! group's own material and UVs (F10-E measured this — a multi-group polygon is
//! one polygon, not two). Collision with a static world must be the union of
//! every stored triangle, so the world path builds **one** mesh and Avian
//! derives **one** `TrimeshFromMesh` collider from it. Drawing one node per
//! group and colliding one collider per group would be equivalent geometry with
//! more entities; refusing a multi-group mesh would drop 307 of the
//! installation's 17 139 stored meshes (`docs/findings/2026-09-29-f10-e-...`).
//!
//! The merge is lossless for geometry: positions and indices are appended
//! verbatim, with every index offset by the preceding groups' vertex count. A
//! per-corner attribute (`normal`, `uv`, `color`) that **every** group stores is
//! concatenated; one that **no** group stores is simply absent, as it was in
//! every group; one that only *some* groups store cannot be concatenated without
//! padding the others with invented values, so it is dropped and named by
//! [`WorldMesh::dropped_attributes`] rather than hidden.
//!
//! # What is *not* claimed
//!
//! No original mesh was read to build this. The world-vertex unit scale is the
//! measured metre (task #677) and the coordinate handedness is measured
//! code-derived (task #436's owner note); the per-object
//! material binding F17 owns is not built here: this module supplies geometry and
//! its provenance, not a material. How a retail world node's `mesh_index` (and
//! its variant) maps to a [`ContentId`] is F18-D's question, and whether two
//! objects naming one mesh should share an engine asset handle is a follow-up
//! recorded in `docs/findings/2026-09-30-f18-b-followup-mesh-source-from-catalog.md`.

use std::collections::BTreeMap;
use std::fmt;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, MeshVertexAttribute, PrimitiveTopology, VertexAttributeValues};
use cs_content::mesh::{MeshPresentationUnknown, MeshUpload, RenderMesh};
use cs_types::content::ContentId;
use cs_types::evidence::ContentHash;

use crate::render::bevy_mesh::{AttributeKind, GroupUpload, MeshAdapterError, upload_groups};

/// One material group's provenance inside a [`WorldMesh`].
///
/// A group is one F17-B upload: the triangles of one stored raw material index.
/// Keeping every group's own fingerprint and counts is what makes "every group
/// was merged" checkable, rather than a number that a partial upload could also
/// produce.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldMeshGroup {
    /// The stored raw material index this group draws.
    material: u32,
    /// The F17-B fingerprint of this group's own uploaded buffers.
    fingerprint: ContentHash,
    /// Triangles this group uploaded, degenerate ones included.
    triangles: usize,
    /// Vertices this group uploaded, after per-group compaction.
    vertices: usize,
    /// Position components the upload canonicalised to a signed zero under
    /// [`crate::render::bevy_mesh::SUBNORMAL_POSITION_CLAIM`].
    subnormal_components: usize,
}

impl WorldMeshGroup {
    /// The stored raw material index this group draws.
    #[must_use]
    pub const fn material(self) -> u32 {
        self.material
    }

    /// The F17-B fingerprint of this group's own buffers.
    #[must_use]
    pub const fn fingerprint(self) -> ContentHash {
        self.fingerprint
    }

    /// Triangles this group uploaded.
    #[must_use]
    pub const fn triangles(self) -> usize {
        self.triangles
    }

    /// Vertices this group uploaded.
    #[must_use]
    pub const fn vertices(self) -> usize {
        self.vertices
    }

    /// Position components canonicalised to a signed zero by the upload's
    /// declared [`crate::render::bevy_mesh::SUBNORMAL_POSITION_CLAIM`] rule.
    /// `0` means every stored component kept its own bit pattern.
    #[must_use]
    pub const fn subnormal_components(self) -> usize {
        self.subnormal_components
    }
}

/// One uploaded mesh, with the provenance of the upload it came from.
#[derive(Clone, Debug)]
pub struct WorldMesh {
    mesh: Mesh,
    fingerprint: ContentHash,
    triangles: usize,
    groups: Vec<WorldMeshGroup>,
    dropped_attributes: Vec<AttributeKind>,
}

impl WorldMesh {
    /// Takes over one material group's upload — the value both the visual and
    /// the collision of an object are built from.
    ///
    /// The fingerprint and the triangle count are read **before** the upload is
    /// consumed, so the record describes the geometry the engine is about to
    /// hold rather than being computed from a second copy of it.
    #[must_use]
    pub fn from_upload(upload: GroupUpload) -> Self {
        let group = WorldMeshGroup {
            material: upload.material(),
            fingerprint: upload.fingerprint(),
            triangles: upload.report().triangles,
            vertices: upload.report().vertices,
            subnormal_components: upload.report().subnormal_components,
        };
        Self {
            fingerprint: group.fingerprint,
            triangles: group.triangles,
            mesh: upload.into_mesh(),
            groups: vec![group],
            dropped_attributes: Vec::new(),
        }
    }

    /// Merges **every** material group of one stored mesh into the single
    /// engine mesh both consumers use.
    ///
    /// A single group is taken over unchanged (its mesh, its fingerprint, its
    /// triangle count), so this is a strict superset of [`Self::from_upload`].
    ///
    /// # Errors
    ///
    /// [`WorldMeshBuildError::NoGeometry`] when `groups` is empty: a stored mesh
    /// with no triangles has nothing to collide or draw, and an empty upload
    /// would present a world object that draws nothing while the report claims it
    /// was built. [`WorldMeshBuildError::GroupWithoutPositions`] or
    /// [`WorldMeshBuildError::GroupWithoutIndices`] when a group's upload is not
    /// the position/index pair the F17-B adapter produces, and
    /// [`WorldMeshBuildError::TooManyVertices`] when the merged vertex count
    /// passes the `u32` index range.
    pub fn from_group_uploads(mut groups: Vec<GroupUpload>) -> Result<Self, WorldMeshBuildError> {
        if groups.is_empty() {
            return Err(WorldMeshBuildError::NoGeometry);
        }
        let provenance: Vec<WorldMeshGroup> = groups
            .iter()
            .map(|upload| WorldMeshGroup {
                material: upload.material(),
                fingerprint: upload.fingerprint(),
                triangles: upload.report().triangles,
                vertices: upload.report().vertices,
                subnormal_components: upload.report().subnormal_components,
            })
            .collect();
        if groups.len() == 1 {
            let upload = groups.pop().expect("the list is non-empty");
            let mut mesh = Self::from_upload(upload);
            mesh.groups = provenance;
            return Ok(mesh);
        }
        let (mesh, dropped_attributes) = merge_group_meshes(&groups)?;
        let triangles = provenance.iter().map(|group| group.triangles).sum();
        Ok(Self {
            mesh,
            fingerprint: combined_fingerprint(&provenance),
            triangles,
            groups: provenance,
            dropped_attributes,
        })
    }

    /// The uploaded geometry: one mesh holding every material group's triangles.
    #[must_use]
    pub const fn mesh(&self) -> &Mesh {
        &self.mesh
    }

    /// The canonical fingerprint of this geometry.
    ///
    /// For a single-group mesh this is the F17-B upload's own fingerprint. For a
    /// merged one it is a digest over every group's material index, fingerprint
    /// and counts, so a change to any group changes this value; the group values
    /// themselves are on [`Self::groups`].
    #[must_use]
    pub const fn fingerprint(&self) -> ContentHash {
        self.fingerprint
    }

    /// How many triangles the upload stored, every group's included. A
    /// mesh-derived collider that carries a different count has been
    /// substituted, which F18 non-negotiable behavior 1 forbids.
    #[must_use]
    pub const fn triangles(&self) -> usize {
        self.triangles
    }

    /// Every material group that was uploaded, in group order.
    #[must_use]
    pub fn groups(&self) -> &[WorldMeshGroup] {
        &self.groups
    }

    /// How many material groups the stored mesh had.
    #[must_use]
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    /// How many stored position components the upload canonicalised to a
    /// signed zero under [`crate::render::bevy_mesh::SUBNORMAL_POSITION_CLAIM`],
    /// summed over every group — `0` means every stored component kept its own
    /// bit pattern.
    #[must_use]
    pub fn subnormal_components(&self) -> usize {
        self.groups
            .iter()
            .map(|group| group.subnormal_components)
            .sum()
    }

    /// Per-corner attributes no merged mesh could carry because at least one
    /// group did not store them for its whole vertex set.
    ///
    /// Empty for a single group (taken over unchanged) and for a merge whose
    /// groups agree. A non-empty list is a visible property of the merge, not a
    /// silent drop.
    #[must_use]
    pub fn dropped_attributes(&self) -> &[AttributeKind] {
        &self.dropped_attributes
    }
}

/// Why a whole stored mesh could not become one world mesh.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorldMeshBuildError {
    /// The F17-B adapter refused a material group.
    Adapter(MeshAdapterError),
    /// The stored mesh has no material group at all, so there is nothing to
    /// collide or draw.
    NoGeometry,
    /// A group's upload has no position buffer, which the F17-B adapter
    /// guarantees it does. Reported rather than indexed.
    GroupWithoutPositions {
        /// The group that had no positions.
        group: usize,
    },
    /// A group's upload has no index buffer, which the F17-B adapter guarantees
    /// it does. Reported rather than indexed.
    GroupWithoutIndices {
        /// The group that had no indices.
        group: usize,
    },
    /// The merged vertex count passes the `u32` index range.
    TooManyVertices {
        /// Vertices already merged.
        vertices: usize,
    },
}

impl fmt::Display for WorldMeshBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Adapter(err) => write!(f, "a material group was refused: {err}"),
            Self::NoGeometry => write!(f, "the stored mesh has no material group"),
            Self::GroupWithoutPositions { group } => {
                write!(f, "material group {group} has no position buffer")
            }
            Self::GroupWithoutIndices { group } => {
                write!(f, "material group {group} has no index buffer")
            }
            Self::TooManyVertices { vertices } => {
                write!(f, "{vertices} merged vertices exceed the u32 index range")
            }
        }
    }
}

impl std::error::Error for WorldMeshBuildError {}

impl From<MeshAdapterError> for WorldMeshBuildError {
    fn from(err: MeshAdapterError) -> Self {
        Self::Adapter(err)
    }
}

/// Merges group uploads into one position/index pair, keeping a per-corner
/// attribute only when every group stores it.
///
/// Positions and indices are appended verbatim; an index is offset by the
/// vertex count already merged, so the merged mesh indexes its own buffer. A
/// duplicate triangle from a multi-group polygon is harmless for a trimesh and
/// is kept, because dropping it would be a decision nothing measured.
fn merge_group_meshes(
    groups: &[GroupUpload],
) -> Result<(Mesh, Vec<AttributeKind>), WorldMeshBuildError> {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut normals: Vec<Option<Vec<[f32; 3]>>> = Vec::with_capacity(groups.len());
    let mut uvs: Vec<Option<Vec<[f32; 2]>>> = Vec::with_capacity(groups.len());
    let mut colors: Vec<Option<Vec<[f32; 4]>>> = Vec::with_capacity(groups.len());

    for upload in groups {
        let group = upload.group();
        let mesh = upload.mesh();
        let base =
            u32::try_from(positions.len()).map_err(|_| WorldMeshBuildError::TooManyVertices {
                vertices: positions.len(),
            })?;
        positions.extend(group_positions(mesh, group)?);
        for index in group_indices(mesh, group)? {
            indices.push(
                index
                    .checked_add(base)
                    .ok_or(WorldMeshBuildError::TooManyVertices {
                        vertices: positions.len(),
                    })?,
            );
        }
        normals.push(attribute_f32x3(mesh, Mesh::ATTRIBUTE_NORMAL));
        uvs.push(attribute_f32x2(mesh, Mesh::ATTRIBUTE_UV_0));
        colors.push(attribute_f32x4(mesh, Mesh::ATTRIBUTE_COLOR));
    }

    let mut dropped: Vec<AttributeKind> = Vec::new();
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    if let Some(values) = merge_attribute(normals, &mut dropped, AttributeKind::Normal) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, values);
    }
    if let Some(values) = merge_attribute(uvs, &mut dropped, AttributeKind::Uv) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, values);
    }
    if let Some(values) = merge_attribute(colors, &mut dropped, AttributeKind::Color) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, values);
    }
    mesh.insert_indices(Indices::U32(indices));
    Ok((mesh, dropped))
}

/// Concatenates one attribute over every group, or reports that the merged mesh
/// cannot carry it.
///
/// An attribute **no** group stores is simply absent, exactly as it was in each
/// group, and is not reported as lost. An attribute **some** groups store and
/// others do not cannot be concatenated without inventing values for the groups
/// that lack it, so it is dropped and named in `dropped`. A per-group attribute
/// is all-or-nothing ([`MeshAdapterError::IncompleteAttribute`]), so "some
/// groups store it" is the only mixed case there is.
fn merge_attribute<T>(
    per_group: Vec<Option<Vec<T>>>,
    dropped: &mut Vec<AttributeKind>,
    kind: AttributeKind,
) -> Option<Vec<T>> {
    let present = per_group.iter().filter(|values| values.is_some()).count();
    if present == 0 {
        return None;
    }
    if present != per_group.len() {
        dropped.push(kind);
        return None;
    }
    let mut merged = Vec::new();
    for values in per_group {
        merged.extend(values.expect("every group stored this attribute"));
    }
    Some(merged)
}

/// The position buffer of one group's upload.
fn group_positions(mesh: &Mesh, group: usize) -> Result<Vec<[f32; 3]>, WorldMeshBuildError> {
    match attribute_f32x3(mesh, Mesh::ATTRIBUTE_POSITION) {
        Some(values) => Ok(values),
        None => Err(WorldMeshBuildError::GroupWithoutPositions { group }),
    }
}

/// The index buffer of one group's upload, as `u32`.
fn group_indices(mesh: &Mesh, group: usize) -> Result<Vec<u32>, WorldMeshBuildError> {
    match mesh.indices() {
        Some(Indices::U32(values)) => Ok(values.clone()),
        Some(Indices::U16(values)) => Ok(values.iter().map(|index| u32::from(*index)).collect()),
        None => Err(WorldMeshBuildError::GroupWithoutIndices { group }),
    }
}

/// A three-component `f32` attribute, when the group stores it.
fn attribute_f32x3(mesh: &Mesh, id: MeshVertexAttribute) -> Option<Vec<[f32; 3]>> {
    match mesh.attribute(id) {
        Some(VertexAttributeValues::Float32x3(values)) => Some(values.clone()),
        _ => None,
    }
}

/// A two-component `f32` attribute, when the group stores it.
fn attribute_f32x2(mesh: &Mesh, id: MeshVertexAttribute) -> Option<Vec<[f32; 2]>> {
    match mesh.attribute(id) {
        Some(VertexAttributeValues::Float32x2(values)) => Some(values.clone()),
        _ => None,
    }
}

/// A four-component `f32` attribute, when the group stores it.
fn attribute_f32x4(mesh: &Mesh, id: MeshVertexAttribute) -> Option<Vec<[f32; 4]>> {
    match mesh.attribute(id) {
        Some(VertexAttributeValues::Float32x4(values)) => Some(values.clone()),
        _ => None,
    }
}

/// A digest over every group's material index, fingerprint and counts. It is a
/// property of this merge, never a claim about stored bytes.
fn combined_fingerprint(groups: &[WorldMeshGroup]) -> ContentHash {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"cs/world/world_mesh/v1\0");
    for group in groups {
        bytes.extend_from_slice(&group.material.to_le_bytes());
        bytes.extend_from_slice(group.fingerprint.as_bytes());
        bytes.extend_from_slice(&(group.vertices as u64).to_le_bytes());
        bytes.extend_from_slice(&(group.triangles as u64).to_le_bytes());
    }
    cs_assets::install::sha256(&bytes)
}

/// The meshes one world's object records reference, keyed by authored id.
///
/// A missing entry is not an error to guess around: the spawn reports the
/// object as uncollidable instead of substituting a box or a hull for geometry
/// nobody supplied.
#[derive(Clone, Debug, Default)]
pub struct WorldMeshes {
    meshes: BTreeMap<ContentId, WorldMesh>,
}

impl WorldMeshes {
    /// An empty source: no world object can be built from a mesh.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            meshes: BTreeMap::new(),
        }
    }

    /// Adds one already-uploaded material group under its authored id, returning
    /// the entry it replaced.
    ///
    /// This is the single-group path: exact for a mesh whose polygons all store
    /// one material index, which is what a synthetic fixture authors. A source
    /// fed from the mesh catalog uses [`Self::insert_mesh_upload`], which sees
    /// every group.
    pub fn insert(&mut self, id: ContentId, upload: GroupUpload) -> Option<WorldMesh> {
        self.meshes.insert(id, WorldMesh::from_upload(upload))
    }

    /// Uploads **every** material group of a stored render mesh under its
    /// authored id, returning the entry it replaced.
    ///
    /// # Errors
    ///
    /// Any [`MeshAdapterError`] [`upload_groups`] returns for a group, wrapped in
    /// [`WorldMeshBuildError::Adapter`], and any [`WorldMeshBuildError`] the
    /// merge returns — a stored mesh with no group at all is refused rather than
    /// presented as an empty visual.
    pub fn insert_render_mesh(
        &mut self,
        id: ContentId,
        render: &RenderMesh,
        unknowns: &[MeshPresentationUnknown],
    ) -> Result<Option<WorldMesh>, WorldMeshBuildError> {
        let groups = upload_groups(render, unknowns)?;
        let world_mesh = WorldMesh::from_group_uploads(groups)?;
        Ok(self.meshes.insert(id, world_mesh))
    }

    /// Uploads the mesh the mesh catalog handed over, every material group of
    /// it, under its authored id, returning the entry it replaced.
    ///
    /// This is the production source. The caller resolves and prepares the
    /// upload against its own [`cs_content::mesh::MeshCatalog`] and session (the
    /// catalog owns bytes and lifetimes; this source never resolves anything
    /// itself), then registers it here under the id its world record names.
    ///
    /// # Errors
    ///
    /// Any [`WorldMeshBuildError`] [`Self::insert_render_mesh`] returns.
    pub fn insert_mesh_upload(
        &mut self,
        id: ContentId,
        upload: &MeshUpload,
    ) -> Result<Option<WorldMesh>, WorldMeshBuildError> {
        self.insert_render_mesh(id, upload.render(), upload.unknowns())
    }

    /// The upload registered for `id`, if any.
    #[must_use]
    pub fn get(&self, id: &ContentId) -> Option<&WorldMesh> {
        self.meshes.get(id)
    }

    /// Whether `id` has geometry here.
    #[must_use]
    pub fn contains(&self, id: &ContentId) -> bool {
        self.meshes.contains_key(id)
    }

    /// How many meshes the source holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.meshes.len()
    }

    /// Whether the source holds nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.meshes.is_empty()
    }
}
