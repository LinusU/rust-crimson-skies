//! The immutable outcome input (F43-A).
//!
//! A [`MissionOutcome`] is the single transaction input a finished mission
//! session hands the campaign: *what* happened ([`Outcome`]), *where* in the
//! graph it happened (`node`), *how well* (`score`, the recordable metric)
//! and *who vouches for it* (`authority`). It is immutable — apply or
//! refuse, never edit.

use std::fmt;

use cs_script::ir::Outcome;

use super::identity::{CampaignNodeKey, OutcomeId};

/// Whether the evidence behind an outcome was an authorized playthrough.
///
/// Spec F43 non-negotiable behavior 1: only completed *authorized* outcomes
/// change progression — a debug, synthetic or modded evidence run either
/// stays on its own profile or lands here as [`Self::Modified`], in which
/// case the transaction still records it but marks the progression it
/// touched ([`CampaignState::modified`]). There is no variant that changes
/// progression silently.
///
/// [`CampaignState::modified`]: super::state::CampaignState::modified
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutcomeAuthority {
    /// An ordinary playthrough of the declared mission.
    Authorized,
    /// A debug/synthetic/modded run. Applies its records and marks the
    /// progression it touched — the mark is permanent for the run.
    Modified {
        /// Why this outcome is not clean evidence (tool, flag or harness).
        reason: String,
    },
}

/// One finished mission's terminal report — the transaction input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionOutcome {
    /// Its stable identity — `(profile, run, session, terminal event)`.
    pub id: OutcomeId,
    /// The campaign node the mission occupied.
    pub node: CampaignNodeKey,
    /// What happened.
    pub outcome: Outcome,
    /// The score the run recorded (the recordable metric — `best` compares
    /// it, `latest` stores it).
    pub score: u64,
    /// Who vouches for the evidence.
    pub authority: OutcomeAuthority,
}

impl fmt::Display for MissionOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "outcome {:?} on {} (run {}, session {})",
            self.outcome, self.node, self.id.run, self.id.session.0
        )
    }
}
