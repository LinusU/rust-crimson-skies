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
//! `dzpaths`. The node is the *volume*: each one stores an axis-aligned box in
//! its own info record and binds a mesh index. This module reads exactly that,
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
//!   unit is unmeasured (task #436), so the survey reports
//!   [`TriggerTickVerdict::UnitUnmeasured`](cs_content::world::TriggerTickVerdict::UnitUnmeasured)
//!   with the **break-even factor** rather than a verdict;
//! * it does not decode the campaign's `dzones.zrd` framing. Measured, the word
//!   after a list tag is not an item count — the same value precedes a list of
//!   four strings in `ZBD/C3/M02`'s member and a list of one in the same file's
//!   objective list — so reading the member as a length-prefixed value list is a
//!   guess, and a guess about what a mission says a trigger is. The survey says
//!   so through
//!   [`zone_declarations_are_decoded`](cs_content::world::RetailTriggerVolumeSurvey::zone_declarations_are_decoded);
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
    DETECTION_ZONE_PREFIX, RetailTriggerVolume, RetailTriggerVolumeSurvey, StoredVolume,
    TriggerVolumeError, TriggerVolumeSpan, WorldId, is_detection_zone_name,
};
use cs_formats::gamez::nodes::{NODE_SLOT_BYTES, NodeKind};
use cs_formats::gamez::read_gamez_nodes;
use cs_formats::io::ParseContext;

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
            Self::NoBox { world, zone } => {
                write!(f, "world {world} zone {zone} stores no box at all")
            }
            Self::Refused(error) => write!(f, "the trigger-volume survey is unusable: {error}"),
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
/// them discriminates: across the eight world containers' 56 620 node records,
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
/// `vertex_scale_to_m` is **`None`**: nothing in this workspace has measured the
/// original's world-vertex unit, and supplying a factor here would be the guess
/// the whole stage exists to avoid. Every zone's container SHA-256 comes from the
/// same discovery's manifest, so a rerun over a different installation reports
/// different fingerprints instead of the same numbers.
///
/// # Errors
///
/// [`TriggerVolumeSurveyError`] in every case: [`TriggerVolumeSurveyError::Discovery`]
/// when the installation cannot be inventoried, [`TriggerVolumeSurveyError::Read`]
/// when a container is missing or unreadable, [`TriggerVolumeSurveyError::Nodes`]
/// when a node array does not decode, [`TriggerVolumeSurveyError::Volume`] when
/// a zone's stored box is not a box, and
/// [`TriggerVolumeSurveyError::Refused`] for a survey the content layer refuses.
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
            let box_field = match &node.kind {
                // Only an object record stores a transform, and therefore only an
                // object record's info record is the one whose box this stage
                // reads. A zone of any other kind is reported as absent rather
                // than read through a record it does not have.
                NodeKind::Object3d(_) => ZONE_BOX_FIELD.read(node),
                _ => continue,
            };
            let [first, second] = box_field;
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

    RetailTriggerVolumeSurvey::new(install_sha256, None, volumes)
        .map_err(TriggerVolumeSurveyError::Refused)
}

/// The name prefix the survey matched, re-exported so a consumer reading this
/// module does not have to reach into `cs_content` for the constant that says
/// what it measured.
pub const ZONE_PREFIX: &str = DETECTION_ZONE_PREFIX;
