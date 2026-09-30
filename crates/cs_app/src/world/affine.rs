//! How one world's authored affine is carried into the runtime, exactly
//! (F18, follow-up of **#421**).
//!
//! An authored [`CanonicalTransform`] is an arbitrary affine: rotation, scale,
//! **shear** and mirror. This module decides what each runtime half does with
//! that affine, and it is the only place that decides it.
//!
//! # The decision
//!
//! | half | what carries the authored affine |
//! | --- | --- |
//! | presentation | the node's [`GlobalTransform`], the whole authored matrix, never a decomposition |
//! | collision | the collider's **shape**, with the authored linear map baked into its geometry |
//!
//! Both halves read the same authored matrix through the same conversion
//! ([`super::canonical_matrix`]), so "visual and collision share provenance and
//! coordinate conversion" (F18 non-negotiable behavior 1) stays structural:
//! there is no second matrix, no second asset and no second conversion, only a
//! choice of which runtime component carries the linear map.
//!
//! # Why the linear map cannot stay in the pose (measured, not assumed)
//!
//! * **Presentation.** Bevy's `Transform` is translation/rotation/scale, and
//!   the transform-propagation systems overwrite a `GlobalTransform` from a
//!   `Transform` on the same entity. Measured on the pinned pair
//!   (`bevy 0.19.1`): a node carrying only the authored `GlobalTransform` keeps
//!   it bit-identical across five `App::update`s, while the same node carrying
//!   `Transform` + `GlobalTransform` has the shear **silently replaced by the
//!   identity** on the first propagation. A presented node therefore carries no
//!   `Transform` at all, exactly as a scene node's
//!   [`crate::scene::NodeVisualTransform`] does.
//! * **Collision.** Avian keeps `Position`/`Rotation` (and the collider scale)
//!   in sync by reading `GlobalTransform::compute_transform()`, which is a
//!   translation/rotation/scale decomposition
//!   (`avian3d-0.7.0/src/physics_transform/mod.rs::transform_to_position`). A
//!   shear in a collider's pose is therefore dropped before the broad phase
//!   sees it, so the linear map goes into the **shape** instead.
//!
//! # What is still refused, and why
//!
//! A placement is refused only when an *exact* one does not exist; the reasons
//! are named by [`AffinePlacementError`] and the refusal happens before the
//! first entity is spawned, so a refused definition leaves the app untouched.
//! Concretely: an authored component that does not survive the runtime's f32, a
//! linear map with determinant zero (the object has no volume, so no collider of
//! it exists), and collision geometry the physics library cannot turn into a
//! solid. **A shear and a mirror are not refusals.**
//!
//! The stage that filed this question gated its own answer on measurement: no
//! retail world object with a sheared authored matrix has been observed, and
//! world geometry is not importable yet, so the *occurrence* census is F18-D's
//! (`specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! `### F18-D`). Everything asserted here is measured on the pinned pair over
//! synthetic content (`Origin::SyntheticFixture`) and is claimed as **designed**,
//! never as `verified_original`; see
//! `docs/findings/2026-09-30-f18-b-followup-sheared-world-object-placement.md`.

use avian3d::parry::shape::{ConvexPolyhedron, SharedShape, TriMesh};
use bevy::prelude::{GlobalTransform, Mat3, Mat4, Quat, Vec3};
use cs_content::scene::CanonicalTransform;

use super::spawn::{INSTANCE_TRANSFORM_TOLERANCE, InstanceTransform, canonical_matrix};

/// Which runtime half carries an object's authored affine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AffinePlacement {
    /// The authored linear map **is** a rotation times a scale, so the pose can
    /// carry all of it. The collision shape is the authored primitive,
    /// untouched.
    Trs {
        /// The decomposed pose, as F18-A has always built it.
        transform: InstanceTransform,
    },
    /// The authored linear map carries a shear, so the pose cannot. The
    /// presentation keeps the whole affine and the collision geometry carries
    /// the linear map itself.
    Sheared {
        /// The authored translation, in meters: the part the pose *can* hold.
        translation: Vec3,
        /// The authored linear map: the part the collision *shape* holds.
        linear: Mat3,
        /// How far the authored matrix is from being a rotation-times-scale
        /// product, as a unitless ratio of the largest matrix entry. This is
        /// [`INSTANCE_TRANSFORM_TOLERANCE`]'s numerator divided by the matrix
        /// scale, so "a shear" is a measured number rather than a flag.
        residual: f32,
    },
}

impl AffinePlacement {
    /// Classifies one object's authored affine.
    ///
    /// The test for [`AffinePlacement::Trs`] is exactly the one F18-A used —
    /// rebuilding from the decomposition must land back on the authored matrix
    /// within [`INSTANCE_TRANSFORM_TOLERANCE`] — so every matrix F18-A placed is
    /// still placed the same way. What changes is the *other* branch: a matrix
    /// that is not a rotation-times-scale product is no longer refused for
    /// being one.
    ///
    /// # Errors
    ///
    /// * [`AffinePlacementError::NotRepresentable`] when a component does not
    ///   survive the record's f64 to the runtime's f32.
    /// * [`AffinePlacementError::CollapsesSpace`] when the authored linear map
    ///   has determinant zero: the object has no volume, so no collision
    ///   geometry of it exists. F18-A refused these too — a zero scale axis
    ///   makes glam's decomposition return a NaN rotation, which never
    ///   round-trips — but it could only report "unrepresentable".
    ///
    /// A matrix that *mirrors* (`det < 0`) is placed like any other: measured,
    /// a mirrored sheared box bakes to the same six outward face normals as the
    /// unmirrored one over its own (mirrored) corner set, so the mirror is a
    /// fact about the geometry rather than a reason to refuse it.
    pub fn of(transform: &CanonicalTransform) -> Result<Self, AffinePlacementError> {
        let authored = canonical_matrix(transform);
        if !authored.is_finite() {
            return Err(AffinePlacementError::NotRepresentable);
        }
        let determinant = transform.determinant();
        if determinant == 0.0 {
            return Err(AffinePlacementError::CollapsesSpace);
        }

        let (scale, rotation, translation) = authored.to_scale_rotation_translation();
        let rebuilt = Mat4::from_scale_rotation_translation(scale, rotation, translation);
        if authored.abs_diff_eq(rebuilt, INSTANCE_TRANSFORM_TOLERANCE) {
            return Ok(Self::Trs {
                transform: InstanceTransform {
                    translation,
                    rotation,
                    scale,
                },
            });
        }
        Ok(Self::Sheared {
            translation: authored.w_axis.truncate(),
            linear: Mat3::from_mat4(authored),
            residual: shear_residual(authored),
        })
    }

    /// Whether the linear part has to travel in the collision shape.
    #[must_use]
    pub const fn is_sheared(&self) -> bool {
        matches!(self, Self::Sheared { .. })
    }

    /// The affine the *presentation* carries: the whole authored matrix,
    /// decomposed by nothing. This is the same value
    /// [`crate::scene::NodeVisualTransform`] carries for a scene node.
    #[must_use]
    pub fn presentation(&self, transform: &CanonicalTransform) -> GlobalTransform {
        // Read the authored matrix rather than a cached copy: the placement is
        // a classification of it, not a second source of truth for it.
        GlobalTransform::from(canonical_matrix(transform))
    }

    /// The pose the *collider* carries. For a shear this is the authored
    /// translation with identity rotation and unit scale, because the linear
    /// map is inside the shape; for a rotation-times-scale map it is the
    /// decomposition, exactly as before.
    #[must_use]
    pub fn collider_pose(&self) -> InstanceTransform {
        match *self {
            Self::Trs { transform } => transform,
            Self::Sheared { translation, .. } => InstanceTransform {
                translation,
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        }
    }

    /// The exact affine image of `shape` under this placement's linear map.
    ///
    /// A [`AffinePlacement::Trs`] placement has nothing to bake and returns the
    /// shape untouched. A [`AffinePlacement::Sheared`] placement returns the
    /// shape whose geometry is the authored primitive's geometry pushed through
    /// the authored linear map, so the collider's pose stays an exact
    /// translation/rotation/scale.
    ///
    /// # Errors
    ///
    /// [`AffinePlacementError::UnbuildableCollision`] when `shape` is a kind
    /// whose exact affine image this module does not build, or when the physics
    /// library cannot build a solid from the mapped geometry. A missing solid is
    /// refused rather than replaced by a shape that is close to it.
    pub fn bake(&self, shape: &SharedShape) -> Result<SharedShape, AffinePlacementError> {
        match *self {
            Self::Trs { .. } => Ok(shape.clone()),
            Self::Sheared { linear, .. } => bake_shape(shape, linear),
        }
    }
}

/// Why one object's authored affine has no exact placement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AffinePlacementError {
    /// A component of the authored matrix does not survive the record's f64 to
    /// the runtime's f32, so neither half can draw or collide with it.
    NotRepresentable,
    /// The authored linear map has determinant zero: the object has no volume,
    /// so no collider of it exists. (A zero scale *axis* with a non-zero
    /// determinant is a different object and is placed as a zero-thickness
    /// primitive, as F18-A placed it.)
    CollapsesSpace,
    /// The physics library could not build a solid from the exact affine image
    /// of the authored collision geometry.
    UnbuildableCollision,
    /// The object is **mesh-derived** and sheared, and this stage does not
    /// build that combination.
    ///
    /// Not an engine limit: Avian's `ColliderConstructor::TrimeshFromMesh`
    /// derives the collider from the presented `Mesh3d` handle, so the authored
    /// linear map would have to be baked into a *second* upload. That upload's
    /// fingerprint would no longer be the authored one, which is exactly the
    /// one-asset provenance claim
    /// [`super::spawn::MeshReference`] carries, so the decision belongs to
    /// F18-B's mesh policy and F18-D's census of whether such an object exists
    /// — not to this module. Refused by name rather than placed from a derived
    /// asset the report would misattribute.
    ShearedMeshUndecided,
}

impl AffinePlacementError {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::NotRepresentable => "not_representable",
            Self::CollapsesSpace => "collapses_space",
            Self::UnbuildableCollision => "unbuildable_collision",
            Self::ShearedMeshUndecided => "sheared_mesh_undecided",
        }
    }
}

impl std::fmt::Display for AffinePlacementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotRepresentable => {
                write!(f, "its authored matrix does not fit the f32 render affine")
            }
            Self::CollapsesSpace => {
                write!(
                    f,
                    "its authored linear map collapses space (determinant zero)"
                )
            }
            Self::UnbuildableCollision => {
                write!(f, "its exact collision geometry could not be built")
            }
            Self::ShearedMeshUndecided => write!(
                f,
                "its collision is mesh-derived and sheared, which would need a second \
                 derived upload this stage does not build"
            ),
        }
    }
}

impl std::error::Error for AffinePlacementError {}

/// How far `authored` is from being a rotation-times-scale product, relative to
/// its own size: the largest entry of `authored - R·S` divided by the largest
/// entry of `authored` (or 1, so a tiny matrix is not reported as infinitely
/// sheared).
///
/// A `Trs` placement has a residual at or below
/// [`INSTANCE_TRANSFORM_TOLERANCE`] divided by that denominator; a `Sheared`
/// placement reports its own shear as a number a later stage can count rather
/// than a boolean it has to trust.
#[must_use]
pub fn shear_residual(authored: Mat4) -> f32 {
    let (scale, rotation, translation) = authored.to_scale_rotation_translation();
    let rebuilt = Mat4::from_scale_rotation_translation(scale, rotation, translation);
    let authored_entries = authored.to_cols_array();
    let rebuilt_entries = rebuilt.to_cols_array();
    let residual = (0..16)
        .map(|index| (authored_entries[index] - rebuilt_entries[index]).abs())
        .fold(0.0_f32, f32::max);
    let largest = authored_entries
        .iter()
        .map(|entry| entry.abs())
        .fold(0.0_f32, f32::max);
    residual / largest.max(1.0)
}

/// The exact affine image of a collision `shape` under `linear`.
///
/// * a [`avian3d::parry::shape::Cuboid`] becomes the convex polyhedron whose
///   eight points are the box's eight corners pushed through `linear` — a
///   parallelepiped, which is convex, so the hull of those eight points **is**
///   the box and nothing is simplified;
/// * a [`TriMesh`] becomes a triangle mesh over the very same stored vertices
///   pushed through `linear`, keeping every stored triangle.
///
/// Both are rebuilt from the *authored* geometry through the *authored* map, so
/// no second mesh, upload or fingerprint is introduced: the collision geometry
/// is a runtime shape derived from the same record the presentation draws
/// (F18 non-negotiable behavior 1).
///
/// # Errors
///
/// [`AffinePlacementError::UnbuildableCollision`] for any other shape kind, and
/// when the physics library cannot build a solid from the mapped geometry.
pub fn bake_shape(shape: &SharedShape, linear: Mat3) -> Result<SharedShape, AffinePlacementError> {
    if let Some(box_shape) = shape.as_cuboid() {
        let (points, triangles) = parallelepiped(box_shape.half_extents, linear);
        return ConvexPolyhedron::from_convex_mesh(points, &triangles)
            .map(SharedShape::new)
            .ok_or(AffinePlacementError::UnbuildableCollision);
    }
    if let Some(mesh) = shape.as_trimesh() {
        let vertices: Vec<Vec3> = mesh
            .vertices()
            .iter()
            .map(|vertex| linear * *vertex)
            .collect();
        return TriMesh::new(vertices, mesh.indices().to_vec())
            .map(SharedShape::new)
            .map_err(|_| AffinePlacementError::UnbuildableCollision);
    }
    Err(AffinePlacementError::UnbuildableCollision)
}

/// The eight points of `half` pushed through `linear`, and the twelve triangles
/// of the parallelepiped they form.
///
/// The corner at index `i` takes its `x`, `y` and `z` signs from bits 0, 1 and 2
/// of `i` (a clear bit is the negative side). The six quads are wound
/// counter-clockwise seen from outside the solid in the *local* frame.
///
/// The winding is **not load-bearing** on the pinned pair, which is why the
/// mirror case needs no special handling: measured, reversing every triangle
/// leaves the built solid's six face normals bit-identical, and a linear map
/// that mirrors (`det < 0`) produces the same six normals over its own mirrored
/// corner set. `ConvexPolyhedron::from_convex_mesh` merges each coplanar quad
/// and orients the merged face from the polygon's own geometry, so what decides
/// the outward direction is the corner set — and that *is* the authored map's
/// exact image. The acceptance test asserts the six normals as that image's,
/// not the winding this table happens to use.
fn parallelepiped(half: Vec3, linear: Mat3) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let points: Vec<Vec3> = (0..8)
        .map(|index| {
            let sign = |bit: u32| if index & (1 << bit) == 0 { -1.0 } else { 1.0 };
            linear * Vec3::new(sign(0) * half.x, sign(1) * half.y, sign(2) * half.z)
        })
        .collect();
    const QUADS: [[u32; 4]; 6] = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    let triangles: Vec<[u32; 3]> = QUADS
        .iter()
        .flat_map(|quad| [[quad[0], quad[2], quad[1]], [quad[0], quad[3], quad[2]]])
        .collect();
    debug_assert_eq!(points.len(), 8);
    debug_assert_eq!(triangles.len(), 12);
    (points, triangles)
}
