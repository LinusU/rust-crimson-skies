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
