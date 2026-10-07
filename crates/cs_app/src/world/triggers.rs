//! The retail trigger-volume survey: one detection-zone node's stored extent,
//! read out of a world container's own node array (task #427).
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! the F18-D evidence stage and the F18-C overlay layer this task measures.
//! Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! # Why this exists
//!
//! Task #401 fixed the *hold* a swept body paid at a trigger volume's face, and
//! left the *report* boundary behind: a volume thinner than one tick of the
//! reaching body's travel is never sampled, so the overlay behind it does not
//! fire. Task #427 was filed to answer the half of that nobody had answered —
//! **how thick is one of the original's triggers**, measured rather than
//! assumed. F18-C's own depot volume is 1 m and F18-A's arch volume is 8 m; both
//! are **fixture choices**, and the T401 record says so. This module is what
//! replaces them with a measurement.
//!
//! # What it reads, and what it does not claim
//!
//! The original's campaign missions each carry a reader-archive member called
//! [`DETECTION_ZONE_MEMBER`](cs_content::world::DETECTION_ZONE_MEMBER) which
//! names their detection zones, and the world containers carry the zones
//! themselves as nodes called `dzpath1`, `dzpath2`, … under a parent called
//! `dzpaths`. The node is the *volume*: each one is an `object3d` record that
//! stores **no transform** — so the box it carries needs no composition — and
//! stores an axis-aligned box in its own info record, bound to a mesh index.
//! This module reads exactly that,
//! through the **production** F11-A node reader
//! ([`cs_formats::gamez::read_gamez_nodes`]) and the **production** F02-B
//! discovery pass, and hands
//! [`RetailTriggerVolumeSurvey`](cs_content::world::RetailTriggerVolumeSurvey)
//! one measured row per numbered zone, carrying the container key, the
//! container's SHA-256, the installation fingerprint and the node's own byte
//! span.
//!
//! What it does **not** do, deliberately:
//!
//! * it does not convert a stored extent to metres. The original's world-vertex
//!   unit is measured now (the metre; tasks #677 and #436), but this survey
//!   supplies no factor — the contract #427 pinned is that a stored extent is
//!   never silently read as a length, so the survey reports
//!   [`TriggerTickVerdict::UnitUnmeasured`](cs_content::world::TriggerTickVerdict::UnitUnmeasured)
//!   with the **break-even factor** rather than a verdict. Whether the survey
//!   *should* consume the measured unit is task #733;
//! * it decodes the campaign's `dzones.zrd` **framing** (task #513: a list's
//!   word is its child count plus one) but not its **meaning**: the survey
//!   carries what each mission states under `disable`, `nosnapshot` and
//!   `objective_numbers`, cross-checked against the container's zone nodes
//!   ([`declaration_gaps`](cs_content::world::RetailTriggerVolumeSurvey::declaration_gaps)),
//!   and none of the three keys' meaning is claimed;
//! * it does not claim the original detected these zones by sensor overlap at
//!   all. Which native consumes them, and how, is F13/F39's question.
//!
//! **Not `verified_original`.** `retail` here is read access to files; no
//! original run happened and nothing about the original's runtime behaviour is
//! asserted.

use std::fmt;
use std::fs;
use std::path::Path;

use cs_assets::install::{self, DiscoveryError};
use cs_content::world::{
    DETECTION_ZONE_MEMBER, DETECTION_ZONE_PREFIX, MissionZoneDeclaration, RetailTriggerVolume,
    RetailTriggerVolumeSurvey, StoredVolume, TriggerVolumeError, TriggerVolumeSpan, WorldId,
    ZoneDeclarationKey, is_detection_zone_name,
};
use cs_formats::gamez::nodes::{NODE_SLOT_BYTES, NodeKind};
use cs_formats::gamez::read_gamez_nodes;
use cs_formats::io::ParseContext;
use cs_formats::script_raw::discover_container;
use cs_formats::zbd::detection_zones::{DetectionZoneKey, read_detection_zones};

use super::audit::GEOMETRY_CONTAINER_FILE;

/// Why a trigger-volume survey could not be produced at all.
#[derive(Debug)]
pub enum TriggerVolumeSurveyError {
    /// The installation could not be discovered.
    Discovery(DiscoveryError),
    /// A world group's geometry container could not be read from disk.
    Read {
        /// The container's logical key.
        container: String,
        /// Why the read failed.
        reason: String,
    },
    /// A world group's node array could not be decoded.
    Nodes {
        /// The container's logical key.
        container: String,
        /// The node reader's own reason.
        reason: String,
    },
    /// The installation declares no world group, so there is no world container
    /// a trigger volume could live in.
    NoWorldGroups,
    /// A zone's stored box was not a box this workspace can carry.
    Volume {
        /// The world the zone is in.
        world: String,
        /// The zone's name.
        zone: String,
        /// The refusal itself.
        reason: TriggerVolumeError,
    },
    /// A numbered zone is a node whose `node_type` tag is not `object3d`, so its
    /// info record is not the one this stage's box field belongs to. Reported
    /// rather than skipped: a zone this survey cannot read is a gap, and a
    /// measurement that silently omits one is exactly what the rest of this
    /// module refuses to be.
    UnexpectedKind {
        /// The world the zone is in.
        world: String,
        /// The zone's name.
        zone: String,
        /// The node type tag the record stores, as the production reader labels
        /// it.
        kind: &'static str,
    },
    /// A numbered zone stores a **transform** (`Object3dCsC.flags` is not
    /// `OBJECT3D_FLAGS_IDENTITY`), so its box is in the node's own space and the
    /// axes the survey reports the extents on are the node's, not the world's.
    ///
    /// Every measured zone is an identity record, so this refusal is what makes
    /// "the box needs no composition" true **by construction** rather than by
    /// observation: a rotated zone is refused by name instead of being reported
    /// with an extent on the wrong axis.
    TransformedZone {
        /// The world the zone is in.
        world: String,
        /// The zone's name.
        zone: String,
        /// The stored `Object3dCsC.flags` word.
        flags: u32,
    },
    /// A numbered zone stored no box at all: every axis of the measured field is
    /// zero. Distinct from [`Self::Volume`] because it is not a malformed box —
    /// it is an **absent** one, which is a different thing to report to a
    /// consumer and a different thing for a later stage to resolve.
    NoBox {
        /// The world the zone is in.
        world: String,
        /// The zone's name.
        zone: String,
    },
    /// The survey the bytes produced was refused.
    Refused(TriggerVolumeError),
    /// A mission's `dzones.zrd` member could not be decoded.
    Declarations {
        /// The reader archive's logical key.
        container: String,
        /// The decoder's own refusal.
        reason: String,
    },
    /// A mission declares zones but sits in no world group the installation
    /// declares, so its names cannot be cross-checked.
    MissionWithoutWorld {
        /// The reader archive's logical key.
        container: String,
    },
}

impl fmt::Display for TriggerVolumeSurveyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(error) => write!(f, "the installation is undiscoverable: {error}"),
            Self::Read { container, reason } => {
                write!(f, "container {container} could not be read: {reason}")
            }
            Self::Nodes { container, reason } => {
                write!(
                    f,
                    "container {container} has no decodable node array: {reason}"
                )
            }
            Self::NoWorldGroups => write!(f, "the installation declares no world group"),
            Self::Volume {
                world,
                zone,
                reason,
            } => {
                write!(
                    f,
                    "world {world} zone {zone} stored an unusable box: {reason}"
                )
            }
            Self::UnexpectedKind { world, zone, kind } => {
                write!(
                    f,
                    "world {world} zone {zone} is a {kind} node, not the object record this \\
                     survey reads a detection zone's box from"
                )
            }
            Self::TransformedZone { world, zone, flags } => {
                write!(
                    f,
                    "world {world} zone {zone} stores a transform (object flags {flags}), so \\
                     its box is in the node's own space rather than the world's"
                )
            }
            Self::NoBox { world, zone } => {
                write!(f, "world {world} zone {zone} stores no box at all")
            }
            Self::Refused(error) => write!(f, "the trigger-volume survey is unusable: {error}"),
            Self::Declarations { container, reason } => {
                write!(
                    f,
                    "{container}: {DETECTION_ZONE_MEMBER} is undecodable: {reason}"
                )
            }
            Self::MissionWithoutWorld { container } => write!(
                f,
                "{container} declares detection zones but sits in no declared world group"
            ),
        }
    }
}

impl std::error::Error for TriggerVolumeSurveyError {}

/// The node info field the original's detection zones carry their box in.
///
/// This is **not** a guess and not a layout this stage derived. F11-A's reader
/// documents three candidate boxes in a node's info record —
/// [`unk116`](cs_formats::gamez::nodes::RawNodeInfo::unk116),
/// [`unk140`](cs_formats::gamez::nodes::RawNodeInfo::unk140) and
/// [`unk164`](cs_formats::gamez::nodes::RawNodeInfo::unk164) — and calls all
/// three unmeasured. Measured over the owner's installation, exactly one of
/// them discriminates: across the eight world containers' 53 303 node records,
/// `unk140` is non-zero in **30 161** and `unk164` in 15 289 and `unk116` in
/// 1 330, while across the **80** numbered `dzpath<N>` zones `unk140` is
/// non-zero in **all 80** and the other two are zero in **all 80**. So the zones
/// are exactly the records `unk140` speaks for, and the stage reads that field
/// through the production reader rather than re-slicing the container itself.
///
/// The field's *meaning* stays unmeasured: nothing here establishes that it is
/// a bounding box, that its first triple is the minimum, or what the original
/// does with it. What is measured is the numbers and where they came from.
pub const ZONE_BOX_FIELD: ZoneBoxField = ZoneBoxField::Unk140;

/// Which of the three unmeasured boxes in a node's info record this stage reads.
///
/// Three arms rather than a bare field, so the choice is a value a test can pin
/// and a reviewer can read: the measurement above names `Unk140` and a test
/// asserts it, so a reader that changed the choice would have to change that
/// assertion too rather than silently re-point the measurement.
///
/// The two unused arms are the point of the type. F11-A documents all three as
/// unmeasured, so the choice between them is a **hypothesis** this stage is
/// recording, and a hypothesis with only one live arm could not be re-pointed
/// without editing the reader. Keeping all three means a later measurement that
/// says the zones carry `unk164` instead is a one-line change plus its own
/// corpus assertion — which is what `accept_t427_the_measured_field_and_corner_order_are_named`
/// is for. Clippy's dead-code lint is satisfied by the exhaustive `read` and
/// `zone_box_field` matches rather than by an `#[allow]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneBoxField {
    /// `RawNodeInfo::unk116`: zero in every measured zone.
    Unk116,
    /// `RawNodeInfo::unk140`: the field every measured zone carries.
    Unk140,
    /// `RawNodeInfo::unk164`: zero in every measured zone.
    Unk164,
}

impl ZoneBoxField {
    /// The field as the production reader stores it.
    const fn read(self, node: &cs_formats::gamez::nodes::RawNode) -> [[f32; 3]; 2] {
        match self {
            Self::Unk116 => node.info.unk116,
            Self::Unk140 => node.info.unk140,
            Self::Unk164 => node.info.unk164,
        }
    }
}

/// Whether a node's stored box is `[min, max]`.
///
/// Measured, not assumed: over all 80 numbered zones in the owner's
/// installation the first triple is at or below the second on every axis, and
/// [`StoredVolume::new`] checks the same rule, so a record that ever violated it
/// would be refused by name rather than silently swapped.
const STORED_BOX_FIRST_IS_MIN: bool = true;

/// The node info field this module's measurement reads.
///
/// Public because it is part of what this module measured rather than an
/// implementation detail: the acceptance tests pin it against the corpus, so a
/// reader can check which field the numbers came from without reading the walk.
#[must_use]
pub const fn zone_box_field() -> &'static str {
    match ZONE_BOX_FIELD {
        ZoneBoxField::Unk116 => "unk116",
        ZoneBoxField::Unk140 => "unk140",
        ZoneBoxField::Unk164 => "unk164",
    }
}

/// Widens one stored `f32` corner to `f64`, which is exact.
fn widen(corner: [f32; 3]) -> [f64; 3] {
    corner.map(f64::from)
}

/// Measures every numbered detection zone in every world group the
/// installation at `install_root` declares.
///
/// One production discovery, and per group one read of that group's own geometry
/// container through the production node reader. The survey's
/// `vertex_scale_to_m` is **`None`**: this entry point supplies no factor even
/// though the unit is measured (tasks #677 and #436), because the verdict this
/// stage exists to state is the refusal-with-a-number — the break-even factor —
/// not a converted length (task #427; consuming the measured unit is task #733).
/// Every zone's container SHA-256 comes from the
/// same discovery's manifest, so a rerun over a different installation reports
/// different fingerprints instead of the same numbers.
///
/// # Errors
///
/// [`TriggerVolumeSurveyError`] in every case: [`TriggerVolumeSurveyError::Discovery`]
/// when the installation cannot be inventoried, [`TriggerVolumeSurveyError::Read`]
/// when a container is missing or unreadable, [`TriggerVolumeSurveyError::Nodes`]
/// when a node array does not decode, [`TriggerVolumeSurveyError::UnexpectedKind`]
/// and [`TriggerVolumeSurveyError::TransformedZone`] when a numbered zone is not
/// the identity object record this stage reads, [`TriggerVolumeSurveyError::Volume`]
/// when a zone's stored box is not a box, [`TriggerVolumeSurveyError::NoBox`] when
/// a zone stores no box at all, and
/// [`TriggerVolumeSurveyError::Refused`] for a survey the content layer refuses.
///
/// Every per-zone refusal **aborts** the survey rather than dropping the zone.
/// That is deliberate and it is the module's one shape: a partial measurement
/// presented as a measurement is the failure this stage exists to prevent, so a
/// consumer sees "could not measure" instead of a shorter list.
pub fn survey_retail_trigger_volumes(
    install_root: &Path,
) -> Result<RetailTriggerVolumeSurvey, TriggerVolumeSurveyError> {
    let found = install::discover(install_root).map_err(TriggerVolumeSurveyError::Discovery)?;
    let install_sha256 = install::fingerprint(&found.manifest).to_hex();
    let groups = found.diagnosis.world_groups.clone();
    if groups.is_empty() {
        return Err(TriggerVolumeSurveyError::NoWorldGroups);
    }

    let mut volumes: Vec<RetailTriggerVolume> = Vec::new();
    for directory in &groups {
        let container_key = format!("{}/{GEOMETRY_CONTAINER_FILE}", directory.logical_key());
        let Some(record) = found
            .manifest
            .files
            .iter()
            .find(|record| record.relative_spelling.logical_key() == container_key)
        else {
            return Err(TriggerVolumeSurveyError::Read {
                container: container_key,
                reason: "production discovery inventoried no such file".to_owned(),
            });
        };
        let container_sha256 = record.sha256.to_hex();
        // The manifest's **original spelling**, for the same reason F18-D's
        // survey uses it: the logical key is lowercased and a case-sensitive
        // filesystem would refuse the join.
        let bytes = fs::read(
            found
                .manifest
                .host_root
                .join(record.relative_spelling.as_str()),
        )
        .map_err(|error| TriggerVolumeSurveyError::Read {
            container: container_key.clone(),
            reason: error.to_string(),
        })?;

        let mut parse = ParseContext::with_defaults(container_key.clone());
        let nodes = read_gamez_nodes(&mut parse, &bytes).map_err(|error| {
            TriggerVolumeSurveyError::Nodes {
                container: container_key.clone(),
                reason: error.to_string(),
            }
        })?;

        // The world's own identity, through the same id grammar the rest of the
        // content layer uses. A group whose name the grammar refuses is a
        // refusal here rather than a zone under a guessed name.
        let name = container_key
            .rsplit('/')
            .nth(1)
            .unwrap_or(&container_key)
            .to_owned();
        let world = WorldId::from_key(&name).map_err(|error| TriggerVolumeSurveyError::Nodes {
            container: container_key.clone(),
            reason: format!("the world group's name is not a world id: {error}"),
        })?;

        for node in &nodes.nodes {
            if !is_detection_zone_name(&node.name) {
                continue;
            }
            let NodeKind::Object3d(object) = &node.kind else {
                // Only an object record's info record is the one whose box this
                // stage reads. A numbered zone of any other kind is **refused by
                // name**, not skipped: the corpus holds none, so a silent `continue`
                // would hide the one that arrived and report a shorter list as if
                // it were the measurement.
                return Err(TriggerVolumeSurveyError::UnexpectedKind {
                    world: world.key().to_owned(),
                    zone: node.name.clone(),
                    kind: node.kind.label(),
                });
            };
            if !object.stores_identity() {
                // The box is read in the node's own axes, and the whole one-tick
                // comparison turns on which axis is thinnest. A node that stores a
                // transform would need that box composed into world space first,
                // and nothing here measures what `unk140` is relative to — so a
                // transformed zone is refused rather than reported on the wrong
                // axis. Measured over the owner's installation every one of the 80
                // numbered zones is an identity record (`Object3dCsC.flags` =
                // `OBJECT3D_FLAGS_IDENTITY`), so this refusal is the invariant
                // being enforced, not a case the retail corpus trips.
                return Err(TriggerVolumeSurveyError::TransformedZone {
                    world: world.key().to_owned(),
                    zone: node.name.clone(),
                    flags: object.flags,
                });
            }
            let [first, second] = ZONE_BOX_FIELD.read(node);
            // The stored record is `f32`; the content layer carries `f64` so a
            // later unit factor multiplies in `f64` and cannot lose the low bits
            // of a stored corner. The widening is exact.
            let (min, max) = if STORED_BOX_FIRST_IS_MIN {
                (widen(first), widen(second))
            } else {
                (widen(second), widen(first))
            };
            let stored =
                StoredVolume::new(min, max).map_err(|reason| TriggerVolumeSurveyError::Volume {
                    world: world.key().to_owned(),
                    zone: node.name.clone(),
                    reason,
                })?;
            if stored.is_empty() {
                // A record that stores all zeros on all three axes is a node
                // that carries no box: the parent `dzpaths` node measures exactly
                // that way, and a numbered zone that did would be a fact worth
                // reporting rather than dropping. It is not dropped: it is
                // refused here by name, because a measurement this survey
                // silently omits is a gap a consumer cannot see. A box that is
                // merely **flat** on one axis is not this case and is kept.
                return Err(TriggerVolumeSurveyError::NoBox {
                    world: world.key().to_owned(),
                    zone: node.name.clone(),
                });
            }
            volumes.push(RetailTriggerVolume::new(
                TriggerVolumeSpan::new(
                    world.clone(),
                    container_key.clone(),
                    container_sha256.clone(),
                    node.index,
                    nodes.info_offset + u64::from(node.index) * NODE_SLOT_BYTES,
                    NODE_SLOT_BYTES,
                ),
                node.name.clone(),
                (node.info.mesh_index >= 0).then_some(node.info.mesh_index),
                stored,
            ));
        }
    }

    let declarations = read_mission_declarations(&found, &groups)?;
    RetailTriggerVolumeSurvey::new(install_sha256, None, volumes)
        .and_then(|survey| survey.with_declarations(declarations))
        .map_err(TriggerVolumeSurveyError::Refused)
}

/// The reader archive every campaign mission keeps its members in.
const MISSION_READER_FILE: &str = "zrdr.zbd";

/// Decodes every mission's `dzones.zrd` through the production discovery and the
/// production [`read_detection_zones`], one declaration per member found.
fn read_mission_declarations(
    found: &install::Discovery,
    groups: &[cs_types::install::RelativePath],
) -> Result<Vec<MissionZoneDeclaration>, TriggerVolumeSurveyError> {
    let mut declarations = Vec::new();
    for record in &found.manifest.files {
        let key = record.relative_spelling.logical_key();
        if key.rsplit('/').next() != Some(MISSION_READER_FILE) {
            continue;
        }
        let bytes = fs::read(
            found
                .manifest
                .host_root
                .join(record.relative_spelling.as_str()),
        )
        .map_err(|error| TriggerVolumeSurveyError::Read {
            container: key.clone(),
            reason: error.to_string(),
        })?;
        let discovery = discover_container(&key, &record.relative_spelling, &bytes);
        for program in discovery.programs() {
            if program.locator().member() != Some(DETECTION_ZONE_MEMBER) {
                continue;
            }
            let decoded = read_detection_zones(program.bytes()).map_err(|error| {
                TriggerVolumeSurveyError::Declarations {
                    container: key.clone(),
                    reason: error.to_string(),
                }
            })?;
            let mission = program.mission().map(str::to_owned).ok_or_else(|| {
                TriggerVolumeSurveyError::MissionWithoutWorld {
                    container: key.clone(),
                }
            })?;
            let world_key = mission.rsplit_once('/').map(|(world, _)| world);
            let world = groups
                .iter()
                .find(|group| Some(group.logical_key().as_str()) == world_key)
                .and_then(|group| {
                    let name = group.logical_key().rsplit('/').next()?.to_owned();
                    WorldId::from_key(&name).ok()
                })
                .ok_or_else(|| TriggerVolumeSurveyError::MissionWithoutWorld {
                    container: key.clone(),
                })?;
            let span = program.locator().span();
            declarations.push(MissionZoneDeclaration::new(
                mission,
                world,
                key.clone(),
                record.sha256.to_hex(),
                (span.offset, span.len),
                decoded
                    .keys()
                    .iter()
                    .map(|key| match key {
                        DetectionZoneKey::Disable => ZoneDeclarationKey::Disable,
                        DetectionZoneKey::NoSnapshot => ZoneDeclarationKey::NoSnapshot,
                        DetectionZoneKey::ObjectiveNumbers => ZoneDeclarationKey::ObjectiveNumbers,
                    })
                    .collect(),
                decoded.disable().unwrap_or_default().to_vec(),
                decoded.no_snapshot().unwrap_or_default().to_vec(),
                decoded.objective_numbers().unwrap_or_default().to_vec(),
            ));
        }
    }
    Ok(declarations)
}

/// The name prefix the survey matched, re-exported so a consumer reading this
/// module does not have to reach into `cs_content` for the constant that says
/// what it measured.
pub const ZONE_PREFIX: &str = DETECTION_ZONE_PREFIX;
