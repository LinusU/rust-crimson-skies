//! Canonical render mesh: GameZ vertices split per corner attribute
//! (`specs/F10-gamez-mesh-topology-and-material-records.md`, stage
//! `### F10-C`, slice F10-C.01; shared contract
//! `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! [`RenderMesh`] is the Bevy-free handoff between a GameZ mesh reader
//! (F10-B's [`RawMesh`] plus its [`MeshTopology`]) and the upload adapter.
//! It is built with [`RenderMesh::build`], which computes the topology, or
//! with [`RenderMesh::from_parts`], which takes a topology the caller
//! already has. No Bevy or Avian type appears here.
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
//! `docs/findings/2026-09-29-f10-c-01-render-vertex-splitting.md`.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use cs_formats::gamez::{FaceStatus, MeshTopology, RawMesh};

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
}
