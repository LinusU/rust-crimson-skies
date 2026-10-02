//! The stunt boundary (F42-A): lower the declared `cs_content::stunts`
//! record into the runtime `cs_sim::stunts` rule, refusing every mandatory
//! field that is still an explicit unknown.
//!
//! Spec: `specs/F42-stunts-fame-photos-and-optional-achievement-events.md`,
//! stage `### F42-A`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! The lowering rule is the boundary contract used across the crate (see
//! [`crate::campaign`]): a [`Resolved::Unknown`] on a mandatory field — an
//! unrecovered gate volume, an unmeasured direction or clearance threshold,
//! an unmeasured payout or an unlinked scrapbook photo — refuses here, where
//! a session can still decline the record, instead of inventing a gate or
//! paying a guess.
//!
//! [`Resolved::Unknown`]: cs_types::content::Resolved::Unknown

use std::fmt;

use cs_content::stunts::StuntDefinition;
use cs_sim::stunts::{
    Gate as RuntimeGate, GateEvidence as RuntimeGateEvidence, StuntReward, StuntRule,
    StuntRuleDraft, TraversalRule,
};
use cs_types::content::{ContentId, Resolved};
use cs_types::evidence::ClaimId;

/// Lowers one declared stunt into a runtime rule.
///
/// # Errors
///
/// [`StuntLowerError`] naming the first explicit unknown on the gate, the
/// direction rule, the clearance rule or the reward, and
/// [`StuntLowerError::Rejected`] when the lowered record fails the runtime's
/// own validation.
pub fn lower_stunt(declared: &StuntDefinition) -> Result<StuntRule, StuntLowerError> {
    let gate = match declared.gate() {
        Resolved::Known(known) => RuntimeGate::new(
            known.value.center_m,
            known.value.normal,
            known.value.right_half_extent_m,
            known.value.up_half_extent_m,
            known.value.half_depth_m,
        )
        .map_err(|error| StuntLowerError::Rejected {
            stunt: declared.id().as_str().to_owned(),
            reason: error.to_string(),
        })?,
        Resolved::Unknown { claim_id, reason } => {
            return Err(StuntLowerError::UnknownGate {
                claim_id: claim_id.clone(),
                reason: reason.clone(),
            });
        }
    };
    let min_forward_cosine = resolve(&declared.rules().min_forward_cosine, |claim_id, reason| {
        StuntLowerError::UnknownDirectionRule { claim_id, reason }
    })?;
    let min_clearance_m = resolve(&declared.rules().min_clearance_m, |claim_id, reason| {
        StuntLowerError::UnknownClearanceRule { claim_id, reason }
    })?;
    let rule = TraversalRule::new(gate, min_forward_cosine, min_clearance_m).map_err(|error| {
        StuntLowerError::Rejected {
            stunt: declared.id().as_str().to_owned(),
            reason: error.to_string(),
        }
    })?;

    let fame = match &declared.reward().fame {
        Resolved::Known(known) => known.value,
        Resolved::Unknown { claim_id, reason } => {
            return Err(StuntLowerError::UnknownFame {
                claim_id: claim_id.clone(),
                reason: reason.clone(),
            });
        }
    };
    let cash_minor = match &declared.reward().cash_minor {
        Resolved::Known(known) => known.value,
        Resolved::Unknown { claim_id, reason } => {
            return Err(StuntLowerError::UnknownCash {
                claim_id: claim_id.clone(),
                reason: reason.clone(),
            });
        }
    };
    let media = match &declared.reward().media {
        Resolved::Known(known) => known.value.clone(),
        Resolved::Unknown { claim_id, reason } => {
            return Err(StuntLowerError::UnknownMedia {
                claim_id: claim_id.clone(),
                reason: reason.clone(),
            });
        }
    };

    StuntRule::try_new(StuntRuleDraft {
        id: declared.id().clone(),
        world: declared.world().clone(),
        rule,
        missions: declared.scope().missions().to_vec(),
        criticality: lower_criticality(declared),
        repeat: lower_repeat(declared),
        reward: StuntReward {
            fame,
            cash_minor,
            media,
        },
        evidence: lower_evidence(declared),
    })
    .map_err(|error| StuntLowerError::Rejected {
        stunt: declared.id().as_str().to_owned(),
        reason: error.to_string(),
    })
}

/// Lowers the declared stunts a mission is eligible for, in authored order.
///
/// The filter is the mission scope, not the world: two missions that share
/// one world can declare different eligible sets (spec F42 behavior 3), and
/// a stunt the mission does not declare is simply absent here — it can never
/// be awarded in that mission.
///
/// # Errors
///
/// [`StuntLowerError`] from [`lower_stunt`], and
/// [`StuntLowerError::DuplicateStuntInSet`] when two lowered rules share one
/// stunt id.
pub fn lower_mission_stunts<'a>(
    mission: &ContentId,
    declared: impl IntoIterator<Item = &'a StuntDefinition>,
) -> Result<Vec<StuntRule>, StuntLowerError> {
    let mut lowered = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for record in declared {
        if !record.scope().contains(mission) {
            continue;
        }
        if seen.contains(&record.id().as_str()) {
            return Err(StuntLowerError::DuplicateStuntInSet {
                stunt: record.id().as_str().to_owned(),
            });
        }
        seen.push(record.id().as_str());
        lowered.push(lower_stunt(record)?);
    }
    Ok(lowered)
}

/// Reads a resolved threshold, refusing an explicit unknown by name.
fn resolve(
    value: &Resolved<f64>,
    wrap: fn(ClaimId, String) -> StuntLowerError,
) -> Result<f64, StuntLowerError> {
    match value {
        Resolved::Known(known) => Ok(known.value),
        Resolved::Unknown { claim_id, reason } => Err(wrap(claim_id.clone(), reason.clone())),
    }
}

fn lower_criticality(declared: &StuntDefinition) -> cs_sim::stunts::StuntCriticality {
    use cs_sim::stunts::StuntCriticality;
    match declared.criticality() {
        cs_content::stunts::StuntCriticality::Optional => StuntCriticality::Optional,
        cs_content::stunts::StuntCriticality::CriticalPath => StuntCriticality::CriticalPath,
    }
}

/// The declared repeat policy is a two-value vocabulary with the same
/// spelling in both halves, so it is carried across by value rather than
/// re-derived.
fn lower_repeat(declared: &StuntDefinition) -> cs_sim::stunts::StuntRepeat {
    use cs_content::stunts::StuntRepeat as Declared;
    use cs_sim::stunts::StuntRepeat as Runtime;
    match declared.repeat() {
        Declared::Once => Runtime::Once,
        Declared::Repeatable => Runtime::Repeatable,
    }
}

/// The declared geometry marking is carried across to the runtime record so a
/// completion can report whether it was earned on a measured volume or on a
/// drawn one.
fn lower_evidence(declared: &StuntDefinition) -> RuntimeGateEvidence {
    use cs_content::stunts::GateEvidence as Declared;
    use cs_sim::stunts::GateEvidence as Runtime;
    match declared.gate_evidence() {
        Declared::Measured => Runtime::Measured,
        Declared::Reconstructed => Runtime::Reconstructed,
        Declared::NotRecovered => Runtime::NotRecovered,
    }
}

/// Why a declared stunt record refused to lower.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StuntLowerError {
    /// The gate volume was never recovered, so there is nothing to test a
    /// traversal against (spec F42 behavior 5: a drawn replacement volume is
    /// marked reconstructed, never invented).
    UnknownGate {
        /// The claim the unknown belongs to.
        claim_id: ClaimId,
        /// Why it is unknown.
        reason: String,
    },
    /// The direction threshold was not measured.
    UnknownDirectionRule {
        /// The claim the unknown belongs to.
        claim_id: ClaimId,
        /// Why it is unknown.
        reason: String,
    },
    /// The clearance threshold was not measured.
    UnknownClearanceRule {
        /// The claim the unknown belongs to.
        claim_id: ClaimId,
        /// Why it is unknown.
        reason: String,
    },
    /// The fame payout was not measured.
    UnknownFame {
        /// The claim the unknown belongs to.
        claim_id: ClaimId,
        /// Why it is unknown.
        reason: String,
    },
    /// The cash payout was not measured.
    UnknownCash {
        /// The claim the unknown belongs to.
        claim_id: ClaimId,
        /// Why it is unknown.
        reason: String,
    },
    /// The scrapbook media reference was not resolved, so a one-time photo
    /// has no identity to deduplicate on.
    UnknownMedia {
        /// The claim the unknown belongs to.
        claim_id: ClaimId,
        /// Why it is unknown.
        reason: String,
    },
    /// The lowered record failed the runtime's own validation. The declared
    /// and runtime rules match, so this names a lowering defect.
    Rejected {
        /// The stunt that was being lowered.
        stunt: String,
        /// The named underlying reason.
        reason: String,
    },
    /// Two declared records share one stunt id in one mission's set.
    DuplicateStuntInSet {
        /// The duplicated stunt key.
        stunt: String,
    },
}

impl fmt::Display for StuntLowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownGate { claim_id, reason } => {
                write!(f, "stunt gate volume is unknown ({claim_id}): {reason}")
            }
            Self::UnknownDirectionRule { claim_id, reason } => write!(
                f,
                "stunt min_forward_cosine is unknown ({claim_id}): {reason}"
            ),
            Self::UnknownClearanceRule { claim_id, reason } => {
                write!(f, "stunt min_clearance_m is unknown ({claim_id}): {reason}")
            }
            Self::UnknownFame { claim_id, reason } => {
                write!(f, "stunt fame reward is unknown ({claim_id}): {reason}")
            }
            Self::UnknownCash { claim_id, reason } => {
                write!(f, "stunt cash reward is unknown ({claim_id}): {reason}")
            }
            Self::UnknownMedia { claim_id, reason } => {
                write!(f, "stunt scrapbook media is unknown ({claim_id}): {reason}")
            }
            Self::Rejected { stunt, reason } => {
                write!(f, "stunt {stunt:?} failed runtime validation: {reason}")
            }
            Self::DuplicateStuntInSet { stunt } => write!(
                f,
                "stunt {stunt:?} is declared more than once in this mission's set"
            ),
        }
    }
}

impl std::error::Error for StuntLowerError {}

/// Re-exported so a caller can attribute a lowering refusal to its declared
/// record without naming two crates' error types.
pub use cs_sim::stunts::StuntError as StuntRuntimeError;

// ------------------------------------------- the retail encoding survey ----
//
// F42-D (task #463) has to audit the original's mission-scoped stunts. The
// content half (`cs_content::stunts`) decodes one `.zrd` scenario member and
// extracts its encoding; this half walks the installation, finds each instant
// action scenario, and joins each fly-through target to the world detection
// zone task #427 measured. The join is where the two measurements meet: the
// scenario says *which* zone, and #427's node survey says *where* it is.
//
// The survey never invents a rule. A direction rule, a clearance rule, a
// payout and a repeat policy are not in the scenario bytes, so the record
// reports them as unmeasured (`RetailStuntEncodingSurvey::direction_rule_is_
// measured()` and friends) rather than filling them in.

use std::fs;
use std::path::Path;

use cs_assets::install::{self, DiscoveryError};
use cs_content::stunts::{
    RetailStuntEncodingSurvey, RetailStuntGate, SCENARIO_MEMBER, SCENARIO_TARGETS_MEMBER,
    StuntEncodingSpan, decode_zrd, scenario_fly_through_targets, scenario_mission_type,
    scenario_zone_bindings,
};
use cs_content::world::{RetailTriggerVolume, WorldId};
use cs_formats::script_raw::{ContainerDiscovery, LocatedProgram, discover_container};
use cs_types::install::RelativePath;

use crate::world::triggers::{TriggerVolumeSurveyError, survey_retail_trigger_volumes};

/// The world-group subdirectory an instant-action scenario lives in.
const INSTANT_ACTION_DIR: &str = "ia1";

/// The reader archive an instant-action scenario lives in.
const SCENARIO_ARCHIVE: &str = "zrdr.zbd";

/// Why a retail stunt-encoding survey could not be produced.
#[derive(Debug)]
pub enum StuntEncodingSurveyError {
    /// The installation could not be discovered.
    Discovery(DiscoveryError),
    /// The installation declares no world group, so no scenario can be placed.
    NoWorldGroups,
    /// A scenario container could not be read from disk or is missing from the
    /// inventory.
    Read {
        /// The container's logical key.
        container: String,
        /// Why the read failed.
        reason: String,
    },
    /// The world groups' own geometry could not be measured, so a target's box
    /// cannot be resolved.
    Geometry(TriggerVolumeSurveyError),
    /// A scenario container does not carry one of the two members the encoding
    /// lives in. Reported by name rather than skipped: a scenario that says
    /// nothing would otherwise look like a scenario with no targets.
    MissingMember {
        /// The container's logical key.
        container: String,
        /// The member that was not found.
        member: &'static str,
    },
    /// A scenario member did not decode as `.zrd`.
    Decode {
        /// The container's logical key.
        container: String,
        /// The member's name.
        member: String,
        /// The decoder's refusal code.
        code: &'static str,
        /// Offset of the refusal inside the member.
        offset: u64,
    },
    /// A world group's name is not a usable world id.
    WorldId {
        /// The container's logical key.
        container: String,
        /// The id grammar's own reason.
        reason: String,
    },
}

impl fmt::Display for StuntEncodingSurveyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(error) => write!(f, "the installation is undiscoverable: {error}"),
            Self::NoWorldGroups => write!(f, "the installation declares no world group"),
            Self::Read { container, reason } => {
                write!(f, "container {container} could not be read: {reason}")
            }
            Self::Geometry(error) => write!(f, "the world geometry could not be measured: {error}"),
            Self::MissingMember { container, member } => {
                write!(f, "container {container} carries no {member}")
            }
            Self::Decode {
                container,
                member,
                code,
                offset,
            } => write!(
                f,
                "container {container} member {member} is not decodable at {offset} ({code})"
            ),
            Self::WorldId { container, reason } => {
                write!(f, "container {container} is not in a world group: {reason}")
            }
        }
    }
}

impl std::error::Error for StuntEncodingSurveyError {}

/// Measures every instant-action scenario's fly-through stunt encoding, joined
/// to the world geometry task #427 measured.
///
/// One production discovery, one production reader-archive discovery per world
/// group, the `.zrd` decoder, and #427's own production node survey for the
/// geometry. Every row carries its container key, that container's SHA-256, the
/// member's byte span and the installation fingerprint, so each number can be
/// traced back to the bytes.
///
/// # Errors
///
/// [`StuntEncodingSurveyError`] in every case, naming the container and member
/// it could not read or decode. The survey does **not** turn a failed read into
/// a shorter list: a scenario that cannot be measured is a refusal, so a
/// consumer never mistakes "measured fewer targets" for "the installation has
/// fewer".
pub fn survey_retail_stunt_encoding(
    install_root: &Path,
) -> Result<RetailStuntEncodingSurvey, StuntEncodingSurveyError> {
    let found = install::discover(install_root).map_err(StuntEncodingSurveyError::Discovery)?;
    let install_sha256 = install::fingerprint(&found.manifest).to_hex();
    let groups = found.diagnosis.world_groups.clone();
    if groups.is_empty() {
        return Err(StuntEncodingSurveyError::NoWorldGroups);
    }

    // The world geometry is #427's measurement, used as-is. Its refusal is
    // this survey's refusal: a target whose world box cannot be measured cannot
    // be reported as measured.
    let geometry =
        survey_retail_trigger_volumes(install_root).map_err(StuntEncodingSurveyError::Geometry)?;

    let mut gates = Vec::new();
    for group in &groups {
        let container_key = format!(
            "{}/{INSTANT_ACTION_DIR}/{SCENARIO_ARCHIVE}",
            group.logical_key()
        );
        let world_name = group
            .logical_key()
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_owned();
        let world =
            WorldId::from_key(&world_name).map_err(|error| StuntEncodingSurveyError::WorldId {
                container: container_key.clone(),
                reason: error.to_string(),
            })?;

        let Some(record) = found
            .manifest
            .files
            .iter()
            .find(|record| record.relative_spelling.logical_key() == container_key)
        else {
            return Err(StuntEncodingSurveyError::Read {
                container: container_key,
                reason: "production discovery inventoried no such file".to_owned(),
            });
        };
        let container_sha256 = record.sha256.to_hex();
        let bytes = fs::read(
            found
                .manifest
                .host_root
                .join(record.relative_spelling.as_str()),
        )
        .map_err(|error| StuntEncodingSurveyError::Read {
            container: container_key.clone(),
            reason: error.to_string(),
        })?;
        let path = RelativePath::new(record.relative_spelling.as_str()).map_err(|error| {
            StuntEncodingSurveyError::Read {
                container: container_key.clone(),
                reason: error.to_string(),
            }
        })?;

        let discovery = discover_container(&container_key, &path, &bytes);
        let scenario = scenario_member(&discovery, SCENARIO_MEMBER, &container_key)?;
        let targets = scenario_member(&discovery, SCENARIO_TARGETS_MEMBER, &container_key)?;

        let scenario_root =
            decode_zrd(scenario.bytes()).map_err(|error| StuntEncodingSurveyError::Decode {
                container: container_key.clone(),
                member: SCENARIO_MEMBER.to_owned(),
                code: error.code(),
                offset: error.offset(),
            })?;
        let targets_root =
            decode_zrd(targets.bytes()).map_err(|error| StuntEncodingSurveyError::Decode {
                container: container_key.clone(),
                member: SCENARIO_TARGETS_MEMBER.to_owned(),
                code: error.code(),
                offset: error.offset(),
            })?;

        let mission_type = scenario_mission_type(&scenario_root).unwrap_or_default();
        let bindings = scenario_zone_bindings(&scenario_root);
        let targets_span = span_of(&container_key, &container_sha256, targets);

        for target in scenario_fly_through_targets(&targets_root) {
            let world_zone = bindings
                .iter()
                .find(|(_, label)| *label == target.zone_label)
                .map(|(node, _)| (*node).to_owned());
            let resolved: Option<RetailTriggerVolume> = world_zone.as_deref().and_then(|zone| {
                geometry
                    .volumes()
                    .iter()
                    .find(|volume| volume.world() == &world && volume.zone() == zone)
                    .cloned()
            });
            gates.push(RetailStuntGate::new(
                world.clone(),
                mission_type,
                &target.zone_label,
                world_zone,
                &target,
                targets_span.clone(),
                resolved,
            ));
        }
    }

    Ok(RetailStuntEncodingSurvey::new(install_sha256, gates))
}

/// The located member with `name`, or a named refusal.
fn scenario_member<'d, 'a>(
    discovery: &'d ContainerDiscovery<'a>,
    name: &'static str,
    container: &str,
) -> Result<&'d LocatedProgram<'a>, StuntEncodingSurveyError> {
    discovery
        .programs()
        .iter()
        .find(|program| program.locator().member() == Some(name))
        .ok_or_else(|| StuntEncodingSurveyError::MissingMember {
            container: container.to_owned(),
            member: name,
        })
}

/// One located member's provenance span.
fn span_of(
    container: &str,
    container_sha256: &str,
    program: &LocatedProgram<'_>,
) -> StuntEncodingSpan {
    let locator = program.locator();
    StuntEncodingSpan::new(
        container,
        container_sha256,
        locator.member().unwrap_or_default(),
        locator.span().offset,
        locator.span().len,
    )
}
