//! Reading one **original** world container and importing it into the records
//! the runtime consumes (task #629, `M01-LC-WORLD-IMPORT`).
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`
//! (`### F18-A`, `### F18-B`), the F18-D evidence stage, and the first-mission
//! path `specs/README.md` names as `VS-M01-RUNTIME`. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! # The gap this closes
//!
//! Nothing turned an original `gamez.zbd` into the
//! [`WorldDefinition`](cs_content::world::WorldDefinition) that
//! [`spawn_world`](super::spawn::spawn_world) consumes. F18-B's
//! `world::fixture` producers are **synthetic only** — they author metres by
//! construction and say so — and F18-A's stored-hierarchy work decoded the node
//! array but deliberately left the world record's own content, the sector
//! partition and the per-object collision roles **unmeasured**. So a mission
//! could load a designed harbor world and nothing could load an original one.
//!
//! This module is the missing hop, and it is three steps with no step optional:
//!
//! 1. **read** the container through the production discovery pass and the
//!    production readers ([`read_gamez_nodes`], [`read_gamez_meshes`]);
//! 2. **import** it through
//!    [`cs_content::world::import_world_container`], which reads the world
//!    record's own partition grid and turns it plus the records it names into a
//!    definition;
//! 3. **upload** the geometry the definition names, so the runtime has both
//!    halves of the "one asset, one set of triangles" contract.
//!
//! # What this module does *not* claim
//!
//! * **Not `verified_original`.** `retail` is read access to the owner's files.
//!   No original run happened, so nothing here is evidence of how the 2000
//!   engine loaded, streamed or collided with a world.
//! * **No unit is measured.** The original's world-vertex unit is task #436's
//!   measurement and it has not been made; `definition` therefore takes its
//!   conversion from the caller's [`SourceAdapter`] and the import report names
//!   the factor it used
//!   ([`WorldImportReport::meters_per_unit`](cs_content::world::WorldImportReport::meters_per_unit)).
//!   Nothing here supplies a factor of its own.
//! * **No role is invented.** Every collision role the container does not state
//!   arrives as an explicit unknown carrying a claim id, and so does every
//!   gameplay surface and the world's boundary.
//! * **The mesh identity is the catalog's.** A stored mesh-array slot is an
//!   element of the shared render-mesh collection
//!   ([`cs_content::mesh::MeshCatalog`], F10-C.03): the definition's mesh
//!   references are [`cs_content::mesh::MeshId::content_id`], the same
//!   `mesh/<container>.<slot>` id the retail baseline inventory gives that
//!   mesh, and the geometry is uploaded from the catalog's own
//!   [`MeshUpload`](cs_content::mesh::MeshUpload) payload. No per-container
//!   name is minted here, so two containers cannot hold one mesh under two ids.

use std::fmt;
use std::fs;
use std::path::Path;

use std::collections::BTreeMap;

use cs_assets::install::{self, DiscoveryError};
use cs_assets::vfs::{ContentSession, SessionBuilder, WORLD_NAMESPACE};
use cs_content::coordinates::SourceAdapter;
use cs_content::mesh::{
    MeshCatalog, MeshDependencies, MeshError, MeshPresentationUnknown, RenderMesh, RenderMeshError,
    ResolvedMesh,
};
use cs_content::scene::{GameZSceneError, MeshSlot};
use cs_content::textures::TextureCatalog;
use cs_content::world::{ImportedWorld, WorldDefinition, WorldId, WorldIdError, WorldImportError};
use cs_formats::gamez::{GameZMeshes, GameZNodes, read_gamez_meshes, read_gamez_nodes};
use cs_formats::io::ParseContext;
use cs_types::asset_id::{AssetKey, ResolveContext, SourceSpan, WorldGroup};
use cs_types::content::{ContentId, ContentIdError, ContentKind, Origin, Provenance};
use cs_types::evidence::{ClaimId, ClaimStatus};

use super::audit::GEOMETRY_CONTAINER_FILE;
use super::meshes::{WorldMeshBuildError, WorldMeshes};

/// The claim an imported world record is filed under.
///
/// **ObservedTool, not `VerifiedOriginal`.** Every fact behind the import was
/// read out of the owner's container bytes by the production readers: the world
/// record's partition grid, the records it names, their stored mesh indices,
/// transforms and world-space bounding boxes. That is measured-from-bytes, which
/// is the strongest class this stage can reach; it says nothing about the
/// original engine's runtime behaviour, which needs an original run.
pub const RETAIL_WORLD_IMPORT: &str = "f18-world.retail-container-import";

/// Why one original world container could not be read, imported or uploaded.
#[derive(Debug)]
pub enum RetailWorldError {
    /// The installation could not be discovered.
    Discovery(DiscoveryError),
    /// The container could not be read from disk.
    Read {
        /// The container's logical key.
        container: String,
        /// Why the read failed.
        reason: String,
    },
    /// Production discovery inventoried no such container.
    Absent {
        /// The container's logical key.
        container: String,
    },
    /// The node array did not decode.
    Nodes {
        /// The container's logical key.
        container: String,
        /// The reader's own reason.
        reason: String,
    },
    /// The mesh array did not decode.
    Meshes {
        /// The container's logical key.
        container: String,
        /// The reader's own reason.
        reason: String,
    },
    /// The group directory's name is not a world id.
    WorldId {
        /// The container's logical key.
        container: String,
        /// The refusal itself.
        reason: WorldIdError,
    },
    /// The mesh-slot table could not be built.
    Slot {
        /// The mesh-array slot a record would name.
        index: usize,
        /// The refusal itself.
        reason: ContentIdError,
    },
    /// The import refused the container.
    Import(WorldImportError),
    /// The session or the mesh catalog could not be opened over the container.
    Catalog {
        /// The container's logical key.
        container: String,
        /// Why.
        reason: String,
    },
    /// The mesh catalog refused to resolve or prepare a stored mesh.
    Mesh {
        /// The mesh-array slot.
        index: u32,
        /// The catalog's own refusal.
        reason: MeshError,
    },
    /// A stored mesh did not become a render mesh.
    RenderMesh {
        /// The mesh-array slot.
        index: u32,
        /// The refusal itself.
        reason: RenderMeshError,
    },
    /// The upload adapter refused a render mesh.
    Upload {
        /// The mesh-array slot.
        index: u32,
        /// The refusal itself.
        reason: WorldMeshBuildError,
    },
    /// The definition named a mesh that is not an element of this container's
    /// mesh catalog.
    UnknownMeshReference {
        /// The mesh reference the definition named.
        mesh: String,
    },
    /// The mesh-slot table could not be reconciled into the scene layer's own
    /// mesh-slot type.
    SceneSlot(GameZSceneError),
    /// A record's mesh reference named nothing.
    Unresolved {
        /// The message.
        reason: String,
    },
}

impl fmt::Display for RetailWorldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(error) => write!(f, "{error}"),
            Self::Read { container, reason } => {
                write!(f, "could not read {container}: {reason}")
            }
            Self::Absent { container } => {
                write!(f, "discovery inventoried no {container}")
            }
            Self::Nodes { container, reason } => {
                write!(
                    f,
                    "could not decode the node array of {container}: {reason}"
                )
            }
            Self::Meshes { container, reason } => {
                write!(
                    f,
                    "could not decode the mesh array of {container}: {reason}"
                )
            }
            Self::WorldId { container, reason } => {
                write!(f, "{container} does not name a world: {reason}")
            }
            Self::Slot { index, reason } => {
                write!(f, "mesh slot {index} has no catalog name: {reason}")
            }
            Self::Import(error) => write!(f, "{error}"),
            Self::Catalog { container, reason } => {
                write!(
                    f,
                    "could not open the mesh catalog of {container}: {reason}"
                )
            }
            Self::Mesh { index, reason } => {
                write!(f, "mesh slot {index} is not a catalog upload: {reason}")
            }
            Self::RenderMesh { index, reason } => {
                write!(f, "mesh slot {index} is not a usable render mesh: {reason}")
            }
            Self::Upload { index, reason } => {
                write!(f, "mesh slot {index} could not be uploaded: {reason}")
            }
            Self::UnknownMeshReference { mesh } => {
                write!(
                    f,
                    "the definition names mesh `{mesh}`, which is not in this container's mesh catalog"
                )
            }
            Self::SceneSlot(error) => write!(f, "{error}"),
            Self::Unresolved { reason } => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for RetailWorldError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Discovery(error) => Some(error),
            Self::WorldId { reason, .. } => Some(reason),
            Self::Slot { reason, .. } => Some(reason),
            Self::Import(error) => Some(error),
            Self::Mesh { reason, .. } => Some(reason),
            Self::RenderMesh { reason, .. } => Some(reason),
            Self::Upload { reason, .. } => Some(reason),
            Self::SceneSlot(error) => Some(error),
            _ => None,
        }
    }
}

impl From<WorldImportError> for RetailWorldError {
    fn from(error: WorldImportError) -> Self {
        Self::Import(error)
    }
}

impl From<DiscoveryError> for RetailWorldError {
    fn from(error: DiscoveryError) -> Self {
        Self::Discovery(error)
    }
}

/// The provenance every value this module's import resolves carries.
///
/// [`ClaimStatus::ObservedTool`], never `VerifiedOriginal`: the facts were read
/// out of the container bytes by the production readers, which is file access,
/// not an original run. `span` is `None` only where no container exists to
/// point at, and the class is then `Unknown` rather than asserted.
fn import_provenance(span: Option<SourceSpan>) -> Result<Provenance, RetailWorldError> {
    Provenance::new(
        ClaimId::new(RETAIL_WORLD_IMPORT).map_err(|error| RetailWorldError::Unresolved {
            reason: format!("the import claim id is invalid: {error}"),
        })?,
        if span.is_some() {
            ClaimStatus::ObservedTool
        } else {
            ClaimStatus::Unknown
        },
        span,
    )
    .map_err(|error| RetailWorldError::Unresolved {
        reason: error.to_string(),
    })
}

/// The **playtest scene's** per-container mesh identity:
/// `<group>.mesh-<index>` in the `mesh` namespace.
///
/// [`crate::playtest_retail`] is the only caller. The world import no longer
/// uses it: its mesh references are elements of the shared render-mesh catalog
/// (`MeshId::content_id`, #638). The playtest scene still names its meshes this
/// way and is the remaining per-container naming to move onto the catalog.
///
/// # Errors
///
/// [`ContentIdError`] when the group's own name is one the id grammar refuses —
/// which is a refusal, not a name to guess around.
pub fn container_mesh_key(group: &str, index: usize) -> Result<ContentId, ContentIdError> {
    ContentId::from_source(
        ContentKind::Mesh,
        &format!("{}.mesh-{index}", group.to_ascii_lowercase()),
    )
}

/// One original world container, read and held open for the importer.
///
/// The node array and the mesh array are decoded **once**, here, and the
/// container's bytes are kept because the world record's partition grid lives in
/// them: the node reader keeps the grid's two counts because they are what make
/// the block knowable, and the content is re-derived from the same bytes.
#[derive(Debug)]
pub struct RetailWorldContainer {
    group: String,
    container_key: String,
    container_sha256: String,
    span: SourceSpan,
    bytes: Vec<u8>,
    nodes: GameZNodes,
    meshes: GameZMeshes,
    slots: Vec<MeshSlot>,
    session: ContentSession,
    catalog: MeshCatalog,
    /// Each present mesh's catalog id and its resolution, so a definition's mesh
    /// reference reaches the catalog's upload without re-parsing the id.
    resolved: BTreeMap<ContentId, ResolvedMesh>,
}

impl RetailWorldContainer {
    /// The group's directory name as the store spells it, e.g. `C1C`.
    #[must_use]
    pub fn group(&self) -> &str {
        &self.group
    }

    /// The container's logical key inside the installation, e.g.
    /// `ZBD/C1C/gamez.zbd`.
    #[must_use]
    pub fn container_key(&self) -> &str {
        &self.container_key
    }

    /// SHA-256 of the whole container file, from production discovery, so a
    /// rerun over a different installation reports a different digest.
    #[must_use]
    pub fn container_sha256(&self) -> &str {
        &self.container_sha256
    }

    /// Where this container's bytes live: the installation fingerprint, the
    /// container's own path inside it and the whole-file span. Every value an
    /// import resolves carries this, so a reader can go back to the bytes.
    #[must_use]
    pub const fn span(&self) -> &SourceSpan {
        &self.span
    }

    /// The decoded node array.
    #[must_use]
    pub const fn nodes(&self) -> &GameZNodes {
        &self.nodes
    }

    /// The decoded mesh array.
    #[must_use]
    pub const fn meshes(&self) -> &GameZMeshes {
        &self.meshes
    }

    /// The mesh catalog the definition's mesh references are elements of.
    #[must_use]
    pub const fn mesh_catalog(&self) -> &MeshCatalog {
        &self.catalog
    }

    /// The content session the mesh catalog was read in.
    #[must_use]
    pub const fn session(&self) -> &ContentSession {
        &self.session
    }

    /// The world this container is.
    ///
    /// The group's own directory name, through the same id grammar the rest of
    /// the content layer uses. A group whose name the grammar refuses is a
    /// refusal here rather than a world under a guessed name.
    pub fn world(&self) -> Result<WorldId, RetailWorldError> {
        WorldId::from_key(&self.group.to_ascii_lowercase()).map_err(|reason| {
            RetailWorldError::WorldId {
                container: self.container_key.clone(),
                reason,
            }
        })
    }

    /// The provenance every value this container's import resolves carries.
    pub fn provenance(&self) -> Result<Provenance, RetailWorldError> {
        import_provenance(Some(self.span.clone()))
    }

    /// Imports the container into the definition the runtime consumes.
    ///
    /// `adapter` supplies the stored-unit-to-metre conversion and the axis map.
    /// This module never supplies one of its own: the original's world-vertex
    /// unit is unmeasured, so a factor chosen here would be a guess about a
    /// length. Whatever the caller passes, the factor and its own evidence class
    /// travel on the returned
    /// [`WorldImportReport`](cs_content::world::WorldImportReport).
    pub fn definition(
        &self,
        origin: Origin,
        adapter: &SourceAdapter,
    ) -> Result<ImportedWorld, RetailWorldError> {
        cs_content::world::import_world_container(
            self.world()?,
            origin,
            &self.nodes,
            &self.bytes,
            &self.slots,
            adapter,
            self.provenance()?,
        )
        .map_err(RetailWorldError::Import)
    }

    /// Uploads the geometry `definition` names, one engine mesh per catalog mesh.
    ///
    /// Each mesh reference is resolved through the container's
    /// [`MeshCatalog`] and uploaded from the catalog's own
    /// [`MeshUpload`](cs_content::mesh::MeshUpload) through the **production**
    /// F17-B adapter ([`WorldMeshes::insert_mesh_upload`]), so a mesh object's
    /// collider is derived from the same triangles its visual draws and its id
    /// is the catalog's. A mesh this container holds no geometry for is **not**
    /// registered: the spawn reports it as
    /// [`SkipReason::MeshUnavailable`](super::spawn::SkipReason) rather than
    /// being handed a substitute shape.
    pub fn uploaded_meshes(
        &self,
        definition: &WorldDefinition,
    ) -> Result<WorldMeshes, RetailWorldError> {
        let mut out = WorldMeshes::new();
        for object in definition.objects() {
            let known = match object.mesh() {
                cs_types::content::Resolved::Known(known) => &known.value,
                cs_types::content::Resolved::Unknown { .. } => continue,
            };
            if out.contains(known) {
                continue;
            }
            let Some(resolved) = self.resolved.get(known) else {
                return Err(self.unknown_reference(known));
            };
            let index = resolved.id().index;
            let upload = self
                .catalog
                .prepare_upload(&self.session, resolved)
                .map_err(|reason| RetailWorldError::Mesh { index, reason })?;
            out.insert_mesh_upload(known.clone(), &upload)
                .map_err(|reason| RetailWorldError::Upload { index, reason })?;
        }
        Ok(out)
    }

    /// The refusal for a reference that is not an element of this container's
    /// catalog. It cannot happen for a definition [`Self::definition`] produced,
    /// so it is a refusal about the *caller* rather than a silent skip: a
    /// definition built from somewhere else names meshes this source does not
    /// hold, and quietly registering nothing for them would leave a world whose
    /// objects report a `MeshUnavailable` gap for a reason the report never
    /// names.
    fn unknown_reference(&self, mesh: &ContentId) -> RetailWorldError {
        RetailWorldError::UnknownMeshReference {
            mesh: mesh.key().to_owned(),
        }
    }
}

/// Builds one stored mesh's render mesh through the production F10-E builder.
///
/// **Public** because a second production consumer needs the same builder rather
/// than a second copy of it: the retail playtest scene (`crate::playtest_retail`,
/// task #648) reads one world container's stored meshes and this is the conversion
/// it uploads, so the area's geometry is built by exactly this function and not by
/// a variant of it.
///
/// # Errors
///
/// [`RetailWorldError::RenderMesh`] when the stored mesh does not have one
/// material group per polygon, or when the F10-E builder refuses it.
pub fn stored_render_mesh(
    slot: &cs_formats::gamez::GameZMesh,
) -> Result<RenderMesh, RetailWorldError> {
    if !slot.groups_are_complete() {
        return Err(RetailWorldError::RenderMesh {
            index: slot.index,
            reason: RenderMeshError::GroupCount {
                polygons: slot.mesh.polygons.len(),
                groups: 0,
            },
        });
    }
    let groups: Vec<Vec<cs_formats::gamez::RawMaterialGroup>> = (0..slot.mesh.polygons.len())
        .map(|polygon| slot.groups(polygon).map(<[_]>::to_vec).unwrap_or_default())
        .collect();
    RenderMesh::from_stored_groups(&slot.mesh, &groups).map_err(|reason| {
        RetailWorldError::RenderMesh {
            index: slot.index,
            reason,
        }
    })
}

/// The presentation decisions open for one render mesh.
///
/// The first three are properties of the render pipeline, not of one container,
/// so they are one fixed list; the fourth exists only when the mesh really
/// stored a polygon with more than one material group, because a mesh that
/// stores one group per polygon has no such question.
///
/// **Public** for the same reason as [`stored_render_mesh`]: the upload adapter
/// takes exactly this list, so every production consumer of a retail render mesh
/// declares the same presentation unknowns rather than a list of its own.
#[must_use]
pub fn stored_presentation_unknowns(render: &RenderMesh) -> Vec<MeshPresentationUnknown> {
    let mut unknowns = vec![
        MeshPresentationUnknown::FrontFaceWinding,
        MeshPresentationUnknown::UvOrigin,
        MeshPresentationUnknown::VertexColor,
    ];
    if render.extra_group_triangles() > 0 {
        unknowns.push(MeshPresentationUnknown::MultiMaterialGroup);
    }
    unknowns
}

/// Reads one world group's container out of an installation.
///
/// The path is the one production discovery spells — the manifest's own
/// `relative_spelling`, not a re-joined logical key — because a case-sensitive
/// filesystem would refuse the join. Both arrays are decoded through the
/// production readers, and the mesh-slot table is built for every slot the array
/// holds so a record's stored `mesh_index` resolves without a second guess.
///
/// # Errors
///
/// [`RetailWorldError::Discovery`] when the installation cannot be inventoried,
/// [`RetailWorldError::Absent`] when it holds no such container,
/// [`RetailWorldError::Read`] when the file cannot be read, and
/// [`RetailWorldError::Nodes`] / [`RetailWorldError::Meshes`] when a section
/// does not decode. A refusal here aborts rather than importing a container the
/// readers could only partly read.
pub fn read_world_container(
    install_root: &Path,
    group: &str,
) -> Result<RetailWorldContainer, RetailWorldError> {
    let found = install::discover(install_root)?;
    let container_key = format!(
        "zbd/{}/{GEOMETRY_CONTAINER_FILE}",
        group.to_ascii_lowercase()
    );
    let Some(record) = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == container_key)
    else {
        return Err(RetailWorldError::Absent {
            container: container_key,
        });
    };
    let container_sha256 = record.sha256.to_hex();
    let bytes = fs::read(
        found
            .manifest
            .host_root
            .join(record.relative_spelling.as_str()),
    )
    .map_err(|error| RetailWorldError::Read {
        container: container_key.clone(),
        reason: error.to_string(),
    })?;

    let mut parse = ParseContext::with_defaults(container_key.clone());
    let nodes = read_gamez_nodes(&mut parse, &bytes).map_err(|error| RetailWorldError::Nodes {
        container: container_key.clone(),
        reason: error.to_string(),
    })?;
    let mut mesh_parse = ParseContext::with_defaults(container_key.clone());
    let meshes = read_gamez_meshes(&mut mesh_parse, &container_key, &bytes).map_err(|error| {
        RetailWorldError::Meshes {
            container: container_key.clone(),
            reason: error.to_string(),
        }
    })?;

    let span = SourceSpan::new(
        install::fingerprint(&found.manifest),
        record.relative_spelling.as_str(),
        None,
        0,
        u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        Some(record.sha256),
    )
    .map_err(|error| RetailWorldError::Unresolved {
        reason: format!("the container's source span is not recordable: {error}"),
    })?;

    let (session, catalog, group_key) = open_catalog(install_root, &found, group, &container_key)?;
    let Some(opened) = catalog.containers().next() else {
        let reason = catalog
            .failures()
            .next()
            .map_or_else(|| "no container opened".to_owned(), |(_, e)| e.to_string());
        return Err(RetailWorldError::Catalog {
            container: container_key,
            reason,
        });
    };
    // The slot's provenance is the catalog's own: the span of the container it
    // read, in the session that read it.
    let provenance = Provenance::new(
        ClaimId::new(RETAIL_WORLD_IMPORT).map_err(|error| RetailWorldError::Unresolved {
            reason: format!("the import claim id is invalid: {error}"),
        })?,
        ClaimStatus::ObservedTool,
        Some(opened.span().clone()),
    )
    .map_err(|error| RetailWorldError::Unresolved {
        reason: error.to_string(),
    })?;
    let mut slots = Vec::with_capacity(meshes.meshes.len());
    let mut resolved = BTreeMap::new();
    for index in 0..meshes.meshes.len() {
        let index = u32::try_from(index).unwrap_or(u32::MAX);
        let id = opened
            .id(index)
            .content_id()
            .map_err(|reason| RetailWorldError::Slot {
                index: index as usize,
                reason,
            })?;
        slots.push(
            MeshSlot::new(id.clone(), provenance.clone()).map_err(RetailWorldError::SceneSlot)?,
        );
        // An absent or refused slot keeps its id (a node may name it) but has no
        // resolution: nothing is registered for it and the spawn reports the gap.
        if let Ok(mesh) = catalog.resolve(&session, &group_key, index) {
            resolved.insert(id, mesh);
        }
    }

    Ok(RetailWorldContainer {
        group: group.to_owned(),
        container_key,
        container_sha256,
        span,
        bytes,
        nodes,
        meshes,
        slots,
        session,
        catalog,
        resolved,
    })
}

/// Opens a content session on the installation scoped to `group`, and the mesh
/// catalog over the group's `gamez.zbd` in it.
fn open_catalog(
    install_root: &Path,
    found: &install::Discovery,
    group: &str,
    container_key: &str,
) -> Result<(ContentSession, MeshCatalog, AssetKey), RetailWorldError> {
    let fail = |reason: String| RetailWorldError::Catalog {
        container: container_key.to_owned(),
        reason,
    };
    let group_path = format!("zbd/{group}");
    let world_group = found
        .diagnosis
        .world_groups
        .iter()
        .find(|candidate| candidate.as_str().eq_ignore_ascii_case(&group_path))
        .ok_or_else(|| fail("discovery names no such world group".to_owned()))?
        .clone();
    let context = ResolveContext::new(install::fingerprint(&found.manifest))
        .with_world_group(WorldGroup::from_relative(world_group));
    let mut builder = SessionBuilder::new(context);
    builder
        .mount_installation(install_root, &found.diagnosis)
        .map_err(|error| fail(error.to_string()))?;
    let session = builder.open();
    let key = |name: &str| {
        AssetKey::from_spelling(WORLD_NAMESPACE, name, "default")
            .map_err(|error| fail(error.to_string()))
    };
    let geometry = key(GEOMETRY_CONTAINER_FILE)?;
    let archive = key("texture.zbd")?;
    let textures = TextureCatalog::open(&session, std::slice::from_ref(&archive));
    let catalog = MeshCatalog::open(
        &session,
        std::slice::from_ref(&geometry),
        &MeshDependencies {
            archive: &archive,
            textures: &textures,
        },
    );
    Ok((session, catalog, geometry))
}

/// The claim the partition grid's role in the imported definition is recorded
/// under, re-exported so a consumer does not have to reach into
/// `cs_content` for it.
pub use cs_content::world::PARTITION_GRID_IS_THE_SECTOR_INDEX as GRID_IS_THE_SECTOR_INDEX;
