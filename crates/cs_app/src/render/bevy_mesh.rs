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
//! * **No vertex value is changed beyond one declared canonicalisation.**
//!   Positions, normals, UVs and corner colors are copied bit-exact from the
//!   IR. Nothing is normalized, no `V` is flipped, no coordinate is wrapped
//!   or clamped, no color is converted or clamped (`cs_content::mesh::
//!   RenderVertex` already guarantees the stored values are unresolved; the
//!   adapter keeps them that way). The single stated exception is
//!   [`SUBNORMAL_POSITION_CLAIM`]: a **subnormal** stored position component
//!   uploads as the signed zero of its own sign, because the engine consumer
//!   that reads positions — parry's binned BVH builder behind a mesh-derived
//!   collider — cannot bin a subnormal extent (it divides by that
//!   extent, the `f32` quotient overflows and the bin index saturates out of
//!   bounds; measured on c3's stored mesh slot 447, task #656). Every other
//!   stored value — `±0.0` itself, `f32::MIN_POSITIVE`, every normal, UV and
//!   color component — keeps its stored bit pattern, and each flush is
//!   counted into [`GroupReport::subnormal_components`].
//! * **No attribute is fabricated.** A group whose vertices all carry a
//!   normal gets the normal buffer; a group where *no* vertex carries one
//!   gets no buffer at all; a group where *some* do is refused
//!   ([`MeshAdapterError::IncompleteAttribute`]) rather than padded with
//!   zeroes, because a padded normal or UV is an invented value that would
//!   silently change every shaded fragment. The one exception is the
//!   declared [`PARTIAL_NORMAL_POLICY`]: [`upload_groups`] splits a group
//!   whose *polygons* differ in whether they store a normal into two uploads,
//!   each complete-or-absent, so nothing is padded and nothing is dropped.
//! * **No triangle is dropped.** `RenderTriangle::degenerate` is counted into
//!   [`GroupReport::degenerate_triangles`] and the triangles stay in the
//!   index buffer: removing them here would be a presentation decision made
//!   where nothing measured one (spec F17 non-negotiable 4 — culling must
//!   not remove gameplay geometry).
//! * **The stored winding is kept, and it is now a measured front face.**
//!   Static analysis of the owner-supplied decrypted executable settled which
//!   winding the original engine drew (`OWNER-STATIC-2026-10-05`, task #396;
//!   `docs/findings/2026-10-14-f10-d-2-front-face-winding-and-culling.md`):
//!   the front face is the side from which stored corners 0→1→2 run
//!   counter-clockwise, under the identity axis map of a right-handed `+Y`-up
//!   world with no mirror. That is [`FRONT_FACE`], Bevy's own default, so the
//!   adapter still never reverses an index triple — but the winding is no
//!   longer an open question: it is a decision the culling rule below rests
//!   on ([`CullingClass`]). The caller still hands in the
//!   [`MeshPresentationUnknown`] list the content pipeline established
//!   ([`GroupUpload::unknowns`]) and the adapter carries it through
//!   untouched: which *stored surfaces* are two-sided is per-polygon data
//!   (`0x400`) nothing has plumbed yet, so that question stays open on the
//!   content side even though the rule is measured.
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
use bevy::render::render_resource::{Face, FrontFace};
use cs_assets::install::sha256;
use cs_content::mesh::{CollisionMesh, MeshPresentationUnknown, RenderMesh};
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

/// What the presentation does with a material group in which some polygons
/// store normals and some do not.
///
/// The GameZ layout stores normal indices **per polygon**, behind the
/// polygon's own `NORMALS` flag bit (`docs/findings/2026-09-29-f10-b-gamez-
/// mesh-layout.md`), so a material group that mixes both kinds of polygon has a
/// vertex set with a normal on some vertices and none on the others. A render
/// vertex is keyed on its stored normal *index*, so a vertex never belongs to
/// both kinds of polygon: the split below is exact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartialNormalPolicy {
    /// One upload for the polygons that store normals
    /// ([`GroupPart::NormalBearing`]) and one for those that store none
    /// ([`GroupPart::NormalFree`]). Every stored value is uploaded unchanged,
    /// no normal is invented and no stored normal is dropped.
    SplitByStoredPresence,
}

/// How far the original's treatment of a normal-free polygon is known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginalNormalFreeBehavior {
    /// Unmeasured: whether the original drew such a polygon unlit, flat shaded,
    /// lit from a computed normal or as an overlay is **not** established (no
    /// original run, no decompiled renderer path, and the pinned mech3ax
    /// reference is silent on it), so the split settles the *data* and leaves
    /// the *shading* of [`GroupPart::NormalFree`] open.
    Unmeasured,
}

/// The declared policy for a group with partly stored normals.
pub const PARTIAL_NORMAL_POLICY: PartialNormalPolicy = PartialNormalPolicy::SplitByStoredPresence;

/// What is known about the original's behavior for the same case. A reader of
/// the policy must read this next to it: the policy is an engineering decision
/// about lossless upload, not a claim about the original.
pub const ORIGINAL_NORMAL_FREE_BEHAVIOR: OriginalNormalFreeBehavior =
    OriginalNormalFreeBehavior::Unmeasured;

/// The owner evidence the winding and culling decisions below are measured
/// from: the owner note `OWNER-STATIC-2026-10-05` on task #396 — static
/// analysis of the decrypted original executable (`crimson.decrypted.exe`,
/// sha256 `43540fc9…6c37d75`), cross-checked on all nine GameZ archives and
/// accepted by the owner in place of a capture.
///
/// Claim class: static code evidence, never `verified_original`. The full
/// decision, its addresses and its retail counts are recorded in
/// `docs/findings/2026-10-14-f10-d-2-front-face-winding-and-culling.md`.
pub const WINDING_EVIDENCE: &str = "OWNER-STATIC-2026-10-05";

/// The measured front-face winding: counter-clockwise is the front side.
///
/// `OWNER-STATIC-2026-10-05` (`WINDING_EVIDENCE`): the front face is the
/// side from which stored corners 0→1→2 run counter-clockwise — the
/// right-hand normal of `(P1−P0)×(P2−P0)` points at the viewer — in a
/// right-handed `+Y`-up world reached through the **identity axis map**, with
/// no mirror between content and screen. `FrontFace::Ccw` is exactly that
/// convention, and it is also Bevy's default, so this constant is both the
/// measured claim and the value the engine's own mesh pipeline already uses;
/// the adapter uploads stored positions and corner order unchanged and never
/// reverses an index triple.
pub const FRONT_FACE: FrontFace = FrontFace::Ccw;

/// Which culling rule one surface draws under, after the measured winding.
///
/// The rule itself is measured (`WINDING_EVIDENCE`): the original
/// back-face-culls one-sided polygons, draws the back side of a two-sided
/// (`0x400`) polygon with the negated normal, and skips the test entirely
/// for billboard meshes. Which stored surface is two-sided or a billboard is
/// per-polygon / per-mesh stored data. The mesh type words are plumbed
/// (`RenderMesh::is_unculled_billboard`, selected by
/// `bevy_state::render_state_for_mesh`); the per-polygon `0x400` flag is not
/// (F10-F.1), so a producer still declares two-sidedness.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CullingClass {
    /// A one-sided surface: the back face is culled. With [`FRONT_FACE`],
    /// that is `cull_mode: Some(Face::Back)` — the original's
    /// per-polygon test (`d < −tol` on the first three stored corners)
    /// culled exactly the back faces of a triangle list.
    OneSided,
    /// A two-sided surface (polygon flag `0x400`, on 52 621 retail outline
    /// polygons and 0 retail strips): both sides are drawn, so nothing is
    /// culled. The original drew the back side with the face normal
    /// negated; that lighting path is not replicated here and stays an
    /// open follow-up recorded in the finding.
    TwoSided,
    /// A camera-facing billboard mesh (mesh word0 == 1, word1 ∈ {0, 1};
    /// 1 340 such retail meshes): the original skipped the back-face test
    /// for the whole mesh, so nothing is culled. Word1 == 2 billboards (40
    /// retail meshes) do *not* skip the test in the original: they draw
    /// under the normal per-polygon rule, so whether one of their polygons
    /// is two-sided is their own unread `0x400` data (F10-F.1) — this
    /// adapter makes no assumption about them.
    Billboard,
}

impl CullingClass {
    /// The cull face this class draws with, `None` when nothing is culled.
    ///
    /// Paired with [`FRONT_FACE`]: `Some(Face::Back)` means "counter-clockwise
    /// front faces are kept, clockwise ones culled", exactly the side the
    /// original's test kept.
    pub const fn cull_face(self) -> Option<Face> {
        match self {
            Self::OneSided => Some(Face::Back),
            Self::TwoSided | Self::Billboard => None,
        }
    }
}

/// The claim id of the adapter's one canonicalisation: a stored **position**
/// component that is subnormal uploads as the signed zero of its own sign.
///
/// # The rule
///
/// For each `f32` component of a stored position: `value.is_subnormal()`
/// uploads as `0.0f32.copysign(value)` — `0x0000_0003` as `+0.0`,
/// `0x8000_0006` as `-0.0` — and every value that is not subnormal uploads
/// with its stored bit pattern unchanged. `±0.0`, `f32::MIN_POSITIVE` (the
/// smallest *normal* `f32`), NaN and infinity all pass through verbatim; only
/// the subnormal magnitude is discarded, never the sign bit — the same
/// canonicalisation hardware flush-to-zero performs. Normals, UVs and corner
/// colors are **not** covered: no measured consumer of them needs it, so they
/// stay bit-exact.
///
/// # Why, argued from the corpus
///
/// `ZBD/C3/gamez.zbd` mesh slot 447 stores two position `y` components as
/// subnormals — `4e-45` and `-8e-45` — on a plane that is otherwise exactly
/// `y = 0` (`docs/findings/2026-10-05-f18-world-units-containers.md`, task
/// #656). The faithful reader decodes them verbatim, the collision view
/// [`ColliderMesh`] canonicalises them at this same boundary before Avian
/// hands them to parry, and parry 0.27's binned BVH builder computes
/// `NUM_BINS * (1 - eps) / (centroid_extent)` — a division that overflows to
/// `inf` when the extent is subnormal, so the bin index saturates to
/// `usize::MAX` and the 8-entry bin array is indexed out of bounds
/// (`bvh_binned_build.rs`). An exactly-zero extent does not panic
/// (`inf * 0` is `NaN`, which casts to `0`), which is why flushing — not a
/// wider rewrite — is the narrow rule the corpus supports. `rebuild_bvh`
/// hardcodes `BvhBuildStrategy::Binned`, so no `TriMeshFlags` combination
/// avoids that path on this version.
///
/// A subnormal position is below `2^-126` of a stored unit — below the
/// resolution of any representable distance — so this is an
/// **engine-compatibility canonicalisation at the upload boundary**, stated
/// and counted ([`GroupReport::subnormal_components`]), not a claim about
/// what the original engine did with these bytes: that is unmeasured.
pub const SUBNORMAL_POSITION_CLAIM: &str = "f17-b.subnormal-position-flushes-to-zero";

/// Which part of a material group an upload holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupPart {
    /// The whole group: its normals are complete or absent.
    Whole,
    /// Only the group's polygons that store a normal on every corner.
    NormalBearing,
    /// Only the group's polygons that store no normal.
    NormalFree,
}

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
    /// How many stored position **components** the
    /// [`SUBNORMAL_POSITION_CLAIM`] canonicalisation uploaded as a signed
    /// zero. `0` means every stored component reached the buffer with its
    /// own bit pattern; anything else is the rule's measured footprint, not
    /// an estimate.
    pub subnormal_components: usize,
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

/// The collision view of one mesh as the physics backend consumes it.
///
/// The input is [`cs_content::mesh::CollisionMesh`] — the canonical
/// [`cs_content::mesh::RenderMesh::collision`] view, whose positions are the
/// **stored** values the reader decoded. This adapter applies the one
/// canonicalisation the engine's physics consumer needs
/// ([`SUBNORMAL_POSITION_CLAIM`]) and hands back the pair a triangle-mesh
/// collider is built from, so a collider sees exactly the values
/// `TrimeshFromMesh` would have read out of the drawn upload — subnormal
/// position flush included — while indexing a different set of triangles.
///
/// A collider built from this never shares an index buffer with the drawn
/// mesh (task #1424): render fidelity and collision geometry are two views of
/// one [`cs_content::mesh::RenderMesh`], not one buffer with two consumers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColliderMesh {
    vertices: Vec<[f32; 3]>,
    indices: Vec<[u32; 3]>,
}

impl ColliderMesh {
    /// Builds the view directly from its two buffers, canonicalised exactly as
    /// [`Self::of`] does.
    ///
    /// This is the entry point for a caller that authors its geometry instead
    /// of reading it — a synthetic fixture, or a test that needs a collider for
    /// a [`Mesh`] it wrote by hand. The pair is stated together so a caller
    /// cannot hand in vertices its indices do not name: they are one view, and
    /// [`Self::of`] is the same pair read out of a stored mesh.
    #[must_use]
    pub fn new(vertices: Vec<[f32; 3]>, indices: Vec<[u32; 3]>) -> Self {
        Self {
            vertices: vertices
                .iter()
                .map(|position| position.map(canonicalise_position_component))
                .collect(),
            indices,
        }
    }

    /// Canonicalises `collision` for the physics backend.
    ///
    /// Positions are copied through [`canonicalise_position_component`], the
    /// same rule the drawn upload applies, so both views of one mesh agree
    /// bit-for-bit on every position they share — including on the subnormal
    /// extent that would otherwise overflow parry's binned BVH builder
    /// ([`SUBNORMAL_POSITION_CLAIM`]).
    #[must_use]
    pub fn of(collision: &CollisionMesh) -> Self {
        Self {
            vertices: collision
                .positions()
                .iter()
                .map(|position| position.map(canonicalise_position_component))
                .collect(),
            indices: collision.indices().to_vec(),
        }
    }

    /// The vertices the index triples name, in first-reached stored order.
    #[must_use]
    pub fn vertices(&self) -> &[[f32; 3]] {
        &self.vertices
    }

    /// The index triples of the canonical topology.
    #[must_use]
    pub fn indices(&self) -> &[[u32; 3]] {
        &self.indices
    }

    /// Triangles this view carries, degenerate ones included.
    #[must_use]
    pub fn triangles(&self) -> usize {
        self.indices.len()
    }

    /// Whether this view carries no triangle at all, so no collider can be
    /// derived from it (see `SkipReason::CollisionTopologyUnavailable`).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

/// One material group's geometry, as Bevy buffers plus the counts and the
/// presentation unknowns that travel with it.
#[derive(Debug)]
pub struct GroupUpload {
    material: u32,
    group: usize,
    part: GroupPart,
    mesh: Mesh,
    report: GroupReport,
    unknowns: Vec<MeshPresentationUnknown>,
    fingerprint: ContentHash,
    collision: ColliderMesh,
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
    build_upload(
        render,
        group,
        source.material,
        &source.triangles,
        GroupPart::Whole,
        unknowns,
    )
}

/// Uploads one material group under [`PARTIAL_NORMAL_POLICY`]: one upload when
/// the group's normals are complete or absent, two (normal-bearing first) when
/// its polygons differ in whether they store a normal.
///
/// # Errors
///
/// As [`upload_group`], except that a group whose polygons *consistently* store
/// or omit normals is no longer refused. A single triangle whose own corners
/// mix stored and absent normals still is, as is a partial UV or color set.
pub fn upload_group_parts(
    render: &RenderMesh,
    group: usize,
    unknowns: &[MeshPresentationUnknown],
) -> Result<Vec<GroupUpload>, MeshAdapterError> {
    let source = render
        .groups()
        .get(group)
        .ok_or(MeshAdapterError::GroupOutOfRange {
            group,
            groups: render.groups().len(),
        })?;
    let (mut bearing, mut free) = (Vec::new(), Vec::new());
    for &triangle_index in &source.triangles {
        let triangle = &render.triangles()[triangle_index];
        let stored = triangle
            .vertices
            .iter()
            .filter(|&&vertex| render.vertices()[vertex as usize].normal.is_some())
            .count();
        match stored {
            0 => free.push(triangle_index),
            3 => bearing.push(triangle_index),
            // A triangle with its own corners split: no partition is exact, so
            // the strict adapter reports it.
            _ => return upload_group(render, group, unknowns).map(|upload| vec![upload]),
        }
    }
    if bearing.is_empty() || free.is_empty() {
        return upload_group(render, group, unknowns).map(|upload| vec![upload]);
    }
    [
        (GroupPart::NormalBearing, bearing),
        (GroupPart::NormalFree, free),
    ]
    .into_iter()
    .map(|(part, triangles)| {
        build_upload(render, group, source.material, &triangles, part, unknowns)
    })
    .collect()
}

fn build_upload(
    render: &RenderMesh,
    group: usize,
    material: u32,
    source_triangles: &[usize],
    part: GroupPart,
    unknowns: &[MeshPresentationUnknown],
) -> Result<GroupUpload, MeshAdapterError> {
    // Compact the group's triangles into their own vertex list. Slots are
    // handed out in first-reached order while walking the group's triangles
    // in stored order, so identical input always yields an identical buffer.
    let vertices = render.vertices();
    let triangles = render.triangles();
    let mut slot_of: HashMap<u32, u32> = HashMap::with_capacity(source_triangles.len() * 3);
    let mut indices = Vec::with_capacity(source_triangles.len() * 3);
    let mut degenerate = 0usize;
    for &triangle_index in source_triangles {
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
    let mut subnormal_components = 0usize;
    for &vertex_index in &slots {
        let vertex = vertices
            .get(vertex_index as usize)
            .expect("RenderMesh validates its own vertex indices at construction");
        subnormal_components += vertex
            .position
            .iter()
            .filter(|component| component.is_subnormal())
            .count();
        positions.push(vertex.position.map(canonicalise_position_component));
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
        triangles: source_triangles.len(),
        degenerate_triangles: degenerate,
        subnormal_components,
        normals: has_normals,
        uvs: has_uvs,
        colors: has_colors,
    };
    // The material, then a part tag; a whole group keeps the digest it always
    // had, a part names itself.
    let mut identity = material.to_le_bytes().to_vec();
    match part {
        GroupPart::Whole => {}
        GroupPart::NormalBearing => identity.extend_from_slice(b"part:normal\0"),
        GroupPart::NormalFree => identity.extend_from_slice(b"part:no-normal\0"),
    }
    let fingerprint = geometry_fingerprint(
        &identity, &report, &positions, &normals, &uvs, &colors, &indices,
    );
    Ok(GroupUpload {
        material,
        group,
        part,
        mesh,
        report,
        unknowns: unknowns.to_vec(),
        fingerprint,
        collision: ColliderMesh::of(render.collision()),
    })
}

/// Uploads every material group of `render`, in group order, carrying
/// `unknowns` into each one, under [`PARTIAL_NORMAL_POLICY`] (a group that
/// splits contributes two uploads sharing one [`GroupUpload::group`]).
///
/// The first refusal wins and names its group: a mesh whose third group
/// cannot be drawn is never silently uploaded as a two-group mesh.
pub fn upload_groups(
    render: &RenderMesh,
    unknowns: &[MeshPresentationUnknown],
) -> Result<Vec<GroupUpload>, MeshAdapterError> {
    let mut uploads = Vec::new();
    for group in 0..render.groups().len() {
        uploads.extend(upload_group_parts(render, group, unknowns)?);
    }
    Ok(uploads)
}

/// `component` as uploaded under [`SUBNORMAL_POSITION_CLAIM`]: the signed zero
/// of its own sign when it is subnormal, its stored bit pattern otherwise.
fn canonicalise_position_component(component: f32) -> f32 {
    if component.is_subnormal() {
        0.0f32.copysign(component)
    } else {
        component
    }
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
    identity: &[u8],
    report: &GroupReport,
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    uvs: &[[f32; 2]],
    colors: &[[f32; 3]],
    indices: &[u32],
) -> ContentHash {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"cs/render/bevy_mesh/v1\0");
    bytes.extend_from_slice(identity);
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

    /// Which part of the group this upload holds.
    pub const fn part(&self) -> GroupPart {
        self.part
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
    /// The upload settles none of them *in the list*: which way `V` runs is
    /// still unmeasured, and although the winding is now measured
    /// ([`FRONT_FACE`], [`WINDING_EVIDENCE`]), which stored surfaces are
    /// two-sided (`0x400`) is per-polygon data no content-side reader has
    /// interpreted yet, so the content pipeline's list still travels as
    /// established.
    pub fn unknowns(&self) -> &[MeshPresentationUnknown] {
        &self.unknowns
    }

    /// A digest of the exact `f32`/`u32` bit patterns handed to Bevy.
    pub const fn fingerprint(&self) -> ContentHash {
        self.fingerprint
    }

    /// The **mesh-level** collision view this group was uploaded alongside.
    ///
    /// It is not this group's geometry: every group of one [`RenderMesh`]
    /// carries the same value, because the collision view is a property of the
    /// stored mesh, not of a material split. It travels here so a consumer
    /// handed only group uploads — [`crate::world::meshes::WorldMesh`] — can
    /// build a collider without a second handle on the render mesh, and the
    /// first group's copy is the one that consumer takes.
    pub const fn collision(&self) -> &ColliderMesh {
        &self.collision
    }
}
