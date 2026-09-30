//! The canonical mesh IR to Bevy vertex/index buffers
//! (`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`,
//! stage `### F17-B`; shared contract
//! `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! `cs_content::mesh::RenderMesh` is the lossless, Bevy-free handoff the
//! GameZ reader produced (`docs/findings/2026-09-28-f10-a-lossless-mesh-ir-
//! and-strip-fixtures.md`). This module is the adapter that turns one of its
//! [`RenderGroup`]s — the triangles of one stored material index — into the
//! buffers a Bevy draw call indexes. It is the piece F10-C.01's findings left
//! open on purpose: "whether an upload builds a contiguous index range per
//! material, splits into one buffer per material or keeps one buffer" is
//! named there as F10-C.03 / F17-B's decision, and the answer here is **one
//! compacted buffer per material group**.
//!
//! What the adapter does *not* do is the point of the whole file:
//!
//! * **No vertex value is changed.** Positions, normals, UVs and corner
//!   colors are copied bit-exact from the IR. Nothing is normalized, no `V`
//!   is flipped, no coordinate is wrapped or clamped, no color is converted
//!   or clamped (`cs_content::mesh::RenderVertex` already guarantees the
//!   stored values are unresolved; the adapter keeps them that way).
//! * **No attribute is fabricated.** A group whose vertices all carry a
//!   normal gets the normal buffer; a group where *no* vertex carries one
//!   gets no buffer at all; a group where *some* do is refused
//!   ([`MeshAdapterError::IncompleteAttribute`]) rather than padded with
//!   zeroes, because a padded normal or UV is an invented value that would
//!   silently change every shaded fragment.
//! * **No triangle is dropped.** `RenderTriangle::degenerate` is counted into
//!   [`GroupReport::degenerate_triangles`] and the triangles stay in the
//!   index buffer: removing them here would be a presentation decision made
//!   where nothing measured one (spec F17 non-negotiable 4 — culling must
//!   not remove gameplay geometry).
//! * **No winding is chosen.** Bevy treats counter-clockwise vertices as
//!   front faces. Whether the stored triangles are wound that way is
//!   unmeasured, so the adapter never reverses an index triple. The caller
//!   hands in the [`MeshPresentationUnknown`] list the content pipeline
//!   established ([`GroupUpload::unknowns`]) and the adapter carries it
//!   through untouched, because settling it is not this stage's to do.
//!
//! [`GroupUpload::fingerprint`] digests the exact `f32`/`u32` bit patterns
//! that were handed to Bevy, so a later frame capture can pin *what the GPU
//! was given* without depending on Bevy's internal buffer layout. It is an
//! `Artifact` fingerprint (`cs_types::evidence::FingerprintKind`), never
//! evidence about original data.

use std::collections::HashMap;
use std::fmt;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use cs_assets::install::sha256;
use cs_content::mesh::{MeshPresentationUnknown, RenderMesh};
use cs_types::evidence::ContentHash;

/// Which per-corner attribute a refusal is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AttributeKind {
    /// The stored normal of a corner.
    Normal,
    /// The stored texture coordinate of a corner.
    Uv,
    /// The stored per-corner color.
    Color,
}

impl AttributeKind {
    /// Stable lowercase identifier.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Uv => "uv",
            Self::Color => "color",
        }
    }
}

impl fmt::Display for AttributeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// Why a material group could not become a Bevy mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshAdapterError {
    /// The caller asked for a group the mesh does not have.
    GroupOutOfRange {
        /// The requested group.
        group: usize,
        /// How many groups the mesh has.
        groups: usize,
    },
    /// Some of the group's corners carry the attribute and some do not, so
    /// the buffer cannot be filled with stored values alone. The group is
    /// refused instead of padded.
    IncompleteAttribute {
        /// The group.
        group: usize,
        /// Which attribute.
        attribute: AttributeKind,
        /// How many of the group's vertices carry it.
        present: usize,
        /// How many vertices the group has.
        total: usize,
    },
}

impl MeshAdapterError {
    /// Stable lowercase identifier, used as an unsupported reason.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::GroupOutOfRange { .. } => "group_out_of_range",
            Self::IncompleteAttribute { .. } => "incomplete_vertex_attribute",
        }
    }
}

impl fmt::Display for MeshAdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GroupOutOfRange { group, groups } => {
                write!(
                    f,
                    "material group {group} is out of range, the mesh has {groups}"
                )
            }
            Self::IncompleteAttribute {
                group,
                attribute,
                present,
                total,
            } => write!(
                f,
                "material group {group} carries a {attribute} on {present} of {total} \
                 vertices; the buffer cannot be filled from stored values alone"
            ),
        }
    }
}

impl std::error::Error for MeshAdapterError {}

/// What one uploaded group turned out to contain.
///
/// Every number is a count of what the buffers hold, so a consumer can report
/// the mesh without re-reading the index buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupReport {
    /// Vertices in the uploaded buffers, after per-group compaction.
    pub vertices: usize,
    /// Triangles uploaded, degenerate ones included.
    pub triangles: usize,
    /// How many of [`Self::triangles`] the IR flagged as degenerate. They
    /// are kept in the index buffer, never dropped.
    pub degenerate_triangles: usize,
    /// Whether the group carries a normal on every vertex.
    pub normals: bool,
    /// Whether the group carries a UV on every vertex.
    pub uvs: bool,
    /// Whether the group carries a color on every vertex. The stored color
    /// has three channels; the fourth component of the uploaded
    /// [`Mesh::ATTRIBUTE_COLOR`] buffer is the declared constant `1.0`,
    /// which is covered by [`MeshPresentationUnknown::VertexColor`].
    pub colors: bool,
}

/// One material group's geometry, as Bevy buffers plus the counts and the
/// presentation unknowns that travel with it.
#[derive(Debug)]
pub struct GroupUpload {
    material: u32,
    group: usize,
    mesh: Mesh,
    report: GroupReport,
    unknowns: Vec<MeshPresentationUnknown>,
    fingerprint: ContentHash,
}

/// Uploads one material group of `render` into a Bevy [`Mesh`], carrying
/// `unknowns` — the presentation questions the content pipeline established
/// are still open — into the upload unchanged.
///
/// # Errors
///
/// [`MeshAdapterError::GroupOutOfRange`] when `group` does not exist, and
/// [`MeshAdapterError::IncompleteAttribute`] when a per-corner attribute is
/// stored for some of the group's vertices but not all.
pub fn upload_group(
    render: &RenderMesh,
    group: usize,
    unknowns: &[MeshPresentationUnknown],
) -> Result<GroupUpload, MeshAdapterError> {
    let source = render
        .groups()
        .get(group)
        .ok_or(MeshAdapterError::GroupOutOfRange {
            group,
            groups: render.groups().len(),
        })?;

    // Compact the group's triangles into their own vertex list. Slots are
    // handed out in first-reached order while walking the group's triangles
    // in stored order, so identical input always yields an identical buffer.
    let vertices = render.vertices();
    let triangles = render.triangles();
    let mut slot_of: HashMap<u32, u32> = HashMap::with_capacity(source.triangles.len() * 3);
    let mut indices = Vec::with_capacity(source.triangles.len() * 3);
    let mut degenerate = 0usize;
    for &triangle_index in &source.triangles {
        let triangle = triangles
            .get(triangle_index)
            .expect("RenderMesh validates its own triangle indices at construction");
        if triangle.degenerate {
            degenerate += 1;
        }
        for &vertex_index in &triangle.vertices {
            let next = u32::try_from(slot_of.len())
                .expect("an index buffer is u32, so its length fits in u32");
            indices.push(*slot_of.entry(vertex_index).or_insert(next));
        }
    }

    // Rebuild the compacted vertex arrays in slot order. `slot_of` maps a
    // render vertex index to the slot it was handed, so invert it: slot `s`
    // holds the vertex index whose values belong at array position `s`. Using
    // the slot numbers themselves as vertex indices is wrong for any group
    // whose render vertices are not exactly the first `total` in order — which
    // is every group but a first-in-stored-order group 0 (measured in
    // `accept_f18_b_every_material_group_of_a_stored_mesh_reaches_the_collider`).
    let total = slot_of.len();
    let mut slots: Vec<u32> = vec![0; total];
    for (&vertex_index, &slot) in &slot_of {
        slots[slot as usize] = vertex_index;
    }
    let mut positions = Vec::with_capacity(total);
    let mut normals = Vec::with_capacity(total);
    let mut uvs = Vec::with_capacity(total);
    let mut colors = Vec::with_capacity(total);
    for &vertex_index in &slots {
        let vertex = vertices
            .get(vertex_index as usize)
            .expect("RenderMesh validates its own vertex indices at construction");
        positions.push(vertex.position);
        if let Some(normal) = vertex.normal {
            normals.push(normal);
        }
        if let Some(uv) = vertex.uv {
            uvs.push(uv);
        }
        if let Some(color) = vertex.color {
            colors.push(color);
        }
    }

    // An attribute is either stored for the whole group or absent from it. A
    // partial one is refused: filling it would invent stored values.
    let has_normals = attribute_is_complete(group, AttributeKind::Normal, normals.len(), total)?;
    let has_uvs = attribute_is_complete(group, AttributeKind::Uv, uvs.len(), total)?;
    let has_colors = attribute_is_complete(group, AttributeKind::Color, colors.len(), total)?;

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions.clone());
    if has_normals {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals.clone());
    }
    if has_uvs {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs.clone());
    }
    if has_colors {
        // `Mesh::ATTRIBUTE_COLOR` is a four-component attribute while the IR
        // stores three channels. The fourth is the declared constant `1.0`:
        // per-corner coverage is not stored, so it is not the surface's
        // coverage (that is `MaterialFacts::coverage`), and `1.0` says "this
        // color contributes no coverage", not "this texel is opaque".
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_COLOR,
            colors
                .iter()
                .map(|color| [color[0], color[1], color[2], 1.0])
                .collect::<Vec<[f32; 4]>>(),
        );
    }
    mesh.insert_indices(Indices::U32(indices.clone()));

    let report = GroupReport {
        vertices: total,
        triangles: source.triangles.len(),
        degenerate_triangles: degenerate,
        normals: has_normals,
        uvs: has_uvs,
        colors: has_colors,
    };
    let fingerprint = geometry_fingerprint(
        source.material,
        &report,
        &positions,
        &normals,
        &uvs,
        &colors,
        &indices,
    );
    Ok(GroupUpload {
        material: source.material,
        group,
        mesh,
        report,
        unknowns: unknowns.to_vec(),
        fingerprint,
    })
}

/// Uploads every material group of `render`, in group order, carrying
/// `unknowns` into each one.
///
/// The first refusal wins and names its group: a mesh whose third group
/// cannot be drawn is never silently uploaded as a two-group mesh.
pub fn upload_groups(
    render: &RenderMesh,
    unknowns: &[MeshPresentationUnknown],
) -> Result<Vec<GroupUpload>, MeshAdapterError> {
    (0..render.groups().len())
        .map(|group| upload_group(render, group, unknowns))
        .collect()
}

/// Whether `stored` means "no vertex has it" (`Ok(false)`) or "every vertex
/// has it" (`Ok(true)`), and refuses the in-between case.
fn attribute_is_complete(
    group: usize,
    attribute: AttributeKind,
    stored: usize,
    total: usize,
) -> Result<bool, MeshAdapterError> {
    match stored {
        0 => Ok(false),
        stored if stored == total => Ok(true),
        stored => Err(MeshAdapterError::IncompleteAttribute {
            group,
            attribute,
            present: stored,
            total,
        }),
    }
}

/// Digests the bit patterns handed to Bevy, so the capture pins the uploaded
/// geometry without depending on Bevy's own buffer layout.
fn geometry_fingerprint(
    material: u32,
    report: &GroupReport,
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    uvs: &[[f32; 2]],
    colors: &[[f32; 3]],
    indices: &[u32],
) -> ContentHash {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"cs/render/bevy_mesh/v1\0");
    bytes.extend_from_slice(&material.to_le_bytes());
    for value in [
        report.vertices as u64,
        report.triangles as u64,
        report.degenerate_triangles as u64,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(u8::from(report.normals));
    bytes.push(u8::from(report.uvs));
    bytes.push(u8::from(report.colors));
    for value in positions.iter().flatten() {
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }
    for value in normals.iter().flatten() {
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }
    for value in uvs.iter().flatten() {
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }
    for value in colors.iter().flatten() {
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }
    for index in indices {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    sha256(&bytes)
}

impl GroupUpload {
    /// The stored material index this group draws.
    pub const fn material(&self) -> u32 {
        self.material
    }

    /// The group's index in [`RenderMesh::groups`].
    pub const fn group(&self) -> usize {
        self.group
    }

    /// The Bevy mesh: one compacted vertex/index buffer pair for this group.
    pub const fn mesh(&self) -> &Mesh {
        &self.mesh
    }

    /// Consumes the upload, returning the Bevy mesh.
    #[must_use]
    pub fn into_mesh(self) -> Mesh {
        self.mesh
    }

    /// What the buffers contain.
    pub const fn report(&self) -> &GroupReport {
        &self.report
    }

    /// The presentation unknowns the caller handed in, carried unchanged.
    /// The upload settles none of them: whether the stored winding is
    /// front-facing and which way `V` runs are still unmeasured.
    pub fn unknowns(&self) -> &[MeshPresentationUnknown] {
        &self.unknowns
    }

    /// A digest of the exact `f32`/`u32` bit patterns handed to Bevy.
    pub const fn fingerprint(&self) -> ContentHash {
        self.fingerprint
    }
}
