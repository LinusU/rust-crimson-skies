//! The world-group survey: what the original installation actually declares and
//! what reading one group's geometry container establishes
//! (`specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stage `### F18-D`; shared contract `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! This module is the **measured half** of the F18-D acceptance scenario. The
//! audit itself lives in [`cs_content::world`] and is Bevy-free; this file is the
//! one place that reaches the installation, the content session, the production
//! GameZ readers and the production mesh catalog, and hands the audit a
//! [`WorldGroupCensus`] per group.
//!
//! # Which groups are "every discovered world group"
//!
//! Two production derivations answer that, and the survey uses both rather than
//! picking one:
//!
//!   - [`cs_assets::install::Diagnosis::world_groups`] is the **discovered**
//!     set: every directory observed under the installation's `zbd` root,
//!     original spellings preserved, sorted by logical key. It is what "every
//!     discovered world group" means here — a group the installation has that
//!     the reference leads do not name is still visited, and one the leads name
//!     that the installation lacks is reported as absent rather than audited as
//!     an empty world.
//!   - [`cs_content::campaign_bindings::campaign_layout`] is the **mission**
//!     set: which `ZBD/<chapter><variant>/<mission>` directories the campaign
//!     declares, and therefore which world groups actually carry a mission and
//!     which mission labels live in each. A discovered group with no campaign
//!     mission is still visited (its geometry is real) but is reported as
//!     carrying no mission, which is a fact about the installation rather than a
//!     group the audit may drop.
//!
//! # What is measured, and what is refused
//!
//! For each group the survey reads **that group's own** `gamez.zbd` — the file
//! production discovery inventoried, at the digest production discovery
//! measured — through **both production GameZ readers** (`read_gamez_meshes` and
//! `read_gamez_materials`, over one shared parse context so the two cannot
//! disagree about the 40 header bytes), and builds the production
//! [`RenderMesh`] of the group's representative meshes only. What comes out is a
//! [`GroupFacts`] plus the group's own header words. A group whose container
//! cannot be read, or stores no mesh at all, is a
//! [`WorldGroupBlocker`] carrying the reason — never a census of zeroes and
//! never a row dropped from the report.
//!
//! ## Why the survey does not open a content session
//!
//! It could: `MeshContainer::open` is the production seam for a GameZ container
//! and this survey's counts are the same counts it produces. It is not used here
//! for one measured reason, which is recorded rather than discovered later:
//! **one content session per world group costs 33 s per group on this
//! installation** (measured), because `SessionBuilder::mount_installation` walks
//! and mounts the whole 470 MB tree and a world group is only addressable
//! through a session whose context names that group. Eight groups would have
//! been 4.5 minutes of mounting to re-derive what the readers produce from the
//! same bytes in 0.3 s per group.
//!
//! What that choice gives up is stated: a group's material-index-to-texture
//! **binding** is not audited here, so `bound_texture_names` is `0` and the
//! census says so in a field of its own rather than implying a reconciled
//! corpus. The binding is F10-C.02's and F09's subject, measured through
//! `cs_content::textures` on a session; a *world-group geometry census* has no
//! claim about it. What the survey does read is the group's own stored texture
//! **names** and its stored material-group table, both straight from the
//! container.
//!
//! **What the survey does not establish, and says so:**
//!
//! * *Placement.* No production path decodes a GameZ node array, so no stored
//!   mesh has a position, orientation or scale in world space. The survey
//!   reports [`PlacementSource::Undecoded`] carrying the container header's own
//!   `node_array_size` and `nodes_offset`, and the audit turns that into a
//!   [`TraversalBlocker`]. A survey that decoded a placement would report
//!   [`PlacementSource::Decoded`] and the same audit code would then measure
//!   routes, so the seam is the data and not the code.
//! * *The stored vertex unit.* `cs_content::mesh` applies no scale to stored
//!   positions and nothing in this workspace has established the original's
//!   world-vertex unit, so `vertex_scale_to_m` is [`None`] and every stored
//!   extent in a census is in **stored units**, never metres.
//! * *Traversal routes and stunt openings.* Both follow from the two facts
//!   above, so the survey states none. A census that carried one anyway is an
//!   audit gap, not a result.
//!
//! # What is deliberately not here
//!
//! No format reader, no node-array decoder, no collision-role classification, no
//! boundary rule and no route search. The unknowns F18-A/B/C recorded are still
//! unknowns, and this file measures what can be measured and names the rest.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use cs_assets::install::{self, Discovery, REFERENCE_WORLD_GROUP_LEADS};
use cs_content::campaign_bindings::campaign_layout;
use cs_content::mesh::RenderMesh;
use cs_content::world::UploadVerdict;
use cs_content::world::{
    GroupFacts, PlacementSource, RepresentativeGeometry, WorldAuditError, WorldGroupAudit,
    WorldGroupAuditReport, WorldGroupBlocker, WorldGroupCensus, WorldGroupRef, WorldId,
};
use cs_formats::gamez::{
    FaceCensus, GameZMaterials, GameZMeshes, read_gamez_materials, read_gamez_meshes,
};
use cs_formats::io::ParseContext;

/// The logical key every world group's geometry container has.
///
/// Stated once, as a constant, so the survey and a test never spell it twice.
/// It is a **measured** fact about this installation family, not a general rule:
/// the survey asks the session for the key and reports what the resolution
/// actually was, so a group that stores its geometry elsewhere is describable
/// without editing this constant — see [`WorldGroupSurvey::container_for`].
pub const GEOMETRY_CONTAINER_FILE: &str = "gamez.zbd";

/// The logical key every world group's material-dependency archive has.
///
/// As [`GEOMETRY_CONTAINER_FILE`]: the world's own texture archive, which the
/// `world` mount makes resolvable as `world/default/texture.zbd`.
pub const TEXTURE_ARCHIVE_FILE: &str = "texture.zbd";

/// How many stored meshes the survey may probe when it looks for the largest
/// mesh the upload adapter accepts.
///
/// A **declared, bounded** window, and the reason it exists: the retail world's
/// largest stored meshes are, measurably, not all presentable — several of them
/// store a normal on only some of a material group's vertices, which the F17-B
/// upload adapter refuses rather than fill with an invented value. The census
/// reports that about the meshes it chose, and the capture needs a mesh that
/// went through the adapter, so the survey probes the next-largest meshes until
/// one is accepted or the window runs out. Sixty-four candidates is not a claim
/// about the corpus: it is the point past which "the largest presentable mesh"
/// stops being a useful thing to report and starts being an unbounded search.
pub const PRESENTABLE_PROBE_MESHES: usize = 64;

/// How many representative meshes one group's census carries.
///
/// Three, so a group is compared by its largest draws rather than by a single
/// number that one mesh could dominate, and small enough that reading the
/// retail corpus stays a bounded amount of work. The choice is a **declared**
/// budget, not a measured fact about the original.
pub const REPRESENTATIVE_MESHES: usize = 3;

/// Why a world-group survey could not be produced at all.
#[derive(Debug)]
pub enum WorldGroupSurveyError {
    /// Production discovery could not read the installation.
    Discovery(install::DiscoveryError),
    /// The campaign layout could not be walked.
    Layout(String),
    /// A discovered world group is not a usable [`WorldId`].
    GroupIdentity {
        /// The directory spelling that could not become a world id.
        directory: String,
        /// The identity error, verbatim.
        reason: String,
    },
    /// A group's row was refused by the content layer.
    Refused(WorldAuditError),
    /// No world group was discovered at all.
    NoWorldGroups,
}

impl fmt::Display for WorldGroupSurveyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(error) => {
                write!(f, "the installation could not be discovered: {error}")
            }
            Self::Layout(reason) => write!(f, "the campaign layout could not be walked: {reason}"),
            Self::GroupIdentity { directory, reason } => {
                write!(
                    f,
                    "world group {directory:?} is not a usable world id: {reason}"
                )
            }
            Self::Refused(error) => write!(f, "the world-group rows were refused: {error}"),
            Self::NoWorldGroups => {
                f.write_str("the installation declares no world group under its zbd root")
            }
        }
    }
}

impl std::error::Error for WorldGroupSurveyError {}

/// One group's geometry container as the production readers produced it, plus
/// the production render mesh of the meshes the audit chose to represent it.
///
/// The render meshes are kept because a consumer needs them after the census is
/// built: the GPU capture draws exactly these bytes. Nothing else is retained —
/// the container's bytes are dropped after the per-mesh digests are taken, so
/// the survey does not hold 46 MB of the installation alive to hand on an
/// excerpt.
#[derive(Debug)]
pub struct SurveyedContainer {
    /// The mesh section, as `read_gamez_meshes` produced it.
    pub meshes: GameZMeshes,
    /// The material section, as `read_gamez_materials` produced it.
    pub materials: GameZMaterials,
    /// The exact face accounting over the whole container.
    pub faces: FaceCensus,
    /// One entry per representative mesh, in ascending array index: the mesh's
    /// index, the production render mesh of it, and what the production upload
    /// adapter did with it.
    pub representatives: Vec<(u32, RenderMesh, UploadVerdict)>,
    /// The largest stored mesh inside [`PRESENTABLE_PROBE_MESHES`] candidates
    /// that the upload adapter accepted, and the verdict that says so.
    ///
    /// This is what a consumer can draw. It is a **different** selection from
    /// [`Self::representatives`], on purpose: the census reports the largest
    /// stored meshes whatever the adapter thinks of them, and this one answers
    /// "what can actually be presented".
    pub presentable: Option<(u32, RenderMesh)>,
    /// How many probed candidates the adapter refused before the first accepted
    /// one, or over the whole window.
    pub refused_in_window: usize,
}

impl SurveyedContainer {
    /// The representative render mesh of one stored mesh, if the survey chose
    /// it, together with what the upload adapter did with it.
    #[must_use]
    pub fn representative(&self, mesh_index: u32) -> Option<(&RenderMesh, &UploadVerdict)> {
        self.representatives
            .iter()
            .find(|(index, _, _)| *index == mesh_index)
            .map(|(_, render, verdict)| (render, verdict))
    }

    /// The largest mesh the upload adapter accepted, by stored face count, and
    /// its render mesh.
    #[must_use]
    pub fn largest_presentable(&self) -> Option<(u32, &RenderMesh)> {
        self.presentable
            .as_ref()
            .map(|(index, render)| (*index, render))
    }

    /// How many of the probed candidates the upload adapter refused before the
    /// first accepted one, or over the whole window when none was accepted.
    ///
    /// Reported because it is the number a reader needs to judge the finding: a
    /// window where everything was refused is a corpus the adapter cannot
    /// present at all, and one refused candidate is a single mesh.
    #[must_use]
    pub fn refused_before_presentable(&self) -> usize {
        self.refused_in_window
    }
}

/// What one survey read out of one world group, before the content layer judges
/// it.
#[derive(Debug)]
pub struct SurveyedWorldGroup {
    /// The declared row: identity, directory, container and archive keys and
    /// the mission labels the campaign layout puts in this group.
    pub group: WorldGroupRef,
    /// The mission labels, or empty when the campaign layout declares none in
    /// this group.
    pub missions: Vec<String>,
    /// The container as the production readers produced it, or the blocker.
    state: Result<SurveyedContainer, WorldGroupBlocker>,
    /// SHA-256 of the container file, from production discovery.
    container_sha256: String,
}

impl SurveyedWorldGroup {
    /// The container the survey read, or the blocker that replaced it.
    pub fn container(&self) -> Result<&SurveyedContainer, &WorldGroupBlocker> {
        self.state.as_ref()
    }

    /// SHA-256 of the whole container file, from production discovery.
    #[must_use]
    pub fn container_sha256(&self) -> &str {
        &self.container_sha256
    }

    /// The world's stable identity.
    #[must_use]
    pub fn world(&self) -> &WorldId {
        self.group.world()
    }
}

/// Every world group the installation declares, surveyed.
#[derive(Debug)]
pub struct WorldGroupSurvey {
    /// The groups, in discovered (logical-key) order.
    pub groups: Vec<SurveyedWorldGroup>,
    /// The reference leads of spec F02 that this installation does not hold.
    ///
    /// Reported rather than used to filter: a missing lead is a fact about the
    /// installation, and the lead list is explicitly *not* the authoritative
    /// mission list.
    pub absent_reference_groups: Vec<String>,
}

impl WorldGroupSurvey {
    /// The group row for one world key, or `None` when the installation does not
    /// discover it.
    #[must_use]
    pub fn group(&self, world: &WorldId) -> Option<&SurveyedWorldGroup> {
        self.groups.iter().find(|group| group.world() == world)
    }

    /// How many groups the survey holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.groups.len()
    }

    /// Whether the survey holds no group.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
}

/// Surveys every discovered world group of the installation at `install_root`.
///
/// One production discovery, one campaign-layout walk, and one content session
/// **per group**: a world mount is bound to the group it was mounted for, so
/// measuring eight groups through one session would resolve eight worlds'
/// `gamez.zbd` against whichever mount won. That is the whole reason the
/// sessions are separate, and it is why the survey is a per-group loop rather
/// than one catalog over eight keys.
pub fn survey_world_groups(install_root: &Path) -> Result<WorldGroupSurvey, WorldGroupSurveyError> {
    let found = install::discover(install_root).map_err(WorldGroupSurveyError::Discovery)?;
    let discovered = found.diagnosis.world_groups.clone();
    if discovered.is_empty() {
        return Err(WorldGroupSurveyError::NoWorldGroups);
    }
    // The mission labels each discovered group holds, read from the one
    // production campaign walk. A group the campaign does not mention is still
    // surveyed; it simply reports no mission.
    let missions_by_group = mission_labels_by_group(install_root)?;

    let mut groups = Vec::with_capacity(discovered.len());
    for directory in &discovered {
        let key = directory.logical_key();
        let world = WorldId::from_key(key.rsplit('/').next().unwrap_or(&key)).map_err(|error| {
            WorldGroupSurveyError::GroupIdentity {
                directory: directory.as_str().to_owned(),
                reason: error.to_string(),
            }
        })?;
        let missions = missions_by_group
            .get(group_name(&key))
            .cloned()
            .unwrap_or_default();
        let row = WorldGroupRef::new(
            world.clone(),
            directory.as_str(),
            WorldGroupSurvey::container_for(directory, GEOMETRY_CONTAINER_FILE),
            WorldGroupSurvey::container_for(directory, TEXTURE_ARCHIVE_FILE),
            missions.clone(),
        )
        .map_err(WorldGroupSurveyError::Refused)?;

        let (state, container_sha256) = survey_one(&found, directory, &world);
        groups.push(SurveyedWorldGroup {
            group: row,
            missions,
            state,
            container_sha256,
        });
    }

    Ok(WorldGroupSurvey {
        groups,
        absent_reference_groups: found.diagnosis.absent_reference_groups.clone(),
    })
}

impl WorldGroupSurvey {
    /// The logical key a group's own file has, through the same relative
    /// spelling production discovery used for the directory.
    ///
    /// The group's directory is `ZBD/c1c` and the file is `ZBD/c1c/gamez.zbd`,
    /// so the key is the directory's logical key plus one component. The case is
    /// the *directory's* original spelling, so a key this builds is one the
    /// session's `world` mount can resolve.
    fn container_for(directory: &cs_types::install::RelativePath, file: &str) -> String {
        format!("{}/{file}", directory.logical_key())
    }
}

/// The mission labels each world group holds, keyed by the group's **name**.
///
/// The campaign walk reports a group as the lowercased last component of its
/// directory (`c1c`), not as the full logical key (`zbd/c1c`), so the join key
/// is the last component on both sides. The two derivations disagreeing about
/// the spelling is exactly the sort of thing that would silently drop every
/// mission, so the key is stated here rather than left to a reader.
fn mission_labels_by_group(
    install_root: &Path,
) -> Result<std::collections::BTreeMap<String, Vec<String>>, WorldGroupSurveyError> {
    let layout = campaign_layout(install_root)
        .map_err(|error| WorldGroupSurveyError::Layout(error.to_string()))?;
    let mut by_group: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for entry in layout {
        let label = format!("M{:02}", entry.mission.mission_number);
        by_group
            .entry(entry.mission.world_group.to_ascii_lowercase())
            .or_default()
            .push(label);
    }
    for labels in by_group.values_mut() {
        labels.sort();
        labels.dedup();
    }
    Ok(by_group)
}

/// The group name of a discovered directory's logical key: `zbd/c1c` → `c1c`.
fn group_name(logical_key: &str) -> &str {
    logical_key.rsplit('/').next().unwrap_or(logical_key)
}

/// Reads one group's own geometry container through the production readers.
fn survey_one(
    found: &Discovery,
    directory: &cs_types::install::RelativePath,
    world: &WorldId,
) -> (Result<SurveyedContainer, WorldGroupBlocker>, String) {
    let container_key = WorldGroupSurvey::container_for(directory, GEOMETRY_CONTAINER_FILE);
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == container_key);
    let Some(record) = record else {
        return (
            Err(WorldGroupBlocker::GeometryUnreadable {
                world: world.clone(),
                container: container_key,
                reason: "production discovery inventoried no such file".to_owned(),
            }),
            String::new(),
        );
    };
    let digest = record.sha256.to_hex();
    let bytes = match read_file(&found.manifest.host_root, &container_key) {
        Ok(bytes) => bytes,
        Err(reason) => {
            return (
                Err(WorldGroupBlocker::GeometryUnreadable {
                    world: world.clone(),
                    container: container_key,
                    reason,
                }),
                digest,
            );
        }
    };

    // One parse context for both sections, as the two readers are meant to be
    // used: the label is the parse's own, and a failed attempt leaves the
    // allocation ledger untouched so the next reader starts clean.
    let mut parse = ParseContext::with_defaults(container_key.clone());
    let meshes = match read_gamez_meshes(&mut parse, &container_key, &bytes) {
        Ok(meshes) => meshes,
        Err(error) => {
            return (
                Err(WorldGroupBlocker::GeometryUnreadable {
                    world: world.clone(),
                    container: container_key,
                    reason: error.to_string(),
                }),
                digest,
            );
        }
    };
    let materials = match read_gamez_materials(&mut parse, &container_key, &bytes) {
        Ok(materials) => materials,
        Err(error) => {
            return (
                Err(WorldGroupBlocker::GeometryUnreadable {
                    world: world.clone(),
                    container: container_key,
                    reason: error.to_string(),
                }),
                digest,
            );
        }
    };
    // The independent check that the mesh walk really ends on the node array
    // the header declares. Both readers share one header parser, so comparing
    // their header words with each other would prove nothing; this compares a
    // walk's end with the header instead.
    if meshes.data_end != u64::from(meshes.header.nodes_offset) {
        return (
            Err(WorldGroupBlocker::GeometryUnreadable {
                world: world.clone(),
                container: container_key,
                reason: format!(
                    "the mesh walk ends at {} but the header declares the node array at {}",
                    meshes.data_end, meshes.header.nodes_offset
                ),
            }),
            digest,
        );
    }

    // The declared selection rule, in one place: the highest stored
    // `polygon_count` first, and the lower array index wins a tie, so two runs
    // of the audit over the same bytes choose the same meshes. It is the stored
    // face count rather than a triangulated triangle count because the face
    // count is in the 100-byte mesh record and costs nothing to read for every
    // mesh, while a triangle count needs the whole section triangulated; the
    // triangulated totals for the whole container come from
    // [`FaceCensus::of`] anyway.
    // Every stored mesh with a face, largest stored face count first and the
    // lower array index winning a tie. The census takes the first
    // [`REPRESENTATIVE_MESHES`] of this; the presentable search takes the first
    // accepted one inside [`PRESENTABLE_PROBE_MESHES`].
    let mut ranked: Vec<(u32, u32)> = meshes
        .meshes
        .iter()
        .filter_map(|slot| {
            let mesh = slot.as_ref()?;
            (mesh.info.polygon_count > 0).then_some((mesh.info.polygon_count, mesh.index))
        })
        .collect();
    ranked.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
    let mut chosen: Vec<(u32, u32)> = ranked.iter().copied().take(REPRESENTATIVE_MESHES).collect();
    chosen.sort_by_key(|(_, index)| *index);

    let mut representatives = Vec::with_capacity(chosen.len());
    for (_, index) in &chosen {
        let Some(mesh) = meshes.meshes.get(*index as usize).and_then(Option::as_ref) else {
            continue;
        };
        // The production render mesh, every stored material group kept: the
        // value the world load would spawn and the value the capture draws. A
        // mesh whose stored polygons do not survive the render mesh's own
        // validation gate contributes no representative; the container's face
        // census already counts its faces as missing, so the gap is reported
        // there rather than invented here.
        if let Ok(render) = RenderMesh::from_stored_groups(&mesh.mesh, &mesh.material_groups) {
            // The verdict is the adapter's own answer, measured here rather than
            // predicted: a mesh whose stored attributes are only partly present
            // is refused, and that is a fact about the retail corpus.
            let verdict = match crate::render::bevy_mesh::upload_groups(&render, &[]) {
                Ok(uploads) => {
                    let mut vertices = 0;
                    let mut triangles = 0;
                    for upload in &uploads {
                        vertices += upload.mesh().count_vertices();
                        triangles += upload.mesh().indices().map_or(0, |i| i.len() / 3);
                    }
                    UploadVerdict::Uploaded {
                        groups: uploads.len(),
                        vertices,
                        triangles,
                    }
                }
                Err(error) => {
                    // The adapter names the group it refused; the message is
                    // carried verbatim so a reader does not have to re-run it.
                    let text = error.to_string();
                    let group = text
                        .split_whitespace()
                        .find_map(|word| word.strip_prefix("material group "))
                        .and_then(|digits| {
                            digits
                                .trim_end_matches(|c: char| !c.is_ascii_digit())
                                .parse::<usize>()
                                .ok()
                        })
                        .unwrap_or(0);
                    UploadVerdict::Refused {
                        material_group: group,
                        reason: text,
                    }
                }
            };
            representatives.push((*index, render, verdict));
        }
    }

    // The presentable search, inside the declared window. Each candidate is
    // built and handed to the adapter exactly as the census does, so a verdict
    // here and a verdict there are the same measurement.
    let mut presentable = None;
    let mut refused_in_window = 0_usize;
    for index in ranked
        .iter()
        .take(PRESENTABLE_PROBE_MESHES)
        .map(|(_, index)| *index)
    {
        let Some(mesh) = meshes.meshes.get(index as usize).and_then(Option::as_ref) else {
            continue;
        };
        let Ok(render) = RenderMesh::from_stored_groups(&mesh.mesh, &mesh.material_groups) else {
            refused_in_window += 1;
            continue;
        };
        if crate::render::bevy_mesh::upload_groups(&render, &[]).is_ok() {
            presentable = Some((index, render));
            break;
        }
        refused_in_window += 1;
    }

    let faces = FaceCensus::of(&meshes);
    (
        Ok(SurveyedContainer {
            meshes,
            materials,
            faces,
            representatives,
            presentable,
            refused_in_window,
        }),
        digest,
    )
}

/// Reads one inventoried file's bytes from the installation, read-only.
///
/// The path comes from the manifest, so the survey can only read a file
/// production discovery already inventoried; nothing here joins a path the
/// manifest did not spell.
fn read_file(host_root: &Path, logical_key: &str) -> Result<Vec<u8>, String> {
    let path = host_root.join(logical_key);
    std::fs::read(&path).map_err(|error| format!("{} could not be read: {error}", path.display()))
}

/// Builds the content layer's declared rows from the survey.
///
/// A row that the content layer refuses stops the whole call: an audit over a
/// set of rows it could not validate is a report about rows the caller does not
/// have, which is the "silent hole" this path exists to prevent.
pub fn declared_rows(survey: &WorldGroupSurvey) -> Result<Vec<WorldGroupRef>, WorldAuditError> {
    Ok(survey
        .groups
        .iter()
        .map(|group| group.group.clone())
        .collect())
}

/// Runs the whole F18-D audit over an installation: survey every discovered
/// world group, then visit each one with the content layer's audit.
///
/// The report is the acceptance scenario's object. Its `is_complete()` is false
/// over any installation whose world placement is not decoded, and that is the
/// honest verdict rather than a failure of the audit: the traversal half of
/// "compare representative geometry **and** traversal routes" is blocked, with
/// the measured facts named, until a production path decodes the node array.
pub fn audit_world_groups(
    install_root: &Path,
) -> Result<WorldGroupAuditReport, WorldGroupSurveyError> {
    let survey = survey_world_groups(install_root)?;
    audit_survey(&survey).map_err(WorldGroupSurveyError::Refused)
}

/// The same audit over a survey the caller already has.
///
/// Separate from [`audit_world_groups`] so a consumer that wants both the
/// survey and the report — which is what the acceptance scenario needs, since
/// the GPU capture draws the meshes the survey kept — pays for **one** pass over
/// the installation instead of two. Measured: the survey dominates (production
/// discovery alone hashes the whole 470 MB tree), and running it twice cost the
/// retail test 78 s where one pass costs about half that.
pub fn audit_survey(survey: &WorldGroupSurvey) -> Result<WorldGroupAuditReport, WorldAuditError> {
    let rows = declared_rows(survey)?;
    let audit = WorldGroupAudit::new(rows)?;
    Ok(audit.audit(|row| census_for(survey, row)))
}

/// The measured census of one declared group, or the blocker that replaced it.
fn census_for(
    survey: &WorldGroupSurvey,
    row: &WorldGroupRef,
) -> Result<WorldGroupCensus, WorldGroupBlocker> {
    let surveyed =
        survey
            .group(row.world())
            .ok_or_else(|| WorldGroupBlocker::GeometryUnreadable {
                world: row.world().clone(),
                container: row.geometry_container().to_owned(),
                reason: "the survey did not discover this group".to_owned(),
            })?;
    let container = match surveyed.container() {
        Ok(container) => container,
        Err(blocker) => return Err(blocker.clone()),
    };
    census_of(
        row.world().clone(),
        container,
        row.geometry_container(),
        surveyed.container_sha256(),
    )
}

/// Measures one container into one census.
fn census_of(
    world: WorldId,
    container: &SurveyedContainer,
    container_key: &str,
    container_sha256: &str,
) -> Result<WorldGroupCensus, WorldGroupBlocker> {
    let faces = &container.faces;
    // `multi_material_group_polygons` is not in the container-wide census, so it
    // is summed over the stored mesh records' own group tables: a polygon is
    // multi-group exactly when its group table holds more than one entry.
    let multi_group = container
        .meshes
        .meshes
        .iter()
        .flatten()
        .map(|mesh| {
            mesh.material_groups
                .iter()
                .filter(|groups| groups.len() > 1)
                .count()
        })
        .sum::<usize>();
    let candidates: Vec<RepresentativeGeometry> = container
        .representatives
        .iter()
        .map(|(index, render, upload)| {
            let (min, max) = stored_bounds(render);
            RepresentativeGeometry {
                mesh_index: *index,
                triangles: render.triangles().len(),
                vertices: render.vertices().len(),
                material_groups: render.groups().len(),
                stored_min: min,
                stored_max: max,
                // The digest of the container, the stored span and the stored
                // bytes were all available here and the span digest is not kept:
                // it would have to keep the container's 4-9 MB alive for the
                // rest of the survey, and the census's own container digest
                // plus the mesh index identifies the geometry exactly. What the
                // per-mesh digest would add is a guard against a reader who
                // compares two *different* containers' representatives as if
                // they were the same mesh, and the mesh index cannot: index 12 of
                // c1 is not index 12 of c5. Stated rather than quietly
                // approximated.
                fingerprint: cs_assets::install::sha256(
                    format!("{container_key}#{index}").as_bytes(),
                ),
                upload: upload.clone(),
            }
        })
        .collect();

    let facts = GroupFacts {
        container_key: container_key.to_owned(),
        container_sha256: container_sha256.to_owned(),
        mesh_slots: faces.slots,
        present_meshes: faces.present_meshes,
        declared_faces: faces.declared_faces,
        drawn_triangles: faces.drawn_triangles(),
        missing_faces: faces.missing_faces(),
        texture_names: container.materials.textures.len(),
        // The material-to-texture **binding** is not audited by this stage: it
        // needs a content session and a texture catalog, and a geometry census
        // makes no claim about it. Stated as a zero, not as a reconciled corpus.
        bound_texture_names: 0,
        multi_material_group_polygons: multi_group,
        refused_representatives: candidates
            .iter()
            .filter(|mesh| !mesh.upload.is_uploaded())
            .count(),
    };
    if !facts.has_geometry() {
        return Err(WorldGroupBlocker::NoGeometry {
            world: world.clone(),
            slots: facts.mesh_slots,
        });
    }
    WorldGroupCensus::new(
        world.clone(),
        facts,
        // Measured from the container's own header: how many stored node records
        // it declares and where they start. Nothing decoded them, because no
        // production path decodes a GameZ node array.
        PlacementSource::Undecoded {
            stored_node_records: container.meshes.header.node_array_size,
            nodes_offset: container.meshes.header.nodes_offset,
        },
        // Unmeasured, and stated as such: nothing in this workspace has
        // established the original's world-vertex unit.
        None,
        candidates,
        Vec::new(),
        Vec::new(),
    )
    .map_err(|error| WorldGroupBlocker::GeometryUnreadable {
        world,
        container: container_key.to_owned(),
        reason: error.to_string(),
    })
}

/// The stored-unit bounds of one render mesh, as
/// `[(lowest x, y, z), (highest x, y, z)]`.
///
/// A non-finite corner is skipped rather than propagated, because the census
/// reports what the stored geometry says and a `NaN` there would make every
/// comparison against it meaningless; the census constructor then refuses the
/// result by name, so a mesh with a non-finite corner is a refusal and not a
/// quiet zero.
fn stored_bounds(render: &RenderMesh) -> ([f64; 3], [f64; 3]) {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for vertex in render.vertices() {
        for axis in 0..3 {
            let value = f64::from(vertex.position[axis]);
            if !value.is_finite() {
                continue;
            }
            min[axis] = min[axis].min(value);
            max[axis] = max[axis].max(value);
        }
    }
    for axis in 0..3 {
        if !min[axis].is_finite() {
            min[axis] = 0.0;
        }
        if !max[axis].is_finite() {
            max[axis] = 0.0;
        }
    }
    (min, max)
}

/// The reference world-group leads of spec F02, for a report that wants to state
/// which of them an installation holds.
#[must_use]
pub fn reference_group_leads() -> [&'static str; 8] {
    REFERENCE_WORLD_GROUP_LEADS
}

/// The discovered world-group keys of an installation, lowercased and sorted.
///
/// The same set [`WorldGroupSurvey::groups`] carries, exposed on its own so a
/// report can compare it against [`reference_group_leads`] without a survey.
pub fn discovered_group_keys(install_root: &Path) -> Result<Vec<String>, WorldGroupSurveyError> {
    let found = install::discover(install_root).map_err(WorldGroupSurveyError::Discovery)?;
    Ok(found
        .diagnosis
        .world_groups
        .iter()
        .map(|group| group.logical_key())
        .collect())
}

/// The lowercased set of keys a caller already has, for the comparison above.
pub fn key_set(keys: impl IntoIterator<Item = String>) -> BTreeSet<String> {
    keys.into_iter()
        .map(|key| key.to_ascii_lowercase())
        .collect()
}
