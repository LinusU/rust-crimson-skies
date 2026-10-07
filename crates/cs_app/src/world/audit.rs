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
//! * *Placement* is decoded (F18-E): `read_gamez_nodes` walks the node array on the
//!   same parse context as the two section readers, and the survey reports
//!   [`PlacementSource::Decoded`] with the number of node records whose stored
//!   `mesh_index` resolves to a present mesh ([`NodeMeshBindings::of`]).
//! * *The stored vertex unit* is the measured metre
//!   ([`CoordinateSource::retail_gamez`], task #677, `observed_tool`, with the
//!   axis convention code-derived per task #436's owner note), read from the
//!   convention rather than spelled as a literal. Never `verified_original`.
//! * *One opening class is located from measured evidence.* A **stunt passage**
//!   is located wherever the group's instant-action scenario declares a
//!   fly-through danger-zone target: the scenario's `ia.zrd` binds the
//!   objective's label to a world node (F42-D / task #463), the node's own
//!   stored box is the one task #427 measured, and the box's narrowest extent
//!   times this container's measured unit is the opening's clearance. The
//!   classification comes from the objective's own `category_label`/
//!   `help_label` pair and from nowhere else — never from a node name.
//! * *The other four classes, and every traversal route, are measured to be
//!   **absent** rather than left unsaid.* The container stores no field that
//!   names a tunnel, an arch, a building opening or a hangar, and the
//!   decrypted image's only name-keyed consumer of a world record is the
//!   four-byte `fvol` prefix, so the corpus holds none of those four and a
//!   node-name match would be a guess. The container stores no path either:
//!   the corpus's only route carrier is a mission reader's `aiv.zrd`, whose
//!   encoding is unmeasured (task #455, `F31-ROUTE-ENCODING`). Each class and
//!   each group therefore reports what was searched and what the corpus holds
//!   ([`UnlocatedOpening`], [`cs_content::world::WorldAuditGap::NoRouteInMeasuredCorpus`]),
//!   never a silent zero and never the old shortfall gap.
//!
//! # What is deliberately not here
//!
//! No format reader, no collision-role classification, no boundary rule and no
//! route search. The unknowns F18-A/B/C recorded are still unknowns, and this
//! file measures what can be measured and names the rest.

use std::fmt;
use std::path::Path;

use crate::render::bevy_mesh::MeshAdapterError;
use crate::stunts::survey_retail_stunt_encoding;
use cs_assets::install::{self, Discovery};
use cs_content::campaign_bindings::campaign_layout;
use cs_content::coordinates::CoordinateSource;
use cs_content::mesh::RenderMesh;
use cs_content::stunts::RetailStuntEncodingSurvey;
use cs_content::world::UploadVerdict;
use cs_content::world::{
    GroupFacts, OpeningClass, PlacementSource, RepresentativeGeometry, RouteSearch, StuntOpening,
    UnlocatedOpening, WorldAuditError, WorldGroupAudit, WorldGroupAuditReport, WorldGroupBlocker,
    WorldGroupCensus, WorldGroupRef, WorldId,
};
use cs_formats::gamez::{
    FaceCensus, GameZMaterials, GameZMeshes, NodeMeshBindings, read_gamez_materials,
    read_gamez_meshes, read_gamez_nodes,
};
use cs_formats::io::ParseContext;
use cs_types::asset_id::SourceSpan;

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

/// The measured reason a world group gives for four of its five opening
/// classes: tunnel, arch, building opening and hangar.
///
/// **A measurement, not a default.** It is produced by [`class_absent`] over
/// three measurements this workspace already took and this task bound together:
///
/// * what the container stores per record — `read_gamez_nodes` decodes a name,
///   flags, a mesh binding, a hierarchy slot, an area partition and three
///   stored bounding boxes, and none of them states an opening
///   (`docs/findings/2026-10-02-gamez-node-array-layout.md`);
/// * what the owner-supplied decrypted image (`crimson.decrypted.exe`, sha256
///   `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`) does
///   with a world record's name: its only name-keyed consumer of one is the
///   four-byte `fvol` prefix (`strncmp` at VA `0x44e087`, inside the fog
///   routine — tasks #716 and #727, `docs/findings/2026-10-07-f18-grid-collision-origin.md`);
/// * what the corpus declares about its zones: the eight instant-action
///   scenarios' `ia.zrd`/`targets.zrd` pairs, surveyed end to end by F42-D
///   (task #463), declare fly-through danger-zone objectives and nothing that
///   states one of these four classes.
///
/// So for these four classes the corpus **holds none** in a world group, and a
/// match against an authored node name (`hangerdoors`, `gate1`, …) would be
/// exactly the guess AGENTS rule 4 forbids. Evidence class `observed_tool`;
/// never `verified_original`.
pub const OPENING_CLASS_ABSENT_FROM_THE_CORPUS: &str = "the container stores no field that states this class (`read_gamez_nodes` decodes, per \
     record, a name, flags, a mesh binding, a hierarchy slot, an area partition and three stored \
     bounding boxes, none of which is an opening), the owner-supplied decrypted image's only \
     name-keyed consumer of a world record is the four-byte `fvol` prefix (`strncmp` at VA \
     0x44e087, tasks #716/#727) and it matches no name of this class, and no instant-action \
     scenario objective declares one (F42-D's survey of all eight `ia.zrd`/`targets.zrd` pairs); \
     so the corpus holds none of this class in this group, and classifying one from an authored \
     node name alone would be a guess (AGENTS rule 4)";

/// The measured reason a world group gives for stating no traversal route.
///
/// **A measurement, not a default.** Two measurements, together exhaustive over
/// what this workspace has read:
///
/// * the world container stores no path — `read_gamez_nodes` decodes a name,
///   flags, a mesh binding, a hierarchy slot, an area partition and three
///   stored bounding boxes per record, and the world record's partition grid
///   is a broad-phase candidate index (task #727, `docs/findings/2026-10-07-f18-grid-collision-origin.md`),
///   never a path;
/// * the corpus's only route carrier is a mission reader's `aiv.zrd`,
///   measured present in every mission of every mission type by F31-D, and its
///   encoding is **unmeasured** — task #455 (`F31-ROUTE-ENCODING`,
///   `docs/findings/2026-10-01-f31-d-route-coverage-in-every-mission-type.md`).
///
/// So no authored sequence of a group's geometry can be stated without
/// guessing, and this is what replaces the shortfall gap for every group the
/// opening rule covered. Evidence class `observed_tool`; never
/// `verified_original`.
pub const ROUTE_ABSENT_FROM_THE_CORPUS: &str = "the world container stores no path (`read_gamez_nodes` decodes, per record, a name, flags, \
     a mesh binding, a hierarchy slot, an area partition and three stored bounding boxes, and the \
     world record's partition grid is a broad-phase candidate index, never a path), and the \
     corpus's only route carrier is a mission reader's `aiv.zrd`, present in every mission of \
     every mission type (F31-D) whose encoding is unmeasured (task #455, F31-ROUTE-ENCODING), \
     so no authored sequence of this group's geometry can be stated without guessing (AGENTS \
     rule 4)";

/// Why one world group locates no opening of `class`.
///
/// The measured statement quoted in [`OPENING_CLASS_ABSENT_FROM_THE_CORPUS`],
/// with the class named so a reader sees which of the five was searched. Only
/// the four classes the corpus states nothing about reach this; a stunt passage
/// is located from the scenario's own declarations instead (or reports why that
/// measurement found none here).
#[must_use]
pub fn class_absent(class: OpeningClass) -> String {
    format!(
        "searched for a {class} in this group's placed records and in the corpus's own \
         declarations: {}",
        OPENING_CLASS_ABSENT_FROM_THE_CORPUS
    )
}

/// Why one world group states no traversal route.
///
/// See [`ROUTE_ABSENT_FROM_THE_CORPUS`]; the group is named so the text can be
/// read on its own in a report row.
#[must_use]
pub fn route_absent(world: &WorldId) -> String {
    format!("{world}: {}", ROUTE_ABSENT_FROM_THE_CORPUS)
}

/// Why one world group locates no stunt passage, given what its instant-action
/// scenario declared and how much of it resolved to a measured world box.
///
/// Both halves are measurements: the declared count is F42-D's decode of the
/// group's own `targets.zrd`, and the resolved count is that survey's join to
/// task #427's boxes. A group that declared none therefore says **the corpus
/// holds none of this class here** — which is the honest answer for `c1c` and
/// `c2b`, the two of the eight groups that author no fly-through objective —
/// rather than reporting a bare unlocated class.
///
/// A group that *declared* targets none of which resolved gets the other
/// answer: a **shortfall**, not an absence. An unbound label or a node with no
/// measured box is an unknown (F42-D reports it as its own gap), and saying
/// "the corpus holds none" about it would turn that unknown into a measured
/// absence — the guess AGENTS rule 4 forbids. Over the owner's installation
/// only the first shape occurs: all 54 declared targets resolved, and `c1c`
/// and `c2b` declared none.
#[must_use]
pub fn stunt_passage_absent(container: &str, declared: usize, resolved: usize) -> String {
    let verdict = if declared == 0 {
        "so the corpus holds none of this class in this group".to_owned()
    } else {
        format!(
            "so none of this group's {declared} declared target(s) could be located from measured \
             evidence and this class stays unlocated with that shortfall stated, never as an \
             absence"
        )
    };
    format!(
        "searched for a stunt passage in this group's own declarations: the instant-action \
         scenario {container} declares {declared} fly-through danger-zone target(s) over its \
         `targets.zrd` (F42-D measured 54 across six of the eight groups, every one resolved to \
         a `dzpath<N>` box task #427 measured) and {resolved} of them resolved to a measured \
         world box here, {verdict}; the campaign's \
         `dzones.zrd` members are framed but their meaning is unmeasured (task #513)"
    )
}

/// Why a stunt passage could not be located at all: the measurement that would
/// have located one did not run.
#[must_use]
pub fn stunt_passage_unmeasured(reason: &str) -> String {
    format!(
        "the stunt-encoding measurement this class is located from could not be taken, so \
         nothing searched for a stunt passage in this group: {reason}"
    )
}

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
    /// How many stored node records place a mesh: the nodes whose stored
    /// `mesh_index` resolves to a present mesh slot
    /// ([`NodeMeshBindings::of`]), as `read_gamez_nodes` decoded them.
    pub placed_objects: usize,
    /// Metres per stored vertex unit, read from the measured GameZ coordinate
    /// convention ([`CoordinateSource::retail_gamez`]) for this container's span.
    pub vertex_scale_to_m: f64,
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
    /// The instant-action stunt encoding F42-D measured, which is where a
    /// **stunt passage** is located from, or the reason it could not be taken.
    ///
    /// One survey for all eight groups rather than one per group: the encoding
    /// lives in the scenarios' `ia.zrd`/`targets.zrd` members and joins to the
    /// world nodes those scenarios name, so the measurement is naturally
    /// corpus-wide. A failure is kept as its message instead of aborting the
    /// world survey — every unlocated class then carries the message as its
    /// measured reason ([`stunt_passage_unmeasured`]), so the shortfall is
    /// visible in every affected row rather than lost.
    stunts: Result<RetailStuntEncodingSurvey, String>,
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

    /// The instant-action stunt encoding this survey measured — the evidence a
    /// **stunt passage** is located from — or the reason it could not be taken.
    pub fn stunts(&self) -> Result<&RetailStuntEncodingSurvey, &str> {
        self.stunts.as_ref().map_err(String::as_str)
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
    // The stunt encoding, measured once for the whole corpus: this is what
    // locates a **stunt passage**, so it is taken before the per-group loop and
    // kept — including its failure, which every affected row will quote.
    let stunts = survey_retail_stunt_encoding(install_root).map_err(|error| error.to_string());

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
        stunts,
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
    // The bytes come from the manifest row's **original spelling**, not from
    // the lowercased logical key. The installation stores its world groups as
    // `ZBD/C1C/gamez.zbd`, and `logical_key()` folds that to
    // `zbd/c1c/gamez.zbd`; joining the folded key onto the host root happens
    // to work on a case-insensitive filesystem and fails on a case-sensitive
    // one, which is exactly the kind of difference the manifest preserved the
    // original spelling to survive.
    let bytes = match read_file(&found.manifest.host_root, record.relative_spelling.as_str()) {
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
    // The node array, through the production reader, on the same parse context.
    let nodes = match read_gamez_nodes(&mut parse, &bytes) {
        Ok(nodes) => nodes,
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
    // `resolved`, not the raw `mesh_index >= 0` count: a node whose stored index
    // names an absent or out-of-range slot places no mesh, so the placed-object
    // count is the cross-checked one — the node array measured against the mesh
    // array it indexes into, rather than the reference's assertion trusted. On
    // the retail corpus the two are equal (every stored index resolves; see
    // `docs/findings/2026-10-03-f10-c-05-node-mesh-index-cross-check.md`).
    let placed_objects =
        usize::try_from(NodeMeshBindings::of(&nodes, &meshes).resolved).unwrap_or(usize::MAX);
    // The unit is the measured one (task #677, `observed_tool`; task #436's
    // owner note adds the code-derived axis convention): the scale is read off
    // the measured convention rather than spelled here as a literal.
    let vertex_scale_to_m = match SourceSpan::new(
        install::fingerprint(&found.manifest),
        record.relative_spelling.as_str(),
        None,
        0,
        u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        Some(record.sha256),
    ) {
        Ok(span) => CoordinateSource::retail_gamez(span)
            .convention()
            .meters_per_unit(),
        Err(error) => {
            return (
                Err(WorldGroupBlocker::GeometryUnreadable {
                    world: world.clone(),
                    container: container_key,
                    reason: format!("the container's source span is not recordable: {error}"),
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
            let verdict = upload_verdict(&render);
            representatives.push((*index, render, verdict));
        }
    }

    // The presentable search, inside the declared window. Each candidate is
    // built and handed to the adapter exactly as the census does — through
    // [`upload_verdict`], the same function — so a verdict here and a verdict
    // there are the same measurement rather than two implementations that
    // happen to agree.
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
        if upload_verdict(&render).is_uploaded() {
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
            placed_objects,
            vertex_scale_to_m,
            faces,
            representatives,
            presentable,
            refused_in_window,
        }),
        digest,
    )
}

/// What the **production** upload adapter does with one render mesh: the
/// census's own measurement, and the single place it is taken.
///
/// Both the census and the presentable search go through this function, so a
/// verdict in a census and a verdict in the capture path cannot be two
/// implementations that happen to agree today.
///
/// The refused material group is read from the adapter's **typed** error
/// ([`MeshAdapterError`]), never by scraping its `Display`. That is not
/// stylistic: `split_whitespace` on `"material group 1 carries a normal on 24
/// of 492 vertices; …"` yields `"material"`, `"group"` and `"1"` as three
/// separate words, so matching the two-word prefix `"material group "` never
/// fires and the group silently fell back to `0`. Every refusal the retail
/// corpus produces on a group other than the first was therefore attributed to
/// material group 0, while the message carried verbatim beside it said
/// otherwise — a census that looked measured and was not. Both adapter
/// refusals carry the group in their payload, so the message is only ever
/// stored for a reader, never parsed.
#[must_use]
pub fn upload_verdict(render: &RenderMesh) -> UploadVerdict {
    match crate::render::bevy_mesh::upload_groups(render, &[]) {
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
            let material_group = match &error {
                MeshAdapterError::GroupOutOfRange { group, .. }
                | MeshAdapterError::IncompleteAttribute { group, .. } => *group,
            };
            UploadVerdict::Refused {
                material_group,
                // Carried verbatim so a reader sees the adapter's own counts
                // without re-running it.
                reason: error.to_string(),
            }
        }
    }
}

/// Reads one inventoried file's bytes from the installation, read-only.
///
/// `relative_spelling` comes from the manifest row and keeps the host's own
/// case, so the survey can only read a file production discovery already
/// inventoried and reads it under the name the host actually uses; nothing here
/// joins a path the manifest did not spell.
fn read_file(host_root: &Path, relative_spelling: &str) -> Result<Vec<u8>, String> {
    let path = host_root.join(relative_spelling);
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
/// The report is the acceptance scenario's object. Its `is_complete()` stays
/// false over the retail installation, and now for a *measured* reason: the
/// placement is decoded, the unit is the measured metre, a **stunt passage** is
/// located in six of the eight groups from F42-D's own declarations, and the
/// other four opening classes plus every traversal route are measured to be
/// **absent** from the corpus rather than left unsaid. The traversal half of
/// "compare representative geometry **and** traversal routes" therefore reports
/// one [`cs_content::world::WorldAuditGap::NoRouteInMeasuredCorpus`] per group
/// — naming what was searched, why the corpus holds no route and which missions
/// the absence affects — and every unlocated class carries the measurement that
/// says so (task #732).
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
        survey.stunts(),
        &scenario_key_for(row.geometry_container()),
    )
}

/// The instant-action scenario archive a world group's stunt encoding lives in,
/// derived from the group's own measured geometry-container key.
///
/// The container key (`zbd/c1c/gamez.zbd`) already carries the directory
/// production discovery spelled, so this only appends the two components F42-D
/// measured (`ia1/zrdr.zbd`) instead of re-spelling the group's path a second
/// time and risking the two disagreeing about case.
fn scenario_key_for(geometry_container: &str) -> String {
    let prefix = geometry_container
        .strip_suffix(GEOMETRY_CONTAINER_FILE)
        .unwrap_or(geometry_container);
    format!("{prefix}{INSTANT_ACTION_DIR}/{SCENARIO_ARCHIVE}")
}

/// The two components of the instant-action scenario archive, as F42-D measured
/// them (`ZBD/<group>/ia1/zrdr.zbd`). Stated here because this module may not
/// edit `cs_app::stunts`, where the same two strings live; the retail test pins
/// the key this produces against the installation.
const INSTANT_ACTION_DIR: &str = "ia1";
/// See [`INSTANT_ACTION_DIR`].
const SCENARIO_ARCHIVE: &str = "zrdr.zbd";

/// Measures one container into one census.
#[allow(clippy::too_many_arguments)]
fn census_of(
    world: WorldId,
    container: &SurveyedContainer,
    container_key: &str,
    container_sha256: &str,
    stunts: Result<&RetailStuntEncodingSurvey, &str>,
    scenario_key: &str,
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
    };
    if !facts.has_geometry() {
        return Err(WorldGroupBlocker::NoGeometry {
            world: world.clone(),
            slots: facts.mesh_slots,
        });
    }
    // The openings the measured rule located, the measured reason each class it
    // did not locate carries, and the verdict about routes. The route side is
    // always [`RouteSearch::Unstated`]: this function has just read the whole
    // container and the container states no path, which is the measurement.
    let (openings, unlocated, route_search) =
        traversal_evidence(&world, container, stunts, scenario_key);
    WorldGroupCensus::new(
        world.clone(),
        facts,
        // Decoded by `read_gamez_nodes`: the stored node records that name a mesh.
        PlacementSource::Decoded {
            placed_objects: container.placed_objects,
        },
        // The measured GameZ unit (one stored unit per metre), code-derived and
        // observed by tool; never `verified_original`.
        Some(container.vertex_scale_to_m),
        candidates,
        // **No route is stated.** See `ROUTE_ABSENT_FROM_THE_CORPUS`: the
        // container stores no path, and the corpus's only route carrier is a
        // mission reader's `aiv.zrd` whose encoding is unmeasured (task #455).
        // Saying so through [`RouteSearch::Unstated`] is what replaces the old
        // shortfall gap with a measurement that names affected content.
        Vec::new(),
        openings,
        unlocated,
        route_search,
    )
    .map_err(|error| WorldGroupBlocker::GeometryUnreadable {
        world,
        container: container_key.to_owned(),
        reason: error.to_string(),
    })
}

/// The stunt passages one group's measured evidence locates.
///
/// This is the **classification** half of the F18 opening rule, and it is a
/// public production function because it is the one thing a caller must be able
/// to check against its own evidence: it takes F42-D's measured survey (task
/// #463), the group it is about, and the group's own measured
/// `vertex_scale_to_m`, and returns one [`StuntOpening`] per fly-through
/// danger-zone target of that group that resolved to a measured world box.
///
/// # What the classification rests on
///
/// A chain of four measurements, none of them a node name:
///
/// * the scenario's `targets.zrd` declares a fly-through danger-zone
///   objective, selected by its own `category_label`/`help_label` pair
///   (`MSG_OBJ_DZ`/`MSG_OBJ_FLYTHROUGH`);
/// * the scenario's `ia.zrd` `dzones` binds that objective's label to a world
///   node — the label direction F42-D measured;
/// * the node's own stored box and mesh binding are the ones task #427 read
///   out of the container;
/// * the box's **narrowest stored extent** times `vertex_scale_to_m` is the
///   opening's clearance in canonical metres, so the clearance and the factor
///   that produced it come from the same two measurements the census carries.
///
/// A target whose label bound no world node, or whose node carries no measured
/// box, contributes nothing here — it stays a reported gap in F42-D's own
/// survey rather than an opening this audit invented.
///
/// # Errors
///
/// None: a survey that could not be taken at all never reaches this function
/// (its message is carried as the measured reason instead), and a group it has
/// no rows for simply locates nothing.
#[must_use]
pub fn locate_stunt_passages(
    world: &WorldId,
    stunts: &RetailStuntEncodingSurvey,
    vertex_scale_to_m: f64,
) -> Vec<StuntOpening> {
    stunts
        .gates()
        .iter()
        .filter(|gate| gate.world() == world)
        .filter_map(|gate| {
            let geometry = gate.geometry()?;
            let mesh_index = u32::try_from(geometry.mesh_index()?).ok()?;
            let clearance = geometry.volume().thinnest_extent() * vertex_scale_to_m;
            clearance.is_finite().then_some(StuntOpening {
                class: OpeningClass::StuntPassage,
                mesh_index,
                clearance_m: Some(clearance),
            })
        })
        .collect()
}

/// The measured traversal evidence for one group: the openings the rule
/// located, the measured reason for every class it did not, and the verdict
/// about routes.
///
/// # What locates an opening here
///
/// One class only, and by [`locate_stunt_passages`] — never by a node name.
/// The other four classes are reported absent with [`class_absent`], and no
/// route is stated: see [`route_absent`].
fn traversal_evidence(
    world: &WorldId,
    container: &SurveyedContainer,
    stunts: Result<&RetailStuntEncodingSurvey, &str>,
    scenario_key: &str,
) -> (Vec<StuntOpening>, Vec<UnlocatedOpening>, RouteSearch) {
    let mut openings = Vec::new();
    let mut declared = 0_usize;
    let mut resolved = 0_usize;
    match stunts {
        Ok(survey) => {
            declared = survey
                .gates()
                .iter()
                .filter(|gate| gate.world() == world)
                .count();
            // The clearance is the zone box's own narrowest stored extent
            // converted by **this container's** measured unit — the same
            // `vertex_scale_to_m` the census carries, so the number a reader
            // sees and the factor that produced it cannot disagree.
            openings = locate_stunt_passages(world, survey, container.vertex_scale_to_m);
            resolved = openings.len();
        }
        Err(reason) => {
            // The measurement itself did not run. Every row below says so by
            // name rather than reporting the class as quietly absent.
            debug_assert!(
                !reason.trim().is_empty(),
                "a failed stunt survey always carries its error"
            );
        }
    }

    let mut unlocated = Vec::with_capacity(OpeningClass::ALL.len());
    for class in OpeningClass::ALL {
        if openings.iter().any(|opening| opening.class == class) {
            continue;
        }
        let measured = match (class, stunts) {
            (OpeningClass::StuntPassage, Ok(_)) => {
                stunt_passage_absent(scenario_key, declared, resolved)
            }
            (OpeningClass::StuntPassage, Err(reason)) => stunt_passage_unmeasured(reason),
            (_, _) => class_absent(class),
        };
        unlocated.push(
            UnlocatedOpening::new(class, measured)
                .expect("every measured reason here is non-empty"),
        );
    }

    (
        openings,
        unlocated,
        RouteSearch::Unstated {
            measured: route_absent(world),
        },
    )
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
