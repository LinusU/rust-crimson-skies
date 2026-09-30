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
//! What is *not* claimed here: no original mesh was read, and the map is not
//! fed by a catalog yet. A retail source fills it from
//! `cs_content::mesh::MeshCatalog` through the same F17-B upload adapter; how a
//! world group streams its meshes in and out of this map is F18-D's question
//! (see `docs/findings/2026-09-30-f18-b-world-import-and-static-collision.md`).

use std::collections::BTreeMap;

use bevy::mesh::Mesh;
use cs_types::content::ContentId;
use cs_types::evidence::ContentHash;

use crate::render::bevy_mesh::GroupUpload;

/// One uploaded mesh, with the provenance of the upload it came from.
#[derive(Clone, Debug)]
pub struct WorldMesh {
    mesh: Mesh,
    fingerprint: ContentHash,
    triangles: usize,
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
        Self {
            triangles: upload.report().triangles,
            fingerprint: upload.fingerprint(),
            mesh: upload.into_mesh(),
        }
    }

    /// The uploaded geometry, exactly as the F17-B adapter produced it.
    #[must_use]
    pub const fn mesh(&self) -> &Mesh {
        &self.mesh
    }

    /// The canonical fingerprint of that upload.
    #[must_use]
    pub const fn fingerprint(&self) -> ContentHash {
        self.fingerprint
    }

    /// How many triangles the upload stored, degenerates included. A
    /// mesh-derived collider that carries a different count has been
    /// substituted, which F18 non-negotiable behavior 1 forbids.
    #[must_use]
    pub const fn triangles(&self) -> usize {
        self.triangles
    }
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

    /// Adds one upload under its authored id, returning the entry it replaced.
    pub fn insert(&mut self, id: ContentId, upload: GroupUpload) -> Option<WorldMesh> {
        self.meshes.insert(id, WorldMesh::from_upload(upload))
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
