//! ECS binding records for canonical scene nodes
//! (`specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md`, stage
//! `### F11-A`; shared contract `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! These are the typed *outputs* a scene consumer binds into the ECS — the
//! records later stages attach to spawned entities. No system runs here yet;
//! F11-B/C own import, LOD selection and spawn wiring.
//!
//! [`SceneNodeBinding`] ties an entity to its canonical
//! [`cs_content::scene::SceneNode`] by [`ContentId`], stamped with the
//! [`SceneGeneration`] that produced it, so a reload under a new generation
//! can never leave a stale binding looking live (F11 non-negotiable
//! behavior 5: session-generation ownership; `IDENTITY-CONTENT`: session
//! generations).
//!
//! [`NodeVisualTransform`] carries a node's composed
//! [`CanonicalTransform`] as a Bevy [`GlobalTransform`]. `GlobalTransform`
//! wraps a full affine matrix, so a mirrored subtree (negative scale)
//! survives intact — Bevy's `Transform` TRS could not hold it. The render
//! path and the collision path both read the node's one composed transform,
//! so neither can diverge from the other across an LOD switch (F11
//! non-negotiable behavior 4).

use bevy::ecs::component::Component;
use bevy::math::Mat4;
use bevy::prelude::GlobalTransform;
use cs_content::scene::CanonicalTransform;
use cs_types::content::ContentId;

/// The generation of a scene load: a monotonically increasing stamp that
/// makes stale entity bindings detectable after a reload.
///
/// Generations count up from 1; generation 0 is never produced by
/// [`Self::next`], so a default-constructed binding is visibly unspawned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SceneGeneration(pub u64);

impl SceneGeneration {
    /// The generation after this one.
    #[must_use]
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

/// Component: marks an entity as bound to one canonical scene node of one
/// scene generation.
///
/// `node` is the stable content id (`scene_node/<container>.<path>`); a
/// reload stamps new bindings with the new [`SceneGeneration`], so old
/// entities are identified by the generation mismatch rather than by
/// surviving pointers.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct SceneNodeBinding {
    /// The canonical scene node this entity presents.
    pub node: ContentId,
    /// The scene generation that spawned the binding.
    pub generation: SceneGeneration,
}

/// Component: the node's composed canonical transform as the ECS-side
/// affine.
///
/// The canonical matrix is arbitrary affine — rotation, scale and shear —
/// so it maps to a [`GlobalTransform`], never to Bevy's `Transform`: a
/// mirrored subtree's negative determinant would be silently lost by a TRS
/// decomposition.
#[derive(Component, Clone, Copy, Debug)]
pub struct NodeVisualTransform(pub GlobalTransform);

/// Why a canonical transform could not be carried into the ECS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeTransformError {
    /// A component did not survive the f64 → f32 cast (overflow to
    /// infinity); the canonical value is outside the renderable range.
    NotRepresentable,
}

impl core::fmt::Display for NodeTransformError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotRepresentable => {
                write!(f, "canonical transform does not fit the f32 render affine")
            }
        }
    }
}

impl std::error::Error for NodeTransformError {}

impl NodeVisualTransform {
    /// Converts a node's composed canonical transform (meters, row-major
    /// 3×3 + translation) into the ECS affine.
    ///
    /// # Errors
    ///
    /// [`NodeTransformError::NotRepresentable`] when an f64 component does
    /// not survive the f32 cast.
    pub fn from_canonical(transform: &CanonicalTransform) -> Result<Self, NodeTransformError> {
        let l = transform.linear();
        let t = transform.translation();
        // Mat4 is column-major; the affine's columns are the linear map's
        // columns and the translation.
        let mut columns = [
            [l[0][0] as f32, l[1][0] as f32, l[2][0] as f32, 0.0],
            [l[0][1] as f32, l[1][1] as f32, l[2][1] as f32, 0.0],
            [l[0][2] as f32, l[1][2] as f32, l[2][2] as f32, 0.0],
            [t[0] as f32, t[1] as f32, t[2] as f32, 1.0],
        ];
        for column in &mut columns {
            if !column.iter().all(|value| value.is_finite()) {
                return Err(NodeTransformError::NotRepresentable);
            }
        }
        Ok(Self(GlobalTransform::from(Mat4::from_cols_array_2d(
            &columns,
        ))))
    }

    /// The ECS affine.
    #[must_use]
    pub fn global(&self) -> GlobalTransform {
        self.0
    }

    /// Whether the composed transform mirrors space — the same flag the
    /// canonical [`CanonicalTransform::mirrored`] reports, read back off the
    /// f32 affine.
    #[must_use]
    pub fn mirrored(&self) -> bool {
        self.0.affine().matrix3.determinant() < 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_types::content::ContentKind;

    /// The binding record holds its stable id and generation; a generation
    /// bump marks a reload.
    #[test]
    fn accept_f11_a_scene_binding_carries_node_id_and_generation() {
        let node = ContentId::from_source(ContentKind::SceneNode, "planes.corsair.wing_l")
            .expect("valid id");
        let generation = SceneGeneration::default().next();
        let binding = SceneNodeBinding {
            node: node.clone(),
            generation,
        };
        assert_eq!(binding.node, node);
        assert_eq!(binding.generation, SceneGeneration(1));
        assert_eq!(generation.next(), SceneGeneration(2));
        // A reload's bindings are distinguishable from the previous scene's.
        assert_ne!(binding.generation, generation.next());
    }

    /// A mirrored canonical transform must reach the ECS intact: Bevy's
    /// `GlobalTransform` is a full affine, so the negative determinant and
    /// the authored translation survive the f64 → f32 conversion.
    #[test]
    fn accept_f11_a_visual_transform_preserves_mirror_and_translation() {
        // Mirror across canonical X, shifted +2 m in Y.
        let mirrored = CanonicalTransform::try_new(
            [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            [0.0, 2.0, 0.0],
        )
        .expect("finite transform");
        let component =
            NodeVisualTransform::from_canonical(&mirrored).expect("representable transform");
        assert!(component.mirrored());
        let matrix = component.global().to_matrix();
        // Column-major: x_axis is column 0; translation is column 3.
        assert_eq!(matrix.x_axis.truncate().to_array(), [-1.0, 0.0, 0.0]);
        assert_eq!(
            matrix.w_axis.truncate().to_array(),
            [0.0, 2.0, 0.0],
            "the authored translation survives"
        );

        // A transform that cannot fit f32 is refused, not silently made
        // infinite.
        let too_large =
            CanonicalTransform::try_new(CanonicalTransform::IDENTITY.linear(), [1e300, 0.0, 0.0])
                .expect("finite f64 transform");
        assert_eq!(
            NodeVisualTransform::from_canonical(&too_large).map(|_| ()),
            Err(NodeTransformError::NotRepresentable)
        );
    }
}
