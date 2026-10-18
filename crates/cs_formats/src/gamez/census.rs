//! The exact face census of one GameZ container: F10-D's AC04 report,
//! reconciled with the original renderer's measured behaviour in F10-D.1.
//!
//! `specs/F10-gamez-mesh-topology-and-material-records.md` AC04 asks for
//! "exact missing/invalid face counts for every private world and airframe".
//! [`FaceCensus`] is the production answer: it walks one parsed container
//! through [`RawMesh::draw_topology`] — the same measured rule the upload
//! path runs — and accounts for **every** face the records declare, with
//! nothing dropped and nothing counted twice.
//!
//! The words mean exactly this, and the report states all of them so no
//! reader has to guess which one a number refers to:
//!
//! * **declared** — `polygon_count` summed over the present mesh records.
//!   It is what the stored records say the container holds.
//! * **stored** — polygon records actually read. The reader reads exactly
//!   `polygon_count` of them, so for a container that read these two are
//!   equal; their difference ([`FaceCensus::shortfall_faces`]) is the count
//!   of faces the records declared and the section did not hold.
//! * **invalid** — stored faces whose data fails validation: an index past
//!   its array, fewer corners than the primitive needs, a non-finite value.
//!   The stored face is broken ([`FaceIssue::is_unsupported`] is `false`).
//! * **unsupported** — stored faces that are valid data the **canonical**
//!   view ([`RawMesh::topology`]) refuses to guess at: an n-gon outline it
//!   cannot ear-clip. The original draws every one of them — it fans the
//!   stored corners — so under the measured draw rule this count is zero and
//!   the field exists to say so.
//! * **culled** — stored outline faces the original's per-frame face test
//!   removes: the first three corners have a zero normal, so `|d| ≤ tol`
//!   always and the fan is never submitted, two-sided flag or not. Measured
//!   on `OWNER-STATIC-2026-10-05`; accounted per face, never a reason to
//!   drop the mesh.
//! * **missing** — stored (or declared) faces that reach **no drawable
//!   triangle**: the shortfall, the rejected faces, the culled faces, and
//!   the faces that decoded but whose triangles are all degenerate.
//!   `invalid` is a subset of `missing`: a broken face is certainly not
//!   drawn.
//! * **two-sided** — stored polygons that carry the measured two-sided flag
//!   `0x400` (`OWNER-STATIC-2026-10-05`): [`FaceCensus::two_sided_outline_faces`]
//!   for outlines and [`FaceCensus::strip_two_sided_flag_faces`] for strips,
//!   which are reported rather than trusted. These are stored facts counted
//!   whatever a face's draw outcome, so they are not part of the
//!   `decoded + invalid + unsupported + culled == stored` sum.
//!
//! Every missing face is listed with its container mesh index, its stored
//! polygon index and a stable reason code, so the count can always be traced
//! back to the faces that produced it.

use super::mesh::FaceStatus;
use super::reader::GameZMeshes;

/// Why one stored face draws nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingFaceReason {
    /// The stored data is invalid: this is the [`super::mesh::FaceIssue`]
    /// code of the rejection (`"too_few_corners"`,
    /// `"position_index_out_of_range"`, `"normal_index_out_of_range"`,
    /// `"non_finite_attribute"`).
    Invalid(&'static str),
    /// The stored data is valid but no outline this engine can triangulate
    /// was found: the [`super::polygon::NgonIssue`] code of the refusal
    /// (`"coincident_corners"`, `"zero_area"`, `"self_intersecting"`,
    /// `"no_ear"`).
    Unsupported(&'static str),
    /// The face decoded, but every triangle it produced has two equal
    /// position indices, so it draws nothing.
    DegenerateOnly,
    /// The original's per-frame face test culls the outline on every frame:
    /// its first three stored corners have a zero normal, so the measured
    /// `|d| ≤ tol` always holds and the fan is never submitted — under the
    /// two-sided flag too, and whatever the camera. `OWNER-STATIC-2026-10-05`.
    Culled,
}

impl MissingFaceReason {
    /// Stable machine-matchable identifier of the reason.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Invalid(code) => code,
            Self::Unsupported(code) => code,
            Self::DegenerateOnly => "degenerate_only",
            Self::Culled => "culled_zero_normal",
        }
    }

    /// Whether the stored face itself is broken, as opposed to being valid
    /// data the canonical view refuses to triangulate, a face that decodes
    /// to nothing, or a face the measured test culls.
    pub const fn is_invalid(self) -> bool {
        matches!(self, Self::Invalid(_))
    }
}

/// One stored face that reaches no drawable triangle, with where it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MissingFace {
    /// Array index of the mesh in the container (a scene node's `mesh_index`).
    pub mesh: u32,
    /// Polygon index in that mesh's stored order.
    pub polygon: usize,
    /// Corners the stored polygon has.
    pub corners: usize,
    /// Why it draws nothing.
    pub reason: MissingFaceReason,
}

/// The exact face accounting of one parsed container.
///
/// Built with [`FaceCensus::of`] (or [`GameZMeshes::face_census`]). Every
/// field is a count over stored records; nothing is estimated and nothing is
/// dropped, so `decoded + invalid + unsupported + culled` always equals
/// `stored` and `missing_faces` is always the size of the story the numbers
/// tell.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FaceCensus {
    /// Array slots the container has, absent stubs included.
    pub slots: usize,
    /// Slots that stored a mesh.
    pub present_meshes: usize,
    /// Slots that are all-zero stubs: no geometry at all.
    pub absent_meshes: usize,
    /// `polygon_count` summed over the present mesh records.
    pub declared_faces: u64,
    /// Polygon records actually read.
    pub stored_faces: u64,
    /// Declared faces the section did not hold, summed per mesh (never
    /// negative: a mesh that stored more than it declared cannot make another
    /// mesh's shortfall disappear).
    pub shortfall_faces: u64,
    /// Stored faces that produced triangles.
    pub decoded_faces: u64,
    /// Stored faces rejected because their data is invalid.
    pub invalid_faces: u64,
    /// Stored faces rejected because they are valid data this view refuses.
    /// Under [`RawMesh::draw_topology`] — which is what this census walks —
    /// the original's fan accepts every outline the canonical ear-clipper
    /// refused, so this counts only what both views refuse: nothing, unless
    /// a future face kind needs it.
    pub unsupported_faces: u64,
    /// Stored outline faces the original's face test culls on every frame
    /// (zero normal over the first three corners). Accounted, never drawn,
    /// never a reason to drop the mesh.
    pub culled_faces: u64,
    /// Stored faces that decoded but whose triangles are all degenerate.
    pub degenerate_only_faces: u64,
    /// Triangles produced, degenerate ones included.
    pub triangles: u64,
    /// Of those, triangles with two equal position indices.
    pub degenerate_triangles: u64,
    /// Stored **outline** polygons that carry the measured two-sided flag
    /// (`OWNER-STATIC-2026-10-05`): the original draws their back side too,
    /// with the face normal negated. Counted whether or not the face decodes —
    /// the flag is a stored fact, not a draw outcome. Retail: 52 621 of the
    /// nine archives' outline polygons.
    pub two_sided_outline_faces: u64,
    /// Stored **strip** polygons that carry the two-sided flag. The flag's
    /// meaning is measured for outlines only and no retail strip stores it, so
    /// this count is reported rather than trusted; it is 0 on the measured
    /// corpus. The reader also raises
    /// [`super::reader::ParseFinding::StripTwoSidedFlag`] for each such face.
    pub strip_two_sided_flag_faces: u64,
    /// Every face that draws nothing, in mesh then polygon order.
    pub missing: Vec<MissingFace>,
}

impl FaceCensus {
    /// The census of one parsed container.
    pub fn of(meshes: &GameZMeshes) -> Self {
        let mut census = FaceCensus {
            slots: meshes.meshes.len(),
            ..FaceCensus::default()
        };
        for slot in &meshes.meshes {
            let Some(mesh) = slot else {
                census.absent_meshes += 1;
                continue;
            };
            census.present_meshes += 1;
            census.declared_faces += u64::from(mesh.info.polygon_count);

            let stored = mesh.mesh.polygons.len();
            census.stored_faces += stored as u64;
            // Per mesh, so one mesh's surplus can never hide another's
            // shortfall.
            census.shortfall_faces +=
                u64::from(mesh.info.polygon_count).saturating_sub(stored as u64);

            // The measured two-sided flag is a stored fact, counted here
            // whatever the face's draw outcome: an outline that carries it is
            // drawn from both sides, and a strip that carries it is reported,
            // not trusted.
            for polygon in &mesh.mesh.polygons {
                if !polygon.is_two_sided() {
                    continue;
                }
                match polygon.kind {
                    super::mesh::PrimitiveKind::Polygon => census.two_sided_outline_faces += 1,
                    super::mesh::PrimitiveKind::TriangleStrip => {
                        census.strip_two_sided_flag_faces += 1;
                    }
                }
            }

            let topology = mesh.draw_topology();
            census.triangles += topology.triangles.len() as u64;

            for (polygon, face) in topology.faces.iter().enumerate() {
                let corners = mesh
                    .mesh
                    .polygons
                    .get(polygon)
                    .map(|polygon| polygon.corners.len())
                    .unwrap_or(0);
                match face {
                    FaceStatus::Decoded {
                        triangles,
                        degenerate,
                    } => {
                        census.decoded_faces += 1;
                        census.degenerate_triangles += *degenerate as u64;
                        // A decoded face whose triangles are all degenerate
                        // (including one that produced none) draws nothing.
                        if triangles == degenerate {
                            census.degenerate_only_faces += 1;
                            census.missing.push(MissingFace {
                                mesh: mesh.index,
                                polygon,
                                corners,
                                reason: MissingFaceReason::DegenerateOnly,
                            });
                        }
                    }
                    FaceStatus::Rejected(issue) => {
                        let reason = match issue {
                            super::mesh::FaceIssue::UnsupportedNgon { reason, .. } => {
                                MissingFaceReason::Unsupported(reason.code())
                            }
                            other => MissingFaceReason::Invalid(other.code()),
                        };
                        if reason.is_invalid() {
                            census.invalid_faces += 1;
                        } else {
                            census.unsupported_faces += 1;
                        }
                        census.missing.push(MissingFace {
                            mesh: mesh.index,
                            polygon,
                            corners,
                            reason,
                        });
                    }
                    FaceStatus::Culled => {
                        census.culled_faces += 1;
                        census.missing.push(MissingFace {
                            mesh: mesh.index,
                            polygon,
                            corners,
                            reason: MissingFaceReason::Culled,
                        });
                    }
                }
            }
        }
        census
    }

    /// Faces that reach no drawable triangle: the shortfall plus every listed
    /// missing face. `invalid_faces`, `unsupported_faces`, `culled_faces` and
    /// `degenerate_only_faces` are its breakdown (their sum, plus the
    /// shortfall, is this number).
    pub const fn missing_faces(&self) -> u64 {
        self.shortfall_faces
            + self.invalid_faces
            + self.unsupported_faces
            + self.culled_faces
            + self.degenerate_only_faces
    }

    /// Triangles that draw something.
    pub const fn drawn_triangles(&self) -> u64 {
        self.triangles - self.degenerate_triangles
    }

    /// Whether every declared face draws something: nothing is missing, so
    /// nothing has to be enumerated for this container.
    pub const fn is_complete(&self) -> bool {
        self.missing_faces() == 0
    }

    /// The meshes that lost at least one **rejected** face, ascending. A mesh
    /// with a rejected face cannot build a render mesh at all — the render
    /// gate refuses an incomplete topology — so this is exactly the set of
    /// meshes the render layer drops. A mesh that lost only culled or
    /// degenerate-only faces still builds: those faces are per-face
    /// accounting, not refusals.
    pub fn rejected_meshes(&self) -> Vec<u32> {
        let mut meshes: Vec<u32> = self
            .missing
            .iter()
            .filter(|face| {
                matches!(
                    face.reason,
                    MissingFaceReason::Invalid(_) | MissingFaceReason::Unsupported(_)
                )
            })
            .map(|face| face.mesh)
            .collect();
        meshes.sort_unstable();
        meshes.dedup();
        meshes
    }
}
