//! One finished mission outcome applied to **every** store that keeps one
//! (F43-V2, task #626).
//!
//! Two subsystems keep a best/latest record per mission and had diverged:
//! the campaign progression store
//! ([`crate::campaign::CampaignState`]'s per-node
//! [`crate::campaign::NodeProgress`], F43-A) hardcoded "higher is better"
//! with a bare `max`, while the scrapbook's
//! [`RecordBook`](crate::records::RecordBook) (F47-A) keeps best/latest under
//! a declared [`RecordRule`] whose direction and difficulty scope are
//! per subject. This module is the reconciliation; the decision it pins:
//!
//! * **Both stores, one entry point.** An outcome writes to the campaign
//!   store (which owns currency, unlocks, `current` and the run's per-node
//!   record) *and* to the profile-level scrapbook (which owns the
//!   player-facing best/latest and the achievement facts).
//!   [`apply_mission_record`] is that entry point, and the `accept_f43_v2_`
//!   tests pin that removing either side fails. A caller that feeds only
//!   one store no longer uses the production path.
//! * **One direction.** Both stores judge "best" with the same declared
//!   [`RecordRule`], through the shared
//!   [`crate::records::BetterIs::is_better`] comparison, so one subject's
//!   rule cannot make the campaign gate and the scrapbook slot mean
//!   different things by the same score.
//! * **Scope stays where it can mean something.** A [`CampaignState`] is
//!   one run under one difficulty, so its per-node record is per difficulty
//!   by construction and ignores the rule's
//!   [`DifficultyScope`](crate::records::DifficultyScope); the
//!   profile-level book is where the scope is honored (one shared slot, or
//!   one slot per difficulty across runs).
//! * **Replay repairs a half-applied outcome.** The two stores persist as
//!   separate profile revisions (their commit boundary is `cs_app`'s; see
//!   `docs/findings/2026-10-13-f43-v2-campaign-scrapbook-record-reconciliation.md`).
//!   A crash between the two commits leaves the campaign applied and the
//!   book not. Because this function submits to the book even when the
//!   campaign side answers
//!   [`AlreadyApplied`](crate::campaign::OutcomeReceipt::AlreadyApplied),
//!   replaying the same outcome finishes the missing half, and both stores'
//!   `OutcomeId` ledgers turn a replay of a complete application into a
//!   no-op.
//!
//! Both stores move together or not at all: every refusal returns before the
//! single commit point, so a caller that gets an error holds bit-identical
//! state.
//!
//! What remains **unmeasured** for the original — the record direction per
//! subject, the difficulty scoping and whether the original kept one store or
//! two — is recorded in the findings entry named above and is not guessed
//! here.

use cs_types::content::ContentId;

use crate::campaign::{
    AppliedOutcome, CampaignError, CampaignGraph, CampaignState, MissionOutcome, RuntimeNodeKind,
};
use crate::records::{RecordError, RecordReceipt, RecordRule, ScrapbookRecords};

/// One finished mission outcome, as every record-keeping store should see
/// it: the transaction input, the subject it is about and the rule its
/// records are kept under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionRecordSubmission<'a> {
    /// The finished mission's terminal report.
    pub outcome: &'a MissionOutcome,
    /// The mission the record belongs to — must be the content id the
    /// outcome's campaign node is bound to (checked here, never assumed).
    pub mission: &'a ContentId,
    /// How this subject's records are kept: direction and difficulty scope.
    pub rule: RecordRule,
}

/// What one reconciled submission changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionRecordReceipt {
    /// The campaign transaction's receipt — plan or already-applied.
    pub campaign: AppliedOutcome,
    /// The scrapbook book's receipt — applied (possibly as the repaired
    /// half of an earlier crash) or already applied.
    pub record: RecordReceipt,
}

/// Why a submission was refused; **both** stores are untouched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MissionRecordError {
    /// The campaign transaction refused the outcome; the state is unchanged
    /// and so is the book.
    Campaign(CampaignError),
    /// The book refused the run (currently only a rule change under an
    /// already-recorded subject); the book is unchanged and so is the state,
    /// because nothing commits until both stores accepted.
    Record(RecordError),
    /// The declared subject is not the mission the outcome's node is bound
    /// to — a record filed under the wrong mission would poison every
    /// unlock predicate that reads it, so it is refused rather than
    /// recorded.
    SubjectMismatch {
        /// The node the outcome names.
        node: crate::campaign::CampaignNodeKey,
        /// The subject the caller declared.
        declared: ContentId,
        /// The mission the node is actually bound to.
        bound: ContentId,
    },
}

impl std::fmt::Display for MissionRecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Campaign(error) => write!(f, "campaign transaction refused: {error}"),
            Self::Record(error) => write!(f, "record book refused: {error}"),
            Self::SubjectMismatch {
                node,
                declared,
                bound,
            } => write!(
                f,
                "node {node} is bound to {bound} but the record declares {declared}"
            ),
        }
    }
}

impl std::error::Error for MissionRecordError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Campaign(error) => Some(error),
            Self::Record(error) => Some(error),
            Self::SubjectMismatch { .. } => None,
        }
    }
}

impl From<CampaignError> for MissionRecordError {
    fn from(error: CampaignError) -> Self {
        Self::Campaign(error)
    }
}

impl From<RecordError> for MissionRecordError {
    fn from(error: RecordError) -> Self {
        Self::Record(error)
    }
}

/// Applies one finished mission outcome to the campaign state **and** the
/// scrapbook records, under the submission's declared [`RecordRule`].
///
/// Order (the shared contract's): the campaign transaction runs first on a
/// copy — its refusals (foreign, already fully applied is *not* one, unknown
/// or ineligible node, overflow) decide for both stores — then the book runs
/// on a copy, then both commit together or not at all.
///
/// The book is submitted on every accepted campaign application, **including**
/// a replay the campaign ledger already held: a campaign-ledger hit is
/// exactly the shape a crash between the two persistence commits leaves
/// behind, and the book's own `(OutcomeId, subject)` ledger turns the
/// replay of a complete application into a no-op.
///
/// # Errors
///
/// [`MissionRecordError`]; both stores are unchanged.
pub fn apply_mission_record(
    state: &mut CampaignState,
    graph: &CampaignGraph,
    scrapbook: &mut ScrapbookRecords,
    submission: MissionRecordSubmission<'_>,
) -> Result<MissionRecordReceipt, MissionRecordError> {
    let MissionRecordSubmission {
        outcome,
        mission,
        rule,
    } = submission;
    let node = graph
        .node(&outcome.node)
        .ok_or(CampaignError::UnknownNode {
            node: outcome.node.clone(),
        })?;
    let RuntimeNodeKind::Mission { mission: bound } = &node.kind else {
        return Err(CampaignError::NotAMission {
            node: outcome.node.clone(),
        }
        .into());
    };
    if bound != mission {
        return Err(MissionRecordError::SubjectMismatch {
            node: outcome.node.clone(),
            declared: mission.clone(),
            bound: bound.clone(),
        });
    }

    // Compute both halves on copies; commit only when both accepted.
    let mut next_state = state.clone();
    let campaign = next_state.apply_outcome_with_rule(graph, outcome, rule)?;
    let mut next_scrapbook = scrapbook.clone();
    let difficulty = next_state.difficulty();
    let record = next_scrapbook
        .records
        .submit(&outcome.id, mission, difficulty, outcome.score, rule)
        .map_err(MissionRecordError::Record)?;

    // Commit — the single mutation point for both stores.
    *state = next_state;
    *scrapbook = next_scrapbook;
    Ok(MissionRecordReceipt { campaign, record })
}
