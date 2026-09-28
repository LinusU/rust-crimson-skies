//! The lossless raw mesh IR and its topology report.
//!
//! [`RawMesh`] is what a GameZ mesh reader produces: the stored arrays and
//! polygons with their raw indices, raw flags and per-corner attributes,
//! nothing merged, reordered or cleaned up. [`RawMesh::topology`] turns it
//! into source-mapped triangles and a per-polygon status, so every broken
//! or unsupported face is counted and named instead of being dropped.
//!
//! The IR is a new-engine design (claim class *Designed*): no Crimson Skies
//! mesh layout is parsed yet (stage F10-B), so nothing here states which
//! stored field feeds which IR field.

use std::fmt;

use super::strip::{StripError, StripTriangle, decode_strip};

/// How a polygon's corners form triangles. Which stored flag selects which
/// kind is a variant fact the reader has to establish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveKind {
    /// Corners in triangle-strip order ([`decode_strip`]).
    TriangleStrip,
    /// One polygon outline. Three corners are one triangle; more need the
    /// validated triangulation of stage F10-B.
    Polygon,
}

/// One polygon corner as stored. Attributes are per corner, not per
/// position: two corners sharing a position index may differ in every
/// other attribute, and the IR keeps both.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawCorner {
    /// Index into [`RawMesh::positions`], unchanged.
    pub position: u32,
    /// Index into [`RawMesh::normals`], unchanged, if the polygon has one.
    pub normal: Option<u32>,
    /// Texture coordinate as stored (no V flip, no wrap).
    pub uv: Option<[f32; 2]>,
    /// Corner color as stored (no clamp, no color-space change).
    pub color: Option<[f32; 3]>,
}

/// One stored polygon.
#[derive(Debug, Clone, PartialEq)]
pub struct RawPolygon {
    /// Corner topology.
    pub kind: PrimitiveKind,
    /// Stored flag bits, unchanged. Their meaning is unknown until a variant
    /// establishes it; no bit is interpreted here.
    pub raw_flags: u32,
    /// Stored material index, unchanged. The material record layout is
    /// unknown; this is a reference, not a resolved material.
    pub material: u32,
    /// Corners in stored order.
    pub corners: Vec<RawCorner>,
}

/// One mesh as stored.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RawMesh {
    /// Vertex positions in stored order and stored units.
    pub positions: Vec<[f32; 3]>,
    /// Vertex normals in stored order.
    pub normals: Vec<[f32; 3]>,
    /// Polygons in stored order.
    pub polygons: Vec<RawPolygon>,
}

/// Why a polygon produced no triangles.
#[derive(Debug, Clone, PartialEq)]
pub enum FaceIssue {
    /// Fewer corners than the primitive kind needs.
    TooFewCorners {
        /// Primitive kind of the polygon.
        kind: PrimitiveKind,
        /// Corners stored.
        corners: usize,
    },
    /// A corner's position index is past [`RawMesh::positions`].
    PositionIndexOutOfRange {
        /// Corner in stored order.
        corner: usize,
        /// Stored index.
        index: u32,
        /// Positions available.
        positions: usize,
    },
    /// A corner's normal index is past [`RawMesh::normals`].
    NormalIndexOutOfRange {
        /// Corner in stored order.
        corner: usize,
        /// Stored index.
        index: u32,
        /// Normals available.
        normals: usize,
    },
    /// A value a corner uses is NaN or infinite.
    NonFinite {
        /// Corner in stored order.
        corner: usize,
        /// `"position"`, `"normal"`, `"uv"` or `"color"`.
        attribute: &'static str,
    },
    /// A polygon with more than three corners: its triangulation must be
    /// validated (stage F10-B), not fanned.
    UnsupportedNgon {
        /// Corners stored.
        corners: usize,
    },
}

impl FaceIssue {
    /// Stable machine-matchable identifier.
    pub fn code(&self) -> &'static str {
        match self {
            Self::TooFewCorners { .. } => "too_few_corners",
            Self::PositionIndexOutOfRange { .. } => "position_index_out_of_range",
            Self::NormalIndexOutOfRange { .. } => "normal_index_out_of_range",
            Self::NonFinite { .. } => "non_finite_attribute",
            Self::UnsupportedNgon { .. } => "unsupported_ngon",
        }
    }

    /// The geometry is well-formed but this stage cannot triangulate it.
    /// Every other issue means the stored face is invalid.
    pub fn is_unsupported(&self) -> bool {
        matches!(self, Self::UnsupportedNgon { .. })
    }
}

impl fmt::Display for FaceIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            Self::TooFewCorners { kind, corners } => {
                write!(f, "{code}: {kind:?} with {corners} corners")
            }
            Self::PositionIndexOutOfRange {
                corner,
                index,
                positions,
            } => write!(
                f,
                "{code}: corner {corner} uses position {index} of {positions}"
            ),
            Self::NormalIndexOutOfRange {
                corner,
                index,
                normals,
            } => write!(
                f,
                "{code}: corner {corner} uses normal {index} of {normals}"
            ),
            Self::NonFinite { corner, attribute } => {
                write!(f, "{code}: corner {corner} has a non-finite {attribute}")
            }
            Self::UnsupportedNgon { corners } => {
                write!(f, "{code}: polygon with {corners} corners")
            }
        }
    }
}

/// Outcome for one stored polygon.
#[derive(Debug, Clone, PartialEq)]
pub enum FaceStatus {
    /// Triangles were produced; `degenerate` of them draw nothing.
    Decoded {
        /// Triangles produced, degenerate ones included.
        triangles: usize,
        /// Of those, triangles with two equal position indices.
        degenerate: usize,
    },
    /// The polygon produced no triangles because of this issue.
    Rejected(FaceIssue),
}

/// One triangle with its source-corner map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshTriangle {
    /// Polygon in stored order.
    pub polygon: usize,
    /// Strip step for strips, 0 for a three-corner polygon.
    pub step: usize,
    /// Corners of that polygon, in drawing order.
    pub corners: [usize; 3],
    /// Their position indices, in drawing order.
    pub positions: [u32; 3],
}

impl MeshTriangle {
    /// Two of the three position indices are equal.
    pub fn is_degenerate(&self) -> bool {
        let [a, b, c] = self.positions;
        a == b || b == c || a == c
    }
}

/// Triangles and per-polygon status for one [`RawMesh`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MeshTopology {
    /// Every triangle of every decoded polygon, degenerate ones included,
    /// in polygon then step order.
    pub triangles: Vec<MeshTriangle>,
    /// One status per stored polygon, same order as [`RawMesh::polygons`].
    pub faces: Vec<FaceStatus>,
}

impl MeshTopology {
    /// Polygons that produced triangles.
    pub fn decoded_faces(&self) -> usize {
        self.faces
            .iter()
            .filter(|face| matches!(face, FaceStatus::Decoded { .. }))
            .count()
    }

    /// Polygons rejected as invalid stored geometry.
    pub fn invalid_faces(&self) -> usize {
        self.rejected()
            .filter(|issue| !issue.is_unsupported())
            .count()
    }

    /// Polygons this stage cannot triangulate yet.
    pub fn unsupported_faces(&self) -> usize {
        self.rejected()
            .filter(|issue| issue.is_unsupported())
            .count()
    }

    /// Every polygon was decoded: the mesh may be uploaded.
    pub fn is_complete(&self) -> bool {
        self.decoded_faces() == self.faces.len()
    }

    /// Triangles that draw something.
    pub fn drawn_triangles(&self) -> impl Iterator<Item = &MeshTriangle> {
        self.triangles.iter().filter(|t| !t.is_degenerate())
    }

    fn rejected(&self) -> impl Iterator<Item = &FaceIssue> {
        self.faces.iter().filter_map(|face| match face {
            FaceStatus::Rejected(issue) => Some(issue),
            FaceStatus::Decoded { .. } => None,
        })
    }
}

impl RawMesh {
    /// Validates every polygon's indices and values, then decodes its
    /// triangles. A bad polygon is reported in [`MeshTopology::faces`] and
    /// the remaining polygons are still decoded.
    pub fn topology(&self) -> MeshTopology {
        let mut topology = MeshTopology::default();
        for (polygon, face) in self.polygons.iter().enumerate() {
            let status = match self.face_triangles(face) {
                Ok(triangles) => {
                    let before = topology.triangles.len();
                    topology
                        .triangles
                        .extend(triangles.into_iter().map(|t| MeshTriangle {
                            polygon,
                            step: t.step,
                            corners: t.corners,
                            positions: t.indices,
                        }));
                    let added = &topology.triangles[before..];
                    FaceStatus::Decoded {
                        triangles: added.len(),
                        degenerate: added.iter().filter(|t| t.is_degenerate()).count(),
                    }
                }
                Err(issue) => FaceStatus::Rejected(issue),
            };
            topology.faces.push(status);
        }
        topology
    }

    fn face_triangles(&self, face: &RawPolygon) -> Result<Vec<StripTriangle>, FaceIssue> {
        let corners = face.corners.len();
        let positions: Vec<u32> = face.corners.iter().map(|c| c.position).collect();
        // A three-corner polygon is a one-step strip: same corners, same
        // order.
        let triangles = decode_strip(&positions).map_err(|error| match error {
            StripError::TooShort { .. } => FaceIssue::TooFewCorners {
                kind: face.kind,
                corners,
            },
        })?;
        for (corner, raw) in face.corners.iter().enumerate() {
            self.check_corner(corner, raw)?;
        }
        if face.kind == PrimitiveKind::Polygon && corners > 3 {
            return Err(FaceIssue::UnsupportedNgon { corners });
        }
        Ok(triangles)
    }

    fn check_corner(&self, corner: usize, raw: &RawCorner) -> Result<(), FaceIssue> {
        let non_finite = |attribute| FaceIssue::NonFinite { corner, attribute };
        let position = usize::try_from(raw.position)
            .ok()
            .and_then(|index| self.positions.get(index))
            .ok_or(FaceIssue::PositionIndexOutOfRange {
                corner,
                index: raw.position,
                positions: self.positions.len(),
            })?;
        if !all_finite(position) {
            return Err(non_finite("position"));
        }
        if let Some(index) = raw.normal {
            let normal = usize::try_from(index)
                .ok()
                .and_then(|i| self.normals.get(i))
                .ok_or(FaceIssue::NormalIndexOutOfRange {
                    corner,
                    index,
                    normals: self.normals.len(),
                })?;
            if !all_finite(normal) {
                return Err(non_finite("normal"));
            }
        }
        if raw.uv.is_some_and(|uv| !all_finite(&uv)) {
            return Err(non_finite("uv"));
        }
        if raw.color.is_some_and(|color| !all_finite(&color)) {
            return Err(non_finite("color"));
        }
        Ok(())
    }
}

fn all_finite(values: &[f32]) -> bool {
    values.iter().all(|value| value.is_finite())
}
